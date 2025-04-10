mod wasm;
mod data;
mod interop;

use std::collections::HashMap;
use data::game_objects::*;
use std::ffi::CStr;
use std::mem;
use std::os::raw::{c_char, c_void};
use std::sync::{Mutex};
use std::sync::atomic::AtomicPtr;
use anyhow::__private::kind::TraitKind;
use crate::interop::parameters::*;
use crate::interop::parameters::params::{ParamData, Parameters};
use crate::wasm::wasm_interpreter::WasmInterpreter;

static mut WASM_INTERPRETER: Option<WasmInterpreter> = None;

type CsMethod = extern "C" fn(CParams) -> CParams;

#[macro_export]
macro_rules! println {
    ( $st:literal ) => {
        print_out($st.to_string())
    };
    ( $st:literal, $($args:expr),* ) => {
        print_out(format!($st, $($args),*))
    }
}
macro_rules! print {
    ( $st:literal ) => {
        println!($st);
    };
    ( $st:literal, $($args:expr),* ) => {
        println!($st, $($args),*);
    }
}


lazy_static::lazy_static! {
    static ref FUNCTION_MAP: Mutex<HashMap<String, std::sync::Arc<AtomicPtr<c_void>>>> = Mutex::new(HashMap::new());
}


#[no_mangle]
unsafe extern "C" fn register_function(function_name: *const c_char, func_ptr: *mut c_void) {
    let func_ptr_arc = std::sync::Arc::new(AtomicPtr::new(func_ptr));
    let name_cstr = CStr::from_ptr(function_name);
    let name = name_cstr.to_string_lossy().to_string();

    let mut map = FUNCTION_MAP.lock().unwrap();
    map.insert(name.to_string(), func_ptr_arc);


}

macro_rules! extern_fn {
    (
        $cs_name: ident as
        $name:ident ( $( $arg:ident : $arg_ty:tt ),* ) $( -> $ret:tt )?
    ) => {
        #[no_mangle]
        pub unsafe fn $name( $( $arg : $arg_ty ),* ) $( -> $ret )? {
            let map = FUNCTION_MAP.lock().unwrap();
            if let Some(_macro_arc) = map.get(stringify!($cs_name)) {
                let _macro_raw_ptr = _macro_arc.load(std::sync::atomic::Ordering::SeqCst);
                let $cs_name: CsMethod = unsafe { mem::transmute(_macro_raw_ptr) };

                let mut params_in = crate::params::Parameters::new();

                $(
                    params_in.push(crate::params::Param::$arg_ty( crate::params::ParamData { $arg_ty: std::mem::ManuallyDrop::new($arg)}));
                )*

                let packed = params_in.pack();

                let res = $cs_name(packed);

                let mut result = unsafe { Parameters::unpack(&res) };

                $(
                if result.size() == 1 {
                    *get_return!(result, $ret, 0).unwrap()
                } else {
                    panic!("critical error in interop function {}/{}", stringify!($name), stringify!($cs_name))
                }
                )?

            } else {

                panic!("critical error in interop function {}/{}", stringify!($name), stringify!($cs_name));
            }

        }
    };
}

macro_rules! extern_fns {
    (
        $(
            $cs_name:ident as
            $name:ident ( $( $arg:ident : $arg_ty:tt ),* ) $( -> $ret:tt )?
        ),* $(,)?
    ) => {
        $(
            extern_fn! {
                $cs_name as
                $name ( $( $arg : $arg_ty ),* ) $( -> $ret )?
            }
        )*
    };
}

// structured as:
// <C# name> as <rust method signature>
extern_fns!(
    cs_print as print_out(msg: String),
    create_color_note as create_color_note(beat: f32) -> ColorNote,
    beatmap_add_color_note as beatmap_add_color_note(note: ColorNote),
);

macro_rules! error_param {
    ( $err_type:expr, $err:expr ) => {
        {
            let mut params = Parameters::new();
            let err = InteropError {
                error_type: $err_type.to_string(),
                message: $err.to_string(),
            };
            params.push(crate::params::Param::InteropError( ParamData { InteropError: std::mem::ManuallyDrop::new(err)}));
            params.pack()
        }
    };
}

macro_rules! return_error {
    ( $err:expr, $err_type:expr ) => {
        if let Err(e) = $err {
            error_param!($err_type, String::from_utf8_lossy(e.to_string().as_bytes()))
        } else {
            Parameters::new().pack()
        }
    };
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
pub unsafe extern "C" fn load_script(str_ptr: *const c_char) -> CParams {
    let cstr = CStr::from_ptr(str_ptr);
    let s = cstr.to_string_lossy().to_string();

    println!("Loading script: {}", s);

    if let Some(wasm) = &mut WASM_INTERPRETER {
        let res = wasm.load_script(&s);

        return_error!(res, "Load Error")
    } else {
        error_param!("Critical Error", "WASM interpreter is not loaded")
    }

}

#[no_mangle]
pub unsafe extern "C" fn call_script_function(str_ptr: *const c_char, params: CParams) -> CParams {
    let cstr = CStr::from_ptr(str_ptr);
    let s = cstr.to_string_lossy().to_string();
    if let Some(wasm) = &mut WASM_INTERPRETER {
        let res = wasm.call_void_method(&s, Parameters::unpack(&params));

        return_error!(res, "Runtime Error")
    } else {
        error_param!("Critical Error", "WASM interpreter is not loaded")
    }

}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
    }
}
