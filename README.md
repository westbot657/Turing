# Turing.rs
A wasm interpreter written in rust that interops with beat saber.  

this library behaves similar to OpenGL in how functions are called

## C# and rust interop specs

Initialize the Turing library via `init_turing()`  

Registering wasm functions: TODO  

Finialize the wasm interpreter via `init_wasm()`  


### Parameters
Create a new parameters builder with `create_params() -> u32`  

Select a params object for future calls via `bind_params(params: u32)`  

Add params via `add_param(param: FfiParam)`  

Read params via `read_param(index: u32) -> FfiParam`  

Delete the params object via `delete_params(params: u32)`


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
}

pub struct FfiParam {
    type_id: u32,
    value: RawParam,
}

```


