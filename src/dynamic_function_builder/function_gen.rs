use std::ffi::CString;
use std::mem::ManuallyDrop;
use wasmi::{AsContextMut, Caller, ExternRef, FuncType, Val};
use wasmi::core::ValType;
use crate::{abort, call_cs};
use crate::data::game_objects::Object;
use crate::interop::parameters::params::Parameters;
use crate::wasm::wasm_interpreter::HostState;

type F = dyn Fn(Caller<'_, HostState<ExternRef>>, &[Val], &mut [Val]) -> Result<(), wasmi::Error>;

macro_rules! push_parameter {
    ( $params:expr, $typ:ident: $obj:expr ) => {
        $params.push(crate::params::Param::$typ( crate::params::ParamData { $typ: std::mem::ManuallyDrop::new($obj) } ))
    };
}

macro_rules! get_return {
    ( $params:expr, $t:tt, $index:expr) => {
        {
            let raw = $params.params.remove($index);
            if raw.0 as u32 != crate::params::ParamType::$t as u32 {
                Err::<std::mem::ManuallyDrop<$t>, String>("wrong value type was returned".to_owned())
            }
            else {
                let p = crate::params::Param::$t(unsafe { raw.1 });
                match p {
                    crate::params::Param::$t(x) => Ok(unsafe { x.$t }),
                    _ => Err("wrong value type was returned".to_owned())
                }
            }

        }
    };
}


pub fn generate(name: &str, params: Vec<ValType>, results: Vec<ValType>) -> (FuncType, Box<F>) {
    (
        FuncType::new(params, results),
        Box::new(|mut caller, ps, rs| -> Result<(), wasmi::Error> {

            let mut p = Parameters::new();

            for i in 0..ps.len() {
                let v = ps.get(i).unwrap();
                let t = params.get(i);
                if let Some(tp) = t {
                    match tp {
                        ValType::I32 => {
                            push_parameter!(p, i32: v.i32().unwrap());
                        }
                        ValType::I64 => {
                            push_parameter!(p, i64: v.i64().unwrap());
                        }
                        ValType::F32 => {
                            push_parameter!(p, f32: v.f32().unwrap().to_float());
                        }
                        ValType::F64 => {
                            push_parameter!(p, f64: v.f64().unwrap().to_float());
                        }
                        ValType::V128 => {
                            let code = CString::new("Unimplemented").unwrap();
                            let msg = CString::new("Param type V128 not handled").unwrap();
                            unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                        }
                        ValType::FuncRef => {
                            let code = CString::new("Unimplemented").unwrap();
                            let msg = CString::new("Param type FuncRef not handled").unwrap();
                            unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                        }
                        ValType::ExternRef => {
                            let obj = caller.data().get(v.i32().unwrap() as u32).unwrap();
                            let o = obj.data(&caller).unwrap().downcast_ref::<Object>().unwrap();
                            push_parameter!(p, Object: *o);
                        }
                    }
                } else {
                    let code = CString::new("Argument Mismatch").unwrap();
                    let msg = CString::new(format!("Expected {} arguments, got {}", params.len(), ps.len())).unwrap();
                    unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                }
            }


            let r = unsafe { call_cs(name, p.pack()) };

            let mut r = unsafe { Parameters::unpack(&r) };

            for i in 0..results.len() {
                let t = results.get(i).unwrap();
                match t {
                    ValType::I32 => {
                        let x = get_return!(r, i32, i);
                        match x {
                            Ok(v) => {
                                let v = ManuallyDrop::into_inner(v);
                                let _ = rs.get(i).insert(&Val::I32(v));
                            }
                            Err(e) => {
                                let code = CString::new("Return Type Mismatch").unwrap();
                                let msg = CString::new(e).unwrap();
                                unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                            }
                        }
                    }
                    ValType::I64 => {
                        let x = get_return!(r, i64, i);
                        match x {
                            Ok(v) => {
                                let v = ManuallyDrop::into_inner(v);
                                let _ = rs.get(i).insert(&Val::I64(v));
                            }
                            Err(e) => {
                                let code = CString::new("Return Type Mismatch").unwrap();
                                let msg = CString::new(e).unwrap();
                                unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                            }
                        }
                    }
                    ValType::F32 => {
                        let x = get_return!(r, f32, i);
                        match x {
                            Ok(v) => {
                                let v = ManuallyDrop::into_inner(v);
                                let _ = rs.get(i).insert(&Val::F32(v.into()));
                            }
                            Err(e) => {
                                let code = CString::new("Return Type Mismatch").unwrap();
                                let msg = CString::new(e).unwrap();
                                unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                            }
                        }
                    }
                    ValType::F64 => {
                        let x = get_return!(r, f64, i);
                        match x {
                            Ok(v) => {
                                let v = ManuallyDrop::into_inner(v);
                                let _ = rs.get(i).insert(&Val::F64(v.into()));
                            }
                            Err(e) => {
                                let code = CString::new("Return Type Mismatch").unwrap();
                                let msg = CString::new(e).unwrap();
                                unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                            }
                        }
                    }
                    ValType::V128 => {
                        let code = CString::new("Unimplemented").unwrap();
                        let msg = CString::new("Return type V128 not handled").unwrap();
                        unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                    }
                    ValType::FuncRef => {
                        let code = CString::new("Unimplemented").unwrap();
                        let msg = CString::new("Return type FuncRef not handled").unwrap();
                        unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                    }
                    ValType::ExternRef => {
                        let x = get_return!(r, Object, i);
                        match x {
                            Ok(v) => {
                                let v = ManuallyDrop::into_inner(v);
                                let _ = rs.get(i).insert(&Val::ExternRef(ExternRef::new(&mut caller.as_context_mut(), v)));
                            }
                            Err(e) => {
                                let code = CString::new("Return Type Mismatch").unwrap();
                                let msg = CString::new(e).unwrap();
                                unsafe { abort(code.as_ptr(), msg.as_ptr()) }
                            }
                        }
                    }
                }
            }

            Ok(())
        })
    )
}

