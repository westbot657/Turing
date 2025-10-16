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

pub fn setup_test_script() -> Result<()> {
    unsafe {
        let fp = r#"tests/wasm/wasm_tests.wasm"#;

        let c_ptr = CString::new(fp)?.into_raw();

        load_script(c_ptr).to_param()?.to_result()
    }
}

#[test]
pub fn test_file_access() -> Result<()> {
    setup_wasm()?;

    unsafe {

        setup_test_script()?;

        let name = CString::new("file_access_test")?;

        let res = call_wasm_fn(name.as_ptr(), 0, param_type::VOID).to_param()?.to_result();

        assert!(res.is_err())
    }

    Ok(())
}

#[test]
pub fn test_math() -> Result<()> {
    setup_wasm()?;

    unsafe {

        setup_test_script()?;

        let name = CString::new("math_ops_test")?;

        let p = create_n_params(2);
        bind_params(p);
        set_param(0, Param::F32(3.5).into()).to_param()?.to_result()?;
        set_param(1, Param::F32(5.0).into()).to_param()?.to_result()?;

        let res = call_wasm_fn(name.as_ptr(), p, param_type::F32).to_param()?;

        match res {
            Param::F32(f) => {
                println!("wasm code added 3.5 to 5.0 for {}", f);
                assert_eq!(f, 3.5 * 5.0)
            }
            _ => return Err(anyhow!("Did not multiply numbers"))
        }

    }


    Ok(())
}


