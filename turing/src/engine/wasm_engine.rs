use std::fs;
use std::marker::PhantomData;
use std::panic::catch_unwind;
use std::path::Path;
use std::sync::Arc;

use crate::engine::types::{ScriptCallback, ScriptFnMetadata};
use crate::engine::wasm_engine::host_helpers::{
    wasm_host_bufcpy, wasm_host_f32_dequeue, wasm_host_f32_enqueue, wasm_host_strcpy,
    wasm_host_u32_dequeue, wasm_host_u32_enqueue,
};
use crate::engine::wasm_engine::typed_calls::TypedFuncEntry;
use crate::engine::wasm_engine::writer::WriterInit;
use crate::interop::params::{DataType, ExtTypes, Param, Params};
use crate::interop::types::Semver;
use crate::key_vec::KeyVec;
use crate::{EngineDataState, ExternalFunctions, ScriptFnKey};
use anyhow::{Context, Result, anyhow};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use wasmtime::{
    AsContext, Caller, Config, Engine, Func, FuncType, Instance, Linker, Memory, Module, Store,
    TypedFunc, Val, ValType,
};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::p1::WasiP1Ctx;

mod host_helpers;
mod params;
mod typed_calls;
mod writer;

#[derive(Default)]
pub struct FastCalls {
    update: Option<TypedFunc<f32, ()>>,
    fixed_update: Option<TypedFunc<f32, ()>>,
}

pub struct WasmInterpreter<Ext: ExternalFunctions> {
    engine: Engine,
    store: Store<WasiP1Ctx>,
    linker: Linker<WasiP1Ctx>,
    script_instance: Option<Instance>,
    memory: Option<Memory>,

    func_cache: KeyVec<ScriptFnKey, (String, Func, Option<TypedFuncEntry>)>,

    fast_calls: FastCalls,
    pub api_versions: FxHashMap<String, Semver>,
    _ext: PhantomData<Ext>,
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> WasmInterpreter<Ext> {
    pub fn new(
        wasm_functions: &FxHashMap<String, ScriptFnMetadata>,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_threads(false);
        // config.cranelift_pcc(true); // do sandbox verification checks
        config.async_support(false);
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        config.wasm_bulk_memory(true);
        config.wasm_reference_types(true);
        config.wasm_multi_memory(false);
        config.max_wasm_stack(512 * 1024); // 512KB
        config.compiler_inlining(true);
        config.consume_fuel(false);

        let wasi = WasiCtxBuilder::new()
            .stdout(WriterInit::<Ext>(
                Arc::new(RwLock::new(Vec::new())),
                false,
                PhantomData,
            ))
            .stderr(WriterInit::<Ext>(
                Arc::new(RwLock::new(Vec::new())),
                true,
                PhantomData,
            ))
            .allow_tcp(false)
            .allow_udp(false)
            .build_p1();

        let engine = Engine::new(&config)?;
        let store = Store::new(&engine, wasi);

        let mut linker = <Linker<WasiP1Ctx>>::new(&engine);

        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t| t)?;

        Self::bind_wasm(&engine, &mut linker, wasm_functions, data)?;

        Ok(WasmInterpreter {
            engine,
            store,
            linker,
            script_instance: None,
            memory: None,
            func_cache: Default::default(),
            fast_calls: FastCalls::default(),
            api_versions: Default::default(),
            _ext: PhantomData,
        })
    }

    fn bind_wasm(
        engine: &Engine,
        linker: &mut Linker<WasiP1Ctx>,
        wasm_fns: &FxHashMap<String, ScriptFnMetadata>,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Result<()> {
        // Utility Functions

        // _host_strcpy(location: *const c_char, size: u32);
        // Should only be used in 2 situations:
        // 1. after a call to a function that "returns" a string, the guest
        //    is required to allocate the size returned in place of the string, and then
        //    call this, passing the allocated pointer and the size.
        //    If the size passed in does not exactly match the cached string, or there is no
        //    cached string, then 0 is returned, otherwise the input pointer is returned.
        // 2. for each argument of a function that expects a string, in linear order,
        //    failing to retrieve all param strings in the correct order will invalidate
        //    the strings with no way to recover.
        let data_strcpy = Arc::clone(&data);
        let data_bufcpy = Arc::clone(&data);
        let data_enqueue = Arc::clone(&data);
        let data_dequeue = Arc::clone(&data);
        let data_enqueue2 = Arc::clone(&data);
        let data_dequeue2 = Arc::clone(&data);
        linker.func_new(
            "env",
            "_host_strcpy",
            FuncType::new(engine, vec![ValType::I32, ValType::I32], vec![]),
            move |caller, p, _| wasm_host_strcpy(&data_strcpy, caller, p),
        )?;
        linker.func_new(
            "env",
            "_host_bufcpy",
            FuncType::new(engine, vec![ValType::I32, ValType::I32], vec![]),
            move |caller, p, _| wasm_host_bufcpy(&data_bufcpy, caller, p),
        )?;
        linker.func_new(
            "env",
            "_host_f32_enqueue",
            FuncType::new(engine, vec![ValType::F32], Vec::new()),
            move |_, p, _| wasm_host_f32_enqueue(&data_enqueue, p),
        )?;
        linker.func_new(
            "env",
            "_host_f32_dequeue",
            FuncType::new(engine, Vec::new(), vec![ValType::F32]),
            move |_, _, r| wasm_host_f32_dequeue(&data_dequeue, r),
        )?;
        linker.func_new(
            "env",
            "_host_u32_enqueue",
            FuncType::new(engine, vec![ValType::I32], Vec::new()),
            move |_, p, _| wasm_host_u32_enqueue(&data_enqueue2, p),
        )?;
        linker.func_new(
            "env",
            "_host_u32_dequeue",
            FuncType::new(engine, Vec::new(), vec![ValType::I32]),
            move |_, _, r| wasm_host_u32_dequeue(&data_dequeue2, r),
        )?;

        // External functions
        for (name, metadata) in wasm_fns.iter() {
            Self::bind_wasm_fn(name, metadata, linker, engine, Arc::clone(&data))
                .with_context(|| format!("Binding {name} script fn metadata {metadata:#?}"))?;
        }

        Ok(())
    }

    fn bind_wasm_fn(
        name: &str,
        metadata: &ScriptFnMetadata,
        linker: &mut Linker<WasiP1Ctx>,
        engine: &Engine,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Result<()> {
        // Convert from `ClassName::functionName` to `_class_name_function_name`
        let internal_name = metadata.as_internal_name(name);

        let mut param_types = metadata
            .param_types
            .iter()
            .map(|d| d.data_type)
            .collect::<Vec<DataType>>();

        if ScriptFnMetadata::is_instance_method(name) {
            // instance methods get an extra first parameter for the instance pointer
            param_types.insert(0, DataType::Object);
        }

        let param_wasm_types = param_types
            .iter()
            .map(|d| d.to_val_type())
            .collect::<Result<Vec<ValType>>>()?;

        // if the only return type is void, we treat it as no return types
        let fn_return_type = metadata
            .return_type
            .first()
            .cloned()
            .map(|d| d.0)
            .unwrap_or(DataType::Void);

        // WE ONLY SUPPORT SINGLE RETURN VALUES FOR NOW
        if metadata.return_type.len() > 1 {
            Ext::log_critical(format!(
                "WASM functions with multiple return values are not supported: {}",
                name
            ));
            return Ok(());
        }

        let r_types = if fn_return_type == DataType::Void {
            Vec::new()
        } else {
            vec![fn_return_type.to_val_type()?]
            // metadata.return_type.iter().map(|d| d.0.to_val_type()).collect::<Result<Vec<ValType>>>()?
        };
        let ft = FuncType::new(engine, param_wasm_types, r_types);
        let cap = metadata.capability.clone();
        let callback = metadata.callback;

        let data2 = Arc::clone(&data);

        Ext::log_debug(format!(
            "Registered wasm function: env::{internal_name} {}",
            ft
        ));

        linker.func_new(
            "env",
            internal_name.clone().as_str(),
            ft,
            move |caller, ps, rs| {
                match catch_unwind(std::panic::AssertUnwindSafe(|| {
                    wasm_bind_env::<Ext>(
                        &data2,
                        caller,
                        &cap,
                        ps,
                        rs,
                        param_types.as_slice(),
                        fn_return_type,
                        &callback,
                    )
                })) {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => {
                        // log errors since wasmtime doesn't propagate them with messages
                        Ext::log_critical(format!(
                            "WASM function {internal_name} returned error: {e}"
                        ));
                        Err(e)
                    }
                    Err(panic) => {
                        let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                            (*s).to_string()
                        } else if let Some(s) = panic.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "Unknown panic payload".to_string()
                        };
                        Ext::log_critical(format!("WASM function {internal_name} panicked: {msg}"));
                        Err(anyhow!("WASM function panicked: {msg}"))
                    }
                }
            },
        )?;
        Ok(())
    }

    pub fn load_script(&mut self, path: &Path) -> Result<()> {
        let wasm = fs::read(path)?;

        let module = Module::new(&self.engine, wasm)?;

        let instance = self.linker.instantiate(&mut self.store, &module)?;

        // Cache instance and exported memory to avoid repeated lookups per call
        let memory = instance
            .get_export(&mut self.store, "memory")
            .and_then(|m| m.into_memory())
            .ok_or_else(|| anyhow!("WASM module does not export memory"))?;

        self.memory = Some(memory);
        // clear any previous function cache and cache exports lazily
        self.func_cache.clear();

        // Pre-create typed wrappers for exported functions where possible to avoid first-call overhead.
        // Try a small set of common signatures and cache the TypedFunc if creation succeeds.
        for export in module.exports() {
            let name = export.name();
            let Some(func) = instance.get_func(&mut self.store, name) else {
                continue;
            };

            // ensure no duplicates
            if self.func_cache.key_of(|x| x.0 == name).is_some() {
                return Err(anyhow!(
                    "Duplicate exported function name in wasm module: {}",
                    name
                ));
            }
            self.func_cache.push((
                name.to_string(),
                func,
                TypedFuncEntry::from_func(&mut self.store, func),
            ));

            if name == "on_update" {
                let Ok(f) = func.typed::<f32, ()>(&mut self.store) else {
                    continue;
                };
                self.fast_calls.update = Some(f);
            } else if name == "on_fixed_update" {
                let Ok(f) = func.typed::<f32, ()>(&mut self.store) else {
                    continue;
                };
                self.fast_calls.fixed_update = Some(f);
            }

            if name.starts_with("_") && name.ends_with("_semver") {
                let Ok(f) = func.typed::<(), u64>(&mut self.store) else {
                    continue;
                };
                let Ok(ver) = f.call(&mut self.store, ()) else {
                    continue;
                };
                let loaded_mod = name
                    .strip_prefix("_")
                    .unwrap()
                    .strip_suffix("_semver")
                    .unwrap()
                    .to_string();
                let version = Semver::from_u64(ver);
                self.api_versions.insert(loaded_mod, version);
            }
        }

        self.script_instance = Some(instance);

        Ok(())
    }

    /// Calls a function in the loaded wasm script with the given parameters and return type.
    pub fn call_fn(
        &mut self,
        cache_key: ScriptFnKey,
        params: Params,
        ret_type: DataType,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Param {
        // Try cache first to avoid repeated name lookup and Val boxing/unboxing.
        // This shouldn't be necessary as all exported functions are indexed on load
        let (f_name, f, typed) = self.func_cache.get(&cache_key);

        // can only do a typed call if all parameters are simple and return type is simple or void, so if we have a cached typed func, we know it will work and skip the Val conversions.
        let can_typed_call = ret_type.is_wasm_simple()
            && params
                .iter()
                .all(|r| r.data_type::<ExtTypes>().is_wasm_simple());

        // Fast-path: typed cache (common signatures). Falls back to dynamic call below.
        if can_typed_call && let Some(typed) = typed {
            return typed
                .invoke(&mut self.store, params, data)
                .unwrap_or_else(|e| {
                    Param::Error(format!("Error calling wasm function typed: {e}"))
                });
        }

        let args = match params.to_wasm_args(data) {
            Ok(a) => a,
            Err(e) => return Param::Error(format!("Params error: {e}")),
        };

        // Fallback dynamic path
        let mut res: SmallVec<[Val; 1]> = match ret_type {
            DataType::Void => SmallVec::new(),
            DataType::F32 => SmallVec::from_buf([Val::F32(0)]),
            DataType::F64 => SmallVec::from_buf([Val::F64(0)]),
            DataType::ExtString | DataType::RustString => SmallVec::from_buf([Val::I32(0)]),
            // We use i64 for opaque pointers since we need the full 64 bits to store the pointer
            DataType::Object => SmallVec::from_buf([Val::I64(0)]),
            DataType::I64 | DataType::U64 => SmallVec::from_buf([Val::I64(0)]),

            DataType::ExtMat4
            | DataType::ExtVec4
            | DataType::ExtQuat
            | DataType::RustMat4
            | DataType::RustVec4
            | DataType::RustQuat => {
                // these are all passed as length to wasm, so we just need to reserve space for the pointer
                SmallVec::from_buf([Val::I32(0)])
            }

            // u32 buffer
            DataType::RustU32Buffer | DataType::ExtU32Buffer => SmallVec::from_buf([Val::I32(0)]),

            _ => SmallVec::from_buf([Val::I32(0)]),
        };

        // this are errors raised by wasm execution
        // e.g. stack overflow, out of bounds memory access, etc.
        if let Err(e) = f.call(&mut self.store, &args, &mut res) {
            return Param::Error(format!("Error calling wasm function: {}\n{}", f_name, e));
        }
        // Return void quickly
        if res.is_empty() {
            return Param::Void;
        }
        let rt = res[0];

        let memory = match &self.memory {
            Some(m) => m,
            None => return Param::Error("WASM memory not initialized".to_string()),
        };

        // convert Val to Param
        // if an error is returned from wasm, convert to Param::Error
        Param::from_wasm_type_val(ret_type, rt, data, memory, &self.store.as_context())
    }

    pub fn fast_call_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        if self.script_instance.is_none() {
            return Err("No script is loaded".to_string());
        }
        let Some(f) = &self.fast_calls.update else {
            return Ok(());
        };
        f.call(&mut self.store, delta_time)
            .map_err(|e| e.to_string())
    }

    pub fn fast_call_fixed_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        if self.script_instance.is_none() {
            return Err("No script is loaded".to_string());
        }
        let Some(f) = &self.fast_calls.fixed_update else {
            return Ok(());
        };
        f.call(&mut self.store, delta_time)
            .map_err(|e| e.to_string())
    }

    pub fn get_fn_key(&self, name: &str) -> Option<ScriptFnKey> {
        self.func_cache.key_of(|x| x.0 == name)
    }
}

/// Wraps a call from wasm into the host environment, checking capability availability
/// and converting parameters and return values as needed.
#[allow(clippy::too_many_arguments)]
fn wasm_bind_env<Ext: ExternalFunctions>(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    cap: &str,
    ps: &[Val],
    rs: &mut [Val],
    p: &[DataType],
    expected_return_type: DataType,
    func: &ScriptCallback,
) -> Result<()> {
    if !data.read().active_capabilities.contains(cap) {
        Ext::log_critical(format!(
            "Attempted to call mod capability '{}' which is not currently loaded",
            cap
        ));
        return Err(anyhow!("Mod capability '{}' is not currently loaded", cap));
    }

    // pre-allocate params to avoid repeated reallocations
    let mut params = Params::of_size(p.len() as u32);
    let memory = caller
        .get_export("memory")
        .and_then(|m| m.into_memory())
        .context("WASM memory not found")?;

    for (exp_typ, value) in p.iter().zip(ps) {
        let param =
            Param::from_wasm_type_val(*exp_typ, *value, data, &memory, &caller.as_context());
        params.push(param)
    }

    let ffi_params = params.to_ffi::<Ext>();
    let ffi_params_struct = ffi_params.as_ffi_array();

    // Call to C#/rust's provided callback using a clone so we can still cleanup
    let res = func(ffi_params_struct).into_param::<Ext>()?;

    let result_data_type = res.data_type::<ExtTypes>();
    if result_data_type != expected_return_type {
        return Err(anyhow!(
            "WASM function returned unexpected type. Expected: {:?}, Got: {:?}",
            expected_return_type,
            result_data_type
        ));
    }

    // Convert Param back to Val for return
    // TODO: Add mechanism for providing error messages to caller
    let Some(rv) = res.into_wasm_val(data)? else {
        return Ok(());
    };
    rs[0] = rv;

    Ok(())
}
