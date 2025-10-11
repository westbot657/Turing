#![allow(static_mut_refs, clippy::new_without_default)]

pub mod wasm;
pub mod interop;
pub mod util;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::mem;

use anyhow::{anyhow, Result};
use wasmi::core::ValType;
use wasmi::{ExternRef, FuncType, Linker};

use crate::wasm::wasm_engine::WasmInterpreter;

use crate::interop::params::Params;

use self::interop::params::{FfiParam, Param};
use self::util::{free_cstr, ToCStr, TrackedHashMap};
use self::wasm::wasm_engine::{HostState, WasmFnBuilder};

#[derive(Default)]
pub struct TuringState {
    pub wasm: Option<WasmInterpreter>,
    pub wasm_fns: HashMap<String, (Vec<ValType>, Vec<ValType>)>,
    pub param_builders: TrackedHashMap<Params>,
    pub active_builder: u32,
    pub active_wasm_fn: Option<WasmFnBuilder>,
}

static mut STATE: Option<RefCell<TuringState>> = None;

const TURING_UNINIT: &str = "Turing has not been initialized";

impl TuringState {
    pub fn new() -> Self {
        Self {
            wasm: None,
            wasm_fns: HashMap::new(),
            param_builders: TrackedHashMap::starting_at(1),
            active_builder: 0,
            active_wasm_fn: None,
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

    pub fn bind_wasm(&mut self, linker: &mut Linker<HostState<ExternRef>>) {
        unsafe {
            if let Some(state) = &mut STATE {
                let mut s = state.borrow_mut();

                let mut fns = HashMap::new();
                mem::swap(&mut fns, &mut s.wasm_fns);

                for (n, (p, r)) in fns.iter() {
                    let ft = FuncType::new(p.clone(), r.clone());

                    linker.func_new("env", n.clone().as_str(), ft, move |mut caller, ps, rs| -> Result<(), wasmi::Error> {

                    })

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
pub extern "C" fn create_wasm_fn(name: *const c_char) -> FfiParam {
    Param::Void.into()
}

#[unsafe(no_mangle)]
/// builds the wasm fn builder and adds it to the list of complete wasm fns
pub extern "C" fn finalize_wasm_fn() -> FfiParam {
    Param::Void.into()
}

#[unsafe(no_mangle)]
/// appends a parameter type to the specified wasm fn builder, types are identical to the ids used
/// for FfiParam
pub extern "C" fn add_wasm_fn_param_type(param_type: u32) -> FfiParam {
    Param::Void.into()
}

#[unsafe(no_mangle)]
/// sets the return type of the specified wasm fn builder
pub extern "C" fn set_wasm_fn_return_type(return_type: u32) -> FfiParam {
    Param::Void.into()
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


