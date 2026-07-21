use crate::parser::html::{DomNode, NodeType};

#[derive(Debug, Clone)]
pub struct DomTree {
    pub nodes: Vec<DomNode>,
}

impl DomTree {
    pub fn new(nodes: Vec<DomNode>) -> Self {
        Self { nodes }
    }

    pub fn get_node(&self, id: usize) -> Option<&DomNode> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut DomNode> {
        self.nodes.get_mut(id)
    }

    pub fn extract_page_title(&self) -> String {
        for node in &self.nodes {
            if node.node_type == NodeType::Element && node.tag_name == "title" {
                for &child_id in &node.children {
                    if let Some(child) = self.get_node(child_id) {
                        if child.node_type == NodeType::Text {
                            return child.text_content.clone();
                        }
                    }
                }
            }
        }
        "Untitled Page".to_string()
    }
}
