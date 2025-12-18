use crate::interop::types::ExtString;
use crate::{EngineDataState, ExternalFunctions, OpaquePointerKey};
use anyhow::{Result, anyhow};
use num_enum::TryFromPrimitive;
use parking_lot::RwLock;
use slotmap::KeyData;
use smallvec::SmallVec;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Display;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, TryFromPrimitive)]
pub enum DataType {
    I8 = 1,
    I16 = 2,
    I32 = 3,
    I64 = 4,
    U8 = 5,
    U16 = 6,
    U32 = 7,
    U64 = 8,
    F32 = 9,
    F64 = 10,
    Bool = 11,
    /// allocated via CString, must be freed via CString::from_raw
    RustString = 12,
    /// allocated externally, handled via Cs::free_string
    ExtString = 13,
    /// Represents an object ID, which is mapped by the pointer backlink system.
    Object = 14,
    // Allocated via CString, must be freed via CString::from_raw
    RustError = 15,
    // Allocated externally, handled via Cs::free_string
    ExtError = 16,
    Void = 17,
}
impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DataType::I8 => "I8",
            DataType::I16 => "I16",
            DataType::I32 => "I32",
            DataType::I64 => "I64",
            DataType::U8 => "U8",
            DataType::U16 => "U16",
            DataType::U32 => "U32",
            DataType::U64 => "U64",
            DataType::F32 => "F32",
            DataType::F64 => "F64",
            DataType::Bool => "BOOL",
            DataType::RustString => "RUST_STRING",
            DataType::ExtString => "EXT_STRING",
            DataType::Object => "OBJECT",
            DataType::RustError => "RUST_ERROR",
            DataType::ExtError => "EXT_ERROR",
            DataType::Void => "VOID",
        };
        write!(f, "{}", s)
    }
}

impl DataType {
    /// Checks if the ParamType is valid.
    pub fn is_valid(&self) -> bool {
        DataType::try_from(*self as u32).is_ok()
    }

    pub fn is_valid_param_type(&self) -> bool {
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
                | DataType::RustString
                | DataType::ExtString
                | DataType::Object
        )
    }

    pub fn is_valid_return_type(&self) -> bool {
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
                | DataType::RustString
                | DataType::ExtString
                | DataType::Object
                | DataType::Void
        )
    }

    #[cfg(feature = "wasm")]
    pub fn to_val_type(&self) -> Result<wasmtime::ValType> {
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
            | DataType::Object => Ok(wasmtime::ValType::I32),

            DataType::I64 | DataType::U64 => Ok(wasmtime::ValType::I64),

            DataType::F32 => Ok(wasmtime::ValType::F32),
            DataType::F64 => Ok(wasmtime::ValType::F64),

            _ => Err(anyhow!("Invalid wasm value type: {}", self)),
        }
    }

    #[cfg(feature = "wasm")]
    pub fn to_wasm_val_param(
        &self,
        val: &wasmtime::Val,
        caller: &mut wasmtime::Caller<'_, wasmtime_wasi::p1::WasiP1Ctx>,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<Param> {
        use crate::engine::wasm_engine::get_wasm_string;
        use wasmtime::Val;

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
                let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(*pointer_id as u64));

                if let Some(true_pointer) = data.read().opaque_pointers.get(pointer_key) {
                    Ok(Param::Object(**true_pointer))
                } else {
                    Err(anyhow!(
                        "opaque pointer does not correspond to a real pointer"
                    ))
                }
            }
            _ => Err(anyhow!("Mismatched parameter type")),
        }
    }

    #[cfg(feature = "lua")]
    pub fn to_lua_val_param(
        &self,
        val: &mlua::Value,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> mlua::Result<Param> {
        match (self, val) {
            (DataType::I8, mlua::Value::Integer(i)) => Ok(Param::I8(*i as i8)),
            (DataType::I16, mlua::Value::Integer(i)) => Ok(Param::I16(*i as i16)),
            (DataType::I32, mlua::Value::Integer(i)) => Ok(Param::I32(*i as i32)),
            (DataType::I64, mlua::Value::Integer(i)) => Ok(Param::I64(*i)),
            (DataType::U8, mlua::Value::Integer(u)) => Ok(Param::U8(*u as u8)),
            (DataType::U16, mlua::Value::Integer(u)) => Ok(Param::U16(*u as u16)),
            (DataType::U32, mlua::Value::Integer(u)) => Ok(Param::U32(*u as u32)),
            (DataType::U64, mlua::Value::Integer(u)) => Ok(Param::U64(*u as u64)),
            (DataType::F32, mlua::Value::Number(f)) => Ok(Param::F32(*f as f32)),
            (DataType::F64, mlua::Value::Number(f)) => Ok(Param::F64(*f)),
            (DataType::Bool, mlua::Value::Boolean(b)) => Ok(Param::Bool(*b)),
            (DataType::RustString | DataType::ExtString, mlua::Value::String(s)) => {
                Ok(Param::String(s.to_string_lossy()))
            }
            (DataType::Object, mlua::Value::Table(t)) => {
                let key = t.raw_get::<mlua::Value>("opaqu")?;
                let key = match key {
                    mlua::Value::Integer(i) => i as u64,
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "Incorrect type for opaque handle".to_string(),
                        ));
                    }
                };
                let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(key));
                if let Some(true_pointer) = data.read().opaque_pointers.get(pointer_key) {
                    Ok(Param::Object(**true_pointer))
                } else {
                    Err(mlua::Error::RuntimeError(
                        "opaque pointer does not correspond to a real pointer".to_string(),
                    ))
                }
            }
            _ => Err(mlua::Error::RuntimeError(format!(
                "Mismatched parameter type: {self} with {val:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    Object(*const c_void),
    Error(String),
    Void,
}

impl Param {
    /// Constructs a Param from a Wasmtime Val and type id.
    #[cfg(feature = "wasm")]
    pub fn from_wasm_type_val(
        typ: DataType,
        val: wasmtime::Val,
        data: &Arc<RwLock<EngineDataState>>,
        memory: &wasmtime::Memory,
        caller: &wasmtime::Store<wasmtime_wasi::p1::WasiP1Ctx>,
    ) -> Self {
        use crate::engine::wasm_engine::get_wasm_string;

        match typ {
            DataType::I8 => Param::I8(val.unwrap_i32() as i8),
            DataType::I16 => Param::I16(val.unwrap_i32() as i16),
            DataType::I32 => Param::I32(val.unwrap_i32()),
            DataType::I64 => Param::I64(val.unwrap_i64()),
            DataType::U8 => Param::U8(val.unwrap_i32() as u8),
            DataType::U16 => Param::U16(val.unwrap_i32() as u16),
            DataType::U32 => Param::U32(val.unwrap_i32() as u32),
            DataType::U64 => Param::U64(val.unwrap_i64() as u64),
            DataType::F32 => Param::F32(val.unwrap_f32()),
            DataType::F64 => Param::F64(val.unwrap_f64()),
            DataType::Bool => Param::Bool(val.unwrap_i32() != 0),
            // allocated externally, we copy the string
            DataType::ExtString => {
                let ptr = val.unwrap_i32() as u32;
                let st = get_wasm_string(ptr, memory.data(caller));
                Param::String(st)
            }
            DataType::RustString => unreachable!("RustString should not be used in from_typval"),
            DataType::Object => {
                let op = val.unwrap_i64() as u64;
                let key = OpaquePointerKey::from(KeyData::from_ffi(op));

                let real = data
                    .read()
                    .opaque_pointers
                    .get(key)
                    .copied()
                    .unwrap_or_default();
                Param::Object(real.ptr)
            }
            DataType::ExtError => {
                let ptr = val.unwrap_i32() as u32;
                let st = get_wasm_string(ptr, memory.data(caller));
                Param::Error(st)
            }
            DataType::RustError => unreachable!("RustError should not be used in from_typval"),
            DataType::Void => Param::Void,
        }
    }

    #[cfg(feature = "lua")]
    pub fn from_lua_type_val(
        typ: DataType,
        val: mlua::Value,
        data: &Arc<RwLock<EngineDataState>>,
        lua: &mlua::Lua,
    ) -> Self {
        match typ {
            DataType::I8 => Param::I8(val.as_integer().unwrap() as i8),
            DataType::I16 => Param::I16(val.as_integer().unwrap() as i16),
            DataType::I32 => Param::I32(val.as_integer().unwrap() as i32),
            DataType::I64 => Param::I64(val.as_integer().unwrap()),
            DataType::U8 => Param::U8(val.as_integer().unwrap() as u8),
            DataType::U16 => Param::U16(val.as_integer().unwrap() as u16),
            DataType::U32 => Param::U32(val.as_integer().unwrap() as u32),
            DataType::U64 => Param::U64(val.as_integer().unwrap() as u64),
            DataType::F32 => Param::F32(val.as_number().unwrap() as f32),
            DataType::F64 => Param::F64(val.as_number().unwrap()),
            DataType::Bool => Param::Bool(val.as_boolean().unwrap()),
            // allocated externally, we copy the string
            DataType::ExtString => Param::String(val.as_string().unwrap().to_string_lossy()),
            DataType::RustString => unreachable!("RustString should not be used in from_typval"),
            DataType::Object => {
                let table = val.as_table().unwrap();
                let op = table.get("opaqu").unwrap();
                let key = OpaquePointerKey::from(KeyData::from_ffi(op));

                let real = data
                    .read()
                    .opaque_pointers
                    .get(key)
                    .copied()
                    .unwrap_or_default();
                Param::Object(real.ptr)
            }
            DataType::ExtError => Param::Error(val.as_error().unwrap().to_string()),
            DataType::RustError => unreachable!("RustError should not be used in from_typval"),
            DataType::Void => Param::Void,
        }
    }

    pub fn to_rs_param(self) -> FfiParam {
        self.to_param_inner(DataType::RustString, DataType::RustError)
    }
    pub fn to_ext_param(self) -> FfiParam {
        self.to_param_inner(DataType::ExtString, DataType::ExtError)
    }

    pub fn to_serde(
        self,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<serde_json::Value, anyhow::Error> {
        Ok(match self {
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
            Param::Error(e) => return Err(anyhow!("{}", e)),
        })
    }

    pub fn from_serde(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Param::I64(i)
                } else if let Some(u) = n.as_u64() {
                    Param::U64(u)
                } else if let Some(f) = n.as_f64() {
                    Param::F64(f)
                } else {
                    Param::Error("Invalid number".to_string())
                }
            }
            serde_json::Value::String(s) => Param::String(s),
            serde_json::Value::Bool(b) => Param::Bool(b),
            serde_json::Value::Null => Param::Void,
            _ => Param::Error("Unsupported return type".to_string()),
        }
    }


    #[rustfmt::skip]
    fn to_param_inner(self, str_type: DataType, err_type: DataType) -> FfiParam {
        match self {
            Param::I8(x) => FfiParam { type_id: DataType::I8, value: RawParam { i8: x } },
            Param::I16(x) => FfiParam { type_id: DataType::I16, value: RawParam { i16: x } },
            Param::I32(x) => FfiParam { type_id: DataType::I32, value: RawParam { i32: x } },
            Param::I64(x) => FfiParam { type_id: DataType::I64, value: RawParam { i64: x } },
            Param::U8(x) => FfiParam { type_id: DataType::U8, value: RawParam { u8: x } },
            Param::U16(x) => FfiParam { type_id: DataType::U16, value: RawParam { u16: x } },
            Param::U32(x) => FfiParam { type_id: DataType::U32, value: RawParam { u32: x } },
            Param::U64(x) => FfiParam { type_id: DataType::U64, value: RawParam { u64: x } },
            Param::F32(x) => FfiParam { type_id: DataType::F32, value: RawParam { f32: x } },
            Param::F64(x) => FfiParam { type_id: DataType::F64, value: RawParam { f64: x } },
            Param::Bool(x) => FfiParam { type_id: DataType::Bool, value: RawParam { bool: x } },
            // allocated via CString, must be freed via CString::from_raw
            Param::String(x) => FfiParam { type_id: str_type, value: RawParam { string: CString::new(x).unwrap().into_raw() } },
            Param::Object(x) => FfiParam { type_id: DataType::Object, value: RawParam { object: x } },
            Param::Error(x) => FfiParam { type_id: err_type, value: RawParam { error: CString::new(x).unwrap().into_raw() } },
            Param::Void => FfiParam { type_id: DataType::Void, value: RawParam { void: () } },
        }
    }

    pub fn to_result<T: FromParam>(self) -> Result<T> {
        T::from_param(self)
    }
}

pub trait FromParam: Sized {
    fn from_param(param: Param) -> Result<Self>;
}
macro_rules! deref_param {
    ( $param:expr, $case:tt ) => {
        match $param {
            Param::$case(v) => Ok(v),
            Param::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Incorrect data type")),
        }
    };
    ( $tp:ty => $case:tt ) => {
        impl FromParam for $tp {
            fn from_param(param: Param) -> Result<Self> {
                deref_param!(param, $case)
            }
        }
    };
}
deref_param! { i8     => I8     }
deref_param! { i16    => I16    }
deref_param! { i32    => I32    }
deref_param! { i64    => I64    }
deref_param! { u8     => U8     }
deref_param! { u16    => U16    }
deref_param! { u32    => U32    }
deref_param! { u64    => U64    }
deref_param! { f32    => F32    }
deref_param! { f64    => F64    }
deref_param! { bool   => Bool   }
deref_param! { String => String }
impl FromParam for () {
    fn from_param(param: Param) -> Result<Self> {
        match param {
            Param::Void => Ok(()),
            Param::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Incorrect data type")),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Params {
    // SmallVec will spill onto the heap if there are more than 4 params
    params: SmallVec<[Param; 4]>,
}

impl Params {
    pub fn new() -> Self {
        Self {
            params: Default::default(),
        }
    }

    pub fn of_size(size: u32) -> Self {
        Self {
            params: SmallVec::with_capacity(size as usize),
        }
    }

    pub fn push(&mut self, param: Param) {
        self.params.push(param);
    }

    pub fn set(&mut self, index: u32, param: Param) {
        self.params[index as usize] = param;
    }

    pub fn get(&self, idx: usize) -> Option<&Param> {
        self.params.get(idx)
    }

    pub fn len(&self) -> u32 {
        self.params.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// Converts the Params into a vector of Wasmtime Val types for function calling.
    #[cfg(feature = "wasm")]
    pub fn to_wasm_args(
        self,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<SmallVec<[wasmtime::Val; 4]>> {
        // Acquire a single write lock for the duration of conversion to avoid
        // repeated locking/unlocking when pushing strings or registering objects.

        use wasmtime::Val;
        let mut s = data.write();

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
                Param::Object(rp) => {
                    let pointer = rp.into();
                    Ok(if let Some(op) = s.pointer_backlink.get(&pointer) {
                        Val::I64(op.0.as_ffi() as i64)
                    } else {
                        let op = s.opaque_pointers.insert(pointer);
                        s.pointer_backlink.insert(pointer, op);
                        Val::I64(op.0.as_ffi() as i64)
                    })
                }
                Param::Error(st) => Err(anyhow!("{st}")),
                _ => unreachable!("Void shouldn't ever be added as an arg"),
            })
            .collect()
    }

    #[cfg(feature = "lua")]
    pub fn to_lua_args(
        self,
        lua: &mlua::Lua,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<mlua::MultiValue> {
        let mut s = data.write();
        let vals = self
            .params
            .into_iter()
            .map(|p| match p {
                Param::I8(i) => Ok(mlua::Value::Integer(i as i64)),
                Param::I16(i) => Ok(mlua::Value::Integer(i as i64)),
                Param::I32(i) => Ok(mlua::Value::Integer(i as i64)),
                Param::I64(i) => Ok(mlua::Value::Integer(i)),
                Param::U8(u) => Ok(mlua::Value::Integer(u as i64)),
                Param::U16(u) => Ok(mlua::Value::Integer(u as i64)),
                Param::U32(u) => Ok(mlua::Value::Integer(u as i64)),
                Param::U64(u) => Ok(mlua::Value::Integer(u as i64)),
                Param::F32(f) => Ok(mlua::Value::Number(f as f64)),
                Param::F64(f) => Ok(mlua::Value::Number(f)),
                Param::Bool(b) => Ok(mlua::Value::Boolean(b)),
                Param::String(s) => Ok(mlua::Value::String(lua.create_string(&s).unwrap())),
                Param::Object(rp) => {
                    let pointer = rp.into();
                    Ok(if let Some(op) = s.pointer_backlink.get(&pointer) {
                        mlua::Value::Integer(op.0.as_ffi() as i64)
                    } else {
                        let op = s.opaque_pointers.insert(pointer);
                        s.pointer_backlink.insert(pointer, op);
                        mlua::Value::Integer(op.0.as_ffi() as i64)
                    })
                }
                Param::Error(st) => Err(anyhow!("{st}")),
                _ => unreachable!("Void shouldn't ever be added as an arg"),
            })
            .collect::<Result<Vec<mlua::Value>>>()?;

        Ok(mlua::MultiValue::from_vec(vals))
    }

    pub fn to_ffi<Ext>(self) -> FfiParams<Ext>
    where
        Ext: ExternalFunctions,
    {
        FfiParams::from_params(self.params)
    }
}

impl Deref for Params {
    type Target = SmallVec<[Param; 4]>;

    fn deref(&self) -> &Self::Target {
        &self.params
    }
}

impl DerefMut for Params {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.params
    }
}

impl IntoIterator for Params {
    type Item = Param;
    type IntoIter = smallvec::IntoIter<[Param; 4]>;

    fn into_iter(self) -> Self::IntoIter {
        self.params.into_iter()
    }
}

/// C repr of ffi data
#[repr(C)]
pub union RawParam {
    i8: i8,
    i16: i16,
    i32: i32,
    i64: i64,
    u8: u8,
    u16: u16,
    u32: u32,
    u64: u64,
    f32: f32,
    f64: f64,
    bool: bool,
    // represented by either RustString or ExtString
    string: *const c_char,
    object: *const c_void,
    error: *const c_char,
    void: (),
}

/// C tagged repr of ffi data
#[repr(C)]
pub struct FfiParam {
    pub type_id: DataType,
    pub value: RawParam,
}

/// A collection of FfiParams.
/// Can be converted to/from Params.
/// Will free allocated resources on drop.
pub struct FfiParams<Ext: ExternalFunctions> {
    pub params: SmallVec<[FfiParam; 4]>,
    marker: PhantomData<Ext>,
}

impl<Ext> Drop for FfiParams<Ext>
where
    Ext: ExternalFunctions,
{
    fn drop(&mut self) {
        if self.params.is_empty() {
            return;
        }

        // Convert the inner params without moving fields out of `self`
        let params: Result<_> = mem::take(&mut self.params)
            .into_iter()
            .map(|p| p.into_param::<Ext>())
            .collect();

        if let Ok(params) = params {
            // drop the converted Params so any allocated resources are freed
            drop(Params { params });
        }
    }
}

impl<Ext> Default for FfiParams<Ext>
where
    Ext: ExternalFunctions,
{
    fn default() -> Self {
        Self::empty()
    }
}

impl<Ext> FfiParams<Ext>
where
    Ext: ExternalFunctions,
{
    pub fn empty() -> Self {
        Self {
            params: SmallVec::new(),
            marker: PhantomData,
        }
    }

    /// Creates FfiParams from a vector of Params.
    pub fn from_params<T>(params: T) -> Self
    where
        T: IntoIterator<Item = Param>,
    {
        let ffi_params = params.into_iter().map(|p| p.to_rs_param()).collect();
        Self {
            params: ffi_params,
            marker: PhantomData,
        }
    }

    /// Creates FfiParams from an FfiParamArray with 'static lifetime.
    pub fn from_ffi_array(array: FfiParamArray<'static>) -> Result<Self> {
        if array.ptr.is_null() || array.count == 0 {
            return Ok(Self::default());
        }
        unsafe {
            let raw_vec = std::ptr::slice_from_raw_parts_mut(
                array.ptr as *mut FfiParam,
                array.count as usize,
            );
            let raw_vec = Box::from_raw(raw_vec);

            // take ownership of the raw_vec
            let owned = raw_vec.into_vec();

            Ok(Self {
                params: SmallVec::from_vec(owned),
                marker: PhantomData,
            })
        }
    }

    /// Converts FfiParams back into Params.
    pub fn to_params(mut self) -> Result<Params> {
        // take the inner SmallVec to avoid moving a field out of a Drop type
        let params: Result<_> = mem::take(&mut self.params)
            .into_iter()
            .map(|p| p.into_param::<Ext>())
            .collect();
        Ok(Params { params: params? })
    }

    /// Creates an FfiParamArray from the FfiParams.
    pub fn as_ffi_array<'a>(&'a self) -> FfiParamArray<'a> {
        FfiParamArray::<'a> {
            count: self.params.len() as u32,
            ptr: self.params.as_ptr(),
            marker: PhantomData,
        }
    }

    /// Leaks the FfiParams into an FfiParamArray with 'static lifetime.
    /// Caller is responsible for freeing the memory.
    /// Freeing is possible by converting back via FfiParams::from_ffi_array and dropping the FfiParams.
    pub fn leak(mut self) -> FfiParamArray<'static> {
        let boxed_slice = mem::take(&mut self.params).into_boxed_slice();
        let count = boxed_slice.len() as u32;
        let ptr = Box::into_raw(boxed_slice) as *const FfiParam;

        FfiParamArray {
            count,
            ptr,
            marker: PhantomData,
        }
    }
}

/// C repr of an array of FfiParams.
/// Does not own the memory, just a view.
/// Can be converted to Params.
#[repr(C)]
#[derive(Clone)]
pub struct FfiParamArray<'a> {
    pub count: u32,
    pub ptr: *const FfiParam,
    pub marker: PhantomData<&'a ()>,
}

impl<'a> FfiParamArray<'a> {
    /// Creates an empty FfiParamArray.
    pub fn empty() -> Self {
        Self {
            count: 0,
            ptr: std::ptr::null(),
            marker: PhantomData,
        }
    }

    /// Clones the parameters from the FfiParamArray without taking ownership.
    /// Does not free any memory.
    pub fn as_params<Ext: ExternalFunctions>(&'a self) -> Result<Params> {
        if self.ptr.is_null() || self.count == 0 {
            return Ok(Params::default());
        }

        unsafe {
            let raw_slice =
                std::ptr::slice_from_raw_parts(self.ptr as *mut FfiParam, self.count as usize);
            let slice = &*raw_slice;

            let result = slice
                .iter()
                .map(|p| p.as_param::<Ext>())
                .collect::<Result<_>>()?;
            Ok(Params { params: result })
        }
    }

    pub fn as_slice(&'a self) -> &'a [FfiParam] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.count as usize) }
    }
}

impl FfiParam {
    pub fn into_param<Ext: ExternalFunctions>(self) -> Result<Param> {
        Ok(match self.type_id {
            DataType::I8 => Param::I8(unsafe { self.value.i8 }),
            DataType::I16 => Param::I16(unsafe { self.value.i16 }),
            DataType::I32 => Param::I32(unsafe { self.value.i32 }),
            DataType::I64 => Param::I64(unsafe { self.value.i64 }),
            DataType::U8 => Param::U8(unsafe { self.value.u8 }),
            DataType::U16 => Param::U16(unsafe { self.value.u16 }),
            DataType::U32 => Param::U32(unsafe { self.value.u32 }),
            DataType::U64 => Param::U64(unsafe { self.value.u64 }),
            DataType::F32 => Param::F32(unsafe { self.value.f32 }),
            DataType::F64 => Param::F64(unsafe { self.value.f64 }),
            DataType::Bool => Param::Bool(unsafe { self.value.bool }),
            DataType::RustString => Param::String(unsafe {
                CString::from_raw(self.value.string as *mut c_char)
                    .to_string_lossy()
                    .into_owned()
            }),
            DataType::ExtString => {
                Param::String(unsafe { ExtString::<Ext>::from(self.value.string).to_string() })
            }
            DataType::Object => Param::Object(unsafe { self.value.object }),
            DataType::RustError => Param::Error(unsafe {
                CString::from_raw(self.value.error as *mut c_char)
                    .to_string_lossy()
                    .into_owned()
            }),
            DataType::ExtError => {
                Param::Error(unsafe { ExtString::<Ext>::from(self.value.error).to_string() })
            }
            DataType::Void => Param::Void,
        })
    }

    pub fn as_param<Ext: ExternalFunctions>(&self) -> Result<Param> {
        Ok(match self.type_id {
            DataType::I8 => Param::I8(unsafe { self.value.i8 }),
            DataType::I16 => Param::I16(unsafe { self.value.i16 }),
            DataType::I32 => Param::I32(unsafe { self.value.i32 }),
            DataType::I64 => Param::I64(unsafe { self.value.i64 }),
            DataType::U8 => Param::U8(unsafe { self.value.u8 }),
            DataType::U16 => Param::U16(unsafe { self.value.u16 }),
            DataType::U32 => Param::U32(unsafe { self.value.u32 }),
            DataType::U64 => Param::U64(unsafe { self.value.u64 }),
            DataType::F32 => Param::F32(unsafe { self.value.f32 }),
            DataType::F64 => Param::F64(unsafe { self.value.f64 }),
            DataType::Bool => Param::Bool(unsafe { self.value.bool }),
            DataType::RustString => Param::String(unsafe {
                CStr::from_ptr(self.value.string)
                    .to_string_lossy()
                    .into_owned()
            }),
            DataType::ExtString => {
                Param::String(unsafe { ExtString::<Ext>::from(self.value.string).to_string() })
            }
            DataType::Object => Param::Object(unsafe { self.value.object }),
            DataType::RustError => Param::Error(unsafe {
                CStr::from_ptr(self.value.error)
                    .to_string_lossy()
                    .into_owned()
            }),
            DataType::ExtError => {
                Param::Error(unsafe { ExtString::<Ext>::from(self.value.error).to_string() })
            }
            DataType::Void => Param::Void,
        })
    }
}

impl From<Param> for FfiParam {
    fn from(value: Param) -> Self {
        value.to_rs_param()
    }
}
