mod wasm;
mod data;

use std::ffi::CString;
use data::game_objects::*;
use crate::data::beatmap_types::Beatmap;
use crate::wasm::wasm_interpreter::WasmInterpreter;

static mut WASM_INTERPRETER: Option<WasmInterpreter> = None;


// Functions that rust calls, and are defined in c#/c++
extern "C" { // function hooks to beat saber

    // Mod systems
    pub fn get_beatmap() -> Beatmap;
    pub fn add_color_note_to_beatmap(note: ColorNote);

    // Vanilla systems
    // ask beat saber to instantiate objects, and then rust modifies their data
    pub fn create_color_note() -> ColorNote;
    pub fn create_bomb() -> BombNote;
    pub fn create_wall() -> Wall;
    pub fn create_arc() -> Arc;
    pub fn create_chain() -> ChainNote;
    pub fn create_chain_head_note() -> ChainHeadNote;
    pub fn create_chain_link_note() -> ChainLinkNote;


    // Chroma systems

    // Noodle systems

    // Vivify systems

}
// end of rust -> c#/c++ defs

// Functions that c#/c++ calls and are defined here

pub unsafe extern "C" fn initialize_wasm() {
    WASM_INTERPRETER = Some(WasmInterpreter::new());
}

/// loads a script from a directory
pub unsafe extern "C" fn load_script(path: CString) {
    unsafe {
        if let Some(wasm_interp) = &mut WASM_INTERPRETER {
            wasm_interp.load_script(path.to_str().unwrap()).unwrap()
        }
    }
}

/// tries to find and call the `init` method in the currently loaded script
pub unsafe extern "C" fn call_script_init() {
    unsafe {
        if let Some(wasm_interp) = &mut WASM_INTERPRETER {
            wasm_interp.call_init().unwrap()
        }
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
