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
use wasmtime::{Caller, Config, Engine, FuncType, Func, Instance, Linker, Memory, MemoryAccessError, Module, Store, Val, ValType};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p1::WasiP1Ctx;
use crate::{wasm_host_strcpy, ExternalFunctions, WasmDataState};
use crate::interop::params::{DataType, FfiParam, FfiParamArray, Param, Params};
use crate::interop::types::ExtPointer;

pub struct WasmInterpreter<Ext: ExternalFunctions> {
    engine: Engine,
    store: Store<WasiP1Ctx>,
    linker: Linker<WasiP1Ctx>,
    script_instance: Option<Instance>,
    memory: Option<Memory>,
    func_cache: FxHashMap<String, Func>,
    _ext: PhantomData<Ext>,
}

pub type WasmCallback = extern "C" fn(FfiParamArray) -> FfiParam;

#[derive(Clone)]
pub struct WasmFnMetadata {
    pub capability: String,
    pub callback: WasmCallback,
    pub param_types: Vec<DataType>,
    pub return_type: Vec<DataType>
}

impl WasmFnMetadata {

    pub fn new(capability: impl ToString, callback: WasmCallback) -> Self {
        Self {
            capability: capability.to_string(),
            callback,
            param_types: Vec::new(),
            return_type: Vec::new(),
        }
    }

    /// May error if DataType is not a valid parameter type
    pub fn add_param_type(&mut self, p: DataType) -> Result<&mut Self> {
         if !p.is_valid_param_type() {
            return Err(anyhow!("DataType '{}' is not a valid parameter type", p))
        }
        self.param_types.push(p);

        Ok(self)
    }

    /// May error if DataType is not a valid return type
    pub fn add_return_type(&mut self, r: DataType) -> Result<&mut Self> {
        if !r.is_valid_return_type() {
            return Err(anyhow!("DataType '{}' is not a valid return type", r))
        }
        self.return_type.push(r);
        Ok(self)
    }

}

struct OutputWriter<Ext: ExternalFunctions + Send> {
    inner: Arc<RwLock<Vec<u8>>>,
    is_err: bool,
    _ext: PhantomData<Ext>,
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
pub fn get_string(message: u32, data: &[u8]) -> String {
    let c = CStr::from_bytes_until_nul(&data[message as usize..]).expect("Not a valid CStr");
    match c.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => c.to_string_lossy().into_owned(),
    }
}

/// writes a string from rust memory to wasm memory.
pub fn write_string(
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

    pub fn new(wasm_functions: &FxHashMap<String, WasmFnMetadata>, data: Arc<RwLock<WasmDataState>>) -> Result<Self> {
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
            _ext: PhantomData::default()
        })
    }

    fn bind_wasm(engine: &Engine, linker: &mut Linker<WasiP1Ctx>, wasm_fns: &FxHashMap<String, WasmFnMetadata>, data: Arc<RwLock<WasmDataState>>) -> Result<()> {

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
            let mut name = name.replace("::", "_").to_case(Case::Snake);
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
        self.script_instance = Some(instance);

        Ok(())
    }

    /// Calls a function in the loaded wasm script with the given parameters and return type.
    pub fn call_fn(
        &mut self,
        name: &str,
        params: Params,
        ret_type: DataType,
        data: Arc<RwLock<WasmDataState>>,
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
    data: &Arc<RwLock<WasmDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    cap: &String,
    ps: &[Val],
    rs: &mut [Val],
    p: &[DataType],
    func: &WasmCallback,
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
