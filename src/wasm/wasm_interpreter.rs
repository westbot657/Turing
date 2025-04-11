use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use anyhow::{anyhow, Result};
use wasmi::*;
use wat;
use crate::*;
use crate::data::game_objects::*;
use std::ops::RangeInclusive;
use paste::paste;

#[derive(Debug)]
struct HostState<T> {
    free_ranges: VecDeque<RangeInclusive<u32>>,
    next_index: u32,
    external_data: HashMap<u32, T>,
}

impl<T> HostState<T> {
    pub fn new() -> Self {
        Self {
            free_ranges: VecDeque::new(),
            next_index: 0,
            external_data: HashMap::new(),
        }
    }

    pub fn add(&mut self, extern_ref: T) -> u32 {
        let i = if let Some(range) = self.free_ranges.front_mut() {
            let val = *range.start();
            *range = (*range.start() + 1)..=*range.end();
            if range.start() > range.end() {
                self.free_ranges.pop_front();
            }
            val
        } else {
            let mut i = self.next_index;
            while self.external_data.contains_key(&i) {
                self.next_index += 1;
                if self.next_index == u32::MAX {
                    panic!("Out Of Memory");
                }
                i = self.next_index;
            }
            self.next_index += 1;
            i
        };

        self.external_data.insert(i, extern_ref);
        i
    }

    pub fn get(&self, i: u32) -> Option<&T> {
        self.external_data.get(&i)
    }

    pub fn get_mut(&mut self, i: u32) -> Option<&mut T> {
        self.external_data.get_mut(&i)
    }

    pub fn remove(&mut self, i: u32) -> Option<T> {
        self.insert_free_range(i);
        self.external_data.remove(&i)
    }

    fn insert_free_range(&mut self, i: u32) {
        let mut inserted = false;

        for idx in 0..self.free_ranges.len() {
            let range = &mut self.free_ranges[idx];

            if i + 1 == *range.start() {
                *range = i..=*range.end();
                inserted = true;
                break;
            } else if i == *range.end() + 1 {
                *range = *range.start()..=i;
                inserted = true;
                break;
            } else if i < *range.start() {
                self.free_ranges.insert(idx, i..=i);
                inserted = true;
                break;
            }
        }

        if !inserted {
            self.free_ranges.push_back(i..=i);
        }

        // Merge adjacent ranges
        let mut merged = VecDeque::new();
        while let Some(mut current) = self.free_ranges.pop_front() {
            while let Some(next) = self.free_ranges.front() {
                if *current.end() + 1 >= *next.start() {
                    let next = self.free_ranges.pop_front().unwrap();
                    current = *current.start()..=*next.end();
                } else {
                    break;
                }
            }
            merged.push_back(current);
        }
        self.free_ranges = merged;
    }
}


pub struct WasmInterpreter {
    engine: Engine,
    store: Store<HostState<ExternRef>>,
    linker: Linker<HostState<ExternRef>>,
    script_instance: Option<(Module, Instance)>,
}

impl WasmInterpreter {
    pub fn new() -> WasmInterpreter {
        let mut config = Config::default();
        config.enforced_limits(EnforcedLimits::strict());
        let engine = Engine::new(&config);
        let mut store = Store::new(&engine, HostState::new());
        let mut linker = <Linker<HostState<ExternRef>>>::new(&engine);

        unsafe {
            bind_data(&engine, &mut store, &mut linker).expect("Failed to setup wasm environment");
        }
        WasmInterpreter {
            engine,
            store,
            linker,
            script_instance: None,
        }
    }

    pub fn load_script(&mut self, path: &str) -> Result<()> {

        let path = Path::new(path);
        let wasm = fs::read(path)?;

        let module = Module::new(&self.engine, wasm)?;

        let instance = self.linker
            .instantiate(&mut self.store, &module)?
            .start(&mut self.store)?;

        self.script_instance = Some((module, instance));

        Ok(())
    }

    pub fn call_void_method(&mut self, name: &str, params: Parameters) -> Result<()> {
        if let Some((_, instance)) = &self.script_instance {
            let init_function = instance.get_typed_func::<(), ()>(&self.store, name)?;
            init_function.call(&mut self.store, ())?;
            Ok(())
        } else {
            Err(anyhow!("no script is currently loaded"))
        }
    }

}

unsafe fn bind_data(engine: &Engine, store: &mut Store<HostState<ExternRef>>, linker: &mut Linker<HostState<ExternRef>>) -> Result<()> {

    // wasm names are prefixed with '_' so that languages
    // can have abstraction layers to turn stuff into normal
    // structures for the language, and use non-prefixed names

    // Static objects
    linker.func_wrap("env", "_log", |caller: Caller<'_, HostState<ExternRef>>, message: i32| {
        let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
        let mut output_string = String::new();
        for i in message..i32::MAX {
            let byte: &u8 = memory.data(&caller).get(i as usize).unwrap();
            if *byte == 0u8 { break }
            output_string.push(char::from(*byte));
        }
        // crate:: is necessary because otherwise it's ambiguous at compile time which macro to use
        crate::println!("[wasm]: {}", output_string);
    })?;


    linker.func_wrap("env", "_drop_reference", |mut caller: Caller<'_, HostState<ExternRef>>, object_index: i32| {
        caller.data_mut().remove(object_index as u32);
    })?;

    // Gameplay Objects
    linker.func_wrap("env", "_create_color_note", |mut caller: Caller<'_, HostState<ExternRef>>, beat: f32| -> i32 {
        let note = unsafe { create_color_note(beat) };
        let extern_ref = ExternRef::new(&mut caller, note);
        caller.data_mut().add(extern_ref) as i32
    })?;


    // INSTANCE FUNCTIONS

    macro_rules! object_add_def {
        ( $linker:ident, $name:ident, $tp:ty ) => {
            $linker.func_wrap("env", concat!("_beatmap_add_", stringify!($name)), |mut caller: Caller<'_, HostState<ExternRef>>, opt: i32| {
                let mut extern_ref = caller.data_mut().remove(opt as u32);
                if let Some(rf) = extern_ref {
                    let val = rf.data(&mut caller).unwrap().downcast_ref::<$tp>().unwrap();
                    unsafe { paste! { [<beatmap_add_ $name>](*val) } }
                } else {
                    panic!("Invalid pointer")
                }
            })?;
        };
    }

    macro_rules! object_remove_def {
        ( $linker:ident, $name:ident, $tp:ty ) => {
            $linker.func_wrap("env", concat!("_beatmap_remove_", stringify!($name)), |mut caller: Caller<'_, HostState<ExternRef>>, opt: i32| {
                let extern_ref = caller.data().get(opt as u32);
                if let Some(rf) = extern_ref {
                    let val = rf.data(&caller).unwrap().downcast_ref::<$tp>().unwrap();
                    unsafe { paste! { [<beatmap_remove_ $name>](*val) } }
                } else {
                    panic!("Invalid pointer")
                }
            })?;
        };
    }

    macro_rules! get_set {
        ( $linker:ident, $name:ident, $tp:ty, $attr:tt, $attr_tp:ty, get ) => {
            $linker.func_wrap("env", concat!("_", stringify!($name), "_get_", stringify!($attr)), |mut caller: Caller<'_, HostState<ExternRef>>, opt: i32| -> i32 {
                let extern_ref = caller.data().get(opt as u32);
                if let Some(rf) = extern_ref {
                    let val = rf.data(&caller).unwrap().downcast_ref::<$tp>().unwrap();
                    let res = unsafe { paste! { [<$name _get_ $attr>](*val) } };

                    let extern_ref = ExternRef::new(&mut caller, res);

                    caller.data_mut().add(extern_ref) as i32

                } else {
                    panic!("Invalid pointer")
                }
            })?;
        };
        ( $linker:ident, $name:ident, $tp:ty, $attr:tt, $attr_tp:ty, set ) => {
            $linker.func_wrap("env", concat!("_", stringify!($name), "_set_", stringify!($attr)), |mut caller: Caller<'_, HostState<ExternRef>>, opt: i32, val: i32| {
                let extern_ref = caller.data().get(opt as u32);
                if let Some(rf) = extern_ref {
                    let v = rf.data(&caller).unwrap().downcast_ref::<$tp>().unwrap();

                    let extern_ref = caller.data().get(val as u32);
                    if let Some(rf) = extern_ref {
                        let v2 = rf.data(&caller).unwrap().downcast_ref::<$attr_tp>().unwrap();
                        unsafe { paste! { [<$name _set_ $attr>](*v, *v2) } }

                    } else {
                        panic!("Invalid pointer")
                    }

                } else {
                    panic!("Invalid pointer")
                }
            })?;
        };
    }

    macro_rules! game_object_common {
        ( $linker:ident, $name:ident, $tp:ty ) => {
            object_add_def! { $linker, $name, $tp }
            object_remove_def! { $linker, $name, $tp }
            get_set! { $linker, $name, $tp, position, glam::Vec3, set }
            get_set! { $linker, $name, $tp, position, glam::Vec3, get }
            get_set! { $linker, $name, $tp, orientation, glam::Quat, set }
            get_set! { $linker, $name, $tp, orientation, glam::Quat, get }
            get_set! { $linker, $name, $tp, color, crate::data::types::Color, set }
            get_set! { $linker, $name, $tp, color, crate::data::types::Color, get }
        };
    }

    game_object_common! { linker, color_note, ColorNote }
    game_object_common! { linker, bomb_note, BombNote }
    game_object_common! { linker, arc, Arc }
    game_object_common! { linker, wall, Wall }
    game_object_common! { linker, chain_head_note, ChainHeadNote }
    game_object_common! { linker, chain_link_note, ChainLinkNote }
    game_object_common! { linker, chain_note, ChainNote }

    todo!(/*
        "_beatmap_remove_color_note",
        "_color_note_get_position",
        "_color_note_set_position",
        "_color_note_get_orientation",
        "_color_note_set_orientation",
        "_color_note_set_color",
        "_color_note_get_color",

        "_create_bomb_note",
        "_beatmap_add_bomb_note",
        "_beatmap_remove_bomb_note",
        "_bomb_note_get_position",
        "_bomb_note_set_position",
        "_bomb_note_get_orientation",
        "_bomb_note_set_orientation",
        "_bomb_note_set_color",
        "_bomb_note_get_color",

        "_create_arc",
        "_beatmap_add_arc",
        "_beatmap_remove_arc",
        "_arc_get_position",
        "_arc_set_position",
        "_arc_get_orientation",
        "_arc_set_orientation",
        "_arc_set_color",
        "_arc_get_color",

        "_create_wall",
        "_beatmap_add_wall",
        "_beatmap_remove_wall",
        "_wall_get_position",
        "_wall_set_position",
        "_wall_get_orientation",
        "_wall_set_orientation",
        "_wall_set_color",
        "_wall_get_color",

        "_create_chain_head_note",
        "_beatmap_add_chain_head_note",
        "_beatmap_remove_chain_head_note",
        "_chain_head_note_get_position",
        "_chain_head_note_set_position",
        "_chain_head_note_get_orientation",
        "_chain_head_note_set_orientation",
        "_chain_head_note_set_color",
        "_chain_head_note_get_color",

        "_create_chain_link_note",
        "_beatmap_add_chain_link_note",
        "_beatmap_remove_chain_link_note",
        "_chain_link_note_get_position",
        "_chain_link_note_set_position",
        "_chain_link_note_get_orientation",
        "_chain_link_note_set_orientation",
        "_chain_link_note_set_color",
        "_chain_link_note_get_color",

        "_create_chain_note",
        "_beatmap_add_chain_note",
        "_beatmap_remove_chain_note",
        "_chain_note_get_position",
        "_chain_note_set_position",
        "_chain_note_get_orientation",
        "_chain_note_set_orientation",
        "_chain_note_set_color",
        "_chain_note_get_color",

        "_saber_set_color",
        "_saber_get_color",
        "_get_left_saber",
        "_get_right_saber",

        "_vec2_from_native",
        "_vec3_from_native",
        "_vec4_from_native",
        "_quat_from_native",

        "_color_set_rgb",
        "_color_set_rgba",

        "_vec2_get_attr_x",
        "_vec2_set_attr_x",
        "_vec2_get_attr_y",
        "_vec2_set_attr_y",

        "_vec3_get_attr_x",
        "_vec3_set_attr_x",
        "_vec3_get_attr_y",
        "_vec3_set_attr_y",
        "_vec3_get_attr_z",
        "_vec3_set_attr_z",

        "_vec4_get_attr_x",
        "_vec4_set_attr_x",
        "_vec4_get_attr_y",
        "_vec4_set_attr_y",
        "_vec4_get_attr_z",
        "_vec4_set_attr_z",
        "_vec4_get_attr_w",
        "_vec4_set_attr_w",

        "_quat_get_attr_x",
        "_quat_set_attr_x",
        "_quat_get_attr_y",
        "_quat_set_attr_y",
        "_quat_get_attr_z",
        "_quat_set_attr_z",
        "_quat_get_attr_w",
        "_quat_set_attr_w",

        "_color_get_attr_r",
        "_color_set_attr_r",
        "_color_get_attr_g",
        "_color_set_attr_g",
        "_color_get_attr_b",
        "_color_set_attr_b",
        "_color_get_attr_a",
        "_color_set_attr_a",
    */);


    Ok(())
}

#[cfg(test)]
mod wasm_tests {
    use anyhow::Result;
    use crate::wasm::wasm_interpreter::HostState;

    #[test]
    fn test_memory() -> Result<()> {

        let mut state = HostState::new();

        state.add(1.0f32);
        state.add(2.0f32);
        state.add(3.0f32);
        state.add(4.0f32);
        state.add(5.0f32);
        state.add(6.0f32);
        state.add(7.0f32);
        state.add(8.0f32);
        state.add(9.0f32);
        state.add(10.0f32);
        println!("{:?}", state);

        state.remove(2);

        println!("{:?}", state);

        state.remove(3);
        println!("{:?}", state);

        state.add(11.0f32);
        println!("{:?}", state);

        state.remove(5);
        println!("{:?}", state);

        state.remove(4);
        println!("{:?}", state);

        Ok(())
    }

}
