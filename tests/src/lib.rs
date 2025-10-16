use std::ffi::{CString, c_char};
use std::fs;


unsafe extern "C" {
    fn log_info(msg: *const c_char);
}

#[unsafe(no_mangle)]
extern "C" fn on_load() {
    unsafe {
        let s = CString::new("log info from wasm!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap();
        let ptr = s.into_raw();

        log_info(ptr);
    }
}

#[unsafe(no_mangle)]
extern "C" fn file_access_test() {

    let bytes = fs::read("/home/westbot/turing/Turing/README.md").unwrap();

    let content = String::from_utf8(bytes);

    println!("Wasm read file contents as:\n{:#?}", content);

}

#[unsafe(no_mangle)]
extern "C" fn math_ops_test(a: f32, b: f32) -> f32 {
    a * b
}


