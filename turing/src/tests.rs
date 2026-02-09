use crate::engine::types::ScriptFnMetadata;
use crate::interop::params::{
    DataType, FfiParam, FfiParamArray, FfiParams, FreeableDataType, ObjectId, Param, Params,
};
use crate::interop::types::U32Buffer;
use crate::{ExternalFunctions, Turing};
use anyhow::Result;
use std::ffi::{CString, c_char, c_void};

struct DirectExt {}
impl ExternalFunctions for DirectExt {
    fn abort(error_type: String, error: String) -> ! {
        panic!("{}: {}", error_type, error)
    }

    fn log_info(msg: impl ToString) {
        println!("\x1b[38;2;50;200;50m[info]: {}\x1b[0m", msg.to_string())
    }

    fn log_warn(msg: impl ToString) {
        println!("\x1b[38;2;255;127;30m[warn]: {}\x1b[0m", msg.to_string())
    }

    fn log_debug(msg: impl ToString) {
        println!("\x1b[38;2;20;200;200m[debug]: {}\x1b[0m", msg.to_string())
    }

    fn log_critical(msg: impl ToString) {
        println!("\x1b[38;2;200;20;20m[critical]: {}\x1b[0m", msg.to_string())
    }

    fn free_string(ptr: *const c_char) {
        let _ = unsafe { CString::from_raw(ptr as *mut c_char) };
    }

    fn free_of_type(ptr: *mut c_void, typ: FreeableDataType) {
        unsafe { typ.free_ptr(ptr) }
    }

    fn free_u32_buffer(buf: U32Buffer) {
        buf.from_rust();
    }
}

struct ObjectA {
    value: u32,
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
            println!("\x1b[38;2;20;200;20m[wasm/info]: {}\x1b[0m", s);
            Param::Void.to_ext_param()
        }
        _ => Param::Error(format!(
            "Invalid argument type, expected String, got {:?}",
            msg
        ))
        .to_ext_param(),
    }
}

extern "C" fn fetch_string(_params: FfiParamArray) -> FfiParam {
    Param::String("this is a host provided string!".to_string()).to_ext_param()
}

extern "C" fn log_info_panic(_params: FfiParamArray) -> FfiParam {
    panic!("Host panic from log_info_panic");
}

extern "C" fn create_object_a(_params: FfiParamArray) -> FfiParam {
    let obj = Box::new(ObjectA { value: 41 });
    let ptr = Box::into_raw(obj) as *const c_void;
    Param::Object(ObjectId::from_ptr(ptr)).to_ext_param()
}

extern "C" fn object_a_foo(params: FfiParamArray) -> FfiParam {
    let Ok(local) = params.as_params::<DirectExt>() else {
        return Param::Error("Failed to unpack params".to_string()).to_ext_param();
    };

    let Some(obj) = local.get(0) else {
        return Param::Error("Missing argument: self".to_string()).to_ext_param();
    };

    let Param::Object(ptr) = obj else {
        return Param::Error(format!(
            "Invalid argument type, expected Object, got {:?}",
            obj
        ))
        .to_ext_param();
    };

    let obj = unsafe { &*(ptr.as_ptr() as *const ObjectA) };
    Param::I32((obj.value + 1) as i32).to_ext_param()
}

fn common_setup_direct(source: &str) -> Result<Turing<DirectExt>> {
    let mut turing = Turing::new();

    let mut metadata = ScriptFnMetadata::new("test".to_owned(), log_info_wasm, None);
    metadata.add_param_type(DataType::RustString, "msg")?;
    turing.add_function("log::info", metadata)?;

    let mut metadata = ScriptFnMetadata::new("test".to_owned(), fetch_string, None);
    metadata.add_return_type(DataType::ExtString)?;
    turing.add_function("fetch_string", metadata)?;

    let mut metadata = ScriptFnMetadata::new("test".to_owned(), log_info_panic, None);
    metadata.add_param_type(DataType::RustString, "msg")?;
    turing.add_function("do_panic", metadata)?;

    let mut metadata = ScriptFnMetadata::new("test".to_owned(), create_object_a, None);
    metadata.add_return_type_named(DataType::Object, "ObjectA".to_string())?;
    turing.add_function("create_ObjectA", metadata)?;

    let mut metadata = ScriptFnMetadata::new("test".to_owned(), object_a_foo, None);
    metadata.add_return_type(DataType::I32)?;
    turing.add_function("ObjectA.foo", metadata)?;

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

    println!(
        "\x1b[38;2;200;200;20m[test/ext]: code multiplied 3.5 by 5.0 for {:#?}\x1b[0m",
        res
    );
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
    s.push(Param::String("Message from host".to_string()));

    let res = turing
        .call_fn_by_name("string_test", s, DataType::ExtString)
        .to_result::<String>()?;

    println!("\x1b[38;2;200;200;20mReceived message from lua: '{res}'\x1b[0m");
    Ok(())
}

#[test]
pub fn test_wasm_panic() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    let res = turing
        .call_fn_by_name("test_panic", Params::new(), DataType::Void)
        .to_result::<()>();

    assert!(res.is_err());

    let err = res.unwrap_err().to_string();
    assert!(err.contains("panic") || err.contains("unreachable") || err.contains("trap"));
    Ok(())
}

#[test]
pub fn test_wasm_object_call() -> Result<()> {
    // Use the pre-built wasm_tests.wasm produced by the `tests` crate build.
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    // create a boxed value and take a pointer to it
    let boxed = Box::new(0xCAFEBABEu64);
    let ptr = boxed.as_ref() as *const u64 as *const c_void;

    let mut params = Params::new();
    params.push(Param::Object(ObjectId::from_ptr(ptr)));

    let res = turing.call_fn_by_name("object_test", params, DataType::Object);

    match res {
        Param::Object(p) => assert_eq!(p.as_ffi() as usize, ptr as usize),
        other => panic!("Unexpected return from object_test: {:#?}", other),
    }

    Ok(())
}

#[test]
pub fn test_wasm_object_method_roundtrip() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    let res = turing.call_fn_by_name("object_test2", Params::new(), DataType::I32);
    assert_eq!(res.to_result::<i32>()?, 42);
    Ok(())
}

/// Tests that a panic in a host function called from WASM is properly propagated
/// to the caller when using the DirectExt external functions.
#[test]
#[ignore]
pub fn test_host_wasm_host_panic() -> Result<()> {
    let mut turing = common_setup_direct(WASM_SCRIPT)?;

    // loading the WASM will call `on_load` which calls the host `log::info` and should panic
    let caps = vec!["test"];
    turing.load_script(WASM_SCRIPT, &caps)?;

    let result = turing.call_fn_by_name("test_panic", Params::new(), DataType::Void);

    let Param::Error(err) = result else {
        panic!("Expected error from panic, got: {:#?}", result);
    };

    assert!(
        err.contains("Host panic from log_info_panic"),
        "Error did not contain expected panic message, got:\n{err}"
    );
    Ok(())
}
