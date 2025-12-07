use std::ffi::CString;
use std::fs;
use anyhow::{anyhow, Result};
use crate::interop::params::{param_type, Param};
use crate::*;


extern "C" fn log_info_stand_in(msg: *const c_char) {
    unsafe {
        println!("wasm output: {}", CStr::from_ptr(msg).to_string_lossy().to_string());
    }
}


//#[test]
pub fn setup_wasm() -> Result<()> {
    unsafe {
        init_turing();
        let cstr = CString::new("log_info")?.into_raw();
        let cap = CString::new("test")?;

        let pointer = log_info_stand_in as *const c_void;
        let cap_ptr = cap.as_ptr();

        let res = create_wasm_fn(cap_ptr, cstr, pointer).to_param()?;
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

        let capabilities = create_n_params(1);
        add_param(Param::String("turing".to_string()));

        load_script(c_ptr, capabilities).to_param()?.to_result()
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

        println!("Testing math ops?");

        let fname = CString::new("log_info").unwrap();
        register_function(fname.as_ptr(), log_info_stand_in as *const c_void);

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

#[test]
pub fn test_stdin_fail() -> Result<()> {
    setup_wasm()?;

    unsafe {

        setup_test_script()?;

        let name = CString::new("test_stdin_fail")?;

        let res = call_wasm_fn(name.as_ptr(), 0, param_type::VOID).to_param()?.to_result();

        println!("stdin test is: {:?}", res);
        assert!(res.is_err())
    }

    Ok(())
}

