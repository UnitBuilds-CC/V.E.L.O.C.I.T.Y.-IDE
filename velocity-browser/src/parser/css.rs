use crate::parser::html::{DomNode, NodeType};

pub struct CssMatcher;

impl CssMatcher {
    /// Match a CSS selector against a list of DOM nodes
    pub fn find_matches<'a>(nodes: &'a [DomNode], selector: &str) -> Vec<&'a DomNode> {
        let selector = selector.trim();
        let mut matches = Vec::new();

        for node in nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            if selector.starts_with('#') {
                let id = &selector[1..];
                if let Some(node_id) = node.attributes.get("id") {
                    if node_id == id {
                        matches.push(node);
                    }
                }
            } else if selector.starts_with('.') {
                let class = &selector[1..];
                if let Some(classes) = node.attributes.get("class") {
                    if classes.split_whitespace().any(|c| c == class) {
                        matches.push(node);
                    }
                }
            } else if selector.contains("[name=") {
                if let Some(start) = selector.find("[name=\"") {
                    let val_start = start + 7;
                    if let Some(end) = selector[val_start..].find('"') {
                        let name_val = &selector[val_start..val_start + end];
                        if let Some(node_name) = node.attributes.get("name") {
                            if node_name == name_val {
                                matches.push(node);
                            }
                        }
                    }
                } else if let Some(start) = selector.find("[name=") {
                    let val_start = start + 6;
                    let val_end = selector[val_start..].find(']').unwrap_or(selector.len() - val_start);
                    let name_val = selector[val_start..val_start + val_end].trim_matches('"').trim_matches('\'');
                    if let Some(node_name) = node.attributes.get("name") {
                        if node_name == name_val {
                            matches.push(node);
                        }
                    }
                }
            } else if node.tag_name.eq_ignore_ascii_case(selector) {
                matches.push(node);
            }
        }

        matches
    }
}
