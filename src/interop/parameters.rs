use std::any::Any;
use std::ffi::{c_char, c_void, CStr, CString};
use crate::data::game_objects::ColorNote;

unsafe trait CSharpConvertible {
    type Raw;
    fn into_cs(self) -> Self::Raw;
    unsafe fn from_cs(raw: Self::Raw) -> Self;
}

unsafe impl CSharpConvertible for String {
    type Raw = *const c_char;

    fn into_cs(self) -> Self::Raw {
        CString::new(self).unwrap().into_raw()
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        CStr::from_ptr(raw).to_string_lossy().to_string()
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
}

unsafe impl CSharpConvertible for i32 {
    type Raw = i32;

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
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
}

unsafe impl CSharpConvertible for ColorNote {
    type Raw = ColorNote;

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }
}

unsafe impl CSharpConvertible for (f32, i32) {
    type Raw = (f32, i32);

    fn into_cs(self) -> Self::Raw {
        self
    }

    unsafe fn from_cs(raw: Self::Raw) -> Self {
        raw
    }
}


#[repr(C)]
pub struct CParams {
    param_count: u32,
    param_ptr_array_ptr: *mut *const c_void
}

pub struct Parameters {
    params: Vec<Box<dyn CSharpConvertible<Raw=dyn Any>>>
}

impl Parameters {
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
        }
    }

    pub fn push(&mut self, param: Box<dyn CSharpConvertible<Raw=dyn Any>>) {
        self.params.push(param);
    }


    pub fn pack(&self) -> CParams {
        todo!()
    }

}

