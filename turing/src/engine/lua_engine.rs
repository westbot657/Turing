use crate::engine::runtime_modules::lua_glam;
use crate::engine::types::{ScriptCallback, ScriptFnMetadata};
use crate::interop::params::{DataType, ObjectId, Param, Params};
use crate::interop::types::Semver;
use crate::key_vec::KeyVec;
use crate::{EngineDataState, ExternalFunctions, ScriptFnKey};
use anyhow::{Result, anyhow};
use convert_case::{Case, Casing};
use mlua::prelude::*;
use mlua::{Function, MultiValue, Table, Value};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

fn vec_u32_to_lua_list(lua: &Lua, vec: Vec<u32>) -> mlua::Result<Value> {
    let table = lua.create_table_with_capacity(vec.len(), 0)?;

    for (i, v) in vec.into_iter().enumerate() {
        // Lua arrays are 1-indexed
        table.set((i + 1) as i64, v)?;
    }

    Ok(Value::Table(table))
}

fn lua_list_to_vec_u32(table: &Table) -> mlua::Result<Vec<u32>> {
    let len = table.len()? as usize;
    let mut vec = Vec::with_capacity(len);

    for i in 1..=len {
        let v: u32 = table
            .get(i as i64)
            .map_err(|_e| mlua::Error::FromLuaConversionError {
                from: "Lua value",
                to: "u32".to_string(),
                message: Some(format!("invalid value at index {}", i)),
            })?;
        vec.push(v);
    }

    Ok(vec)
}

impl DataType {
    pub fn to_lua_val_param(
        &self,
        val: &Value,
        _data: &Arc<RwLock<EngineDataState>>,
    ) -> mlua::Result<Param> {
        match (self, val) {
            (DataType::I8, Value::Integer(i)) => Ok(Param::I8(*i as i8)),
            (DataType::I16, Value::Integer(i)) => Ok(Param::I16(*i as i16)),
            (DataType::I32, Value::Integer(i)) => Ok(Param::I32(*i as i32)),
            (DataType::I64, Value::Integer(i)) => Ok(Param::I64(*i)),
            (DataType::U8, Value::Integer(u)) => Ok(Param::U8(*u as u8)),
            (DataType::U16, Value::Integer(u)) => Ok(Param::U16(*u as u16)),
            (DataType::U32, Value::Integer(u)) => Ok(Param::U32(*u as u32)),
            (DataType::U64, Value::Integer(u)) => Ok(Param::U64(*u as u64)),
            (DataType::F32, Value::Number(f)) => Ok(Param::F32(*f as f32)),
            (DataType::F64, Value::Number(f)) => Ok(Param::F64(*f)),
            (DataType::Bool, Value::Boolean(b)) => Ok(Param::Bool(*b)),
            (DataType::RustString | DataType::ExtString, Value::String(s)) => {
                Ok(Param::String(s.to_string_lossy()))
            }
            (DataType::Object, Value::Integer(t)) => {
                let op = *t as u64;
                Ok(Param::Object(ObjectId::new(op)))
            }
            (DataType::RustU32Buffer | DataType::ExtU32Buffer, Value::Table(t)) => {
                Ok(Param::U32Buffer(lua_list_to_vec_u32(t)?))
            }
            _ => Err(mlua::Error::RuntimeError(format!(
                "Mismatched parameter type: {self} with {val:?}"
            ))),
        }
    }
}

impl Param {
    pub fn from_lua_type_val(
        typ: DataType,
        val: Value,
        _data: &Arc<RwLock<EngineDataState>>,
        _lua: &Lua,
    ) -> Self {
        match typ {
            DataType::I8 => Param::I8(val.as_integer().unwrap() as i8),
            DataType::I16 => Param::I16(val.as_integer().unwrap() as i16),
            DataType::I32 => Param::I32(val.as_integer().unwrap() as i32),
            DataType::I64 => Param::I64(val.as_integer().unwrap()),
            DataType::U8 => Param::U8(val.as_integer().unwrap() as u8),
            DataType::U16 => Param::U16(val.as_integer().unwrap() as u16),
            DataType::U32 => Param::U32(val.as_integer().unwrap() as u32),
            DataType::U64 => Param::U64(val.as_integer().unwrap() as u64),
            DataType::F32 => Param::F32(val.as_number().unwrap() as f32),
            DataType::F64 => Param::F64(val.as_number().unwrap()),
            DataType::Bool => Param::Bool(val.as_boolean().unwrap()),
            // allocated externally, we copy the string
            DataType::RustString | DataType::ExtString => {
                Param::String(val.as_string().unwrap().to_string_lossy())
            }
            DataType::Object => Param::Object(ObjectId::new(val.as_integer().unwrap() as u64)),
            DataType::RustError | DataType::ExtError => {
                Param::Error(val.as_error().unwrap().to_string())
            }
            DataType::Void => Param::Void,
            DataType::Vec2 => lua_glam::unpack_vec2(val),
            DataType::Vec3 => lua_glam::unpack_vec3(val),
            DataType::RustVec4 | DataType::ExtVec4 => lua_glam::unpack_vec4(val),
            DataType::RustQuat | DataType::ExtQuat => lua_glam::unpack_quat(val),
            DataType::RustMat4 | DataType::ExtMat4 => lua_glam::unpack_mat4(val),
            DataType::RustU32Buffer | DataType::ExtU32Buffer => {
                Param::U32Buffer(lua_list_to_vec_u32(val.as_table().unwrap()).unwrap())
            }
        }
    }

    pub fn into_lua_val(
        self,
        data: &Arc<RwLock<EngineDataState>>,
        lua: &Lua,
    ) -> mlua::Result<Value> {
        let _s = data.write();

        Ok(match self {
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
            Param::Object(pointer) => Value::Integer(pointer.as_ffi() as i64),
            Param::Error(er) => {
                return Err(mlua::Error::RuntimeError(format!(
                    "Error executing C# function: {er}"
                )));
            }
            Param::Void => Value::Nil,
            Param::Vec2(v) => lua_glam::create_vec2(v, lua)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))?,
            Param::Vec3(v) => lua_glam::create_vec3(v, lua)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))?,
            Param::Vec4(v) => lua_glam::create_vec4(v, lua)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))?,
            Param::Quat(q) => lua_glam::create_quat(q, lua)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))?,
            Param::Mat4(m) => lua_glam::create_mat4(m, lua)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}", e)))?,
            Param::U32Buffer(b) => vec_u32_to_lua_list(lua, b)?,
        })
    }
}

impl Params {
    pub fn to_lua_args(self, lua: &Lua, data: &Arc<RwLock<EngineDataState>>) -> Result<MultiValue> {
        if self.is_empty() {
            return Ok(MultiValue::new());
        }
        let _s = data.write();
        let vals = self
            .params
            .into_iter()
            .map(|p| match p {
                Param::I8(i) => Ok(Value::Integer(i as i64)),
                Param::I16(i) => Ok(Value::Integer(i as i64)),
                Param::I32(i) => Ok(Value::Integer(i as i64)),
                Param::I64(i) => Ok(Value::Integer(i)),
                Param::U8(u) => Ok(Value::Integer(u as i64)),
                Param::U16(u) => Ok(Value::Integer(u as i64)),
                Param::U32(u) => Ok(Value::Integer(u as i64)),
                Param::U64(u) => Ok(Value::Integer(u as i64)),
                Param::F32(f) => Ok(Value::Number(f as f64)),
                Param::F64(f) => Ok(Value::Number(f)),
                Param::Bool(b) => Ok(Value::Boolean(b)),
                Param::String(s) => Ok(Value::String(lua.create_string(&s).unwrap())),
                Param::Object(rp) => Ok(Value::Integer(rp.as_ffi() as i64)),
                Param::Error(st) => Err(anyhow!("{st}")),
                Param::Void => unreachable!("Void shouldn't ever be added as an arg"),
                Param::Vec2(v) => lua_glam::create_vec2(v, lua).map_err(|e| anyhow!("{e}")),
                Param::Vec3(v) => lua_glam::create_vec3(v, lua).map_err(|e| anyhow!("{e}")),
                Param::Vec4(v) => lua_glam::create_vec4(v, lua).map_err(|e| anyhow!("{e}")),
                Param::Quat(q) => lua_glam::create_quat(q, lua).map_err(|e| anyhow!("{e}")),
                Param::Mat4(m) => lua_glam::create_mat4(m, lua).map_err(|e| anyhow!("{e}")),
                Param::U32Buffer(b) => vec_u32_to_lua_list(lua, b).map_err(|e| anyhow!("{e}")),
            })
            .collect::<Result<Vec<Value>>>()?;

        Ok(MultiValue::from_vec(vals))
    }
}

pub struct LuaInterpreter<Ext: ExternalFunctions> {
    lua_fns: FxHashMap<String, ScriptFnMetadata>,
    func_cache: KeyVec<ScriptFnKey, (String, Function)>,
    data: Arc<RwLock<EngineDataState>>,
    engine: Option<(Lua, Table, Table)>,
    fast_calls: FastCallLua,
    pub api_versions: FxHashMap<String, Semver>,
    _ext: PhantomData<Ext>,
}

#[derive(Default)]
struct FastCallLua {
    update: Option<Function>,
    fixed_update: Option<Function>,
}

impl<Ext: ExternalFunctions> LuaInterpreter<Ext> {
    pub fn new(
        lua_functions: &FxHashMap<String, ScriptFnMetadata>,
        data: Arc<RwLock<EngineDataState>>,
    ) -> Result<Self> {
        Ok(Self {
            lua_fns: lua_functions.clone(),
            func_cache: KeyVec::new(),
            data,
            engine: None,
            fast_calls: FastCallLua::default(),
            api_versions: Default::default(),
            _ext: PhantomData,
        })
    }

    fn generate_function(
        &self,
        lua: &Lua,
        table: &Table,
        name: &str,
        metadata: &ScriptFnMetadata,
    ) -> Result<()> {
        let cap = metadata.capability.clone();
        let callback = metadata.callback;
        let pts = metadata
            .param_types
            .iter()
            .map(|d| d.data_type)
            .collect::<Vec<_>>();
        let data = Arc::clone(&self.data);

        let func = lua
            .create_function(
                move |lua, args: LuaVariadic<Value>| -> mlua::Result<Value> {
                    lua_bind_env::<Ext>(&data, lua, &cap, &args, &pts, &callback)
                },
            )
            .map_err(|e| anyhow!("Failed to create function: {e}"))?;

        Ext::log_debug(format!("Adding function '{name}' to table"));
        table
            .set(name, func)
            .map_err(|e| anyhow!("Failed to set function: {e}"))?;

        Ok(())
    }

    fn create_class_table_if_missing(api: &Table, cname: &str, lua: &Lua) -> Result<()> {
        if api.raw_get::<Table>(cname).is_err() {
            let cls_table = lua
                .create_table()
                .map_err(|e| anyhow!("Failed to create table: {e}"))?;
            cls_table
                .raw_set("__index", cls_table.clone())
                .map_err(|e| anyhow!("Failed to set table as self's __index member: {e}"))?;
            Ext::log_debug(format!("Created new table: '{cname}'"));
            api.raw_set(cname, cls_table)
                .map_err(|e| anyhow!("Failed to add class to api: {e}"))?;
        }
        Ok(())
    }

    fn generate_new_method(lua: &Lua, class_table: &Table) -> Result<()> {
        if class_table.contains_key("new").unwrap_or(false) {
            return Ok(());
        }

        let new_fn = lua
            .create_function({
                let class_table = class_table.clone();
                move |lua, args: LuaVariadic<Value>| {
                    if args.len() != 1 {
                        return Err(mlua::Error::RuntimeError(format!(
                            "expected 1 argument, got {}",
                            args.len()
                        )));
                    }

                    let val = match &args[0] {
                        Value::Integer(i) => *i,
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "expected integer argument".to_string(),
                            ));
                        }
                    };

                    let instance = lua.create_table()?;
                    instance.set("opaqu", val)?;

                    instance.set_metatable(Some(class_table.clone()))?;

                    Ok(instance)
                }
            })
            .map_err(|e| anyhow!("Failed to create 'new' method: {e}"))?;

        class_table
            .set("new", new_fn)
            .map_err(|e| anyhow!("Failed to bind 'new' method: {e}"))?;

        Ok(())
    }

    fn bind_lua(&self, api: &Table, lua: &Lua) -> Result<()> {
        for (name, metadata) in self.lua_fns.iter() {
            if ScriptFnMetadata::is_instance_method(name) {
                let parts: Vec<&str> = name.splitn(2, ".").collect();
                let cname = parts[0].to_case(Case::Pascal);
                let fname = parts[1].to_case(Case::Snake);

                Self::create_class_table_if_missing(api, cname.as_str(), lua)?;

                let Ok(table) = api.raw_get::<Table>(cname.as_str()) else {
                    return Err(anyhow!("table['{cname}'] is not a table"));
                };
                self.generate_function(lua, &table, fname.as_str(), metadata)?;
            } else if ScriptFnMetadata::is_static_method(name) {
                let parts: Vec<&str> = name.splitn(2, ":").collect();
                let cname = parts[0].to_case(Case::Pascal);
                let fname = parts[1].to_case(Case::Snake);

                Self::create_class_table_if_missing(api, cname.as_str(), lua)?;

                let Ok(table) = api.raw_get::<Table>(cname.as_str()) else {
                    return Err(anyhow!("table['{cname}'] is not a table"));
                };

                Self::generate_new_method(lua, &table)?;

                self.generate_function(lua, &table, fname.as_str(), metadata)?;
            } else {
                let name = name.to_case(Case::Snake);
                self.generate_function(lua, api, name.as_str(), metadata)?;
            };
        }

        lua_glam::create_class_tables(lua, api)?;

        Ok(())
    }

    pub fn load_script(&mut self, path: &Path) -> Result<()> {
        let lua_src = fs::read_to_string(path)?;

        let lua = Lua::new();
        let api = lua
            .create_table()
            .map_err(|e| anyhow!("Failed to create lua table: {e}"))?;

        self.bind_lua(&api, &lua)?;

        let env = lua
            .create_table()
            .map_err(|e| anyhow!("Failed to create lua table: {e}"))?;

        env.set("turing_api", api.clone())
            .map_err(|e| anyhow!("Failed to set turing_api table: {e}"))?;

        env.set(
            "math",
            lua.globals()
                .get::<Table>("math")
                .map_err(|e| anyhow!("Couldn't get math module: {e}"))?,
        )
        .map_err(|e| anyhow!("Failed to add math module to environment: {e}"))?;

        let env2 = env.clone();
        let require = lua
            .create_function(move |_, name: String| -> mlua::Result<Value> {
                if name == "turing_api" {
                    env2.get::<Value>("turing_api")
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "Module '{name}' no found"
                    )))
                }
            })
            .map_err(|e| anyhow!("Failed to define 'require' function: {e}"))?;

        env.raw_set("require", require)
            .map_err(|e| anyhow!("Failed to add 'require' to env: {e}"))?;

        let module: Table = lua
            .load(lua_src)
            .set_environment(env)
            .eval()
            .map_err(|e| anyhow!("Failed to evaluate module: {e}"))?;

        let func = module.get::<Value>("on_update").map_err(|e| e.to_string());
        if let Ok(Value::Function(f)) = func {
            self.fast_calls.update = Some(f);
        }
        let func = module
            .get::<Value>("on_fixed_update")
            .map_err(|e| e.to_string());
        if let Ok(Value::Function(f)) = func {
            self.fast_calls.fixed_update = Some(f);
        }

        for pair in module.pairs::<mlua::String, Function>() {
            let Ok((name, val)) = pair else { continue };
            let name = name.to_string_lossy();
            self.func_cache.push((name.clone(), val.clone()));
            if name.starts_with("_") && name.ends_with("_semver") {
                let Ok(version) = val.call::<Value>(MultiValue::new()) else {
                    continue;
                };
                let version = match version {
                    Value::Integer(i) => i as u64,
                    _ => continue,
                };
                let name = name
                    .strip_prefix("_")
                    .unwrap()
                    .strip_suffix("_semver")
                    .unwrap()
                    .to_string();
                self.api_versions.insert(name, Semver::from_u64(version));
            }
        }

        self.engine = Some((lua, module, api));

        Ok(())
    }

    pub fn call_fn(
        &mut self,
        cache_key: ScriptFnKey,
        params: Params,
        ret_type: DataType,
        data: &Arc<RwLock<EngineDataState>>,
    ) -> Param {
        let Some((lua, module, _)) = &mut self.engine else {
            return Param::Error("No script is loaded".to_string());
        };

        // we assume the function exists because we cached it earlier
        let (name, _) = &self.func_cache.get(&cache_key);
        let name = name.as_str();

        let func = module.get::<Value>(name);
        if let Err(e) = func {
            return Param::Error(format!("Failed to find function '{name}': {e}"));
        }
        let func = func.unwrap();
        let args = params.to_lua_args(lua, data);
        if let Err(e) = args {
            return Param::Error(format!("{e}"));
        }
        let args = args.unwrap();

        let res = match func {
            Value::Function(f) => f.call::<Value>(args),
            _ => return Param::Error(format!("'{name}' is not a function")),
        };

        if let Err(e) = res {
            return Param::Error(e.to_string());
        }
        let res = res.unwrap();
        if res.is_null() || res.is_nil() {
            return Param::Void;
        }

        Param::from_lua_type_val(ret_type, res, data, lua)
    }

    pub fn fast_call_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        if self.engine.is_none() {
            return Err("No script is loaded".to_string());
        };

        if let Some(f) = &self.fast_calls.update {
            f.call::<Value>(Value::Number(delta_time as f64))
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    pub fn fast_call_fixed_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        if self.engine.is_none() {
            return Err("No script is loaded".to_string());
        };

        if let Some(f) = &self.fast_calls.fixed_update {
            f.call::<Value>(Value::Number(delta_time as f64))
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    pub fn get_fn_key(&self, name: &str) -> Option<ScriptFnKey> {
        self.func_cache.key_of(|(n, _)| n == name)
    }
}

fn lua_bind_env<Ext: ExternalFunctions>(
    data: &Arc<RwLock<EngineDataState>>,
    lua: &Lua,
    cap: &str,
    ps: &LuaVariadic<Value>,
    p: &[DataType],
    func: &ScriptCallback,
) -> mlua::Result<Value> {
    if !data.read().active_capabilities.contains(cap) {
        return Err(mlua::Error::RuntimeError(format!(
            "Mod capability '{cap}' is not currently loaded"
        )));
    }

    let mut params = Params::of_size(p.len() as u32);
    for (exp_typ, value) in p.iter().zip(ps.iter()) {
        params.push(exp_typ.to_lua_val_param(value, data)?)
    }

    let ffi_params = params.to_ffi::<Ext>();
    let ffi_params_struct = ffi_params.as_ffi_array();

    func(ffi_params_struct)
        .into_param::<Ext>()
        .map_err(|_| mlua::Error::RuntimeError("unreachable".to_string()))?
        .into_lua_val(data, lua)
}
