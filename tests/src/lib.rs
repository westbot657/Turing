use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::{fs, io};

pub type ObjectHandle = u64;

unsafe extern "C" {
    fn _test_log__info(msg: *const c_char);
    fn _test_fetch_string() -> u32;
    fn _test_create_object_a() -> ObjectHandle;
    fn _test_object_a__foo(handle: ObjectHandle) -> i32;

    /// For internal use only.
    /// Copies a string from the host's memory to the pointer specified
    pub fn _host_strcpy(location: *mut c_char, size: u32);
    /// For internal use only.
    /// Copies a Vec<u32> from the host's memory to the pointer specified
    pub fn _host_bufcpy(location: *mut c_void, size: u32);
    /// For internal use only.
    /// Pushes an f32 to a queue for passing math objects
    pub fn _host_f32_enqueue(f: f32);
    /// For internal use only.
    /// pops an f32 from the queue for passing math objects
    pub fn _host_f32_dequeue() -> f32;
    /// For internal use only.
    /// Pushes a u32 to a queue for passing buffer lengths
    pub fn _host_u32_enqueue(u: u32);
    /// For internal use only.
    /// pops a u32 from the queue for passing buffer lengths
    pub fn _host_u32_dequeue() -> u32;

}

/// Core Systems ////
pub mod alg {
    use super::*;

    pub use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn dequeue_vec2() -> Vec2 {
        let x = unsafe { _host_f32_dequeue() };
        let y = unsafe { _host_f32_dequeue() };
        Vec2::new(x, y)
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn enqueue_vec2(v: Vec2) -> u32 {
        unsafe {
            _host_f32_enqueue(v.x);
            _host_f32_enqueue(v.y);
        }
        2
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn dequeue_vec3() -> Vec3 {
        let x = unsafe { _host_f32_dequeue() };
        let y = unsafe { _host_f32_dequeue() };
        let z = unsafe { _host_f32_dequeue() };
        Vec3::new(x, y, z)
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn enqueue_vec3(v: Vec3) -> u32 {
        unsafe {
            _host_f32_enqueue(v.x);
            _host_f32_enqueue(v.y);
            _host_f32_enqueue(v.z);
        }
        3
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn dequeue_vec4() -> Vec4 {
        let x = unsafe { _host_f32_dequeue() };
        let y = unsafe { _host_f32_dequeue() };
        let z = unsafe { _host_f32_dequeue() };
        let w = unsafe { _host_f32_dequeue() };
        Vec4::new(x, y, z, w)
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn enqueue_vec4(v: Vec4) -> u32 {
        unsafe {
            _host_f32_enqueue(v.x);
            _host_f32_enqueue(v.y);
            _host_f32_enqueue(v.z);
            _host_f32_enqueue(v.w);
        }
        4
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn dequeue_quat() -> Quat {
        let x = unsafe { _host_f32_dequeue() };
        let y = unsafe { _host_f32_dequeue() };
        let z = unsafe { _host_f32_dequeue() };
        let w = unsafe { _host_f32_dequeue() };
        Quat::from_xyzw(x, y, z, w)
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn enqueue_quat(v: Quat) -> u32 {
        unsafe {
            _host_f32_enqueue(v.x);
            _host_f32_enqueue(v.y);
            _host_f32_enqueue(v.z);
            _host_f32_enqueue(v.w);
        }
        4
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn dequeue_mat4() -> Mat4 {
        let mut arr = [0f32; 16];
        for i in 0..16 {
            arr[i] = unsafe { _host_f32_dequeue() };
        }

        Mat4::from_cols_array(&arr)
    }

    /// Not for public use. This method is marked public so that API extensions may access it.
    /// A user script should never be calling this directly
    pub fn enqueue_mat4(m: Mat4) -> u32 {
        let slice = m.to_cols_array();

        for v in slice {
            unsafe { _host_f32_enqueue(v) };
        }

        16
    }
}

macro_rules! println {
    ( $( $tok:expr ),* ) => {
        {
            let s = CString::new(format!($($tok),*)).unwrap();
            unsafe { _test_log__info(s.as_ptr()) }
        }
    };
}

#[unsafe(no_mangle)]
extern "C" fn on_load() {
    unsafe {
        let s =
            CString::new("log info from wasm!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap();
        let ptr = s.into_raw();

        _test_log__info(ptr);
    }
}

#[unsafe(no_mangle)]
extern "C" fn file_access_test() {
    let current_path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = current_path.parent().unwrap().join("README.md");
    let bytes = fs::read(readme).expect("Failed to read README.md");

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
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");
    println!("You typed: {}", input.trim());
}

#[unsafe(no_mangle)]
extern "C" fn test_string_fetch() {
    let sz = unsafe { _test_fetch_string() };
    let mut turing_str = vec![0; sz as usize];
    unsafe { _host_strcpy(turing_str.as_mut_ptr(), sz) };
    let turing_str = unsafe { CStr::from_ptr(turing_str.as_ptr() as *const c_char) };
    let string = turing_str.to_string_lossy().into_owned();

    println!("Received string from host: '{}'", string)
}

#[unsafe(no_mangle)]
extern "C" fn test_panic() {
    panic!("This is a panic from within wasm!");
}

#[unsafe(no_mangle)]
extern "C" fn object_test(val: ObjectHandle) -> ObjectHandle {
    // Echo back the raw i64 value so host-side opaque pointer handling can be tested.
    val
}

#[unsafe(no_mangle)]
extern "C" fn object_test2() -> i32 {
    let obj = unsafe { _test_create_object_a() };
    println!("Created ObjectA with handle {}", obj);
    let result = unsafe { _test_object_a__foo(obj) };
    println!("Called ObjectA.foo(), got result {}", result);
    result
}

// The host passes vector/matrix values by enqueuing their float components into
// a shared f32 queue and then calling the wasm function with a single `i32`
// argument representing the size (number of floats) that follow. The `_size`
// parameter is unused inside the function body but must be present so the
// function signature matches what the host/engine expects when passing queued
// math objects (e.g. Vec2 = 2, Vec4 = 4, Mat4 = 16). The return is the same
// pattern: the function enqueues the result and returns the size as `u32`.
#[unsafe(no_mangle)]
extern "C" fn vec2_test(_size: i32) -> u32 {
    let v = alg::dequeue_vec2();
    println!("WASM: vec2_test received ({}, {})", v.x, v.y);
    alg::enqueue_vec2(v)
}

// See note above on `_size` parameter: it's required for ABI compatibility
// with the engine's math-queue passing convention. The function reads the
// queued Vec4 and enqueues the result to return it to the host.
#[unsafe(no_mangle)]
extern "C" fn vec4_test(_size: i32) -> u32 {
    let v = alg::dequeue_vec4();
    println!(
        "WASM: vec4_test received ({}, {}, {}, {})",
        v.x, v.y, v.z, v.w
    );
    alg::enqueue_vec4(v)
}

// Mat4 follows the same convention: 16 floats are enqueued by the host and
// the function must accept an `i32` size parameter to match the host call
// signature. The function dequeues the Mat4, performs any work, then enqueues
// the resulting Mat4 and returns the size.
#[unsafe(no_mangle)]
extern "C" fn mat4_test(_size: i32) -> u32 {
    let m = alg::dequeue_mat4();
    println!("WASM: mat4_test received");
    alg::enqueue_mat4(m)
}
