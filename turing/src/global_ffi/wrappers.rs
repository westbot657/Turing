#![allow(static_mut_refs, clippy::new_without_default)]

use std::ffi::{c_char, c_void, CString};
use std::mem;
use crate::ExternalFunctions;
use crate::interop::params::FreeableDataType;

pub type CsAbort = extern "C" fn(*const c_char, *const c_char);
pub type CsLog = extern "C" fn(*const c_char);
pub type CsFree = extern "C" fn(*const c_char);

pub struct CsFns {
    pub abort: CsAbort,
    pub log_info: CsLog,
    pub log_warn: CsLog,
    pub log_critical: CsLog,
    pub log_debug: CsLog,
    pub free_cs_string: CsFree,
}

extern "C" fn null_abort(_: *const c_char, _: *const c_char) {
    eprintln!("Null abort called, exiting process.");
    std::process::abort()
}
extern "C" fn null_log(_: *const c_char) {}
extern "C" fn null_free(_: *const c_char) {
    eprintln!("null free called, exiting process.");
    std::process::abort()
}

impl CsFns {
    pub const fn new() -> Self {
        Self {
            abort: null_abort,
            log_info: null_log,
            log_warn: null_log,
            log_critical: null_log,
            log_debug: null_log,
            free_cs_string: null_free,
        }
    }

    /// # Safety
    /// as long as ptr points to a valid function, this is fine
    pub unsafe fn link(&mut self, fn_name: &str, ptr: *const c_void) {
        if ptr.is_null() {
            eprintln!("Cannot link '{}' with null pointer", fn_name);
            std::process::abort()
        }
        unsafe {
            match fn_name {
                "abort" => self.abort = mem::transmute(ptr),
                "log_info" => self.log_info = mem::transmute(ptr),
                "log_warn" => self.log_warn = mem::transmute(ptr),
                "log_critical" => self.log_critical = mem::transmute(ptr),
                "log_debug" => self.log_debug = mem::transmute(ptr),
                "free_cs_string" => self.free_cs_string = mem::transmute(ptr),
                _ => {
                    eprintln!("Invalid function name: '{}', process will abort.", fn_name);
                    std::process::abort()
                }
            }
        }
    }
}

pub static mut CS_FNS: CsFns = CsFns::new();

impl ExternalFunctions for CsFns {
    fn abort(error_type: String, error: String) -> ! {
        unsafe {
            let et = CString::new(error_type).unwrap_or_default();
            let e = CString::new(error).unwrap_or_default();
            (CS_FNS.abort)(et.as_ptr(), e.as_ptr());
        }
        eprintln!("C# abort returned when it shouldn't have, aborting process completely.");
        std::process::abort()
    }
    fn log_info(msg: impl ToString) {
        unsafe {
            if let Ok(msg) = CString::new(msg.to_string()) {
                (CS_FNS.log_info)(msg.as_ptr())
            }
        }
    }
    fn log_warn(msg: impl ToString) {
        unsafe {
            if let Ok(msg) = CString::new(msg.to_string()) {
                (CS_FNS.log_warn)(msg.as_ptr())
            }
        }
    }
    fn log_debug(msg: impl ToString) {
        unsafe {
            if let Ok(msg) = CString::new(msg.to_string()) {
                (CS_FNS.log_debug)(msg.as_ptr())
            }
        }
    }
    fn log_critical(msg: impl ToString) {
        unsafe {
            if let Ok(msg) = CString::new(msg.to_string()) {
                (CS_FNS.log_critical)(msg.as_ptr())
            }
        }
    }
    fn free_string(ptr: *const c_char) {
        unsafe {
            (CS_FNS.free_cs_string)(ptr)
        }
    }

    fn free_of_type(ptr: *mut c_void, typ: FreeableDataType) {
        todo!()
    }

}