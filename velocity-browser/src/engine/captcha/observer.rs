//! DOM-level observation pipeline for captcha challenges.
//!
//! Complements the pixel-level fingerprinting by extracting structural
//! information from the DOM: interactive elements, grid layouts, instruction
//! text, iframe boundaries, and canvas elements.

use crate::dom::DomTree;

/// A snapshot of the challenge's DOM structure.
#[derive(Debug, Clone)]
pub struct ChallengeSnapshot {
    /// Node ID of the challenge container element.
    pub container_node_id: usize,
    /// Interactive elements found within the challenge.
    pub interactive_elements: Vec<InteractiveElement>,
    /// Detected grid layout (if any).
    pub grid_layout: Option<GridLayout>,
    /// Instruction text extracted from the challenge.
    pub instruction_text: Option<String>,
    /// Node IDs of iframe boundaries.
    pub iframe_boundaries: Vec<usize>,
    /// Structural markers (data attributes, class patterns).
    pub structural_markers: Vec<String>,
    /// Node IDs of canvas elements within the challenge.
    pub canvas_elements: Vec<usize>,
}

/// An interactive element within the challenge.
#[derive(Debug, Clone)]
pub struct InteractiveElement {
    pub node_id: usize,
    pub tag: String,
    /// Semantic role: "checkbox", "tile", "slider", "button", "image", "input".
    pub role: String,
    /// Position (x, y, width, height) from layout attributes.
    pub position: (f64, f64, f64, f64),
    /// Current state of the element.
    pub state: ElementState,
    /// CSS classes on the element.
    pub classes: Vec<String>,
}

/// State of an interactive element.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementState {
    Default,
    Selected,
    Disabled,
    Active,
    Flipped,
}

/// Detected grid layout within the challenge.
#[derive(Debug, Clone)]
pub struct GridLayout {
    pub rows: u8,
    pub cols: u8,
    pub cell_node_ids: Vec<usize>,
}

/// The challenge observer — extracts structural information from the DOM.
pub struct ChallengeObserver;

impl ChallengeObserver {
    /// Observe the DOM tree and produce a challenge snapshot.
    /// Searches for captcha container elements and extracts their structure.
    pub fn observe(tree: &DomTree) -> Option<ChallengeSnapshot> {
        // Find the captcha container
        let container_id = Self::find_container(tree)?;

        let interactive_elements = Self::extract_interactive_elements(tree, container_id);
        let grid_layout = Self::detect_grid_layout(tree, container_id);
        let instruction_text = Self::extract_instruction_text(tree, container_id);
        let iframe_boundaries = Self::find_iframes(tree, container_id);
        let structural_markers = Self::extract_markers(tree, container_id);
        let canvas_elements = Self::find_canvases(tree, container_id);

        Some(ChallengeSnapshot {
            container_node_id: container_id,
            interactive_elements,
            grid_layout,
            instruction_text,
            iframe_boundaries,
            structural_markers,
            canvas_elements,
        })
    }

    /// Find the captcha container node by looking for known patterns.
    fn find_container(tree: &DomTree) -> Option<usize> {
        for (i, node) in tree.nodes.iter().enumerate() {
            // Check for captcha-related classes
            if let Some(class) = node.attributes.get("class") {
                let class_lower = class.to_lowercase();
                if class_lower.contains("captcha")
                    || class_lower.contains("hcaptcha")
                    || class_lower.contains("recaptcha")
                    || class_lower.contains("g-recaptcha")
                    || class_lower.contains("h-captcha")
                    || class_lower.contains("cf-turnstile")
                    || class_lower.contains("challenge")
                {
                    return Some(i);
                }
            }

            // Check for captcha-related IDs
            if let Some(id) = node.attributes.get("id") {
                let id_lower = id.to_lowercase();
                if id_lower.contains("captcha") || id_lower.contains("challenge") {
                    return Some(i);
                }
            }

            // Check for data-sitekey (reCAPTCHA/hCaptcha marker)
            if node.attributes.contains_key("data-sitekey") {
                return Some(i);
            }

            // Check iframe src for captcha providers
            if node.tag_name == "iframe" {
                if let Some(src) = node.attributes.get("src") {
                    if src.contains("hcaptcha")
                        || src.contains("recaptcha")
                        || src.contains("turnstile")
                        || src.contains("funcaptcha")
                        || src.contains("arkoselabs")
                    {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    /// Extract interactive elements from the challenge container's subtree.
    fn extract_interactive_elements(tree: &DomTree, container_id: usize) -> Vec<InteractiveElement> {
        let mut elements = Vec::new();
        Self::walk_subtree(tree, container_id, &mut |node_id, node| {
            let role = Self::classify_element_role(node);
            if let Some(role) = role {
                let classes = node
                    .attributes
                    .get("class")
                    .map(|c| c.split_whitespace().map(|s| s.to_string()).collect())
                    .unwrap_or_default();

                let position = Self::extract_position(node);
                let state = Self::extract_state(node);

                elements.push(InteractiveElement {
                    node_id,
                    tag: node.tag_name.clone(),
                    role,
                    position,
                    state,
                    classes,
                });
            }
        });
        elements
    }

    /// Classify an element's role based on its tag and attributes.
    fn classify_element_role(node: &crate::parser::html::DomNode) -> Option<String> {
        let tag = node.tag_name.as_str();
        let role_attr = node.attributes.get("role").map(|r| r.as_str());
        let class = node.attributes.get("class").map(|c| c.to_lowercase());
        let class_ref = class.as_deref().unwrap_or("");

        // Explicit role attribute
        if let Some(role) = role_attr {
            return Some(match role {
                "checkbox" => "checkbox".to_string(),
                "button" => "button".to_string(),
                "slider" => "slider".to_string(),
                "img" | "image" => "image".to_string(),
                "textbox" => "input".to_string(),
                _ => role.to_string(),
            });
        }

        // Tag-based classification
        match tag {
            "input" => {
                let input_type = node.attributes.get("type").map(|t| t.as_str()).unwrap_or("text");
                Some(match input_type {
                    "checkbox" => "checkbox".to_string(),
                    "range" => "slider".to_string(),
                    "submit" | "button" => "button".to_string(),
                    _ => "input".to_string(),
                })
            }
            "button" => Some("button".to_string()),
            "img" => Some("image".to_string()),
            "canvas" => Some("canvas".to_string()),
            "div" | "span" | "td" => {
                // Class-based tile/checkbox detection
                if class_ref.contains("tile") || class_ref.contains("cell") {
                    Some("tile".to_string())
                } else if class_ref.contains("checkbox") || class_ref.contains("check") {
                    Some("checkbox".to_string())
                } else if class_ref.contains("slider") || class_ref.contains("handle") {
                    Some("slider".to_string())
                } else if class_ref.contains("button") || class_ref.contains("btn") {
                    Some("button".to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract position from layout-related attributes.
    fn extract_position(node: &crate::parser::html::DomNode) -> (f64, f64, f64, f64) {
        let x = node.attributes.get("data-x")
            .or(node.attributes.get("left"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let y = node.attributes.get("data-y")
            .or(node.attributes.get("top"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let w = node.attributes.get("data-width")
            .or(node.attributes.get("width"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let h = node.attributes.get("data-height")
            .or(node.attributes.get("height"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        (x, y, w, h)
    }

    /// Extract element state from attributes.
    fn extract_state(node: &crate::parser::html::DomNode) -> ElementState {
        if node.attributes.contains_key("disabled") {
            return ElementState::Disabled;
        }
        if node.attributes.contains_key("checked") || node.attributes.contains_key("selected") {
            return ElementState::Selected;
        }
        if let Some(class) = node.attributes.get("class") {
            let class_lower = class.to_lowercase();
            if class_lower.contains("active") {
                return ElementState::Active;
            }
            if class_lower.contains("selected") || class_lower.contains("checked") {
                return ElementState::Selected;
            }
            if class_lower.contains("flipped") || class_lower.contains("rotated") {
                return ElementState::Flipped;
            }
        }
        ElementState::Default
    }

    /// Detect grid layout within the challenge container.
    fn detect_grid_layout(tree: &DomTree, container_id: usize) -> Option<GridLayout> {
        let mut cells = Vec::new();
        Self::walk_subtree(tree, container_id, &mut |node_id, node| {
            let class = node.attributes.get("class").map(|c| c.to_lowercase()).unwrap_or_default();
            if class.contains("tile") || class.contains("cell") || class.contains("grid-item") {
                cells.push(node_id);
            }
            // Also detect table-based grids
            if node.tag_name == "td" {
                cells.push(node_id);
            }
        });

        if cells.is_empty() {
            return None;
        }

        // Infer grid dimensions from cell count
        let count = cells.len();
        let (rows, cols) = match count {
            4 => (2, 2),
            9 => (3, 3),
            16 => (4, 4),
            25 => (5, 5),
            6 => (2, 3),
            12 => (3, 4),
            _ => {
                // Try to find a square-ish factorization
                let sqrt = (count as f64).sqrt() as u8;
                if sqrt * sqrt == count as u8 {
                    (sqrt, sqrt)
                } else {
                    (1, count as u8)
                }
            }
        };

        Some(GridLayout {
            rows,
            cols,
            cell_node_ids: cells,
        })
    }

    /// Extract instruction text from the challenge.
    fn extract_instruction_text(tree: &DomTree, container_id: usize) -> Option<String> {
        let mut text = None;
        Self::walk_subtree(tree, container_id, &mut |_node_id, node| {
            if text.is_some() {
                return;
            }
            // Look for instruction-like elements
            let class = node.attributes.get("class").map(|c| c.to_lowercase()).unwrap_or_default();
            if (class.contains("instruction") || class.contains("prompt") || class.contains("label"))
                && !node.text_content.is_empty() {
                    text = Some(node.text_content.clone());
                }
            // Also check for heading tags
            if matches!(node.tag_name.as_str(), "h1" | "h2" | "h3" | "h4" | "p" | "span")
                && !node.text_content.is_empty() && node.text_content.len() > 5 {
                    text = Some(node.text_content.clone());
                }
        });
        text
    }

    /// Find iframe boundaries within the challenge.
    fn find_iframes(tree: &DomTree, container_id: usize) -> Vec<usize> {
        let mut iframes = Vec::new();
        Self::walk_subtree(tree, container_id, &mut |node_id, node| {
            if node.tag_name == "iframe" {
                iframes.push(node_id);
            }
        });
        iframes
    }

    /// Extract structural markers (data attributes, class patterns).
    fn extract_markers(tree: &DomTree, container_id: usize) -> Vec<String> {
        let mut markers = Vec::new();
        if let Some(node) = tree.get_node(container_id) {
            // Collect data-* attributes as markers
            for key in node.attributes.keys() {
                if key.starts_with("data-") {
                    markers.push(key.clone());
                }
            }
            // Collect class-based markers
            if let Some(class) = node.attributes.get("class") {
                for cls in class.split_whitespace() {
                    if cls.contains("captcha") || cls.contains("challenge") || cls.contains("verify") {
                        markers.push(format!("class:{}", cls));
                    }
                }
            }
        }
        markers
    }

    /// Find canvas elements within the challenge.
    fn find_canvases(tree: &DomTree, container_id: usize) -> Vec<usize> {
        let mut canvases = Vec::new();
        Self::walk_subtree(tree, container_id, &mut |node_id, node| {
            if node.tag_name == "canvas" {
                canvases.push(node_id);
            }
        });
        canvases
    }

    /// Walk the subtree rooted at `root_id`, calling `f` for each node.
    fn walk_subtree<F>(tree: &DomTree, root_id: usize, f: &mut F)
    where
        F: FnMut(usize, &crate::parser::html::DomNode),
    {
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = tree.get_node(id) {
                f(id, node);
                for &child in &node.children {
                    stack.push(child);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(id: usize, tag: &str, attrs: &[(&str, &str)], children: Vec<usize>) -> DomNode {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }
        DomNode {
            id,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes,
            text_content: String::new(),
            children,
            parent: None,
        }
    }

    #[test]
    fn detect_container_by_class() {
        let nodes = vec![
            make_node(0, "div", &[("class", "page-wrapper")], vec![1]),
            make_node(1, "div", &[("class", "h-captcha")], vec![]),
        ];
        let tree = DomTree::new(nodes);
        let snapshot = ChallengeObserver::observe(&tree);
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().container_node_id, 1);
    }

    #[test]
    fn detect_container_by_iframe_src() {
        let nodes = vec![
            make_node(0, "div", &[], vec![1]),
            make_node(1, "iframe", &[("src", "https://hcaptcha.com/1/api2/anchor")], vec![]),
        ];
        let tree = DomTree::new(nodes);
        let snapshot = ChallengeObserver::observe(&tree);
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().container_node_id, 1);
    }

    #[test]
    fn extract_grid_layout() {
        let nodes = vec![
            make_node(0, "div", &[("class", "captcha-container")], vec![1, 2, 3, 4, 5, 6, 7, 8, 9]),
            make_node(1, "div", &[("class", "tile")], vec![]),
            make_node(2, "div", &[("class", "tile")], vec![]),
            make_node(3, "div", &[("class", "tile")], vec![]),
            make_node(4, "div", &[("class", "tile")], vec![]),
            make_node(5, "div", &[("class", "tile")], vec![]),
            make_node(6, "div", &[("class", "tile")], vec![]),
            make_node(7, "div", &[("class", "tile")], vec![]),
            make_node(8, "div", &[("class", "tile")], vec![]),
            make_node(9, "div", &[("class", "tile")], vec![]),
        ];
        let tree = DomTree::new(nodes);
        let snapshot = ChallengeObserver::observe(&tree).unwrap();
        let grid = snapshot.grid_layout.unwrap();
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.cols, 3);
        assert_eq!(grid.cell_node_ids.len(), 9);
    }

    #[test]
    fn classify_interactive_elements() {
        let nodes = vec![
            make_node(0, "div", &[("class", "recaptcha-container")], vec![1, 2, 3]),
            make_node(1, "input", &[("type", "checkbox"), ("role", "checkbox")], vec![]),
            make_node(2, "button", &[("class", "verify-btn")], vec![]),
            make_node(3, "img", &[("src", "challenge.png")], vec![]),
        ];
        let tree = DomTree::new(nodes);
        let snapshot = ChallengeObserver::observe(&tree).unwrap();
        assert_eq!(snapshot.interactive_elements.len(), 3);

        let roles: Vec<&str> = snapshot.interactive_elements.iter().map(|e| e.role.as_str()).collect();
        assert!(roles.contains(&"checkbox"));
        assert!(roles.contains(&"button"));
        assert!(roles.contains(&"image"));
    }

    #[test]
    fn extract_instruction_text() {
        let mut instr_node = make_node(1, "p", &[("class", "instructions")], vec![]);
        instr_node.text_content = "Select all images with traffic lights".to_string();

        let nodes = vec![
            make_node(0, "div", &[("class", "captcha-box")], vec![1]),
            instr_node,
        ];
        let tree = DomTree::new(nodes);
        let snapshot = ChallengeObserver::observe(&tree).unwrap();
        assert_eq!(
            snapshot.instruction_text.as_deref(),
            Some("Select all images with traffic lights")
        );
    }

    #[test]
    fn no_captcha_returns_none() {
        let nodes = vec![
            make_node(0, "div", &[("class", "normal-page")], vec![1]),
            make_node(1, "p", &[], vec![]),
        ];
        let tree = DomTree::new(nodes);
        assert!(ChallengeObserver::observe(&tree).is_none());
    }
}
