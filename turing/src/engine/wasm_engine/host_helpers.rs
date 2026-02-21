use std::{
    ffi::{CStr, CString},
    sync::Arc,
};

use anyhow::anyhow;
use parking_lot::RwLock;
use wasmtime::{Caller, Memory, MemoryAccessError, Val};
use wasmtime_wasi::p1::WasiP1Ctx;

use crate::EngineDataState;

/// gets a string out of wasm memory into rust memory.
pub fn get_wasm_string(message: u32, data: &[u8]) -> String {
    let c = CStr::from_bytes_until_nul(&data[message as usize..]).expect("Not a valid CStr");
    match c.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => c.to_string_lossy().into_owned(),
    }
}

/// writes a string from rust memory to wasm memory.
pub fn write_wasm_string(
    pointer: u32,
    string: &str,
    memory: &Memory,
    caller: Caller<'_, WasiP1Ctx>,
) -> Result<(), MemoryAccessError> {
    let c = CString::new(string).unwrap();
    let bytes = c.into_bytes_with_nul();
    memory.write(caller, pointer as usize, &bytes)
}

pub fn write_u32_vec(
    pointer: u32,
    buf: &[u32],
    memory: &Memory,
    caller: Caller<'_, WasiP1Ctx>,
) -> Result<(), MemoryAccessError> {
    let mut bytes = Vec::with_capacity(buf.len() * 4);
    for (i, num) in buf.iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&num.to_le_bytes())
    }
    memory.write(caller, pointer as usize, &bytes)
}

/// internal for use in the wasm engine only
pub fn wasm_host_strcpy(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();

    if let Some(next_str) = data.write().str_cache.pop_front()
        && next_str.len() + 1 == size as usize
        && let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory())
    {
        write_wasm_string(ptr as u32, &next_str, &memory, caller)?;
        return Ok(());
    }

    Err(anyhow!(
        "An error occurred whilst copying string to wasm memory"
    ))
}

pub fn wasm_host_bufcpy(
    data: &Arc<RwLock<EngineDataState>>,
    mut caller: Caller<'_, WasiP1Ctx>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let ptr = ps[0].i32().unwrap();
    let size = ps[1].i32().unwrap();

    if let Some(next_buf) = data.write().u32_buffer_queue.pop_front()
        && next_buf.len() == size as usize
        && let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory())
    {
        write_u32_vec(ptr as u32, &next_buf, &memory, caller)?;
        return Ok(());
    }

    Err(anyhow!(
        "An error occurred whilst copying a Vec<u32> to wasm memory"
    ))
}

pub fn wasm_host_f32_dequeue(
    data: &Arc<RwLock<EngineDataState>>,
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let mut d = data.write();
    let Some(next) = d.f32_queue.pop_front() else {
        return Err(anyhow!("f32 queue is empty"));
    };
    rs[0] = Val::F32(next.to_bits());
    Ok(())
}

pub fn wasm_host_f32_enqueue(
    data: &Arc<RwLock<EngineDataState>>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let new = ps
        .first()
        .ok_or_else(|| anyhow!("no first parameter provided"))?
        .f32()
        .ok_or_else(|| anyhow!("parameter is not f32"))?;

    let mut d = data.write();
    d.f32_queue.push_back(new);

    Ok(())
}

pub fn wasm_host_u32_dequeue(
    data: &Arc<RwLock<EngineDataState>>,
    rs: &mut [Val],
) -> Result<(), anyhow::Error> {
    let mut d = data.write();
    let Some(next) = d.f32_queue.pop_front() else {
        return Err(anyhow!("f32 queue is empty"));
    };
    rs[0] = Val::I32(next.to_bits() as i32);
    Ok(())
}

pub fn wasm_host_u32_enqueue(
    data: &Arc<RwLock<EngineDataState>>,
    ps: &[Val],
) -> Result<(), anyhow::Error> {
    let new = ps
        .first()
        .ok_or_else(|| anyhow!("no first parameter provided"))?
        .i32()
        .ok_or_else(|| anyhow!("parameter is not u32"))?;

    let mut d = data.write();
    d.f32_queue.push_back(f32::from_bits(new as u32));

    Ok(())
}

/// internal for use in the wasm engine only
///
/// This is used for copying a Vec<u32> from the host to wasm memory. The Vec<u32> should be enqueued using `wasm_host_u32_enqueue` before calling this function, and the pointer and length of the buffer in wasm memory should be passed as parameters.
pub fn get_u32_vec(ptr: u32, len: u32, data: &[u8]) -> Option<Vec<u32>> {
    let start = ptr as usize;
    let end = start.checked_add((len as usize).checked_mul(4)?)?;
    if end > data.len() {
        return None;
    }
    let mut vec = vec![0u32; len as usize];
    for (i, u) in &mut vec.iter_mut().enumerate() {
        let offset = start + i * 4;
        *u = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
    }
    Some(vec)
}
