use std::collections::HashMap;

use crate::dom::DomTree;
use crate::nda::NdaTriple;
use crate::parser::html::{DomNode, NodeType};
use crate::style::StyleCascader;

/// Viewport width used as the initial containing block.
const VIEWPORT_WIDTH: f32 = 800.0;
/// Approximate advance width of one character (monospace assumption).
const CHAR_WIDTH: f32 = 8.0;
/// Line box height for a single run of text.
const LINE_HEIGHT: f32 = 16.0;

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

impl LayoutBox {
    /// Total vertical space this box occupies including margins.
    fn margin_box_height(&self) -> f32 {
        self.height + self.margin[0] + self.margin[2]
    }

    /// Total horizontal space this box occupies including margins.
    fn margin_box_width(&self) -> f32 {
        self.width + self.margin[1] + self.margin[3]
    }
}

pub struct LayoutEngine2D {
    pub cascader: StyleCascader,
}

impl LayoutEngine2D {
    pub fn new(cascader: StyleCascader) -> Self {
        Self { cascader }
    }

    /// Compute a real box-model layout and return every box in document order
    /// with absolute (viewport-relative) coordinates.
    ///
    /// This performs recursive block/inline flow: block boxes stack vertically
    /// inside their containing block; inline boxes and text pack horizontally
    /// into line boxes that wrap at the content width; children are positioned
    /// inside their parent's padding box, so the geometry is genuinely nested
    /// rather than a flat stack of fixed-height rows.
    pub fn build_layout_tree(&self, tree: &DomTree) -> Vec<LayoutBox> {
        let mut roots = Vec::new();
        let mut cursor_y = 0.0f32;
        if let Some(doc) = tree.nodes.first() {
            for &child in &doc.children {
                if let Some(mut b) = self.layout_node(tree, child, VIEWPORT_WIDTH) {
                    translate(&mut b, 0.0, cursor_y);
                    cursor_y += b.margin_box_height();
                    roots.push(b);
                }
            }
        }

        let mut flat = Vec::new();
        for root in &roots {
            flatten(root, &mut flat);
        }
        flat
    }

    /// Lay out a single node relative to a placement origin of (0, 0), where the
    /// returned box's (x, y) is the border-box corner offset by its own margins.
    /// The caller translates the box into its final position.
    fn layout_node(&self, tree: &DomTree, id: usize, avail_width: f32) -> Option<LayoutBox> {
        let node = tree.get_node(id)?;
        match node.node_type {
            NodeType::Text => {
                let text = node.text_content.trim();
                if text.is_empty() {
                    return None;
                }
                let count = text.chars().count() as f32;
                let width = (count * CHAR_WIDTH).min(avail_width.max(CHAR_WIDTH)).max(CHAR_WIDTH);
                let per_line = (avail_width / CHAR_WIDTH).floor().max(1.0);
                let lines = (count / per_line).ceil().max(1.0);
                Some(LayoutBox {
                    node_id: id,
                    display: DisplayMode::Inline,
                    x: 0.0,
                    y: 0.0,
                    width,
                    height: lines * LINE_HEIGHT,
                    padding: [0.0; 4],
                    margin: [0.0; 4],
                    z_index: 0,
                    is_visible: true,
                    children: Vec::new(),
                })
            }
            NodeType::Element => {
                let style = self.computed_style_for(node);
                let display = resolve_display(&style, &node.tag_name);
                if display == DisplayMode::None {
                    return None;
                }

                let padding = parse_edges(&style, "padding");
                let margin = parse_edges(&style, "margin");
                let (intrinsic_w, intrinsic_h) = intrinsic_size(&node.tag_name);
                let explicit_w = parse_px(&style, "width").or(intrinsic_w);
                let explicit_h = parse_px(&style, "height").or(intrinsic_h);
                let is_inline = display == DisplayMode::Inline;

                // Width available to lay out children (the content box).
                let outer_width = match explicit_w {
                    Some(w) => w,
                    None if is_inline => avail_width,
                    None => (avail_width - margin[1] - margin[3]).max(0.0),
                };
                let content_width = (outer_width - padding[1] - padding[3]).max(0.0);

                let (mut children, used_width, used_height) =
                    self.layout_children(tree, node, content_width);

                // Position children inside the padding box.
                for c in &mut children {
                    translate(c, padding[3], padding[0]);
                }

                let inner_width = if is_inline && explicit_w.is_none() {
                    used_width
                } else {
                    content_width
                };
                let border_w = match explicit_w {
                    Some(w) => w,
                    None => inner_width + padding[1] + padding[3],
                };
                let border_h = match explicit_h {
                    Some(h) => h,
                    None => used_height + padding[0] + padding[2],
                };

                let visible = style.get("visibility").map(|v| v != "hidden").unwrap_or(true);

                Some(LayoutBox {
                    node_id: id,
                    display,
                    x: margin[3],
                    y: margin[0],
                    width: border_w,
                    height: border_h,
                    padding,
                    margin,
                    z_index: parse_z(&style),
                    is_visible: visible,
                    children,
                })
            }
            NodeType::Document => None,
        }
    }

    /// Flow a node's children into block and inline runs within `content_width`,
    /// returning the positioned children plus the width and height consumed.
    fn layout_children(
        &self,
        tree: &DomTree,
        node: &DomNode,
        content_width: f32,
    ) -> (Vec<LayoutBox>, f32, f32) {
        let mut children = Vec::new();
        let mut line_x = 0.0f32;
        let mut line_top = 0.0f32;
        let mut line_height = 0.0f32;
        let mut max_width = 0.0f32;

        for &child_id in &node.children {
            let Some(mut cb) = self.layout_node(tree, child_id, content_width) else {
                continue;
            };
            let is_block = matches!(cb.display, DisplayMode::Block | DisplayMode::Flex);
            let mb_w = cb.margin_box_width();
            let mb_h = cb.margin_box_height();

            if is_block {
                // A block breaks the current line, then occupies its own row.
                if line_height > 0.0 {
                    line_top += line_height;
                    line_x = 0.0;
                    line_height = 0.0;
                }
                translate(&mut cb, 0.0, line_top);
                line_top += mb_h;
                max_width = max_width.max(mb_w);
                children.push(cb);
            } else {
                // Inline/text: pack onto the current line, wrapping when full.
                if line_x > 0.0 && line_x + mb_w > content_width {
                    line_top += line_height;
                    line_x = 0.0;
                    line_height = 0.0;
                }
                translate(&mut cb, line_x, line_top);
                line_x += mb_w;
                line_height = line_height.max(mb_h);
                max_width = max_width.max(line_x);
                children.push(cb);
            }
        }

        if line_height > 0.0 {
            line_top += line_height;
        }
        (children, max_width, line_top)
    }

    fn computed_style_for(&self, node: &DomNode) -> HashMap<String, String> {
        self.cascader.compute_computed_style(|sel| {
            sel.contains(&node.tag_name)
                || node.attributes.get("id").map(|s| s.as_str()) == Some(sel.trim_start_matches('#'))
        })
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

/// Shift a box and all its descendants by (dx, dy).
fn translate(b: &mut LayoutBox, dx: f32, dy: f32) {
    b.x += dx;
    b.y += dy;
    for c in &mut b.children {
        translate(c, dx, dy);
    }
}

/// Depth-first flatten into document order, clearing nested children so each
/// node appears exactly once with absolute coordinates.
fn flatten(b: &LayoutBox, out: &mut Vec<LayoutBox>) {
    let mut shallow = b.clone();
    shallow.children = Vec::new();
    out.push(shallow);
    for c in &b.children {
        flatten(c, out);
    }
}

fn resolve_display(style: &HashMap<String, String>, tag: &str) -> DisplayMode {
    match style.get("display").map(|s| s.as_str()) {
        Some("none") => DisplayMode::None,
        Some("flex") => DisplayMode::Flex,
        Some("inline") | Some("inline-block") => DisplayMode::Inline,
        Some("block") => DisplayMode::Block,
        _ if default_inline(tag) => DisplayMode::Inline,
        _ => DisplayMode::Block,
    }
}

/// Tags that default to inline in the UA stylesheet.
fn default_inline(tag: &str) -> bool {
    matches!(
        tag,
        "span" | "a" | "b" | "i" | "em" | "strong" | "code" | "small" | "label" | "img" | "sub"
            | "sup" | "u" | "mark" | "abbr" | "cite" | "q"
    )
}

/// Intrinsic (width, height) defaults for replaced/void elements.
fn intrinsic_size(tag: &str) -> (Option<f32>, Option<f32>) {
    match tag {
        "img" | "input" | "button" | "select" | "textarea" => (Some(120.0), Some(20.0)),
        "hr" => (None, Some(2.0)),
        "br" => (None, Some(LINE_HEIGHT)),
        _ => (None, None),
    }
}

fn parse_px(style: &HashMap<String, String>, prop: &str) -> Option<f32> {
    style.get(prop).and_then(|v| {
        let v = v.trim().trim_end_matches("px").trim();
        v.parse::<f32>().ok()
    })
}

/// Parse a box edge property (padding/margin) supporting a single shorthand
/// value plus per-side overrides (`padding-top`, etc.).
fn parse_edges(style: &HashMap<String, String>, base: &str) -> [f32; 4] {
    let mut e = [0.0f32; 4];
    if let Some(v) = parse_px(style, base) {
        e = [v; 4];
    }
    if let Some(v) = parse_px(style, &format!("{}-top", base)) {
        e[0] = v;
    }
    if let Some(v) = parse_px(style, &format!("{}-right", base)) {
        e[1] = v;
    }
    if let Some(v) = parse_px(style, &format!("{}-bottom", base)) {
        e[2] = v;
    }
    if let Some(v) = parse_px(style, &format!("{}-left", base)) {
        e[3] = v;
    }
    e
}

fn parse_z(style: &HashMap<String, String>) -> i32 {
    style
        .get("z-index")
        .and_then(|v| v.trim().parse::<i32>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::HtmlParser;

    fn layout(html: &str, cascader: StyleCascader) -> (DomTree, Vec<LayoutBox>) {
        let tree = DomTree::new(HtmlParser::parse_html5(html));
        let engine = LayoutEngine2D::new(cascader);
        let boxes = engine.build_layout_tree(&tree);
        (tree, boxes)
    }

    fn box_for_tag<'a>(tree: &DomTree, boxes: &'a [LayoutBox], tag: &str) -> &'a LayoutBox {
        boxes
            .iter()
            .find(|b| tree.get_node(b.node_id).map(|n| n.tag_name == tag).unwrap_or(false))
            .expect("box for tag")
    }

    #[test]
    fn sibling_blocks_stack_vertically() {
        let (tree, boxes) = layout("<div>A</div><div>B</div>", StyleCascader::new());
        let divs: Vec<_> = boxes
            .iter()
            .filter(|b| tree.get_node(b.node_id).map(|n| n.tag_name == "div").unwrap_or(false))
            .collect();
        assert_eq!(divs.len(), 2);
        assert!(divs[1].y > divs[0].y, "second block should be below the first");
    }

    #[test]
    fn child_is_contained_within_parent() {
        let (tree, boxes) = layout("<div><p>Hello</p></div>", StyleCascader::new());
        let div = box_for_tag(&tree, &boxes, "div");
        let p = box_for_tag(&tree, &boxes, "p");
        assert!(p.x >= div.x && p.y >= div.y, "child origin inside parent");
        assert!(p.y + p.height <= div.y + div.height + 0.01, "child fits vertically");
        assert!(div.height > 0.0, "parent derives height from content");
    }

    #[test]
    fn text_contributes_height() {
        let (tree, boxes) = layout("<p>Hello world</p>", StyleCascader::new());
        let p = box_for_tag(&tree, &boxes, "p");
        assert!(p.height >= LINE_HEIGHT, "text gives the paragraph a line box");
    }

    #[test]
    fn display_none_is_skipped() {
        let mut cascader = StyleCascader::new();
        let mut decl = HashMap::new();
        decl.insert("display".to_string(), "none".to_string());
        cascader.add_rule("div", decl);
        let (tree, boxes) = layout("<div>gone</div>", cascader);
        assert!(
            boxes
                .iter()
                .all(|b| tree.get_node(b.node_id).map(|n| n.tag_name != "div").unwrap_or(true)),
            "display:none element produces no box"
        );
    }

    #[test]
    fn explicit_width_and_height_are_honored() {
        let mut cascader = StyleCascader::new();
        let mut decl = HashMap::new();
        decl.insert("width".to_string(), "200px".to_string());
        decl.insert("height".to_string(), "100px".to_string());
        cascader.add_rule("div", decl);
        let (tree, boxes) = layout("<div>x</div>", cascader);
        let div = box_for_tag(&tree, &boxes, "div");
        assert_eq!(div.width, 200.0);
        assert_eq!(div.height, 100.0);
    }

    #[test]
    fn inline_tags_default_to_inline_display() {
        let (tree, boxes) = layout("<span>hi</span><a href=\"#\">link</a>", StyleCascader::new());
        let span = box_for_tag(&tree, &boxes, "span");
        let a = box_for_tag(&tree, &boxes, "a");
        assert_eq!(span.display, DisplayMode::Inline);
        assert_eq!(a.display, DisplayMode::Inline);
    }

    #[test]
    fn padding_and_margin_are_parsed() {
        let mut cascader = StyleCascader::new();
        let mut decl = HashMap::new();
        decl.insert("padding".to_string(), "10px".to_string());
        decl.insert("margin".to_string(), "5px".to_string());
        cascader.add_rule("div", decl);
        let (tree, boxes) = layout("<div>content</div>", cascader);
        let div = box_for_tag(&tree, &boxes, "div");
        assert_eq!(div.padding, [10.0; 4]);
        assert_eq!(div.margin, [5.0; 4]);
    }

    #[test]
    fn z_index_is_extracted_from_style() {
        let mut cascader = StyleCascader::new();
        let mut decl = HashMap::new();
        decl.insert("z-index".to_string(), "42".to_string());
        cascader.add_rule("div", decl);
        let (tree, boxes) = layout("<div>layered</div>", cascader);
        let div = box_for_tag(&tree, &boxes, "div");
        assert_eq!(div.z_index, 42);
    }
}
