use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use anyhow::{anyhow, Result};
use wasmi::*;
use wat;
use crate::*;
use crate::data::game_objects::*;
use std::ops::RangeInclusive;

#[derive(Debug)]
pub struct HostState<T> {
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

fn get_string(message: u32, memory: &Memory, caller: &Caller<'_, HostState<ExternRef>>) -> String {
    let mut output_string = String::new();
    for i in message..u32::MAX {
        let byte: &u8 = memory.data(caller).get(i as usize).unwrap();
        if *byte == 0u8 { break }
        output_string.push(char::from(*byte));
    }
    output_string
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
