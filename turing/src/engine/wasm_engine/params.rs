use std::sync::Arc;

use anyhow::anyhow;
use smallvec::SmallVec;
use wasmtime::Memory;
use wasmtime::ValType;
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::EngineDataState;
use crate::engine::wasm_engine::host_helpers::get_u32_vec;
use crate::engine::wasm_engine::host_helpers::get_wasm_string;
use crate::interop::params::ObjectId;
use crate::interop::params::Param;
use crate::interop::params::Params;

use wasmtime::StoreContext;

use parking_lot::RwLock;

use wasmtime::Val;

use anyhow::Result;

use crate::interop::params::DataType;

macro_rules! dequeue {
    ($data:expr, $typ:tt :: $init:tt; $x:tt ) => {{
        let mut s = $data.write();

        let arr = array_from_iter::<$x>(s.f32_queue.drain(..$x));
        Param::$typ(glam::$typ::$init(arr))
    }};
}

macro_rules! dequeue_ref {
    ($data:expr, $typ:tt :: $init:tt; $x:tt ) => {{
        let mut s = $data.write();

        let arr = array_from_iter::<$x>(s.f32_queue.drain(..$x));
        Param::$typ(glam::$typ::$init(&arr))
    }};
}

pub(crate) fn array_from_iter<const N: usize>(iter: impl IntoIterator<Item = f32>) -> [f32; N] {
    let mut arr = [0.0; N];
    for (i, v) in iter.into_iter().take(N).enumerate() {
        arr[i] = v;
    }
    arr
}

impl DataType {
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
            | DataType::ExtMat4
            | DataType::ExtU32Buffer
            | DataType::RustU32Buffer => Ok(ValType::I32),

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
        caller: &StoreContext<WasiP1Ctx>,
    ) -> Self {
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

            (DataType::Vec2, _) => dequeue!(data, Vec2::from_array; 2),
            (DataType::Vec3, _) => dequeue!(data, Vec3::from_array; 3),
            (DataType::RustVec4 | DataType::ExtVec4, _) => dequeue!(data, Vec4::from_array; 4),
            (DataType::RustQuat | DataType::ExtQuat, _) => dequeue!(data, Quat::from_array; 4),
            (DataType::RustMat4 | DataType::ExtMat4, _) => {
                dequeue_ref!(data, Mat4::from_cols_array; 16)
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
