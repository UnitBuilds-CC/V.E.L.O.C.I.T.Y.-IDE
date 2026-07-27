use crate::nda::NdaTriple;

/// A node in the Accessibility Object Model (AOM) tree.
#[derive(Debug, Clone)]
pub struct SpatialNode {
    pub id: String,
    pub role: String,
    pub name: String,
    /// ARIA properties.
    pub aria_label: Option<String>,
    pub aria_hidden: bool,
    pub aria_disabled: bool,
    pub aria_checked: Option<bool>,
    pub aria_expanded: Option<bool>,
    pub aria_level: Option<u32>,
    pub aria_value_now: Option<f64>,
    pub aria_value_min: Option<f64>,
    pub aria_value_max: Option<f64>,
    /// Bounding box for spatial reasoning.
    pub bounds: Option<(f32, f32, f32, f32)>, // x, y, w, h
    /// Child node IDs.
    pub children: Vec<String>,
}

/// AOM extractor producing NDA triples from accessibility tree nodes.
pub struct AomExtractor;

impl AomExtractor {
    /// Extract triples from spatial nodes.
    pub fn extract_triples(nodes: &[SpatialNode]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(nodes.len() * 4);
        for node in nodes {
            if node.aria_hidden { continue; }
            triples.push(NdaTriple::new(&node.id, 10, &node.role));
            if !node.name.is_empty() {
                triples.push(NdaTriple::new(&node.id, 11, &node.name));
            }
            if let Some(ref label) = node.aria_label {
                triples.push(NdaTriple::new(&node.id, 12, label));
            }
            if node.aria_disabled {
                triples.push(NdaTriple::new(&node.id, 13, "disabled"));
            }
            if let Some(checked) = node.aria_checked {
                triples.push(NdaTriple::new(&node.id, 14, if checked { "true" } else { "false" }));
            }
            if let Some(expanded) = node.aria_expanded {
                triples.push(NdaTriple::new(&node.id, 15, if expanded { "true" } else { "false" }));
            }
            if let Some(level) = node.aria_level {
                triples.push(NdaTriple::new(&node.id, 16, &level.to_string()));
            }
            if let Some(val) = node.aria_value_now {
                triples.push(NdaTriple::new(&node.id, 17, &val.to_string()));
            }
            if let Some((x, y, w, h)) = node.bounds {
                triples.push(NdaTriple::new(&node.id, 18, &format!("{:.0},{:.0},{:.0},{:.0}", x, y, w, h)));
            }
            for child_id in &node.children {
                triples.push(NdaTriple::new(&node.id, 19, child_id));
            }
        }
        triples
    }

    /// Serialize the accessibility tree to a flat text representation.
    pub fn to_accessibility_text(nodes: &[SpatialNode]) -> String {
        let mut text = String::new();
        for node in nodes {
            if node.aria_hidden { continue; }
            text.push_str(&format!("[{}] role={} name=\"{}\"", node.id, node.role, node.name));
            if node.aria_disabled { text.push_str(" disabled"); }
            if let Some(checked) = node.aria_checked {
                text.push_str(&format!(" checked={}", checked));
            }
            if let Some((x, y, w, h)) = node.bounds {
                text.push_str(&format!(" bounds=({:.0},{:.0},{:.0},{:.0})", x, y, w, h));
            }
            text.push('\n');
        }
        text
    }

    /// Find nodes matching a role.
    pub fn find_by_role<'a>(nodes: &'a [SpatialNode], role: &str) -> Vec<&'a SpatialNode> {
        nodes.iter().filter(|n| n.role == role && !n.aria_hidden).collect()
    }

    /// Find a node by ID.
    pub fn find_by_id<'a>(nodes: &'a [SpatialNode], id: &str) -> Option<&'a SpatialNode> {
        nodes.iter().find(|n| n.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, role: &str, name: &str) -> SpatialNode {
        SpatialNode {
            id: id.to_string(), role: role.to_string(), name: name.to_string(),
            aria_label: None, aria_hidden: false, aria_disabled: false,
            aria_checked: None, aria_expanded: None, aria_level: None,
            aria_value_now: None, aria_value_min: None, aria_value_max: None,
            bounds: None, children: Vec::new(),
        }
    }

    #[test]
    fn test_extract_triples() {
        let nodes = vec![make_node("n1", "button", "Submit")];
        let triples = AomExtractor::extract_triples(&nodes);
        assert!(triples.len() >= 2); // role + name
    }

    #[test]
    fn test_hidden_skipped() {
        let mut node = make_node("n1", "button", "Hidden");
        node.aria_hidden = true;
        let triples = AomExtractor::extract_triples(&[node]);
        assert_eq!(triples.len(), 0);
    }

    #[test]
    fn test_aria_properties() {
        let mut node = make_node("n1", "checkbox", "Accept");
        node.aria_checked = Some(true);
        node.aria_disabled = true;
        node.aria_label = Some("Accept terms".to_string());
        let triples = AomExtractor::extract_triples(&[node]);
        assert!(triples.len() >= 4); // role + name + label + checked + disabled
    }

    #[test]
    fn test_bounds() {
        let mut node = make_node("n1", "button", "Click");
        node.bounds = Some((10.0, 20.0, 100.0, 30.0));
        let triples = AomExtractor::extract_triples(&[node]);
        assert!(triples.iter().any(|t| t.predicate_id == 18));
    }

    #[test]
    fn test_children() {
        let mut parent = make_node("p1", "list", "Items");
        parent.children = vec!["c1".to_string(), "c2".to_string()];
        let triples = AomExtractor::extract_triples(&[parent]);
        assert!(triples.iter().filter(|t| t.predicate_id == 19).count() == 2);
    }

    #[test]
    fn test_accessibility_text() {
        let nodes = vec![make_node("n1", "button", "Submit")];
        let text = AomExtractor::to_accessibility_text(&nodes);
        assert!(text.contains("button"));
        assert!(text.contains("Submit"));
    }

    #[test]
    fn test_find_by_role() {
        let nodes = vec![
            make_node("n1", "button", "A"),
            make_node("n2", "link", "B"),
            make_node("n3", "button", "C"),
        ];
        let buttons = AomExtractor::find_by_role(&nodes, "button");
        assert_eq!(buttons.len(), 2);
    }

    #[test]
    fn test_find_by_id() {
        let nodes = vec![make_node("n1", "button", "A")];
        assert!(AomExtractor::find_by_id(&nodes, "n1").is_some());
        assert!(AomExtractor::find_by_id(&nodes, "n2").is_none());
    }
}
