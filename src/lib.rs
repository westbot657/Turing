mod wasm;
mod data;
mod interop;

use std::collections::HashMap;
use data::game_objects::*;
use std::ffi::{CStr, CString};
use std::{mem, slice};
use std::mem::ManuallyDrop;
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::AtomicPtr;
use wasmi::{AsContextMut, Caller, Engine, ExternRef, FuncType, Linker, Store, Val};
use wasmi::core::ValType;
use crate::interop::parameters::*;
use crate::interop::parameters::params::Parameters;
use crate::wasm::wasm_interpreter::{HostState, WasmInterpreter};
use anyhow::{anyhow, Result};

type F = dyn Fn(Caller<'_, HostState<ExternRef>>, &[Val], &mut [Val]) -> Result<(), wasmi::Error>;

static mut WASM_INTERPRETER: Option<WasmInterpreter> = None;
static mut WASM_FNS: Option<HashMap<String, (Vec<ValType>, Vec<ValType>)>> = None;


macro_rules! push_parameter {
    ( $params:expr, $typ:ident: $obj:expr ) => {
        $params.push(crate::params::Param::$typ( crate::params::ParamData { $typ: std::mem::ManuallyDrop::new($obj) } ))
    };
}

macro_rules! get_return {
    ( $params:expr, $t:tt, $index:expr) => {
        {
            let raw = $params.params.remove($index);
            if raw.0 as u32 != crate::params::ParamType::$t as u32 {
                Err::<std::mem::ManuallyDrop<$t>, String>("wrong value type was returned".to_owned())
            }
            else {
                let p = crate::params::Param::$t(unsafe { raw.1 });
                match p {
                    crate::params::Param::$t(x) => Ok(unsafe { x.$t }),
                    _ => Err("wrong value type was returned".to_owned())
                }
            }

        }
    };
}

pub unsafe fn bind_data(linker: &mut Linker<HostState<ExternRef>>) -> Result<()> {
   if WASM_FNS.is_some() {
       let mut fns = None;
       mem::swap(&mut WASM_FNS, &mut fns);
       let mut fns = fns.unwrap();
       for f in fns {

           let name = f.0;
           let params = f.1.0;
           let results = f.1.1;

           let ft = FuncType::new(params.clone(), results.clone());

           linker.func_new("env", name.clone().as_str(), ft, move |mut caller, ps, rs| -> std::result::Result<(), wasmi::Error> {

               let mut p = Parameters::new();

               for i in 0..ps.len() {
                   let v = ps.get(i).unwrap();
                   let t = params.get(i);
                   if let Some(tp) = t {
                       match tp {
                           ValType::I32 => {
                               push_parameter!(p, i32: v.i32().unwrap());
                           }
                           ValType::I64 => {
                               push_parameter!(p, i64: v.i64().unwrap());
                           }
                           ValType::F32 => {
                               push_parameter!(p, f32: v.f32().unwrap().to_float());
                           }
                           ValType::F64 => {
                               push_parameter!(p, f64: v.f64().unwrap().to_float());
                           }
                           ValType::V128 => {
                               let code = CString::new("Unimplemented").unwrap();
                               let msg = CString::new("Param type V128 not handled").unwrap();
                               unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                           }
                           ValType::FuncRef => {
                               let code = CString::new("Unimplemented").unwrap();
                               let msg = CString::new("Param type FuncRef not handled").unwrap();
                               unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                           }
                           ValType::ExternRef => {
                               let obj = caller.data().get(v.i32().unwrap() as u32).unwrap();
                               let o = obj.data(&caller).unwrap().downcast_ref::<Object>().unwrap();
                               push_parameter!(p, Object: *o);
                           }
                       }
                   } else {
                       let code = CString::new("Argument Mismatch").unwrap();
                       let msg = CString::new(format!("Expected {} arguments, got {}", params.len(), ps.len())).unwrap();
                       unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                   }
               }


               let r = unsafe { call_cs(name.as_str(), p.pack()) };

               let mut r = unsafe { Parameters::unpack(&r) };

               for i in 0..results.len() {
                   let t = results.get(i).unwrap();
                   match t {
                       ValType::I32 => {
                           let x = get_return!(r, i32, i);
                           match x {
                               Ok(v) => {
                                   let v = ManuallyDrop::into_inner(v);
                                   let _ = rs.get(i).insert(&Val::I32(v));
                               }
                               Err(e) => {
                                   let code = CString::new("Return Type Mismatch").unwrap();
                                   let msg = CString::new(e).unwrap();
                                   unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                               }
                           }
                       }
                       ValType::I64 => {
                           let x = get_return!(r, i64, i);
                           match x {
                               Ok(v) => {
                                   let v = ManuallyDrop::into_inner(v);
                                   let _ = rs.get(i).insert(&Val::I64(v));
                               }
                               Err(e) => {
                                   let code = CString::new("Return Type Mismatch").unwrap();
                                   let msg = CString::new(e).unwrap();
                                   unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                               }
                           }
                       }
                       ValType::F32 => {
                           let x = get_return!(r, f32, i);
                           match x {
                               Ok(v) => {
                                   let v = ManuallyDrop::into_inner(v);
                                   let _ = rs.get(i).insert(&Val::F32(v.into()));
                               }
                               Err(e) => {
                                   let code = CString::new("Return Type Mismatch").unwrap();
                                   let msg = CString::new(e).unwrap();
                                   unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                               }
                           }
                       }
                       ValType::F64 => {
                           let x = get_return!(r, f64, i);
                           match x {
                               Ok(v) => {
                                   let v = ManuallyDrop::into_inner(v);
                                   let _ = rs.get(i).insert(&Val::F64(v.into()));
                               }
                               Err(e) => {
                                   let code = CString::new("Return Type Mismatch").unwrap();
                                   let msg = CString::new(e).unwrap();
                                   unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                               }
                           }
                       }
                       ValType::V128 => {
                           let code = CString::new("Unimplemented").unwrap();
                           let msg = CString::new("Return type V128 not handled").unwrap();
                           unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                       }
                       ValType::FuncRef => {
                           let code = CString::new("Unimplemented").unwrap();
                           let msg = CString::new("Return type FuncRef not handled").unwrap();
                           unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                       }
                       ValType::ExternRef => {
                           let x = get_return!(r, Object, i);
                           match x {
                               Ok(v) => {
                                   let v = ManuallyDrop::into_inner(v);
                                   let _ = rs.get(i).insert(&Val::ExternRef(ExternRef::new(&mut caller.as_context_mut(), v)));
                               }
                               Err(e) => {
                                   let code = CString::new("Return Type Mismatch").unwrap();
                                   let msg = CString::new(e).unwrap();
                                   unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                               }
                           }
                       }
                   }
               }

               Ok(())
           })?;
       }
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

    if WASM_FNS.is_none() {
        WASM_FNS = Some(HashMap::new());
    }

    if let Some(fns) = &mut WASM_FNS {
        fns.insert(name, (pts, rts));
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
