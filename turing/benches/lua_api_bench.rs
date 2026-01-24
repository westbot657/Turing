use criterion::{Criterion, criterion_group, criterion_main};
use std::ffi::{c_void, CString};
use std::hint::black_box;
use turing_rs::engine::types::ScriptFnMetadata;
use turing_rs::interop::params::{DataType, FreeableDataType, Param, Params};
use turing_rs::{ExternalFunctions, Turing};

struct DirectExt {}
impl ExternalFunctions for DirectExt {
    fn abort(_error_type: String, _error: String) -> ! {
        panic!("extern abort called")
    }

    fn log_info(msg: impl ToString) {
        let _ = msg.to_string();
    }
    fn log_warn(_msg: impl ToString) {}
    fn log_debug(_msg: impl ToString) {}
    fn log_critical(_msg: impl ToString) {}

    fn free_string(ptr: *const std::os::raw::c_char) {
        let _ = unsafe { CString::from_raw(ptr as *mut std::os::raw::c_char) };
    }

    fn free_of_type(ptr: *mut c_void, typ: FreeableDataType) {
        unsafe { typ.free_ptr(ptr) }
    }
}

extern "C" fn log_info_wasm(
    _params: turing_rs::interop::params::FfiParamArray,
) -> turing_rs::interop::params::FfiParam {
    Param::Void.to_ext_param()
}

extern "C" fn fetch_string(
    _params: turing_rs::interop::params::FfiParamArray,
) -> turing_rs::interop::params::FfiParam {
    Param::String(CString::new("this is a host provided string!").unwrap()).to_ext_param()
}

fn setup_turing_for_lua() -> Turing<DirectExt> {
    let mut turing = Turing::new();

    let mut meta = ScriptFnMetadata::new(Some("test".to_owned()), log_info_wasm, "::info(msg: &str) -> void : _log_info".to_string(), None);
    let _ = meta.add_param_type(DataType::RustString);
    turing.add_function("Log.info", meta).unwrap();

    let mut meta = ScriptFnMetadata::new(Some("test".to_owned()), fetch_string, "fetch_string() -> String : _fetch_string".to_string(), None);
    let _ = meta.add_return_type(DataType::ExtString);
    turing.add_function("fetch_string", meta).unwrap();

    turing.build().unwrap()
}

fn bench_turing_lua_math(c: &mut Criterion) {
    let mut turing = setup_turing_for_lua();

    // load the test Lua module used by the repo
    let lua_path = "../tests/wasm/lua_test.lua";
    turing.load_script(lua_path, &["test"]).unwrap();
    let math_ops_test = turing.get_fn_key("math_ops_test").expect("fn key not found");

    c.bench_function("turing_lua_math_ops", |b| {
        b.iter(|| {
            let mut params = Params::new();
            params.push(Param::F32(3.5));
            params.push(Param::F32(5.0));

            let res = turing.call_fn(math_ops_test, params, DataType::F32);
            let _ = black_box(res.to_result::<f32>().unwrap());
        })
    });
}

fn bench_turing_lua_string_roundtrip(c: &mut Criterion) {
    let mut turing = setup_turing_for_lua();
    let lua_path = "../tests/wasm/lua_test.lua";
    turing.load_script(lua_path, &["test"]).unwrap();
    let string_test = turing.get_fn_key("string_test").expect("fn key not found");

    c.bench_function("turing_lua_string_roundtrip", |b| {
        b.iter(|| {
            let mut params = Params::of_size(1);
            params.push(Param::String(CString::new("Message from host").unwrap()));

            let res = turing.call_fn(string_test, params, DataType::ExtString);
            let _ = black_box(res.to_result::<String>().unwrap());
        })
    });
}

criterion_group!(
    benches,
    bench_turing_lua_math,
    bench_turing_lua_string_roundtrip
);
criterion_main!(benches);
