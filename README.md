# Turing.rs
A wasm interpreter written in rust that interops with beat saber.  

this library behaves similar to OpenGL in how functions are called

## C# and rust interop specs

Initialize the Turing library via `init_turing()`  

Finalize the wasm interpreter via `init_wasm()`  

if things break, and you need to reset the state, call `uninit_turing()`, this will reset the state to as if the program just started.  

> [!WARNING]
> Reentry loops will not work.
> this means chains of host → wasm → host → wasm are invalid and will error.


### Parameters
Create a new parameters builder with `create_params() -> u32` or `create_n_params(count: u32) -> u32`  

Select a params object for future calls via `bind_params(params: u32)`  

Add params via `add_param(param: FfiParam)`  

Set a specific param via `set_param(index: u32, param: FfiParam)`

Read params via `read_param(index: u32) -> FfiParam`  

Delete the params object via `delete_params(params: u32)`  

### wasm functions

create a new function with `create_wasm_fn(capability: *const c_char, name: *const c_char, pointer: *const c_void) -> FfiParam`  

add param types via `add_wasm_fn_param_type(param_type: u32) -> FfiParam`  

set return type via `set_wasm_fn_return_type(return_type: u32) -> FfiParam`  

The previous 2 functions always operate on the last created function.

call a wasm function via `call_wasm_fn(name: *const c_char, params: u32, expected_return_type: u32) -> FfiParam`  

load a script with `load_script(source: *const c_char, loaded_capabilites: u32) -> FfiParam`, with loaded_capabilities being a Params object id

### Helpers

`free_string(ptr: *const c_char)`  

register C# functions that rust uses directly with `register_function(name: *const c_char, pointer: *const c_void)`  
rust expects these functions to be registered after `init_turing` and before anything else
- `abort(error_code: *const c_char, error_message: *const c_char) -> !`
- `log_info(msg: *const c_char)`
- `log_warn(msg: *const c_char)`
- `log_critical(msg: *const c_char)`
- `log_debug(msg: *const c_char)`
Failing to register these 5 functions is technically fine since they all have an empty fallback implementation, though it is not recommended  


### Interop Structs
```rs
pub union RawParam {
    I8: i8,                // id: 1
    I16: i16,              //     2
    I32: i32,              //     3
    I64: i64,              //     4
    U8: u8,                //     5
    U16: u16,              //     6
    U32: u32,              //     7
    U64: u64,              //     8
    F32: f32,              //     9
    F64: f64,              //    10
    Bool: bool,            //    11
    String: *const c_char, //    12
    Object: *const c_void, //    13
    Error: *const c_char,  //    14
    Void: (),              //    15 or 0
}

pub struct FfiParam {
    type_id: u32,
    value: RawParam,
}

```


