use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use deno_core::op2;
use deno_core::{JsRuntime, OpState, RuntimeOptions, serde_v8, v8};
use deno_error::JsErrorBox;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::OpaquePointerKey;
use crate::engine::types::ScriptFnMetadata;
use crate::interop::params::{DataType, Param, Params};
use crate::interop::types::ExtPointer;
use crate::{EngineDataState, ExternalFunctions};
use slotmap::KeyData;


pub struct DenoEngine<Ext>
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    runtime: JsRuntime,
    module_name: Option<String>,
    deno_fns: FxHashMap<String, ScriptFnMetadata>,
    deno_fn_handles: FxHashMap<String, v8::Global<v8::Function>>,
    data: Arc<RwLock<EngineDataState>>,
    _ext: PhantomData<Ext>,
}

// Convert a host Param into a V8 `Value` within the provided scope.
fn param_to_v8<'s>(
    scope: &mut deno_core::v8::PinScope<'s, '_>,
    param: Param,
    data: &Arc<RwLock<EngineDataState>>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    // Convert Param -> serde_json -> V8 using serde_v8 to avoid scope type mismatches

    match param {
        Param::I8(i) => serde_v8::to_v8(scope, i).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::I16(i) => serde_v8::to_v8(scope, i).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::I32(i) => serde_v8::to_v8(scope, i).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::I64(i) => serde_v8::to_v8(scope, i).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::U8(u) => serde_v8::to_v8(scope, u).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::U16(u) => serde_v8::to_v8(scope, u).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::U32(u) => serde_v8::to_v8(scope, u).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::U64(u) => serde_v8::to_v8(scope, u).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::F32(f) => serde_v8::to_v8(scope, f).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::F64(f) => serde_v8::to_v8(scope, f).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::Bool(b) => serde_v8::to_v8(scope, b).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::String(s) => {
            serde_v8::to_v8(scope, s).map_err(|e| JsErrorBox::generic(e.to_string()))
        }
        Param::Void => serde_v8::to_v8(scope, ()).map_err(|e| JsErrorBox::generic(e.to_string())),
        Param::Object(ptr) => {
            let mut write = data.write();
            let key = write.get_opaque_pointer(ExtPointer { ptr });
            let id = key.0.as_ffi();
            serde_v8::to_v8(scope, id).map_err(|e| JsErrorBox::generic(e.to_string()))
        }
        Param::Error(e) => Err(JsErrorBox::generic(e)),
    }
}

// Convert a V8 `Value` into a host `Param`.
fn v8_to_param<'s>(
    scope: &mut deno_core::v8::PinScope<'s, '_>,
    data: &Arc<RwLock<EngineDataState>>,
    value: v8::Local<'s, v8::Value>,
    expect_type: Option<DataType>,
) -> Param {
    if let Some(expect_type) = expect_type {
        return match expect_type {
            DataType::Void => Param::Void,
            DataType::Bool => {
                if value.is_boolean() {
                    Param::Bool(value.boolean_value(scope))
                } else {
                    Param::Error("expected boolean".to_string())
                }
            }
            DataType::I32 => {
                if value.is_int32() {
                    Param::I32(value.int32_value(scope).unwrap())
                } else {
                    Param::Error("expected int32".to_string())
                }
            }
            DataType::F64 => {
                if value.is_number() {
                    Param::F64(value.number_value(scope).unwrap())
                } else {
                    Param::Error("expected number".to_string())
                }
            }
            DataType::RustString | DataType::ExtString => {
                if value.is_string() {
                    let s = value.to_rust_string_lossy(scope);
                    Param::String(s)
                } else {
                    Param::Error("expected string".to_string())
                }
            }
            DataType::Object => {
                // expect a big integer id
                if value.is_big_int() {
                    let id = value.to_big_int(scope).unwrap().i64_value().0 as u64;
                    let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(id));

                    let read = data.read();
                    let Some(real) = read.opaque_pointers.get(pointer_key) else {
                        return Param::Error(format!("Invalid opaque pointer id: {}", id));
                    };
                    return Param::Object(real.ptr);
                } else {
                    Param::Error("expected object id (bigint)".to_string())
                }
            }
            _ => unreachable!("unsupported expected type {}", expect_type),
        };
    }

    if value.is_undefined() || value.is_null() {
        return Param::Void;
    }
    if value.is_boolean() {
        return Param::Bool(value.boolean_value(scope));
    }
    if value.is_int32() {
        return Param::I32(value.int32_value(scope).unwrap());
    }
    if value.is_uint32() {
        return Param::U32(value.uint32_value(scope).unwrap());
    }
    if value.is_big_int() {
        return Param::I64(value.to_big_int(scope).unwrap().i64_value().0);
    }
    if value.is_number() {
        return Param::F64(value.number_value(scope).unwrap());
    }
    if value.is_string() {
        let s = value.to_rust_string_lossy(scope);
        return Param::String(s);
    }
    if value.is_object() {
        // get the object's identity field
        let obj = value.to_object(scope).unwrap();
        let id_key = v8::String::new(scope, "__turing_pointer_id").unwrap();
        let id_val = obj.get(scope, id_key.into()).unwrap();
        // assume it's a big integer
        let id = id_val.to_big_int(scope).unwrap().i64_value().0 as u64;
        let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(id));

        let read = data.read();
        let Some(real) = read.opaque_pointers.get(pointer_key) else {
            return Param::Error(format!("Invalid opaque pointer id: {}", id));
        };
        return Param::Object(real.ptr);
    }

    if value.is_array() {
        return Param::Error("Array return types are not supported".to_string());
    }

    if value.is_function() {
        return Param::Error("Function return types are not supported".to_string());
    }

    unreachable!("Does not support {value:?}")
}

// Single dispatch op which receives a JSON array like `["fn.name", [arg0, arg1, ...]]`
/// Handles calling registered FFI functions from JS.
#[op2]
#[global]
fn turing_dispatch<Ext: ExternalFunctions>(
    state: &mut OpState,
    payload: v8::Local<v8::Value>,
) -> Result<v8::Global<v8::Value>, JsErrorBox> {
    // payload expected to be [name, args]

    if !payload.is_array() {
        return Err(JsErrorBox::generic("invalid payload"));
    }

    let data = state.borrow::<Arc<RwLock<EngineDataState>>>().clone();
    let map = state
        .borrow::<FxHashMap<String, ScriptFnMetadata>>()
        .clone();

    // parse array
    let array = v8::Local::<v8::Array>::try_from(payload)
        .map_err(|_| JsErrorBox::generic("invalid payload"))?;

    let runtime = state.borrow_mut::<JsRuntime>();
    deno_core::scope!(scope, runtime);
    // let scope = state.borrow_mut();

    let (name, args) = {
        let length = array.length();
        let name = array
            .get_index(scope, 0)
            .ok_or_else(|| JsErrorBox::generic("invalid payload"))?
            .to_string(scope)
            .ok_or_else(|| JsErrorBox::generic("invalid function name"))?
            .to_rust_string_lossy(scope);
        let args = (0..length)
            .filter_map(|i| array.get_index(scope, i))
            .collect::<Vec<_>>();

        (name, args)
    };

    let metadata = map
        .get(&name)
        .ok_or_else(|| JsErrorBox::generic("function not found"))?
        .clone();

    let p_types = metadata.param_types.clone();

    let params = Params::from_iter(
        p_types
            .iter()
            .enumerate()
            .map(|(i, exp_type)| {
                let arg = args
                    .get(i)
                    .ok_or_else(|| JsErrorBox::generic("missing argument"))?;
                let param = v8_to_param(scope, &data, *arg, Some(*exp_type));
                Ok(param)
            })
            .collect::<Result<Vec<Param>, JsErrorBox>>()?,
    );

    let ffi = params.to_ffi::<Ext>();
    let ffi_arr = ffi.as_ffi_array();
    let ret = (metadata.callback)(ffi_arr);

    // convert return to JSON
    let ret = ret
        .into_param::<Ext>()
        .map_err(|e| JsErrorBox::generic("failed return value"))?;

    let j = param_to_v8(scope, ret, &data)?;

    let global = v8::Global::new(scope, j);

    Ok(global)
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
                const JS: &[deno_core::ExtensionFileSource] = &deno_core::include_js_files!(turing);
                ::std::borrow::Cow::Borrowed(JS)
            },
            esm_files: {
                const JS: &[deno_core::ExtensionFileSource] = &deno_core::include_js_files!(turing);
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

        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![turing_op::<Ext>::init()],
            module_loader: None,
            ..Default::default()
        });

        // Inject a small helper once to minimize per-call overhead.
        // `__turing_call(name, argsArray)` will look up the function and call it.
        let helper = r#"globalThis.__turing_call = function(name, args) { const fn = globalThis[name]; if (typeof fn !== 'function') throw new Error('function not found'); return fn.apply(null, args); };"#;
        runtime
            .execute_script("__turing_helper", helper)
            .map_err(|e| anyhow!(e.to_string()))?;

        // Pre-cache function handles for faster calls.
        let mut fn_handles: FxHashMap<String, v8::Global<v8::Function>> = FxHashMap::default();
        {
            let context = runtime.main_context();
            deno_core::scope!(scope, &mut runtime);

            for name in js_functions.keys() {
                let ctx = v8::Local::new(scope, &context);
                let g = ctx.global(scope);
                let Some(key) = v8::String::new(scope, name.as_str()) else {
                    continue;
                };
                let Some(val) = g.get(scope, key.into()) else {
                    continue;
                };
                if !val.is_function() {
                    continue;
                }
                let Ok(func) = v8::Local::<v8::Function>::try_from(val) else {
                    continue;
                };
                fn_handles.insert(name.clone(), v8::Global::new(scope, func));
            }
        }

        Ok(Self {
            deno_fn_handles: fn_handles,
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
        name: &str,
        params: Params,
        ret_type: crate::interop::params::DataType,
        data: Arc<RwLock<EngineDataState>>,
    ) -> crate::interop::params::Param {
        // Basic implementation: serialize params to JSON and invoke the global JS
        // function by name. For now the return value is not converted in full
        // generality — we return `Void` on success and `Error(...)` on failure.

        // If we have a cached function handle, call it directly using V8 locals
        if let Some(func_global) = self.deno_fn_handles.get(name).cloned() {
            return self.quick_call(ret_type, &data, params, &func_global);
        }
        // Fallback: stringify args and run the helper (older path)
        let json_args = params
            .into_iter()
            .map(|p| p.to_serde(&data))
            .collect::<Result<Vec<_>, _>>();

        let args_literal = match json_args {
            Ok(vec) => serde_json::to_string(&serde_json::Value::Array(vec))
                .unwrap_or_else(|_| "[]".to_string()),
            Err(e) => return Param::Error(format!("argument conversion error: {}", e)),
        };

        let call_code = format!(
            "__turing_call({}, {});",
            serde_json::to_string(&name).unwrap(),
            args_literal
        );

        let script_name = format!("turing_call:{}", name);
        match self.runtime.execute_script(script_name, call_code) {
            Ok(global_val) => {
                // convert return value directly from V8
                deno_core::scope!(scope, self.runtime);
                let local = v8::Local::new(scope, &global_val);

                let param = v8_to_param(scope, &data, local, Some(ret_type));

                if ret_type == DataType::Object {
                    match param {
                        Param::I64(i) => {
                            let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(i as u64));
                            let real = data
                                .read()
                                .opaque_pointers
                                .get(pointer_key)
                                .copied()
                                .unwrap_or_default();
                            return Param::Object(real.ptr);
                        }
                        Param::U64(u) => {
                            let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(u));
                            let real = data
                                .read()
                                .opaque_pointers
                                .get(pointer_key)
                                .copied()
                                .unwrap_or_default();
                            return Param::Object(real.ptr);
                        }
                        _ => {
                            return Param::Error("expected object id (number) from JS".to_string());
                        }
                    }
                }

                param
            }
            Err(e) => Param::Error(e.to_string()),
        }
    }

    fn quick_call(
        &mut self,
        ret_type: DataType,
        data: &Arc<parking_lot::lock_api::RwLock<parking_lot::RawRwLock, EngineDataState>>,
        args_vec: Params,
        func_global: &v8::Global<v8::Function>,
    ) -> Param {
        deno_core::scope!(scope, self.runtime);
        // let context = self.runtime.main_context();
        // let isolate = self.runtime.v8_isolate();
        // deno_core::v8::scope!(let scope, isolate);
        // let context = v8::Local::new(scope, &context);
        // let scope = &mut ContextScope::new(scope, context);

        // enter scope and convert Params -> V8 locals
        let v8_args: Vec<v8::Local<v8::Value>> = match args_vec
            .into_iter()
            .map(|p| match param_to_v8(scope, p, data) {
                Ok(l) => Ok(l),
                Err(e) => Err(format!("argument conversion error: {}", e)),
            })
            .collect::<Result<_, _>>()
        {
            Ok(v) => v,
            Err(e) => return Param::Error(e),
        };

        let local_func = v8::Local::new(scope, func_global);
        let recv = v8::undefined(scope).into();
        let result = local_func.call(scope, recv, &v8_args);
        let result = match result {
            Some(r) => r,
            None => return Param::Error("JS call threw".to_string()),
        };

        // convert V8 value -> Param directly
        let param = v8_to_param(scope, data, result, None);

        // If the caller expects an object, interpret numeric return as opaque id
        if ret_type == DataType::Object {
            match param {
                Param::I64(i) => {
                    let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(i as u64));
                    let real = data
                        .read()
                        .opaque_pointers
                        .get(pointer_key)
                        .copied()
                        .unwrap_or_default();
                    return Param::Object(real.ptr);
                }
                Param::U64(u) => {
                    let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(u));
                    let real = data
                        .read()
                        .opaque_pointers
                        .get(pointer_key)
                        .copied()
                        .unwrap_or_default();
                    return Param::Object(real.ptr);
                }
                _ => return Param::Error("expected object id (number) from JS".to_string()),
            }
        }

        param
    }
}
