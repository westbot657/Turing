use std::ffi::{c_char, c_void, CString};
use anyhow::Result;
use glam::{Mat2, Mat3, Mat4, Quat, Vec4};
use crate::engine::types::ScriptFnMetadata;
use crate::{ExternalFunctions, Turing};
use crate::interop::params::{DataType, FfiParam, FfiParamArray, FreeableDataType, Param, Params};


struct DirectExt {}
impl ExternalFunctions for DirectExt {
    fn abort(error_type: String, error: String) -> ! {
        panic!("{}: {}", error_type, error)
    }

    fn log_info(msg: impl ToString) {
        println!("[info]: {}", msg.to_string())
    }

    fn log_warn(msg: impl ToString) {
        println!("[warn]: {}", msg.to_string())
    }

    fn log_debug(msg: impl ToString) {
        println!("[debug]: {}", msg.to_string())
    }

    fn log_critical(msg: impl ToString) {
        println!("[critical]: {}", msg.to_string())
    }

    fn free_string(ptr: *const c_char) {
        let _ = unsafe { CString::from_raw(ptr as *mut c_char) };
    }

    fn free_of_type(ptr: *mut c_void, typ: FreeableDataType)  {
        unsafe {
            match typ {
                FreeableDataType::ExtVec4 => { drop(Box::from_raw(ptr as *mut Vec4)); }
                FreeableDataType::ExtQuat => { drop(Box::from_raw(ptr as *mut Quat)); }
                FreeableDataType::ExtMat2 => { drop(Box::from_raw(ptr as *mut Mat2)); }
                FreeableDataType::ExtMat3 => { drop(Box::from_raw(ptr as *mut Mat3)); }
                FreeableDataType::ExtMat4 => { drop(Box::from_raw(ptr as *mut Mat4)); }
            }
        }
    }
    
}

extern "C" fn log_info_wasm(params: FfiParamArray) -> FfiParam {
    let Ok(local) = params.as_params::<DirectExt>() else {
        return Param::Error("Failed to unpack params".to_string()).to_ext_param();
    };

    let Some(msg) = local.get(0) else {
        return Param::Error("Missing argument: msg".to_string()).to_ext_param();
    };

    match msg {
        Param::String(s) => {
            println!("[wasm/info]: {}", s);
            Param::Void.to_ext_param()
        },
        _ => Param::Error(format!("Invalid argument type, expected String, got {:?}", msg)).to_ext_param()
    }
}

extern "C" fn fetch_string(_params: FfiParamArray) -> FfiParam {
    Param::String("this is a host provided string!".to_string()).to_ext_param()
}

fn common_setup_direct(source: &str) -> Result<Turing<DirectExt>> {
    let mut turing = Turing::new();

    let mut metadata = ScriptFnMetadata::new("test", log_info_wasm);
    metadata.add_param_type(DataType::RustString)?;
    turing.add_function("log.info", metadata)?;

    let mut metadata = ScriptFnMetadata::new("test", fetch_string);
    metadata.add_return_type(DataType::ExtString)?;
    turing.add_function("fetch_string", metadata)?;

    let mut turing = turing.build()?;
    setup_test_script(&mut turing, source)?;

    Ok(turing)
}

const WASM_SCRIPT: &str = "../tests/wasm/wasm_tests.wasm";
const LUA_SCRIPT: &str = "../tests/wasm/lua_test.lua";

fn setup_test_script<Ext: ExternalFunctions + Send + Sync + 'static>(turing: &mut Turing<Ext>, source: &str) -> Result<()> {
    let capabilities = vec!["test"];

    turing.load_script(source, &capabilities)?;
    Ok(())
}

#[test]
pub fn test_file_access() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    let res = turing
        .call_fn("file_access_test", Params::new(), DataType::Void)
        .to_result::<()>();

    assert!(res.is_err());
    Ok(())
}

fn test_math(mut turing: Turing<DirectExt>) -> Result<()> {
    let mut params = Params::new();
    params.push(Param::F32(3.5));
    params.push(Param::F32(5.0));

    let res = turing
        .call_fn("math_ops_test", params, DataType::F32);

    println!("[test/ext]: code multiplied 3.5 by 5.0 for {:#?}", res);
    assert!((res.to_result::<f32>()? - 17.5).abs() < f32::EPSILON);

    Ok(())
}

#[test]
pub fn test_math_wasm() -> Result<()> {
    let turing = common_setup_direct(WASM_SCRIPT)?;
    test_math(turing)
}

#[test]
pub fn test_math_lua() -> Result<()> {
    let turing = common_setup_direct(LUA_SCRIPT)?;
    test_math(turing)
}

#[test]
pub fn test_stdin_fail() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    turing
        .call_fn("test_stdin_fail", Params::new(), DataType::Void)
        .to_result::<()>()
}

#[test]
pub fn test_string_fetch() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    turing
        .call_fn("test_string_fetch", Params::new(), DataType::Void)
        .to_result::<()>()
}

#[test]
pub fn test_lua_string_fetch() -> Result<()> {
    let mut turing = common_setup_direct(LUA_SCRIPT)?;

    let mut s = Params::of_size(1);
    s.push(Param::String("Message from host".to_string()));

    let res = turing
        .call_fn("string_test", s, DataType::ExtString)
        .to_result::<String>()?;

    println!("Received message from lua: '{res}'");
    Ok(())
}

