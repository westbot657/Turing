use std::ffi::{CStr, CString};

use anyhow::{anyhow, Result};

use crate::interop::params::{param_type, Param};
use crate::{add_wasm_fn_param_type, bind_params, call_wasm_fn, create_params, create_wasm_fn, load_script};


#[test]
pub fn setup_wasm() -> Result<()> {
    unsafe {
        init_turing();
        let cstr = CString::new("log_info").unwrap().into_raw();
        let res = create_wasm_fn(cstr, pointer).to_param()?;
        if let Param::Error(e) = res {
            let cstr = CStr::from_ptr(e).to_string_lossy().to_string();
            return Err(anyhow!("Creation of wasm function failed"));
        }
        let res = add_wasm_fn_param_type(param_type::STRING).to_param()?;
        if let Param::Error(e) = res {
            let cstr = CStr::from_ptr(e).to_string_lossy().to_string();
            return Err(anyhow!("Addition of function parameter type failed"));
        }


        let res = init_wasm().to_param()?;
        if let Param::Error(e) = res {
            let cstr = CStr::from_ptr(e).to_string_lossy().to_string();
            return Err(anyhow!("Failed to initialize wasm engine"));
        }

    }
    Ok(())
}


#[test]
pub fn test_file_access() -> Result<()> {
    setup_wasm()?;

    unsafe {
        let fp = r#"~/turing/WasmTests/target/wasm32-wasip1/release/wasm_tests.wasm"#;

        let c_ptr = CString::new(fp).unwrap().into_raw();

        load_script(c_ptr).to_param()?.to_result()?;


        let name = CString::new("file_access_test").unwrap();

        call_wasm_fn(name.as_ptr(), 0);

    }

    Ok(())
}



