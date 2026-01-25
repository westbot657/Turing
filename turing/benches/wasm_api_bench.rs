use criterion::{Criterion, criterion_group, criterion_main};
use turing_rs::engine::types::ScriptFnMetadata;
use std::env;
use std::ffi::{c_void, CString};
use std::fs::File;
use std::hint::black_box;
use std::io::Write;
use turing_rs::interop::params::{DataType, FfiParam, FfiParamArray, FreeableDataType, Param, Params};
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

// called from wasm
extern "C" fn log_info_wasm(params: FfiParamArray) -> FfiParam {
    Param::Void.to_ext_param()
}

// called from wasm
extern "C" fn fetch_string(_params: FfiParamArray) -> FfiParam {
    Param::String("this is a host provided string!".to_string()).to_ext_param()
}

fn setup_turing_with_callbacks() -> Turing<DirectExt> {
    let mut turing = Turing::new();

    let mut meta = ScriptFnMetadata::new("test".to_owned(), log_info_wasm, None);
    let _ = meta.add_param_type(DataType::RustString, "msg");
    turing.add_function("log_info", meta).unwrap();

    let mut meta = ScriptFnMetadata::new("test".to_owned(), fetch_string, None);
    let _ = meta.add_return_type(DataType::ExtString);
    turing.add_function("fetch_string", meta).unwrap();

    turing.build().unwrap()
}

fn bench_call_wasm_add(c: &mut Criterion) {
    let mut turing = setup_turing_with_callbacks();

    // prepare a tiny wasm module that exports `add(i32,i32)->i32` and a memory
    let wat = r#"(module (memory (export "memory") 1) (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))"#;
    let wasm = wat::parse_str(wat).unwrap();

    let mut path = env::temp_dir();
    path.push("turing_bench_add.wasm");
    let mut file = File::create(&path).unwrap();
    file.write_all(&wasm).unwrap();

    let capabilities = vec!["test"]; // match the registered capability
    turing
        .load_script(path.to_str().unwrap(), &capabilities)
        .unwrap();
    let add = turing.get_fn_key("add").expect("fn add not available");

    c.bench_function("turing_call_wasm_add", |b| {
        b.iter(|| {
            let mut params = Params::new();
            params.push(Param::I32(1));
            params.push(Param::I32(2));

            let res = turing.call_fn(add, params, DataType::I32);
            let _ = black_box(res.to_result::<i32>().unwrap());
        })
    });
}

fn bench_call_tests_wasm_math(c: &mut Criterion) {
    let mut turing = setup_turing_with_callbacks();
    turing
        .load_script("../tests/wasm/wasm_tests.wasm", &vec!["test"])
        .unwrap();
    let math_ops_test = turing.get_fn_key("math_ops_test").expect("fn key not found");

    c.bench_function("turing_call_tests_wasm_math", |b| {
        b.iter(|| {
            let mut params = Params::new();
            params.push(Param::F32(3.5));
            params.push(Param::F32(5.0));

            let res = turing.call_fn(math_ops_test, params, DataType::F32);
            let _ = black_box(res.to_result::<f32>().unwrap());
        })
    });
}

fn bench_fetch_string_from_wasm(c: &mut Criterion) {
    let mut turing = setup_turing_with_callbacks();
    turing
        .load_script("../tests/wasm/wasm_tests.wasm", &vec!["test"])
        .unwrap();
    let test_string_fetch = turing.get_fn_key("test_string_fetch").expect("fetch not found");

    c.bench_function("turing_fetch_string_from_wasm", |b| {
        b.iter(|| {
            let _ = turing.call_fn(test_string_fetch, Params::new(), DataType::Void);
        })
    });
}

fn bench_call_wasm_update_and_fixed(c: &mut Criterion) {
    let mut turing = setup_turing_with_callbacks();

    // create a tiny wasm module exporting `update(f32)` and `fixed_update(f32)`
    let wat = r#"(module (memory (export "memory") 1)
        (func (export "on_update") (param f32) (local.get 0) drop)
        (func (export "on_fixed_update") (param f32) (local.get 0) drop))"#;
    let wasm = wat::parse_str(wat).unwrap();

    let mut path = env::temp_dir();
    path.push("turing_bench_update.wasm");
    let mut file = File::create(&path).unwrap();
    file.write_all(&wasm).unwrap();

    let capabilities = vec!["test"];
    turing
        .load_script(path.to_str().unwrap(), &capabilities)
        .unwrap();

    c.bench_function("turing_call_wasm_update", |b| {
        b.iter(|| {
            let _ = turing.fast_call_update(black_box(0.016_f32));
        })
    });

    c.bench_function("turing_call_wasm_fixed_update", |b| {
        b.iter(|| {
            let _ = turing.fast_call_fixed_update(black_box(0.016_f32));
        })
    });
}


criterion_group!(
    benches,
    bench_call_wasm_add,
    bench_call_tests_wasm_math,
    bench_fetch_string_from_wasm,
    bench_call_wasm_update_and_fixed,
);
criterion_main!(benches);