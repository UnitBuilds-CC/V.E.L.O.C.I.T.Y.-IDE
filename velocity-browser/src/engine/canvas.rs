use crate::engine::canvas_context::DrawCommand;
use crate::nda::{NdaDocument, NdaTriple};
use crate::predicates::{
    CANVAS_CONTEXT, CANVAS_DRAW_CALLS, CANVAS_IMAGE, CANVAS_SIZE, CANVAS_SHAPE, CANVAS_SUMMARY,
    CANVAS_TEXT,
};

#[derive(Debug, Clone)]
pub struct CanvasElement {
    pub id: String,
    pub context_type: String, // "2d", "webgl", "webgl2", "bitmaprenderer"
    pub width: u32,
    pub height: u32,
    pub draw_call_count: u64,
    /// Recorded 2D draw operations (empty for non-2d contexts). This is what
    /// lets an agent read canvas text/shapes without screenshotting.
    pub display_list: Vec<DrawCommand>,
}

impl CanvasElement {
    /// Metadata-only canvas (no recorded 2D display list).
    pub fn new(id: &str, context_type: &str, width: u32, height: u32) -> Self {
        Self {
            id: id.to_string(),
            context_type: context_type.to_string(),
            width,
            height,
            draw_call_count: 0,
            display_list: Vec::new(),
        }
    }

    /// A 2d canvas carrying its recorded display list.
    pub fn with_display_list(id: &str, width: u32, height: u32, display_list: Vec<DrawCommand>) -> Self {
        Self {
            id: id.to_string(),
            context_type: "2d".to_string(),
            width,
            height,
            draw_call_count: display_list.len() as u64,
            display_list,
        }
    }
}

pub struct CanvasExtractor;

impl CanvasExtractor {
    /// Legacy hashed-triple export (metadata only). Kept for the existing
    /// packed-binary state path; use [`extract_canvases_document`] for content
    /// an agent can actually read.
    pub fn extract_canvases_nda(canvases: &[CanvasElement]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(canvases.len() * 3);
        for canvas in canvases {
            triples.push(NdaTriple::new(&canvas.id, CANVAS_CONTEXT, &canvas.context_type));
            triples.push(NdaTriple::new(&canvas.id, CANVAS_SIZE, &format!("{}x{}", canvas.width, canvas.height)));
            triples.push(NdaTriple::new(&canvas.id, CANVAS_DRAW_CALLS, &canvas.draw_call_count.to_string()));
        }
        triples
    }

    /// Lossless export: canvas metadata plus every drawn text/shape/image as
    /// recoverable literals, so the agent understands the canvas contents.
    pub fn extract_canvases_document(canvases: &[CanvasElement]) -> NdaDocument {
        let mut doc = NdaDocument::new();
        for canvas in canvases {
            doc.push_str(&canvas.id, CANVAS_CONTEXT, &canvas.context_type);
            doc.push_str(&canvas.id, CANVAS_SIZE, &format!("{}x{}", canvas.width, canvas.height));
            doc.push_int(&canvas.id, CANVAS_DRAW_CALLS, canvas.draw_call_count as i64);
            Self::emit_display_list(&mut doc, &canvas.id, &canvas.display_list);
        }
        doc
    }

    fn emit_display_list(doc: &mut NdaDocument, id: &str, list: &[DrawCommand]) {
        let mut text_count = 0u64;
        let mut shape_count = 0u64;
        let mut image_count = 0u64;
        for cmd in list {
            match cmd {
                DrawCommand::FillText { text, x, y, .. }
                | DrawCommand::StrokeText { text, x, y, .. } => {
                    text_count += 1;
                    doc.push_str(id, CANVAS_TEXT, &format!("{}@{},{}", text, x, y));
                }
                DrawCommand::DrawImage { src, dx, dy, dw, dh } => {
                    image_count += 1;
                    doc.push_str(id, CANVAS_IMAGE, &format!("{}@{},{},{},{}", src, dx, dy, dw, dh));
                }
                DrawCommand::FillRect { x, y, w, h, .. } => {
                    shape_count += 1;
                    doc.push_str(id, CANVAS_SHAPE, &format!("fillRect {},{},{},{}", x, y, w, h));
                }
                DrawCommand::StrokeRect { x, y, w, h, .. } => {
                    shape_count += 1;
                    doc.push_str(id, CANVAS_SHAPE, &format!("strokeRect {},{},{},{}", x, y, w, h));
                }
                DrawCommand::Arc { x, y, radius, .. } => {
                    shape_count += 1;
                    doc.push_str(id, CANVAS_SHAPE, &format!("arc {},{} r{}", x, y, radius));
                }
                // Path scaffolding (begin/move/line/close/fill/stroke/clear)
                // is not itself readable content; it is summarized via counts.
                _ => {}
            }
        }
        doc.push_str(
            id,
            CANVAS_SUMMARY,
            &format!("{} texts, {} shapes, {} images", text_count, shape_count, image_count),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_preserves_canvas_text() {
        let list = vec![
            DrawCommand::FillText {
                text: "Checkout".to_string(),
                x: 12.0,
                y: 34.0,
                font: "16px Arial".to_string(),
                style: "#000".to_string(),
            },
            DrawCommand::FillRect { x: 0.0, y: 0.0, w: 5.0, h: 5.0, style: "#fff".to_string() },
        ];
        let canvas = CanvasElement::with_display_list("canvas_1", 300, 150, list);
        let doc = CanvasExtractor::extract_canvases_document(&[canvas]);
        let texts: Vec<String> = doc
            .facts
            .iter()
            .filter(|f| f.predicate == CANVAS_TEXT)
            .filter_map(|f| doc.object_display(f))
            .collect();
        assert_eq!(texts, vec!["Checkout@12,34".to_string()]);
    }

    #[test]
    fn metadata_nda_export() {
        let canvas = CanvasElement::new("c1", "2d", 640, 480);
        let triples = CanvasExtractor::extract_canvases_nda(&[canvas]);
        assert_eq!(triples.len(), 3);
        // Verify predicates are present
        let predicates: Vec<u16> = triples.iter().map(|t| t.predicate_id).collect();
        assert!(predicates.contains(&CANVAS_CONTEXT));
        assert!(predicates.contains(&CANVAS_SIZE));
        assert!(predicates.contains(&CANVAS_DRAW_CALLS));
    }

    #[test]
    fn document_captures_images_and_shapes() {
        let list = vec![
            DrawCommand::DrawImage { src: "logo.png".to_string(), dx: 10.0, dy: 20.0, dw: 100.0, dh: 50.0 },
            DrawCommand::FillRect { x: 0.0, y: 0.0, w: 50.0, h: 50.0, style: "red".to_string() },
            DrawCommand::Arc { x: 25.0, y: 25.0, radius: 10.0, start_angle: 0.0, end_angle: std::f64::consts::TAU },
        ];
        let canvas = CanvasElement::with_display_list("c2", 200, 200, list);
        let doc = CanvasExtractor::extract_canvases_document(&[canvas]);
        let images: Vec<String> = doc.facts.iter()
            .filter(|f| f.predicate == CANVAS_IMAGE)
            .filter_map(|f| doc.object_display(f))
            .collect();
        assert_eq!(images.len(), 1);
        assert!(images[0].contains("logo.png"));
        let shapes: Vec<String> = doc.facts.iter()
            .filter(|f| f.predicate == CANVAS_SHAPE)
            .filter_map(|f| doc.object_display(f))
            .collect();
        assert_eq!(shapes.len(), 2); // fillRect + arc
    }

    #[test]
    fn summary_counts_are_accurate() {
        let list = vec![
            DrawCommand::FillText { text: "A".to_string(), x: 0.0, y: 0.0, font: "12px".to_string(), style: "".to_string() },
            DrawCommand::StrokeText { text: "B".to_string(), x: 10.0, y: 10.0, font: "12px".to_string(), style: "".to_string() },
            DrawCommand::FillRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0, style: "".to_string() },
        ];
        let canvas = CanvasElement::with_display_list("c3", 100, 100, list);
        let doc = CanvasExtractor::extract_canvases_document(&[canvas]);
        let summary: Vec<String> = doc.facts.iter()
            .filter(|f| f.predicate == CANVAS_SUMMARY)
            .filter_map(|f| doc.object_display(f))
            .collect();
        assert_eq!(summary.len(), 1);
        assert!(summary[0].contains("2 texts"));
        assert!(summary[0].contains("1 shapes"));
        assert!(summary[0].contains("0 images"));
    }
}
