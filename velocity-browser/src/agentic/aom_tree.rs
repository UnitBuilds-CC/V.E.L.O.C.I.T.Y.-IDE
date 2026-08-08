use crate::dom::DomTree;
use crate::nda::{NdaDocument, NdaTriple};
use crate::parser::html::NodeType;
use crate::predicates::{
    AOM_ACTIONABILITY, AOM_EXPANDED, AOM_FOCUSED, AOM_NAME, AOM_ROLE, AOM_VALUE,
};

/// Recursively collect the visible text content of a node and its descendants,
/// mirroring DOM `textContent` semantics. Used as a fallback accessible name
/// when no explicit attribute (`aria-label`, `placeholder`, etc.) is present.
fn collect_inner_text(tree: &DomTree, node_id: usize) -> String {
    let mut buf = String::new();
    inner_text_walk(tree, node_id, &mut buf);
    let trimmed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed
}

fn inner_text_walk(tree: &DomTree, id: usize, out: &mut String) {
    if let Some(node) = tree.get_node(id) {
        if node.node_type == NodeType::Text {
            out.push_str(&node.text_content);
        }
        for &child in &node.children {
            inner_text_walk(tree, child, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(id: usize, tag: &str, attrs: &[(&str, &str)]) -> DomNode {
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
            children: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn button_role_and_actionability() {
        let tree = DomTree::new(vec![make_node(0, "button", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, "button");
        assert_eq!(nodes[0].actionability_score, 100);
    }

    #[test]
    fn link_role_and_actionability() {
        let tree = DomTree::new(vec![make_node(0, "a", &[("href", "/page")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, "link");
        assert_eq!(nodes[0].actionability_score, 100);
    }

    #[test]
    fn input_text_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "text")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "textbox");
        assert_eq!(nodes[0].actionability_score, 90);
    }

    #[test]
    fn input_checkbox_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "checkbox")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "checkbox");
        assert_eq!(nodes[0].actionability_score, 90);
    }

    #[test]
    fn heading_role() {
        let tree = DomTree::new(vec![make_node(0, "h1", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "heading");
        assert_eq!(nodes[0].actionability_score, 40);
    }

    #[test]
    fn generic_div_without_label_skipped() {
        let tree = DomTree::new(vec![make_node(0, "div", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert!(nodes.is_empty());
    }

    #[test]
    fn generic_div_with_aria_label_included() {
        let tree = DomTree::new(vec![make_node(0, "div", &[("aria-label", "sidebar")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "sidebar");
    }

    #[test]
    fn aria_label_overrides_visible_text() {
        let tree = DomTree::new(vec![make_node(0, "button", &[("aria-label", "Close dialog")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].name, "Close dialog");
    }

    #[test]
    fn to_nda_triples_includes_role_and_actionability() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "button".into(),
            name: "Submit".into(),
            value: String::new(),
            actionability_score: 100,
            is_focused: false,
            is_expanded: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        // role + name + actionability = 3
        assert_eq!(triples.len(), 3);
        let role_triple = triples.iter().find(|t| t.predicate_id == AOM_ROLE).unwrap();
        assert_eq!(role_triple.object_hash, crate::nda::hash_str("button"));
    }

    #[test]
    fn to_nda_triples_includes_focused() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "textbox".into(),
            name: "email".into(),
            value: String::new(),
            actionability_score: 90,
            is_focused: true,
            is_expanded: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        let focused = triples.iter().find(|t| t.predicate_id == AOM_FOCUSED);
        assert!(focused.is_some());
    }

    #[test]
    fn to_nda_triples_includes_expanded() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "combobox".into(),
            name: "select".into(),
            value: String::new(),
            actionability_score: 90,
            is_focused: false,
            is_expanded: true,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        let expanded = triples.iter().find(|t| t.predicate_id == AOM_EXPANDED);
        assert!(expanded.is_some());
    }

    #[test]
    fn to_nda_document_nonempty() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "button".into(),
            name: "OK".into(),
            value: "val".into(),
            actionability_score: 100,
            is_focused: false,
            is_expanded: false,
        }];
        let doc = AgenticAomTree::to_nda_document(&nodes);
        assert!(!doc.facts.is_empty());
    }

    #[test]
    fn input_submit_is_button_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "submit")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "button");
    }

    #[test]
    fn select_is_combobox() {
        let tree = DomTree::new(vec![make_node(0, "select", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "combobox");
    }

    #[test]
    fn value_attribute_captured() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "text"), ("value", "hello")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].value, "hello");
    }
}
#[derive(Debug, Clone)]
pub struct AgenticAomNode {
    pub id: String,
    pub role: String,
    pub name: String,
    pub value: String,
    pub actionability_score: u8,
    pub is_focused: bool,
    pub is_expanded: bool,
}

pub struct AgenticAomTree;

impl AgenticAomTree {
    pub fn build_aom_nodes(tree: &DomTree) -> Vec<AgenticAomNode> {
        let mut aom_nodes = Vec::new();

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            let explicit_role = node.attributes.get("role").map(|s| s.as_str());
            let role = explicit_role.unwrap_or_else(|| match node.tag_name.as_str() {
                "button" => "button",
                "a" => "link",
                "input" => {
                    let type_attr = node.attributes.get("type").map(|s| s.as_str()).unwrap_or("text");
                    match type_attr {
                        "button" | "submit" | "reset" => "button",
                        "checkbox" => "checkbox",
                        "radio" => "radio",
                        _ => "textbox",
                    }
                }
                "select" => "combobox",
                "textarea" => "textbox",
                "form" => "form",
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                "nav" => "navigation",
                "main" => "main",
                "article" => "article",
                "section" => "region",
                _ => "generic",
            });

            if role == "generic" && !node.attributes.contains_key("aria-label") && !node.attributes.contains_key("id") {
                continue;
            }

            // Buttons and links are known by what they show: visible text
            // beats name/id attributes (which are developer plumbing, not
            // what an agent reads on screen). aria-label still wins overall.
            let content_named = matches!(role, "button" | "link");
            let attr_name = if content_named {
                node.attributes.get("aria-label")
                    .cloned()
                    .or_else(|| node.attributes.get("title").cloned())
                    .filter(|s| !s.is_empty())
                    .or_else(|| Some(collect_inner_text(tree, node.id)).filter(|s| !s.is_empty()))
                    .or_else(|| node.attributes.get("name").cloned())
                    .or_else(|| node.attributes.get("id").cloned())
                    .unwrap_or_default()
            } else {
                node.attributes.get("aria-label")
                    .cloned()
                    .or_else(|| node.attributes.get("placeholder").cloned())
                    .or_else(|| node.attributes.get("name").cloned())
                    .or_else(|| node.attributes.get("id").cloned())
                    .or_else(|| node.attributes.get("title").cloned())
                    .unwrap_or_default()
            };
            let name = if attr_name.is_empty() {
                collect_inner_text(tree, node.id)
            } else {
                attr_name
            };

            let value = node.attributes.get("value").cloned().unwrap_or_default();
            let is_focused = node.attributes.contains_key("autofocus");
            let is_expanded = node.attributes.get("aria-expanded").map(|s| s == "true").unwrap_or(false);

            let actionability_score = match role {
                "button" | "link" => 100,
                "textbox" | "checkbox" | "radio" | "combobox" => 90,
                "form" => 75,
                "navigation" | "heading" => 40,
                _ => 10,
            };

            aom_nodes.push(AgenticAomNode {
                id: format!("node_{}", node.id),
                role: role.to_string(),
                name,
                value,
                actionability_score,
                is_focused,
                is_expanded,
            });
        }

        aom_nodes
    }

    pub fn to_nda_triples(aom_nodes: &[AgenticAomNode]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(aom_nodes.len() * 4);
        for node in aom_nodes {
            triples.push(NdaTriple::new(&node.id, AOM_ROLE, &node.role));
            if !node.name.is_empty() {
                triples.push(NdaTriple::new(&node.id, AOM_NAME, &node.name));
            }
            if !node.value.is_empty() {
                triples.push(NdaTriple::new(&node.id, AOM_VALUE, &node.value));
            }
            triples.push(NdaTriple::new(&node.id, AOM_ACTIONABILITY, &node.actionability_score.to_string()));
            if node.is_focused {
                triples.push(NdaTriple::new(&node.id, AOM_FOCUSED, "focused"));
            }
            if node.is_expanded {
                triples.push(NdaTriple::new(&node.id, AOM_EXPANDED, "expanded"));
            }
        }
        triples
    }

    /// Export the AOM as a lossless [`NdaDocument`] the agent can actually read:
    /// roles, names, and values survive as recoverable strings (not hashes).
    /// Facts are emitted in stable node/predicate order for easy diffing.
    pub fn to_nda_document(aom_nodes: &[AgenticAomNode]) -> NdaDocument {
        let mut doc = NdaDocument::new();
        for node in aom_nodes {
            doc.push_str(&node.id, AOM_ROLE, &node.role);
            if !node.name.is_empty() {
                doc.push_str(&node.id, AOM_NAME, &node.name);
            }
            if !node.value.is_empty() {
                doc.push_str(&node.id, AOM_VALUE, &node.value);
            }
            doc.push_int(&node.id, AOM_ACTIONABILITY, node.actionability_score as i64);
            if node.is_focused {
                doc.push_str(&node.id, AOM_FOCUSED, "focused");
            }
            if node.is_expanded {
                doc.push_str(&node.id, AOM_EXPANDED, "expanded");
            }
        }
        doc
    }
}
