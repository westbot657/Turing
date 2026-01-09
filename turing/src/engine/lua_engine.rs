use std::fs;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;
use mlua::prelude::*;
use anyhow::{anyhow, Result};
use convert_case::{Case, Casing};
use mlua::{Function, MultiValue, Table, Value};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use slotmap::KeyData;
use crate::engine::types::{ScriptCallback, ScriptFnMetadata};
use crate::key_vec::KeyVec;
use crate::{EngineDataState, ExternalFunctions, OpaquePointerKey, ScriptFnKey};
use crate::interop::params::{DataType, Param, Params};
use crate::interop::types::{ExtPointer, Semver};


impl DataType {
    pub fn to_lua_val_param(&self, val: &Value, data: &Arc<RwLock<EngineDataState>>) -> mlua::Result<Param> {
        match (self, val) {
            (DataType::I8,  Value::Integer(i)) => Ok(Param::I8(*i as i8)),
            (DataType::I16, Value::Integer(i)) => Ok(Param::I16(*i as i16)),
            (DataType::I32, Value::Integer(i)) => Ok(Param::I32(*i as i32)),
            (DataType::I64, Value::Integer(i)) => Ok(Param::I64(*i)),
            (DataType::U8,  Value::Integer(u)) => Ok(Param::U8(*u as u8)),
            (DataType::U16, Value::Integer(u)) => Ok(Param::U16(*u as u16)),
            (DataType::U32, Value::Integer(u)) => Ok(Param::U32(*u as u32)),
            (DataType::U64, Value::Integer(u)) => Ok(Param::U64(*u as u64)),
            (DataType::F32, Value::Number(f)) => Ok(Param::F32(*f as f32)),
            (DataType::F64, Value::Number(f)) => Ok(Param::F64(*f)),
            (DataType::Bool, Value::Boolean(b)) => Ok(Param::Bool(*b)),
            (DataType::RustString | DataType::ExtString, Value::String(s)) => Ok(Param::String(s.to_string_lossy())),
            (DataType::Object, Value::Table(t)) => {
                let key = t.raw_get::<Value>("opaqu")?;
                let key = match key {
                    Value::Integer(i) => i as u64,
                    _ => return Err(mlua::Error::RuntimeError("Incorrect type for opaque handle".to_string()))
                };
                let pointer_key = OpaquePointerKey::from(KeyData::from_ffi(key));
                if let Some(true_pointer) = data.read().opaque_pointers.get(pointer_key) {
                    Ok(Param::Object(**true_pointer))
                } else {
                    Err(mlua::Error::RuntimeError("opaque pointer does not correspond to a real pointer".to_string()))
                }
            }
            _ => Err(mlua::Error::RuntimeError(format!("Mismatched parameter type: {self} with {val:?}")))
        }
    }
}

impl Param {
    pub fn from_lua_type_val(
        typ: DataType,
        val: Value,
        data: &Arc<RwLock<EngineDataState>>,
        _lua: &Lua
    ) -> Self {

        macro_rules! unpack_table {
            () => {};
        }

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
            DataType::RustString | DataType::ExtString => Param::String(val.as_string().unwrap().to_string_lossy()),
            DataType::Object => {
                let table = val.as_table().unwrap();
                let op = table.get("opaqu").unwrap();
                let key = OpaquePointerKey::from(KeyData::from_ffi(op));

                let real = data.read()
                    .opaque_pointers
                    .get(key)
                    .copied()
                    .unwrap_or_default();
                Param::Object(real.ptr)
            }
            DataType::RustError | DataType::ExtError => Param::Error(val.as_error().unwrap().to_string()),
            DataType::Void => Param::Void,
            DataType::Vec2 => todo!(),
            DataType::Vec3 => todo!(),
            DataType::RustVec4 | DataType::ExtVec4 => todo!(),
            DataType::RustQuat | DataType::ExtQuat => todo!(),
            DataType::RustMat4 | DataType::ExtMat4 => todo!(),
        }
    }

    pub fn into_lua_val(
        self,
        data: &Arc<RwLock<EngineDataState>>,
        lua: &Lua
    ) -> mlua::Result<Value> {
        let mut s = data.write();

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
            Param::Object(pointer) => {
                let pointer = ExtPointer::from(pointer);
                let opaque = s.get_opaque_pointer(pointer);
                Value::Integer(opaque.0.as_ffi() as i64)
            }
            Param::Error(er) => {
                return Err(mlua::Error::RuntimeError(format!("Error executing C# function: {er}")))
            }
            Param::Void => Value::Nil,
            Param::Vec2(v) => todo!(),
            Param::Vec3(v) => todo!(),
            Param::Vec4(v) => todo!(),
            Param::Quat(q) => todo!(),
            Param::Mat4(m) => todo!(),
        })
    }

}

impl Params {
    pub fn to_lua_args(self, lua: &Lua, data: &Arc<RwLock<EngineDataState>>) -> Result<MultiValue> {
        if self.is_empty() {
            return Ok(MultiValue::new())
        }
        let mut s = data.write();
        let vals = self.params.into_iter().map(|p|
            match p {
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
                Param::Object(rp) => {
                    let pointer = rp.into();
                    Ok(if let Some(op) = s.pointer_backlink.get(&pointer) {
                        Value::Integer(op.0.as_ffi() as i64)
                    } else {
                        let op = s.opaque_pointers.insert(pointer);
                        s.pointer_backlink.insert(pointer, op);
                        Value::Integer(op.0.as_ffi() as i64)
                    })
                }
                Param::Error(st) => {
                    Err(anyhow!("{st}"))
                }
                Param::Void => unreachable!("Void shouldn't ever be added as an arg"),
                Param::Vec2(v) => todo!(),
                Param::Vec3(v) => todo!(),
                Param::Vec4(v) => todo!(),
                Param::Quat(q) => todo!(),
                Param::Mat4(m) => todo!(),
            }
        ).collect::<Result<Vec<Value>>>()?;

        Ok(MultiValue::from_vec(vals))
    }
}

pub struct LuaInterpreter<Ext: ExternalFunctions> {
    lua_fns: FxHashMap<String, ScriptFnMetadata>,
    func_cache: KeyVec<ScriptFnKey, (String, Function)>,
    data: Arc<RwLock<EngineDataState>>,
    engine: Option<(Lua, Table)>,
    fast_calls: FastCallLua,
    pub api_versions: FxHashMap<String, Semver>,
    _ext: PhantomData<Ext>
}

#[derive(Default)]
struct FastCallLua {
    update: Option<Function>,
    fixed_update: Option<Function>,
}

impl<Ext: ExternalFunctions> LuaInterpreter<Ext> {

    pub fn new(lua_functions: &FxHashMap<String, ScriptFnMetadata>, data: Arc<RwLock<EngineDataState>>) -> Result<Self> {
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

    fn generate_function(&self, lua: &Lua, table: &Table, name: &str, metadata: &ScriptFnMetadata) -> Result<()> {
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
        for (name, metadata) in self.lua_fns.iter() {
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

    pub fn load_script(&mut self, path: &Path, data: &Arc<RwLock<EngineDataState>>) -> Result<()> {

        let lua = fs::read_to_string(path)?;

        let engine = Lua::new();
        let api = engine.create_table().map_err(|e| anyhow!("Failed to create lua table: {e}"))?;

        self.bind_lua(&api, &engine)?;

        let env = engine.create_table().map_err(|e| anyhow!("Failed to create lua table: {e}"))?;

        env.set("turing_api", api).map_err(|e| anyhow!("Failed to set turing_api table: {e}"))?;

        env.set(
            "math",
            engine.globals()
                .get::<Table>("math").map_err(|e| anyhow!("Couldn't get math module: {e}"))?
        ).map_err(|e| anyhow!("Failed to add math module to environment: {e}"))?;


        let env2 = env.clone();
        let require = engine.create_function(move |_, name: String| -> mlua::Result<Value> {
            if name == "turing_api" {
                env2.get::<Value>("turing_api")
            } else {
                Err(mlua::Error::RuntimeError(format!("Module '{name}' no found")))
            }
        }).map_err(|e| anyhow!("Failed to define 'require' function: {e}"))?;

        env.raw_set("require", require).map_err(|e| anyhow!("Failed to add 'require' to env: {e}"))?;


        let module: Table = engine.load(lua)
            .set_environment(env)
            .eval().map_err(|e| anyhow!("Failed to evaluate module: {e}"))?;
        
        let func = module.get::<Value>("on_update").map_err(|e| e.to_string());
        if let Ok(Value::Function(f)) = func {
            self.fast_calls.update = Some(f);
        }
        let func = module.get::<Value>("on_fixed_update").map_err(|e| e.to_string());
        if let Ok(Value::Function(f)) = func {
            self.fast_calls.fixed_update = Some(f);
        }

        for pair in module.pairs::<mlua::String, Function>() {
            let Ok((name, val)) = pair else { continue };
            let name = name.to_string_lossy();
            self.func_cache.push((name.clone(), val.clone()));
            if name.starts_with("_") && name.ends_with("_semver") {
                let Ok(version) = val.call::<Value>(MultiValue::new()) else { continue };
                let version = match version {
                    Value::Integer(i) => i as u64,
                    _ => continue
                };
                let name = name.strip_prefix("_").unwrap().strip_suffix("_semver").unwrap().to_string();
                self.api_versions.insert(name, Semver::from_u64(version));
            }
        }
        
        self.engine = Some((engine, module));

        Ok(())
    }

    pub fn call_fn(
        &mut self,
        cache_key: ScriptFnKey,
        params: Params,
        ret_type: DataType,
        data: &Arc<RwLock<EngineDataState>>
    ) -> Param {
        let Some((lua, module)) = &mut self.engine else {
            return Param::Error("No script is loaded".to_string())
        };
        
        // we assume the function exists because we cached it earlier
        let Some((name, _)) = &self.func_cache.get(&cache_key) else {
            return Param::Error(format!("Function with key '{cache_key:?}' not found"))
        };
        let name = name.as_str();

        let func = module.get::<Value>(name);
        if let Err(e) = func {
            return Param::Error(format!("Failed to find function '{name}': {e}"));
        }
        let func = func.unwrap();
        let args = params.to_lua_args(lua, data);
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
        
        Param::from_lua_type_val(ret_type, res, data, lua)
    }
    
    pub fn fast_call_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        if self.engine.is_none() {
            return Err("No script is loaded".to_string())
        };
        
        if let Some(f) = &self.fast_calls.update {
            f.call::<Value>(Value::Number(delta_time as f64)).map(|_| ()).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    pub fn fast_call_fixed_update(&mut self, delta_time: f32) -> std::result::Result<(), String> {
        if self.engine.is_none() {
            return Err("No script is loaded".to_string())
        };

        if let Some(f) = &self.fast_calls.fixed_update {
            f.call::<Value>(Value::Number(delta_time as f64)).map(|_| ()).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }
    
    pub fn get_fn_key(&self, name: &str) -> Option<ScriptFnKey> {
        println!("Looking for Lua function key for name: {}", name);
        println!("Available Lua functions:");
        println!("{:?}", self.lua_fns.iter().map(|(n, _)| n).collect::<Vec<&String>>());

        self.func_cache.key_of(|(n, _)| n == name)
    }
    
}

fn lua_bind_env<Ext: ExternalFunctions>(
    data: &Arc<RwLock<EngineDataState>>,
    lua: &Lua,
    cap: &str,
    ps: &LuaVariadic<Value>,
    p: &[DataType],
    func: &ScriptCallback
) -> mlua::Result<Value> {

    if !data.read().active_capabilities.contains(cap) {
        return Err(mlua::Error::RuntimeError(format!("Mod capability '{cap}' is not currently loaded")))
    }

    let mut params = Params::of_size(p.len() as u32);
    for (exp_typ, value) in p.iter().zip(ps.iter()) {
        params.push(exp_typ.to_lua_val_param(value, data)?)
    }

    let ffi_params = params.to_ffi::<Ext>();
    let ffi_params_struct = ffi_params.as_ffi_array();

    func(ffi_params_struct).into_param::<Ext>().map_err(|_| mlua::Error::RuntimeError("unreachable".to_string()))?.into_lua_val(data, lua)

}
