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
                let color = if (cx + cy).is_multiple_of(2) { c1 } else { c2 };
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

    /// Render a horizontal linear gradient.
    pub fn render_gradient_h(width: usize, height: usize, left: [u8; 4], right: [u8; 4]) -> PixelBuffer {
        let mut buf = PixelBuffer::new(width, height);
        for x in 0..width {
            let t = if width > 1 { x as f32 / (width - 1) as f32 } else { 0.0 };
            let r = (left[0] as f32 * (1.0 - t) + right[0] as f32 * t) as u8;
            let g = (left[1] as f32 * (1.0 - t) + right[1] as f32 * t) as u8;
            let b = (left[2] as f32 * (1.0 - t) + right[2] as f32 * t) as u8;
            let a = (left[3] as f32 * (1.0 - t) + right[3] as f32 * t) as u8;
            for y in 0..height {
                buf.set_pixel(x, y, r, g, b, a);
            }
        }
        buf
    }

    /// Draw an ellipse outline on a PixelBuffer using the midpoint ellipse algorithm.
    pub fn stroke_ellipse(buf: &mut PixelBuffer, cx: i32, cy: i32, rx: i32, ry: i32, r: u8, g: u8, b: u8, a: u8) {
        if rx <= 0 || ry <= 0 { return; }
        let mut x = 0i32;
        let mut y = ry;
        let rx2 = rx * rx;
        let ry2 = ry * ry;
        // Region 1
        let mut p = (ry2 as f32) - (rx2 as f32) * (ry as f32) + 0.25 * (rx2 as f32);
        while ry2 * x < rx2 * y {
            for &(px, py) in &[(cx+x,cy+y),(cx-x,cy+y),(cx+x,cy-y),(cx-x,cy-y)] {
                if px >= 0 && py >= 0 { buf.set_pixel(px as usize, py as usize, r, g, b, a); }
            }
            x += 1;
            if p < 0.0 {
                p += 2.0 * (ry2 as f32) * (x as f32) + (ry2 as f32);
            } else {
                y -= 1;
                p += 2.0 * (ry2 as f32) * (x as f32) - 2.0 * (rx2 as f32) * (y as f32) + (ry2 as f32);
            }
        }
        // Region 2
        let mut p2 = (ry2 as f32) * ((x as f32) + 0.5).powi(2) + (rx2 as f32) * ((y as f32) - 1.0).powi(2) - (rx2 as f32) * (ry2 as f32);
        while y >= 0 {
            for &(px, py) in &[(cx+x,cy+y),(cx-x,cy+y),(cx+x,cy-y),(cx-x,cy-y)] {
                if px >= 0 && py >= 0 { buf.set_pixel(px as usize, py as usize, r, g, b, a); }
            }
            y -= 1;
            if p2 > 0.0 {
                p2 -= 2.0 * (rx2 as f32) * (y as f32) + (rx2 as f32);
            } else {
                x += 1;
                p2 += 2.0 * (ry2 as f32) * (x as f32) - 2.0 * (rx2 as f32) * (y as f32) + (rx2 as f32);
            }
        }
    }

    /// Copy a rectangular region within the same buffer.
    pub fn copy_within(buf: &mut PixelBuffer, src_x: usize, src_y: usize, dst_x: usize, dst_y: usize, w: usize, h: usize) {
        let tmp = buf.crop(src_x, src_y, w, h);
        buf.blit(&tmp, dst_x, dst_y, 0, 0, w, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_white() {
        let buf = PixelBuffer::new(4, 4);
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(3, 3), [255, 255, 255, 255]);
    }

    #[test]
    fn test_set_get_pixel() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.set_pixel(3, 4, 255, 0, 0, 255);
        assert_eq!(buf.get_pixel(3, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn test_out_of_bounds_returns_white() {
        let buf = PixelBuffer::new(4, 4);
        assert_eq!(buf.get_pixel(10, 10), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(100, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn test_out_of_bounds_set_is_noop() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.set_pixel(10, 10, 0, 0, 0, 0);
        assert_eq!(buf.get_pixel(3, 3), [255, 255, 255, 255]);
    }

    #[test]
    fn test_clear() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.clear(0, 128, 255, 200);
        assert_eq!(buf.get_pixel(0, 0), [0, 128, 255, 200]);
        assert_eq!(buf.get_pixel(3, 3), [0, 128, 255, 200]);
    }

    #[test]
    fn test_fill_rect() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.fill_rect(2, 2, 3, 3, 255, 0, 0, 255);
        assert_eq!(buf.get_pixel(2, 2), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(4, 4), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(5, 5), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(1, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn test_stroke_rect_corners() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.stroke_rect(1, 1, 4, 4, 0, 0, 0, 255);
        assert_eq!(buf.get_pixel(1, 1), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(4, 1), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(1, 4), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(4, 4), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(2, 2), [255, 255, 255, 255]);
    }

    #[test]
    fn test_draw_line_horizontal() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.draw_line(0, 3, 5, 3, 0, 0, 0, 255);
        for x in 0..=5 {
            assert_eq!(buf.get_pixel(x, 3), [0, 0, 0, 255]);
        }
    }

    #[test]
    fn test_draw_line_vertical() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.draw_line(2, 0, 2, 5, 0, 0, 0, 255);
        for y in 0..=5 {
            assert_eq!(buf.get_pixel(2, y), [0, 0, 0, 255]);
        }
    }

    #[test]
    fn test_fill_circle_center() {
        let mut buf = PixelBuffer::new(16, 16);
        buf.fill_circle(8, 8, 3, 255, 0, 0, 255);
        assert_eq!(buf.get_pixel(8, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn test_stroke_circle_points() {
        let mut buf = PixelBuffer::new(20, 20);
        buf.stroke_circle(10, 10, 5, 0, 0, 0, 255);
        assert_eq!(buf.get_pixel(15, 10), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(5, 10), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(10, 5), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(10, 15), [0, 0, 0, 255]);
    }

    #[test]
    fn test_blend_pixel_opaque() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.clear(0, 0, 0, 255);
        buf.blend_pixel(0, 0, 255, 0, 0, 255);
        assert_eq!(buf.get_pixel(0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn test_blend_pixel_semi_transparent() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.clear(0, 0, 0, 255);
        buf.blend_pixel(0, 0, 255, 255, 255, 128);
        let px = buf.get_pixel(0, 0);
        assert!(px[0] > 100 && px[0] < 200);
    }

    #[test]
    fn test_blit() {
        let mut src = PixelBuffer::new(4, 4);
        src.clear(0, 0, 0, 0);
        src.set_pixel(0, 0, 255, 0, 0, 255);
        let mut dst = PixelBuffer::new(8, 8);
        dst.blit(&src, 2, 2, 0, 0, 4, 4);
        assert_eq!(dst.get_pixel(2, 2), [255, 0, 0, 255]);
    }

    #[test]
    fn test_crop() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.clear(255, 255, 255, 255);
        buf.set_pixel(3, 3, 0, 0, 0, 255);
        let cropped = buf.crop(2, 2, 4, 4);
        assert_eq!(cropped.width, 4);
        assert_eq!(cropped.height, 4);
        assert_eq!(cropped.get_pixel(1, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn test_compute_hash_differs() {
        let buf1 = PixelBuffer::new(4, 4);
        let mut buf2 = PixelBuffer::new(4, 4);
        buf2.set_pixel(0, 0, 0, 0, 0, 0);
        assert_ne!(buf1.compute_hash(), buf2.compute_hash());
    }

    #[test]
    fn test_render_blank() {
        let buf = SoftwareRasterizer::render_blank(10, 10);
        assert_eq!(buf.width, 10);
        assert_eq!(buf.height, 10);
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn test_render_checkerboard() {
        let buf = SoftwareRasterizer::render_checkerboard(4, 4, 2, [0, 0, 0, 255], [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(2, 0), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(0, 2), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(2, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn test_render_gradient_v() {
        let buf = SoftwareRasterizer::render_gradient_v(4, 3, [0, 0, 0, 255], [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(0, 2), [255, 255, 255, 255]);
    }

    #[test]
    fn test_render_gradient_h() {
        let buf = SoftwareRasterizer::render_gradient_h(3, 4, [0, 0, 0, 255], [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(2, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn test_stroke_ellipse() {
        let mut buf = PixelBuffer::new(20, 20);
        SoftwareRasterizer::stroke_ellipse(&mut buf, 10, 10, 5, 3, 0, 0, 0, 255);
        assert_eq!(buf.get_pixel(15, 10), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(5, 10), [0, 0, 0, 255]);
    }

    #[test]
    fn test_copy_within() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.clear(255, 255, 255, 255);
        buf.set_pixel(0, 0, 255, 0, 0, 255);
        SoftwareRasterizer::copy_within(&mut buf, 0, 0, 4, 4, 1, 1);
        assert_eq!(buf.get_pixel(4, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn draw_line_diagonal() {
        let mut buf = PixelBuffer::new(10, 10);
        buf.draw_line(0, 0, 5, 5, 0, 0, 0, 255);
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(buf.get_pixel(5, 5), [0, 0, 0, 255]);
        // Some intermediate pixels should be set
        assert_eq!(buf.get_pixel(2, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn fill_rect_clamped_to_buffer() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.fill_rect(5, 5, 100, 100, 255, 0, 0, 255);
        assert_eq!(buf.get_pixel(5, 5), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(7, 7), [255, 0, 0, 255]);
        // Outside the buffer is still white
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn stroke_ellipse_zero_radius_noop() {
        let mut buf = PixelBuffer::new(10, 10);
        SoftwareRasterizer::stroke_ellipse(&mut buf, 5, 5, 0, 0, 0, 0, 0, 255);
        // Should not panic, and no pixels should be set
        assert_eq!(buf.get_pixel(5, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn blend_pixel_out_of_bounds_noop() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.clear(0, 0, 0, 255);
        buf.blend_pixel(100, 100, 255, 255, 255, 128);
        // No panic, original pixels unchanged
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn crop_beyond_buffer_boundary() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.clear(100, 200, 50, 255);
        let cropped = buf.crop(2, 2, 10, 10);
        assert_eq!(cropped.width, 10);
        assert_eq!(cropped.height, 10);
        // Pixels within source bounds should be copied
        assert_eq!(cropped.get_pixel(0, 0), [100, 200, 50, 255]);
        // Pixels beyond source bounds should be white (default)
        assert_eq!(cropped.get_pixel(5, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn render_gradient_v_single_row() {
        let buf = SoftwareRasterizer::render_gradient_v(4, 1, [0, 0, 0, 255], [255, 255, 255, 255]);
        // Single row should be the top color (t=0)
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn render_gradient_h_single_col() {
        let buf = SoftwareRasterizer::render_gradient_h(1, 4, [0, 0, 0, 255], [255, 255, 255, 255]);
        // Single column should be the left color (t=0)
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn compute_hash_deterministic() {
        let mut buf = PixelBuffer::new(4, 4);
        buf.set_pixel(0, 0, 1, 2, 3, 4);
        let h1 = buf.compute_hash();
        let h2 = buf.compute_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn fill_circle_boundary_pixels() {
        let mut buf = PixelBuffer::new(20, 20);
        buf.fill_circle(10, 10, 5, 0, 0, 0, 255);
        // Center should be filled
        assert_eq!(buf.get_pixel(10, 10), [0, 0, 0, 255]);
        // Edge should be filled
        assert_eq!(buf.get_pixel(15, 10), [0, 0, 0, 255]);
        // Far outside should not be filled
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn stroke_rect_interior_is_empty() {
        let mut buf = PixelBuffer::new(10, 10);
        buf.stroke_rect(2, 2, 6, 6, 0, 0, 0, 255);
        // Interior should be untouched (white)
        assert_eq!(buf.get_pixel(4, 4), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(5, 5), [255, 255, 255, 255]);
    }
}
