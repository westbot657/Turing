use std::cmp::Ordering;
use std::ffi::{c_char, c_void, CStr};
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr;
use crate::ExternalFunctions;

#[derive(Default, Eq, Ord, Clone)]
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
        self.as_u64().partial_cmp(&other.as_u64())
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

impl<Ext: ExternalFunctions> Display for ExtString<Ext> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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

