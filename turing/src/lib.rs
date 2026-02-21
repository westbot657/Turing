extern crate core;

use crate::engine::Engine;
use crate::engine::types::ScriptFnMetadata;
use crate::interop::params::{DataType, FreeableDataType, Param, Params};
use crate::interop::types::{Semver, U32Buffer};
use anyhow::{Result, anyhow};
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod engine;
pub mod interop;
pub mod key_vec;
mod spec_gen;

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
    fn free_of_type(ptr: *mut c_void, typ: FreeableDataType);
    fn free_u32_buffer(buf: U32Buffer);
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ScriptFnKey(u32);

impl ScriptFnKey {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != u32::MAX
    }
}

impl From<u32> for ScriptFnKey {
    fn from(value: u32) -> Self {
        ScriptFnKey(value)
    }
}

impl From<ScriptFnKey> for u32 {
    fn from(value: ScriptFnKey) -> Self {
        value.0
    }
}

impl From<ScriptFnKey> for usize {
    fn from(value: ScriptFnKey) -> Self {
        value.0 as usize
    }
}

#[derive(Default)]
pub struct EngineDataState {
    /// queue of strings for wasm to fetch (needed due to reentrancy limitations)
    pub str_cache: VecDeque<String>,
    /// which mods are currently active
    pub active_capabilities: FxHashSet<String>,
    /// queue for algebraic type's data
    pub f32_queue: VecDeque<f32>,
    /// queue for Vec<u32>s
    pub u32_buffer_queue: VecDeque<Vec<u32>>,
}

impl EngineDataState {}

pub struct Turing<Ext: ExternalFunctions + Send + Sync + 'static> {
    pub engine: Option<Engine<Ext>>,
    pub data: Arc<RwLock<EngineDataState>>,
    pub script_fns: FxHashMap<String, ScriptFnMetadata>,
    _ext: PhantomData<Ext>,
}

pub struct TuringSetup<Ext: ExternalFunctions + Send + Sync + 'static> {
    script_fns: FxHashMap<String, ScriptFnMetadata>,
    _ext: PhantomData<Ext>,
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
            return Err(anyhow!(
                "A function named '{}' has already been registered",
                name
            ));
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

    fn build(
        script_fns: FxHashMap<String, ScriptFnMetadata>,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Self {
        Self {
            engine: None,
            script_fns,
            data,
            _ext: PhantomData,
        }
    }

    /// Enables a capability for the currently loaded script
    pub fn register_capability(&mut self, name: impl ToString) {
        self.data
            .write()
            .active_capabilities
            .insert(name.to_string());
    }

    /// Disables a capability for the currently loaded script
    pub fn unregister_capability(&mut self, name: impl AsRef<str>) {
        self.data.write().active_capabilities.remove(name.as_ref());
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

        for cap in &capabilities {
            Ext::log_info(format!("Registered capability: {}", cap));
        }
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

    pub fn get_fn_key(&self, arg: &str) -> Option<ScriptFnKey> {
        let Some(engine) = &self.engine else {
            panic!("Engine not initialized");
        };

        engine.get_fn_key(arg)
    }

    pub fn call_fn_by_name(
        &mut self,
        name: impl ToString,
        params: Params,
        expected_return_type: DataType,
    ) -> Param {
        let Some(engine) = &mut self.engine else {
            return Param::Error("No code engine is active".to_string());
        };
        let key = engine.get_fn_key(&name.to_string());

        let Some(key) = key else {
            return Param::Error(format!("Function '{}' not found", name.to_string()));
        };
        self.call_fn(key, params, expected_return_type)
    }

    pub fn call_fn(
        &mut self,
        cache_key: ScriptFnKey,
        params: Params,
        expected_return_type: DataType,
    ) -> Param {
        // let name = name.to_string();
        let Some(engine) = &mut self.engine else {
            return Param::Error("No code engine is active".to_string());
        };

        if !cache_key.is_valid() {
            return Param::Error("Invalid function key".to_string());
        }

        engine.call_fn(cache_key, params, expected_return_type, &self.data)
    }

    pub fn fast_call_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        let Some(engine) = &mut self.engine else {
            return Err("Engine not initialized".to_string());
        };

        engine.fast_call_update(delta_time)
    }

    pub fn fast_call_fixed_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        let Some(engine) = &mut self.engine else {
            return Err("Engine not initialized".to_string());
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

/// Panic hook that logs panic information using the provided external functions.
pub fn panic_hook<Ext>(file_out: Option<PathBuf>, info: &std::panic::PanicHookInfo)
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    let msg = info.payload_as_str().unwrap_or("Unknown panic payload");

    let location = if let Some(location) = info.location() {
        format!("file '{}' at line {}", location.file(), location.line())
    } else {
        "unknown location".to_string()
    };

    let full_msg = format!("Panic occurred at {}: {}", location, msg);
    // Capture a backtrace and include it in outputs
    let backtrace = std::backtrace::Backtrace::force_capture();
    let backtrace_str = format!("{}", backtrace);

    let full_msg_with_bt = format!("{}\nBacktrace:\n{}", full_msg, backtrace_str);

    Ext::log_critical(format!(
        "Writing panic information to file and stderr: {:?}",
        file_out
    ));
    if let Some(file_path) = file_out
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .open(file_path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{}", full_msg_with_bt);
        let _ = file.flush();
    }

    eprintln!("{}", full_msg);
    eprintln!("Backtrace:\n{}", backtrace_str);

    // Log as critical error (include backtrace)
    Ext::log_critical(full_msg_with_bt);
}
