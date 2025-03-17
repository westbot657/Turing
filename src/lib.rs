mod wasm;
mod data;

use std::collections::HashMap;
use data::game_objects::*;
use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::{c_char, c_void};
use std::sync::{Mutex};
use std::sync::atomic::AtomicPtr;
use glam::{Quat, Vec3};
use crate::wasm::wasm_interpreter::WasmInterpreter;

static mut WASM_INTERPRETER: Option<WasmInterpreter> = None;


macro_rules! println {
    ( $st:literal ) => {
        print_out($st)
    };
    ( $st:literal, $($args:expr),* ) => {
        print_out(format!($st, $($args),*).as_str())
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

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Note {
    pub position: Vec3,
    pub orientation: Quat,
}


// Global mutable static to store the callback
// static CS_PRINT_CALLBACK: Mutex<CsPrintFunc> = Mutex::new(None);

lazy_static::lazy_static! {
    static ref FUNCTION_MAP: Mutex<HashMap<String, std::sync::Arc<AtomicPtr<c_void>>>> = Mutex::new(HashMap::new());
}

/// Sets the callback function for cs_print.
// #[no_mangle]
// pub extern "C" fn set_cs_print(func: CsPrintFunc) {
//     let mut callback = CS_PRINT_CALLBACK.lock().unwrap();
//     *callback = func;
// }

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
    ( $name:ident ( $( $param:ident: $typ:ty ),* ) -> $ret:ty as $cname:tt : $ext_typ:ty $body:block ) => {
        #[no_mangle]
        pub fn $name( $( $param: $typ ),* ) -> $ret {
            let map = FUNCTION_MAP.lock().unwrap();
            if let Some(_macro_arc) = map.get(stringify!($cname)) {
                let _macro_raw_ptr = _macro_arc.load(std::sync::atomic::Ordering::SeqCst);
                // this is actually VERY unsafe, but as long as the C# code exposes the correct method with the correct argument count and correct types, then it should be fine.
                let $cname: $ext_typ = unsafe { mem::transmute(_macro_raw_ptr) };
                $body
            }
        }
    };
    ( $name:ident ( $( $param:ident: $typ:ty ),* ) -> $ret:ty as $cname:tt : $ext_typ:ty $body:block $fallback:block ) => {
        #[no_mangle]
        pub fn $name( $( $param: $typ ),* ) -> $ret {
            let map = FUNCTION_MAP.lock().unwrap();
            if let Some(_macro_arc) = map.get(stringify!($cname)) {
                let _macro_raw_ptr = _macro_arc.load(std::sync::atomic::Ordering::SeqCst);
                // this is actually VERY unsafe, but as long as the C# code exposes the correct method with the correct argument count and correct types, then it should be fine.
                let $cname: $ext_typ = unsafe { mem::transmute(_macro_raw_ptr) };
                $body
            } else {
                $fallback
            }
        }
    };
}

macro_rules! cs_unreachable {
    () => {
        unreachable!("This should not be reachable as long as C# defines everything correctly")
    };
}

// Type aliases for the function pointers
type FnStrPtrRetNull = extern "C" fn(*const c_char);
type FnFloatRetNote = extern "C" fn(f32) -> *mut ColorNote;
type FnNoteRetNull = extern "C" fn(*mut ColorNote);

extern_fn!(print_out(message: &str) -> () as cs_print : FnStrPtrRetNull {
    let c_string = CString::new(message).unwrap();
    let c_ptr = c_string.as_ptr();
    unsafe { cs_print(c_ptr) }
});

extern_fn!(create_color_note(beat: f32) -> *mut ColorNote as create_note : FnFloatRetNote {
    create_note(beat)
} {
    cs_unreachable!()
});

extern_fn!(beatmap_add_color_note(note: *mut ColorNote) -> () as add_note_to_map : FnNoteRetNull {
    add_note_to_map(note);
});



// Functions that c#/c++ calls and are defined here
#[no_mangle]
pub unsafe extern "C" fn initialize_wasm() {
    WASM_INTERPRETER = Some(WasmInterpreter::new());

    println!("Initialized wasm interpreter");
}

/// loads a script from a directory
#[no_mangle]
pub unsafe extern "C" fn load_script(raw_path: *const c_char) {
    let path = CStr::from_ptr(raw_path);
    if let Some(wasm_interp) = &mut WASM_INTERPRETER {
        wasm_interp.load_script(path.to_str().unwrap()).unwrap();
        println!("Loaded wasm script");
    }
}

/// tries to find and call the `init` method in the currently loaded script
#[no_mangle]
pub unsafe extern "C" fn call_script_init() {
    if let Some(wasm_interp) = &mut WASM_INTERPRETER {
        println!("Calling wasm init");
        wasm_interp.call_init().unwrap();
        println!("Called wasm init");
    }
}


// end of c#/c++ -> rust defs


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
    }
}
