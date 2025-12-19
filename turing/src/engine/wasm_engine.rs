use std::ffi::{CStr, CString};
use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc};
use std::task::Poll;

use anyhow::{anyhow, Result};
use convert_case::{Case, Casing};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use tokio::io::AsyncWrite;
use wasmtime::{Caller, Config, Engine, FuncType, Func, Instance, Linker, Memory, MemoryAccessError, Module, Store, TypedFunc, Val, ValType};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p1::WasiP1Ctx;
use crate::engine::types::{ScriptCallback, ScriptFnMetadata};
use crate::{ExternalFunctions, EngineDataState};
use crate::interop::params::{DataType, FfiParam, FfiParamArray, Param, Params};
use crate::interop::types::ExtPointer;

pub struct WasmInterpreter<Ext: ExternalFunctions> {
    engine: Engine,
    store: Store<WasiP1Ctx>,
    linker: Linker<WasiP1Ctx>,
    script_instance: Option<Instance>,
    memory: Option<Memory>,
    func_cache: FxHashMap<String, Func>,
    typed_cache: FxHashMap<String, TypedFuncEntry>,
    _ext: PhantomData<Ext>,
}


struct OutputWriter<Ext: ExternalFunctions + Send> {
    inner: Arc<RwLock<Vec<u8>>>,
    is_err: bool,
    _ext: PhantomData<Ext>,
}

enum TypedFuncEntry {
    NoParamsVoid(TypedFunc<(), ()>),
    NoParamsI32(TypedFunc<(), i32>),
    NoParamsI64(TypedFunc<(), i64>),
    NoParamsF32(TypedFunc<(), f32>),
    NoParamsF64(TypedFunc<(), f64>),
    I32ToI32(TypedFunc<(i32,), i32>),
    I64ToI64(TypedFunc<(i64,), i64>),
    F32ToF32(TypedFunc<(f32,), f32>),
    F64ToF64(TypedFunc<(f64,), f64>),
    I32I32ToI32(TypedFunc<(i32,i32), i32>),
}

impl TypedFuncEntry {
    fn invoke(&self, store: &mut Store<WasiP1Ctx>, args: &[Val]) -> Result<Param, String> {
        match self {
            TypedFuncEntry::NoParamsVoid(t) => t.call(store, ()).map(|_| Param::Void).map_err(|e| e.to_string()),
            TypedFuncEntry::NoParamsI32(t) => t.call(store, ()).map(Param::I32).map_err(|e| e.to_string()),
            TypedFuncEntry::NoParamsI64(t) => t.call(store, ()).map(Param::I64).map_err(|e| e.to_string()),
            TypedFuncEntry::NoParamsF32(t) => t.call(store, ()).map(Param::F32).map_err(|e| e.to_string()),
            TypedFuncEntry::NoParamsF64(t) => t.call(store, ()).map(Param::F64).map_err(|e| e.to_string()),
            TypedFuncEntry::I32ToI32(t) => {
                if args.len() != 1 { return Err("Arg mismatch".to_string()) }
                let a0 = args[0].i32().ok_or_else(|| "Arg conversion".to_string())?;
                t.call(store, (a0,)).map(Param::I32).map_err(|e| e.to_string())
            }
            TypedFuncEntry::I64ToI64(t) => {
                if args.len() != 1 { return Err("Arg mismatch".to_string()) }
                let a0 = args[0].i64().ok_or_else(|| "Arg conversion".to_string())?;
                t.call(store, (a0,)).map(Param::I64).map_err(|e| e.to_string())
            }
            TypedFuncEntry::F32ToF32(t) => {
                if args.len() != 1 { return Err("Arg mismatch".to_string()) }
                let a0 = args[0].f32().ok_or_else(|| "Arg conversion".to_string())?;
                t.call(store, (a0,)).map(Param::F32).map_err(|e| e.to_string())
            }
            TypedFuncEntry::F64ToF64(t) => {
                if args.len() != 1 { return Err("Arg mismatch".to_string()) }
                let a0 = args[0].f64().ok_or_else(|| "Arg conversion".to_string())?;
                t.call(store, (a0,)).map(Param::F64).map_err(|e| e.to_string())
            }
            TypedFuncEntry::I32I32ToI32(t) => {
                if args.len() != 2 { return Err("Arg mismatch".to_string()) }
                let a0 = args[0].i32().ok_or_else(|| "Arg conversion".to_string())?;
                let a1 = args[1].i32().ok_or_else(|| "Arg conversion".to_string())?;
                t.call(store, (a0, a1)).map(Param::I32).map_err(|e| e.to_string())
            }
        }
    }

    fn from_func(store: &mut Store<WasiP1Ctx>, func: Func) -> Option<Self> {
        // try 0 params
        if let Ok(t) = func.typed::<(), ()>(&store) { return Some(TypedFuncEntry::NoParamsVoid(t)); }
        if let Ok(t) = func.typed::<(), i32>(&store) { return Some(TypedFuncEntry::NoParamsI32(t)); }
        if let Ok(t) = func.typed::<(), i64>(&store) { return Some(TypedFuncEntry::NoParamsI64(t)); }
        if let Ok(t) = func.typed::<(), f32>(&store) { return Some(TypedFuncEntry::NoParamsF32(t)); }
        if let Ok(t) = func.typed::<(), f64>(&store) { return Some(TypedFuncEntry::NoParamsF64(t)); }

        // 1 param -> same-typed returns
        if let Ok(t) = func.typed::<(i32,), i32>(&store) { return Some(TypedFuncEntry::I32ToI32(t)); }
        if let Ok(t) = func.typed::<(i64,), i64>(&store) { return Some(TypedFuncEntry::I64ToI64(t)); }
        if let Ok(t) = func.typed::<(f32,), f32>(&store) { return Some(TypedFuncEntry::F32ToF32(t)); }
        if let Ok(t) = func.typed::<(f64,), f64>(&store) { return Some(TypedFuncEntry::F64ToF64(t)); }

        // 2 params (i32,i32)->i32
        if let Ok(t) = func.typed::<(i32,i32), i32>(&store) { return Some(TypedFuncEntry::I32I32ToI32(t)); }

        // Not a supported typed signature
        None
    }
}

impl<Ext: ExternalFunctions + Send> std::io::Write for OutputWriter<Ext> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write().extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        // Move the inner buffer out so we avoid an extra copy when converting
        // bytes -> String. Taking the write lock lets us swap the Vec<u8>.
        let vec = {
            let mut guard = self.inner.write();
            std::mem::take(&mut *guard)
        };
        if !vec.is_empty() {
            let s = String::from_utf8_lossy(&vec).into_owned();
            if self.is_err {
                Ext::log_critical(s)
            } else {
                Ext::log_info(s);
            }
        }
        Ok(())
    }
}

impl<Ext: ExternalFunctions + Send> AsyncWrite for OutputWriter<Ext> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        self.inner.write().extend(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        // Move the inner buffer out so we avoid an extra copy when converting
        // bytes -> String. Taking the write lock lets us swap the Vec<u8>.
        let vec = {
            let mut guard = self.inner.write();
            std::mem::take(&mut *guard)
        };
        if !vec.is_empty() {
            let s = String::from_utf8_lossy(&vec).into_owned();
            if self.is_err {
                Ext::log_critical(s);
            } else {
                Ext::log_info(s);
            }
        }
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct WriterInit<Ext: ExternalFunctions>(Arc<RwLock<Vec<u8>>>, bool, PhantomData<Ext>);

impl<Ext: ExternalFunctions> IsTerminal for WriterInit<Ext> {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> StdoutStream for WriterInit<Ext> {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(OutputWriter::<Ext> {
            inner: self.0.clone(),
            is_err: self.1,
            _ext: PhantomData::default()
        })
    }
}

/// gets a string out of wasm memory into rust memory.
pub fn get_wasm_string(message: u32, data: &[u8]) -> String {
    let c = CStr::from_bytes_until_nul(&data[message as usize..]).expect("Not a valid CStr");
    match c.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => c.to_string_lossy().into_owned(),
    }
}

/// writes a string from rust memory to wasm memory.
pub fn write_wasm_string(
    pointer: u32,
    string: &str,
    memory: &Memory,
    caller: Caller<'_, WasiP1Ctx>,
) -> Result<(), MemoryAccessError> {
    let c = CString::new(string).unwrap();
    let bytes = c.into_bytes_with_nul();
    memory.write(caller, pointer as usize, &bytes)
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> WasmInterpreter<Ext> {

    pub fn new(wasm_functions: &FxHashMap<String, ScriptFnMetadata>, data: Arc<RwLock<EngineDataState>>) -> Result<Self> {
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
            .stdout(WriterInit::<Ext>(Arc::new(RwLock::new(Vec::new())), false, PhantomData::default()))
            .stderr(WriterInit::<Ext>(Arc::new(RwLock::new(Vec::new())), true, PhantomData::default()))
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
            typed_cache: Default::default(),
            _ext: PhantomData::default()
        })
    }

    fn bind_wasm(engine: &Engine, linker: &mut Linker<WasiP1Ctx>, wasm_fns: &FxHashMap<String, ScriptFnMetadata>, data: Arc<RwLock<EngineDataState>>) -> Result<()> {

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
        linker.func_new(
            "env",
            "_host_strcpy",
            FuncType::new(engine, vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            move |caller, p, r| {
                wasm_host_strcpy(&data_strcpy, caller, p, r)
            }
        )?;

        // External functions
        for (name, metadata) in wasm_fns.into_iter() {

            // Convert from `ClassName::functionName` to `_class_name_function_name`
            let mut name = name.replace(":", "_").replace(".", "_").to_case(Case::Snake);
            name.insert(0, '_');

            let p_types = metadata.param_types.iter().map(|d| d.to_val_type()).collect::<Result<Vec<ValType>>>()?;
            let r_types = metadata.return_type.iter().map(|d| d.to_val_type()).collect::<Result<Vec<ValType>>>()?;

            let ft = FuncType::new(engine, p_types, r_types);
            let cap = metadata.capability.clone();
            let callback = metadata.callback;
            let pts = metadata.param_types.clone();

            let data2 = Arc::clone(&data);
            linker.func_new(
                "env",
                name.as_str(),
                ft,
                move |caller, ps, rs| {
                    wasm_bind_env::<Ext>(&data2, caller, &cap, ps, rs, pts.as_slice(), &callback)
                }
            )?;

        }

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
        self.typed_cache.clear();

        // Pre-create typed wrappers for exported functions where possible to avoid first-call overhead.
        // Try a small set of common signatures and cache the TypedFunc if creation succeeds.
        for export in module.exports() {
            let name = export.name();
            let Some(func) = instance.get_func(&mut self.store, name) else { continue };

            if let Some(entry) = TypedFuncEntry::from_func(&mut self.store, func) {
                self.typed_cache.insert(name.to_string(), entry);
            }
        }

        self.script_instance = Some(instance);


        Ok(())
    }

    /// Calls a function in the loaded wasm script with the given parameters and return type.
    pub fn call_fn(
        &mut self,
        name: &str,
        params: Params,
        ret_type: DataType,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Param {
        let Some(instance) = &mut self.script_instance else {
            return Param::Error("No script is loaded or reentry was attempted".to_string());
        };
        // Try cache first to avoid repeated name lookup and Val boxing/unboxing.
        let Some(f) = self.func_cache.get(name).copied().or_else(|| {
            let found = instance.get_func(&mut self.store, name)?;
            self.func_cache.insert(name.to_string(), found);
            Some(found)
        }) else {
            return Param::Error("Function does not exist".to_string());
        };
        let memory = match &self.memory {
            Some(m) => m,
            None => return Param::Error("WASM memory not initialized".to_string()),
        };
        let args = params.to_wasm_args(&data);
        if let Err(e) = args {
            return Param::Error(format!("{e}"))
        }
        let args = args.unwrap();

        // Fast-path: typed cache (common signatures). Falls back to dynamic call below.
        if let Some(entry) = self.typed_cache.get(name) {
            match entry.invoke(&mut self.store, &args) {
                Ok(p) => return p,
                Err(e) => return Param::Error(e),
            }
        }

        // Fallback dynamic path
        let mut res: SmallVec<[Val; 1]> = match ret_type {
            DataType::Void => SmallVec::new(),
            DataType::F32 => SmallVec::from_buf([Val::F32(0)]),
            DataType::F64 => SmallVec::from_buf([Val::F64(0)]),
            DataType::I64
            | DataType::U64 => SmallVec::from_buf([Val::I64(0)]),
            _ => SmallVec::from_buf([Val::I32(0)]),
        };

        if let Err(e) = f.call(&mut self.store, &args, &mut res) {
            return Param::Error(e.to_string());
        }
        // Return void quickly
        if res.is_empty() {
            return Param::Void;
        }
        let rt = res[0];

        // convert Val to Param
        Param::from_wasm_type_val(ret_type, rt, &data, &memory, &self.store)
    }
}


fn wasm_bind_env<Ext: ExternalFunctions>(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    cap: &String,
    ps: &[Val],
    rs: &mut [Val],
    p: &[DataType],
    func: &ScriptCallback,
) -> Result<()> {

    if !data.read().active_capabilities.contains(cap) {
        return Err(anyhow!("Mod capability '{}' is not currently loaded", cap))
    }

    // pre-allocate params to avoid repeated reallocations
    let mut params = Params::of_size(p.len() as u32);
    for (exp_typ, value) in p.iter().zip(ps) {
        params.push(exp_typ.to_wasm_val_param(value, &mut caller, &data)?)
    }
    
    let ffi_params= params.to_ffi::<Ext>();
    let ffi_params_struct = ffi_params.as_ffi_array();

    // Call to C#/rust's provided callback using a clone so we can still cleanup
    let res = func(ffi_params_struct).into_param::<Ext>()?;
    
    let mut s = data.write();

    // Convert Param back to Val for return
    let rv = match res {
        Param::I8(i) => Val::I32(i as i32),
        Param::I16(i) => Val::I32(i as i32),
        Param::I32(i) => Val::I32(i),
        Param::I64(i) => Val::I64(i),
        Param::U8(u) => Val::I32(u as i32),
        Param::U16(u) => Val::I32(u as i32),
        Param::U32(u) => Val::I32(u as i32),
        Param::U64(u) => Val::I64(u as i64),
        Param::F32(f) => Val::F32(f.to_bits()),
        Param::F64(f) => Val::F64(f.to_bits()),
        Param::Bool(b) => Val::I32(if b { 1 } else { 0 }),
        Param::String(st) => {
            let l = st.len() + 1;
            s.str_cache.push_back(st);
            Val::I32(l as i32)
        }
        Param::Object(pointer) => {
            let pointer = ExtPointer::from(pointer);
            let opaque = s.get_opaque_pointer(pointer);
            Val::I64(opaque.0.as_ffi() as i64)
        }
        Param::Error(er) => {
            return Err(anyhow!("Error executing C# function: {}", er));
        }
        Param::Void => return Ok(()),
    };
    rs[0] = rv;

    Ok(())
}


/// internal for use in the wasm engine only
pub fn wasm_host_strcpy(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();

    if let Some(next_str) = data.write().str_cache.pop_front()
        && next_str.len() + 1 == size as usize
    {
        if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
            write_wasm_string(ptr as u32, &next_str, &memory, caller)?;
            rs[0] = Val::I32(ptr);
        }
        return Ok(());
    }

    Ok(())
}

