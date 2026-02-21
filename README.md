# Turing.rs
A scripting engine written in rust that interops with beat saber.  


# C# and rust interop specs

Here are the true types of type aliases, for ffi however,
these concrete types are only to tell you which functions
should be given what values.

type `ScriptFnMap` = `HashMap<String, ScriptFnMetadata>`  
type `TuringInstance` = `Turing<CsFns>`  
type `TuringInit` = `Result<Turing<CsFns>>`  

---
## Helper functions

### `free_string(ptr: *mut c_char)`
frees a rust-allocated string.

### `register_function(name: *const c_char, callback: *const c_void)`
registers functions that rust needs to work with interop.
valid functions are:
- `abort(err_type: *const c_char, err_msg: *const c_char)`
- `log_info(*const c_char)`
- `log_warn(*const c_char)`
- `log_critical(*const c_char)`
- `log_debug(*const c_char)`
- `free_cs_string(*const c_char)`

---
# Wasm initialization phase functions

### `create_fn_map() -> *mut ScriptFnMap`

### `create_fn_metadata(capability: *const c_char, callback: WasmCallback) -> *mut ScriptFnMetadata`

### `add_param_types_to_fn_data(data: *mut ScriptFnMetadata, params: *mut DataType, params_count: u32) -> *const c_char`

### `set_fn_return_type(data: *mut ScriptFnMetadata, return_type: DataType) -> *const c_char`

### `add_fn_to_map(map: *mut ScriptFnMap, name: *const c_char, data: *mut ScriptFnMetadata)`
name should be in one of these formats:
- `ClassName:methodName`
- `ClassName.functionName`
- `function_name`
case style doesn't matter, only the `:` and `.`


### `copy_fn_map(map: *mut ScriptFnMap) -> *mut ScriptFnMap`

### `delete_fn_map(map: *mut ScriptFnMap)`

## 

---
## Turing Instance creation

### `create_instance(fns_ptr: *mut ScriptFnMap) -> *mut TuringInit`

### `check_error(res_ptr: *mut TuringInit) -> *const c_char`

### `unwrap_instance(res_ptr: *mut TuringInit) -> *mut TuringInstance`

### `delete_instance(turing: *mut TuringInstance)`

---
# Params modification

### `create_params(size: u32) -> *mut Params`

### `add_param(params: *mut Params, param: FfiParam)`

### `delete_params(params: *mut Params)`

### `get_param(params: *mut Params, index: u32) -> FfiParam`

### `delete_param(param: FfiParam)`

---
# Script runtime

### `load_script(turing: *mut TuringInstance, source: *const c_char, loaded_capabilities: *mut *const c_char, capability_count: u32) -> FfiParam`
This will either load the wasm or lua engine based on the source's file extension.

### `call_fn(turing: *mut TuringInstance, name: *const c_char, params: *mut Params, expected_return_type: DataType) -> FfiParam`
Will automatically call the appropriate functions based on the current code engine.

### `fast_call_update(turing: *mut TuringInstance, delta_time: f32) -> *const c_char`
Bypasses the params system entirely to call `on_update` if it's loaded.  
This function may return an error string, so check if it's non-null

### `fast_call_fixed_update(turing: *mut TuringInstance, delta_time: f32) -> *const c_char`
Same as `fast_call_update` but calls `on_fixed_update` instead

---
# Script validation

### `get_api_versions(turing: *mut TuringInstance) -> *mut VersionTable`

### `versions_contains_mod(versions: *mut VersionTable, name: *const c_char) -> bool`

### `get_mod_version(versions: *mut VersionTable, name: *const c_char) -> u64`
returns a semantic version in the form of (major: u32, minor: u16, patch: u16) in that specific packing order

### `free_versions_table(versions: *mut VersionTable)`


---
## Interop Structs
```rs
pub enum DataType {
    I8 = 1,
    I16 = 2,
    I32 = 3,
    I64 = 4,
    U8 = 5,
    U16 = 6,
    U32 = 7,
    U64 = 8,
    F32 = 9,
    F64 = 10,
    Bool = 11,
    RustString = 12,
    ExtString = 13,
    Object = 14,
    RustError = 15,
    ExtError = 16,
    Void = 17,
}

pub union RawParam {
    I8: i8,
    I16: i16,
    I32: i32,
    I64: i64,
    U8: u8,
    U16: u16,
    U32: u32,
    U64: u64,
    F32: f32,
    F64: f64,
    Bool: bool,
    RustString: *const c_char,
    ExtString: *const c_char,
    Object: *const c_void,
    RustError: *const c_char,
    ExtError: *const c_char,
    Void: (),
}

pub struct FfiParam {
    type_id: DataType,
    value: RawParam,
}

```


### Compiling for Windows from Linux
Download the `mingw-64` package and compile using:
```
cargo x w
```

---

**Linear Algebra Types (WASM usage)**

- The engine supports passing common linear-algebra types via a small queue ABI: `Vec2`, `Vec3`, `Vec4`, `Quat`, and `Mat4`.
- Hosts pass these types by pushing their float components into a shared f32 queue and calling the wasm function with one integer size argument per queued math parameter. Each size value indicates how many floats that parameter contains (e.g. `Vec2 = 2`, `Vec4 = 4`, `Mat4 = 16`).
- WASM functions should accept one `u32` (or `u32`) per queued math param and return a `u32` size for the result. Use the provided `alg` helpers to dequeue/enqueue values. For example, a function taking two `Vec2` values will have signature `(u32, u32) -> u32` and should call `alg::dequeue_vec2()` twice in the same order the host enqueued them.

Example wasm-side (Rust):

```rust
// Single Vec4 param: one u32 size argument
#[unsafe(no_mangle)]
extern "C" fn vec4_test(_size: u32) -> u32 {
    let v = alg::dequeue_vec4(); // reads 4 f32 from the host queue
    // ... do work on v ...
    alg::enqueue_vec4(v) // enqueue 4 f32 to return to host and return size
}

// Two Vec2 params: two u32 size arguments in order
#[unsafe(no_mangle)]
extern "C" fn two_vec2s(_a_size: u32, _b_size: u32) -> u32 {
    let a = alg::dequeue_vec2();
    let b = alg::dequeue_vec2();
    // ... do work on a and b ...
    alg::enqueue_vec2(a)
}
```

Host-side notes:

- When calling from the host API, push `Param::Vec2`, `Param::Vec4`, or `Param::Mat4` into a `Params` value. The engine will enqueue the float components for each math param and pass a single integer size argument to wasm for each math param (in the same order they appear in `Params`).
- The wasm function must return a `u32` size and enqueue the result floats; the engine converts the queued floats back into `Param` values for the caller.

Host-side example (Rust):

```rust
let mut params = Params::new();
params.push(Param::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)));
let res = turing.call_fn_by_name("vec4_test", params, DataType::Vec4);
let v = res.to_result::<Vec4>()?;
```

If your function takes multiple math params, the wasm signature must include one `u32` size argument per param.
```