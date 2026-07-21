use crate::engine::PixelBuffer;

pub struct Canvas2DContext {
    pub width: usize,
    pub height: usize,
    pub pixel_buffer: PixelBuffer,
}

impl Canvas2DContext {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixel_buffer: PixelBuffer::new(width, height),
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: u8, g: u8, b: u8, a: u8) {
        for curr_y in y..(y + h).min(self.height) {
            for curr_x in x..(x + w).min(self.width) {
                self.pixel_buffer.set_pixel(curr_x, curr_y, r, g, b, a);
            }
        }
    }

    pub fn clear_rect(&mut self, x: usize, y: usize, w: usize, h: usize) {
        self.fill_rect(x, y, w, h, 0, 0, 0, 0);
    }
}
