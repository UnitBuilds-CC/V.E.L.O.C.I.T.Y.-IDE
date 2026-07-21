use crate::dom::DomTree;
use crate::nda::NdaTriple;
use crate::parser::html::NodeType;

pub struct AgenticAomNode {
    pub id: String,
    pub role: String,
    pub name: String,
    pub value: String,
    pub actionability: u8,
}

pub struct AgenticAomTree;

impl AgenticAomTree {
    pub fn build_aom_nodes(tree: &DomTree) -> Vec<AgenticAomNode> {
        let mut aom_nodes = Vec::new();

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            let role = match node.tag_name.as_str() {
                "button" => "button",
                "a" => "link",
                "input" => {
                    let type_attr = node.attributes.get("type").map(|s| s.as_str()).unwrap_or("text");
                    match type_attr {
                        "button" | "submit" => "button",
                        "checkbox" => "checkbox",
                        "radio" => "radio",
                        _ => "textbox",
                    }
                }
                "select" => "combobox",
                "textarea" => "textbox",
                "form" => "form",
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                _ => continue,
            };

            let name = node.attributes.get("aria-label")
                .cloned()
                .or_else(|| node.attributes.get("placeholder").cloned())
                .or_else(|| node.attributes.get("name").cloned())
                .or_else(|| node.attributes.get("id").cloned())
                .unwrap_or_default();

            let value = node.attributes.get("value").cloned().unwrap_or_default();

            let actionability = match role {
                "button" | "link" => 10,
                "textbox" | "checkbox" | "combobox" => 8,
                _ => 1,
            };

            aom_nodes.push(AgenticAomNode {
                id: format!("node_{}", node.id),
                role: role.to_string(),
                name,
                value,
                actionability,
            });
        }

        aom_nodes
    }

    pub fn to_nda_triples(aom_nodes: &[AgenticAomNode]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(aom_nodes.len() * 3);
        for node in aom_nodes {
            triples.push(NdaTriple::new(&node.id, 10, &node.role));
            if !node.name.is_empty() {
                triples.push(NdaTriple::new(&node.id, 11, &node.name));
            }
            if !node.value.is_empty() {
                triples.push(NdaTriple::new(&node.id, 12, &node.value));
            }
            triples.push(NdaTriple::new(&node.id, 13, &node.actionability.to_string()));
        }
        triples
    }
}
