// Export functions

use anyhow::anyhow;
use slotmap::KeyData;
use std::collections::HashSet;
use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char, c_void},
    mem, panic, path,
};
use wasmtime::{Caller, Val};
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::{
    ExtFns, IntoWasm, OpaquePointerKey, PARAM_KEY_INVALID, ParamKey, ParamsKey, ToCStr,
    TuringState, get_string,
    interop::params::{FfiParam, Param, ParamType, Params},
    wasm::wasm_engine::WasmInterpreter,
    write_string,
};

static mut STATE: Option<RefCell<TuringState>> = None;
/// External functions provided by the FFI environment.
static mut EXT_FNS: Option<RefCell<ExtFns>> = None;

const TURING_UNINIT: &str = "Turing has not been initialized";

pub type FfiCallback = extern "C" fn(ParamKey) -> FfiParam;

/// turing.rs internal Log system, not used by wasm.
pub struct Log {}
macro_rules! mlog {
    ($f:tt => $msg:tt ) => {
        unsafe {
            if let Some(csf) = &mut EXT_FNS {
                let cs = csf.borrow();
                (cs.$f)($msg.to_string().to_cstr_ptr());
            }
        }
    };
}
impl Log {
    pub fn info(msg: impl ToString) {
        mlog!(log_info => msg);
    }
    pub fn warn(msg: impl ToString) {
        mlog!(log_warn => msg);
    }
    pub fn critical(msg: impl ToString) {
        mlog!(log_critical => msg);
    }
    pub fn debug(msg: impl ToString) {
        mlog!(log_debug => msg);
    }
}

pub struct Ext {}
impl Ext {
    pub fn free_string(ptr: *const c_char) {
        if let Some(csf) = unsafe { &mut EXT_FNS } {
            let cs = csf.borrow_mut();
            (cs.free_cs_string)(ptr);
        }
    }
}

// Core systems
#[unsafe(no_mangle)]
/// Initialize all turing state.
pub extern "C" fn init_turing() {
    panic::set_hook(Box::new(|info| {
        eprintln!("Caught panic: {}", info);
        Log::critical(format!("Caught panic: {}", info));
    }));

    unsafe {
        STATE = Some(RefCell::new(TuringState::new()));
        EXT_FNS = Some(RefCell::new(ExtFns::new()));
    }
}

#[unsafe(no_mangle)]
/// Clear out all state. wipes memory, should be clean to use after a second init.
pub extern "C" fn uninit_turing() {
    unsafe {
        STATE = None;
        EXT_FNS = None;
    }
}

#[unsafe(no_mangle)]
/// starts building a new wasm function. May return an error
/// # Safety
/// only safe if `name: *const c_char` points at a valid string
/// and `pointer: *const c_void` points at a function with the signature `Fn(ParamKey) -> FfiParam`
pub unsafe extern "C" fn create_wasm_fn(
    capability: *const c_char,
    name: *const c_char,
    pointer: *const c_void,
) -> FfiParam {
    unsafe {
        let name = CStr::from_ptr(name).to_string_lossy().to_string();
        let capability = CStr::from_ptr(capability).to_string_lossy().to_string();

        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };

        let mut s = state.borrow_mut();

        if s.wasm_fns.contains_key(&name) {
            return Param::Error(format!("wasm fn is already defined: '{}'", name)).into();
        }
        s.active_wasm_fn = Some(name.clone());
        s.wasm_fns
            .insert(name, (capability, pointer, Vec::new(), Vec::new()));
        Param::Void.into()
    }
}

#[unsafe(no_mangle)]
/// appends a parameter type to the specified wasm fn builder, types are identical to the ids used
/// for FfiParam
pub extern "C" fn add_wasm_fn_param_type(param_type: ParamType) -> FfiParam {
    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let mut s = state.borrow_mut();
        if s.wasm_fns.is_empty() || s.active_wasm_fn.is_none() {
            return Param::Error("no wasm function to add parameter type to".to_string()).into();
        }

        if !matches!(
            param_type,
            ParamType::I8
                | ParamType::I16
                | ParamType::I32
                | ParamType::I64
                | ParamType::U8
                | ParamType::U16
                | ParamType::U32
                | ParamType::U64
                | ParamType::F32
                | ParamType::F64
                | ParamType::BOOL
                | ParamType::RustString
                | ParamType::ExtString
                | ParamType::OBJECT
        ) {
            return Param::Error(format!(
                "Invalid param type id: {} {}",
                param_type, param_type
            ))
            .into();
        }
        let active = s.active_wasm_fn.as_ref().unwrap().clone();
        let fn_builder = s.wasm_fns.get_mut(&active).unwrap();
        fn_builder.2.push(param_type);
        Param::Void.into()
    }
}

#[unsafe(no_mangle)]
/// sets the return type of the specified wasm fn builder
pub extern "C" fn set_wasm_fn_return_type(return_type: ParamType) -> FfiParam {
    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let mut s = state.borrow_mut();
        if s.wasm_fns.is_empty() || s.active_wasm_fn.is_none() {
            return Param::Error("no wasm function to add parameter type to".to_string()).into();
        }
        if !matches!(
            return_type,
            ParamType::I8
                | ParamType::I16
                | ParamType::I32
                | ParamType::I64
                | ParamType::U8
                | ParamType::U16
                | ParamType::U32
                | ParamType::U64
                | ParamType::F32
                | ParamType::F64
                | ParamType::BOOL
                | ParamType::RustString
                | ParamType::ExtString
                | ParamType::OBJECT
                | ParamType::VOID
        ) {
            return Param::Error(format!(
                "Invalid param type id: {} {}",
                return_type, return_type
            ))
            .into();
        }

        let active = s.active_wasm_fn.as_ref().unwrap().clone();
        let fn_builder = s.wasm_fns.get_mut(&active).unwrap();
        fn_builder.3.push(return_type);
        Param::Void.into()
    }
}

#[unsafe(no_mangle)]
/// Takes all registered wasm functions, generates their wasm code, and then starts the wasm engine
pub extern "C" fn init_wasm() -> FfiParam {
    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let interp = {
            let mut s = state.borrow_mut();
            WasmInterpreter::new(&mut s).ok()
        };
        let mut s = state.borrow_mut();
        let Some(t) = interp else {
            return Param::Error("Failed to initialize wasm engine".to_string()).into();
        };

        s.wasm = Some(t);
        Param::Void.into()
    }
}

// Params

#[unsafe(no_mangle)]
/// Creates a param builder and returns it's uid
pub extern "C" fn create_params() -> ParamKey {
    unsafe {
        let Some(state) = &mut STATE else {
            return PARAM_KEY_INVALID;
        };

        let mut s = state.borrow_mut();
        let x = s.param_builders.insert(Params::new());
        s.active_builder = Some(x);
        x.0.as_ffi() as ParamKey
    }
}

#[unsafe(no_mangle)]
/// Creates a param builder with a set length.
pub extern "C" fn create_n_params(size: u32) -> ParamKey {
    unsafe {
        let Some(state) = &mut STATE else {
            return 0;
        };
        let mut s = state.borrow_mut();
        let x = s.param_builders.insert(Params::of_size(size));
        s.active_builder = Some(x);
        x.0.as_ffi() as ParamKey
    }
}

#[unsafe(no_mangle)]
/// Binds a param object for use or modification.
pub extern "C" fn bind_params(params: ParamKey) {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            s.active_builder = Some(ParamsKey::from(KeyData::from_ffi(params)));
        }
    }
}

#[unsafe(no_mangle)]
/// Returns an FfiParam that will either be Error or Void
/// Strings (from error and string) are copied, and you are safe to free them after calling.
pub extern "C" fn add_param(value: FfiParam) -> FfiParam {
    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let mut s = state.borrow_mut();
        let typ_id = value.type_id;
        let Ok(val) = value.to_param() else {
            return Param::Error(format!(
                "Failed to add parameter. Invalid type id: {}",
                typ_id
            ))
            .into();
        };
        if let Err(e) = s.push_param(val) {
            return Param::Error(e.to_string()).into();
        }
        Param::Void.into()
    }
}

#[unsafe(no_mangle)]
/// Returns an error if the index is out of bounds
/// Strings (from error and string) are copied, and you are safe to free them after calling.
pub extern "C" fn set_param(index: u32, value: FfiParam) -> FfiParam {
    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let mut s = state.borrow_mut();
        let typ_id = value.type_id;
        let Ok(val) = value.to_param() else {
            return Param::Error(format!(
                "Failed to set parameter. Invalid type id: {}",
                typ_id
            ))
            .into();
        };
        if let Err(e) = s.set_param(index, val) {
            return Param::Error(e.to_string()).into();
        }
        Param::Void.into()
    }
}

#[unsafe(no_mangle)]
/// returns the param at the specified index. the param will be an error value if the index is out
/// of bounds or the state is not initialized.
pub extern "C" fn read_param(index: u32) -> FfiParam {
    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let s = state.borrow_mut();
        s.read_param(index).into()
    }
}

#[unsafe(no_mangle)]
/// frees the params object tied to an id if present, otherwise does nothing.
pub extern "C" fn delete_params(params: ParamKey) {
    unsafe {
        let param_key = ParamsKey::from(KeyData::from_ffi(params));
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            s.param_builders.remove(param_key);
            // clear active builder if it was this one
            if s.active_builder == Some(param_key) {
                s.active_builder = None;
            }
        }
    }
}

#[unsafe(no_mangle)]
/// Calls a function passing it the specified params object. The underlying params object is
/// NOT deleted, but its contents are.
/// If params is 0, calls with an empty parameters object
pub unsafe extern "C" fn call_wasm_fn(
    name: *const c_char,
    params: ParamKey,
    expected_return_type: ParamType,
) -> FfiParam {
    if !expected_return_type.is_valid() {
        return Param::Error(format!("Invalid return type: {}", expected_return_type)).into();
    };

    unsafe {
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let mut s = state.borrow_mut();
        let params2 = if params == PARAM_KEY_INVALID {
            Params::new()
        } else if let Ok(p) = s.swap_params(ParamsKey::from(KeyData::from_ffi(params))) {
            p
        } else {
            return Param::Error("Params object does not exist".to_string()).into();
        };

        let Some(mut wasm) = s.wasm.take() else {
            return Param::Error("Wasm engine not initialized".to_string()).into();
        };
        drop(s); // release borrow before calling wasm

        let name = CStr::from_ptr(name).to_string_lossy().to_string();
        let res = wasm.call_fn(&name, params2, expected_return_type, state);

        state.borrow_mut().wasm = Some(wasm);

        res.into()
    }
}

#[unsafe(no_mangle)]
/// Frees a rust-allocated C string.
/// # Safety
/// dereferencing raw pointers is fun.
pub unsafe extern "C" fn free_string(ptr: *mut c_char) {
    unsafe { free_cstr(ptr) };
}

#[unsafe(no_mangle)]
/// Registers one of the functions that rust uses directly. this is not for wasm functions.
pub unsafe extern "C" fn register_function(name: *const c_char, pointer: *const c_void) {
    unsafe {
        let cstr = CStr::from_ptr(name).to_string_lossy().to_string();
        if let Some(csf) = &mut EXT_FNS {
            let mut cs = csf.borrow_mut();
            cs.link(&cstr, pointer);
        }
    }
}

#[unsafe(no_mangle)]
/// Loads a script by path, also takes an FfiParam id which acts as a list of the loaded mods.
/// Note: the params object is emptied but not deleted.
pub unsafe extern "C" fn load_script(
    source: *const c_char,
    loaded_capabilities: ParamKey,
) -> FfiParam {
    unsafe {
        let source = CStr::from_ptr(source).to_string_lossy().to_string();

        let source = path::Path::new(&source);

        if let Err(e) = source.metadata() {
            return Param::Error(format!(
                "Script does not exist: {:#?}, {:#?}",
                source.to_str(),
                e
            ))
            .into();
        }
        let Some(state) = &mut STATE else {
            return Param::Error(TURING_UNINIT.to_string()).into();
        };
        let mut s = state.borrow_mut();
        {
            let Some(wasm) = &mut s.wasm else {
                return Param::Error("Wasm engine not initialized".to_string()).into();
            };
            if let Err(e) = wasm.load_script(source) {
                return Param::Error(format!("Failed to instantiate wasm module: {}", e)).into();
            };
        }
        s.turing_mini_ctx.active_capabilities.clear();
        let mut caps = HashSet::new();
        {
            let Some(list) = s.param_builders.get(ParamsKey::from(KeyData::from_ffi(
                loaded_capabilities as u64,
            ))) else {
                return Param::Error(format!(
                    "No params object associated with given ParamKey: {}",
                    loaded_capabilities
                ))
                .into();
            };
            for i in 0..list.len() {
                let Some(p) = list.get(i as usize) else {
                    return Param::Error(format!("Unexpected end of params list at index {}", i))
                        .into();
                };
                let Param::String(s) = p else {
                    return Param::Error(format!(
                        "capability at index {} is not a string value",
                        i
                    ))
                    .into();
                };
                caps.insert(s.clone());
            }
        }
        mem::swap(&mut caps, &mut s.turing_mini_ctx.active_capabilities);

        Param::Void.into()
    }
}

/// Frees a rust-allocated C String.
/// # Safety
/// safety was never an option.
pub unsafe fn free_cstr(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

// _host_strcpy(location: *const c_char, size: u32);
// Should only be used in 2 situations:
// 1. after a call to a function that "returns" a string, the guest
//    is required to allocate the size returned in place of the string, and then
//    call this, passing the allocated pointer and the size.
//    If the size passed in does not exactly match the cached string, or there is no
//    cached string, then 0 is returned, otherwise the input pointer is returned.
// 2. for each argument of a function that expects a string, in linear order,
//    failing to retrieve all param strings in the correct order will invalidate
//    the strings with no way to recover.

pub fn wasm_host_strcpy(
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();
    unsafe {
        let Some(state) = &mut STATE else {
            rs[0] = Val::I32(0);
            return Ok(());
        };

        if let Some(st) = state.borrow_mut().turing_mini_ctx.str_cache.pop_front()
            && st.len() + 1 == size as usize
        {
            if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
                write_string(ptr as u32, st, &memory, caller)?;
                rs[0] = Val::I32(ptr);
            }
            return Ok(());
        }
        rs[0] = Val::I32(0);
        Ok(())
    }
}
pub fn wasm_bind_env(
    mut caller: Caller<'_, WasiP1Ctx>,
    cap: &String,
    ps: &[Val],
    rs: &mut [Val],
    p: Vec<ParamType>,
    func: extern "C" fn(ParamKey) -> FfiParam,
) -> Result<(), anyhow::Error> {
    let mut params = Params::new();

    if let Some(state) = unsafe { &mut STATE } {
        let s = state.borrow_mut();
        if !s.turing_mini_ctx.active_capabilities.contains(cap) {
            return Err(anyhow!("Mod capability '{}' is not loaded", cap));
        }
    }

    // set up function parameters
    for (exp_typ, value) in p.iter().zip(ps) {
        match (exp_typ, value) {
            (ParamType::I8, Val::I32(i)) => params.push(Param::I8(*i as i8)),
            (ParamType::I16, Val::I32(i)) => params.push(Param::I16(*i as i16)),
            (ParamType::I32, Val::I32(i)) => params.push(Param::I32(*i)),
            (ParamType::I64, Val::I64(i)) => params.push(Param::I64(*i)),
            (ParamType::U8, Val::I32(u)) => params.push(Param::U8(*u as u8)),
            (ParamType::U16, Val::I32(u)) => params.push(Param::U16(*u as u16)),
            (ParamType::U32, Val::I32(u)) => params.push(Param::U32(*u as u32)),
            (ParamType::U64, Val::I64(u)) => params.push(Param::U64(*u as u64)),
            (ParamType::F32, Val::F32(f)) => params.push(Param::F32(f32::from_bits(*f))),
            (ParamType::F64, Val::F64(f)) => params.push(Param::F64(f64::from_bits(*f))),
            (ParamType::BOOL, Val::I32(b)) => params.push(Param::Bool(*b != 0)),
            (ParamType::RustString | ParamType::ExtString, Val::I32(ptr)) => {
                let ptr = *ptr as u32;

                let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
                    return Err(anyhow!("wasm does not export memory")).into_wasm();
                };
                let st = get_string(ptr, memory.data(&caller));
                params.push(Param::String(st));
            }
            (ParamType::OBJECT, Val::I64(pointer_id)) => {
                let pointer_key =
                    OpaquePointerKey::from(KeyData::from_ffi(*pointer_id as ParamKey));

                if let Some(state) = unsafe { &mut STATE } {
                    let s = state.borrow_mut();
                    if let Some(true_pointer) = s.turing_mini_ctx.opaque_pointers.get(pointer_key) {
                        params.push(Param::Object(*true_pointer));
                    } else {
                        return Err(anyhow!(
                            "opaque pointer does not correspond to a real pointer"
                        ))
                        .into_wasm();
                    }
                }
            }
            _ => params.push(Param::Error("Mismatched parameter type".to_string())),
        }
    }

    // push parameters into an FfiParams
    let pid = if let Some(state) = unsafe { &mut STATE } {
        let mut s = state.borrow_mut();

        s.param_builders.insert(params)
    } else {
        unreachable!("This cannot happen (probably)")
    };

    // Call to C#/rust's provided callback
    let res = func(pid.0.as_ffi() as ParamKey).to_param().into_wasm()?;

    // coerce C# return value into wasm
    if let Some(state) = unsafe { &mut STATE } {
        let mut s = state.borrow_mut();
        let rv = match res {
            Param::I8(i) => Val::I32(i as i32),
            Param::I16(i) => Val::I32(i as i32),
            Param::I32(i) => Val::I32(i),
            Param::I64(i) => Val::I64(i),
            Param::U8(u) => Val::I32(u as i32),
            Param::U16(u) => Val::I32(u as i32),
            Param::U32(u) => Val::I32(u as i32),
            Param::U64(u) => Val::I64(u as i64),
            Param::F32(f) => Val::F32(f.to_bits()),
            Param::F64(f) => Val::F64(f.to_bits()),
            Param::Bool(b) => Val::I32(if b { 1 } else { 0 }),
            Param::String(st) => {
                let l = st.len() + 1;
                s.turing_mini_ctx.str_cache.push_back(st);
                Val::I32(l as i32)
            }
            Param::Object(p) => {
                let opaque = if let Some(opaque) = s.turing_mini_ctx.pointer_backlink.get(&p) {
                    *opaque
                } else {
                    let op = s.turing_mini_ctx.opaque_pointers.insert(p);
                    s.turing_mini_ctx.pointer_backlink.insert(p, op);
                    op
                };
                Val::I32(opaque.0.as_ffi() as i32)
            }
            Param::Error(er) => {
                return Err(anyhow!("Error executing C# function: {}", er)).into_wasm();
            }
            Param::Void => return Ok(()),
        };

        rs[0] = rv;
    }

    Ok(())
}
