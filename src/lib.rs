#![allow(static_mut_refs, clippy::new_without_default)]

pub mod wasm;
pub mod interop;
pub mod util;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::mem;

use anyhow::{anyhow, Result};
use wasmi::core::ValType;
use wasmi::{Caller, ExternRef, FuncType, Linker, Memory, Val, F32};

use crate::wasm::wasm_engine::WasmInterpreter;

use crate::interop::params::Params;

use self::interop::params::{FfiParam, Param};
use self::util::{free_cstr, ToCStr, TrackedHashMap};

#[derive(Default)]
pub struct TuringState {
    pub wasm: Option<WasmInterpreter>,
    pub wasm_fns: HashMap<String, (*const c_void, Vec<u32>, Vec<u32>)>,
    pub param_builders: TrackedHashMap<Params>,
    pub active_builder: u32,
    pub active_wasm_fn: Option<String>,
    pub opaque_pointers: TrackedHashMap<*const c_void>,
}

static mut STATE: Option<RefCell<TuringState>> = None;

const TURING_UNINIT: &str = "Turing has not been initialized";

type FfiCallback = extern "C" fn(u32) -> FfiParam;

trait IntoWasmi<T> {
    fn into_wasmi(self) -> Result<T, wasmi::Error>;
}

impl<T, E> IntoWasmi<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn into_wasmi(self) -> Result<T, wasmi::Error> {
        self.map_err(|e| wasmi::Error::new(e.into().to_string()))
    }
}



fn get_string(message: u32, memory: &Memory, caller: &Caller<'_, ()>) -> String {
    let mut output_string = String::new();
    for i in message..u32::MAX {
        let byte: &u8 = memory.data(caller).get(i as usize).unwrap();
        if *byte == 0u8 { break }
        output_string.push(char::from(*byte));
    }
    output_string
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
                Param::Error("Index out of bounds".to_cstr_ptr())
            } else if let Some(val) = builder.get(index as usize) {
                val
            } else {
                Param::Error("Could not read param for unknown reason".to_cstr_ptr())
            }
        } else {
            Param::Error("param object does not exist".to_cstr_ptr())
        }
    }

    pub fn bind_wasm(&mut self, linker: &mut Linker<()>) {
        unsafe {
            if let Some(state) = &mut STATE {

                let mut fns = HashMap::new();
                {
                    let mut s = state.borrow_mut();
                    mem::swap(&mut fns, &mut s.wasm_fns);
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

                    let ft = FuncType::new(p_types, r_type);

                    let p = p.clone();
                    let r = r.clone();

                    linker.func_new("env", n.clone().as_str(), ft, move |mut caller, ps, rs| -> Result<(), wasmi::Error> {
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
                                (7, Val::F32(f)) => params.push(Param::F32(f.to_float())),
                                (8, Val::I32(b)) => params.push(Param::Bool(*b != 0)),
                                (9, Val::I32(ptr)) => {
                                    if let Some(state) = &mut STATE {
                                        let ptr = *ptr as u32;
                                        let s = state.borrow_mut();

                                        if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                                            let s = get_string(ptr, &memory, &caller);
                                            params.push(Param::String(s.to_cstr_ptr()));
                                        } else {
                                            return Err(anyhow!("wasm does not export memory")).into_wasmi();
                                        }

                                    } else {
                                        return Err(anyhow!("if you are reading this, something has gone horribly wrong")).into_wasmi();
                                    }
                                },
                                (10, Val::I32(p)) => {
                                    let p = *p as u32;
                                    if let Some(state) = &mut STATE {
                                        let s = state.borrow_mut();
                                        if let Some(true_pointer) = s.opaque_pointers.get(&p) {
                                            params.push(Param::Object(*true_pointer));
                                        } else {
                                            return Err(anyhow!("opaque pointer does not correspond to a real pointer")).into_wasmi();
                                        }
                                    }
                                }
                                _ => params.push(Param::Error("Mismatched parameter type".to_cstr_ptr()))
                            }
                        }

                        if let Some(state) = &mut STATE {
                            let mut s = state.borrow_mut();

                            let pid = s.param_builders.add(params);

                            let res = func(pid).to_param().into_wasmi()?;

                            let rv = match res {
                                Param::I8(i)  => Val::I32(i as i32),
                                Param::I16(i) => Val::I32(i as i32),
                                Param::I32(i) => Val::I32(i),
                                Param::U8(u)  => Val::I32(u as i32),
                                Param::U16(u) => Val::I32(u as i32),
                                Param::U32(u) => Val::I32(u as i32),
                                Param::F32(f) => Val::F32(F32::from_float(f)),
                                Param::Bool(b) => Val::I32(if b { 1 } else { 0 }),
                                Param::String(s) => {},
                                Param::Object(p) => {},
                                _ => return Err(anyhow!("Invalid return value")).into_wasmi()
                            };


                        }



                        Ok(())

                    }).unwrap();

                }

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
                Param::Error(format!("wasm fn is already defined: '{}'", name).to_cstr_ptr())
            } else {
                s.active_wasm_fn = Some(name.clone());
                s.wasm_fns.insert(name, (pointer, Vec::new(), Vec::new()));
                Param::Void
            }
        } else {
            Param::Error(TURING_UNINIT.to_cstr_ptr())
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
                Param::Error("no wasm function to add parameter type to".to_cstr_ptr())
            } else if (1..=10).contains(&param_type) {
                let active = s.active_wasm_fn.as_ref().unwrap().clone();
                let fn_builder = s.wasm_fns.get_mut(&active).unwrap();
                fn_builder.1.push(param_type);
                Param::Void
            } else {
                Param::Error(format!("Invalid param type id: {}", param_type).to_cstr_ptr())
            }
        }
        else {
            Param::Error(TURING_UNINIT.to_cstr_ptr())
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
                Param::Error("no wasm function to add parameter type to".to_cstr_ptr())
            } else if (1..=10).contains(&return_type) {
                let active = s.active_wasm_fn.as_ref().unwrap().clone();
                let fn_builder = s.wasm_fns.get_mut(&active).unwrap();
                fn_builder.2.push(return_type);
                Param::Void
            } else {
                Param::Error(format!("Invalid param type id: {}", return_type).to_cstr_ptr())
            }
        } else {
            Param::Error(TURING_UNINIT.to_cstr_ptr())
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
                Param::Error("Failed to initialize wasm engine".to_cstr_ptr())
            }
        } else {
            Param::Error(TURING_UNINIT.to_cstr_ptr())
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
pub extern "C" fn add_param(value: FfiParam) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let typ_id = value.type_id;
            if let Ok(val) = value.to_param() {
                if let Err(e) = s.push_param(val) {
                    Param::Error(e.to_string().to_cstr_ptr())
                } else {
                    Param::Void
                }
            } else {
                Param::Error(format!("Failed to add parameter. Invalid type id: {}", typ_id).to_cstr_ptr())
            }
        } else {
            Param::Error(TURING_UNINIT.to_cstr_ptr())
        }
    }.into()
}

#[unsafe(no_mangle)]
/// Returns an error if the index is out of bounds
pub extern "C" fn set_param(index: u32, value: FfiParam) -> FfiParam {
    unsafe {
        if let Some(state) = &mut STATE {
            let mut s = state.borrow_mut();
            let typ_id = value.type_id;
            if let Ok(val) = value.to_param() {
                if let Err(e) = s.set_param(index, val) {
                    Param::Error(e.to_string().to_cstr_ptr())
                } else {
                    Param::Void
                }
            } else {
                Param::Error(format!("Failed to set parameter. Invalid type id: {}", typ_id).to_cstr_ptr())
            }
        } else {
            Param::Error(TURING_UNINIT.to_cstr_ptr())
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
            Param::Error(TURING_UNINIT.to_cstr_ptr())
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
/// Frees a rust-allocated C string.
/// # Safety 
/// dereferencing raw pointers is fun.
pub unsafe extern "C" fn free_string(ptr: *mut c_char) {
    unsafe { free_cstr(ptr) };
}




// Import functions
unsafe extern "C" {
    /// Called when things go so horribly wrong that proper recovery is not possible
    pub fn abort(error_code: *const c_char, error_message: *const c_char) -> !;
    pub fn log_info(message: *const c_char);
    pub fn log_warn(message: *const c_char);
    pub fn log_error(message: *const c_char);
    pub fn log_debug(message: *const c_char);

}

// rust-local wrappers

pub struct Log {}
macro_rules! mlog {
    ($func:tt : $msg:tt ) => {
        let s = $msg.to_string();
        let s = CString::new(s).unwrap();
        let ptr = s.as_ptr();
        unsafe {
            $func(ptr);
        }
    };
}
impl Log {

    pub fn info(msg: impl ToString) {
        mlog!(log_info: msg);
    }

    pub fn warn(msg: impl ToString) {
        mlog!(log_warn: msg);
    }

    pub fn error(msg: impl ToString) {
        mlog!(log_error: msg);
    }

    pub fn debug(msg: impl ToString) {
        mlog!(log_debug: msg);
    }

}


