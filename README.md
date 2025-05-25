# Turing.rs
A wasm interpreter written in rust that interops with beat saber.  







# Interop Specifications

## Structs

```rust
#[repr(C)]
struct CParam {
    data_type: u32,
    pub data: *const c_void, // points at the associated data that matches data_type
}
```

```rust
#[repr(C)]
struct CParams {
    param_count: u32,
    param_ptr_array_ptr: *mut *mut CParam,
}
```
C Parameter types:

| id  | type (pointer) |
|-----|----------------|
| 0   | i8             |
| 1   | i16            |
| 2   | i32            |
| 3   | i64            |
| 4   | u8             |
| 5   | u16            |
| 6   | u32            |
| 7   | u64            |
| 8   | f32            |
| 9   | f64            |
| 10  | bool           |
| 11  | string         |
| 100 | Object         |
| 200 | FuncRef        |
| 900 | InteropError   |

```rust
#[repr(C)]
struct InteroperableError {
    type_ptr: *mut c_char, // pointer to a C-String describing the kind of error
    error_message: *mut c_char, // pointer to a C-String describing the error itself
}
```

```rust
#[repr(C)]
struct RsObject {
    ptr: *const c_void // pointer to a C# managed object.
}
```

```rust
struct ParamTypes {
    count: u32,
    array: *const u32
}
```
Parameter/Return types:  

| id       | type    |
|----------|---------|
| 2        | i32     |
| 3        | i64     |
| 8        | f32     |
| 9        | f64     |
| 100..199 | Object  |
| 200      | FuncRef |


## Functions

### defined by rust, Called by C#

```rust
unsafe extern "C" fn generate_wasm_fn(name: *const c_char, func_ptr: *mut c_void, param_types: ParamTypes, return_types: ParamTypes) { ... }
```
C# should call this to bind every wasm function.  
C# must call `initialize_wasm()` to actually bind functions to wasm (so no late-adding functions)  

```rust
pub unsafe extern "C" fn initialize_wasm() { ... }
```
links all defined functions and initializes the wasm interpreter  

```rust
pub unsafe extern "C" fn free_params(params: CParams) { ... }
```
Completely frees the CParams structure (excluding pointers to C# managed objects)  

```rust
pub unsafe extern "C" fn load_script(str_ptr: *const c_char) { ... }
```
loads a script file (*.wasm), given the absolute path to it.  

```rust
pub unsafe extern "C" fn call_script_function(str_ptr: *const c_char, params: CParams) { ... }
```
calls a function defined in wasm. currently doesn't handle a return value.  

### defined by C#, Called by rust

```rust
extern "C" { fn abort(error_code: *const c_char, error_message: *const c_char); }
```
this is how rust and wasm will throw an error for C# to catch.  
C# is expected to either re-throw or properly disengage and give a user-friendly error.  
