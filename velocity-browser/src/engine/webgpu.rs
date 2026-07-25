/// WebGPU compute buffer for GPU-accelerated computations.
#[derive(Debug, Clone)]
pub struct WebGpuComputeBuffer {
    pub buffer_id: usize,
    pub size_bytes: usize,
    pub usage: BufferUsage,
    pub data: Vec<u8>,
}

/// Buffer usage flags for WebGPU buffers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BufferUsage {
    Storage,
    Uniform,
    Vertex,
    Index,
    MapRead,
    MapWrite,
}

/// Shader stage flags for pipeline configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

/// WebGPU compute pipeline configuration.
#[derive(Debug, Clone)]
pub struct ComputePipeline {
    pub pipeline_id: usize,
    pub shader_source: String,
    pub workgroup_size: (u32, u32, u32),
    pub buffer_bindings: Vec<usize>,
}

/// WebGPU compute engine for GPU-accelerated operations.
pub struct WebGpuComputeEngine {
    pub active_buffers: Vec<WebGpuComputeBuffer>,
    pub pipelines: Vec<ComputePipeline>,
    next_buffer_id: usize,
    next_pipeline_id: usize,
}

impl Default for WebGpuComputeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WebGpuComputeEngine {
    pub fn new() -> Self {
        Self {
            active_buffers: Vec::new(),
            pipelines: Vec::new(),
            next_buffer_id: 1,
            next_pipeline_id: 1,
        }
    }

    /// Create a buffer with the given size and usage.
    pub fn create_buffer(&mut self, size_bytes: usize) -> usize {
        self.create_buffer_with_usage(size_bytes, BufferUsage::Storage)
    }

    /// Create a buffer with specific usage flags.
    pub fn create_buffer_with_usage(&mut self, size_bytes: usize, usage: BufferUsage) -> usize {
        let bid = self.next_buffer_id;
        self.next_buffer_id += 1;
        self.active_buffers.push(WebGpuComputeBuffer {
            buffer_id: bid,
            size_bytes,
            usage,
            data: vec![0u8; size_bytes],
        });
        bid
    }

    /// Write data to a buffer.
    pub fn write_buffer(&mut self, buffer_id: usize, data: &[u8]) -> bool {
        if let Some(buf) = self.active_buffers.iter_mut().find(|b| b.buffer_id == buffer_id) {
            let len = data.len().min(buf.size_bytes);
            buf.data[..len].copy_from_slice(&data[..len]);
            true
        } else {
            false
        }
    }

    /// Read data from a buffer.
    pub fn read_buffer(&self, buffer_id: usize) -> Option<&[u8]> {
        self.active_buffers.iter()
            .find(|b| b.buffer_id == buffer_id)
            .map(|b| b.data.as_slice())
    }

    /// Create a compute pipeline with a shader and workgroup size.
    pub fn create_pipeline(&mut self, shader_src: &str, workgroup_size: (u32, u32, u32)) -> usize {
        let pid = self.next_pipeline_id;
        self.next_pipeline_id += 1;
        self.pipelines.push(ComputePipeline {
            pipeline_id: pid,
            shader_source: shader_src.to_string(),
            workgroup_size,
            buffer_bindings: Vec::new(),
        });
        pid
    }

    /// Bind a buffer to a pipeline.
    pub fn bind_buffer(&mut self, pipeline_id: usize, buffer_id: usize) -> bool {
        if let Some(pipeline) = self.pipelines.iter_mut().find(|p| p.pipeline_id == pipeline_id) {
            if !pipeline.buffer_bindings.contains(&buffer_id) {
                pipeline.buffer_bindings.push(buffer_id);
            }
            true
        } else {
            false
        }
    }

    /// Dispatch a compute shader with the given workgroup count.
    pub fn dispatch_compute(&self, _shader_src: &str, workgroup_count: (u32, u32, u32)) -> bool {
        workgroup_count.0 > 0 && workgroup_count.1 > 0 && workgroup_count.2 > 0
    }

    /// Dispatch a specific pipeline by ID.
    pub fn dispatch_pipeline(&self, pipeline_id: usize, workgroup_count: (u32, u32, u32)) -> bool {
        if let Some(pipeline) = self.pipelines.iter().find(|p| p.pipeline_id == pipeline_id) {
            // Validate all bound buffers exist
            let all_buffers_valid = pipeline.buffer_bindings.iter().all(|&bid| {
                self.active_buffers.iter().any(|b| b.buffer_id == bid)
            });
            all_buffers_valid && workgroup_count.0 > 0 && workgroup_count.1 > 0 && workgroup_count.2 > 0
        } else {
            false
        }
    }

    /// Remove a buffer by ID.
    pub fn remove_buffer(&mut self, buffer_id: usize) -> bool {
        if let Some(pos) = self.active_buffers.iter().position(|b| b.buffer_id == buffer_id) {
            self.active_buffers.remove(pos);
            // Remove references from pipelines
            for pipeline in &mut self.pipelines {
                pipeline.buffer_bindings.retain(|&id| id != buffer_id);
            }
            true
        } else {
            false
        }
    }

    /// Get the total GPU memory used by all buffers.
    pub fn total_memory_bytes(&self) -> usize {
        self.active_buffers.iter().map(|b| b.size_bytes).sum()
    }

    /// Get the number of active buffers.
    pub fn buffer_count(&self) -> usize {
        self.active_buffers.len()
    }
}
