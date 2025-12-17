use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;
use mlua::prelude::*;
use anyhow::{anyhow, Result};
use convert_case::{Case, Casing};
use mlua::{Table, Value};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use crate::wasm::wasm_engine::{WasmCallback, WasmFnMetadata};
use crate::{ExternalFunctions, WasmDataState};
use crate::interop::params::{DataType, Param, Params};
use crate::interop::types::ExtPointer;

pub struct LuaInterpreter<Ext: ExternalFunctions> {
    lua_fns: FxHashMap<String, WasmFnMetadata>,
    data: Arc<RwLock<WasmDataState>>,
    engine: Option<(Lua, Table)>,
    _ext: PhantomData<Ext>
}

impl<Ext: ExternalFunctions> LuaInterpreter<Ext> {

    pub fn new(lua_functions: &FxHashMap<String, WasmFnMetadata>, data: Arc<RwLock<WasmDataState>>) -> Result<Self> {
        Ok(Self {
            lua_fns: lua_functions.clone(),
            data,
            engine: None,
            _ext: PhantomData::default()
        })
    }

    fn generate_function(&self, lua: &Lua, table: &Table, name: &str, metadata: &WasmFnMetadata) -> Result<()> {
        let cap = metadata.capability.to_owned();
        let callback = metadata.callback;
        let pts = metadata.param_types.clone();
        let data = Arc::clone(&self.data);

        let func = lua.create_function(move |lua, args: LuaVariadic<Value>| -> mlua::Result<Value> {
            lua_bind_env::<Ext>(
                &data, lua, &cap, &args, pts.as_slice(), &callback
            )
        }).map_err(|e| anyhow!("Failed to create function: {e}"))?;

        table.set(name, func).map_err(|e| anyhow!("Failed to set function: {e}"))?;

        Ok(())
    }

    fn create_class_table_if_missing(api: &Table, cname: &str, engine: &Lua) -> Result<()> {
        if api.raw_get::<Table>(cname).is_err() {
            let cls_table = engine.create_table().map_err(|e| anyhow!("Failed to create table: {e}"))?;
            cls_table.raw_set("__index", cls_table.clone()).map_err(|e| anyhow!("Failed to set table as self's __index member: {e}"))?;
            api.raw_set(
                cname,
                cls_table
            ).map_err(|e| anyhow!("Failed to add class to api: {e}"))?;
        }
        Ok(())
    }

    fn generate_new_method(lua: &Lua, class_table: &Table) -> Result<()> {
        if class_table.contains_key("new").unwrap_or(false) {
            return Ok(());
        }

        let new_fn = lua.create_function({
            let class_table = class_table.clone();
            move |lua, args: LuaVariadic<Value>| {
                if args.len() != 1 {
                    return Err(mlua::Error::RuntimeError(format!("expected 1 argument, got {}", args.len())));
                }

                let val = match &args[0] {
                    Value::Integer(i) => *i,
                    _ => { return Err(mlua::Error::RuntimeError("expected integer argument".to_string(), )) }
                };

                let instance = lua.create_table()?;
                instance.set("opaqu", val)?;

                instance.set_metatable(Some(class_table.clone()))?;

                Ok(instance)
            }
        }).map_err(|e| anyhow!("Failed to create 'new' method: {e}"))?;

        class_table.set("new", new_fn).map_err(|e| anyhow!("Failed to bind 'new' method: {e}"))?;

        Ok(())
    }

    fn bind_lua(&self, api: &Table, engine: &Lua) -> Result<()> {
        for (name, metadata) in &self.lua_fns {
            if name.contains(".") {
                let parts: Vec<&str> = name.splitn(2, ".").collect();
                let cname = parts[0].to_case(Case::Pascal);
                let fname = parts[1].to_case(Case::Snake);

                Self::create_class_table_if_missing(api, cname.as_str(), engine)?;

                let Ok(table) = api.raw_get::<Table>(cname.as_str()) else { return Err(anyhow!("table['{cname}'] is not a table")) };
                self.generate_function(engine, &table, fname.as_str(), metadata)?;
            } else if name.contains(":") {
                let parts: Vec<&str> = name.splitn(2, ":").collect();
                let cname = parts[0].to_case(Case::Pascal);
                let fname = parts[1].to_case(Case::Snake);

                Self::create_class_table_if_missing(api, cname.as_str(), engine)?;

                let Ok(table) = api.raw_get::<Table>(cname.as_str()) else { return Err(anyhow!("table['{cname}'] is not a table")) };

                Self::generate_new_method(engine, &table)?;

                self.generate_function(engine, &table, fname.as_str(), metadata)?;
            } else {
                let name = name.to_case(Case::Snake);
                self.generate_function(engine, api, name.as_str(), metadata)?;
            };
        }

        Ok(())
    }

    pub fn load_script(&mut self, path: &Path) -> Result<()> {

        let lua = fs::read_to_string(path)?;

        let engine = Lua::new();
        let mut api = engine.create_table().map_err(|e| anyhow!("Failed to create lua table: {e}"))?;

        self.bind_lua(&mut api, &engine)?;

        let env = engine.create_table().map_err(|e| anyhow!("Failed to create lua table: {e}"))?;

        env.set("turing_api", api).map_err(|e| anyhow!("Failed to set turing_api table: {e}"))?;

        env.set(
            "math",
            engine.globals()
                .get::<Table>("math").map_err(|e| anyhow!("Couldn't get math module: {e}"))?
        ).map_err(|e| anyhow!("Failed to add math module to environment: {e}"))?;


        let require = engine.create_function(|lua, name: String| {
            if name == "turing_api" {
                lua.globals().get("turing_api")
            } else {
                Err::<Table, _>(mlua::Error::RuntimeError(format!("Module '{name}' no found")))
            }
        }).map_err(|e| anyhow!("Failed to define 'require' function: {e}"))?;

        env.set("require", require).map_err(|e| anyhow!("Failed to add 'require' to env: {e}"))?;

        let module: Table = engine.load(lua)
            .set_environment(env)
            .eval().map_err(|e| anyhow!("Failed to evaluate module: {e}"))?;

        self.engine = Some((engine, module));

        Ok(())
    }

    pub fn call_fn(
        &mut self,
        name: &str,
        params: Params,
        ret_type: DataType,
        data: Arc<RwLock<WasmDataState>>
    ) -> Param {
        let Some((lua, module)) = &mut self.engine else {
            return Param::Error("No script is loaded".to_string())
        };

        let func = module.get::<Value>(name);
        if let Err(e) = func {
            return Param::Error(format!("Failed to find function '{name}'"));
        }
        let func = func.unwrap();
        let args = params.to_lua_args(lua, &data);
        if let Err(e) = args {
            return Param::Error(format!("{e}"))
        }
        let args = args.unwrap();

        let res = match func {
            Value::Function(f) => {
                f.call::<Value>(args)
            }
            _ => return Param::Error(format!("'{name}' is not a function"))
        };
        
        if let Err(e) = res {
            return Param::Error(e.to_string());
        }
        let res = res.unwrap();
        if res.is_null() || res.is_nil() {
            return Param::Void;
        }
        
        Param::from_lua_type_val(ret_type, res, &data, &lua)
    }

}

fn lua_bind_env<Ext: ExternalFunctions>(
    data: &Arc<RwLock<WasmDataState>>,
    lua: &Lua,
    cap: &str,
    ps: &LuaVariadic<Value>,
    p: &[DataType],
    func: &WasmCallback
) -> mlua::Result<Value> {

    if !data.read().active_capabilities.contains(cap) {
        return Err(mlua::Error::RuntimeError(format!("Mod capability '{cap}' is not currently loaded")))
    }

    let mut params = Params::of_size(p.len() as u32);
    for (exp_typ, value) in p.iter().zip(ps.iter()) {
        params.push(exp_typ.to_lua_val_param(value, &data)?)
    }

    let ffi_params = params.to_ffi::<Ext>();
    let ffi_params_struct = ffi_params.as_ffi_array();

    let res = func(ffi_params_struct).into_param::<Ext>().map_err(|e| mlua::Error::RuntimeError("unreachable".to_string()))?;

    let mut s = data.write();

    Ok(match res {
        Param::I8(i) => Value::Integer(i as i64),
        Param::I16(i) => Value::Integer(i as i64),
        Param::I32(i) => Value::Integer(i as i64),
        Param::I64(i) => Value::Integer(i),
        Param::U8(u) => Value::Integer(u as i64),
        Param::U16(u) => Value::Integer(u as i64),
        Param::U32(u) => Value::Integer(u as i64),
        Param::U64(u) => Value::Integer(u as i64),
        Param::F32(f) => Value::Number(f as f64),
        Param::F64(f) => Value::Number(f),
        Param::Bool(b) => Value::Boolean(b),
        Param::String(s) => Value::String(lua.create_string(&s)?),
        Param::Object(pointer) => {
            let pointer = ExtPointer::from(pointer);
            let opaque = s.get_opaque_pointer(pointer);
            Value::Integer(opaque.0.as_ffi() as i64)
        }
        Param::Error(er) => {
            return Err(mlua::Error::RuntimeError(format!("Error executing C# function: {er}")))
        }
        Param::Void => Value::Nil
    })
}
