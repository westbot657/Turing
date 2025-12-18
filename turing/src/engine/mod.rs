use std::sync::Arc;

use parking_lot::RwLock;

use crate::{
    EngineDataState, ExternalFunctions,
    interop::params::{DataType, Param, Params},
};

#[cfg(feature = "lua")]
pub mod lua_engine;

#[cfg(feature = "wasm")]
pub mod wasm_engine;

#[cfg(feature = "deno")]
pub mod deno_engine;

pub mod types;

pub enum Engine<Ext>
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    #[cfg(feature = "wasm")]
    Wasm(wasm_engine::WasmInterpreter<Ext>),
    #[cfg(feature = "lua")]
    Lua(lua_engine::LuaInterpreter<Ext>),
    #[cfg(feature = "deno")]
    Deno(deno_engine::DenoEngine<Ext>),
}

impl<Ext> Engine<Ext>
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    pub fn call_fn(
        &mut self,
        name: &str,
        params: Params,
        ret_type: DataType,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Param {
        match self {
            #[cfg(feature = "wasm")]
            Engine::Wasm(engine) => engine.call_fn(name, params, ret_type, data),
            #[cfg(feature = "lua")]
            Engine::Lua(engine) => engine.call_fn(name, params, ret_type, data),
            #[cfg(feature = "deno")]
            Engine::Deno(engine) => engine.call_fn(name, params, ret_type, data),
            _ => Param::Error("No code engine is active".to_string()),
        }
    }
}
