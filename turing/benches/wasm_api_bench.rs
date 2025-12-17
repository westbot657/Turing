use criterion::{Criterion, criterion_group, criterion_main};
use turing::engine::types::ScriptFnMetadata;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::hint::black_box;
use std::io::Write;

use turing::interop::params::{DataType, FfiParam, FfiParamArray, Param, Params};
use turing::{ExternalFunctions, Turing};

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

    let mut meta = ScriptFnMetadata::new("test", log_info_wasm);
    let _ = meta.add_param_type(DataType::RustString);
    turing.add_function("log_info", meta).unwrap();

    let mut meta = ScriptFnMetadata::new("test", fetch_string);
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

    c.bench_function("turing_call_wasm_add", |b| {
        b.iter(|| {
            let mut params = Params::new();
            params.push(Param::I32(1));
            params.push(Param::I32(2));

            let res = turing.call_fn("add", params, DataType::I32);
            let _ = black_box(res.to_result::<i32>().unwrap());
        })
    });
}

fn bench_call_tests_wasm_math(c: &mut Criterion) {
    let mut turing = setup_turing_with_callbacks();
    turing
        .load_script("../tests/wasm/wasm_tests.wasm", &vec!["test"])
        .unwrap();

    c.bench_function("turing_call_tests_wasm_math", |b| {
        b.iter(|| {
            let mut params = Params::new();
            params.push(Param::F32(3.5));
            params.push(Param::F32(5.0));

            let res = turing.call_fn("math_ops_test", params, DataType::F32);
            let _ = black_box(res.to_result::<f32>().unwrap());
        })
    });
}

fn bench_fetch_string_from_wasm(c: &mut Criterion) {
    let mut turing = setup_turing_with_callbacks();
    turing
        .load_script("../tests/wasm/wasm_tests.wasm", &vec!["test"])
        .unwrap();

    c.bench_function("turing_fetch_string_from_wasm", |b| {
        b.iter(|| {
            let _ = turing.call_fn("test_string_fetch", Params::new(), DataType::Void);
        })
    });
}


criterion_group!(
    benches,
    bench_call_wasm_add,
    bench_call_tests_wasm_math,
    bench_fetch_string_from_wasm,
);
criterion_main!(benches);
