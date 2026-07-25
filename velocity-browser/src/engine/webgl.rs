use crate::engine::PixelBuffer;

#[derive(Debug, Clone)]
pub struct Matrix4x4 {
    pub m: [[f32; 4]; 4],
}

impl Matrix4x4 {
    pub fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn multiply(&self, other: &Matrix4x4) -> Matrix4x4 {
        let mut result = [[0.0f32; 4]; 4];
        for row in 0..4 {
            for col in 0..4 {
                let mut sum = 0.0f32;
                for k in 0..4 {
                    sum += self.m[row][k] * other.m[k][col];
                }
                result[row][col] = sum;
            }
        }
        Matrix4x4 { m: result }
    }

    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov_y / 2.0).tan();
        let range_inv = 1.0 / (near - far);
        Self {
            m: [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [0.0, 0.0, (far + near) * range_inv, 2.0 * far * near * range_inv],
                [0.0, 0.0, -1.0, 0.0],
            ],
        }
    }

    pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Self {
        let rl = right - left;
        let tb = top - bottom;
        let fn_ = far - near;
        Self {
            m: [
                [2.0 / rl, 0.0, 0.0, -(right + left) / rl],
                [0.0, 2.0 / tb, 0.0, -(top + bottom) / tb],
                [0.0, 0.0, -2.0 / fn_, -(far + near) / fn_],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn translate(tx: f32, ty: f32, tz: f32) -> Self {
        let mut m = Self::identity();
        m.m[0][3] = tx;
        m.m[1][3] = ty;
        m.m[2][3] = tz;
        m
    }

    pub fn scale(sx: f32, sy: f32, sz: f32) -> Self {
        Self {
            m: [
                [sx, 0.0, 0.0, 0.0],
                [0.0, sy, 0.0, 0.0],
                [0.0, 0.0, sz, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

/// Shader program model: vertex + fragment shader pair with uniform locations.
#[derive(Debug, Clone)]
pub struct ShaderProgram {
    pub id: u32,
    pub vertex_source: String,
    pub fragment_source: String,
    pub uniforms: std::collections::HashMap<String, ShaderUniform>,
    pub attributes: std::collections::HashMap<String, u32>,
    pub linked: bool,
}

#[derive(Debug, Clone)]
pub enum ShaderUniform {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat4(Matrix4x4),
    Int(i32),
}

/// Texture unit with format, dimensions, and pixel data.
#[derive(Debug, Clone)]
pub struct Texture2D {
    pub id: u32,
    pub width: usize,
    pub height: usize,
    pub format: TextureFormat,
    pub data: Vec<u8>,
    pub min_filter: TextureFilter,
    pub mag_filter: TextureFilter,
    pub wrap_s: TextureWrap,
    pub wrap_t: TextureWrap,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextureFormat {
    RGBA8,
    RGB8,
    Alpha8,
    Depth16,
    Depth24Stencil8,
}

impl TextureFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            TextureFormat::RGBA8 => 4,
            TextureFormat::RGB8 => 3,
            TextureFormat::Alpha8 => 1,
            TextureFormat::Depth16 => 2,
            TextureFormat::Depth24Stencil8 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextureFilter { Nearest, Linear }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextureWrap { ClampToEdge, Repeat, MirroredRepeat }

/// Framebuffer object with color attachment and optional depth buffer.
#[derive(Debug, Clone)]
pub struct Framebuffer {
    pub id: u32,
    pub color_texture_id: Option<u32>,
    pub depth_renderbuffer: bool,
    pub width: usize,
    pub height: usize,
    pub complete: bool,
}

/// Index buffer for indexed draw calls.
#[derive(Debug, Clone)]
pub struct IndexBuffer {
    pub id: u32,
    pub indices: Vec<u32>,
    pub element_type: IndexType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndexType { UnsignedShort, UnsignedInt }

/// Viewport state.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
}

pub struct WebGLContext {
    pub width: usize,
    pub height: usize,
    pub projection_matrix: Matrix4x4,
    pub model_view_matrix: Matrix4x4,
    pub vertex_buffer: Vec<f32>,
    pub pixel_buffer: PixelBuffer,
    pub viewport: Viewport,
    pub programs: Vec<ShaderProgram>,
    pub textures: Vec<Texture2D>,
    pub framebuffers: Vec<Framebuffer>,
    pub index_buffers: Vec<IndexBuffer>,
    pub active_texture_unit: u32,
    pub current_program_id: Option<u32>,
    pub current_framebuffer_id: Option<u32>,
    pub depth_test_enabled: bool,
    pub blend_enabled: bool,
    pub clear_color: [f32; 4],
    next_id: u32,
}

impl WebGLContext {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            projection_matrix: Matrix4x4::identity(),
            model_view_matrix: Matrix4x4::identity(),
            vertex_buffer: Vec::new(),
            pixel_buffer: PixelBuffer::new(width, height),
            viewport: Viewport { x: 0, y: 0, width, height },
            programs: Vec::new(),
            textures: Vec::new(),
            framebuffers: Vec::new(),
            index_buffers: Vec::new(),
            active_texture_unit: 0,
            current_program_id: None,
            current_framebuffer_id: None,
            depth_test_enabled: false,
            blend_enabled: false,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn viewport(&mut self, x: i32, y: i32, width: usize, height: usize) {
        self.viewport = Viewport { x, y, width, height };
    }

    pub fn clear(&mut self, r: f32, g: f32, b: f32, a: f32) {
        let ri = (r * 255.0) as u8;
        let gi = (g * 255.0) as u8;
        let bi = (b * 255.0) as u8;
        let ai = (a * 255.0) as u8;
        for y in 0..self.height {
            for x in 0..self.width {
                self.pixel_buffer.set_pixel(x, y, ri, gi, bi, ai);
            }
        }
    }

    // -- Shader programs --

    pub fn create_program(&mut self, vertex_src: &str, fragment_src: &str) -> u32 {
        let id = self.alloc_id();
        self.programs.push(ShaderProgram {
            id,
            vertex_source: vertex_src.to_string(),
            fragment_source: fragment_src.to_string(),
            uniforms: std::collections::HashMap::new(),
            attributes: std::collections::HashMap::new(),
            linked: true,
        });
        id
    }

    pub fn use_program(&mut self, program_id: Option<u32>) {
        self.current_program_id = program_id;
    }

    pub fn set_uniform(&mut self, name: &str, value: ShaderUniform) {
        if let Some(prog_id) = self.current_program_id {
            if let Some(prog) = self.programs.iter_mut().find(|p| p.id == prog_id) {
                prog.uniforms.insert(name.to_string(), value);
            }
        }
    }

    pub fn get_uniform_location(&self, name: &str) -> Option<usize> {
        if let Some(prog_id) = self.current_program_id {
            if let Some(prog) = self.programs.iter().find(|p| p.id == prog_id) {
                return prog.uniforms.keys().position(|k| k == name);
            }
        }
        None
    }

    // -- Textures --

    pub fn create_texture(&mut self, width: usize, height: usize, format: TextureFormat, data: &[u8]) -> u32 {
        let id = self.alloc_id();
        self.textures.push(Texture2D {
            id, width, height, format,
            data: data.to_vec(),
            min_filter: TextureFilter::Linear,
            mag_filter: TextureFilter::Linear,
            wrap_s: TextureWrap::ClampToEdge,
            wrap_t: TextureWrap::ClampToEdge,
        });
        id
    }

    pub fn active_texture(&mut self, unit: u32) {
        self.active_texture_unit = unit;
    }

    pub fn bind_texture(&self, _texture_id: u32) {
        // Binding is tracked via active_texture_unit; in a real GPU this would
        // bind to the GL texture object. Here it's a no-op state setter.
    }

    pub fn set_texture_params(&mut self, texture_id: u32, min: TextureFilter, mag: TextureFilter, wrap_s: TextureWrap, wrap_t: TextureWrap) {
        if let Some(tex) = self.textures.iter_mut().find(|t| t.id == texture_id) {
            tex.min_filter = min;
            tex.mag_filter = mag;
            tex.wrap_s = wrap_s;
            tex.wrap_t = wrap_t;
        }
    }

    pub fn sample_texture(&self, texture_id: u32, u: f32, v: f32) -> [u8; 4] {
        if let Some(tex) = self.textures.iter().find(|t| t.id == texture_id) {
            let bpp = tex.format.bytes_per_pixel();
            let wrap_u = match tex.wrap_s {
                TextureWrap::Repeat => ((u * tex.width as f32) % tex.width as f32) as usize,
                TextureWrap::ClampToEdge => (u * (tex.width - 1) as f32).clamp(0.0, (tex.width - 1) as f32) as usize,
                TextureWrap::MirroredRepeat => {
                    let t = (u * tex.width as f32) % (2.0 * tex.width as f32);
                    if t < tex.width as f32 { t as usize } else { 2 * tex.width - 1 - t as usize }
                }
            };
            let wrap_v = match tex.wrap_t {
                TextureWrap::Repeat => ((v * tex.height as f32) % tex.height as f32) as usize,
                TextureWrap::ClampToEdge => (v * (tex.height - 1) as f32).clamp(0.0, (tex.height - 1) as f32) as usize,
                TextureWrap::MirroredRepeat => {
                    let t = (v * tex.height as f32) % (2.0 * tex.height as f32);
                    if t < tex.height as f32 { t as usize } else { 2 * tex.height - 1 - t as usize }
                }
            };
            let offset = (wrap_v * tex.width + wrap_u) * bpp;
            if offset + bpp <= tex.data.len() {
                match tex.format {
                    TextureFormat::RGBA8 => [tex.data[offset], tex.data[offset+1], tex.data[offset+2], tex.data[offset+3]],
                    TextureFormat::RGB8 => [tex.data[offset], tex.data[offset+1], tex.data[offset+2], 255],
                    TextureFormat::Alpha8 => [255, 255, 255, tex.data[offset]],
                    _ => [0, 0, 0, 255],
                }
            } else {
                [0, 0, 0, 255]
            }
        } else {
            [0, 0, 0, 255]
        }
    }

    // -- Framebuffers --

    pub fn create_framebuffer(&mut self, width: usize, height: usize, with_depth: bool) -> u32 {
        let id = self.alloc_id();
        let color_tex_id = self.create_texture(width, height, TextureFormat::RGBA8, &vec![0u8; width * height * 4]);
        self.framebuffers.push(Framebuffer {
            id,
            color_texture_id: Some(color_tex_id),
            depth_renderbuffer: with_depth,
            width,
            height,
            complete: true,
        });
        id
    }

    pub fn bind_framebuffer(&mut self, framebuffer_id: Option<u32>) {
        self.current_framebuffer_id = framebuffer_id;
    }

    pub fn check_framebuffer_status(&self, framebuffer_id: u32) -> bool {
        self.framebuffers.iter().any(|fb| fb.id == framebuffer_id && fb.complete)
    }

    // -- Index buffers --

    pub fn create_index_buffer(&mut self, indices: &[u32]) -> u32 {
        let id = self.alloc_id();
        let element_type = if indices.iter().all(|&i| i <= u16::MAX as u32) {
            IndexType::UnsignedShort
        } else {
            IndexType::UnsignedInt
        };
        self.index_buffers.push(IndexBuffer {
            id,
            indices: indices.to_vec(),
            element_type,
        });
        id
    }

    // -- Draw calls --

    pub fn buffer_data(&mut self, vertices: &[f32]) {
        self.vertex_buffer = vertices.to_vec();
    }

    pub fn draw_arrays_triangles(&mut self, r: u8, g: u8, b: u8, a: u8) {
        let mut idx = 0;
        while idx + 5 < self.vertex_buffer.len() {
            let x = self.vertex_buffer[idx].max(0.0) as usize;
            let y = self.vertex_buffer[idx + 1].max(0.0) as usize;
            self.pixel_buffer.set_pixel(x, y, r, g, b, a);
            idx += 2;
        }
    }

    pub fn draw_elements(&mut self, index_buffer_id: u32, r: u8, g: u8, b: u8, a: u8) {
        if let Some(ib) = self.index_buffers.iter().find(|b| b.id == index_buffer_id) {
            for &index in &ib.indices {
                let base = (index as usize) * 2;
                if base + 1 < self.vertex_buffer.len() {
                    let x = self.vertex_buffer[base].max(0.0) as usize;
                    let y = self.vertex_buffer[base + 1].max(0.0) as usize;
                    self.pixel_buffer.set_pixel(x, y, r, g, b, a);
                }
            }
        }
    }

    pub fn draw_textured_quad(&mut self, texture_id: u32, x: usize, y: usize, w: usize, h: usize) {
        for dy in 0..h {
            for dx in 0..w {
                let u = dx as f32 / w as f32;
                let v = dy as f32 / h as f32;
                let [r, g, b, a] = self.sample_texture(texture_id, u, v);
                let px = x + dx;
                let py = y + dy;
                if px < self.width && py < self.height {
                    self.pixel_buffer.set_pixel(px, py, r, g, b, a);
                }
            }
        }
    }
}
