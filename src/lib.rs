mod wasm;
mod data;
mod interop;
pub mod dynamic_function_builder;

use std::collections::HashMap;
use data::game_objects::*;
use std::ffi::{CStr, CString};
use std::{mem, slice};
use std::os::raw::{c_char, c_void};
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicPtr;
use wasmi::{Caller, Engine, ExternRef, FuncType, Linker, Store, Val};
use wasmi::core::ValType;
use crate::interop::parameters::*;
use crate::interop::parameters::params::Parameters;
use dynamic_function_builder::function_gen::generate;
use crate::wasm::wasm_interpreter::{HostState, WasmInterpreter};
use anyhow::{anyhow, Result};

type F = dyn Fn(Caller<'_, HostState<ExternRef>>, &[Val], &mut [Val]) -> Result<(), wasmi::Error>;

static mut WASM_INTERPRETER: Option<WasmInterpreter> = None;
static mut WASM_FNS: Option<HashMap<String, (FuncType, Box<F>)>> = None;

pub unsafe fn bind_data(engine: &Engine, store: &mut Store<HostState<ExternRef>>, linker: &mut Linker<HostState<ExternRef>>) -> Result<()> {
   if WASM_FNS.is_some() {
       let mut fns = None;
       mem::swap(&mut WASM_FNS, &mut fns);
       let mut fns = fns.unwrap();
       for f in &fns {

           let a0 = &f.0;
           let b0 = &f.1.0;

           linker.func_new("env", a0, b0.clone(), |a, b, c| {
               if WASM_FNS.is_some() {
                   let mut fns = None;
                   mem::swap(&mut WASM_FNS, &mut fns);
                   let mut fns = fns.unwrap();
                   let pair = fns.get(a0.as_str()).unwrap();
                   let res = pair.1(a,b,c);
                   mem::swap(&mut WASM_FNS, &mut Some(fns));
                   res
               } else {
                   Ok(())
               }
           })?;
       }

       mem::swap(&mut WASM_FNS, &mut Some(fns));

   }
   Ok(())
}

type CsMethod = extern "C" fn(CParams) -> CParams;


lazy_static::lazy_static! {
    static ref FUNCTION_MAP: Mutex<HashMap<String, std::sync::Arc<AtomicPtr<c_void>>>> = Mutex::new(HashMap::new());
}


extern "C" {
    pub fn abort(error_code: *const std::ffi::c_char, error_message: *const std::ffi::c_char) -> !;
}

#[macro_export]
macro_rules! throw {
    ( $code:expr, $msg:expr ) => {
        {
            let c = CString::new($code).unwrap();
            let m = CString::new($msg).unwrap();
            unsafe { abort(c.as_ptr(), m.as_ptr()) };
        }
    };
}

pub fn call_cs(name: &str, params: CParams) -> CParams {
    let map = FUNCTION_MAP.lock().unwrap();
    if let Some(arc) = map.get(name) {
        let raw_ptr = arc.load(std::sync::atomic::Ordering::SeqCst);
        let callback: CsMethod = unsafe { mem::transmute(raw_ptr) };

        callback(params)
    } else {
        throw!("Not Defined", format!("Function '{}' not found", name));
    }
}


#[repr(C)]
struct ParamTypes {
    count: u32,
    array: *const u32
}


fn to_param_type(p: &u32) -> ValType {
    match p {
        2 => ValType::I32,
        3 => ValType::I64,
        8 => ValType::F32,
        9 => ValType::F64,
        100..199 => ValType::ExternRef,
        200 => ValType::FuncRef,
        _ => {
            throw!("Invalid Type", format!("Type {p} is not recognized"));
        }
    }
}

#[no_mangle]
unsafe extern "C" fn generate_wasm_fn(name: *const std::ffi::c_char, func_ptr: *mut std::ffi::c_void, params_types: ParamTypes, return_types: ParamTypes) {

    let func_ptr_arc = std::sync::Arc::new(AtomicPtr::new(func_ptr));
    let name_cstr = CStr::from_ptr(name);
    let name = name_cstr.to_string_lossy().to_string();

    {
        let mut map = crate::FUNCTION_MAP.lock().unwrap();
        map.insert(name.to_string(), func_ptr_arc);
    }

    let mut pts = Vec::new();
    let mut rts = Vec::new();

    let ptp_array = slice::from_raw_parts(params_types.array, params_types.count as usize);
    let rtp_array = slice::from_raw_parts(return_types.array, return_types.count as usize);

    for p in ptp_array {
        pts.push(to_param_type(p));
    }

    for p in rtp_array {
        rts.push(to_param_type(p));
    }

    let binding = generate(&name, pts, rts);

    if WASM_FNS.is_none() {
        WASM_FNS = Some(HashMap::new());
    }

    if let Some(fns) = &mut WASM_FNS {
        fns.insert(name, binding);
    }

}


//////////////////////////////////////////////////////
// Functions that c#/c++ calls are defined here


#[no_mangle]
pub unsafe extern "C" fn initialize_wasm() {
    WASM_INTERPRETER = Some(WasmInterpreter::new());

    println!("Initialized wasm interpreter");
}

#[no_mangle]
pub unsafe extern "C" fn free_params(params: CParams) {
    Parameters::free_cs(params);
}

#[no_mangle]
pub unsafe extern "C" fn load_script(str_ptr: *const c_char) {
    let cstr = CStr::from_ptr(str_ptr);
    let s = cstr.to_string_lossy().to_string();

    println!("Loading script: {}", s);

    if let Some(wasm) = &mut WASM_INTERPRETER {
        let res = wasm.load_script(&s);

        if let Err(e) = res {
            throw!("Script Loading Error", e.to_string())
        }
    } else {
        throw!("Critical Error", "WASM interpreter is not loaded")
    }

}

#[no_mangle]
pub unsafe extern "C" fn call_script_function(str_ptr: *const c_char, params: CParams) {
    let cstr = CStr::from_ptr(str_ptr);
    let s = cstr.to_string_lossy().to_string();
    if let Some(wasm) = &mut WASM_INTERPRETER {
        let res = wasm.call_void_method(&s, Parameters::unpack(&params));

        if let Err(e) = res {
            throw!("Script Call Error", e.to_string())
        }

    } else {
        throw!("Critical Error", "WASM interpreter is not loaded")
    }

}
