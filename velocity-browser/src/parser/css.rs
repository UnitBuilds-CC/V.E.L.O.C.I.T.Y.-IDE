use crate::parser::html::{DomNode, NodeType};

pub struct CssMatcher;

impl CssMatcher {
    /// Match a compound CSS selector against a list of DOM nodes
    pub fn find_matches<'a>(nodes: &'a [DomNode], selector: &str) -> Vec<&'a DomNode> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();
        for node in nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            if Self::matches_node(node, selector) {
                matches.push(node);
            }
        }

        matches
    }

    fn matches_node(node: &DomNode, selector: &str) -> bool {
        // Multi-selector split by comma
        if selector.contains(',') {
            return selector.split(',').any(|s| Self::matches_node(node, s.trim()));
        }

        // Space-separated descendant selector (match last element)
        let parts: Vec<&str> = selector.split_whitespace().collect();
        let target_part = parts.last().cloned().unwrap_or(selector);

        // Attribute selector e.g. [attr=val] or [attr="val"]
        if target_part.starts_with('[') && target_part.ends_with(']') {
            let inner = &target_part[1..target_part.len() - 1];
            if let Some((k, v)) = inner.split_once('=') {
                let attr_key = k.trim();
                let attr_val = v.trim().trim_matches('"').trim_matches('\'');
                return node.attributes.get(attr_key).map(|s| s.as_str()) == Some(attr_val);
            } else {
                return node.attributes.contains_key(inner.trim());
            }
        }

        // ID selector e.g. #id
        if let Some(id) = target_part.strip_prefix('#') {
            return node.attributes.get("id").map(|s| s.as_str()) == Some(id);
        }

        // Class selector e.g. .class
        if let Some(class) = target_part.strip_prefix('.') {
            if let Some(classes) = node.attributes.get("class") {
                return classes.split_whitespace().any(|c| c == class);
            }
            return false;
        }

        // Tag name selector e.g. button, input, div
        if !target_part.is_empty() && !target_part.contains('[') && !target_part.contains('#') && !target_part.contains('.') {
            return node.tag_name.eq_ignore_ascii_case(target_part);
        }

        // Compound tag#id.class or tag[attr=val]
        let mut matched = true;
        let mut current = target_part;

        if let Some(attr_start) = current.find('[') {
            if let Some(attr_end) = current.find(']') {
                let inner = &current[attr_start + 1..attr_end];
                if let Some((k, v)) = inner.split_once('=') {
                    let attr_key = k.trim();
                    let attr_val = v.trim().trim_matches('"').trim_matches('\'');
                    if node.attributes.get(attr_key).map(|s| s.as_str()) != Some(attr_val) {
                        matched = false;
                    }
                } else if !node.attributes.contains_key(inner.trim()) {
                    matched = false;
                }
                current = &current[..attr_start];
            }
        }

        if matched && current.contains('#') {
            if let Some((_, id)) = current.split_once('#') {
                let clean_id = id.split('.').next().unwrap_or(id);
                if node.attributes.get("id").map(|s| s.as_str()) != Some(clean_id) {
                    matched = false;
                }
            }
        }

        if matched && current.contains('.') {
            for class in current.split('.').skip(1) {
                let clean_class = class.split('#').next().unwrap_or(class);
                if let Some(classes) = node.attributes.get("class") {
                    if !classes.split_whitespace().any(|c| c == clean_class) {
                        matched = false;
                    }
                } else {
                    matched = false;
                }
            }
        }

        let tag_part = current.split('#').next().unwrap_or(current).split('.').next().unwrap_or(current);
        if matched && !tag_part.is_empty()
            && !node.tag_name.eq_ignore_ascii_case(tag_part) {
                matched = false;
            }

        matched
    }
}

/// Match a single DOM node against a selector, supporting pseudo-classes.
pub fn matches_pseudo(node: &DomNode, pseudo: &str) -> bool {
    match pseudo {
        ":first-child" => {
            // True if this node is the first element child of its parent
            node.parent.is_some_and(|_| {
                // Simplified: check if node has no previous element sibling
                true // Caller must verify with tree context
            })
        }
        ":last-child" => {
            node.parent.is_some_and(|_| true)
        }
        ":root" => node.tag_name.eq_ignore_ascii_case("html") && node.parent.is_none(),
        ":empty" => node.children.is_empty() && node.text_content.trim().is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(tag: &str, attrs: Vec<(&str, &str)>) -> DomNode {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }
        DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes,
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn test_tag_selector() {
        let node = make_node("div", vec![]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_tag_selector_case_insensitive() {
        let node = make_node("DIV", vec![]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_class_selector() {
        let node = make_node("div", vec![("class", "active")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, ".active");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_class_selector_multiple_classes() {
        let node = make_node("div", vec![("class", "foo active bar")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, ".active");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_id_selector() {
        let node = make_node("div", vec![("id", "main")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "#main");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_attribute_selector_with_value() {
        let node = make_node("input", vec![("type", "text")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "[type=text]");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_attribute_selector_presence() {
        let node = make_node("input", vec![("disabled", "")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "[disabled]");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_comma_separated_selectors() {
        let node = make_node("span", vec![]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div, span");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_compound_tag_class() {
        let node = make_node("div", vec![("class", "active")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div.active");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_compound_tag_id() {
        let node = make_node("div", vec![("id", "main")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div#main");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_no_match_wrong_tag() {
        let node = make_node("span", vec![]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_no_match_wrong_class() {
        let node = make_node("div", vec![("class", "foo")]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, ".bar");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_empty_selector() {
        let node = make_node("div", vec![]);
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_skips_text_nodes() {
        let mut node = make_node("div", vec![]);
        node.node_type = NodeType::Text;
        let nodes = vec![node];
        let matches = CssMatcher::find_matches(&nodes, "div");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_matches_pseudo_root() {
        let node = make_node("html", vec![]);
        assert!(matches_pseudo(&node, ":root"));
    }

    #[test]
    fn test_matches_pseudo_empty() {
        let node = make_node("div", vec![]);
        assert!(matches_pseudo(&node, ":empty"));
    }

    #[test]
    fn test_matches_pseudo_empty_false() {
        let mut node = make_node("div", vec![]);
        node.text_content = "hello".to_string();
        assert!(!matches_pseudo(&node, ":empty"));
    }
}
