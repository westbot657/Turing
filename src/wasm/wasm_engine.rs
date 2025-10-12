use std::collections::{HashMap, VecDeque};
use std::fs;
use std::ops::RangeInclusive;
use std::path::Path;

use anyhow::Result;
use wasmi::{Config, EnforcedLimits, Engine, ExternRef, Instance, Linker, Module, Store, ValType};

use crate::TuringState;

#[derive(Debug)]
pub struct WasmInterpreter {
    engine: Engine,
    store: Store<()>,
    linker: Linker<()>,
    script_instance: Option<(Module, Instance)>,
}

impl WasmInterpreter {
    pub fn new(state: &mut TuringState) -> Result<WasmInterpreter> {
        let mut config = Config::default();
        config.enforced_limits(EnforcedLimits::strict());
        let engine = Engine::new(&config);
        let mut store = Store::new(&engine, ());
        let mut linker = <Linker<()>>::new(&engine);

        state.bind_wasm(&mut linker);

        Ok(WasmInterpreter {
            engine,
            store,
            linker,
            script_instance: None,
        })
    }

    pub fn load_script(&mut self, path: &str) -> Result<()> {

        let path = Path::new(path);
        let wasm = fs::read(path)?;

        let module = Module::new(&self.engine, wasm)?;

        let instance = self.linker.instantiate_and_start(&mut self.store, &module)?;

        self.script_instance = Some((module, instance));

        Ok(())
    }
}

