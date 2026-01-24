use crate::engine::types::ScriptFnMetadata;
use crate::interop::params::{DataType, FfiParam, FfiParamArray, FreeableDataType, Param, Params};
use crate::{ExternalFunctions, Turing};
use anyhow::Result;
use std::ffi::{CStr, CString, c_char, c_void};

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

    fn free_of_type(ptr: *mut c_void, typ: FreeableDataType) {
        unsafe { typ.free_ptr(ptr) }
    }
}

extern "C" fn log_info_wasm(params: FfiParamArray) -> FfiParam {
    let Ok(local) = params.as_params::<DirectExt>() else {
        return Param::Error("Failed to unpack params".into()).to_ext_param();
    };

    let Some(msg) = local.get(0) else {
        return Param::Error("Missing argument: msg".into()).to_ext_param();
    };

    match msg {
        Param::String(s) => {
            println!("[wasm/info]: {}", s.to_string_lossy());
            Param::Void.to_ext_param()
        }
        _ => Param::Error(format!(
            "Invalid argument type, expected String, got {:?}",
            msg
        ).into())
        .to_ext_param(),
    }
}

extern "C" fn fetch_string(_params: FfiParamArray) -> FfiParam {
    Param::String(CString::new("this is a host provided string!").unwrap()).to_ext_param()
}

fn common_setup_direct(source: &str) -> Result<Turing<DirectExt>> {
    let mut turing = Turing::new();

    let mut metadata = ScriptFnMetadata::new(
        Some("test".to_owned()),
        log_info_wasm,
        "::info(msg: &str) -> void : _log_info".to_owned(),
        None,
    );
    metadata.add_param_type(DataType::RustString)?;
    turing.add_function("log.info", metadata)?;

    let mut metadata = ScriptFnMetadata::new(
        Some("test".to_owned()),
        fetch_string,
        "fetch_string() -> String : _fetch_string".to_string(),
        None,
    );
    metadata.add_return_type(DataType::ExtString)?;
    turing.add_function("fetch_string", metadata)?;

    let mut turing = turing.build()?;
    setup_test_script(&mut turing, source)?;

    Ok(turing)
}

const WASM_SCRIPT: &str = "../tests/wasm/wasm_tests.wasm";
const LUA_SCRIPT: &str = "../tests/wasm/lua_test.lua";

fn setup_test_script<Ext: ExternalFunctions + Send + Sync + 'static>(
    turing: &mut Turing<Ext>,
    source: &str,
) -> Result<()> {
    let capabilities = vec!["test"];

    turing.load_script(source, &capabilities)?;
    Ok(())
}

#[test]
pub fn test_file_access() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    let res = turing
        .call_fn_by_name("file_access_test", Params::new(), DataType::Void)
        .to_result::<()>();

    assert!(res.is_err());
    Ok(())
}

fn test_math(mut turing: Turing<DirectExt>) -> Result<()> {
    let mut params = Params::new();
    params.push(Param::F32(3.5));
    params.push(Param::F32(5.0));

    let res = turing.call_fn_by_name("math_ops_test", params, DataType::F32);

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
        .call_fn_by_name("test_stdin_fail", Params::new(), DataType::Void)
        .to_result::<()>()
}

#[test]
pub fn test_string_fetch() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    turing
        .call_fn_by_name("test_string_fetch", Params::new(), DataType::Void)
        .to_result::<()>()
}

#[test]
pub fn test_lua_string_fetch() -> Result<()> {
    let mut turing = common_setup_direct(LUA_SCRIPT)?;

    let mut s = Params::of_size(1);
    s.push(Param::String(CString::new("Message from host").unwrap()));

    let res = turing
        .call_fn_by_name("string_test", s, DataType::ExtString)
        .to_result::<String>()?;

    println!("Received message from lua: '{res}'");
    Ok(())
}
