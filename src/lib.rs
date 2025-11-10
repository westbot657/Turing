#![allow(static_mut_refs, clippy::new_without_default)]

pub mod wasm;
pub mod interop;
pub mod util;

#[cfg(test)]
pub mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_void, CStr, CString};
use std::{mem, path};

use anyhow::{anyhow, Result};
use wasmtime::{Caller, Engine, FuncType, Linker, Memory, Val, ValType};
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::wasm::wasm_engine::WasmInterpreter;

use crate::interop::params::{param_type, Params};

use self::interop::params::{FfiParam, Param};
use self::util::{free_cstr, ToCStr, TrackedHashMap};

type AbortFn = extern "C" fn(*const c_char, *const c_char);
type LogFn = extern "C" fn(*const c_char);
type FreeStr = extern "C" fn(*const c_char);

pub struct CsFns {
    pub abort: AbortFn,
    pub log_info: LogFn,
    pub log_warn: LogFn,
    pub log_critical: LogFn,
    pub log_debug: LogFn,
    pub free_cs_string: FreeStr,
}

extern "C" fn null_abort(_: *const c_char, _: *const c_char) {}
extern "C" fn null_log(_: *const c_char) {}
extern "C" fn null_free(_: *const c_char) {}
impl CsFns {
    pub fn new() -> Self {
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
    /// 404 safety not found (only safe if the function pointer points to a perfectly matching
    ///     function as the default)
    pub unsafe fn link(&mut self, fn_name: &str, ptr: *const c_void) {
        unsafe {
            match fn_name {
                "abort" => self.abort = mem::transmute(ptr),
                "log_info" => self.log_info = mem::transmute(ptr),
                "log_warn" => self.log_warn = mem::transmute(ptr),
                "log_critical" => self.log_critical = mem::transmute(ptr),
                "log_debug" => self.log_debug = mem::transmute(ptr),
                "free_cs_string" => self.free_cs_string = mem::transmute(ptr),
                _ => {}
            }
        }
    }

}

impl Default for CsFns {
    fn default() -> Self {
        Self::new()
    }
}



#[derive(Default)]
pub struct TuringState {
    pub wasm: Option<WasmInterpreter>,
    pub wasm_fns: HashMap<String, (*const c_void, Vec<u32>, Vec<u32>)>,
    pub param_builders: TrackedHashMap<Params>,
    pub active_builder: u32,
    pub active_wasm_fn: Option<String>,
    pub opaque_pointers: TrackedHashMap<*const c_void>,
    pub pointer_backlink: HashMap<*const c_void, u32>,
    pub str_cache: VecDeque<String>
}

static mut STATE: Option<RefCell<TuringState>> = None;
static mut CSFNS: Option<RefCell<CsFns>> = None;

const TURING_UNINIT: &str = "Turing has not been initialized";

type FfiCallback = extern "C" fn(u32) -> FfiParam;


trait IntoWasm<T> {
    fn into_wasm(self) -> Result<T, wasmtime::Error>;
}

impl<T, E> IntoWasm<T> for Result<T, E>
where
    E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
{
    fn into_wasm(self) -> Result<T, wasmtime::Error> {
        self.map_err(|e| wasmtime::Error::msg(format!("{}", e)))
    }
}


fn get_string(message: u32, data: &[u8]) -> String {
    let mut output_string = String::new();
    for i in message..u32::MAX {
        let byte: &u8 = data.get(i as usize).unwrap();
        if *byte == 0u8 { break }
        output_string.push(char::from(*byte));
    }
    output_string
}

fn write_string(pointer: u32, string: String, memory: &Memory, caller: Caller<'_, WasiP1Ctx>) {
    let string = CString::new(string).unwrap();
    let string = string.into_bytes_with_nul();
    memory.write(caller, pointer as usize, &string);
}

impl TuringState {
    pub fn new() -> Self {
        Self {
            wasm: None,
            wasm_fns: HashMap::new(),
            param_builders: TrackedHashMap::starting_at(1),
            active_builder: 0,
            active_wasm_fn: None,
            opaque_pointers: TrackedHashMap::starting_at(1),
            pointer_backlink: HashMap::new(),
            str_cache: VecDeque::new(),
        }
    }

    pub fn push_param(&mut self, param: Param) -> Result<()> {
        if let Some(builder) = self.param_builders.get_mut(&self.active_builder) {
            builder.push(param);
            Ok(())
        } else {
            Err(anyhow!("param object does not exist"))
        }
    }

    pub fn set_param(&mut self, index: u32, value: Param) -> Result<()> {
        if let Some(builder) = self.param_builders.get_mut(&self.active_builder) {
            if builder.len() > index {
                builder.set(index, value);
                Ok(())
            } else if builder.len() == index {
                builder.push(value);
                Ok(())
            } else {
                Err(anyhow!("Index out of bounds"))
            }
        } else {
            Err(anyhow!("param object does not exist"))
        }
    }

    pub fn read_param(&self, index: u32) -> Param {
        if let Some(builder) = self.param_builders.get(&self.active_builder) {
            if index >= builder.len() {
                Param::Error("Index out of bounds".to_string())
            } else if let Some(val) = builder.get(index as usize) {
                val.clone()
            } else {
                Param::Error("Could not read param for unknown reason".to_string())
            }
        } else {
            Param::Error("param object does not exist".to_string())
        }
    }

    pub fn swap_params(&mut self, params: u32) -> Result<Params> {
        let mut p = Params::new();
        if let Some(p) = self.param_builders.swap(&params, p) {
            Ok(p)
        } else {
            Err(anyhow!("Params object does not exist"))
        }
    }

    pub fn bind_wasm(&mut self, engine: &Engine, linker: &mut Linker<WasiP1Ctx>) {
        unsafe {
            // Utility Functions

            // Should only be used in 2 situations:
            // 1. after a call to a function that "retuns" a string, the guest
            //    is required to allocate the size returned in place of the string, and then
            //    call this, passing the allocated pointer and the size.
            //    If the size passed in does not exactly match the cached string, or there is no
            //    cached string, then 0 is returned, otherwise the input pointer is returned.
            // 2. for each argument of a function that expects a string, in linear order,
            //    failing to retrieve all param strings in the correct order will invalidate
            //    the strings with no way to recover.
            linker.func_new("env", "retrieve_string",
                FuncType::new(engine, vec![ValType::I32, ValType::I32], vec![ValType::I32]),
                |mut caller, ps, rs| -> Result<(), wasmtime::Error> {
                    let ptr = ps[0].i32().unwrap();
                    let size = ps[1].i32().unwrap();
                    unsafe {
                        if let Some(state) = &mut STATE {
                            let mut s = state.borrow_mut();
                            if let Some(st) = s.str_cache.pop_front() {
                                if st.len() + 1 == size as usize {
                                    if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
                                        write_string(ptr as u32, st, &memory, caller);
                                        rs[0] = Val::I32(ptr);
                                    }
                                    return Ok(())
                                }
                            }
                            rs[0] = Val::I32(0);
                            Ok(())
                        } else {
                            unreachable!("wasm can't be called if state doesn't exist");
                        }
                    }
                }
            ).unwrap();



            // C# wasm bindings
            let fns;
            {
                fns = self.wasm_fns.clone();
            }

            for (n, (func_ptr, p, r)) in fns.iter() {

                let func: FfiCallback = mem::transmute(func_ptr);

                let mut p_types = Vec::new();
                let mut r_type = Vec::new();
                for pt in p {
                    let p_type = match pt {
                        1 | 2 | 3 | 4 | 5 | 6 | 8 | 9 | 10 => ValType::I32,
                        7 => ValType::F32,
                        _ => unreachable!("invalid parameter type")
                    };
                    p_types.push(p_type);
                }
                if !r.is_empty() {
                    let r_typ = match r[0] {
                        1 | 2 | 3 | 4 | 5 | 6 | 8 | 9 | 10 => ValType::I32,
                        7 => ValType::F32,
                        _ => unreachable!("invalid return type")
                    };
                    r_type.push(r_typ);
                }

                let ft = FuncType::new(engine, p_types, r_type);

                let p = p.clone();
                let r = r.clone();

                linker.func_new("env", n.clone().as_str(), ft, move |mut caller, ps, rs| -> Result<(), wasmtime::Error> {
                    let mut params = Params::new();

                    // set up function parameters
                    for (exp_typ, value) in p.iter().zip(ps) {
                        match (exp_typ, value) {
                            (1, Val::I32(i)) => params.push(Param::I8(*i as i8)),
                            (2, Val::I32(i)) => params.push(Param::I16(*i as i16)),
                            (3, Val::I32(i)) => params.push(Param::I32(*i)),
                            (4, Val::I32(u)) => params.push(Param::U8(*u as u8)),
                            (5, Val::I32(u)) => params.push(Param::U16(*u as u16)),
                            (6, Val::I32(u)) => params.push(Param::U32(*u as u32)),
                            (7, Val::F32(f)) => params.push(Param::F32(f32::from_bits(*f))),
                            (8, Val::I32(b)) => params.push(Param::Bool(*b != 0)),
                            (9, Val::I32(ptr)) => {
                                if let Some(state) = &mut STATE {
                                    let ptr = *ptr as u32;
                                    let s = state.borrow_mut();

                                    if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                                        let s = get_string(ptr, memory.data(&caller));
                                        params.push(Param::String(s));
                                    } else {
                                        return Err(anyhow!("wasm does not export memory")).into_wasm();
                                    }

                                } else {
                                    return Err(anyhow!("if you are reading this, something has gone horribly wrong")).into_wasm();
                                }
                            },
                            (10, Val::I32(p)) => {
                                let p = *p as u32;
                                if let Some(state) = &mut STATE {
                                    let s = state.borrow_mut();
                                    if let Some(true_pointer) = s.opaque_pointers.get(&p) {
                                        params.push(Param::Object(*true_pointer));
                                    } else {
                                        return Err(anyhow!("opaque pointer does not correspond to a real pointer")).into_wasm();
                                    }
                                }
                            }
                            _ => params.push(Param::Error("Mismatched parameter type".to_string()))
                        }
                    }

                    if let Some(state) = &mut STATE {
                        let mut s = state.borrow_mut();

                        let pid = s.param_builders.add(params);

                        let res = func(pid).to_param().into_wasm()?;

                        let rv = match res {
                            Param::I8(i)  => Val::I32(i as i32),
                            Param::I16(i) => Val::I32(i as i32),
                            Param::I32(i) => Val::I32(i),
                            Param::U8(u)  => Val::I32(u as i32),
                            Param::U16(u) => Val::I32(u as i32),
                            Param::U32(u) => Val::I32(u as i32),
                            Param::F32(f) => Val::F32(f.to_bits()),
                            Param::Bool(b) => Val::I32(if b { 1 } else { 0 }),
                            Param::String(st) => {
                                let l = st.len() + 1;
                                s.str_cache.push_back(st);
                                Val::I32(l as i32)
                            },
                            Param::Object(p) => {
                                let opaque = if let Some(opaque) = s.pointer_backlink.get(&p) {
                                    *opaque
                                } else {
                                    let op = s.opaque_pointers.add(p);
                                    s.pointer_backlink.insert(p, op);
                                    op
                                };
                                Val::I32(opaque as i32)
                            },
                            _ => return Err(anyhow!("Invalid return value")).into_wasm()
                        };

                    }

                    Ok(())

                }).unwrap();

            }

        }
    }

}



// Export functions

// Core systems
#[unsafe(no_mangle)]
pub extern "C" fn init_turing() {
    unsafe {
        STATE = Some(RefCell::new(TuringState::new()));
        CSFNS = Some(RefCell::new(CsFns::new()));
    }

}


#[unsafe(no_mangle)]
/// starts building a new wasm function. May return an error
/// # Safety
/// only safe if name: *const c_char points at a valid string
pub unsafe extern "C" fn create_wasm_fn(name: *const c_char, pointer: *const c_void) -> FfiParam {
    unsafe {
        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();

            if s.wasm_fns.contains_key(&name) {
                Param::Error(format!("wasm fn is already defined: '{}'", name))
            } else {
                s.active_wasm_fn = Some(name.clone());
                s.wasm_fns.insert(name, (pointer, Vec::new(), Vec::new()));
                Param::Void
            }
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}

#[unsafe(no_mangle)]
/// appends a parameter type to the specified wasm fn builder, types are identical to the ids used
/// for FfiParam
pub extern "C" fn add_wasm_fn_param_type(param_type: u32) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            if s.wasm_fns.is_empty() || s.active_wasm_fn.is_none() {
                Param::Error("no wasm function to add parameter type to".to_string())
            } else if (1..=10).contains(&param_type) {
                let active = s.active_wasm_fn.as_ref().unwrap().clone();
                let fn_builder = s.wasm_fns.get_mut(&active).unwrap();
                fn_builder.1.push(param_type);
                Param::Void
            } else {
                Param::Error(format!("Invalid param type id: {}", param_type))
            }
        }
        else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}

#[unsafe(no_mangle)]
/// sets the return type of the specified wasm fn builder
pub extern "C" fn set_wasm_fn_return_type(return_type: u32) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            if s.wasm_fns.is_empty() || s.active_wasm_fn.is_none() {
                Param::Error("no wasm function to add parameter type to".to_string())
            } else if (1..=10).contains(&return_type) {
                let active = s.active_wasm_fn.as_ref().unwrap().clone();
                let fn_builder = s.wasm_fns.get_mut(&active).unwrap();
                fn_builder.2.push(return_type);
                Param::Void
            } else {
                Param::Error(format!("Invalid param type id: {}", return_type))
            }
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}


#[unsafe(no_mangle)]
/// Takes all registered wasm functions, generates their wasm code, and then starts the wasm engine
pub extern "C" fn init_wasm() -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let interp = {
                let mut s = state.borrow_mut();
                WasmInterpreter::new(&mut s).ok()
            };
            let mut s = state.borrow_mut();
            if let Some(t) = interp {
                s.wasm = Some(t);
                Param::Void
            } else {
                Param::Error("Failed to initialize wasm engine".to_string())
            }
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}



// Params

#[unsafe(no_mangle)]
/// Creates a param builder and returns it's uid
pub extern "C" fn create_params() -> u32 {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let x = s.param_builders.add(Params::new());
            s.active_builder = x;
            x
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_n_params(size: u32) -> u32 {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let x = s.param_builders.add(Params::of_size(size));
            s.active_builder = x;
            x
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bind_params(params: u32) {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            s.active_builder = params;
        }
    }
}

#[unsafe(no_mangle)]
/// Returns an FfiParam that will either be Error or Void
/// Strings (from error and string) are copied and you are safe to free them after calling.
pub extern "C" fn add_param(value: FfiParam) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let typ_id = value.type_id;
            if let Ok(val) = value.to_param() {
                if let Err(e) = s.push_param(val) {
                    Param::Error(e.to_string())
                } else {
                    Param::Void
                }
            } else {
                Param::Error(format!("Failed to add parameter. Invalid type id: {}", typ_id))
            }
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}

#[unsafe(no_mangle)]
/// Returns an error if the index is out of bounds
/// Strings (from error and string) are copied and you are safe to free them after calling.
pub extern "C" fn set_param(index: u32, value: FfiParam) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let typ_id = value.type_id;
            if let Ok(val) = value.to_param() {
                if let Err(e) = s.set_param(index, val) {
                    Param::Error(e.to_string())
                } else {
                    Param::Void
                }
            } else {
                Param::Error(format!("Failed to set parameter. Invalid type id: {}", typ_id))
            }
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn read_param(index: u32) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let s = state.borrow_mut();
            s.read_param(index)
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn delete_params(params: u32) {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            s.param_builders.remove(&params);
            if s.active_builder == params {
                s.active_builder = 0;
            }
        }
    }
}

#[unsafe(no_mangle)]
/// Calls a function passing it the specified params object. The underlying params object is
/// NOT deleted, but its contents are.
/// If params is 0, calls with an empty parameters object
pub unsafe extern "C" fn call_wasm_fn(name: *const c_char, params: u32, expected_return_type: u32) -> FfiParam {
    let expected_return_type = if expected_return_type == 0 {
        param_type::VOID
    } else {
        expected_return_type
    };
    if !(0..=12u32).contains(&expected_return_type) {
        return Param::Error(format!("Invalid return type: {}", expected_return_type)).into();
    }
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let params = if params == 0 {
                Params::new()
            } else if let Ok(p) = s.swap_params(params) {
                p
            } else{
                return Param::Error("Params object does not exist".to_string()).into();
            };
            if let Some(mut wasm) = s.wasm.take() {
                let name = CStr::from_ptr(name).to_string_lossy().to_string();
                let res = wasm.call_fn(&name, params, &mut s, expected_return_type);
                s.wasm = Some(wasm);
                res
            } else {
                Param::Error("Wasm engine is not initialized".to_string())
            }
        } else {
            Param::Error(TURING_UNINIT.to_string())
        }
    }.into()
}



#[unsafe(no_mangle)]
/// Frees a rust-allocated C string.
/// # Safety 
/// dereferencing raw pointers is fun.
pub unsafe extern "C" fn free_string(ptr: *mut c_char) {
    unsafe { free_cstr(ptr) };
}


#[unsafe(no_mangle)]
unsafe extern "C" fn register_function(name: *const c_char, pointer: *const c_void) {
    unsafe {
        let cstr = CStr::from_ptr(name).to_string_lossy().to_string();
        if let Some(csf) = &mut CSFNS {
            let mut cs = csf.borrow_mut();
            cs.link(&cstr, pointer);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn load_script(source: *const c_char) -> FfiParam {
    unsafe {
        let source = CStr::from_ptr(source).to_string_lossy().to_string();

        let source = path::Path::new(&source);

        if let Err(e) = source.metadata() {
            Param::Error(format!("Script does not exist: {:#?}, {:#?}", source.to_str(), e))
        } else {
            if let Some(state) = &mut STATE {
                let mut s = state.borrow_mut();
                if let Some(wasm) = &mut s.wasm {
                    if let Ok(_) = wasm.load_script(source) {
                        Param::Void
                    } else {
                        Param::Error("Failed to instantiate wasm module".to_string())
                    }
                } else {
                    Param::Error("Wasm engine not initialized".to_string())
                }
            } else {
                Param::Error(TURING_UNINIT.to_string())
            }
        }

    }.into()
}



pub struct Log {}
macro_rules! mlog {
    ($f:tt => $msg:tt ) => {
        unsafe {
            if let Some(csf) = &mut CSFNS {
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







