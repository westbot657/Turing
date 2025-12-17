use std::sync::Arc;

use parking_lot::RwLock;

use crate::{
    EngineDataState, ExternalFunctions,
    engine::{lua_engine::LuaInterpreter, wasm_engine::WasmInterpreter},
    interop::params::{DataType, Param, Params},
};

pub mod lua_engine;
pub mod wasm_engine;

pub mod types;

pub enum Engine<Ext>
where
    Ext: ExternalFunctions + Send + Sync + 'static,
{
    Wasm(WasmInterpreter<Ext>),
    Lua(LuaInterpreter<Ext>),
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
    ) -> Param{
        match self {
            Engine::Wasm(engine) => engine.call_fn(name, params, ret_type, data),
            Engine::Lua(engine) => engine.call_fn(name, params, ret_type, data),
        }
    }
}
