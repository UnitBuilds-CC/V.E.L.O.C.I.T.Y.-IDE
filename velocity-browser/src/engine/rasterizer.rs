use crate::layout::LayoutBox;
use crate::nda::NdaTriple;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct PixelBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>, // RGBA 32-bit pixel data
}

impl PixelBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0xFFFFFFFF; width * height], // White background default
        }
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for row in y..(y + h).min(self.height) {
            for col in x..(x + w).min(self.width) {
                let idx = row * self.width + col;
                if idx < self.pixels.len() {
                    self.pixels[idx] = color;
                }
            }
        }
    }

    pub fn compute_pixel_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.pixels.hash(&mut hasher);
        hasher.finish()
    }
}

pub struct SoftwareRasterizer;

impl SoftwareRasterizer {
    pub fn render_layout(boxes: &[LayoutBox], width: usize, height: usize) -> PixelBuffer {
        let mut buffer = PixelBuffer::new(width, height);
        for b in boxes {
            if b.is_visible {
                let bx = (b.x as usize).min(width);
                let by = (b.y as usize).min(height);
                let bw = (b.width as usize).min(width - bx);
                let bh = (b.height as usize).min(height - by);
                buffer.draw_rect(bx, by, bw, bh, 0xFF0000FF); // Blue elements
            }
        }
        buffer
    }

    pub fn raster_to_nda(buffer: &PixelBuffer, session_id: &str) -> Vec<NdaTriple> {
        let hash = buffer.compute_pixel_hash();
        vec![
            NdaTriple::new(session_id, 80, &format!("{}x{}", buffer.width, buffer.height)),
            NdaTriple::new(session_id, 81, &hash.to_string()),
        ]
    }
}
