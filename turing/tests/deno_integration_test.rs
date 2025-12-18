use std::sync::Arc;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use turing::{Turing, TuringSetup};
use turing::engine::types::ScriptFnMetadata;
use turing::interop::params::{DataType, Params, Param};

#[test]
fn deno_basic_call() {
    // This test only checks that loading and calling a deno script compiles and runs
    // in minimal fashion. It will skip if the deno feature is not enabled.
    #[cfg(not(feature = "deno"))]
    {
        eprintln!("deno feature not enabled; skipping test");
        return;
    }

    // build a Turing instance with a minimal dummy `ExternalFunctions` impl
    struct TestExt;
    impl turing::ExternalFunctions for TestExt {
        fn abort(_error_type: String, _error: String) -> ! { panic!("abort") }
        fn log_info(_msg: impl ToString) {}
        fn log_warn(_msg: impl ToString) {}
        fn log_debug(_msg: impl ToString) {}
        fn log_critical(_msg: impl ToString) {}
        fn free_string(_ptr: *const std::os::raw::c_char) {}
    }

    let setup: TuringSetup<TestExt> = Turing::new();
    let mut t = setup.build().unwrap();

    // load the test deno script
    let script_path = std::path::Path::new("tests/deno_test.js");
    // create engine directly
    let data = Arc::new(RwLock::new(turing::EngineDataState::default()));
    let script_fns: FxHashMap<String, ScriptFnMetadata> = FxHashMap::default();

    // load script via the public Turing API and call functions
    #[cfg(feature = "deno")]
    {
        t.load_script(script_path.to_string_lossy(), &["deno"]).unwrap();

        // call add(2,3) via Turing API
        let mut params = Params::of_size(2);
        params.push(Param::I32(2));
        params.push(Param::I32(3));
        let ret = t.call_fn("add", params, DataType::I32);
        match ret {
            Param::I32(v) => assert_eq!(v, 5),
            Param::I64(v) => assert_eq!(v as i32, 5),
            _ => panic!("unexpected return type: {:?}", ret),
        }

        // call makeOpaque(42) — the JS helper returns a numeric id in this test
        let mut p2 = Params::of_size(1);
        p2.push(Param::I64(42));
        let ret2 = t.call_fn("makeOpaque", p2, DataType::I64);
        match ret2 {
            Param::I64(v) => assert_eq!(v, 42),
            Param::U64(u) => assert_eq!(u as i64, 42),
            _ => panic!("expected numeric id from makeOpaque, got {:?}", ret2),
        }
    }
}
