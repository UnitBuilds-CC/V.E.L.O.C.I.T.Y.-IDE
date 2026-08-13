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

    pub fn new(flags: u32) -> Self {
        Self(flags)
    }
    pub fn contains(&self, flag: u32) -> bool {
        self.0 & flag != 0
    }
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
    Buffer {
        buffer_id: usize,
        offset: usize,
        size: usize,
    },
    TextureView {
        texture_id: usize,
    },
    Sampler {
        sampler_id: usize,
    },
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
        if let Some(buf) = self
            .active_buffers
            .iter_mut()
            .find(|b| b.buffer_id == buffer_id)
        {
            let len = data.len().min(buf.size_bytes);
            buf.data[..len].copy_from_slice(&data[..len]);
            true
        } else {
            false
        }
    }

    pub fn read_buffer(&self, buffer_id: usize) -> Option<&[u8]> {
        self.active_buffers
            .iter()
            .find(|b| b.buffer_id == buffer_id)
            .map(|b| b.data.as_slice())
    }

    pub fn remove_buffer(&mut self, buffer_id: usize) -> bool {
        if let Some(pos) = self
            .active_buffers
            .iter()
            .position(|b| b.buffer_id == buffer_id)
        {
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
        if let Some(pipeline) = self
            .pipelines
            .iter_mut()
            .find(|p| p.pipeline_id == pipeline_id)
        {
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
            let all_buffers_valid = pipeline
                .buffer_bindings
                .iter()
                .all(|&bid| self.active_buffers.iter().any(|b| b.buffer_id == bid));
            all_buffers_valid
                && workgroup_count.0 > 0
                && workgroup_count.1 > 0
                && workgroup_count.2 > 0
        } else {
            false
        }
    }

    /// Simulate a simple compute operation: copy input buffer to output buffer.
    pub fn run_copy_compute(
        &mut self,
        pipeline_id: usize,
        input_buffer: usize,
        output_buffer: usize,
    ) -> bool {
        if !self.dispatch_pipeline(pipeline_id, (1, 1, 1)) {
            return false;
        }
        let input_data = self
            .active_buffers
            .iter()
            .find(|b| b.buffer_id == input_buffer)
            .map(|b| b.data.clone());
        if let Some(data) = input_data {
            if let Some(out) = self
                .active_buffers
                .iter_mut()
                .find(|b| b.buffer_id == output_buffer)
            {
                let len = data.len().min(out.size_bytes);
                out.data[..len].copy_from_slice(&data[..len]);
                return true;
            }
        }
        false
    }

    // ── Render Pipelines ──

    pub fn create_render_pipeline(
        &mut self,
        vertex_shader: &str,
        fragment_shader: &str,
        color_format: TextureFormat,
    ) -> usize {
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

    pub fn set_render_pipeline_topology(
        &mut self,
        pipeline_id: usize,
        topology: PrimitiveTopology,
    ) -> bool {
        if let Some(p) = self
            .render_pipelines
            .iter_mut()
            .find(|p| p.pipeline_id == pipeline_id)
        {
            p.primitive_topology = topology;
            true
        } else {
            false
        }
    }

    pub fn set_render_pipeline_blend(&mut self, pipeline_id: usize, enabled: bool) -> bool {
        if let Some(p) = self
            .render_pipelines
            .iter_mut()
            .find(|p| p.pipeline_id == pipeline_id)
        {
            p.blend_enabled = enabled;
            true
        } else {
            false
        }
    }

    // ── Textures ──

    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        format: TextureFormat,
        usage: TextureUsage,
    ) -> usize {
        let tid = self.next_texture_id;
        self.next_texture_id += 1;
        let size = (width * height) as usize * format.bytes_per_pixel();
        self.textures.push(GpuTexture {
            texture_id: tid,
            width,
            height,
            depth_or_array_layers: 1,
            format,
            usage,
            data: vec![0u8; size],
        });
        tid
    }

    pub fn write_texture(&mut self, texture_id: usize, data: &[u8]) -> bool {
        if let Some(tex) = self
            .textures
            .iter_mut()
            .find(|t| t.texture_id == texture_id)
        {
            let len = data.len().min(tex.data.len());
            tex.data[..len].copy_from_slice(&data[..len]);
            true
        } else {
            false
        }
    }

    pub fn read_texture(&self, texture_id: usize) -> Option<&[u8]> {
        self.textures
            .iter()
            .find(|t| t.texture_id == texture_id)
            .map(|t| t.data.as_slice())
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

    pub fn add_compute_pass(
        &mut self,
        encoder_id: usize,
        pipeline_id: usize,
        bind_groups: Vec<usize>,
        workgroup_count: (u32, u32, u32),
    ) -> bool {
        if let Some(enc) = self
            .command_encoders
            .iter_mut()
            .find(|e| e.encoder_id == encoder_id)
        {
            enc.compute_passes.push(ComputePass {
                pipeline_id,
                bind_groups,
                workgroup_count,
            });
            true
        } else {
            false
        }
    }

    pub fn add_copy_command(
        &mut self,
        encoder_id: usize,
        src: usize,
        dst: usize,
        src_offset: usize,
        dst_offset: usize,
        size: usize,
    ) -> bool {
        if let Some(enc) = self
            .command_encoders
            .iter_mut()
            .find(|e| e.encoder_id == encoder_id)
        {
            enc.copy_commands.push(CopyCommand {
                src_buffer: src,
                dst_buffer: dst,
                src_offset,
                dst_offset,
                size,
            });
            true
        } else {
            false
        }
    }

    /// Submit a command encoder: execute all copy commands.
    pub fn submit_encoder(&mut self, encoder_id: usize) -> bool {
        let encoder = self
            .command_encoders
            .iter()
            .find(|e| e.encoder_id == encoder_id);
        let copies = match encoder {
            Some(e) => e.copy_commands.clone(),
            None => return false,
        };
        for copy in &copies {
            let src_data = self
                .active_buffers
                .iter()
                .find(|b| b.buffer_id == copy.src_buffer)
                .map(|b| b.data.clone());
            if let Some(data) = src_data {
                if let Some(dst) = self
                    .active_buffers
                    .iter_mut()
                    .find(|b| b.buffer_id == copy.dst_buffer)
                {
                    let len = copy
                        .size
                        .min(data.len().saturating_sub(copy.src_offset))
                        .min(dst.size_bytes.saturating_sub(copy.dst_offset));
                    dst.data[copy.dst_offset..copy.dst_offset + len]
                        .copy_from_slice(&data[copy.src_offset..copy.src_offset + len]);
                }
            }
        }
        true
    }

    pub fn remove_encoder(&mut self, encoder_id: usize) -> bool {
        if let Some(pos) = self
            .command_encoders
            .iter()
            .position(|e| e.encoder_id == encoder_id)
        {
            self.command_encoders.remove(pos);
            true
        } else {
            false
        }
    }

    // ── Compute Execution ──

    /// Execute a compute pipeline on bound buffers, simulating shader operations.
    pub fn execute_compute(
        &mut self,
        pipeline_id: usize,
        workgroup_count: (u32, u32, u32),
    ) -> bool {
        if !self.dispatch_pipeline(pipeline_id, workgroup_count) {
            return false;
        }

        let pipeline = match self.pipelines.iter().find(|p| p.pipeline_id == pipeline_id) {
            Some(p) => p,
            None => return false,
        };

        let buffer_ids = pipeline.buffer_bindings.clone();
        if buffer_ids.is_empty() {
            return true;
        }

        // Simulate compute: apply shader-like operations to bound buffers
        let total_workgroups = (workgroup_count.0 * workgroup_count.1 * workgroup_count.2) as usize;
        let shader_kind = Self::infer_shader_kind(&pipeline.shader_source);

        match shader_kind {
            ShaderKind::Copy => {
                // Copy first buffer to all others
                if let Some(first_buf) = buffer_ids.first() {
                    let data = self
                        .active_buffers
                        .iter()
                        .find(|b| b.buffer_id == *first_buf)
                        .map(|b| b.data.clone());
                    if let Some(src_data) = data {
                        for &bid in buffer_ids.iter().skip(1) {
                            if let Some(dst) =
                                self.active_buffers.iter_mut().find(|b| b.buffer_id == bid)
                            {
                                let len = src_data.len().min(dst.size_bytes);
                                dst.data[..len].copy_from_slice(&src_data[..len]);
                            }
                        }
                    }
                }
            }
            ShaderKind::Transform => {
                // Apply simple transform: multiply each byte by 2 (mod 256)
                for bid in buffer_ids {
                    if let Some(buf) = self.active_buffers.iter_mut().find(|b| b.buffer_id == bid) {
                        for byte in buf.data.iter_mut() {
                            *byte = byte.wrapping_mul(2);
                        }
                    }
                }
            }
            ShaderKind::ColorFill => {
                // Fill buffers with a pattern based on workgroup count
                let pattern = (total_workgroups % 256) as u8;
                for bid in buffer_ids {
                    if let Some(buf) = self.active_buffers.iter_mut().find(|b| b.buffer_id == bid) {
                        for byte in buf.data.iter_mut() {
                            *byte = pattern;
                        }
                    }
                }
            }
            ShaderKind::Identity => {
                // No-op
            }
        }

        true
    }

    fn infer_shader_kind(shader_source: &str) -> ShaderKind {
        if shader_source.contains("copy") || shader_source.contains("COPY") {
            ShaderKind::Copy
        } else if shader_source.contains("transform") || shader_source.contains("TRANSFORM") {
            ShaderKind::Transform
        } else if shader_source.contains("color") || shader_source.contains("fill") {
            ShaderKind::ColorFill
        } else {
            ShaderKind::Identity
        }
    }

    // ── Render Pass Execution ──

    /// Execute a render pass, simulating vertex processing and rasterization.
    pub fn execute_render_pass(&mut self, encoder_id: usize, render_pass_id: usize) -> bool {
        let render_pass = match self
            .command_encoders
            .iter()
            .find(|e| e.encoder_id == encoder_id)
        {
            Some(enc) => enc.render_passes.get(render_pass_id).cloned(),
            None => None,
        };

        let render_pass = match render_pass {
            Some(rp) => rp,
            None => return false,
        };

        // Validate pipeline
        if !self
            .render_pipelines
            .iter()
            .any(|p| p.pipeline_id == render_pass.pipeline_id)
        {
            return false;
        }

        // Simulate vertex processing: read vertex buffer
        let vertex_data = self
            .active_buffers
            .iter()
            .find(|b| b.buffer_id == render_pass.vertex_buffer_id)
            .map(|b| b.data.clone());

        let vertex_data = match vertex_data {
            Some(d) => d,
            None => return false,
        };

        // Simulate rasterization: write to color attachment texture
        if let Some(tex) = self
            .textures
            .iter_mut()
            .find(|t| t.texture_id == render_pass.color_attachment)
        {
            // Simple triangle fill: fill texture with a gradient based on vertex data
            let pattern_byte = vertex_data.first().copied().unwrap_or(128);
            for pixel in tex.data.chunks_exact_mut(4) {
                pixel[0] = pattern_byte; // R
                pixel[1] = pattern_byte.wrapping_add(64); // G
                pixel[2] = pattern_byte.wrapping_add(128); // B
                pixel[3] = 255; // A
            }
        }

        true
    }

    /// Add a render pass to an encoder.
    pub fn add_render_pass(
        &mut self,
        encoder_id: usize,
        pipeline_id: usize,
        color_attachment: usize,
        vertex_buffer_id: usize,
        vertex_count: u32,
    ) -> bool {
        if let Some(enc) = self
            .command_encoders
            .iter_mut()
            .find(|e| e.encoder_id == encoder_id)
        {
            enc.render_passes.push(RenderPass {
                pipeline_id,
                color_attachment,
                depth_attachment: None,
                vertex_buffer_id,
                index_buffer_id: None,
                bind_groups: Vec::new(),
                vertex_count,
                index_count: None,
            });
            true
        } else {
            false
        }
    }

    // ── Query Support ──

    /// Query buffer information for agent inspection.
    pub fn query_buffer_summary(&self, buffer_id: usize) -> Option<(usize, usize, BufferUsage)> {
        self.active_buffers
            .iter()
            .find(|b| b.buffer_id == buffer_id)
            .map(|b| (b.size_bytes, b.data.len(), b.usage))
    }

    /// Query texture information for agent inspection.
    pub fn query_texture_summary(
        &self,
        texture_id: usize,
    ) -> Option<(u32, u32, TextureFormat, usize)> {
        self.textures
            .iter()
            .find(|t| t.texture_id == texture_id)
            .map(|t| (t.width, t.height, t.format, t.data.len()))
    }

    // ── Timestamp Queries ──

    /// Begin a timestamp query (records fake GPU timestamp).
    pub fn begin_timestamp_query(&mut self, encoder_id: usize) -> bool {
        self.command_encoders
            .iter()
            .any(|e| e.encoder_id == encoder_id)
    }

    /// End a timestamp query (records fake GPU timestamp).
    pub fn end_timestamp_query(&mut self, encoder_id: usize) -> bool {
        self.command_encoders
            .iter()
            .any(|e| e.encoder_id == encoder_id)
    }
}

/// Shader kind for compute operations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaderKind {
    Identity,
    Copy,
    Transform,
    ColorFill,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_compute_copy() {
        let mut engine = WebGpuComputeEngine::new();
        let buf1 = engine.create_buffer(16);
        let buf2 = engine.create_buffer(16);
        engine.write_buffer(
            buf1,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );

        let pipeline = engine.create_pipeline("@compute fn copy() { /* copy */ }", (4, 1, 1));
        engine.bind_buffer(pipeline, buf1);
        engine.bind_buffer(pipeline, buf2);

        assert!(engine.execute_compute(pipeline, (1, 1, 1)));
        let data2 = engine.read_buffer(buf2).unwrap();
        assert_eq!(
            data2,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn execute_compute_transform() {
        let mut engine = WebGpuComputeEngine::new();
        let buf = engine.create_buffer(4);
        engine.write_buffer(buf, &[10, 20, 30, 40]);

        let pipeline =
            engine.create_pipeline("@compute fn transform() { /* transform */ }", (1, 1, 1));
        engine.bind_buffer(pipeline, buf);

        assert!(engine.execute_compute(pipeline, (1, 1, 1)));
        let data = engine.read_buffer(buf).unwrap();
        assert_eq!(data, &[20, 40, 60, 80]);
    }

    #[test]
    fn execute_render_pass() {
        let mut engine = WebGpuComputeEngine::new();
        let vertex_buf = engine.create_buffer_with_usage(12, BufferUsage::Vertex);
        engine.write_buffer(vertex_buf, &[100, 50, 25]);

        let texture = engine.create_texture(
            4,
            4,
            TextureFormat::Rgba8Unorm,
            TextureUsage::new(TextureUsage::RENDER_ATTACHMENT),
        );
        let pipeline =
            engine.create_render_pipeline("vertex", "fragment", TextureFormat::Rgba8Unorm);

        let encoder = engine.create_command_encoder();
        engine.add_render_pass(encoder, pipeline, texture, vertex_buf, 3);

        assert!(engine.execute_render_pass(encoder, 0));
        let tex_data = engine.read_texture(texture).unwrap();
        assert_eq!(tex_data.len(), 64); // 4x4 pixels * 4 bytes
        assert_eq!(tex_data[0], 100); // R
        assert_eq!(tex_data[1], 164); // G (100 + 64)
        assert_eq!(tex_data[2], 228); // B (100 + 128)
        assert_eq!(tex_data[3], 255); // A
    }

    #[test]
    fn query_buffer_and_texture() {
        let mut engine = WebGpuComputeEngine::new();
        let buf = engine.create_buffer(1024);
        let tex = engine.create_texture(
            256,
            256,
            TextureFormat::Rgba8Unorm,
            TextureUsage::new(TextureUsage::TEXTURE_BINDING),
        );

        let (size, data_len, usage) = engine.query_buffer_summary(buf).unwrap();
        assert_eq!(size, 1024);
        assert_eq!(data_len, 1024);
        assert_eq!(usage, BufferUsage::Storage);

        let (w, h, fmt, data_len) = engine.query_texture_summary(tex).unwrap();
        assert_eq!(w, 256);
        assert_eq!(h, 256);
        assert_eq!(fmt, TextureFormat::Rgba8Unorm);
        assert_eq!(data_len, 256 * 256 * 4);
    }

    #[test]
    fn vertex_format_byte_sizes() {
        assert_eq!(VertexFormat::Float32.byte_size(), 4);
        assert_eq!(VertexFormat::Float32x2.byte_size(), 8);
        assert_eq!(VertexFormat::Float32x3.byte_size(), 12);
        assert_eq!(VertexFormat::Float32x4.byte_size(), 16);
        assert_eq!(VertexFormat::Uint8x4.byte_size(), 4);
    }

    #[test]
    fn texture_format_bytes_per_pixel() {
        assert_eq!(TextureFormat::Rgba8Unorm.bytes_per_pixel(), 4);
        assert_eq!(TextureFormat::Bgra8Unorm.bytes_per_pixel(), 4);
        assert_eq!(TextureFormat::R8Unorm.bytes_per_pixel(), 1);
        assert_eq!(TextureFormat::Depth24Plus.bytes_per_pixel(), 4);
        assert_eq!(TextureFormat::Depth24PlusStencil8.bytes_per_pixel(), 4);
    }

    #[test]
    fn texture_usage_contains() {
        let u = TextureUsage::new(TextureUsage::COPY_SRC | TextureUsage::COPY_DST);
        assert!(u.contains(TextureUsage::COPY_SRC));
        assert!(u.contains(TextureUsage::COPY_DST));
        assert!(!u.contains(TextureUsage::TEXTURE_BINDING));
    }

    #[test]
    fn write_buffer_nonexistent() {
        let mut engine = WebGpuComputeEngine::new();
        assert!(!engine.write_buffer(999, &[1, 2, 3]));
    }

    #[test]
    fn read_buffer_nonexistent() {
        let engine = WebGpuComputeEngine::new();
        assert!(engine.read_buffer(999).is_none());
    }

    #[test]
    fn remove_buffer_nonexistent() {
        let mut engine = WebGpuComputeEngine::new();
        assert!(!engine.remove_buffer(999));
    }

    #[test]
    fn remove_buffer_cleans_pipeline_bindings() {
        let mut engine = WebGpuComputeEngine::new();
        let buf = engine.create_buffer(8);
        let pipe = engine.create_pipeline("shader", (1, 1, 1));
        engine.bind_buffer(pipe, buf);
        assert_eq!(engine.pipelines[0].buffer_bindings.len(), 1);
        engine.remove_buffer(buf);
        assert!(engine.pipelines[0].buffer_bindings.is_empty());
    }

    #[test]
    fn total_memory_and_buffer_count() {
        let mut engine = WebGpuComputeEngine::new();
        engine.create_buffer(100);
        engine.create_buffer(200);
        assert_eq!(engine.buffer_count(), 2);
        assert_eq!(engine.total_memory_bytes(), 300);
    }

    #[test]
    fn dispatch_compute_zero_workgroup() {
        let engine = WebGpuComputeEngine::new();
        assert!(!engine.dispatch_compute("shader", (0, 1, 1)));
        assert!(!engine.dispatch_compute("shader", (1, 0, 1)));
        assert!(!engine.dispatch_compute("shader", (1, 1, 0)));
    }

    #[test]
    fn dispatch_pipeline_nonexistent() {
        let engine = WebGpuComputeEngine::new();
        assert!(!engine.dispatch_pipeline(999, (1, 1, 1)));
    }

    #[test]
    fn bind_buffer_duplicate_ignored() {
        let mut engine = WebGpuComputeEngine::new();
        let buf = engine.create_buffer(8);
        let pipe = engine.create_pipeline("s", (1, 1, 1));
        assert!(engine.bind_buffer(pipe, buf));
        assert!(engine.bind_buffer(pipe, buf)); // duplicate
        assert_eq!(engine.pipelines[0].buffer_bindings.len(), 1);
    }

    #[test]
    fn query_buffer_summary_nonexistent() {
        let engine = WebGpuComputeEngine::new();
        assert!(engine.query_buffer_summary(999).is_none());
    }

    #[test]
    fn query_texture_summary_nonexistent() {
        let engine = WebGpuComputeEngine::new();
        assert!(engine.query_texture_summary(999).is_none());
    }

    #[test]
    fn timestamp_query_valid_encoder() {
        let mut engine = WebGpuComputeEngine::new();
        let enc = engine.create_command_encoder();
        assert!(engine.begin_timestamp_query(enc));
        assert!(engine.end_timestamp_query(enc));
    }

    #[test]
    fn timestamp_query_invalid_encoder() {
        let mut engine = WebGpuComputeEngine::new();
        assert!(!engine.begin_timestamp_query(999));
        assert!(!engine.end_timestamp_query(999));
    }
}
