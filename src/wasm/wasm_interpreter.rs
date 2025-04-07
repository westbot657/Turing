use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use anyhow::{anyhow, Result};
use wasmi::*;
use wat;
use crate::*;
use crate::data::game_objects::*;

struct HostState {
    free_locations: VecDeque<u32>,
    next_index: u32,
    external_data: HashMap<u32, ExternRef>,
}

impl HostState {
    pub fn new() -> HostState {
        HostState {
            free_locations: VecDeque::new(),
            next_index: 0,
            external_data: HashMap::new(),
        }
    }

    pub fn add(&mut self, extern_ref: ExternRef) -> u32 {
        let i;
        if self.free_locations.is_empty() {
            i = self.next_index;
            self.next_index += 1;
        } else {
            i = self.free_locations.pop_front().unwrap();
        }
        self.external_data.insert(i, extern_ref);
        i
    }

    pub fn get(&self, i: u32) -> Option<&ExternRef> {
        self.external_data.get(&i)
    }

    pub fn get_mut(&mut self, i: u32) -> Option<&mut ExternRef> {
        self.external_data.get_mut(&i)
    }

    pub fn remove(&mut self, i: u32) -> Option<ExternRef> {
        self.free_locations.push_front(i);
        self.external_data.remove(&i)
    }

}

pub struct WasmInterpreter {
    engine: Engine,
    store: Store<HostState>,
    linker: Linker<HostState>,
    script_instance: Option<(Module, Instance)>,
}

impl WasmInterpreter {
    pub fn new() -> WasmInterpreter {
        let mut config = Config::default();
        config.enforced_limits(EnforcedLimits::strict());
        let engine = Engine::new(&config);
        let mut store = Store::new(&engine, HostState::new());
        let mut linker = <Linker<HostState>>::new(&engine);

        unsafe {
            bind_data(&engine, &mut store, &mut linker).expect("Failed to setup wasm environment");
        }
        WasmInterpreter {
            engine,
            store,
            linker,
            script_instance: None,
        }
    }

    pub fn load_script(&mut self, path: &str) -> Result<()> {

        let path = Path::new(path);
        let wasm = fs::read(path)?;

        let module = Module::new(&self.engine, wasm)?;

        let instance = self.linker
            .instantiate(&mut self.store, &module)?
            .start(&mut self.store)?;

        self.script_instance = Some((module, instance));

        Ok(())
    }

    pub fn call_void_method(&mut self, name: &str, params: Parameters) -> Result<()> {
        if let Some((_, instance)) = &self.script_instance {
            let init_function = instance.get_typed_func::<(), ()>(&self.store, name)?;
            init_function.call(&mut self.store, ())?;
            Ok(())
        } else {
            Err(anyhow!("no script is currently loaded"))
        }
    }

}

unsafe fn bind_data(engine: &Engine, store: &mut Store<HostState>, linker: &mut Linker<HostState>) -> Result<()> {

    // wasm names are prefixed with '_' so that languages
    // can have abstraction layers to turn stuff into normal
    // structures for the language, and use non-prefixed names

    // GLOBAL VARIABLES


    // GLOBAL FUNCTIONS
    linker.func_wrap("env", "_create_color_note", |mut caller: Caller<'_, HostState>, beat: f32| -> i32 {
        let note = unsafe { create_color_note(beat) };
        let extern_ref = ExternRef::new(&mut caller, note);

        caller.data_mut().add(extern_ref) as i32

    })?;
    // linker.define("env", "_create_color_note", function_create_color_note)?;


    // INSTANCE FUNCTIONS
    linker.func_wrap("env", "_beatmap_add_color_note", |mut caller: Caller<'_, HostState>, note_opt: i32| {
        let extern_ref = caller.data_mut().remove(note_opt as u32);

        if let Some(rf) = extern_ref {
            let val = rf.data(&mut caller).unwrap().downcast_ref::<ColorNote>().unwrap();

            beatmap_add_color_note(*val);

        } else {
            panic!("Invalid pointer");
        }

    })?;

    linker.func_wrap("env", "_log", |caller: Caller<'_, HostState>, message: i32| {

        let memory = caller.get_export("memory").unwrap().into_memory().unwrap();

        let mut output_string = String::new();

        for i in message..i32::MAX {
            let byte: &u8 = memory.data(&caller).get(i as usize).unwrap();

            if *byte == 0u8 {
                break
            }

            output_string.push(char::from(*byte));

        }

        // crate:: is necessary because otherwise it's ambiguous at compile time which macro to use
        crate::println!("[wasm]: {}", output_string);

    })?;


    Ok(())
}

#[cfg(test)]
mod wasm_tests {
    use std::fs;
    use anyhow::Result;
    use wasmprinter::print_bytes;

    #[test]
    fn test_wasm() -> Result<()> {
        let data = fs::read(r"C:\Users\Westb\Desktop\turing_wasm\target\wasm32-unknown-unknown\debug\turing_wasm.wasm")?;

        let wasm = wat::parse_bytes(&data)?;


        println!("{:#}", print_bytes(&wasm)?);

        Ok(())
    }
}
