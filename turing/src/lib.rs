use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc};
use crate::wasm::wasm_engine::{write_string, WasmFnMetadata, WasmInterpreter};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use slotmap::{new_key_type, SlotMap};
use wasmtime::{Caller, Val};
use wasmtime_wasi::p1::WasiP1Ctx;
use crate::interop::params::{DataType, Param, Params};
use crate::interop::types::ExtPointer;

pub mod wasm;
pub mod interop;

#[cfg(test)]
mod tests;

pub trait ExternalFunctions {
    fn abort(error_type: String, error: String) -> !;
    fn log_info(msg: impl ToString);
    fn log_warn(msg: impl ToString);
    fn log_debug(msg: impl ToString);
    fn log_critical(msg: impl ToString);
    fn free_string(ptr: *const c_char);
}

new_key_type! {
    pub struct OpaquePointerKey;
}

#[derive(Default)]
pub struct WasmDataState {
    /// maps opaque pointer ids to real pointers
    pub opaque_pointers: SlotMap<OpaquePointerKey, ExtPointer<c_void>>,
    /// maps real pointers back to their opaque pointer ids
    pub pointer_backlink: HashMap<ExtPointer<c_void>, OpaquePointerKey>,
    /// queue of strings for wasm to fetch (needed due to reentrancy limitations)
    pub str_cache: VecDeque<String>,
    /// which mods are currently active
    pub active_capabilities: HashSet<String>,
}

pub struct Turing<Ext: ExternalFunctions + Send + Sync + 'static> {
    pub wasm: WasmInterpreter<Ext>,
    pub data: Arc<RwLock<WasmDataState>>,
    _ext: PhantomData<Ext>
}

pub struct TuringSetup<Ext: ExternalFunctions + Send + Sync + 'static> {
    wasm_fns: HashMap<String, WasmFnMetadata>,
    _ext: PhantomData<Ext>
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> TuringSetup<Ext> {

    pub fn build(self) -> Result<Turing<Ext>> {
        let data = Arc::new(RwLock::new(WasmDataState::default()));
        let wasm = WasmInterpreter::new(self.wasm_fns, Arc::clone(&data))?;
        Ok(Turing::build(wasm, data))
    }

    /// Attempts to add a new function. Returns err if the function already exists
    pub fn add_function(&mut self, name: impl ToString, metadata: WasmFnMetadata) -> Result<()> {
        let name = name.to_string();
        if self.wasm_fns.contains_key(&name) {
            return Err(anyhow!("A function named '{}' has already been registered", name))
        }
        self.wasm_fns.insert(name, metadata);
        Ok(())
    }

}

impl<Ext: ExternalFunctions + Send + Sync + 'static> Turing<Ext> {

    pub fn new() -> TuringSetup<Ext> {
        TuringSetup {
            wasm_fns: HashMap::default(),
            _ext: PhantomData::default(),
        }
    }

    fn build(wasm: WasmInterpreter<Ext>, data: Arc<RwLock<WasmDataState>>) -> Self {
        Self {
            wasm,
            data,
            _ext: PhantomData::default(),
        }
    }

    pub fn load_script(&mut self, source: impl ToString, loaded_capabilities: &[impl ToString]) -> Result<()> {

        let source = source.to_string();
        let source = Path::new(&source);
        let capabilities: HashSet<String> = loaded_capabilities.iter().map(|c| c.to_string()).collect();

        if let Err(e) = source.metadata() {
            return Err(anyhow!("Script does not exist: {:#?}, {:#?}", source, e))
        }

        self.wasm.load_script(source)?;
        let mut write = self.data.write();
        write.active_capabilities = capabilities;

        Ok(())
    }

    pub fn call_wasm_fn(&mut self, name: impl ToString, params: Params, expected_return_type: DataType) -> Param {
        let name = name.to_string();
        self.wasm.call_fn(&name, params, expected_return_type, Arc::clone(&self.data))
    }


}

/// internal for use in the wasm engine only
pub(crate) fn wasm_host_strcpy(
    data: &Arc<RwLock<WasmDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();

    if let Some(next_str) = data.write().str_cache.pop_front()
        && next_str.len() + 1 == size as usize
    {
        if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
            write_string(ptr as u32, next_str, &memory, caller)?;
            rs[0] = Val::I32(ptr);
        }
        return Ok(());
    }

    Ok(())
}

