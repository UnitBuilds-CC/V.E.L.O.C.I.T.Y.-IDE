use crate::engine::PixelBuffer;

/// A single recorded 2D drawing operation.
///
/// The display list is the agent's window into *what* was drawn - text,
/// shapes, images - without rasterizing to pixels and guessing via OCR. Text
/// content is preserved verbatim so the agent can read canvas-rendered labels
/// (the whole point of an agent-first browser).
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    ClearRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    FillRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        style: String,
    },
    StrokeRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        style: String,
    },
    FillText {
        text: String,
        x: f64,
        y: f64,
        font: String,
        style: String,
    },
    StrokeText {
        text: String,
        x: f64,
        y: f64,
        font: String,
        style: String,
    },
    DrawImage {
        src: String,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    },
    BeginPath,
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    Arc {
        x: f64,
        y: f64,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    ClosePath,
    Fill {
        style: String,
    },
    Stroke {
        style: String,
    },
}

/// A 2D canvas context that records every drawing operation into a semantic
/// display list *and* rasterizes rectangles into a pixel buffer (for callers
/// that still want pixels). The display list is authoritative for agents.
#[derive(Debug, Clone)]
pub struct Canvas2DContext {
    pub width: usize,
    pub height: usize,
    pub pixel_buffer: PixelBuffer,
    pub display_list: Vec<DrawCommand>,
    pub fill_style: String,
    pub stroke_style: String,
    pub font: String,
}

impl Canvas2DContext {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixel_buffer: PixelBuffer::new(width, height),
            display_list: Vec::new(),
            fill_style: "#000000".to_string(),
            stroke_style: "#000000".to_string(),
            font: "10px sans-serif".to_string(),
        }
    }

    /// Number of recorded draw operations.
    pub fn draw_call_count(&self) -> u64 {
        self.display_list.len() as u64
    }

    pub fn set_fill_style(&mut self, style: &str) {
        self.fill_style = style.to_string();
    }

    pub fn set_stroke_style(&mut self, style: &str) {
        self.stroke_style = style.to_string();
    }

    pub fn set_font(&mut self, font: &str) {
        self.font = font.to_string();
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let (r, g, b, a) = parse_css_color(&self.fill_style);
        self.rasterize_rect(x, y, w, h, r, g, b, a);
        self.display_list.push(DrawCommand::FillRect {
            x,
            y,
            w,
            h,
            style: self.fill_style.clone(),
        });
    }

    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let (r, g, b, a) = parse_css_color(&self.stroke_style);
        // Rasterize the 4 edges of the rectangle outline (1px stroke width)
        let x0 = x.max(0.0) as usize;
        let y0 = y.max(0.0) as usize;
        let x1 = ((x + w).max(0.0) as usize).min(self.width);
        let y1 = ((y + h).max(0.0) as usize).min(self.height);
        // Top edge
        if y0 < self.height {
            for cx in x0..x1 {
                self.pixel_buffer.set_pixel(cx, y0, r, g, b, a);
            }
        }
        // Bottom edge
        if y1 > 0 && y1 <= self.height {
            let by = (y1 - 1).min(self.height - 1);
            for cx in x0..x1 {
                self.pixel_buffer.set_pixel(cx, by, r, g, b, a);
            }
        }
        // Left edge
        if x0 < self.width {
            for cy in y0..y1 {
                self.pixel_buffer.set_pixel(x0, cy, r, g, b, a);
            }
        }
        // Right edge
        if x1 > 0 && x1 <= self.width {
            let rx = (x1 - 1).min(self.width - 1);
            for cy in y0..y1 {
                self.pixel_buffer.set_pixel(rx, cy, r, g, b, a);
            }
        }
        self.display_list.push(DrawCommand::StrokeRect {
            x,
            y,
            w,
            h,
            style: self.stroke_style.clone(),
        });
    }

    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.rasterize_rect(x, y, w, h, 0, 0, 0, 0);
        self.display_list
            .push(DrawCommand::ClearRect { x, y, w, h });
    }

    pub fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.display_list.push(DrawCommand::FillText {
            text: text.to_string(),
            x,
            y,
            font: self.font.clone(),
            style: self.fill_style.clone(),
        });
    }

    pub fn stroke_text(&mut self, text: &str, x: f64, y: f64) {
        self.display_list.push(DrawCommand::StrokeText {
            text: text.to_string(),
            x,
            y,
            font: self.font.clone(),
            style: self.stroke_style.clone(),
        });
    }

    pub fn draw_image(&mut self, src: &str, dx: f64, dy: f64, dw: f64, dh: f64) {
        self.display_list.push(DrawCommand::DrawImage {
            src: src.to_string(),
            dx,
            dy,
            dw,
            dh,
        });
    }

    pub fn begin_path(&mut self) {
        self.display_list.push(DrawCommand::BeginPath);
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.display_list.push(DrawCommand::MoveTo { x, y });
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.display_list.push(DrawCommand::LineTo { x, y });
    }

    pub fn arc(&mut self, x: f64, y: f64, radius: f64, start_angle: f64, end_angle: f64) {
        self.display_list.push(DrawCommand::Arc {
            x,
            y,
            radius,
            start_angle,
            end_angle,
        });
    }

    pub fn close_path(&mut self) {
        self.display_list.push(DrawCommand::ClosePath);
    }

    pub fn fill(&mut self) {
        self.display_list.push(DrawCommand::Fill {
            style: self.fill_style.clone(),
        });
    }

    pub fn stroke(&mut self) {
        self.display_list.push(DrawCommand::Stroke {
            style: self.stroke_style.clone(),
        });
    }

    /// All text (fill + stroke) drawn to the canvas, in draw order.
    pub fn drawn_text(&self) -> Vec<&str> {
        self.display_list
            .iter()
            .filter_map(|c| match c {
                DrawCommand::FillText { text, .. } | DrawCommand::StrokeText { text, .. } => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// Export pixel buffer as a data URL (image/png in base64).
    /// This produces a minimal uncompressed BMP-like encoding wrapped in base64
    /// since we don't have a PNG encoder, but matches the toDataURL() API shape.
    pub fn to_data_url(&self, mime_type: &str) -> String {
        let _mime = if mime_type.is_empty() {
            "image/png"
        } else {
            mime_type
        };
        // Encode as uncompressed BMP and then base64
        let bmp = self.encode_bmp();
        let b64 = base64_encode(&bmp);
        format!("data:image/bmp;base64,{}", b64)
    }

    /// Get raw pixel data as RGBA byte slice (for getImageData).
    pub fn get_image_data(&self, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(w * h * 4);
        for row in y..(y + h).min(self.height) {
            for col in x..(x + w).min(self.width) {
                let px = self.pixel_buffer.get_pixel(col, row);
                data.push(px[0]);
                data.push(px[1]);
                data.push(px[2]);
                data.push(px[3]);
            }
        }
        data
    }

    /// Encode pixel buffer as a BMP file (24-bit, no compression).
    fn encode_bmp(&self) -> Vec<u8> {
        let w = self.width;
        let h = self.height;
        let row_size = (w * 3).div_ceil(4) * 4; // rows padded to 4-byte boundary
        let pixel_data_size = row_size * h;
        let file_size = 54 + pixel_data_size;

        let mut bmp = Vec::with_capacity(file_size);
        // BMP header
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
        bmp.extend_from_slice(&[0u8; 4]); // reserved
        bmp.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
                                                     // DIB header (BITMAPINFOHEADER)
        bmp.extend_from_slice(&40u32.to_le_bytes()); // header size
        bmp.extend_from_slice(&(w as i32).to_le_bytes());
        bmp.extend_from_slice(&(h as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes()); // planes
        bmp.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
        bmp.extend_from_slice(&[0u8; 24]); // compression through to color table (all zeros)
                                           // Pixel data (bottom-up row order for BMP)
        for row in (0..h).rev() {
            for col in 0..w {
                let px = self.pixel_buffer.get_pixel(col, row);
                bmp.push(px[2]); // BMP stores BGR
                bmp.push(px[1]);
                bmp.push(px[0]);
            }
            // Pad row to 4-byte boundary
            let padding = row_size - (w * 3);
            for _ in 0..padding {
                bmp.push(0);
            }
        }
        bmp
    }

    fn rasterize_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: u8, g: u8, b: u8, a: u8) {
        let x0 = x.max(0.0) as usize;
        let y0 = y.max(0.0) as usize;
        let x1 = ((x + w).max(0.0) as usize).min(self.width);
        let y1 = ((y + h).max(0.0) as usize).min(self.height);
        for cy in y0..y1 {
            for cx in x0..x1 {
                self.pixel_buffer.set_pixel(cx, cy, r, g, b, a);
            }
        }
    }
}

/// Parse a `#rgb`/`#rrggbb` CSS color into RGBA. Unknown formats fall back to
/// opaque black; this is only used for the secondary pixel raster.
fn parse_css_color(s: &str) -> (u8, u8, u8, u8) {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                return (r, g, b, 255);
            }
            3 => {
                let expand = |c: &str| u8::from_str_radix(c, 16).map(|v| v * 17).unwrap_or(0);
                let r = expand(&hex[0..1]);
                let g = expand(&hex[1..2]);
                let b = expand(&hex[2..3]);
                return (r, g, b, 255);
            }
            _ => {}
        }
    }
    (0, 0, 0, 255)
}

/// Minimal base64 encoder (no external deps).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_text_in_display_list() {
        let mut ctx = Canvas2DContext::new(200, 100);
        ctx.set_font("16px Arial");
        ctx.set_fill_style("#ff0000");
        ctx.fill_text("Add to cart", 10.0, 20.0);
        assert_eq!(ctx.draw_call_count(), 1);
        assert_eq!(ctx.drawn_text(), vec!["Add to cart"]);
    }

    #[test]
    fn fill_rect_records_and_rasterizes() {
        let mut ctx = Canvas2DContext::new(10, 10);
        ctx.set_fill_style("#00ff00");
        ctx.fill_rect(0.0, 0.0, 2.0, 2.0);
        assert_eq!(ctx.pixel_buffer.get_pixel(0, 0), [0, 255, 0, 255]);
        assert!(matches!(ctx.display_list[0], DrawCommand::FillRect { .. }));
    }

    #[test]
    fn path_ops_are_recorded_in_order() {
        let mut ctx = Canvas2DContext::new(50, 50);
        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.line_to(10.0, 10.0);
        ctx.stroke();
        assert_eq!(ctx.display_list.len(), 4);
        assert!(matches!(ctx.display_list[0], DrawCommand::BeginPath));
    }

    #[test]
    fn to_data_url_produces_valid_prefix() {
        let mut ctx = Canvas2DContext::new(4, 4);
        ctx.set_fill_style("#ff0000");
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        let url = ctx.to_data_url("");
        assert!(url.starts_with("data:image/bmp;base64,"));
        // Should be non-empty base64
        assert!(url.len() > 30);
    }

    #[test]
    fn get_image_data_returns_correct_size() {
        let ctx = Canvas2DContext::new(10, 10);
        let data = ctx.get_image_data(0, 0, 5, 5);
        assert_eq!(data.len(), 5 * 5 * 4);
    }

    #[test]
    fn base64_encode_basic() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b"Hi"), "SGk=");
        assert_eq!(base64_encode(b"ABC"), "QUJD");
    }
}
