#[derive(Debug, Clone)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

pub struct WasmInterpreter {
    pub memory: Vec<u8>,
    pub stack: Vec<WasmValue>,
}

impl WasmInterpreter {
    pub fn new(initial_memory_pages: usize) -> Self {
        Self {
            memory: vec![0u8; initial_memory_pages * 64 * 1024], // 64KB per page
            stack: Vec::new(),
        }
    }

    pub fn execute_i32_add(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let (Some(WasmValue::I32(b)), Some(WasmValue::I32(a))) = (self.stack.pop(), self.stack.pop()) {
            self.stack.push(WasmValue::I32(a.wrapping_add(b)));
            Ok(())
        } else {
            Err("Wasm stack underflow or type mismatch".into())
        }
    }

    pub fn write_memory(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if offset + bytes.len() <= self.memory.len() {
            self.memory[offset..offset + bytes.len()].copy_from_slice(bytes);
            Ok(())
        } else {
            Err("Wasm memory out of bounds".into())
        }
    }

    pub fn read_memory(&self, offset: usize, len: usize) -> Result<&[u8], Box<dyn std::error::Error + Send + Sync>> {
        if offset + len <= self.memory.len() {
            Ok(&self.memory[offset..offset + len])
        } else {
            Err("Wasm memory out of bounds".into())
        }
    }
}
