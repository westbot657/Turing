use criterion::{Criterion, black_box, criterion_group, criterion_main};
use turing::PARAM_KEY_INVALID;
use std::ffi::{CString, c_char};

use std::env;
use std::fs::File;
use std::io::Write;
use turing::ffi::{
    add_param, bind_params, call_wasm_fn, create_n_params, create_params, delete_params,
    init_turing, init_wasm, load_script, read_param, uninit_turing,
};
use turing::interop::params::{FfiParam, Param, ParamType, RawParam};
use wat;

fn bench_ffi_add_read_i32(c: &mut Criterion) {
    init_turing();

    c.bench_function("ffi_api_add_read_i32", |b| {
        b.iter(|| {
            let params = create_params();
            bind_params(params);

            // Construct FfiParam via From<Param>
            let f: FfiParam = FfiParam::from(Param::I32(123));
            let _res = add_param(f);

            let ret = read_param(0);
            let _ = black_box(ret.to_param()).unwrap();

            delete_params(params);
        })
    });

    uninit_turing();
}

fn bench_ffi_add_read_string(c: &mut Criterion) {
    init_turing();

    let original_s = "a".repeat(256);
    c.bench_function("ffi_api_add_read_string", |b| {
        b.iter(|| unsafe {
            let params = create_params();
            if params == PARAM_KEY_INVALID {
                panic!("Failed to create params");
            }
            bind_params(params);

            // Create a Param::String and convert to FfiParam (this allocates a C string pointer).
            let c_str = CString::new(black_box(original_s.clone())).unwrap();
            let raw_val: RawParam = std::mem::transmute(c_str.as_ptr() as *mut c_char);
            let f = FfiParam {
                type_id: ParamType::STRING,
                value: raw_val,
            };

            let result = add_param(f).to_param().unwrap();
            assert_eq!(result, Param::Void, "add_param failed {:?}", result);


            let ret = read_param(0);
            let ret_param = ret.to_param().unwrap();
            let Param::String(s) = black_box(ret_param.clone()) else {
                panic!("Expected Param::String, found {ret_param:?}");
            };

            delete_params(params);
        })
    });

    uninit_turing();
}

fn bench_call_wasm_add(c: &mut Criterion) {
    init_turing();
    init_wasm();

    // prepare a tiny wasm module that exports `add(i32,i32)->i32` and a memory
    let wat = r#"(module (memory (export "memory") 1) (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))"#;
    let wasm = wat::parse_str(wat).unwrap();

    let mut path = env::temp_dir();
    path.push("turing_bench_add.wasm");
    let mut file = File::create(&path).unwrap();
    file.write_all(&wasm).unwrap();

    let path_cs = CString::new(path.to_str().unwrap()).unwrap();
    let _ = unsafe { load_script(path_cs.as_ptr(), 0) };

    let name_cs = CString::new("add").unwrap();

    c.bench_function("ffi_api_call_wasm_add", |b| {
        b.iter(|| {
            let params = create_params();
            bind_params(params);

            let f1: FfiParam = FfiParam::from(Param::I32(1));
            let f2: FfiParam = FfiParam::from(Param::I32(2));
            let _ = add_param(f1);
            let _ = add_param(f2);

            let ret = unsafe { call_wasm_fn(name_cs.as_ptr(), params, ParamType::I32) };
            let _ = black_box(ret.to_param()).unwrap();

            delete_params(params);
        })
    });

    uninit_turing();
}

criterion_group!(
    benches,
    bench_ffi_add_read_i32,
    bench_ffi_add_read_string,
    bench_call_wasm_add
);
criterion_main!(benches);
