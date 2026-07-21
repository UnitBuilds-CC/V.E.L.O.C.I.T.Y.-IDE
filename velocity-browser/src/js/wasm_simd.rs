#[derive(Debug, Clone, Copy)]
pub struct WasmV128Vector {
    pub lane_bytes: [u8; 16],
}

pub struct WasmSimdPipeline;

impl WasmSimdPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn execute_vector_add(&self, a: &WasmV128Vector, b: &WasmV128Vector) -> WasmV128Vector {
        let mut res = [0u8; 16];
        for i in 0..16 {
            res[i] = a.lane_bytes[i].wrapping_add(b.lane_bytes[i]);
        }
        WasmV128Vector { lane_bytes: res }
    }
}
