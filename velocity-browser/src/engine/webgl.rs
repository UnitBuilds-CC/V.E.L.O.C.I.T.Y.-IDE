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
}

pub struct WebGLContext {
    pub width: usize,
    pub height: usize,
    pub projection_matrix: Matrix4x4,
    pub vertex_buffer: Vec<f32>,
    pub pixel_buffer: PixelBuffer,
}

impl WebGLContext {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            projection_matrix: Matrix4x4::identity(),
            vertex_buffer: Vec::new(),
            pixel_buffer: PixelBuffer::new(width, height),
        }
    }

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
}
