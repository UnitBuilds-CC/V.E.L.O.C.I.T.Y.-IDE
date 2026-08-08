use crate::engine::PixelBuffer;
use crate::nda::NdaTriple;
use crate::predicates::OCR_OPAQUE_REGION;

/// An opaque visual region detected in a raster buffer.
///
/// The engine deliberately does NOT fabricate recognized text: without a real
/// glyph recognizer it cannot honestly claim to know what a region says. It
/// reports the region's location and marks it opaque (confidence 0) so the
/// agent knows "there is something visual here it could not read" rather than
/// being fed a made-up string. Readable canvas text comes from the 2D display
/// list (fillText), not from guessing at pixels.
#[derive(Debug, Clone)]
pub struct OcrTextBoundingBox {
    /// Empty for opaque regions. Reserved for a future real recognizer.
    pub text: String,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    /// Recognition confidence in 0.0..=1.0. Always 0.0 for opaque regions -
    /// never fabricated.
    pub confidence: f32,
}

impl OcrTextBoundingBox {
    /// True when this box carries genuinely recognized text (never the case
    /// until a real recognizer lands).
    pub fn is_recognized(&self) -> bool {
        !self.text.is_empty() && self.confidence > 0.0
    }
}

pub struct VelocityOcrEngine {
    pub luminance_threshold: u8,
}

impl Default for VelocityOcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl VelocityOcrEngine {
    pub fn new() -> Self {
        Self { luminance_threshold: 128 }
    }

    /// Segment a raster buffer into opaque (dark-pixel) regions. This locates
    /// *where* visual content is, honestly, without inventing what it says.
    pub fn process_pixel_buffer(&self, buffer: &PixelBuffer) -> Vec<OcrTextBoundingBox> {
        let mut regions = Vec::new();
        let mut in_region = false;
        let mut start_x = 0;
        let mut start_y = 0;

        for y in (0..buffer.height).step_by(10) {
            for x in (0..buffer.width).step_by(10) {
                let pixel = buffer.get_pixel(x, y);
                let lum = ((pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3) as u8;

                if lum < self.luminance_threshold {
                    if !in_region {
                        in_region = true;
                        start_x = x;
                        start_y = y;
                    }
                } else if in_region {
                    in_region = false;
                    regions.push(OcrTextBoundingBox {
                        text: String::new(),
                        x: start_x,
                        y: start_y,
                        width: (x - start_x).max(10),
                        height: 20,
                        confidence: 0.0,
                    });
                }
            }
        }

        regions
    }

    /// Export opaque regions as NDA triples. Emits `OCR_OPAQUE_REGION` facts
    /// ("x,y,w,h"); does not emit fabricated recognized-text facts.
    pub fn export_ocr_nda(&self, session_id: &str, boxes: &[OcrTextBoundingBox]) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for b in boxes {
            triples.push(NdaTriple::new(
                session_id,
                OCR_OPAQUE_REGION,
                &format!("{},{},{},{}", b.x, b.y, b.width, b.height),
            ));
        }
        triples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_regions_carry_no_fabricated_text() {
        let engine = VelocityOcrEngine::new();
        let buffer = PixelBuffer::new(100, 100);
        let regions = engine.process_pixel_buffer(&buffer);
        for r in &regions {
            assert!(r.text.is_empty(), "OCR must not fabricate text");
            assert_eq!(r.confidence, 0.0, "opaque regions must not claim confidence");
            assert!(!r.is_recognized());
        }
    }

    #[test]
    fn export_uses_opaque_region_predicate() {
        let engine = VelocityOcrEngine::new();
        let boxes = vec![OcrTextBoundingBox {
            text: String::new(),
            x: 1,
            y: 2,
            width: 3,
            height: 4,
            confidence: 0.0,
        }];
        let triples = engine.export_ocr_nda("session_1", &boxes);
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].predicate_id, OCR_OPAQUE_REGION);
    }

    #[test]
    fn is_recognized_false_for_opaque() {
        let b = OcrTextBoundingBox {
            text: String::new(),
            x: 0, y: 0, width: 10, height: 10,
            confidence: 0.0,
        };
        assert!(!b.is_recognized());
    }

    #[test]
    fn is_recognized_true_with_text_and_confidence() {
        let b = OcrTextBoundingBox {
            text: "Hello".into(),
            x: 0, y: 0, width: 10, height: 10,
            confidence: 0.95,
        };
        assert!(b.is_recognized());
    }

    #[test]
    fn is_recognized_false_with_text_but_zero_confidence() {
        let b = OcrTextBoundingBox {
            text: "Hello".into(),
            x: 0, y: 0, width: 10, height: 10,
            confidence: 0.0,
        };
        assert!(!b.is_recognized());
    }

    #[test]
    fn default_threshold_is_128() {
        let engine = VelocityOcrEngine::default();
        assert_eq!(engine.luminance_threshold, 128);
    }

    #[test]
    fn export_nda_multiple_boxes() {
        let engine = VelocityOcrEngine::new();
        let boxes = vec![
            OcrTextBoundingBox { text: String::new(), x: 0, y: 0, width: 10, height: 10, confidence: 0.0 },
            OcrTextBoundingBox { text: String::new(), x: 20, y: 30, width: 40, height: 50, confidence: 0.0 },
        ];
        let triples = engine.export_ocr_nda("sess", &boxes);
        assert_eq!(triples.len(), 2);
    }

    #[test]
    fn dark_pixel_buffer_produces_regions() {
        let engine = VelocityOcrEngine::new();
        let mut buffer = PixelBuffer::new(50, 50);
        // Fill first half with dark pixels, second half with light
        for y in 0..50 {
            for x in 0..25 {
                buffer.set_pixel(x, y, 10, 10, 10, 255);
            }
            for x in 25..50 {
                buffer.set_pixel(x, y, 250, 250, 250, 255);
            }
        }
        let regions = engine.process_pixel_buffer(&buffer);
        // Should detect at least one opaque region (dark area)
        assert!(!regions.is_empty());
    }
}
