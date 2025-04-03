use std::any::Any;
use std::ffi::{c_char, c_void, CStr, CString};
use crate::data::game_objects::ColorNote;

unsafe trait CSharpConvertible {
    type Raw;
    fn into_cs(self) -> Self::Raw;
    unsafe fn from_cs(raw: Self::Raw) -> Self;
    fn as_ptr(&self) -> *const c_void;
}

unsafe impl CSharpConvertible for String {
    type Raw = *const c_char;

    fn into_cs(self) -> Self::Raw {
        CString::new(self).unwrap().into_raw()
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        CStr::from_ptr(raw).to_string_lossy().to_string()
    }

    fn as_ptr(&self) -> *const c_void {
        self.clone().into_cs() as *const c_void
    }

}

unsafe impl CSharpConvertible for f32 {
    type Raw = f32;

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }

    fn as_ptr(&self) -> *const c_void {
        Box::into_raw(Box::new(self.into_cs())) as *const c_void
    }

}

unsafe impl CSharpConvertible for i32 {
    type Raw = i32;

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }

    fn as_ptr(&self) -> *const c_void {
        Box::into_raw(Box::new(self.into_cs())) as *const c_void
    }

}

unsafe impl CSharpConvertible for bool {
    type Raw = bool;

    fn into_cs(self) -> Self::Raw {
        self
    }
    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }
    fn as_ptr(&self) -> *const c_void {
        Box::into_raw(Box::new(self.into_cs())) as *const c_void
    }
}

unsafe impl CSharpConvertible for () {
    type Raw = ();

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }

    fn as_ptr(&self) -> *const c_void {
        Box::into_raw(Box::new(self.into_cs())) as *const c_void
    }

}

unsafe impl CSharpConvertible for ColorNote {
    type Raw = ColorNote;

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }

    fn as_ptr(&self) -> *const c_void {
        Box::into_raw(Box::new(self.into_cs())) as *const c_void
    }

}

// unsafe impl CSharpConvertible for (f32, i32) {
//     type Raw = (f32, i32);
//
//     fn into_cs(self) -> Self::Raw {
//         self
//     }
//
//     unsafe fn from_cs(raw: Self::Raw) -> Self {
//         raw
//     }
//
//     fn as_ptr(&self) -> *const c_void {
//         Box::into_raw(Box::new(self.into_cs())) as *const c_void
//     }
//
// }


pub enum Param {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    ColorNote(ColorNote),
    // Tuple(Vec<Box<Param>>)
}


#[repr(C)]
pub struct CParam {
    id: *const c_char,
    data: *const c_void,
}

#[repr(C)]
pub struct CParams {
    param_count: u32,
    param_ptr_array_ptr: *mut *mut CParam,
}


impl CParam {
    pub fn new(id: *const c_char, data: *const c_void) -> Self {
        CParam { id, data }
    }
}

pub struct Parameters {
    params: Vec<Param>,
}

impl Parameters {
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
        }
    }

    pub fn push(&mut self, param: Param) {
        self.params.push(param);
    }

    fn get_id(param: &Param) -> *const c_char {
        let id_str = match param {
            Param::Int(_) => "int",
            Param::Float(_) => "float",
            Param::Bool(_) => "bool",
            Param::String(_) => "string",
            Param::ColorNote(_) => "ColorNote",
            // Param::Tuple(_) => "tuple",
        };
        CString::new(id_str).unwrap().into_raw()
    }

    fn get_ptr(param: &Param) -> *const c_void {
        match param {
            Param::Int(x) => x.as_ptr(),
            Param::Float(x) => x.as_ptr(),
            Param::Bool(x) => x.as_ptr(),
            Param::String(x) => x.as_ptr(),
            Param::ColorNote(x) => x.as_ptr(),
            // Param::Tuple(x) => {
            //
            // },
        }
    }

    pub fn pack(self) -> CParams {

        let mut params = Vec::new();

        for param in &self.params {
            let id = Self::get_id(param);
            let ptr = Self::get_ptr(param);


            params.push(Box::into_raw(Box::new(CParam::new(id, ptr))));

        }

        CParams {
            param_count: params.len() as u32,
            param_ptr_array_ptr: params.as_mut_ptr()
        }
    }

}

