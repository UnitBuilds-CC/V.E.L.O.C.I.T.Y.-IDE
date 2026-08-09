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
            let bounds_str = format!(
                "{},{},{},{}",
                box_model.x, box_model.y, box_model.width, box_model.height
            );
            triples.push(NdaTriple::new(&subject, 60, &bounds_str));
            triples.push(NdaTriple::new(
                &subject,
                61,
                if box_model.is_visible {
                    "visible"
                } else {
                    "hidden"
                },
            ));
        }

        triples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::HtmlParser;

    #[test]
    fn bounding_box_origin_matches_depth_and_id() {
        let bb = LayoutEngine::compute_bounding_box(0, 0);
        assert_eq!(bb.x, 10.0);
        assert_eq!(bb.y, 20.0);
        assert_eq!(bb.width, 120.0);
        assert_eq!(bb.height, 32.0);
        assert!(bb.is_visible);
    }

    #[test]
    fn bounding_box_x_advances_with_depth() {
        let bb0 = LayoutEngine::compute_bounding_box(0, 0);
        let bb3 = LayoutEngine::compute_bounding_box(0, 3);
        assert_eq!(bb3.x - bb0.x, 15.0); // 3 * 5.0
    }

    #[test]
    fn bounding_box_y_advances_with_node_id() {
        let bb0 = LayoutEngine::compute_bounding_box(0, 0);
        let bb5 = LayoutEngine::compute_bounding_box(5, 0);
        assert_eq!(bb5.y - bb0.y, 120.0); // 5 * 24.0
    }

    #[test]
    fn bounding_box_dimensions_are_constant() {
        for id in 0..10 {
            for depth in 0..5 {
                let bb = LayoutEngine::compute_bounding_box(id, depth);
                assert_eq!(bb.width, 120.0);
                assert_eq!(bb.height, 32.0);
            }
        }
    }

    #[test]
    fn compute_layout_triples_skips_text_nodes() {
        let tree = DomTree::new(HtmlParser::parse_html5("just text"));
        let triples = LayoutEngine::compute_layout_triples(&tree);
        // Text-only documents have no element nodes, so no triples
        assert!(triples.is_empty());
    }

    #[test]
    fn compute_layout_triples_emits_bounds_and_visibility() {
        let tree = DomTree::new(HtmlParser::parse_html5("<div><p>hi</p></div>"));
        let triples = LayoutEngine::compute_layout_triples(&tree);
        // Each element gets 2 triples (bounds predicate_id=60, visibility predicate_id=61)
        let element_count = tree
            .nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Element)
            .count();
        assert_eq!(triples.len(), element_count * 2);
        // Check that bounds triples use predicate_id 60
        let bounds = triples.iter().filter(|t| t.predicate_id == 60).count();
        assert_eq!(bounds, element_count);
        // Check that visibility triples use predicate_id 61
        let vis = triples.iter().filter(|t| t.predicate_id == 61).count();
        assert_eq!(vis, element_count);
    }

    #[test]
    fn compute_layout_triples_bounds_predicate_format() {
        let tree = DomTree::new(HtmlParser::parse_html5("<span>x</span>"));
        let triples = LayoutEngine::compute_layout_triples(&tree);
        let bounds_triple = triples.iter().find(|t| t.predicate_id == 60).unwrap();
        // predicate_id should be 60 for bounds
        assert_eq!(bounds_triple.predicate_id, 60);
        // subject_hash should be non-zero (it's a hash)
        assert!(bounds_triple.subject_hash != 0);
    }

    #[test]
    fn compute_layout_triples_visibility_predicate() {
        let tree = DomTree::new(HtmlParser::parse_html5("<div>content</div>"));
        let triples = LayoutEngine::compute_layout_triples(&tree);
        let vis = triples.iter().find(|t| t.predicate_id == 61).unwrap();
        assert_eq!(vis.predicate_id, 61);
    }

    #[test]
    fn compute_layout_triples_subject_hashes_differ_per_node() {
        let tree = DomTree::new(HtmlParser::parse_html5("<div><span>hi</span></div>"));
        let triples = LayoutEngine::compute_layout_triples(&tree);
        let bounds: Vec<_> = triples.iter().filter(|t| t.predicate_id == 60).collect();
        // Each node should have a distinct subject hash
        let mut hashes = bounds.iter().map(|t| t.subject_hash).collect::<Vec<_>>();
        hashes.sort();
        hashes.dedup();
        assert_eq!(
            hashes.len(),
            bounds.len(),
            "each node should have unique subject hash"
        );
    }

    #[test]
    fn compute_layout_triples_empty_tree() {
        let tree = DomTree::new(vec![]);
        let triples = LayoutEngine::compute_layout_triples(&tree);
        assert!(triples.is_empty());
    }
}
