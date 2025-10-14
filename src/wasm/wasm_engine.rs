use std::collections::{HashMap, VecDeque};
use std::fs;
use std::ops::RangeInclusive;
use std::path::Path;

use anyhow::Result;
use wasmtime::{Config, Engine, ExternRef, Instance, Linker, Module, Store, ValType};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use crate::TuringState;

pub struct WasmInterpreter {
    engine: Engine,
    store: Store<WasiP1Ctx>,
    linker: Linker<WasiP1Ctx>,
    script_instance: Option<(Module, Instance)>,
}

impl WasmInterpreter {
    pub fn new(state: &mut TuringState) -> Result<WasmInterpreter> {
        let mut config = Config::new();
        config.wasm_threads(false);
        config.cranelift_pcc(true); // do sandbox verification checks
        config.async_support(false);
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        config.wasm_bulk_memory(true);
        config.wasm_reference_types(true);
        config.wasm_multi_memory(false);
        config.max_wasm_stack(512 * 1024); // 512KB
        config.consume_fuel(false);

        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .allow_tcp(false)
            .allow_udp(false)
            .build_p1();

        let engine = Engine::new(&config)?;
        let mut store = Store::new(&engine, wasi);


        let mut linker = <Linker<WasiP1Ctx>>::new(&engine);

        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t| t)?;

        state.bind_wasm(&engine, &mut linker);

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

        let instance = self.linker.instantiate(&mut self.store, &module)?;

        self.script_instance = Some((module, instance));

        Ok(())
    }
}

