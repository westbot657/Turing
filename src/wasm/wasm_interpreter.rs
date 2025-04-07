use std::fs;
use std::path::Path;
use std::rc::Rc;
use anyhow::{anyhow, Result};
use wasmi::*;
use wasmi::core::UntypedVal;
use wat;
use crate::*;
use crate::data::game_objects::*;

struct HostState {

}

impl HostState {
    fn new() -> HostState {
        HostState {}
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


macro_rules! unpack_ref {
    ($store:ident, $var_in:ident => $var:ident : $typ:ty $body:block ) => {
        if let Some(macro_any) = $var_in.data(&$store) {
            let $var = macro_any.downcast_ref::<$typ>().unwrap();
            $body
        }
    };
}

unsafe fn bind_data(engine: &Engine, store: &mut Store<HostState>, linker: &mut Linker<HostState>) -> Result<()> {

    // wasm names are prefixed with '_' so that languages
    // can have abstraction layers to turn stuff into normal
    // structures for the language, and use non-prefixed names

    // GLOBAL VARIABLES


    // GLOBAL FUNCTIONS
    linker.func_wrap("env", "_create_color_note", |caller: Caller<'_, HostState>, beat: f32| {
        let note = unsafe { create_color_note(beat) };
        ExternRef::new(caller, note)
    })?;
    // linker.define("env", "_create_color_note", function_create_color_note)?;


    // INSTANCE FUNCTIONS
    linker.func_wrap("env", "_beatmap_add_color_note", |caller: Caller<'_, HostState>, note_opt: ExternRef| {
        println!("wtf?");
        unpack_ref!(caller, note_opt => note: ColorNote {
            // beatmap_add_color_note(note.clone());
        });

    })?;

    // linker.func_wrap("env", "_log", |caller: Caller<'_, HostState>, message: i64| {
    //
    // })?;


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
