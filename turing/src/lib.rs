extern crate core;

use std::collections::VecDeque;
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc};
use crate::engine::Engine;
use crate::engine::types::ScriptFnMetadata;
use crate::engine::wasm_engine::write_wasm_string;
use anyhow::{anyhow, Result};
use convert_case::{Case, Casing};
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use slotmap::{new_key_type, SlotMap};
use wasmtime::{Caller, Val};
use wasmtime_wasi::p1::WasiP1Ctx;
use crate::interop::params::{DataType, Param, Params};
use crate::interop::types::ExtPointer;

pub mod engine;
pub mod interop;

#[cfg(test)]
mod tests;

#[cfg(target_os = "windows")]
mod win_ffi;

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
pub struct EngineDataState {
    /// maps opaque pointer ids to real pointers
    pub opaque_pointers: SlotMap<OpaquePointerKey, ExtPointer<c_void>>,
    /// maps real pointers back to their opaque pointer ids
    pub pointer_backlink: FxHashMap<ExtPointer<c_void>, OpaquePointerKey>,
    /// queue of strings for wasm to fetch (needed due to reentrancy limitations)
    pub str_cache: VecDeque<String>,
    /// which mods are currently active
    pub active_capabilities: FxHashSet<String>,
}

impl EngineDataState {
    pub fn get_opaque_pointer(&mut self, pointer: ExtPointer<c_void>) -> OpaquePointerKey {
        if let Some(opaque) = self.pointer_backlink.get(&pointer) {
            *opaque
        } else {
            let op = self.opaque_pointers.insert(pointer);
            self.pointer_backlink.insert(pointer, op);
            op
        }
    }
}



pub struct Turing<Ext: ExternalFunctions + Send + Sync + 'static> {
    pub engine: Option<Engine<Ext>>,
    pub data: Arc<RwLock<EngineDataState>>,
    pub script_fns: FxHashMap<String, ScriptFnMetadata>,
    _ext: PhantomData<Ext>
}

pub struct TuringSetup<Ext: ExternalFunctions + Send + Sync + 'static> {
    script_fns: FxHashMap<String, ScriptFnMetadata>,
    _ext: PhantomData<Ext>
}

impl<Ext: ExternalFunctions + Send + Sync + 'static> TuringSetup<Ext> {

    pub fn build(self) -> Result<Turing<Ext>> {
        let data = Arc::new(RwLock::new(EngineDataState::default()));
        Ok(Turing::build(self.script_fns, data))
    }

    /// Attempts to add a new function. Returns err if the function already exists
    pub fn add_function(&mut self, name: impl ToString, metadata: ScriptFnMetadata) -> Result<()> {
        let name = name.to_string();
        if self.script_fns.contains_key(&name) {
            return Err(anyhow!("A function named '{}' has already been registered", name))
        }
        self.script_fns.insert(name, metadata);
        Ok(())
    }

}

impl<Ext: ExternalFunctions + Send + Sync + 'static> Turing<Ext> {

    pub fn new() -> TuringSetup<Ext> {
        TuringSetup {
            script_fns: Default::default(),
            _ext: PhantomData::default(),
        }
    }

    fn build(script_fns: FxHashMap<String, ScriptFnMetadata>, data: Arc<RwLock<EngineDataState>>) -> Self {
        Self {
            engine: None,
            script_fns,
            data,
            _ext: PhantomData::default(),
        }
    }
    pub fn load_script(
        &mut self,
        source: impl ToString,
        loaded_capabilities: &[impl ToString],
    ) -> Result<()> {
        // drop any existing engine
        self.engine.take();

        let source = source.to_string();
        let source = Path::new(&source);
        let capabilities: FxHashSet<String> =
            loaded_capabilities.iter().map(|c| c.to_string()).collect();

        if let Err(e) = source.metadata() {
            return Err(anyhow!("Script does not exist: {:#?}, {:#?}", source, e));
        }

        let Some(extension) = source.extension() else {
            return Err(anyhow!(
                "script file has no extension, must be either .wasm or .lua"
            ));
        };
        match extension.to_string_lossy().as_ref() {
            "wasm" => {
                let mut wasm_interpreter = engine::wasm_engine::WasmInterpreter::new(
                    &self.script_fns,
                    Arc::clone(&self.data),
                )?;
                wasm_interpreter.load_script(source)?;
                self.engine = Some(Engine::Wasm(wasm_interpreter));
            }
            "lua" => {
                let mut lua_interpreter = engine::lua_engine::LuaInterpreter::new(
                    &self.script_fns,
                    Arc::clone(&self.data),
                )?;
                lua_interpreter.load_script(source)?;
                self.engine = Some(Engine::Lua(lua_interpreter));
            }
            _ => {
                return Err(anyhow!(
                    "Unknown script extension: '{extension:?}' must be .wasm or .lua"
                ));
            }
        }

        let mut write = self.data.write();
        write.active_capabilities = capabilities;

        Ok(())
    }

    pub fn call_fn(&mut self, name: impl ToString, params: Params, expected_return_type: DataType) -> Param {
        let name = name.to_string();
        let Some(engine) = &mut self.engine else {
            return Param::Error("No code engine is active".to_string())
        };

        engine.call_fn(
            &name,
            params,
            expected_return_type,
            Arc::clone(&self.data)
        )
    }


}

/// internal for use in the wasm engine only
pub(crate) fn wasm_host_strcpy(
    data: &Arc<RwLock<EngineDataState>>,
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
            write_wasm_string(ptr as u32, &next_str, &memory, caller)?;
            rs[0] = Val::I32(ptr);
        }
        return Ok(());
    }

    Ok(())
}

