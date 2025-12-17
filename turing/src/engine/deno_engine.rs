use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use deno_core::{JsRuntime, OpState, RuntimeOptions};
use deno_core::op2;
use deno_error::JsErrorBox;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::engine::types::ScriptFnMetadata;
use crate::interop::params::Params;
use crate::{EngineDataState, ExternalFunctions};

pub struct DenoEngine<Ext>
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    runtime: JsRuntime,
    module_name: Option<String>,
    deno_fns: FxHashMap<String, ScriptFnMetadata>,
    data: Arc<RwLock<EngineDataState>>,
    _ext: PhantomData<Ext>,
}

// Single dispatch op which receives a JSON array like `["fn.name", [arg0, arg1, ...]]`
#[op2]
#[serde]
fn turing_dispatch<Ext: ExternalFunctions>(
    state: &mut OpState,
    #[serde] payload: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    use crate::OpaquePointerKey;
    use crate::interop::params::{DataType, Param, Params};
    use slotmap::KeyData;

    // payload expected to be [name, args]
    let (name, args) = match payload {
        serde_json::Value::Array(mut a) if !a.is_empty() => {
            let name = a
                .remove(0)
                .as_str()
                .ok_or_else(|| JsErrorBox::generic("invalid call name"))?
                .to_string();
            let args = if !a.is_empty() {
                a.remove(0)
            } else {
                serde_json::Value::Array(vec![])
            };
            (name, args)
        }
        _ => return Err(JsErrorBox::generic("invalid payload")),
    };

    let map = state.borrow::<FxHashMap<String, ScriptFnMetadata>>();
    let data = state.borrow::<Arc<RwLock<EngineDataState>>>();

    let metadata = map
        .get(&name)
        .ok_or_else(|| JsErrorBox::generic("function not found"))?;
    let p_types = metadata.param_types.clone();

    let args_arr = match args {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };

    let mut params = Params::of_size(p_types.len() as u32);
    for (t, v) in p_types.iter().zip(args_arr.into_iter()) {
        let p = match t {
            DataType::I8 => Param::I8(
                v.as_i64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as i8,
            ),
            DataType::I16 => Param::I16(
                v.as_i64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as i16,
            ),
            DataType::I32 => Param::I32(
                v.as_i64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as i32,
            ),
            DataType::I64 => Param::I64(
                v.as_i64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))?,
            ),
            DataType::U8 => Param::U8(
                v.as_u64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as u8,
            ),
            DataType::U16 => Param::U16(
                v.as_u64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as u16,
            ),
            DataType::U32 => Param::U32(
                v.as_u64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as u32,
            ),
            DataType::U64 => Param::U64(
                v.as_u64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))?,
            ),
            DataType::F32 => Param::F32(
                v.as_f64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))? as f32,
            ),
            DataType::F64 => Param::F64(
                v.as_f64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))?,
            ),
            DataType::Bool => Param::Bool(
                v.as_bool()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))?,
            ),
            DataType::RustString | DataType::ExtString => Param::String(
                v.as_str()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))?
                    .to_string(),
            ),
            DataType::Object => {
                let id = v
                    .as_u64()
                    .ok_or_else(|| JsErrorBox::type_error("type mismatch"))?;
                let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(id));
                let real = data
                    .read()
                    .opaque_pointers
                    .get(pointer_key)
                    .copied()
                    .unwrap_or_default();
                Param::Object(real.ptr)
            }
            _ => return Err(JsErrorBox::type_error("unsupported parameter type")),
        };
        params.push(p);
    }

    let ffi = params.to_ffi::<Ext>();
    let ffi_arr = ffi.as_ffi_array();
    let ret = (metadata.callback)(ffi_arr);

    // convert return to JSON
    let j = match ret
        .into_param::<Ext>()
        .map_err(|e| JsErrorBox::generic("failed return value"))?
    {
        Param::I8(i) => serde_json::Value::from(i),
        Param::I16(i) => serde_json::Value::from(i),
        Param::I32(i) => serde_json::Value::from(i),
        Param::I64(i) => serde_json::Value::from(i),
        Param::U8(u) => serde_json::Value::from(u),
        Param::U16(u) => serde_json::Value::from(u),
        Param::U32(u) => serde_json::Value::from(u),
        Param::U64(u) => serde_json::Value::from(u),
        Param::F32(f) => serde_json::Value::from(f),
        Param::F64(f) => serde_json::Value::from(f),
        Param::Bool(b) => serde_json::Value::from(b),
        Param::String(s) => serde_json::Value::from(s),
        Param::Void => serde_json::Value::Null,
        Param::Object(ptr) => {
            let mut s = data.write();
            let key = s.get_opaque_pointer(ptr.into());
            serde_json::Value::from(key.0.as_ffi())
        }
        Param::Error(e) => return Err(JsErrorBox::generic(e)),
    };

    Ok(j)
}

#[doc = r""]
#[doc = r" An extension for use with the Deno JS runtime."]
#[doc = r" To use it, provide it as an argument when instantiating your runtime:"]
#[doc = r""]
#[doc = r" ```rust,ignore"]
#[doc = r" use deno_core::{ JsRuntime, RuntimeOptions };"]
#[doc = r""]
#[doc = concat!("let mut extensions = vec![",stringify!(turing),"::init()];")]
#[doc = r" let mut js_runtime = JsRuntime::new(RuntimeOptions {"]
#[doc = r"   extensions,"]
#[doc = r"   ..Default::default()"]
#[doc = r" });"]
#[doc = r" ```"]
#[doc = r""]
#[allow(non_camel_case_types)]
pub struct turing_op<Ext: ExternalFunctions> {
    _phantom: ::std::marker::PhantomData<Ext>,
}

impl<Ext: ExternalFunctions> turing_op<Ext> {
    fn ext() -> deno_core::Extension {
        #[allow(unused_imports)]
        use deno_core::Op;
        deno_core::Extension {
            name: ::std::stringify!(turing),
            deps: &[],
            js_files: {
                const JS: &[deno_core::ExtensionFileSource] =
                    &deno_core::include_js_files!(turing);
                ::std::borrow::Cow::Borrowed(JS)
            },
            esm_files: {
                const JS: &[deno_core::ExtensionFileSource] =
                    &deno_core::include_js_files!(turing);
                ::std::borrow::Cow::Borrowed(JS)
            },
            lazy_loaded_esm_files: {
                const JS: &[deno_core::ExtensionFileSource] =
                    &deno_core::include_lazy_loaded_js_files!(turing);
                ::std::borrow::Cow::Borrowed(JS)
            },
            esm_entry_point: {
                const V: ::std::option::Option<&'static ::std::primitive::str> =
                    deno_core::or!(, ::std::option::Option::None);
                V
            },
            ops: ::std::borrow::Cow::Owned(vec![{ turing_dispatch::<Ext>() }]),
            objects: ::std::borrow::Cow::Borrowed(&[]),
            external_references: ::std::borrow::Cow::Borrowed(&[]),
            global_template_middleware: ::std::option::Option::None,
            global_object_middleware: ::std::option::Option::None,
            op_state_fn: ::std::option::Option::None,
            needs_lazy_init: false,
            middleware_fn: ::std::option::Option::None,
            enabled: true,
        }
    }
    #[inline(always)]
    #[allow(unused_variables)]
    fn with_ops_fn(ext: &mut deno_core::Extension) {
        deno_core::extension!(!__ops__ ext __eot__);
    }
    #[inline(always)]
    #[allow(unused_variables)]
    fn with_middleware(ext: &mut deno_core::Extension) {}

    #[inline(always)]
    #[allow(unused_variables)]
    #[allow(clippy::redundant_closure_call)]
    fn with_customizer(ext: &mut deno_core::Extension) {}

    #[doc = r" Initialize this extension for runtime or snapshot creation."]
    #[doc = r""]
    #[doc = r" # Returns"]
    #[doc = r" an Extension object that can be used during instantiation of a JsRuntime"]
    #[allow(dead_code)]
    pub fn init() -> deno_core::Extension {
        let mut ext = Self::ext();
        Self::with_ops_fn(&mut ext);
        deno_core::extension!(!__config__ ext);
        Self::with_middleware(&mut ext);
        Self::with_customizer(&mut ext);
        ext
    }
    #[doc = r" Initialize this extension for runtime or snapshot creation."]
    #[doc = r""]
    #[doc = r" If this method is used, you must later call `JsRuntime::lazy_init_extensions`"]
    #[doc = r" with the result of this extension's `args` method."]
    #[doc = r""]
    #[doc = r" # Returns"]
    #[doc = r" an Extension object that can be used during instantiation of a JsRuntime"]
    #[allow(dead_code)]
    pub fn lazy_init() -> deno_core::Extension {
        let mut ext = Self::ext();
        Self::with_ops_fn(&mut ext);
        ext.needs_lazy_init = true;
        Self::with_middleware(&mut ext);
        Self::with_customizer(&mut ext);
        ext
    }
    #[doc = r" Create an `ExtensionArguments` value which must be passed to"]
    #[doc = r" `JsRuntime::lazy_init_extensions`."]
    #[allow(dead_code, unused_mut)]
    pub fn args() -> deno_core::ExtensionArguments {
        
        deno_core::extension!(!__config__ args);
        deno_core::ExtensionArguments {
            name: ::std::stringify!(turing),
            op_state_fn: ::std::option::Option::None,
        }
    }
}

impl<Ext> DenoEngine<Ext>
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    pub fn new(
        js_functions: &FxHashMap<String, ScriptFnMetadata>,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Result<Self> {
        // Register a single dispatch op generated by the `#[op]` macro.

        let runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![
                turing_op::<Ext>::init(),
            ],
            module_loader: None,
            ..Default::default()
        });

        Ok(Self {
            runtime,
            module_name: None,
            deno_fns: js_functions.clone(),
            data,
            _ext: PhantomData,
        })
    }

    pub fn load_script(&mut self, path: &Path) -> Result<()> {
        let script = fs::read_to_string(path)?;

        let mname = path.to_string_lossy().to_string();
        self.runtime
            .execute_script(mname.clone(), script)
            .map_err(|e| anyhow!(e.to_string()))?;
        self.module_name = Some(mname);

        Ok(())
    }

    pub fn call_fn(
        &mut self,
        _name: &str,
        _params: Params,
        _ret_type: crate::interop::params::DataType,
        _data: Arc<RwLock<EngineDataState>>,
    ) -> crate::interop::params::Param {
        crate::interop::params::Param::Error(
            "Deno engine call from Rust -> JS is not implemented".to_string(),
        )
    }
}
