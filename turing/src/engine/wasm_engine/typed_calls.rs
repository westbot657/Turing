use std::sync::Arc;

use anyhow::{Result, bail};
use parking_lot::RwLock;
use wasmtime::{Func, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::{
    EngineDataState,
    interop::params::{ObjectId, Param, Params},
};

pub enum TypedFuncEntry {
    NoParamsVoid(TypedFunc<(), ()>),
    NoParamsI32(TypedFunc<(), i32>),
    NoParamsI64(TypedFunc<(), i64>),
    NoParamsObject(TypedFunc<(), u64>),
    NoParamsF32(TypedFunc<(), f32>),
    NoParamsF64(TypedFunc<(), f64>),
    // update and fixed update
    F32ToVoid(TypedFunc<f32, ()>),

    I32ToI32(TypedFunc<i32, i32>),
    I64ToI64(TypedFunc<i64, i64>),
    F32ToF32(TypedFunc<f32, f32>),
    F64ToF64(TypedFunc<f64, f64>),
    I32I32ToI32(TypedFunc<(i32, i32), i32>),
}

impl TypedFuncEntry {
    pub fn invoke(
        &self,
        store: &mut Store<WasiP1Ctx>,
        args: Params,
        _data: &Arc<RwLock<EngineDataState>>,
    ) -> Result<Param, wasmtime::Error> {
        let get_object = |id: u64| -> Result<Param> { Ok(Param::Object(ObjectId::new(id))) };

        match self {
            TypedFuncEntry::NoParamsVoid(t) => t.call(store, ()).map(|_| Param::Void),
            TypedFuncEntry::NoParamsObject(t) => t.call(store, ()).and_then(get_object),
            TypedFuncEntry::NoParamsI32(t) => t.call(store, ()).map(Param::I32),
            TypedFuncEntry::NoParamsI64(t) => t.call(store, ()).map(Param::I64),
            TypedFuncEntry::NoParamsF32(t) => t.call(store, ()).map(Param::F32),
            TypedFuncEntry::NoParamsF64(t) => t.call(store, ()).map(Param::F64),
            TypedFuncEntry::I32ToI32(t) => {
                if args.len() != 1 {
                    bail!("Arg length `{}` != 1", args.len())
                }
                let a0 = match &args[0] {
                    Param::I32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i32 failed", args[0]),
                };
                t.call(store, a0).map(Param::I32)
            }
            TypedFuncEntry::I64ToI64(t) => {
                if args.len() != 1 {
                    bail!("Arg length `{}` != 1", args.len())
                }
                let a0 = match &args[0] {
                    Param::I64(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i64 failed", args[0]),
                };
                t.call(store, a0).map(Param::I64)
            }
            TypedFuncEntry::F32ToF32(t) => {
                if args.len() != 1 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::F32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> f32 failed", args[0]),
                };
                t.call(store, a0).map(Param::F32)
            }
            TypedFuncEntry::F32ToVoid(typed_func) => {
                if args.len() != 1 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::F32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> f32 failed", args[0]),
                };
                typed_func.call(store, a0).map(|_| Param::Void)
            }
            TypedFuncEntry::F64ToF64(t) => {
                if args.len() != 1 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::F64(v) => *v,
                    _ => bail!("Arg conversion {:?} -> f64 failed", args[0]),
                };
                t.call(store, a0).map(Param::F64)
            }
            TypedFuncEntry::I32I32ToI32(t) => {
                if args.len() != 2 {
                    bail!("Arg mismatch")
                }
                let a0 = match &args[0] {
                    Param::I32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i32 failed", args[0]),
                };
                let a1 = match &args[1] {
                    Param::I32(v) => *v,
                    _ => bail!("Arg conversion {:?} -> i32 failed", args[1]),
                };
                t.call(store, (a0, a1)).map(Param::I32)
            }
        }
    }

    pub fn from_func(store: &mut Store<WasiP1Ctx>, func: Func) -> Option<Self> {
        // try 0 params
        if let Ok(t) = func.typed::<(), ()>(&store) {
            return Some(TypedFuncEntry::NoParamsVoid(t));
        }
        if let Ok(t) = func.typed::<(), i32>(&store) {
            return Some(TypedFuncEntry::NoParamsI32(t));
        }
        if let Ok(t) = func.typed::<(), i64>(&store) {
            return Some(TypedFuncEntry::NoParamsI64(t));
        }
        if let Ok(t) = func.typed::<(), f32>(&store) {
            return Some(TypedFuncEntry::NoParamsF32(t));
        }
        if let Ok(t) = func.typed::<(), f64>(&store) {
            return Some(TypedFuncEntry::NoParamsF64(t));
        }

        // 1 param -> same-typed returns
        if let Ok(t) = func.typed::<f32, ()>(&store) {
            return Some(TypedFuncEntry::F32ToVoid(t));
        }

        if let Ok(t) = func.typed::<i32, i32>(&store) {
            return Some(TypedFuncEntry::I32ToI32(t));
        }
        if let Ok(t) = func.typed::<i64, i64>(&store) {
            return Some(TypedFuncEntry::I64ToI64(t));
        }
        if let Ok(t) = func.typed::<f32, f32>(&store) {
            return Some(TypedFuncEntry::F32ToF32(t));
        }
        if let Ok(t) = func.typed::<f64, f64>(&store) {
            return Some(TypedFuncEntry::F64ToF64(t));
        }

        // 2 params (i32,i32)->i32
        if let Ok(t) = func.typed::<(i32, i32), i32>(&store) {
            return Some(TypedFuncEntry::I32I32ToI32(t));
        }

        // Not a supported typed signature
        None
    }
}
