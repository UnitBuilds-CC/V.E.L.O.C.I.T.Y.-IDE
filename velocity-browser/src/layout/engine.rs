use crate::dom::DomTree;
use crate::nda::NdaTriple;
use crate::parser::html::NodeType;
use crate::style::StyleCascader;

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayMode {
    Block,
    Inline,
    Flex,
    None,
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node_id: usize,
    pub display: DisplayMode,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub padding: [f32; 4], // top, right, bottom, left
    pub margin: [f32; 4],
    pub z_index: i32,
    pub is_visible: bool,
    pub children: Vec<LayoutBox>,
}

pub struct LayoutEngine2D {
    pub cascader: StyleCascader,
}

impl LayoutEngine2D {
    pub fn new(cascader: StyleCascader) -> Self {
        Self { cascader }
    }

    pub fn build_layout_tree(&self, tree: &DomTree) -> Vec<LayoutBox> {
        let mut boxes = Vec::new();
        let mut current_y = 0.0;

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            let computed_style = self.cascader.compute_computed_style(|sel| {
                sel.contains(&node.tag_name) || node.attributes.get("id").map(|s| s.as_str()) == Some(sel.trim_start_matches('#'))
            });

            let display = match computed_style.get("display").map(|s| s.as_str()) {
                Some("none") => DisplayMode::None,
                Some("flex") => DisplayMode::Flex,
                Some("inline") | Some("inline-block") => DisplayMode::Inline,
                _ => DisplayMode::Block,
            };

            if display == DisplayMode::None {
                continue;
            }

            let height = 32.0;
            let width = if display == DisplayMode::Block { 800.0 } else { 120.0 };
            let x = 10.0;
            let y = current_y;

            if display == DisplayMode::Block {
                current_y += height + 8.0;
            }

            boxes.push(LayoutBox {
                node_id: node.id,
                display,
                x,
                y,
                width,
                height,
                padding: [4.0, 4.0, 4.0, 4.0],
                margin: [0.0, 0.0, 8.0, 0.0],
                z_index: 0,
                is_visible: true,
                children: Vec::new(),
            });
        }

        boxes
    }

    pub fn export_layout_nda(&self, boxes: &[LayoutBox]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(boxes.len() * 3);
        for b in boxes {
            let subject = format!("node_{}", b.node_id);
            let bounds_str = format!("{},{},{},{}", b.x, b.y, b.width, b.height);
            triples.push(NdaTriple::new(&subject, 70, &bounds_str));
            triples.push(NdaTriple::new(&subject, 71, if b.is_visible { "visible" } else { "hidden" }));
            triples.push(NdaTriple::new(&subject, 72, &format!("{:?}", b.display).to_lowercase()));
        }
        triples
    }
}
