pub struct PixelBuffer {
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u8>,
}

impl PixelBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            buffer: vec![255; width * height * 4], // 32-bit RGBA
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, a: u8) {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) * 4;
            self.buffer[idx] = r;
            self.buffer[idx + 1] = g;
            self.buffer[idx + 2] = b;
            self.buffer[idx + 3] = a;
        }
    }

    pub fn compute_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.buffer.hash(&mut hasher);
        hasher.finish()
    }
}

pub struct SoftwareRasterizer;

impl SoftwareRasterizer {
    pub fn rasterize_layout_boxes(boxes: &[crate::layout::LayoutBox], width: usize, height: usize) -> PixelBuffer {
        let mut buffer = PixelBuffer::new(width, height);
        for b in boxes {
            let x_start = b.x.max(0.0) as usize;
            let y_start = b.y.max(0.0) as usize;
            let x_end = (b.x + b.width).max(0.0) as usize;
            let y_end = (b.y + b.height).max(0.0) as usize;

            for y in y_start..y_end.min(height) {
                for x in x_start..x_end.min(width) {
                    buffer.set_pixel(x, y, 200, 200, 200, 255);
                }
            }
        }
        buffer
    }
}
