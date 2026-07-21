use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct CanvasElement {
    pub id: String,
    pub context_type: String, // "2d", "webgl", "webgl2", "bitmaprenderer"
    pub width: u32,
    pub height: u32,
    pub draw_call_count: u64,
}

pub struct CanvasExtractor;

impl CanvasExtractor {
    pub fn extract_canvases_nda(canvases: &[CanvasElement]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(canvases.len() * 3);
        for canvas in canvases {
            triples.push(NdaTriple::new(&canvas.id, 40, &canvas.context_type));
            triples.push(NdaTriple::new(&canvas.id, 41, &format!("{}x{}", canvas.width, canvas.height)));
            triples.push(NdaTriple::new(&canvas.id, 42, &canvas.draw_call_count.to_string()));
        }
        triples
    }
}
