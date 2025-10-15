use std::collections::{HashMap, VecDeque};
use std::{fs, mem};
use std::io::Write;
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use wasmtime::{Config, Engine, ExternRef, Instance, Linker, Module, Store, ValType};
use wasmtime_wasi::cli::StdoutStream;
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::p2::{OutputStream, StreamResult};
use wasmtime_wasi::WasiCtxBuilder;

use crate::interop::params::{Param, Params};
use crate::util::ToCStr;
use crate::{Log, TuringState};

pub struct WasmInterpreter {
    engine: Engine,
    store: Store<WasiP1Ctx>,
    linker: Linker<WasiP1Ctx>,
    script_instance: Option<Instance>,
}

pub struct OutBuf {
    buf: String,
}
impl OutBuf {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
        }
    }
}
impl OutputStream for OutBuf {
    fn write(&mut self, bytes: Bytes) -> wasmtime_wasi::p2::StreamResult<()> {
        self.buf += bytes;
        Ok(())
    }
    fn flush(&mut self) -> wasmtime_wasi::p2::StreamResult<()> {
        Ok(())
    }
    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(2048)
    }
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
            .stdout()
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

    pub fn load_script(&mut self, path: &Path) -> Result<()> {

        let wasm = fs::read(path)?;

        let module = Module::new(&self.engine, wasm)?;

        let instance = self.linker.instantiate(&mut self.store, &module)?;

        self.script_instance = Some(instance);

        Ok(())
    }

    pub fn call_fn(&mut self, name: &str, params: Params) -> Param {

        let mut instance = self.script_instance.take();

        let res = if let Some(instance) = &mut instance {
            if let Some(f) = instance.get_func(&mut self.store, name) {

                todo!()

            } else {
                Param::Error("Function does not exist".to_string())
            }
        } else {
            Param::Error("No script is loaded or reentry was attempted".to_string())
        };


        self.script_instance = instance;

        res
    }

}

