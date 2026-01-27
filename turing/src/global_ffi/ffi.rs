#![allow(static_mut_refs)]

use core::slice;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::ptr;
use anyhow::{anyhow, Result};
use rustc_hash::FxHashMap;
use crate::interop::params::{DataType, FfiParam, FreeableDataType, Param, Params};
use crate::interop::types::Semver;
use crate::{Turing, panic_hook, spec_gen};
use crate::engine::types::{ScriptCallback, ScriptFnMetadata};
use crate::global_ffi::wrappers::*;

pub type ScriptFnMap = FxHashMap<String, ScriptFnMetadata>;
pub type TuringInstance = Turing<CsFns>;
pub type TuringInitResult = Result<Turing<CsFns>>;
pub type VersionTable = Vec<(String, Semver)>;
pub type CacheKey = u32;

trait VerTableImpl {
    fn contains_key(&self, key: &str) -> bool;
    fn get_ver(&self, key: &str) -> Option<&Semver>;
}

impl VerTableImpl for VersionTable {
    fn contains_key(&self, key: &str) -> bool {
        self.iter().any(|(k, _)| key == k)
    }
    fn get_ver(&self, key: &str) -> Option<&Semver> {
        self.iter().find_map(|(k, v)| if k == key { Some(v) } else { None })
    }
}

/// Installs a panic hook that will log panic information to stderr and optionally to a specified file.
/// Will also log via the CsFns logging functions.
/// # Safety
/// `crash_dmp_out` must be either null or a valid pointer to a UTF-8 C-String.
/// If non-null, the panic hook will attempt to write panic information to the specified file path.
#[unsafe(no_mangle)]
unsafe extern "C" fn turing_install_panic_hook(crash_dmp_out: *const c_char) {
    let file_out = if crash_dmp_out.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(crash_dmp_out).to_string_lossy().into_owned() })
    };

    let file_path = file_out.map(PathBuf::from);

    std::panic::set_hook(Box::new(move |info| {
        panic_hook::<CsFns>(file_path.clone(), info);
    }));
}


#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be a valid pointer to a string made via rust's `CString::into_raw` method.
unsafe extern "C" fn turing_free_string(ptr: *mut c_char) {
    let _ = unsafe { CString::from_raw(ptr) };
}

#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must be a valid pointer to a `Mat4`, `Vec4`, or `Quat`.
/// `typ` must be a `FreeableDataType` compatible number, and must match the type the `ptr` points to.
unsafe extern "C" fn turing_free_of_type(ptr: *mut c_void, typ: FreeableDataType) {
    unsafe { typ.free_ptr(ptr) }
}

#[unsafe(no_mangle)]
extern "C" fn turing_register_function(name: *const c_char, callback: *const c_void) {
    unsafe {
        let cstr = CStr::from_ptr(name).to_string_lossy().into_owned();
        CS_FNS.link(&cstr, callback);
    }
}

#[unsafe(no_mangle)]
extern "C" fn turing_create_fn_map() -> *mut ScriptFnMap {
    let map = Box::new(FxHashMap::default());
    Box::into_raw(map)
}

#[unsafe(no_mangle)]
/// # Safety
/// `map` must be a valid pointer to a `HashMap<String, ScriptFnMetadata>`.
/// `name` must be a non-null `UTF-8` string.
/// `data` must be a valid pointer to a `ScriptFnMetadata`.
unsafe extern "C" fn turing_fn_map_add_data(map: *mut ScriptFnMap, name: *const c_char, data: *mut ScriptFnMetadata) {
    let data = unsafe { *Box::from_raw(data) };

    let name = unsafe { CStr::from_ptr(name).to_string_lossy().into_owned() };
    let map = unsafe { &mut *map };

    map.insert(name, data);
}

#[unsafe(no_mangle)]
/// # Safety
/// `map` must be a valid pointer to a `HashMap<String, ScriptFnMetadata>`
unsafe extern "C" fn turing_fn_map_copy(map: *mut ScriptFnMap) -> *mut ScriptFnMap {
    Box::into_raw(Box::new(unsafe { &*map }.clone()))
}

#[unsafe(no_mangle)]
/// # Safety
/// `map` must be a valid pointer to a `HashMap<String, ScriptFnMetadata>`.
/// This function should only be called if a map is made and then never ends up getting used
unsafe extern "C" fn turing_delete_fn_map(map: *mut ScriptFnMap) {
    let _ = unsafe { Box::from_raw(map) };
}

#[unsafe(no_mangle)]
/// # Safety
/// `capability` must be a valid C string pointer of valid `UTF-8` or null.
/// `callback` must be a valid pointer to a function: `extern "C" fn(FfiParamsArray) -> FfiParam`.
/// `doc_comment` must be either null or a valid pointer to a string. When null, the function is considered to not have a doc comment.
unsafe extern "C" fn turing_create_script_data(
    capability: *const c_char,
    callback: ScriptCallback,
    doc_comment: *const c_char,
) -> *mut ScriptFnMetadata {
    if capability.is_null() {
        panic!("turing_create_script_data(): capability must be a valid string pointer, null is not allowed");
    }

    let cap = unsafe {
        CStr::from_ptr(capability).to_string_lossy().to_string()
    };

    let doc = if doc_comment.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(doc_comment).to_string_lossy().to_string() })
    };
    let data = ScriptFnMetadata::new(cap, callback, doc);
    Box::into_raw(Box::new(data))
}

#[unsafe(no_mangle)]
/// # Safety
/// `data` must be a valid pointer to a `ScriptFnMetadata`.
/// `params` must point to the first element of `DataType` array.
/// `param_names` must point to the fist element of a valid-c-string array.
/// `param_type_names` must point to the first element of an optional c-string array.
/// `params_count` must be the accurate size of the `params`, `param_names`, and `param_type_names` array.
/// Returns a pointer to an error message, if the pointer is null then no error occurred. Caller is responsible for freeing this string.
/// none of the passed data is freed.
unsafe extern "C" fn turing_script_data_add_param_type(data: *mut ScriptFnMetadata, params: *mut DataType, param_names: *mut *const c_char, param_type_names: *mut *const c_char, params_count: u32) -> *const c_char {
    let data = unsafe { &mut *data };
    let array = unsafe { slice::from_raw_parts(params, params_count as usize) };
    let names = unsafe { slice::from_raw_parts(param_names, params_count as usize) };
    let type_names = unsafe { slice::from_raw_parts(param_type_names, params_count as usize) };

    for i in 0..(params_count as usize) {
        let ty = array[i];
        let name = unsafe { CStr::from_ptr(names[i]) }.to_string_lossy().into_owned();

        let ty_ptr = type_names[i];
        match ty_ptr.is_null() {
            true => {
                if let Err(e) = data.add_param_type(ty, name) {
                    return CString::new(format!("{}", e)).unwrap().into_raw()
                }
            },
            false => {
                let ty_name = unsafe { CStr::from_ptr(ty_ptr) }.to_string_lossy().into_owned();
                if let Err(e) = data.add_param_type_named(ty, name, ty_name) {
                    return CString::new(format!("{}", e)).unwrap().into_raw()
                }
            },
        };
    }

    ptr::null()
}

#[unsafe(no_mangle)]
/// # Safety
/// `data` must be a valid pointer to a `ScriptFnMetadata`.
/// Returns a pointer to an error message, if the pointer is null then no error occurred. Caller is responsible for freeing this string.
/// none of the passed data is freed.
unsafe extern "C" fn turing_script_data_set_return_type(data: *mut ScriptFnMetadata, return_type: DataType, type_names: *const c_char) -> *const c_char {
    let data = unsafe { &mut *data };
    let return_type_name = unsafe { type_names.as_ref().map(|ptr|  CStr::from_ptr(ptr).to_string_lossy().into_owned() ) };

    if let Err(e) = match return_type_name {
        Some(name) => data.add_return_type_named(return_type, name),
        None => data.add_return_type(return_type),
    } {
        return CString::new(format!("{}", e)).unwrap().into_raw()
    }
    ptr::null()
}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// `source` must be a valid `UTF-8` string.
/// `loaded_capabilities` must be a valid pointer to an array of valid string pointers.
/// Returns an `FfiParam` that is either void or an error value.
unsafe extern "C" fn turing_script_load(turing: *mut TuringInstance, source: *const c_char, loaded_capabilities: *mut *const c_char, capability_count: u32) -> FfiParam {
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
        Param::Error(format!("{}\n{}", e, e.backtrace()))
    } else {
        Param::Void
    }.to_rs_param()

}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// `name_key` must be a cache key, from calling `turing_script_cache_fn_name`.
/// `params` must be a valid pointer to a `Params`.
/// If `params` is null, an empty `Params` will be used for the function call instead.
/// `params` will not be freed.
unsafe extern "C" fn turing_script_call_fn(turing: *mut TuringInstance, name_key: CacheKey, params: *mut Params, expected_return_type: DataType) -> FfiParam {
    let turing = unsafe { &mut *turing };

    let params = if params.is_null() {
        Params::new()
    } else {
        unsafe { &*params }.clone()
    };

    turing.call_fn((name_key).into(), params, expected_return_type).to_rs_param()

}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// `name` must be a valid pointer to a UTF-8 C-String.
unsafe extern "C" fn turing_script_get_fn_name(turing: *mut TuringInstance, name: *const c_char) -> CacheKey {
    let turing = unsafe { &mut *turing };
    
    let name = unsafe { CStr::from_ptr(name).to_string_lossy() };
    
    turing.get_fn_key(name.as_ref()).map(|x| x.0).unwrap_or(u32::MAX)
}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// The caller is responsible for freeing the returned error string if not null
unsafe extern "C" fn turing_script_fast_call_update(turing: *mut TuringInstance, delta_time: f32) -> *const c_char {
    let turing = unsafe { &mut *turing };

    if let Err(e) = turing.fast_call_update(delta_time) {
        CString::new(e).unwrap().into_raw()
    } else {
        ptr::null()
    }

}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// The caller is responsible for freeing the returned error string if not null
unsafe extern "C" fn turing_script_fast_call_fixed_update(turing: *mut TuringInstance, delta_time: f32) -> *const c_char {
    let turing = unsafe { &mut *turing };
    if let Err(e) = turing.fast_call_fixed_update(delta_time) {
        CString::new(e).unwrap().into_raw()
    } else {
        ptr::null()
    }
}

/// Dumps the currently loaded script definitions to the specified output directory.
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// `out_dir` must be a valid pointer to a UTF-8 C-String.
/// 
/// The caller is responsible for freeing the returned error string if not null
#[unsafe(no_mangle)]
unsafe extern "C" fn turing_script_dump_sec(out_dir: *const c_char, wasm_fns_ptr: *mut ScriptFnMap, versions: *mut VersionTable) -> *const c_char {
    let map = unsafe { &*wasm_fns_ptr };
    let versions = unsafe { &*versions };

    let versions_map = versions.clone().into_iter().collect();

    let out = unsafe { CStr::from_ptr(out_dir).to_string_lossy().into_owned() };
    let out = std::path::Path::new(&out);

    match spec_gen::generator::generate_specs(map, &versions_map, out) {
        Ok(_) => ptr::null(),
        Err(e) => CString::new(format!("{}", e)).unwrap().into_raw(),
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `wasm_fns_ptr` must be a valid pointer to a `HashMap<String, ScriptFnMetadata>`.
/// `wasm_fns_ptr` will be freed during this function and must no longer be used.
unsafe extern "C" fn turing_create_instance(wasm_fns_ptr: *mut ScriptFnMap) -> *mut TuringInitResult {
    let map = unsafe { Box::from_raw(wasm_fns_ptr) };
    let mut turing = Turing::new();
    turing.script_fns = *map;
    let turing = Box::new(turing.build());
    Box::into_raw(turing)
}


#[unsafe(no_mangle)]
/// # Safety
/// `res_ptr` must be a valid pointer to a `Result<Turing>`.
/// the caller is responsible for freeing the returned string if not null.
unsafe extern "C" fn turing_instance_check_error(res_ptr: *mut TuringInitResult) -> *const c_char {
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
unsafe extern "C" fn turing_instance_unwrap(res_ptr: *mut TuringInitResult) -> *mut TuringInstance {
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
unsafe extern "C" fn turing_delete_instance(turing: *mut TuringInstance) {
    let _ = unsafe { Box::from_raw(turing) };
}


#[unsafe(no_mangle)]
extern "C" fn turing_create_params(size: u32) -> *mut Params {
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
unsafe extern "C" fn turing_params_add_param(params: *mut Params, param: FfiParam) {
    let params = unsafe { &mut *params };
    let param = param.as_param::<CsFns>().unwrap();
    params.push(param);
}

#[unsafe(no_mangle)]
/// # Safety
/// `params` must be a valid pointer to a `Params` and must not be used after this call.
unsafe extern "C" fn turing_delete_params(params: *mut Params) {
    let _ = unsafe { Box::from_raw(params) };
}

#[unsafe(no_mangle)]
/// # Safety
/// `params` must be a valid pointer to a `Params`.
/// Returns a copy of an `FfiParam` which may be an error value if an error occurs.
unsafe extern "C" fn turing_params_get_param(params: *mut Params, index: u32) -> FfiParam {
    let params = unsafe { &*params };

    if let Some(p) = params.get(index as usize) {
        p.clone().to_rs_param()
    } else {
        Param::Error("index out of bounds".to_string()).to_rs_param()
    }
}

#[unsafe(no_mangle)]
/// This will correctly (probably) free an FfiParam including rust and ext strings
extern "C" fn turing_delete_param(param: FfiParam) {
    let _ = param.into_param::<CsFns>().unwrap();
}

#[unsafe(no_mangle)]
/// # Safety
/// `turing` must be a valid pointer to a `Turing`.
/// The returned table may be null if no engine is active or no script is loaded.
unsafe extern "C" fn turing_versions_get(turing: *mut TuringInstance) -> *mut VersionTable {
    let turing = unsafe { &*turing };
    let Some(versions) = turing.get_api_versions() else {
        return ptr::null::<VersionTable>() as *mut _;
    };
    
    let versions: VersionTable = versions.iter().map(|(n, v)| (n.clone(), *v)).collect();
    let versions = Box::new(versions.clone());
    Box::into_raw(versions)
}

#[unsafe(no_mangle)]
/// Creates a new VersionTable and returns a pointer to it. You must free this with `turing_delete_versions`
extern "C" fn turing_versions_create() -> *mut VersionTable {
    let versions: VersionTable = Default::default();
    let versions = Box::new(versions);
    Box::into_raw(versions)
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`.
/// `name` must be a valid pointer to a UTF-8 C string
/// `packed_version` is packed as major:u32 << 32 | minor:u16 << 16 | patch:u16
unsafe extern "C" fn turing_versions_set_api_version(versions: *mut VersionTable, name: *const c_char, packed_version: u64) {
    let versions = unsafe { &mut *versions };
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    versions.push((name.to_string(), Semver::from_u64(packed_version)))
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`.
/// will return false if `versions` is null
unsafe extern "C" fn turing_versions_contains_mod(versions: *mut VersionTable, name: *const c_char) -> bool {
    let versions = unsafe { &*versions };
    let name = unsafe { CStr::from_ptr(name).to_string_lossy().into_owned() };
    versions.contains_key(&name)
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`.
/// returns the version as a packed u64 of (major u32, minor u16, patch u16)
/// will return 0 if `versions` is null or if the specified mod name is not in the table.
unsafe extern "C" fn turing_versions_get_mod_version(versions: *mut VersionTable, name: *const c_char) -> u64 {
    let versions = unsafe { &*versions };
    let name = unsafe { CStr::from_ptr(name).to_string_lossy().into_owned() };
    let Some(v) = versions.get_ver(&name) else {
        return 0;
    };
    v.as_u64()
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`
unsafe extern "C" fn turing_delete_versions(versions: *mut VersionTable) {
    let _ = unsafe { *Box::from_raw(versions) };
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`
unsafe extern "C" fn turing_versions_get_count(versions: *mut VersionTable) -> u32 {
    let versions = unsafe { &*versions };
    versions.len() as u32
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`
/// `index` must be within `0..<versions.len()` (checked with turing_versions_get_count)
unsafe extern "C" fn turing_versions_get_mod_name(versions: *mut VersionTable, index: u32) -> *const c_char {
    let versions = unsafe { &*versions };

    let Some((name, _)) = versions.get(index as usize) else {
        return ptr::null()
    };

    CString::new(name.clone()).unwrap().into_raw()
}

#[unsafe(no_mangle)]
/// # Safety
/// `versions` must be a valid pointer to a `VersionTable`
/// `index` must be within `0..<versions.len()` (checked with turing_versions_get_count)
unsafe extern "C" fn turing_versions_get_mod_version_indexed(versions: *mut VersionTable, index: u32) -> u64 {
    let versions = unsafe { &*versions };

    let Some((_, v)) = versions.get(index as usize) else {
        return 0;
    };

    v.as_u64()
}
