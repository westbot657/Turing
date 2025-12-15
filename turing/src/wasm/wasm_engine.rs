use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::task::Poll;

use anyhow::{anyhow, Result};
use tokio::io::AsyncWrite;
use wasmtime::{Caller, Config, Engine, FuncType, Instance, Linker, Memory, MemoryAccessError, Module, Store, Val, ValType};
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
    _ext: PhantomData<Ext>,
}

pub type WasmCallback = extern "C" fn(FfiParamArray) -> FfiParam;

pub struct WasmFnMetadata {
    capability: String,
    callback: WasmCallback,
    param_types: Vec<DataType>,
    return_type: Vec<DataType>
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
        self.inner.write().unwrap().extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let s = {
            str::from_utf8(&self.inner.read().unwrap())
                .unwrap()
                .to_string()
        };
        self.inner.write().unwrap().clear();
        if self.is_err {
            Ext::log_critical(s)
        } else {
            Ext::log_info(s);
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
        self.inner.write().unwrap().extend(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        let s = {
            str::from_utf8(&self.inner.read().unwrap())
                .unwrap()
                .to_string()
        };
        self.inner.write().unwrap().clear();
        if self.is_err {
            Ext::log_critical(s);
        } else {
            Ext::log_info(s);
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
    CStr::from_bytes_until_nul(&data[message as usize..])
        .expect("Not a valid CStr")
        .to_string_lossy()
        .to_string()
}

/// writes a string from rust memory to wasm memory.
pub fn write_string(
    pointer: u32,
    string: String,
    memory: &Memory,
    caller: Caller<'_, WasiP1Ctx>,
) -> Result<(), MemoryAccessError> {
    let string = CString::new(string).unwrap();
    let string = string.into_bytes_with_nul();
    memory.write(caller, pointer as usize, &string)
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> WasmInterpreter<Ext> {

    pub fn new(wasm_functions: HashMap<String, WasmFnMetadata>, data: Arc<RwLock<WasmDataState>>) -> Result<WasmInterpreter<Ext>> {
        let mut config = Config::new();
        config.wasm_threads(false);
        // config.cranelift_pcc(true); // do sandbox verification checks
        config.async_support(false);
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        config.wasm_bulk_memory(true);
        config.wasm_reference_types(true);
        config.wasm_multi_memory(false);
        config.max_wasm_stack(512 * 1024); // 512KB
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
            _ext: PhantomData::default()
        })
    }

    fn bind_wasm(engine: &Engine, linker: &mut Linker<WasiP1Ctx>, wasm_fns: HashMap<String, WasmFnMetadata>, data: Arc<RwLock<WasmDataState>>) -> Result<()> {

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

            let p_types = metadata.param_types.iter().map(|d| d.to_val_type()).collect::<Result<Vec<ValType>>>()?;
            let r_types = metadata.return_type.iter().map(|d| d.to_val_type()).collect::<Result<Vec<ValType>>>()?;

            let ft = FuncType::new(engine, p_types, r_types);
            let cap = metadata.capability;
            let callback = metadata.callback;
            let pts = metadata.param_types;

            let data2 = Arc::clone(&data);
            linker.func_new(
                "env",
                name.clone().as_str(),
                ft,
                move |caller, ps, rs| {
                    wasm_bind_env::<Ext>(&data2, caller, &cap, ps, rs, &pts, &callback)
                }
            )?;

        }

        Ok(())
    }

    pub fn load_script(&mut self, path: &Path) -> Result<()> {
        let wasm = fs::read(path)?;

        let module = Module::new(&self.engine, wasm)?;

        let instance = self.linker.instantiate(&mut self.store, &module)?;

        self.script_instance = Some(instance);

        Ok(())
    }

    /// Calls a function in the loaded wasm script with the given parameters and return type.
    pub fn call_fn(
        &mut self,
        name: &str,
        params: Params,
        ret_type: DataType,
        // use a refcell to avoid borrow issues
        data: Arc<RwLock<WasmDataState>>,
    ) -> Param {
        let Some(instance) = &mut self.script_instance else {
            return Param::Error("No script is loaded or reentry was attempted".to_string());
        };

        let Some(f) = instance.get_func(&mut self.store, name) else {
            return Param::Error("Function does not exist".to_string());
        };
        let memory = instance
            .get_export(&mut self.store, "memory")
            .and_then(|m| m.into_memory())
            .unwrap();
        let args = params.to_args(&data);

        let mut res = match ret_type {
            DataType::Void => Vec::new(),
            DataType::F32 => vec![Val::F32(0)],
            DataType::F64 => vec![Val::F64(0)],
            DataType::I64
            | DataType::U64 => vec![Val::I64(0)],
            _ => vec![Val::I32(0)],
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
        Param::from_type_val(ret_type, rt, &data, &memory, &self.store)
    }
}


fn wasm_bind_env<Ext: ExternalFunctions>(
    data: &Arc<RwLock<WasmDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    cap: &String,
    ps: &[Val],
    rs: &mut [Val],
    p: &Vec<DataType>,
    func: &WasmCallback
) -> Result<()> {

    if !data.read().expect("WasmDataState lock poisoned").active_capabilities.contains(cap) {
        return Err(anyhow!("Mod capability '{}' is not currently loaded", cap))
    }

    let mut params = Params::new();
    for (exp_typ, value) in p.iter().zip(ps) {
        params.push(exp_typ.to_param_with_val(value, &mut caller, &data)?)
    }

    let ffi_params = params.to_ffi();

    // Call to C#/rust's provided callback using a clone so we can still cleanup
    let res = func(ffi_params.clone()).to_param::<Ext>()?;

    // Cleanup
    let _ = ffi_params.to_params::<Ext>();

    let mut s = data.write().unwrap();

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
            let pointer = ExtPointer::from(pointer );
            let opaque = if let Some(opaque) = s.pointer_backlink.get(&pointer) {
                *opaque
            } else {
                let op = s.opaque_pointers.insert(pointer);
                s.pointer_backlink.insert(pointer, op);
                op
            };
            Val::I32(opaque.0.as_ffi() as i32)
        }
        Param::Error(er) => {
            return Err(anyhow!("Error executing C# function: {}", er))?;
        }
        Param::Void => return Ok(()),
    };
    rs[0] = rv;

    Ok(())
}
