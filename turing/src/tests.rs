use crate::ffi::{add_param, add_wasm_fn_param_type, bind_params, call_wasm_fn, create_n_params, create_wasm_fn, free_string, init_turing, init_wasm, load_script, read_param, register_function, set_param, set_wasm_fn_return_type, uninit_turing};
use crate::interop::params::{Param, FfiParam};
use crate::*;
use anyhow::{Result, anyhow};
use std::ffi::CString;
use serial_test::serial;

extern "C" fn log_info_stand_in(msg: *const c_char) {
    unsafe {
        println!(
            "wasm output: {}",
            CStr::from_ptr(msg).to_string_lossy().to_string()
        );
    }
}

extern "C" fn log_info_wasm(params: ParamKey) -> FfiParam {
    bind_params(params);
    let s = read_param(0).to_param();
    match s {
        Ok(Param::String(s)) => {
            println!("[wasm/info]: {}", s)
        }
        Ok(Param::Error(e)) => {
            eprintln!("[error/wasm]: {}", e)
        }
        Err(e) => {
            eprintln!("[error/cs]: {}", e)
        },
        _ => {
            println!("Unexpected param in log_info_wasm")
        }
    }

    Param::Void.into()
}

extern "C" fn fetch_string(_params: ParamKey) -> FfiParam {
    Param::String("this is a host provided string!".to_string()).into()
}

pub fn setup_wasm() -> Result<()> {
    unsafe {
        uninit_turing();
        init_turing();

        let cstr = CString::new("free_cs_string")?;
        register_function(cstr.as_ptr(), free_string as *const c_void);

        let cstr = CString::new("log_info")?;
        register_function(cstr.as_ptr(), log_info_stand_in as *const c_void);

        let cap = CString::new("test")?;

        let cap_ptr = cap.as_ptr();
        let res = create_wasm_fn(cap_ptr, cstr.as_ptr(), log_info_wasm as *const c_void).to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Creation of wasm function failed: {}", e));
        }
        let res = add_wasm_fn_param_type(ParamType::STRING).to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Addition of function parameter type failed: {}", e));
        }

        let cstr = CString::new("fetch_string")?;
        let pointer = fetch_string as *const c_void;
        let res = create_wasm_fn(cap_ptr, cstr.as_ptr(), pointer).to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Creation of wasm function failed: {}", e));
        }
        let res = set_wasm_fn_return_type(ParamType::STRING).to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Setting return type failed: {}", e))
        }


        let res = init_wasm().to_param()?;
        if let Param::Error(e) = res {
            return Err(anyhow!("Failed to initialize wasm engine: {}", e));
        }

    }
    Ok(())
}

pub fn setup_test_script() -> Result<()> {
    unsafe {
        let fp = r#"../tests/wasm/wasm_tests.wasm"#;

        let c_ptr = CString::new(fp)?;

        let capabilities = create_n_params(2);
        add_param(Param::String("turing".to_string()).to_ffi_param());
        add_param(Param::String("test".to_string()).to_ffi_param());

        load_script(c_ptr.as_ptr(), capabilities).to_param()?.to_result()
    }
}

#[test]
#[serial]
pub fn test_file_access() -> Result<()> {
    setup_wasm()?;
    println!("======================");

    unsafe {
        setup_test_script()?;

        let name = CString::new("file_access_test")?;

        let res = call_wasm_fn(name.as_ptr(), 0, ParamType::VOID)
            .to_param()?
            .to_result();

        println!("[test/cs]: file access result is err: {}", res.is_err());
        assert!(res.is_err())
    }

    Ok(())
}

#[test]
#[serial]
pub fn test_math() -> Result<()> {
    setup_wasm()?;

    println!("======================");
    unsafe {
        setup_test_script()?;

        println!("[test/cs]: Testing math ops?");

        let fname = CString::new("log_info")?;
        register_function(fname.as_ptr(), log_info_stand_in as *const c_void);

        let name = CString::new("math_ops_test")?;

        let p = create_n_params(2);
        bind_params(p);
        set_param(0, Param::F32(3.5).into())
            .to_param()?
            .to_result()?;
        set_param(1, Param::F32(5.0).into())
            .to_param()?
            .to_result()?;

        let res = call_wasm_fn(name.as_ptr(), p, ParamType::F32).to_param()?;

        match res {
            Param::F32(f) => {
                println!("[test/cs]: wasm code added 3.5 to 5.0 for {}", f);
                assert_eq!(f, 3.5 * 5.0)
            }
            _ => return Err(anyhow!("Did not multiply numbers")),
        }
    }

    Ok(())
}

#[test]
#[serial]
pub fn test_stdin_fail() -> Result<()> {
    setup_wasm()?;
    println!("======================");

    unsafe {
        setup_test_script()?;

        let name = CString::new("test_stdin_fail")?;

        let res = call_wasm_fn(name.as_ptr(), 0, ParamType::VOID)
            .to_param()?
            .to_result();

        println!("[test/cs]: stdin test is: {:?}", res);
        assert!(res.is_ok())
    }

    Ok(())
}

#[test]
#[serial]
pub fn test_string_fetch() -> Result<()> {
    setup_wasm()?;
    println!("======================");

    unsafe {
        setup_test_script()?;

        let name = CString::new("test_string_fetch")?;

        let res = call_wasm_fn(name.as_ptr(), 0, ParamType::VOID)
            .to_param()?
            .to_result();

        println!("[test/cs]: string fetch result is {:?}", res);
        assert!(res.is_ok());

    }

    Ok(())
}
