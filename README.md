# Turing.rs
A wasm interpreter written in rust that interops with beat saber.  

this library behaves similar to OpenGL in how functions are called

# C# and rust interop specs

Here are the true types of type aliases, for ffi however,
these concrete types are only to tell you which functions
should be given what values.

type `WasmFnMap` = `HashMap<String, WasmFnMetadata>`  
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

### `create_wasm_fn_map() -> *mut WasmFnMap`

### `create_wasm_fn_metadata(capability: *const c_char, callback: WasmCallback) -> *mut WasmFnMetadata`

### `add_param_types_to_wasm_fn_data(data: *mut WasmFnMetadata, params: *mut DataType, params_count: u32) -> *const c_char`

### `set_wasm_fn_return_type(data: *mut WasmFnMetadata, return_type: DataType) -> *const c_char`

### `add_wasm_fn_to_map(map: *mut WasmFnMap, name: *const c_char, data: *mut WasmFnMetadata)`

### `copy_wasm_fn_map(map: *mut WasmFnMap) -> *mut WasmFnMap`

### `delete_wasm_fn_map(map: *mut WasmFnMap)`

## 

---
## Turing Instance creation

### `create_instance(wasm_fns_ptr: *mut WasmFnMap) -> *mut TuringInit`

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
# Wasm runtime

### `load_wasm_script(turing: *mut TuringInstance, source: *const c_char, loaded_capabilities: *mut *const c_char, capability_count: u32) -> FfiParam`

### `call_wasm_fn(turing: *mut TuringInstance, name: *const c_char, params: *mut Params, expected_return_type: DataType) -> FfiParam`

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


