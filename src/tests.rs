use std::ffi::CString;

use anyhow::{anyhow, Result};

use crate::interop::params::{param_type, Param};
use crate::*;


extern "C" fn log_info_stand_in(msg: *const c_char) {
    
}


//#[test]
pub fn setup_wasm() -> Result<()> {
    unsafe {
        init_turing();
        let cstr = CString::new("log_info")?.into_raw();

        let pointer = log_info_stand_in as *const c_void;

        let res = create_wasm_fn(cstr, pointer).to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Creation of wasm function failed"));
        }
        let res = add_wasm_fn_param_type(param_type::STRING).to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Addition of function parameter type failed"));
        }


        let res = init_wasm().to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Failed to initialize wasm engine"));
        }

    }
    Ok(())
}


#[test]
pub fn test_file_access() -> Result<()> {
    setup_wasm()?;

    unsafe {
        let fp = r#"/home/westbot/turing/WasmTests/target/wasm32-wasip1/release/wasm_tests.wasm"#;

        let c_ptr = CString::new(fp)?.into_raw();

        load_script(c_ptr).to_param()?.to_result()?;


        let name = CString::new("file_access_test")?;

        call_wasm_fn(name.as_ptr(), 0).to_param()?.to_result()?;

    }

    Ok(())
}



