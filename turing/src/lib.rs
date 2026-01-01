extern crate core;

use std::collections::VecDeque;
use std::ffi::c_char;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;
use crate::engine::Engine;
use crate::engine::types::ScriptFnMetadata;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use slotmap::{new_key_type, SlotMap};
use crate::interop::params::{DataType, Param, Params};
use crate::interop::types::{ExtPointer, Semver};

pub mod engine;
pub mod interop;

#[cfg(test)]
mod tests;

#[cfg(feature = "global_ffi")]
mod global_ffi;

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
    pub opaque_pointers: SlotMap<OpaquePointerKey, ExtPointer>,
    /// maps real pointers back to their opaque pointer ids
    pub pointer_backlink: FxHashMap<ExtPointer, OpaquePointerKey>,
    /// queue of strings for wasm to fetch (needed due to reentrancy limitations)
    pub str_cache: VecDeque<String>,
    /// which mods are currently active
    pub active_capabilities: FxHashSet<String>,
}

impl EngineDataState {
    pub fn get_opaque_pointer(&mut self, pointer: ExtPointer) -> OpaquePointerKey {
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

    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> TuringSetup<Ext> {
        TuringSetup {
            script_fns: Default::default(),
            _ext: PhantomData,
        }
    }

    fn build(script_fns: FxHashMap<String, ScriptFnMetadata>, data: Arc<RwLock<EngineDataState>>) -> Self {
        Self {
            engine: None,
            script_fns,
            data,
            _ext: PhantomData,
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
            #[cfg(feature = "wasm")]
            "wasm" => {
                let mut wasm_interpreter = engine::wasm_engine::WasmInterpreter::new(
                    &self.script_fns,
                    Arc::clone(&self.data),
                )?;
                wasm_interpreter.load_script(source)?;
                self.engine = Some(Engine::Wasm(wasm_interpreter));
            }
            #[cfg(feature = "lua")]
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

    pub fn fast_call_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        let Some(engine) = &mut self.engine else {
            return Err("Engine not initialized".to_string())
        };

        engine.fast_call_update(delta_time)
    }

    pub fn fast_call_fixed_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        let Some(engine) = &mut self.engine else {
            return Err("Engine not initialized".to_string())
        };

        engine.fast_call_fixed_update(delta_time)
    }

    pub fn get_api_versions(&self) -> Option<&FxHashMap<String, Semver>> {
        let Some(engine) = &self.engine else {
            return None;
        };
        
        engine.get_api_versions()
    }

}
