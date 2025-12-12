use std::ffi::{CString, c_char, CStr};
use std::{fs, io};


unsafe extern "C" {
    fn log_info(msg: *const c_char);
    fn fetch_string() -> u32;
    fn _host_strcpy(location: u32, size: u32) -> u32;
}

macro_rules! println {
    ( $( $tok:expr ),* ) => {
        {
            let s = CString::new(format!($($tok),*)).unwrap();
            unsafe { log_info(s.as_ptr()) }
        }
    };
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
    println!("WASM: Multiplying {} and {}", a, b);
    a * b
}

#[unsafe(no_mangle)]
extern "C" fn test_stdin_fail() {
    println!("trying to read input");
    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read line");
    println!("You typed: {}", input.trim());
}

#[unsafe(no_mangle)]
extern "C" fn test_string_fetch() {
    let sz = unsafe { fetch_string() };
    let mut turing_str = vec![0; sz as usize];
    unsafe { _host_strcpy(turing_str.as_mut_ptr() as u32, sz) };
    let turing_str = unsafe { CStr::from_ptr(turing_str.as_ptr() as *const c_char) };
    let string = turing_str.to_string_lossy().into_owned();

    println!("Received string from host: '{}'", string)
}
