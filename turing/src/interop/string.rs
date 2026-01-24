use std::{ffi::{CStr, CString}, fmt::{Display, Formatter}, ops::Deref};

/// A Rust string representation for interop purposes.
/// It can either be a CString or a standard String.
/// This is used to reduce unnecessary allocations and conversions.
#[derive(Clone, Debug, PartialEq, Hash , Eq)]
pub enum RustString {
    CString(CString),
    String(String),
}

impl From<CString> for RustString {
    fn from(value: CString) -> Self {
        RustString::CString(value)
    }
}

impl From<String> for RustString {
    fn from(value: String) -> Self {
        RustString::String(value)
    }
}

impl From<&str> for RustString {
    fn from(value: &str) -> Self {
        RustString::String(value.to_owned())
    }
}

impl From<&CStr> for RustString {
    fn from(value: &CStr) -> Self {
        RustString::CString(value.to_owned())
    }
}

impl RustString {
    /// Converts the RustString into a CString.
    /// If it's already a CString, it returns it directly.
    /// If it's a String, it converts it to CString.
    pub fn to_cstring(&self) -> CString {
        match self {
            RustString::CString(cstr) => cstr.clone(),
            RustString::String(s) => CString::new(s.as_str()).unwrap(),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            RustString::CString(cstr) => cstr.as_bytes(),
            RustString::String(s) => s.as_bytes(),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            RustString::CString(cstr) => cstr.to_str().unwrap(),
            RustString::String(s) => s.as_str(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            RustString::CString(cstr) => cstr.to_bytes().len(),
            RustString::String(s) => s.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            RustString::CString(cstr) => cstr.to_bytes().is_empty(),
            RustString::String(s) => s.is_empty(),
        }
    }

    pub fn into_string(self) -> String {
        match self {
            RustString::CString(cstr) => cstr.into_string().unwrap(),
            RustString::String(s) => s,
        }
    }

    pub fn into_cstring(self) -> CString {
        match self {
            RustString::CString(cstr) => cstr,
            RustString::String(s) => CString::new(s).unwrap(),
        }
    }
}

/// Converts the RustString into a standard String.
/// If it's already a String, it returns it directly.
/// If it's a CString, it converts it to String.
impl Display for RustString {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RustString::CString(cstr) => write!(f, "{}", cstr.to_string_lossy()),
            RustString::String(s) => write!(f, "{}", s),
        }
    }
}

impl Deref for RustString {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            RustString::CString(cstr) => cstr.as_bytes(),
            RustString::String(s) => s.as_bytes(),
        }
    }
}