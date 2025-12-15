use std::ffi::{c_char, c_void, CStr};
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr;
use crate::ExternalFunctions;

/// A string allocated externally, to be managed by the external environment.
pub struct ExtString<Ext: ExternalFunctions> {
    pub ptr: *const c_char,
    _ext: PhantomData<Ext>
}

impl<Ext: ExternalFunctions> ExtString<Ext> {
    pub fn new(ptr: *const c_char) -> Self {
        ExtString { ptr, _ext: PhantomData::default() }
    }
}

impl<Ext: ExternalFunctions> Drop for ExtString<Ext> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            Ext::free_string(self.ptr);
        }
    }
}

impl<Ext: ExternalFunctions> From<&CStr> for ExtString<Ext> {
    fn from(s: &CStr) -> Self {
        ExtString {
            ptr: s.as_ptr() as *mut c_char,
            _ext: PhantomData::default()
        }
    }
}

impl<Ext: ExternalFunctions> From<*const c_char> for ExtString<Ext> {
    fn from(ptr: *const c_char) -> Self {
        ExtString {
            ptr: ptr as *mut c_char,
            _ext: PhantomData::default()
        }
    }
}

impl<Ext: ExternalFunctions> Deref for ExtString<Ext> {
    type Target = CStr;

    fn deref(&self) -> &Self::Target {
        unsafe { CStr::from_ptr(self.ptr) }
    }
}

impl<Ext: ExternalFunctions> std::fmt::Display for ExtString<Ext> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.deref().to_string_lossy())
    }
}

#[derive(Debug)]
pub struct ExtPointer<T> {
    pub ptr: *const T,
}

impl <T> ExtPointer<T> {
    pub fn new(ptr: *const T) -> Self {
        ExtPointer { ptr }
    }

    pub fn null() -> Self {
        ExtPointer { ptr: ptr::null() }
    }
}

unsafe impl<T> Send for ExtPointer<T> {}
unsafe impl<T> Sync for ExtPointer<T> {}

impl<T> Deref for ExtPointer<T> {
    type Target = *const T;

    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}

impl<T> From<*const T> for ExtPointer<T> {
    fn from(ptr: *const T) -> Self {
        ExtPointer { ptr }
    }
}

impl<T> Hash for ExtPointer<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.ptr as *const c_void).hash(state);
    }
}

impl<T> Clone for ExtPointer<T> {
    fn clone(&self) -> Self {
        ExtPointer { ptr: self.ptr }
    }
}

impl<T> Copy for ExtPointer<T> {}
impl<T> PartialEq for ExtPointer<T> {
    fn eq(&self, other: &Self) -> bool {
        ptr::addr_eq(self.ptr, other.ptr)
    }
}

impl<T> Eq for ExtPointer<T> {}
impl<T> Default for ExtPointer<T> {
    fn default() -> Self {
        ExtPointer { ptr: ptr::null() }
    }
}

