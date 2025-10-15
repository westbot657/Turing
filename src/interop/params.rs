use std::ffi::{c_char, c_void, CStr, CString};
use std::mem;

use anyhow::{anyhow, Result};
use wasmtime::Val;

pub mod param_type {
    pub const I8: u32 = 1;
    pub const I16: u32 = 2;
    pub const I32: u32 = 3;
    pub const U8: u32 = 4;
    pub const U16: u32 = 5;
    pub const U32: u32 = 6;
    pub const F32: u32 = 7;
    pub const BOOL: u32 = 8;
    pub const STRING: u32 = 9;
    pub const OBJECT: u32 = 10;
    pub const ERROR: u32 = 11;
    pub const VOID: u32 = 12;
}

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
    void: u32
}

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
            Param::I8(x)     => FfiParam { type_id: 1,  value: RawParam { i8:     x } },
            Param::I16(x)    => FfiParam { type_id: 2,  value: RawParam { i16:    x } },
            Param::I32(x)    => FfiParam { type_id: 3,  value: RawParam { i32:    x } },
            Param::U8(x)     => FfiParam { type_id: 4,  value: RawParam { u8:     x } },
            Param::U16(x)    => FfiParam { type_id: 5,  value: RawParam { u16:    x } },
            Param::U32(x)    => FfiParam { type_id: 6,  value: RawParam { u32:    x } },
            Param::F32(x)    => FfiParam { type_id: 7,  value: RawParam { f32:    x } },
            Param::Bool(x)   => FfiParam { type_id: 8,  value: RawParam { bool:   x } },
            Param::String(x) => FfiParam { type_id: 9,  value: RawParam { string: CString::new(x).unwrap().into_raw() } },
            Param::Object(x) => FfiParam { type_id: 10, value: RawParam { object: x } },
            Param::Error(x)  => FfiParam { type_id: 11, value: RawParam { error:  CString::new(x).unwrap().into_raw() } },
            Param::Void      => FfiParam { type_id: 12, value: RawParam { void:   0 } },
        }
    }

    /// if self is an Error value, returns Err, else Ok(())
    /// If self is a String, it will free the raw pointer (unless null)
    pub fn to_result(self) -> Result<()> {
        match self {
            Param::Error(e) => Err(anyhow!(e)),
            _ => Ok(())
        }
    }

}

impl FfiParam {
    pub fn to_param(self) -> Result<Param> {
        Ok(match self.type_id {
            1  => Param::I8(     unsafe { self.value.i8     } ),
            2  => Param::I16(    unsafe { self.value.i16    } ),
            3  => Param::I32(    unsafe { self.value.i32    } ),
            4  => Param::U8(     unsafe { self.value.u8     } ),
            5  => Param::U16(    unsafe { self.value.u16    } ),
            6  => Param::U32(    unsafe { self.value.u32    } ),
            7  => Param::F32(    unsafe { self.value.f32    } ),
            8  => Param::Bool(   unsafe { self.value.bool   } ),
            9  => Param::String( unsafe { CStr::from_ptr(self.value.string).to_string_lossy().to_string() } ),
            10 => Param::Object( unsafe { self.value.object } ),
            11 => Param::Error(  unsafe { CStr::from_ptr(self.value.error).to_string_lossy().to_string() } ),
            12 => Param::Void,
            _  => return Err(anyhow!("Unknown type variant: {}", self.type_id))
        })
    }
}

impl From<Param> for FfiParam {
    fn from(value: Param) -> Self {
        value.to_ffi_param()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Params {
    params: Vec<Param>
}

impl Params {

    pub fn new() -> Self {
        Self {
            params: Vec::new()
        }
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



