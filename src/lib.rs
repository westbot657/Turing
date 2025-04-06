mod wasm;
mod data;
mod interop;

use std::collections::HashMap;
use data::game_objects::*;
use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::{c_char, c_void};
use std::sync::{Mutex};
use std::sync::atomic::AtomicPtr;
use glam::{Quat, Vec3};
use crate::interop::parameters::*;
use crate::wasm::wasm_interpreter::WasmInterpreter;

static mut WASM_INTERPRETER: Option<WasmInterpreter> = None;

type CsMethod = extern "C" fn(CParams) -> CParams;

macro_rules! println {
    ( $st:literal ) => {
        print_out($st.to_owned())
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

    { // scope so that FUNCTION_MAP unlocks before print_out
        let mut map = FUNCTION_MAP.lock().unwrap();
        map.insert(name.to_string(), func_ptr_arc);
    }

    println!("bound function to name '{}'", name);
}

macro_rules! extern_fn {
    (
        $cs_name: ident as
        $name:ident ( $( $arg:ident : $arg_ty:ty ),* ) $( -> $ret:ty )?
    ) => {
        #[no_mangle]
        pub fn $name( $( $arg : $arg_ty ),* ) $( -> $ret )? {
            let map = FUNCTION_MAP.lock().unwrap();
            if let Some(_macro_arc) = map.get(stringify!($cs_name)) {
                let _macro_raw_ptr = _macro_arc.load(std::sync::atomic::Ordering::SeqCst);
                let $cs_name: CsMethod = unsafe { mem::transmute(_macro_raw_ptr) };

                let mut params_in = crate::interop::parmeters::Parameters::new();

                $(
                    push_parameter!(params_in, $arg_ty: $arg);
                )*

                let packed = params_in.pack();

                let res = $cs_name(packed);

                let result = res.unpack();

                $(
                if result.size() == 1 {
                    return get_return!(result, $ret, 0)
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
            $name:ident ( $( $arg:ident : $arg_ty:ty ),* ) $( -> $ret:ty )?
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


extern_fns!(
    cs_print as print_out(message: String),
    create_note as create_color_note(beat: f32) -> ColorNote,
);





//////////////////////////////////////////////////////
// Functions that c#/c++ calls are defined here


#[no_mangle]
pub unsafe extern "C" fn initialize_wasm() {
    WASM_INTERPRETER = Some(WasmInterpreter::new());

    println!("Initialized wasm interpreter");
}

// /// loads a script from a directory
// #[no_mangle]
// pub unsafe extern "C" fn load_script(raw_path: *const c_char) {
//     let path = CStr::from_ptr(raw_path);
//     if let Some(wasm_interp) = &mut WASM_INTERPRETER {
//         wasm_interp.load_script(path.to_str().unwrap()).unwrap();
//         println!("Loaded wasm script");
//     }
// }

// /// tries to find and call the `init` method in the currently loaded script
// #[no_mangle]
// pub unsafe extern "C" fn call_script_init() {
//     if let Some(wasm_interp) = &mut WASM_INTERPRETER {
//         println!("Calling wasm init");
//         wasm_interp.call_init().unwrap();
//         println!("Called wasm init");
//     }
// }

// this function doesn't actually make sense to exist...
// #[no_mangle]
// pub unsafe extern "C" fn bind_colornote(note: ColorNote) {
//
// }

// end of c#/c++ -> rust defs


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
    }
}
