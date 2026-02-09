use std::ffi::{CStr, CString};
use std::fs;
use std::marker::PhantomData;
use std::panic::catch_unwind;
use std::path::Path;
use std::sync::Arc;
use std::task::Poll;

use crate::engine::types::{ScriptCallback, ScriptFnMetadata};
use crate::interop::params::{DataType, ExtTypes, ObjectId, Param, Params};
use crate::interop::types::Semver;
use crate::key_vec::KeyVec;
use crate::{EngineDataState, ExternalFunctions, ScriptFnKey};
use anyhow::{Result, anyhow, bail};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use tokio::io::AsyncWrite;
use wasmtime::{
    Caller, Config, Engine, Func, FuncType, Instance, Linker, Memory, MemoryAccessError, Module,
    Store, TypedFunc, Val, ValType,
};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p1::WasiP1Ctx;

impl DataType {
    pub fn to_wasm_val_param(
        &self,
        val: &Val,
        caller: &mut Caller<'_, WasiP1Ctx>,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<Param> {
        use crate::engine::wasm_engine::get_wasm_string;
        use wasmtime::Val;

        macro_rules! dequeue {
            ($typ:tt :: $init:tt; $x:tt ) => {{
                let mut s = data.write();
                let arr = s.f32_queue.drain(..$x).collect::<Vec<f32>>();
                Ok(Param::$typ(glam::$typ::$init(arr.as_slice().try_into()?)))
            }};
        }

        match (self, val) {
            (DataType::I8, Val::I32(i)) => Ok(Param::I8(*i as i8)),
            (DataType::I16, Val::I32(i)) => Ok(Param::I16(*i as i16)),
            (DataType::I32, Val::I32(i)) => Ok(Param::I32(*i)),
            (DataType::I64, Val::I64(i)) => Ok(Param::I64(*i)),
            (DataType::U8, Val::I32(u)) => Ok(Param::U8(*u as u8)),
            (DataType::U16, Val::I32(u)) => Ok(Param::U16(*u as u16)),
            (DataType::U32, Val::I32(u)) => Ok(Param::U32(*u as u32)),
            (DataType::U64, Val::I64(u)) => Ok(Param::U64(*u as u64)),
            (DataType::F32, Val::F32(f)) => Ok(Param::F32(f32::from_bits(*f))),
            (DataType::F64, Val::F64(f)) => Ok(Param::F64(f64::from_bits(*f))),
            (DataType::Bool, Val::I32(b)) => Ok(Param::Bool(*b != 0)),
            (DataType::RustString | DataType::ExtString, Val::I32(ptr)) => {
                let ptr = *ptr as u32;

                let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
                    return Err(anyhow!("wasm does not export memory"));
                };
                let st = get_wasm_string(ptr, memory.data(&caller));
                Ok(Param::String(st))
            }
            (DataType::Object, Val::I64(pointer_id)) => {
                let pointer_id = *pointer_id as u64;

                if pointer_id == 0 {
                    // reserved value for null pointers
                    return Ok(Param::Object(ObjectId::null()));
                }

                Ok(Param::Object(ObjectId::new(pointer_id)))
            }
            (DataType::Vec2, Val::I32(2)) => dequeue!(Vec2::from_array; 2),
            (DataType::Vec3, Val::I32(3)) => dequeue!(Vec3::from_array; 3),
            (DataType::RustVec4 | DataType::ExtVec4, Val::I32(4)) => dequeue!(Vec4::from_array; 4),
            (DataType::RustQuat | DataType::ExtQuat, Val::I32(4)) => dequeue!(Quat::from_array; 4),
            (DataType::RustMat4 | DataType::ExtMat4, Val::I32(16)) => {
                dequeue!(Mat4::from_cols_array; 16)
            }
            _ => Err(anyhow!("Mismatched parameter type")),
        }
    }

    #[cfg(feature = "wasm")]
    pub fn to_val_type(&self) -> Result<ValType> {
        match self {
            DataType::I8
            | DataType::I16
            | DataType::I32
            | DataType::U8
            | DataType::U16
            | DataType::U32
            | DataType::Bool
            | DataType::RustString
            | DataType::ExtString
            | DataType::Vec2
            | DataType::Vec3
            | DataType::RustVec4
            | DataType::ExtVec4
            | DataType::RustQuat
            | DataType::ExtQuat
            | DataType::RustMat4
            | DataType::ExtMat4 => Ok(ValType::I32),

            DataType::I64 | DataType::U64 | DataType::Object => Ok(ValType::I64),

            DataType::F32 => Ok(ValType::F32),
            DataType::F64 => Ok(ValType::F64),
            DataType::Void => Err(anyhow!(
                "Void is only allowed as a singular return type for WASM."
            )), // voids are represented as i32 0

            _ => Err(anyhow!("Invalid wasm value type: {}", self)),
        }
    }
}

impl Param {
    pub fn from_wasm_type_val(
        typ: DataType,
        val: Val,
        data: &Arc<RwLock<EngineDataState>>,
        memory: &Memory,
        caller: &Store<WasiP1Ctx>,
    ) -> Self {
        macro_rules! dequeue {
            ($typ:tt :: $init:tt; $x:tt ) => {{
                let mut s = data.write();
                Param::$typ(glam::$typ::$init(s.f32_queue.make_contiguous()))
            }};
        }

        match (typ, val) {
            (DataType::I8, Val::I32(i)) => Param::I8(i as i8),
            (DataType::I16, Val::I32(i)) => Param::I16(i as i16),
            (DataType::I32, Val::I32(i)) => Param::I32(i),
            (DataType::I64, Val::I64(i)) => Param::I64(i),
            (DataType::U8, Val::I32(u)) => Param::U8(u as u8),
            (DataType::U16, Val::I32(u)) => Param::U16(u as u16),
            (DataType::U32, Val::I32(u)) => Param::U32(u as u32),
            (DataType::U64, Val::I64(u)) => Param::U64(u as u64),
            (DataType::F32, Val::F32(f)) => Param::F32(f32::from_bits(f)),
            (DataType::F64, Val::F64(f)) => Param::F64(f64::from_bits(f)),
            (DataType::Bool, Val::I32(b)) => Param::Bool(b != 0),
            (DataType::RustString | DataType::ExtString, Val::I32(ptr)) => {
                let ptr = ptr as u32;
                let st = get_wasm_string(ptr, memory.data(caller));
                Param::String(st)
            }
            (DataType::Object, Val::I64(op)) => Param::Object(ObjectId::new(op as u64)),
            (DataType::RustError | DataType::ExtError, Val::I32(ptr)) => {
                let ptr = ptr as u32;
                let st = get_wasm_string(ptr, memory.data(caller));
                Param::Error(format!("WASM Error: {}", st))
            }
            (DataType::Void, _) => Param::Void,

            (DataType::Vec2, _) => dequeue!(Vec2::from_slice; 2),
            (DataType::Vec3, _) => dequeue!(Vec3::from_slice; 3),
            (DataType::RustVec4 | DataType::ExtVec4, _) => dequeue!(Vec4::from_slice; 4),
            (DataType::RustQuat | DataType::ExtQuat, _) => dequeue!(Quat::from_slice; 4),
            (DataType::RustMat4 | DataType::ExtMat4, _) => {
                dequeue!(Mat4::from_cols_slice; 16)
            }
            (DataType::RustU32Buffer | DataType::ExtU32Buffer, Val::I32(ptr)) => {
                let ptr = ptr as u32;
                let len = data.write().f32_queue.pop_front().unwrap().to_bits();
                Param::U32Buffer(get_u32_vec(ptr, len, memory.data(caller)).unwrap())
            }
            // Fallback: if the Val doesn't match the expected variant, return an error Param
            _ => Param::Error(format!(
                "Type mismatch converting WASM value to Param: expected {:?}, got {:?}",
                typ, val
            )),
        }
    }

    pub fn into_wasm_val(self, data: &Arc<RwLock<EngineDataState>>) -> Result<Option<Val>> {
        let mut s = data.write();
        macro_rules! enqueue {
            ( $v:tt ; $sz:tt ) => {{
                s.f32_queue.append(&mut $v.to_array().into());
                Val::I32($sz)
            }};
            ($m:tt # $sz:tt) => {{
                s.f32_queue.append(&mut $m.to_cols_array().into());
                Val::I32($sz)
            }};
        }
        Ok(Some(match self {
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
            Param::Error(er) => {
                return Err(anyhow!("Error executing host function: {}", er));
            }
            Param::Object(pointer) => {
                if pointer.is_null() {
                    // reserved value for null pointers
                    return Ok(Some(Val::I64(pointer.as_ffi() as i64)));
                }

                Val::I64(pointer.as_ffi() as i64)
            }
            Param::Void => return Ok(None),
            Param::Vec2(v) => enqueue!(v; 2),
            Param::Vec3(v) => enqueue!(v; 3),
            Param::Vec4(v) => enqueue!(v; 4),
            Param::Quat(q) => enqueue!(q; 4),
            Param::Mat4(m) => enqueue!(m # 16),
            Param::U32Buffer(v) => {
                let l = v.len();
                s.u32_buffer_queue.push_back(v);
                Val::I32(l as i32)
            }
        }))
    }
}

impl Params {
    /// Converts the Params into a vector of Wasmtime Val types for function calling.
    pub fn to_wasm_args(self, data: &Arc<RwLock<EngineDataState>>) -> Result<SmallVec<[Val; 4]>> {
        // Acquire a single write lock for the duration of conversion to avoid
        // repeated locking/unlocking when pushing strings or registering objects.
        if self.is_empty() {
            return Ok(SmallVec::default());
        }

        let mut s = data.write();
        macro_rules! enqueue {
            ( $v:tt ; $sz:tt ) => {{
                s.f32_queue.append(&mut $v.to_array().into());
                Ok(Val::I32($sz))
            }};
            ($m:tt # $sz:tt) => {{
                s.f32_queue.append(&mut $m.to_cols_array().into());
                Ok(Val::I32($sz))
            }};
        }

        self.params
            .into_iter()
            .map(|p| match p {
                Param::I8(i) => Ok(Val::I32(i as i32)),
                Param::I16(i) => Ok(Val::I32(i as i32)),
                Param::I32(i) => Ok(Val::I32(i)),
                Param::I64(i) => Ok(Val::I64(i)),
                Param::U8(u) => Ok(Val::I32(u as i32)),
                Param::U16(u) => Ok(Val::I32(u as i32)),
                Param::U32(u) => Ok(Val::I32(u as i32)),
                Param::U64(u) => Ok(Val::I64(u as i64)),
                Param::F32(f) => Ok(Val::F32(f.to_bits())),
                Param::F64(f) => Ok(Val::F64(f.to_bits())),
                Param::Bool(b) => Ok(Val::I32(if b { 1 } else { 0 })),
                Param::String(st) => {
                    let l = st.len() + 1;
                    s.str_cache.push_back(st);
                    Ok(Val::I32(l as i32))
                }
                Param::Object(rp) => Ok(Val::I64(rp.as_ffi() as i64)),
                Param::Error(st) => Err(anyhow!("{st}")),
                Param::Void => unreachable!("Void shouldn't ever be added as an arg"),
                Param::Vec2(v) => enqueue!(v; 2),
                Param::Vec3(v) => enqueue!(v; 3),
                Param::Vec4(v) => enqueue!(v; 4),
                Param::Quat(q) => enqueue!(q; 4),
                Param::Mat4(m) => enqueue!(m # 16),
                Param::U32Buffer(v) => {
                    let l = v.len();
                    s.u32_buffer_queue.push_back(v);
                    Ok(Val::I32(l as i32))
                }
            })
            .collect()
    }
}

impl DataType {
    /// Returns true if this Param can be directly represented as a simple WASM value (i32, i64, f32, f64),
    ///  meaning it can be passed to and from WASM without any special handling or conversion.
    pub fn is_wasm_simple(&self) -> bool {
        matches!(
            self,
            DataType::I8
                | DataType::I16
                | DataType::I32
                | DataType::I64
                | DataType::U8
                | DataType::U16
                | DataType::U32
                | DataType::U64
                | DataType::F32
                | DataType::F64
                | DataType::Bool
        )
    }
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

#[derive(Default)]
struct FastCalls {
    update: Option<TypedFunc<f32, ()>>,
    fixed_update: Option<TypedFunc<f32, ()>>,
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
    NoParamsObject(TypedFunc<(), u64>),
    NoParamsF32(TypedFunc<(), f32>),
    NoParamsF64(TypedFunc<(), f64>),
    // update and fixed update
    F32ToVoid(TypedFunc<f32, ()>),

    I32ToI32(TypedFunc<i32, i32>),
    I64ToI64(TypedFunc<i64, i64>),
    F32ToF32(TypedFunc<f32, f32>),
    F64ToF64(TypedFunc<f64, f64>),
    I32I32ToI32(TypedFunc<(i32, i32), i32>),
}

impl TypedFuncEntry {
    fn invoke(
        &self,
        store: &mut Store<WasiP1Ctx>,
        args: Params,
        _data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<Param, wasmtime::Error> {
        let get_object = |id: u64| -> Result<Param> { Ok(Param::Object(ObjectId::new(id))) };

        match self {
            TypedFuncEntry::NoParamsVoid(t) => t.call(store, ()).map(|_| Param::Void),
            TypedFuncEntry::NoParamsObject(t) => {
                t.call(store, ()).and_then(get_object)
            }
            TypedFuncEntry::NoParamsI32(t) => t.call(store, ()).map(Param::I32),
            TypedFuncEntry::NoParamsI64(t) => t.call(store, ()).map(Param::I64),
            TypedFuncEntry::NoParamsF32(t) => t.call(store, ()).map(Param::F32),
            TypedFuncEntry::NoParamsF64(t) => t.call(store, ()).map(Param::F64),
            TypedFuncEntry::I32ToI32(t) => {
                if args.len() != 1 {
                    bail!("Arg length `{}` != 1", args.len())
                }
                let a0 = match &args[0] {
                    Param::I32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i32 failed", args[0]),
                };
                t.call(store, a0).map(Param::I32)
            }
            TypedFuncEntry::I64ToI64(t) => {
                if args.len() != 1 {
                    bail!("Arg length `{}` != 1", args.len())
                }
                let a0 = match &args[0] {
                    Param::I64(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i64 failed", args[0]),
                };
                t.call(store, a0).map(Param::I64)
            }
            TypedFuncEntry::F32ToF32(t) => {
                if args.len() != 1 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::F32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> f32 failed", args[0]),
                };
                t.call(store, a0).map(Param::F32)
            }
            TypedFuncEntry::F32ToVoid(typed_func) => {
                if args.len() != 1 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::F32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> f32 failed", args[0]),
                };
                typed_func.call(store, a0).map(|_| Param::Void)
            }
            TypedFuncEntry::F64ToF64(t) => {
                if args.len() != 1 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::F64(v) => *v,
                    _ => bail!("Arg conversion {:?} -> f64 failed", args[0]),
                };
                t.call(store, a0).map(Param::F64)
            }
            TypedFuncEntry::I32I32ToI32(t) => {
                if args.len() != 2 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::I32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i32 failed", args[0]),
                };
                let a1 = match &args[1] {
                    Param::I32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i32 failed", args[1]),
                };
                t.call(store, (a0, a1)).map(Param::I32)
            }
        }
    }

    fn from_func(store: &mut Store<WasiP1Ctx>, func: Func) -> Option<Self> {
        // try 0 params
        if let Ok(t) = func.typed::<(), ()>(&store) {
            return Some(TypedFuncEntry::NoParamsVoid(t));
        }
        if let Ok(t) = func.typed::<(), i32>(&store) {
            return Some(TypedFuncEntry::NoParamsI32(t));
        }
        if let Ok(t) = func.typed::<(), i64>(&store) {
            return Some(TypedFuncEntry::NoParamsI64(t));
        }
        if let Ok(t) = func.typed::<(), f32>(&store) {
            return Some(TypedFuncEntry::NoParamsF32(t));
        }
        if let Ok(t) = func.typed::<(), f64>(&store) {
            return Some(TypedFuncEntry::NoParamsF64(t));
        }

        // 1 param -> same-typed returns
        if let Ok(t) = func.typed::<f32, ()>(&store) {
            return Some(TypedFuncEntry::F32ToVoid(t));
        }

        if let Ok(t) = func.typed::<i32, i32>(&store) {
            return Some(TypedFuncEntry::I32ToI32(t));
        }
        if let Ok(t) = func.typed::<i64, i64>(&store) {
            return Some(TypedFuncEntry::I64ToI64(t));
        }
        if let Ok(t) = func.typed::<f32, f32>(&store) {
            return Some(TypedFuncEntry::F32ToF32(t));
        }
        if let Ok(t) = func.typed::<f64, f64>(&store) {
            return Some(TypedFuncEntry::F64ToF64(t));
        }

        // 2 params (i32,i32)->i32
        if let Ok(t) = func.typed::<(i32, i32), i32>(&store) {
            return Some(TypedFuncEntry::I32I32ToI32(t));
        }

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
            _ext: PhantomData,
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

fn get_u32_vec(ptr: u32, len: u32, data: &[u8]) -> Option<Vec<u32>> {
    let start = ptr as usize;
    let end = start.checked_add((len as usize).checked_mul(4)?)?;
    if end > data.len() {
        return None;
    }
    let mut vec = vec![0u32; len as usize];
    for (i, u) in &mut vec.iter_mut().enumerate() {
        let offset = start + i * 4;
        *u = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
    }
    Some(vec)
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

pub fn write_u32_vec(
    pointer: u32,
    buf: &[u32],
    memory: &Memory,
    caller: Caller<'_, WasiP1Ctx>,
) -> Result<(), MemoryAccessError> {
    let mut bytes = Vec::with_capacity(buf.len() * 4);
    for (i, num) in buf.iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&num.to_le_bytes())
    }
    memory.write(caller, pointer as usize, &bytes)
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
                continue;
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
                            Ext::log_critical(format!(
                                "WASM function {internal_name} panicked: {msg}"
                            ));
                            Err(anyhow!("WASM function panicked: {msg}"))
                        }
                    }
                },
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
        Param::from_wasm_type_val(ret_type, rt, data, memory, &self.store)
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
    for (exp_typ, value) in p.iter().zip(ps) {
        let param = exp_typ.to_wasm_val_param(value, &mut caller, data)?;
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

/// internal for use in the wasm engine only
pub fn wasm_host_strcpy(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();

    if let Some(next_str) = data.write().str_cache.pop_front()
        && next_str.len() + 1 == size as usize
        && let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
            write_wasm_string(ptr as u32, &next_str, &memory, caller)?;
            return Ok(());
        }

    Err(anyhow!(
        "An error occurred whilst copying string to wasm memory"
    ))
}

pub fn wasm_host_bufcpy(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();

    if let Some(next_buf) = data.write().u32_buffer_queue.pop_front()
        && next_buf.len() == size as usize
        && let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
            write_u32_vec(ptr as u32, &next_buf, &memory, caller)?;
            return Ok(());
        }

    Err(anyhow!(
        "An error occurred whilst copying a Vec<u32> to wasm memory"
    ))
}

pub fn wasm_host_f32_dequeue(
    data: &Arc<RwLock<EngineDataState>>,
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let mut d = data.write();
    let Some(next) = d.f32_queue.pop_front() else {
        return Err(anyhow!("f32 queue is empty"));
    };
    rs[0] = Val::F32(next.to_bits());
    Ok(())
}

pub fn wasm_host_f32_enqueue(
    data: &Arc<RwLock<EngineDataState>>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let new = ps
        .first()
        .ok_or_else(|| anyhow!("no first parameter provided"))?
        .f32()
        .ok_or_else(|| anyhow!("parameter is not f32"))?;

    let mut d = data.write();
    d.f32_queue.push_back(new);

    Ok(())
}

pub fn wasm_host_u32_dequeue(
    data: &Arc<RwLock<EngineDataState>>,
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let mut d = data.write();
    let Some(next) = d.f32_queue.pop_front() else {
        return Err(anyhow!("f32 queue is empty"));
    };
    rs[0] = Val::I32(next.to_bits() as i32);
    Ok(())
}

pub fn wasm_host_u32_enqueue(
    data: &Arc<RwLock<EngineDataState>>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let new = ps
        .first()
        .ok_or_else(|| anyhow!("no first parameter provided"))?
        .i32()
        .ok_or_else(|| anyhow!("parameter is not u32"))?;

    let mut d = data.write();
    d.f32_queue.push_back(f32::from_bits(new as u32));

    Ok(())
}
