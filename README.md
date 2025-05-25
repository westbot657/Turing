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

## Functions
| Categorization   | Defined by Beat Saber Mod                                                                                                                                                                                                                                             | Defined by Turing.rs                                                                              |
|------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------|
| Util / WASM      | `cs_print(*mut c_char)`                                                                                                                                                                                                                                               | Logs a message to the console. <br/><br/> **Parameters**: <br/> - A pointer to the message string |
|                  | Starts the WASM interpreter                                                                                                                                                                                                                                           | `initialize_wasm()`                                                                               |
|                  | Loads a WASM script. <br/><br/> **Parameters**: <br/> - A pointer to the script's path string                                                                                                                                                                         | `load_script(*mut c_char)`                                                                        |
|                  | Deallocates a CParams object and all parameters that it held. <br/><br/> **Parameters**: <br/> - The CParams object to deallocate                                                                                                                                     | `free_params(CParams)`                                                                            |
|                  | Calls a function from the currently loaded WASM script. <br/><br/> **Parameters**: <br/> - A pointer to the function name string <br/> - A CParams object for wasm function parameters <br/> **Returns**: <br/> - A CParams object containing return values or errors | `call_script_function(*mut c_char, CParams) -> CParams`                                           |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Color Notes      | `create_color_note(f32)`                                                                                                                                                                                                                                              | Creates a ColorNote <br/><br/> **Parameters**: <br/> - The beat to create the note for            |
|                  | `beatmap_add_color_note(ColorNote)`                                                                                                                                                                                                                                   | Adds a ColorNote to the beatmap                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Bomb Notes       |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Walls            |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Arcs             |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Chain Notes      |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Chain Head Notes |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |
| Chain Link Notes |                                                                                                                                                                                                                                                                       |                                                                                                   |
|                  |                                                                                                                                                                                                                                                                       |                                                                                                   |



# WASM Specifications
| Categorization   | Defined by Turing.rs ("env")     | Defined by WASM                                                                                                                                          |
|------------------|----------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|
| System           | `_log(i32)`                      | Logs a message to the console <br/><br/> **Parameters**: <br/> - message string address pointer                                                          |
|                  | `_drop_reference(i32)`           | Drops a host-managed object such as ColorNotes, Vec3s, etc <br/><br/> **Parameters**: <br/> - host object reference "pointer"                            |
|                  |                                  |                                                                                                                                                          |
| Color Notes      | `_create_color_note(f32) -> i32` | Creates a ColorNote <br/><br/> **Parameters**: <br/> - The beat to create the note for <br/><br/> **Returns**: <br/> - a host object reference "pointer" |
|                  | `_beatmap_add_color_note(i32)`   | Adds a ColorNote to the beatmap <br/><br/> **Parameters**: <br/> - host object reference "pointer" for a ColorNote object                                |
|                  |                                  |                                                                                                                                                          |
| Bomb Notes       |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
| Walls            |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
| Arcs             |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
| Chain Notes      |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
| Chain Head Notes |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
| Chain Link Notes |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |
|                  |                                  |                                                                                                                                                          |





