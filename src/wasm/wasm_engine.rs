use std::cell::{RefCell, RefMut};
use std::collections::{HashMap, VecDeque};
use std::task::Poll;
use std::{fs, mem};
use std::io::Write;
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use tokio::io::AsyncWrite;
use wasmtime::{Config, Engine, ExternRef, Instance, Linker, Module, Store, Val, ValType};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
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

struct OutputWriter {
    inner: Arc<RwLock<Vec<u8>>>,
    is_err: bool,
}
impl std::io::Write for OutputWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write().unwrap().extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let s = { str::from_utf8(&self.inner.read().unwrap()).unwrap().to_string() };
        self.inner.write().unwrap().clear();
        if self.is_err {
            Log::error(s);
        } else {
            Log::info(s);
        }
        Ok(())
    }
}

impl AsyncWrite for OutputWriter {
    fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        self.inner.write().unwrap().extend(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<std::result::Result<(), std::io::Error>> {
        let s = { str::from_utf8(&self.inner.read().unwrap()).unwrap().to_string() };
        self.inner.write().unwrap().clear();
        if self.is_err {
            Log::error(s);
        } else {
            Log::info(s);
        }
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<std::result::Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct WriterInit(Arc<RwLock<Vec<u8>>>, bool);

impl IsTerminal for WriterInit {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for WriterInit {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(OutputWriter {
            inner: self.0.clone(),
            is_err: self.1,
        })
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
            .stdout(WriterInit(Arc::new(RwLock::new(Vec::new())), false))
            .stderr(WriterInit(Arc::new(RwLock::new(Vec::new())), true))
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

    pub fn call_fn(&mut self, name: &str, params: Params, state: &mut RefMut<'_, TuringState>) -> Param {

        let mut instance = self.script_instance.take();

        let ret = state.wasm_fns.get(name).unwrap().2.clone();

        let res = if let Some(instance) = &mut instance {
            if let Some(f) = instance.get_func(&mut self.store, name) {

                let memory = instance.get_export(&mut self.store, "memory").and_then(|m| m.into_memory()).unwrap();
                let args = params.to_args(state);

                let mut res = Vec::new();
                for r in &ret {
                    res.push(match r {
                        7 => Val::F32(0),
                        _ => Val::I32(0),
                    })
                }

                if let Err(e) = f.call(&mut self.store, &args, &mut res) {
                    Param::Error(e.to_string())
                } else {
                    if res.len() > 0 {
                        let rt = res[0];
                        Param::from_typval(ret[0], rt, state, &memory, &mut self.store)
                    } else {
                        Param::Void
                    }

                }

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

