#![allow(static_mut_refs, clippy::new_without_default)]

pub mod interop;
pub mod util;
pub mod wasm;

pub mod ffi;

#[cfg(test)]
pub mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString, c_char, c_void};
use std::{mem, panic, path};

use anyhow::{Result, anyhow};
use slotmap::{Key, SlotMap, new_key_type};
use wasmtime::{Caller, Engine, FuncType, Linker, Memory, MemoryAccessError, Val, ValType};
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::ffi::{FfiCallback, wasm_bind_env, wasm_host_strcpy};
use crate::wasm::wasm_engine::WasmInterpreter;

use crate::interop::params::{ParamType, Params};

use self::interop::params::{FfiParam, Param};
use self::util::{ToCStr, TrackedHashMap};



type AbortFn = extern "C" fn(*const c_char, *const c_char);
type LogFn = extern "C" fn(*const c_char);
type FreeStr = extern "C" fn(*const c_char);

/// pre-defined functions that turing.rs uses directly.
pub struct CsFns {
    pub abort: AbortFn,
    pub log_info: LogFn,
    pub log_warn: LogFn,
    pub log_critical: LogFn,
    pub log_debug: LogFn,
    pub free_cs_string: FreeStr,
}

extern "C" fn null_abort(_: *const c_char, _: *const c_char) {}
extern "C" fn null_log(_: *const c_char) {}
extern "C" fn null_free(_: *const c_char) {}
impl CsFns {
    pub fn new() -> Self {
        Self {
            abort: null_abort,
            log_info: null_log,
            log_warn: null_log,
            log_critical: null_log,
            log_debug: null_log,
            free_cs_string: null_free,
        }
    }

    /// # Safety
    /// 404 safety not found (only safe if the function pointer points to a perfectly matching
    ///     function as the default)
    pub unsafe fn link(&mut self, fn_name: &str, ptr: *const c_void) {
        unsafe {
            match fn_name {
                "abort" => self.abort = mem::transmute(ptr),
                "log_info" => self.log_info = mem::transmute(ptr),
                "log_warn" => self.log_warn = mem::transmute(ptr),
                "log_critical" => self.log_critical = mem::transmute(ptr),
                "log_debug" => self.log_debug = mem::transmute(ptr),
                "free_cs_string" => self.free_cs_string = mem::transmute(ptr),
                _ => {
                    eprintln!("Unrecognized function name: {}", fn_name);
                }
            }
        }
    }
}

impl Default for CsFns {
    fn default() -> Self {
        Self::new()
    }
}

type WasmFunctionMetadata = (String, *const c_void, Vec<ParamType>, Vec<ParamType>);

#[derive(Default)]
pub struct TuringState {
    /// The WASM engine
    pub wasm: Option<WasmInterpreter>,
    /// collection of functions to link to wasm. Only used during the initialization phase
    pub wasm_fns: HashMap<String, WasmFunctionMetadata>,
    /// id-tracking map of param objects for ffi.
    pub param_builders: SlotMap<ParamsKey, Params>,
    pub active_builder: Option<ParamsKey>,
    /// active WASM function definition for use in the initialization phase
    pub active_wasm_fn: Option<String>,

    /// Stores state that is passed around
    pub turing_mini_ctx: TuringDataState,
}

#[derive(Default)]
pub struct TuringDataState {
    /// maps opaque pointer ids to real pointers
    pub opaque_pointers: SlotMap<OpaquePointerKey, *const c_void>,
    /// maps real pointers back to their opaque pointer ids
    pub pointer_backlink: HashMap<*const c_void, OpaquePointerKey>,
    /// queue of strings for wasm to fetch (needed due to reentrancy limitations)
    pub str_cache: VecDeque<String>,
}

pub type ParamKey = u64;
pub const PARAM_KEY_INVALID: ParamKey = 0;

new_key_type! {
    pub struct ParamsKey;
    pub struct OpaquePointerKey;
}

trait IntoWasm<T> {
    fn into_wasm(self) -> Result<T, wasmtime::Error>;
}

impl<T, E> IntoWasm<T> for Result<T, E>
where
    E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
{
    fn into_wasm(self) -> Result<T, wasmtime::Error> {
        self.map_err(|e| wasmtime::Error::msg(format!("{}", e)))
    }
}

/// gets a string out of wasm memory into rust memory.
fn get_string(message: u32, data: &[u8]) -> String {
    CStr::from_bytes_until_nul(&data[message as usize..])
        .expect("Not a valid CStr")
        .to_string_lossy()
        .to_string()
    // let mut output_string = String::new();
    // for i in message..u32::MAX {
    //     let byte: &u8 = data.get(i as usize).unwrap();
    //     if *byte == 0u8 {
    //         break;
    //     }
    //     output_string.push(char::from(*byte));
    // }
    // output_string
}

/// writes a string from rust memory to wasm memory.
fn write_string(
    pointer: u32,
    string: String,
    memory: &Memory,
    caller: Caller<'_, WasiP1Ctx>,
) -> Result<(), MemoryAccessError> {
    let string = CString::new(string).unwrap();
    let string = string.into_bytes_with_nul();
    memory.write(caller, pointer as usize, &string)
}

impl TuringState {
    pub fn new() -> Self {
        Self {
            wasm: None,
            wasm_fns: HashMap::new(),
            param_builders: SlotMap::with_key(),
            active_builder: None,
            active_wasm_fn: None,
            turing_mini_ctx: TuringDataState::default(),
        }
    }

    pub fn push_param(&mut self, param: Param) -> Result<()> {
        let Some(builder) = self.active_builder else {
            return Err(anyhow!("active builder not set"));
        };

        match self.param_builders.get_mut(builder) {
            Some(builder) => {
                builder.push(param);
                Ok(())
            }
            None => Err(anyhow!("param object does not exist")),
        }
    }

    pub fn set_param(&mut self, index: u32, value: Param) -> Result<()> {
        let Some(builder) = self.active_builder else {
            return Err(anyhow!("active builder not set"));
        };

        let Some(builder) = self.param_builders.get_mut(builder) else {
            return Err(anyhow!("param object does not exist"));
        };
        if builder.len() > index {
            builder.set(index, value);
            return Ok(());
        }
        if builder.len() == index {
            builder.push(value);
            return Ok(());
        }
        Err(anyhow!("Index out of bounds"))
    }

    pub fn read_param(&self, index: u32) -> Param {
        let Some(builder) = self.active_builder else {
            return Param::Error("active builder not set".to_string());
        };

        if let Some(builder) = self.param_builders.get(builder) {
            if index >= builder.len() {
                Param::Error("Index out of bounds".to_string())
            } else if let Some(val) = builder.get(index as usize) {
                val.clone()
            } else {
                Param::Error("Could not read param for unknown reason".to_string())
            }
        } else {
            Param::Error("param object does not exist".to_string())
        }
    }

    pub fn swap_params(&mut self, params: ParamsKey) -> Result<Params> {
        let p = Params::new();
        if let Some(slot) = self.param_builders.get_mut(params) {
            let old = mem::replace(slot, p);
            Ok(old)
        } else {
            Err(anyhow!("Params object does not exist"))
        }
    }

    pub fn bind_wasm(&mut self, engine: &Engine, linker: &mut Linker<WasiP1Ctx>) {
        unsafe {
            // Utility Functions

            // _host_strcpy(location: *const c_char, size: u32);
            // Should only be used in 2 situations:
            // 1. after a call to a function that "returns" a string, the guest
            //    is required to allocate the size returned in place of the string, and then
            //    call this, passing the allocated pointer and the size.
            //    If the size passed in does not exactly match the cached string, or there is no
            //    cached string, then 0 is returned, otherwise the input pointer is returned.
            // 2. for each argument of a function that expects a string, in linear order,
            //    failing to retrieve all param strings in the correct order will invalidate
            //    the strings with no way to recover.

            linker
                .func_new(
                    "env",
                    "_host_strcpy",
                    FuncType::new(engine, vec![ValType::I32, ValType::I32], vec![ValType::I32]),
                    wasm_host_strcpy,
                )
                .unwrap();

            // C# wasm bindings
            let fns;
            {
                fns = self.wasm_fns.clone();
            }

            // n: wasm name, cap: capability (mod), p: param types, r: return type.
            for (n, (cap, func_ptr, p, r)) in fns.iter() {
                let func: FfiCallback = mem::transmute(*func_ptr);

                // unpack ffi type ids into wasm types
                let mut p_types = Vec::new();
                let mut r_type = Vec::new();
                for pt in p {
                    let p_type = match pt {
                        ParamType::I8
                        | ParamType::I16
                        | ParamType::I32
                        | ParamType::U8
                        | ParamType::U16
                        | ParamType::U32
                        | ParamType::BOOL
                        | ParamType::STRING
                        | ParamType::OBJECT => ValType::I32,

                        ParamType::F32 => ValType::F32,
                        _ => unreachable!("invalid parameter type"),
                    };
                    p_types.push(p_type);
                }
                if !r.is_empty() {
                    let r_typ = match r[0] {
                        ParamType::I8
                        | ParamType::I16
                        | ParamType::I32
                        | ParamType::U8
                        | ParamType::U16
                        | ParamType::U32
                        | ParamType::BOOL
                        | ParamType::STRING
                        | ParamType::OBJECT => ValType::I32,
                        ParamType::F32 => ValType::F32,
                        _ => unreachable!("invalid return type"),
                    };
                    r_type.push(r_typ);
                }

                // register function to wasm.
                let ft = FuncType::new(engine, p_types, r_type);
                let p = p.clone();

                linker
                    .func_new(
                        "env",
                        n.clone().as_str(),
                        ft,
                        move |caller: Caller<'_, WasiP1Ctx>, ps: &[Val], rs: &mut [Val]| {
                            wasm_bind_env(caller, ps, rs, p.clone(), func)
                        },
                    )
                    .unwrap();
            }
        }
    }
}
