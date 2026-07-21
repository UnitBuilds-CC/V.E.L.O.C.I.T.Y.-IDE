use crate::dom::DomTree;
use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct AgenticAomNode {
    pub id: usize,
    pub role: String,
    pub name: String,
    pub description: String,
    pub is_interactive: bool,
}

pub struct AgenticAomTree;

impl AgenticAomTree {
    pub fn build_aom_nodes(tree: &DomTree) -> Vec<AgenticAomNode> {
        let mut nodes = Vec::new();
        for n in &tree.nodes {
            let role = n.attributes.get("role").cloned().unwrap_or_else(|| n.tag_name.clone());
            let name = n.attributes.get("aria-label").cloned().unwrap_or_else(|| n.text_content.clone());
            let is_interactive = matches!(n.tag_name.as_str(), "button" | "a" | "input" | "select" | "textarea");
            nodes.push(AgenticAomNode {
                id: n.id,
                role,
                name,
                description: String::new(),
                is_interactive,
            });
        }
        nodes
    }

    pub fn to_nda_triples(nodes: &[AgenticAomNode]) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for node in nodes {
            let subject = format!("node_{}", node.id);
            triples.push(NdaTriple::new(&subject, 10, &node.role));
            triples.push(NdaTriple::new(&subject, 11, &node.name));
            triples.push(NdaTriple::new(&subject, 12, if node.is_interactive { "true" } else { "false" }));
        }
        triples
    }
}
