use std::cell::RefMut;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Display;
use std::mem;

use anyhow::{Result, anyhow};
use wasmtime::{Memory, Store, Val};
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::{TuringState, get_string};

/// These ids must remain consistent on both sides of ffi.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamType {
    I8 = 1,
    I16 = 2,
    I32 = 3,
    U8 = 4,
    U16 = 5,
    U32 = 6,
    F32 = 7,
    BOOL = 8,
    STRING = 9,
    OBJECT = 10,
    ERROR = 11,
    VOID = 12,
}

impl Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ParamType::I8 => "I8",
            ParamType::I16 => "I16",
            ParamType::I32 => "I32",
            ParamType::U8 => "U8",
            ParamType::U16 => "U16",
            ParamType::U32 => "U32",
            ParamType::F32 => "F32",
            ParamType::BOOL => "BOOL",
            ParamType::STRING => "STRING",
            ParamType::OBJECT => "OBJECT",
            ParamType::ERROR => "ERROR",
            ParamType::VOID => "VOID",
        };
        write!(f, "{}", s)
    }
}

impl TryFrom<u32> for ParamType {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self> {
        // if 0, we return void too
        match value {
            1 => Ok(ParamType::I8),
            2 => Ok(ParamType::I16),
            3 => Ok(ParamType::I32),
            4 => Ok(ParamType::U8),
            5 => Ok(ParamType::U16),
            6 => Ok(ParamType::U32),
            7 => Ok(ParamType::F32),
            8 => Ok(ParamType::BOOL),
            9 => Ok(ParamType::STRING),
            10 => Ok(ParamType::OBJECT),
            11 => Ok(ParamType::ERROR),
            0 | 12 => Ok(ParamType::VOID),
            _ => Err(anyhow!("Unknown ParamType id: {}", value)),
        }
    }
}

impl ParamType {
    /// Checks if the ParamType is valid.
    pub fn is_valid(&self) -> bool {
        ParamType::try_from(*self as u32).is_ok()
    }
}

/// local repr of ffi data
/// FFI friendly enum for passing parameters to/from wasm functions
#[derive(Debug, Clone)]
pub enum Param {
    I8(i8),
    I16(i16),
    I32(i32),
    U8(u8),
    U16(u16),
    U32(u32),
    F32(f32),
    Bool(bool),
    String(String),
    Object(*const c_void),
    Error(String),
    Void,
}

/// C repr of ffi data
#[repr(C)]
pub union RawParam {
    i8: i8,
    i16: i16,
    i32: i32,
    u8: u8,
    u16: u16,
    u32: u32,
    f32: f32,
    bool: bool,
    string: *const c_char,
    object: *const c_void,
    error: *const c_char,
    void: u32,
}

/// C tagged repr of ffi data
#[repr(C)]
pub struct FfiParam {
    pub type_id: u32,
    pub value: RawParam,
}

#[repr(C)]
pub struct FfiParamArray {
    pub count: u32,
    pub ptr: *const c_void,
}

impl Param {
    pub fn to_ffi_param(self) -> FfiParam {
        match self {
            Param::I8(x) => FfiParam {
                type_id: 1,
                value: RawParam { i8: x },
            },
            Param::I16(x) => FfiParam {
                type_id: 2,
                value: RawParam { i16: x },
            },
            Param::I32(x) => FfiParam {
                type_id: 3,
                value: RawParam { i32: x },
            },
            Param::U8(x) => FfiParam {
                type_id: 4,
                value: RawParam { u8: x },
            },
            Param::U16(x) => FfiParam {
                type_id: 5,
                value: RawParam { u16: x },
            },
            Param::U32(x) => FfiParam {
                type_id: 6,
                value: RawParam { u32: x },
            },
            Param::F32(x) => FfiParam {
                type_id: 7,
                value: RawParam { f32: x },
            },
            Param::Bool(x) => FfiParam {
                type_id: 8,
                value: RawParam { bool: x },
            },
            Param::String(x) => FfiParam {
                type_id: 9,
                value: RawParam {
                    string: CString::new(x).unwrap().into_raw(),
                },
            },
            Param::Object(x) => FfiParam {
                type_id: 10,
                value: RawParam { object: x },
            },
            Param::Error(x) => FfiParam {
                type_id: 11,
                value: RawParam {
                    error: CString::new(x).unwrap().into_raw(),
                },
            },
            Param::Void => FfiParam {
                type_id: 12,
                value: RawParam { void: 0 },
            },
        }
    }

    /// If self is an Error value, returns Err, else Ok(())
    /// If self is a String, it will free the raw pointer (unless null)
    pub fn to_result(self) -> Result<()> {
        match self {
            Param::Error(e) => Err(anyhow!(e)),
            _ => Ok(()),
        }
    }

    /// Constructs a Param from a Wasmtime Val and type id.
    pub fn from_typval(
        typ: ParamType,
        val: Val,
        state: &RefMut<'_, TuringState>,
        memory: &Memory,
        caller: &Store<WasiP1Ctx>,
    ) -> Self {
        match typ {
            ParamType::I8 => Param::I8(val.unwrap_i32() as i8),
            ParamType::I16 => Param::I16(val.unwrap_i32() as i16),
            ParamType::I32 => Param::I32(val.unwrap_i32()),
            ParamType::U8 => Param::U8(val.unwrap_i32() as u8),
            ParamType::U16 => Param::U16(val.unwrap_i32() as u16),
            ParamType::U32 => Param::U32(val.unwrap_i32() as u32),
            ParamType::F32 => Param::F32(val.unwrap_f32()),
            ParamType::BOOL => Param::Bool(val.unwrap_i32() != 0),
            ParamType::STRING => {
                let ptr = val.unwrap_i32() as u32;
                let st = get_string(ptr, memory.data(caller));
                Param::String(st)
            }
            ParamType::OBJECT => {
                let op = val.unwrap_i32() as u32;
                let real = state
                    .opaque_pointers
                    .get(&op)
                    .copied()
                    .unwrap_or(std::ptr::null::<c_void>());
                Param::Object(real)
            }
            ParamType::ERROR => {
                let ptr = val.unwrap_i32() as u32;
                let st = get_string(ptr, memory.data(caller));
                Param::Error(st)
            }
            ParamType::VOID => Param::Void,
            _ => unreachable!(
                "this is only called after type validation has been done on the type id"
            ),
        }
    }
}

impl FfiParam {
    pub fn to_param(self) -> Result<Param> {
        Ok(match self.type_id {
            1 => Param::I8(unsafe { self.value.i8 }),
            2 => Param::I16(unsafe { self.value.i16 }),
            3 => Param::I32(unsafe { self.value.i32 }),
            4 => Param::U8(unsafe { self.value.u8 }),
            5 => Param::U16(unsafe { self.value.u16 }),
            6 => Param::U32(unsafe { self.value.u32 }),
            7 => Param::F32(unsafe { self.value.f32 }),
            8 => Param::Bool(unsafe { self.value.bool }),
            9 => Param::String(unsafe {
                CStr::from_ptr(self.value.string)
                    .to_string_lossy()
                    .to_string()
            }),
            10 => Param::Object(unsafe { self.value.object }),
            11 => Param::Error(unsafe {
                CStr::from_ptr(self.value.error)
                    .to_string_lossy()
                    .to_string()
            }),
            12 => Param::Void,
            _ => return Err(anyhow!("Unknown type variant: {}", self.type_id)),
        })
    }
}

impl From<Param> for FfiParam {
    fn from(value: Param) -> Self {
        value.to_ffi_param()
    }
}

/// A collection of parameters to be passed to a wasm function.
#[derive(Debug, Clone, Default)]
pub struct Params {
    params: Vec<Param>,
}

impl Params {
    pub fn new() -> Self {
        Self { params: Vec::new() }
    }

    pub fn of_size(size: u32) -> Self {
        Self {
            params: Vec::with_capacity(size as usize),
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
    pub fn to_args(self, state: &mut RefMut<'_, TuringState>) -> Vec<Val> {
        let mut vals = Vec::new();

        for p in self.params {
            vals.push(match p {
                Param::I8(i) => Val::I32(i as i32),
                Param::I16(i) => Val::I32(i as i32),
                Param::I32(i) => Val::I32(i),
                Param::U8(u) => Val::I32(u as i32),
                Param::U16(u) => Val::I32(u as i32),
                Param::U32(u) => Val::I32(u as i32),
                Param::F32(f) => Val::F32(f.to_bits()),
                Param::Bool(b) => Val::I32(if b { 1 } else { 0 }),
                Param::String(st) => {
                    let l = st.len() + 1;
                    state.str_cache.push_back(st);
                    Val::I32(l as i32)
                }
                Param::Object(rp) => match state.pointer_backlink.get(&rp) {
                    Some(op) => Val::I32(*op as i32),
                    None => {
                        let op = state.opaque_pointers.add(rp);
                        state.pointer_backlink.insert(rp, op);
                        Val::I32(op as i32)
                    }
                },
                Param::Error(st) => {
                    let l = st.len() + 1;
                    state.str_cache.push_back(st);
                    Val::I32(l as i32)
                }
                _ => unreachable!("Void shouldn't ever be added as an arg"),
            })
        }

        vals
    }
}

impl From<Vec<Param>> for FfiParamArray {
    fn from(vec: Vec<Param>) -> Self {
        if vec.is_empty() {
            return FfiParamArray {
                count: 0,
                ptr: std::ptr::null(),
            };
        }

        let ffi_params: Vec<FfiParam> = vec.into_iter().map(Into::into).collect();

        let count = ffi_params.len() as u32;
        let ptr = ffi_params.as_ptr() as *const c_void;

        mem::forget(ffi_params);

        FfiParamArray { count, ptr }
    }
}

impl TryFrom<FfiParamArray> for Vec<Param> {
    type Error = anyhow::Error;

    fn try_from(array: FfiParamArray) -> Result<Self> {
        if array.ptr.is_null() || array.count == 0 {
            return Ok(Vec::new());
        }

        unsafe {
            let raw_vec = Vec::from_raw_parts(
                array.ptr as *mut FfiParam,
                array.count as usize,
                array.count as usize,
            );

            let mut result = Vec::with_capacity(raw_vec.len());
            for ffi_param in raw_vec {
                result.push(ffi_param.to_param()?);
            }

            Ok(result)
        }
    }
}
