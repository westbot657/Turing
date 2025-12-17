use std::ffi::{c_char, CString};
use anyhow::Result;
use crate::{ExternalFunctions, Turing};
use crate::interop::params::{DataType, FfiParam, FfiParamArray, Param, Params};
use crate::wasm::wasm_engine::WasmFnMetadata;


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

fn common_setup_direct() -> Result<Turing<DirectExt>> {
    let mut turing = Turing::new();

    let mut metadata = WasmFnMetadata::new("test", log_info_wasm);
    metadata.add_param_type(DataType::RustString)?;
    turing.add_function("log_info", metadata)?;

    let mut metadata = WasmFnMetadata::new("test", fetch_string);
    metadata.add_return_type(DataType::ExtString)?;
    turing.add_function("fetch_string", metadata)?;

    let mut turing = turing.build()?;
    setup_test_script(&mut turing)?;

    Ok(turing)
}

fn setup_test_script<Ext: ExternalFunctions + Send + Sync + 'static>(turing: &mut Turing<Ext>) -> Result<()> {
    let fp = r#"../tests/wasm/wasm_tests.wasm"#;
    let capabilities = vec!["test"];

    turing.load_script(fp, &capabilities)?;
    Ok(())
}

#[test]
pub fn test_file_access() -> Result<()> {
    let mut turing = common_setup_direct()?;

    let res = turing
        .call_fn("file_access_test", Params::new(), DataType::Void)
        .to_result::<()>();

    assert!(res.is_err());
    Ok(())
}

#[test]
pub fn test_math() -> Result<()> {
    let mut turing = common_setup_direct()?;

    let mut params = Params::new();
    params.push(Param::F32(3.5));
    params.push(Param::F32(5.0));

    let res = turing
        .call_fn("math_ops_test", params, DataType::F32);

    println!("[test/ext]: wasm code multiplied 3.5 by 5.0 for {:#?}", res);
    assert!((res.to_result::<f32>()? - 17.5).abs() < f32::EPSILON); 

    Ok(())
}

#[test]
pub fn test_stdin_fail() -> Result<()> {
    let mut turing = common_setup_direct()?;

    turing
        .call_fn("test_stdin_fail", Params::new(), DataType::Void)
        .to_result::<()>()
}

#[test]
pub fn test_string_fetch() -> Result<()> {
    let mut turing = common_setup_direct()?;

    turing
        .call_fn("test_string_fetch", Params::new(), DataType::Void)
        .to_result::<()>()
}

