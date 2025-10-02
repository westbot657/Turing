#![allow(static_mut_refs)]

pub mod wasm;
pub mod interop;
pub mod util;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::default;
use std::ffi::{c_char, CStr, CString};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
use wasmi::core::ValType;

use crate::wasm::wasm_engine::WasmInterpreter;

use crate::interop::params::Params;

use self::interop::params::{FfiParam, Param};
use self::util::{free_cstr, ToCStr, TrackedHashMap};


struct TuringState {
    pub wasm: Option<WasmInterpreter>,
    pub wasm_fns: HashMap<String, (Vec<ValType>, Vec<ValType>)>,
    pub param_builders: TrackedHashMap<Params>,
    pub active_builder: u32,
}

static mut STATE: Option<RefCell<TuringState>> = None;


impl TuringState {
    pub fn new() -> Self {
        Self {
            wasm: None,
            wasm_fns: HashMap::new(),
            param_builders: TrackedHashMap::starting_at(1),
            active_builder: 0,
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
pub extern "C" fn init_wasm() {
    unsafe {
        if let Some(state) = &mut STATE {
            state.borrow_mut().wasm = Some(WasmInterpreter::new());
        }
    }
}



// Params

#[unsafe(no_mangle)]
pub extern "C" fn create_params() -> u32 { // TODO: add create_n_params(size) to pre-allocate the Vec
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
                if let Err(res) = s.push_param(val) {
                    Param::Error(res.to_string().to_cstr_ptr())
                } else {
                    Param::Void
                }
            } else {
                Param::Error(format!("Failed to add parameter. Invalid type id: {}", typ_id).to_cstr_ptr())
            }
        } else {
            Param::Error("Turing has not been initialized".to_cstr_ptr())
        }
    }.into()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free_string(ptr: *mut c_char) {
    unsafe { free_cstr(ptr) };
}



// Import functions
unsafe extern "C" {
    /// Called when things go so horribly wrong that proper recovery is not possible
    pub fn abort(error_code: *const c_char, error_message: *const c_char) -> !;




}




