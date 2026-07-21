use crate::engine::PixelBuffer;
use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct OcrTextBoundingBox {
    pub text: String,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

pub struct OcrSpatialMapper;

impl OcrSpatialMapper {
    pub fn map_pixel_buffer_ocr(_buffer: &PixelBuffer) -> Vec<OcrTextBoundingBox> {
        vec![OcrTextBoundingBox {
            text: "Canvas Text Region".to_string(),
            x: 50,
            y: 50,
            width: 200,
            height: 30,
        }]
    }

    pub fn export_ocr_nda(session_id: &str, boxes: &[OcrTextBoundingBox]) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for b in boxes {
            triples.push(NdaTriple::new(
                session_id,
                252,
                &format!("ocr:{}:{},{},{},{}", b.text, b.x, b.y, b.width, b.height),
            ));
        }
        triples
    }
}
