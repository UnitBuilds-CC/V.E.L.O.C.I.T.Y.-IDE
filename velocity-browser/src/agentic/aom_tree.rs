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

            let attr_name = node.attributes.get("aria-label")
                .cloned()
                .or_else(|| node.attributes.get("placeholder").cloned())
                .or_else(|| node.attributes.get("name").cloned())
                .or_else(|| node.attributes.get("id").cloned())
                .or_else(|| node.attributes.get("title").cloned())
                .unwrap_or_default();
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
