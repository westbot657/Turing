# Turing.rs
A wasm interpreter written in rust that interops with beat saber.  

this library behaves similar to OpenGL in how functions are called

## C# and rust interop specs

Initialize the Turing library via `init_turing()`  

Finialize the wasm interpreter via `init_wasm()`  

> [!WARNING]
> Reentry loops will not work.
> this means chains of host->wasm->host->wasm are invalid and will error.


### Parameters
Create a new parameters builder with `create_params() -> u32`  

Select a params object for future calls via `bind_params(params: u32)`  

Add params via `add_param(param: FfiParam)`  

Read params via `read_param(index: u32) -> FfiParam`  

Delete the params object via `delete_params(params: u32)`  

### wasm functions

create a new function with `create_wasm_fn(name: *const c_char, pointer: *const c_void) -> FfiParam`  

add param types via `add_wasm_fn_param_type(param_type: u32) -> FfiParam`  

set return type via `set_wasm_fn_return_type(return_type: u32) -> FfiParam`  

call a wasm function via `call_wasm_fn(name: *const c_char, params: u32) -> FfiParam`  


### Helpers

`free_string(ptr: *const c_char)`  

register rust <-> C# functions with `register_function(name: *const c_char, pointer: *const c_void)`  
rust expects these functions to be registered after `init_turing` and before anything else
- `abort(error_code: *const c_char, error_message: *const c_char) -> !`
- `log_info(msg: *const c_char)`
- `log_warn(msg: *const c_char)`
- `log_error(msg: *const c_char)`
- `log_debug(msg: *const c_char)`
Failing to register these is technically fine since they all have an empty fallback implementation  


### Interop Structs
```rs
pub union RawParam {
    I8: i8,                // id: 1
    I16: i16,              //     2
    I32: i32,              //     3
    U8: u8,                //     4
    U16: u16,              //     5
    U32: u32,              //     6
    F32: f32,              //     7
    Bool: bool,            //     8
    String: *const c_char, //     9
    Object: *const c_void, //    10
    Error: *const c_char,  //    11
    Void: u32,             //    12 // value is always 0
}

pub struct FfiParam {
    type_id: u32,
    value: RawParam,
}

```


