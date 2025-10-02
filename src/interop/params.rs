use std::ffi::{c_char, c_void};

use anyhow::{anyhow, Result};

#[derive(Debug, Clone, Copy)]
pub enum Param {
    I8(i8),
    I16(i16),
    I32(i32),
    U8(u8),
    U16(u16),
    U32(u32),
    F32(f32),
    Bool(bool),
    String(*const c_char),
    Object(*const c_void),
    Error(*const c_char),
    Void,
}

#[repr(C)]
pub union RawParam {
    I8: i8,
    I16: i16,
    I32: i32,
    U8: u8,
    U16: u16,
    U32: u32,
    F32: f32,
    Bool: bool,
    String: *const c_char,
    Object: *const c_void,
    Error: *const c_char,
    Void: u32
}

#[repr(C)]
pub struct FfiParam {
    pub type_id: u32,
    pub value: RawParam,
}

impl Param {
    pub fn to_ffi_param(self) -> FfiParam {
        match self {
            Param::I8(x) => FfiParam { type_id: 1, value: RawParam { I8: x } },
            Param::I16(x) => FfiParam { type_id: 2, value: RawParam { I16: x } },
            Param::I32(x) => FfiParam { type_id: 3, value: RawParam { I32: x } },
            Param::U8(x) => FfiParam { type_id: 4, value: RawParam { U8: x } },
            Param::U16(x) => FfiParam { type_id: 5, value: RawParam { U16: x } },
            Param::U32(x) => FfiParam { type_id: 6, value: RawParam { U32: x } },
            Param::F32(x) => FfiParam { type_id: 7, value: RawParam { F32: x } },
            Param::Bool(x) => FfiParam { type_id: 8, value: RawParam { Bool: x } },
            Param::String(x) => FfiParam { type_id: 9, value: RawParam { String: x } },
            Param::Object(x) => FfiParam { type_id: 10, value: RawParam { Object: x } },
            Param::Error(x) => FfiParam { type_id: 11, value: RawParam { Error: x } },
            Param::Void => FfiParam { type_id: 12, value: RawParam { Void: 0 } },
        }
    }
}

impl FfiParam {
    pub fn to_param(self) -> Result<Param> {
        Ok(match self.type_id {
            1 => Param::I8( unsafe { self.value.I8 } ),
            2 => Param::I16( unsafe { self.value.I16 } ),
            3 => Param::I32( unsafe { self.value.I32 } ),
            4 => Param::U8( unsafe { self.value.U8 } ),
            5 => Param::U16( unsafe { self.value.U16 } ),
            6 => Param::U32( unsafe { self.value.U32 } ),
            7 => Param::F32( unsafe { self.value.F32 } ),
            8 => Param::Bool( unsafe { self.value.Bool } ),
            9 => Param::String( unsafe { self.value.String } ),
            10 => Param::Object( unsafe { self.value.Object } ),
            11 => Param::Error( unsafe { self.value.Error } ),
            12 => Param::Void,
            _ => return Err(anyhow!("Unknown type variant: {}", self.type_id))
        })
    }
}

impl From<Param> for FfiParam {
    fn from(value: Param) -> Self {
        value.to_ffi_param()
    }
}


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

    pub fn get(&self, idx: usize) -> Option<Param> {
        self.params.get(idx).copied()
    }
}


