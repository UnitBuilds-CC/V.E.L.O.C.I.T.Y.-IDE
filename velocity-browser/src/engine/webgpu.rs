#[derive(Debug, Clone)]
pub struct WebGpuComputeBuffer {
    pub buffer_id: usize,
    pub size_bytes: usize,
}

pub struct WebGpuComputeEngine {
    pub active_buffers: Vec<WebGpuComputeBuffer>,
}

impl Default for WebGpuComputeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WebGpuComputeEngine {
    pub fn new() -> Self {
        Self { active_buffers: Vec::new() }
    }

    pub fn create_buffer(&mut self, size_bytes: usize) -> usize {
        let bid = self.active_buffers.len() + 1;
        self.active_buffers.push(WebGpuComputeBuffer {
            buffer_id: bid,
            size_bytes,
        });
        bid
    }

    pub fn dispatch_compute(&self, _shader_src: &str, workgroup_count: (u32, u32, u32)) -> bool {
        workgroup_count.0 > 0 && workgroup_count.1 > 0 && workgroup_count.2 > 0
    }
}
