#![allow(static_mut_refs)]

use core::slice;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;
use anyhow::{anyhow, Result};
use crate::interop::params::{DataType, FfiParam, Param, Params};
use crate::Turing;
use crate::wasm::wasm_engine::{WasmCallback, WasmFnMetadata};
use crate::win_ffi::wrappers::*;

#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be a valid pointer to a string made via rust's `CString::into_raw` method.
/// If `ptr` is null this function returns without attempting to free.
unsafe extern "C" fn free_string(ptr: *mut c_char) {
    if ptr.is_null() { return }
    let _ = unsafe { CString::from_raw(ptr) };
}

#[unsafe(no_mangle)]
extern "C" fn register_function(name: *const c_char, pointer: *const c_void) {
    unsafe {
        let cstr = CStr::from_ptr(name).to_string_lossy().into_owned();
        CS_FNS.link(&cstr, pointer);
    }
}

#[unsafe(no_mangle)]
extern "C" fn create_wasm_fn_map() -> *mut HashMap<String, WasmFnMetadata> {
    let map = Box::new(HashMap::new());
    Box::into_raw(map)
}

#[unsafe(no_mangle)]
/// # Safety
/// `capability` must be a valid C string pointer of valid `UTF-8`.
/// `callback` must be a valid pointer to a function: `extern "C" fn(FfiParamsArray) -> FfiParam`.
unsafe extern "C" fn create_wasm_fn_metadata(capability: *const c_char, callback: WasmCallback) -> *mut WasmFnMetadata {
    let cap = unsafe { CStr::from_ptr(capability).to_string_lossy() };
    let data = WasmFnMetadata::new(cap, callback);
    Box::into_raw(Box::new(data))
}

#[unsafe(no_mangle)]
/// # Safety
/// `data` must be a valid pointer to a `WasmFnMetadata`.
/// `params` must point to the first element of `DataType` array.
/// `params_count` must be the accurate size of the `params` array.
/// Returns a pointer to an error message, if the pointer is null then no error occurred. Caller is responsible for freeing this string.
/// none of the passed data is freed.
unsafe extern "C" fn add_param_types_to_wasm_fn_data(data: *mut WasmFnMetadata, params: *mut DataType, params_count: u32) -> *const c_char {
    if data.is_null() || params.is_null() {
        return CString::new("data or params was null".to_string()).unwrap().into_raw()
    }
    let data = unsafe { &mut *data };
    let array = unsafe { slice::from_raw_parts(params, params_count as usize) };

    for ty in array {
        if let Err(e) = data.add_param_type(*ty) {
            return CString::new(format!("{}", e)).unwrap().into_raw()
        }
    }
    ptr::null()
}

#[unsafe(no_mangle)]
/// # Safety
/// `data` must be a valid pointer to a `WasmFnMetadata`.
/// Returns a pointer to an error message, if the pointer is null then no error occurred. Caller is responsible for freeing this string.
/// none of the passed data is freed.
unsafe extern "C" fn set_wasm_fn_return_type(data: *mut WasmFnMetadata, return_type: DataType) -> *const c_char {
    if data.is_null() {
        return CString::new("data is null".to_string()).unwrap().into_raw()
    }
    let data = unsafe { &mut *data };
    if let Err(e) = data.add_return_type(return_type) {
        return CString::new(format!("{}", e)).unwrap().into_raw()
    }
    ptr::null()
}

#[unsafe(no_mangle)]
/// # Safety
/// `map` must be a valid pointer to a `HashMap<String, WasmFnMetadata>`.
/// `name` must be a non-null `UTF-8` string.
/// `data` must be a valid pointer to a `WasmFnMetadata`.
/// Will silently fail if any are null so check before calling.
/// If `data` is not null it will be freed regardless of validity of other args.
/// Only `data` will be freed.
unsafe extern "C" fn add_wasm_fn_to_map(map: *mut HashMap<String, WasmFnMetadata>, name: *const c_char, data: *mut WasmFnMetadata) {
    if data.is_null() { return }

    let data = unsafe { *Box::from_raw(data) };
    if map.is_null() || name.is_null() { return }

    let name = unsafe { CStr::from_ptr(name).to_string_lossy().into_owned() };
    let map = unsafe { &mut *map };

    map.insert(name, data);
}

#[unsafe(no_mangle)]
/// # Safety
/// `map` must be a valid pointer to a `HashMap<String, WasmFnMetadata>`
unsafe extern "C" fn copy_wasm_fn_map(map: *mut HashMap<String, WasmFnMetadata>) -> *mut HashMap<String, WasmFnMetadata> {
    if map.is_null() { return map }
    Box::into_raw(Box::new(unsafe { &*map }.clone()))
}


#[unsafe(no_mangle)]
/// # Safety
/// `map` must be a valid pointer to a `HashMap<String, WasmFnMetadata>`.
/// This function should only be called if a map is made and then never ends up getting used
unsafe extern "C" fn delete_wasm_fn_map(map: *mut HashMap<String, WasmFnMetadata>) {
    let _ = unsafe { Box::from_raw(map) };
}


#[unsafe(no_mangle)]
/// # Safety
/// `wasm_fns_ptr` must be a valid pointer to a `HashMap<String, WasmFnMetadata>`.
/// `wasm_fns_ptr` will be freed during this function and must no longer be used.
unsafe extern "C" fn create_instance(wasm_fns_ptr: *mut HashMap<String, WasmFnMetadata>) -> *mut Result<Turing<CsFns>> {
    let map = unsafe { Box::from_raw(wasm_fns_ptr) };
    let mut turing = Turing::new();
    turing.wasm_fns = *map;
    let turing = Box::new(turing.build());
    Box::into_raw(turing)
}


#[unsafe(no_mangle)]
/// # Safety
/// `res_ptr` must be a valid pointer to a `Result<Turing>`.
/// the caller is responsible for freeing the returned string if not null.
unsafe extern "C" fn check_error(res_ptr: *mut Result<Turing<CsFns>>) -> *const c_char {
    let res = unsafe { &*res_ptr };

    if let Err(e) = res {
        CString::new(format!("{}", e)).unwrap().into_raw()
    } else {
        ptr::null()
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `res_ptr` must be a valid pointer to a `Result<Turing>`.
/// `res_ptr` must have been checked with `check_error` and handled if an error was returned.
/// If `res_ptr` points to an `Err` value, this function will abort the process.
/// `res_ptr` will be freed during this function and must no longer be used.
unsafe extern "C" fn unwrap_instance(res_ptr: *mut Result<Turing<CsFns>>) -> *mut Turing<CsFns> {
    let res = unsafe { *Box::from_raw(res_ptr) };

    let Ok(turing) = res else {
        eprintln!("unwrap_turing(): res_ptr pointed to Err, aborting process");
        std::process::abort();
    };
    let turing = Box::new(turing);
    Box::into_raw(turing)
}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`
unsafe extern "C" fn delete_instance(turing: *mut Turing<CsFns>) {
    let _ = unsafe { Box::from_raw(turing) };
}


#[unsafe(no_mangle)]
extern "C" fn create_params(size: u32) -> *mut Params {
    Box::into_raw(Box::new(if size == 0 {
        Params::new()
    } else {
        Params::of_size(size)
    }))
}

#[unsafe(no_mangle)]
/// # Safety
/// `params` must be a valid pointer to a `Params`.
/// This function silently fails if params is null.
unsafe extern "C" fn add_param(params: *mut Params, param: FfiParam) {
    if params.is_null() { return }
    let params = unsafe { &mut *params };
    let param = param.as_param::<CsFns>().unwrap();
    params.push(param);
}

#[unsafe(no_mangle)]
/// # Safety
/// `params` must be a valid pointer to a `Params` and must not be used after this call.
unsafe extern "C" fn delete_params(params: *mut Params) {
    if params.is_null() { return }
    let _ = unsafe { Box::from_raw(params) };
}

#[unsafe(no_mangle)]
/// # Safety
/// `params` must be a valid pointer to a `Params`.
/// Returns a copy of an `FfiParam` which may be an error value if an error occurs.
unsafe extern "C" fn get_param(params: *mut Params, index: u32) -> FfiParam {
    if params.is_null() {
        return Param::Error("params is null".to_string()).to_rs_param()
    }

    let params = unsafe { &*params };

    if let Some(p) = params.get(index as usize) {
        p.clone().to_rs_param()
    } else {
        Param::Error("index out of bounds".to_string()).to_rs_param()
    }
}

#[unsafe(no_mangle)]
/// This will correctly (probably) free an FfiParam including rust and ext strings
extern "C" fn delete_param(param: FfiParam) {
    let _ = param.into_param::<CsFns>().unwrap();
}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// `source` must be a valid `UTF-8` string.
/// `loaded_capabilities` must be a valid pointer to an array of valid string pointers.
/// Returns an `FfiParam` that is either void or an error value.
unsafe extern "C" fn load_wasm_script(turing: *mut Turing<CsFns>, source: *const c_char, loaded_capabilities: *mut *const c_char, capability_count: u32) -> FfiParam {
    if turing.is_null() {
        return Param::Error("turing is null".to_string()).to_rs_param()
    }
    if source.is_null() {
        return Param::Error("source is null".to_string()).to_rs_param()
    }
    if loaded_capabilities.is_null() {
        return Param::Error("loaded_capabilities is null".to_string()).to_rs_param()
    }

    let turing = unsafe { &mut *turing };
    let source = unsafe { CStr::from_ptr(source).to_string_lossy() };

    let cstr_array = unsafe { slice::from_raw_parts(loaded_capabilities, capability_count as usize) };

    let res = cstr_array
        .iter()
        .map(|c_str| {
            if c_str.is_null() {
                Err(anyhow!("capability string is null"))
            } else {
                Ok(unsafe { CStr::from_ptr(*c_str).to_string_lossy().into_owned() })
            }
        })
        .collect::<Result<Vec<String>>>();

    let capabilities = match res {
        Ok(ls) => ls,
        Err(e) => return Param::Error(format!("{}", e)).to_rs_param()
    };
    
    if let Err(e) = turing.load_script(source, &capabilities) {
        Param::Error(format!("{}", e))
    } else {
        Param::Void
    }.to_rs_param()

}


#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// `name` must be a valid `UTF-8` string.
/// `params` must be a valid pointer to a `Params`.
/// If `params` is null, an empty `Params` will be used for the function call instead.
/// `params` will not be freed.
unsafe extern "C" fn call_wasm_fn(turing: *mut Turing<CsFns>, name: *const c_char, params: *mut Params, expected_return_type: DataType) -> FfiParam {
    if turing.is_null() {
        return Param::Error("turing is null".to_string()).to_rs_param()
    }
    if name.is_null() {
        return Param::Error("name is null".to_string()).to_rs_param()
    }
    let turing = unsafe { &mut *turing };

    let name = unsafe { CStr::from_ptr(name).to_string_lossy() };

    let params = if params.is_null() {
        Params::new()
    } else {
        unsafe { &*params }.clone()
    };

    turing.call_wasm_fn(name, params, expected_return_type).to_rs_param()

}


