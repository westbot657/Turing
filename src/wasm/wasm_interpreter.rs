use std::fs;
use anyhow::{anyhow, Result};
use serde_json::Value;
use wasmi::*;
use wat;



type HostState = u32; // idk what this is or what it means or how it's used
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
        let mut store = Store::new(&engine, 0);
        let mut linker = <Linker<HostState>>::new(&engine);

        bind_data(&engine, &mut store, &mut linker);

        WasmInterpreter {
            engine,
            store,
            linker,
            script_instance: None,
        }
    }

    /// TODO: make this method take in beatmap data and bind it for global use (and any other global data)
    pub fn load_script(&mut self, path: &str) -> Result<()> {

        let data = fs::read_to_string(path)?;

        let wasm = wat::parse_str(&data)?;

        let module = Module::new(&self.engine, &mut &wasm[..])?;

        let instance = self.linker
            .instantiate(&mut self.store, &module)?
            .start(&mut self.store)?;

        self.script_instance = Some((module, instance));

        Ok(())
    }

    pub fn call_void_method(&mut self, name: &str) -> Result<()> {
        if let Some((_, instance)) = &self.script_instance {
            let init_function = instance.get_typed_func::<(), ()>(&self.store, name)?;
            init_function.call(&mut self.store, ())?;
            Ok(())
        } else {
            Err(anyhow!("no script is currently loaded"))
        }
    }

    pub fn call_init(&mut self) -> Result<()> {
        self.call_void_method("init")
    }

    pub fn call_end(&mut self) -> Result<()> {
        self.call_void_method("end")
    }

    pub fn call_update(&mut self) -> Result<()> {
        self.call_void_method("update")
    }




}

fn bind_data(engine: &Engine, store: &mut Store<HostState>, linker: &mut Linker<HostState>) {

}

#[cfg(test)]
mod wasm_tests {
    use std::fs;
    use anyhow::Result;

    #[test]
    fn test_wasm() -> Result<()> {
        let data = fs::read_to_string(r"C:\Users\Westb\Desktop\turing_wasm\pkg\turing_wasm_bg.wasm")?;

        let wasm = wat::parse_str(&data)?;

        println!("{:#?}", wasm);

        Ok(())
    }
}
