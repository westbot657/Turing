use std::{
    ffi::{CStr, c_char},
    ops::Deref,
};

use crate::ffi::Ext;

/// A string allocated externally, to be managed by the external environment.
pub struct ExtString {
    pub ptr: *const c_char,
}

impl ExtString {
    pub fn new(ptr: *const c_char) -> Self {
        ExtString { ptr }
    }
}

impl Drop for ExtString {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            Ext::free_string(self.ptr);
        }
    }
}

impl From<&CStr> for ExtString {
    fn from(s: &CStr) -> Self {
        ExtString {
            ptr: s.as_ptr() as *mut c_char,
        }
    }
}

impl From<*const c_char> for ExtString {
    fn from(ptr: *const c_char) -> Self {
        ExtString {
            ptr: ptr as *mut c_char,
        }
    }
}

impl Deref for ExtString {
    type Target = CStr;

    fn deref(&self) -> &Self::Target {
        unsafe { CStr::from_ptr(self.ptr) }
    }
}

impl std::fmt::Display for ExtString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.deref().to_string_lossy())
    }
}
