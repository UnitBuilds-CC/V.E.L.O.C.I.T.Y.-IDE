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

/// Render pipeline for GPU-accelerated rendering.
#[derive(Debug, Clone)]
pub struct RenderPipeline {
    pub pipeline_id: usize,
    pub vertex_shader: String,
    pub fragment_shader: String,
    pub vertex_buffer_layout: Vec<VertexAttribute>,
    pub color_format: TextureFormat,
    pub depth_format: Option<TextureFormat>,
    pub primitive_topology: PrimitiveTopology,
    pub blend_enabled: bool,
}

/// Vertex attribute descriptor.
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    pub name: String,
    pub format: VertexFormat,
    pub offset: usize,
    pub shader_location: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint8x4,
}

impl VertexFormat {
    pub fn byte_size(&self) -> usize {
        match self {
            VertexFormat::Float32 => 4,
            VertexFormat::Float32x2 => 8,
            VertexFormat::Float32x3 => 12,
            VertexFormat::Float32x4 => 16,
            VertexFormat::Uint8x4 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
}

/// GPU texture for WebGPU.
#[derive(Debug, Clone)]
pub struct GpuTexture {
    pub texture_id: usize,
    pub width: u32,
    pub height: u32,
    pub depth_or_array_layers: u32,
    pub format: TextureFormat,
    pub usage: TextureUsage,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextureFormat {
    Rgba8Unorm,
    Bgra8Unorm,
    R8Unorm,
    Depth24Plus,
    Depth24PlusStencil8,
}

impl TextureFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm => 4,
            TextureFormat::R8Unorm => 1,
            TextureFormat::Depth24Plus => 4,
            TextureFormat::Depth24PlusStencil8 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureUsage(pub u32);

impl TextureUsage {
    pub const COPY_SRC: u32 = 0x01;
    pub const COPY_DST: u32 = 0x02;
    pub const TEXTURE_BINDING: u32 = 0x04;
    pub const STORAGE_BINDING: u32 = 0x08;
    pub const RENDER_ATTACHMENT: u32 = 0x10;

    pub fn new(flags: u32) -> Self { Self(flags) }
    pub fn contains(&self, flag: u32) -> bool { self.0 & flag != 0 }
}

/// Bind group for connecting resources to shaders.
#[derive(Debug, Clone)]
pub struct BindGroup {
    pub group_id: usize,
    pub layout_id: usize,
    pub entries: Vec<BindGroupEntry>,
}

#[derive(Debug, Clone)]
pub struct BindGroupEntry {
    pub binding: u32,
    pub resource: BindResource,
}

#[derive(Debug, Clone)]
pub enum BindResource {
    Buffer { buffer_id: usize, offset: usize, size: usize },
    TextureView { texture_id: usize },
    Sampler { sampler_id: usize },
}

/// Command encoder for batching GPU operations.
#[derive(Debug, Clone)]
pub struct CommandEncoder {
    pub encoder_id: usize,
    pub compute_passes: Vec<ComputePass>,
    pub render_passes: Vec<RenderPass>,
    pub copy_commands: Vec<CopyCommand>,
}

#[derive(Debug, Clone)]
pub struct ComputePass {
    pub pipeline_id: usize,
    pub bind_groups: Vec<usize>,
    pub workgroup_count: (u32, u32, u32),
}

#[derive(Debug, Clone)]
pub struct RenderPass {
    pub pipeline_id: usize,
    pub color_attachment: usize,
    pub depth_attachment: Option<usize>,
    pub vertex_buffer_id: usize,
    pub index_buffer_id: Option<usize>,
    pub bind_groups: Vec<usize>,
    pub vertex_count: u32,
    pub index_count: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CopyCommand {
    pub src_buffer: usize,
    pub dst_buffer: usize,
    pub src_offset: usize,
    pub dst_offset: usize,
    pub size: usize,
}

/// WebGPU compute engine for GPU-accelerated operations.
pub struct WebGpuComputeEngine {
    pub active_buffers: Vec<WebGpuComputeBuffer>,
    pub pipelines: Vec<ComputePipeline>,
    pub render_pipelines: Vec<RenderPipeline>,
    pub textures: Vec<GpuTexture>,
    pub bind_groups: Vec<BindGroup>,
    pub command_encoders: Vec<CommandEncoder>,
    next_buffer_id: usize,
    next_pipeline_id: usize,
    next_texture_id: usize,
    next_bind_group_id: usize,
    next_encoder_id: usize,
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
            render_pipelines: Vec::new(),
            textures: Vec::new(),
            bind_groups: Vec::new(),
            command_encoders: Vec::new(),
            next_buffer_id: 1,
            next_pipeline_id: 1,
            next_texture_id: 1,
            next_bind_group_id: 1,
            next_encoder_id: 1,
        }
    }

    // ── Buffers ──

    pub fn create_buffer(&mut self, size_bytes: usize) -> usize {
        self.create_buffer_with_usage(size_bytes, BufferUsage::Storage)
    }

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

    pub fn write_buffer(&mut self, buffer_id: usize, data: &[u8]) -> bool {
        if let Some(buf) = self.active_buffers.iter_mut().find(|b| b.buffer_id == buffer_id) {
            let len = data.len().min(buf.size_bytes);
            buf.data[..len].copy_from_slice(&data[..len]);
            true
        } else {
            false
        }
    }

    pub fn read_buffer(&self, buffer_id: usize) -> Option<&[u8]> {
        self.active_buffers.iter()
            .find(|b| b.buffer_id == buffer_id)
            .map(|b| b.data.as_slice())
    }

    pub fn remove_buffer(&mut self, buffer_id: usize) -> bool {
        if let Some(pos) = self.active_buffers.iter().position(|b| b.buffer_id == buffer_id) {
            self.active_buffers.remove(pos);
            for pipeline in &mut self.pipelines {
                pipeline.buffer_bindings.retain(|&id| id != buffer_id);
            }
            true
        } else {
            false
        }
    }

    pub fn total_memory_bytes(&self) -> usize {
        self.active_buffers.iter().map(|b| b.size_bytes).sum()
    }

    pub fn buffer_count(&self) -> usize {
        self.active_buffers.len()
    }

    // ── Compute Pipelines ──

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

    pub fn dispatch_compute(&self, _shader_src: &str, workgroup_count: (u32, u32, u32)) -> bool {
        workgroup_count.0 > 0 && workgroup_count.1 > 0 && workgroup_count.2 > 0
    }

    pub fn dispatch_pipeline(&self, pipeline_id: usize, workgroup_count: (u32, u32, u32)) -> bool {
        if let Some(pipeline) = self.pipelines.iter().find(|p| p.pipeline_id == pipeline_id) {
            let all_buffers_valid = pipeline.buffer_bindings.iter().all(|&bid| {
                self.active_buffers.iter().any(|b| b.buffer_id == bid)
            });
            all_buffers_valid && workgroup_count.0 > 0 && workgroup_count.1 > 0 && workgroup_count.2 > 0
        } else {
            false
        }
    }

    /// Simulate a simple compute operation: copy input buffer to output buffer.
    pub fn run_copy_compute(&mut self, pipeline_id: usize, input_buffer: usize, output_buffer: usize) -> bool {
        if !self.dispatch_pipeline(pipeline_id, (1, 1, 1)) { return false; }
        let input_data = self.active_buffers.iter().find(|b| b.buffer_id == input_buffer).map(|b| b.data.clone());
        if let Some(data) = input_data {
            if let Some(out) = self.active_buffers.iter_mut().find(|b| b.buffer_id == output_buffer) {
                let len = data.len().min(out.size_bytes);
                out.data[..len].copy_from_slice(&data[..len]);
                return true;
            }
        }
        false
    }

    // ── Render Pipelines ──

    pub fn create_render_pipeline(&mut self, vertex_shader: &str, fragment_shader: &str, color_format: TextureFormat) -> usize {
        let pid = self.next_pipeline_id;
        self.next_pipeline_id += 1;
        self.render_pipelines.push(RenderPipeline {
            pipeline_id: pid,
            vertex_shader: vertex_shader.to_string(),
            fragment_shader: fragment_shader.to_string(),
            vertex_buffer_layout: Vec::new(),
            color_format,
            depth_format: None,
            primitive_topology: PrimitiveTopology::TriangleList,
            blend_enabled: false,
        });
        pid
    }

    pub fn set_render_pipeline_topology(&mut self, pipeline_id: usize, topology: PrimitiveTopology) -> bool {
        if let Some(p) = self.render_pipelines.iter_mut().find(|p| p.pipeline_id == pipeline_id) {
            p.primitive_topology = topology;
            true
        } else { false }
    }

    pub fn set_render_pipeline_blend(&mut self, pipeline_id: usize, enabled: bool) -> bool {
        if let Some(p) = self.render_pipelines.iter_mut().find(|p| p.pipeline_id == pipeline_id) {
            p.blend_enabled = enabled;
            true
        } else { false }
    }

    // ── Textures ──

    pub fn create_texture(&mut self, width: u32, height: u32, format: TextureFormat, usage: TextureUsage) -> usize {
        let tid = self.next_texture_id;
        self.next_texture_id += 1;
        let size = (width * height) as usize * format.bytes_per_pixel();
        self.textures.push(GpuTexture {
            texture_id: tid,
            width, height,
            depth_or_array_layers: 1,
            format,
            usage,
            data: vec![0u8; size],
        });
        tid
    }

    pub fn write_texture(&mut self, texture_id: usize, data: &[u8]) -> bool {
        if let Some(tex) = self.textures.iter_mut().find(|t| t.texture_id == texture_id) {
            let len = data.len().min(tex.data.len());
            tex.data[..len].copy_from_slice(&data[..len]);
            true
        } else { false }
    }

    pub fn read_texture(&self, texture_id: usize) -> Option<&[u8]> {
        self.textures.iter().find(|t| t.texture_id == texture_id).map(|t| t.data.as_slice())
    }

    // ── Bind Groups ──

    pub fn create_bind_group(&mut self, layout_id: usize, entries: Vec<BindGroupEntry>) -> usize {
        let gid = self.next_bind_group_id;
        self.next_bind_group_id += 1;
        self.bind_groups.push(BindGroup {
            group_id: gid,
            layout_id,
            entries,
        });
        gid
    }

    // ── Command Encoders ──

    pub fn create_command_encoder(&mut self) -> usize {
        let eid = self.next_encoder_id;
        self.next_encoder_id += 1;
        self.command_encoders.push(CommandEncoder {
            encoder_id: eid,
            compute_passes: Vec::new(),
            render_passes: Vec::new(),
            copy_commands: Vec::new(),
        });
        eid
    }

    pub fn add_compute_pass(&mut self, encoder_id: usize, pipeline_id: usize, bind_groups: Vec<usize>, workgroup_count: (u32, u32, u32)) -> bool {
        if let Some(enc) = self.command_encoders.iter_mut().find(|e| e.encoder_id == encoder_id) {
            enc.compute_passes.push(ComputePass { pipeline_id, bind_groups, workgroup_count });
            true
        } else { false }
    }

    pub fn add_copy_command(&mut self, encoder_id: usize, src: usize, dst: usize, src_offset: usize, dst_offset: usize, size: usize) -> bool {
        if let Some(enc) = self.command_encoders.iter_mut().find(|e| e.encoder_id == encoder_id) {
            enc.copy_commands.push(CopyCommand { src_buffer: src, dst_buffer: dst, src_offset, dst_offset, size });
            true
        } else { false }
    }

    /// Submit a command encoder: execute all copy commands.
    pub fn submit_encoder(&mut self, encoder_id: usize) -> bool {
        let encoder = self.command_encoders.iter().find(|e| e.encoder_id == encoder_id);
        let copies = match encoder {
            Some(e) => e.copy_commands.clone(),
            None => return false,
        };
        for copy in &copies {
            let src_data = self.active_buffers.iter().find(|b| b.buffer_id == copy.src_buffer).map(|b| b.data.clone());
            if let Some(data) = src_data {
                if let Some(dst) = self.active_buffers.iter_mut().find(|b| b.buffer_id == copy.dst_buffer) {
                    let len = copy.size.min(data.len().saturating_sub(copy.src_offset)).min(dst.size_bytes.saturating_sub(copy.dst_offset));
                    dst.data[copy.dst_offset..copy.dst_offset + len].copy_from_slice(&data[copy.src_offset..copy.src_offset + len]);
                }
            }
        }
        true
    }

    pub fn remove_encoder(&mut self, encoder_id: usize) -> bool {
        if let Some(pos) = self.command_encoders.iter().position(|e| e.encoder_id == encoder_id) {
            self.command_encoders.remove(pos);
            true
        } else { false }
    }
}
