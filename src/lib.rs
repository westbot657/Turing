#![allow(static_mut_refs)]

pub mod wasm;
pub mod interop;
pub mod util;

use std::cell::RefCell;
use std::collections::HashMap;
use std::default;
use std::ffi::c_char;
use std::sync::{Mutex, OnceLock};

use wasmi::core::ValType;

use crate::wasm::wasm_engine::WasmInterpreter;

use crate::interop::params::Params;

use self::util::TrackedHashMap;


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
pub extern "C" fn create_params() -> u32 {
    unsafe {
        if let Some(state) = &mut STATE {
            state.borrow_mut().param_builders.add(Params::new())
        } else {
            0
        }
    }
}









// Import functions
unsafe extern "C" {
    pub fn abort(error_code: *const c_char, error_message: *const c_char) -> !;
}




