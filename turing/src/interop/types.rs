use std::cmp::Ordering;
use std::ffi::{c_char, c_void, CStr};
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::{ptr, slice};
use serde::{Deserialize, Serialize};

use crate::ExternalFunctions;

#[derive(Debug, Default, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct Semver {
    pub major: u32,
    pub minor: u16,
    pub patch: u16,
}

impl Semver {
    pub fn new(major: u32, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    pub fn from_u64(ver: u64) -> Self {
        Self {
            major: (ver >> 32) as u32,
            minor: ((ver >> 16) & 0xFFFF) as u16,
            patch: (ver & 0xFFFF) as u16,
        }
    }

    pub fn as_u64(&self) -> u64 {
        ((self.major as u64) << 32) | ((self.minor as u64) << 16) | (self.patch as u64)
    }

    pub fn into_u64(self) -> u64 {
        ((self.major as u64) << 32) | ((self.minor as u64) << 16) | (self.patch as u64)
    }

}

impl PartialEq for Semver {
    fn eq(&self, other: &Self) -> bool {
        self.as_u64() == other.as_u64()
    }
}

impl PartialOrd for Semver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Semver {
    fn cmp(&self, other: &Self) -> Ordering {
        other.as_u64().cmp(&other.as_u64())
    }
}

impl Display for Semver {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}


/// A string allocated externally, to be managed by the external environment.
pub struct ExtString<Ext: ExternalFunctions> {
    pub ptr: *const c_char,
    _ext: PhantomData<Ext>
}

impl<Ext: ExternalFunctions> ExtString<Ext> {
    pub fn new(ptr: *const c_char) -> Self {
        ExtString { ptr, _ext: PhantomData }
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
            _ext: PhantomData
        }
    }
}

impl<Ext: ExternalFunctions> From<*const c_char> for ExtString<Ext> {
    fn from(ptr: *const c_char) -> Self {
        ExtString {
            ptr: ptr as *mut c_char,
            _ext: PhantomData
        }
    }
}

impl<Ext: ExternalFunctions> Deref for ExtString<Ext> {
    type Target = CStr;

    fn deref(&self) -> &Self::Target {
        unsafe { CStr::from_ptr(self.ptr) }
    }
}

impl<Ext: ExternalFunctions> Display for ExtString<Ext> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.deref().to_string_lossy())
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ExtPointer {
    pub ptr: *const c_void
}

impl ExtPointer {
    pub fn new(ptr: *const c_void) -> Self {
        ExtPointer { ptr }
    }

    pub fn null() -> Self {
        ExtPointer { ptr: ptr::null() }
    }
}

unsafe impl Send for ExtPointer {}
unsafe impl Sync for ExtPointer {}

impl Deref for ExtPointer {
    type Target = *const c_void;

    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}

impl From<*const c_void> for ExtPointer {
    fn from(ptr: *const c_void) -> Self {
        ExtPointer { ptr }
    }
}

impl Hash for ExtPointer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state);
    }
}

// impl Clone for ExtPointer {
//     fn clone(&self) -> Self {
//         ExtPointer { ptr: self.ptr }
//     }
// }
//
// impl Copy for ExtPointer {}
impl PartialEq for ExtPointer {
    fn eq(&self, other: &Self) -> bool {
        ptr::addr_eq(self.ptr, other.ptr)
    }
}

impl Eq for ExtPointer {}
impl Default for ExtPointer {
    fn default() -> Self {
        ExtPointer { ptr: ptr::null() }
    }
}



#[repr(C)]
#[derive(Copy)]
pub struct U32Buffer {
    pub size: u32,
    pub array: *mut u32,
}

impl Clone for U32Buffer {
    fn clone(&self) -> Self { *self }
}

impl U32Buffer {
    /// Moves the data into a Vec<u32> and frees the underlying data directly
    pub fn from_rust(self) -> Vec<u32> {
        let slice = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.array, self.size as usize)) };
        slice.into_vec()

    }

    /// Copies the data into a Vec<u32> and asks the external code to free the underlying data
    pub fn from_ext<Ext: ExternalFunctions>(self) -> Vec<u32> {
        let slice = unsafe { slice::from_raw_parts(self.array, self.size as usize) };
        let v = slice.to_vec();
        Ext::free_u32_buffer(self);
        v
    }

    /// Copies the data into a Vec<u32> without freeing in any way
    pub fn borrow(&self) -> Vec<u32> {
        let slice = unsafe { slice::from_raw_parts(self.array, self.size as usize) };
        slice.to_vec()
    }

}

