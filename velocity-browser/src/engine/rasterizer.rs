#[derive(Debug, Clone)]
pub struct PixelBuffer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl PixelBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![255; width * height * 4],
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, a: u8) {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) * 4;
            self.data[idx] = r;
            self.data[idx + 1] = g;
            self.data[idx + 2] = b;
            self.data[idx + 3] = a;
        }
    }

    pub fn get_pixel(&self, x: usize, y: usize) -> [u8; 4] {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) * 4;
            [self.data[idx], self.data[idx + 1], self.data[idx + 2], self.data[idx + 3]]
        } else {
            [255, 255, 255, 255]
        }
    }

    pub fn compute_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.data.hash(&mut hasher);
        hasher.finish()
    }
}

pub struct SoftwareRasterizer;

impl SoftwareRasterizer {
    pub fn render_blank(width: usize, height: usize) -> PixelBuffer {
        PixelBuffer::new(width, height)
    }
}
