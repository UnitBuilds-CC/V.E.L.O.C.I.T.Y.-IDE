use crate::dom::DomTree;
use crate::nda::NdaTriple;
use crate::parser::html::NodeType;

#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_visible: bool,
}

pub struct LayoutEngine;

impl LayoutEngine {
    pub fn compute_bounding_box(node_id: usize, depth: usize) -> BoundingBox {
        let x = 10.0 + (depth as f32) * 5.0;
        let y = 20.0 + (node_id as f32) * 24.0;
        let width = 120.0;
        let height = 32.0;

        BoundingBox {
            x,
            y,
            width,
            height,
            is_visible: true,
        }
    }

    pub fn compute_layout_triples(tree: &DomTree) -> Vec<NdaTriple> {
        let mut triples = Vec::new();

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            let box_model = Self::compute_bounding_box(node.id, 1);
            let subject = format!("node_{}", node.id);
            let bounds_str = format!("{},{},{},{}", box_model.x, box_model.y, box_model.width, box_model.height);
            triples.push(NdaTriple::new(&subject, 60, &bounds_str));
            triples.push(NdaTriple::new(&subject, 61, if box_model.is_visible { "visible" } else { "hidden" }));
        }

        triples
    }
}
