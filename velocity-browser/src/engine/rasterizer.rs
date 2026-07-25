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

    /// Alpha-blend a pixel over the existing content (source-over compositing).
    pub fn blend_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height { return; }
        let idx = (y * self.width + x) * 4;
        let src_a = a as f32 / 255.0;
        let inv_a = 1.0 - src_a;
        self.data[idx]     = (r as f32 * src_a + self.data[idx] as f32 * inv_a) as u8;
        self.data[idx + 1] = (g as f32 * src_a + self.data[idx + 1] as f32 * inv_a) as u8;
        self.data[idx + 2] = (b as f32 * src_a + self.data[idx + 2] as f32 * inv_a) as u8;
        self.data[idx + 3] = (a as f32 + self.data[idx + 3] as f32 * inv_a).min(255.0) as u8;
    }

    /// Fill the entire buffer with a solid color.
    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_pixel(x, y, r, g, b, a);
            }
        }
    }

    /// Fill a rectangle with a solid color.
    pub fn fill_rect(&mut self, x0: usize, y0: usize, w: usize, h: usize, r: u8, g: u8, b: u8, a: u8) {
        let x1 = (x0 + w).min(self.width);
        let y1 = (y0 + h).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                self.set_pixel(x, y, r, g, b, a);
            }
        }
    }

    /// Draw a rectangle outline.
    pub fn stroke_rect(&mut self, x0: usize, y0: usize, w: usize, h: usize, r: u8, g: u8, b: u8, a: u8) {
        let x1 = (x0 + w).min(self.width);
        let y1 = (y0 + h).min(self.height);
        // Top and bottom edges
        for x in x0..x1 {
            self.set_pixel(x, y0, r, g, b, a);
            if y1 > 0 { self.set_pixel(x, y1 - 1, r, g, b, a); }
        }
        // Left and right edges
        for y in y0..y1 {
            self.set_pixel(x0, y, r, g, b, a);
            if x1 > 0 { self.set_pixel(x1 - 1, y, r, g, b, a); }
        }
    }

    /// Draw a line using Bresenham's algorithm.
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, r: u8, g: u8, b: u8, a: u8) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut cx = x0;
        let mut cy = y0;
        loop {
            if cx >= 0 && cy >= 0 {
                self.set_pixel(cx as usize, cy as usize, r, g, b, a);
            }
            if cx == x1 && cy == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy {
                if cx == x1 { break; }
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                if cy == y1 { break; }
                err += dx;
                cy += sy;
            }
        }
    }

    /// Fill a circle using the midpoint circle algorithm.
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, r: u8, g: u8, b: u8, a: u8) {
        let mut x = radius;
        let mut y = 0i32;
        let mut d = 1 - radius;
        while x >= y {
            // Fill horizontal spans for each octant pair
            for dx in -x..=x {
                let px = cx + dx;
                let py_up = cy + y;
                let py_dn = cy - y;
                if px >= 0 && py_up >= 0 { self.set_pixel(px as usize, py_up as usize, r, g, b, a); }
                if px >= 0 && py_dn >= 0 { self.set_pixel(px as usize, py_dn as usize, r, g, b, a); }
            }
            for dx in -y..=y {
                let px = cx + dx;
                let py_up = cy + x;
                let py_dn = cy - x;
                if px >= 0 && py_up >= 0 { self.set_pixel(px as usize, py_up as usize, r, g, b, a); }
                if px >= 0 && py_dn >= 0 { self.set_pixel(px as usize, py_dn as usize, r, g, b, a); }
            }
            y += 1;
            if d < 0 {
                d += 2 * y + 1;
            } else {
                x -= 1;
                d += 2 * (y - x) + 1;
            }
        }
    }

    /// Draw a circle outline.
    pub fn stroke_circle(&mut self, cx: i32, cy: i32, radius: i32, r: u8, g: u8, b: u8, a: u8) {
        let mut x = radius;
        let mut y = 0i32;
        let mut d = 1 - radius;
        while x >= y {
            for &(px, py) in &[
                (cx+x, cy+y), (cx-x, cy+y), (cx+x, cy-y), (cx-x, cy-y),
                (cx+y, cy+x), (cx-y, cy+x), (cx+y, cy-x), (cx-y, cy-x),
            ] {
                if px >= 0 && py >= 0 {
                    self.set_pixel(px as usize, py as usize, r, g, b, a);
                }
            }
            y += 1;
            if d < 0 {
                d += 2 * y + 1;
            } else {
                x -= 1;
                d += 2 * (y - x) + 1;
            }
        }
    }

    /// Copy a rectangular region from another PixelBuffer.
    pub fn blit(&mut self, src: &PixelBuffer, dst_x: usize, dst_y: usize, src_x: usize, src_y: usize, w: usize, h: usize) {
        for dy in 0..h {
            for dx in 0..w {
                let sx = src_x + dx;
                let sy = src_y + dy;
                let ddx = dst_x + dx;
                let ddy = dst_y + dy;
                if sx < src.width && sy < src.height && ddx < self.width && ddy < self.height {
                    let px = src.get_pixel(sx, sy);
                    self.set_pixel(ddx, ddy, px[0], px[1], px[2], px[3]);
                }
            }
        }
    }

    /// Create a sub-image (crop) from a rectangular region.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> PixelBuffer {
        let mut result = PixelBuffer::new(w, h);
        for dy in 0..h {
            for dx in 0..w {
                let sx = x + dx;
                let sy = y + dy;
                if sx < self.width && sy < self.height {
                    let px = self.get_pixel(sx, sy);
                    result.set_pixel(dx, dy, px[0], px[1], px[2], px[3]);
                }
            }
        }
        result
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

    /// Render a checkerboard pattern (useful as a placeholder background).
    pub fn render_checkerboard(width: usize, height: usize, cell_size: usize, c1: [u8; 4], c2: [u8; 4]) -> PixelBuffer {
        let mut buf = PixelBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let cx = x / cell_size;
                let cy = y / cell_size;
                let color = if (cx + cy) % 2 == 0 { c1 } else { c2 };
                buf.set_pixel(x, y, color[0], color[1], color[2], color[3]);
            }
        }
        buf
    }

    /// Render a vertical linear gradient.
    pub fn render_gradient_v(width: usize, height: usize, top: [u8; 4], bottom: [u8; 4]) -> PixelBuffer {
        let mut buf = PixelBuffer::new(width, height);
        for y in 0..height {
            let t = if height > 1 { y as f32 / (height - 1) as f32 } else { 0.0 };
            let r = (top[0] as f32 * (1.0 - t) + bottom[0] as f32 * t) as u8;
            let g = (top[1] as f32 * (1.0 - t) + bottom[1] as f32 * t) as u8;
            let b = (top[2] as f32 * (1.0 - t) + bottom[2] as f32 * t) as u8;
            let a = (top[3] as f32 * (1.0 - t) + bottom[3] as f32 * t) as u8;
            for x in 0..width {
                buf.set_pixel(x, y, r, g, b, a);
            }
        }
        buf
    }
}
