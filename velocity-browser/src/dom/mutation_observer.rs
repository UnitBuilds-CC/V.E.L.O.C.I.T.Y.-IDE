use crate::dom::DomTree;
use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct MutationRecord {
    pub target_node_id: usize,
    pub attribute_name: Option<String>,
    pub added_nodes: Vec<usize>,
    pub removed_nodes: Vec<usize>,
}

pub struct NativeMutationObserver {
    pub records: Vec<MutationRecord>,
}

impl NativeMutationObserver {
    pub fn new() -> Self {
        Self { records: Vec::new() }
    }

    pub fn observe_attribute_change(&mut self, target_node_id: usize, attr_name: &str) {
        self.records.push(MutationRecord {
            target_node_id,
            attribute_name: Some(attr_name.to_string()),
            added_nodes: Vec::new(),
            removed_nodes: Vec::new(),
        });
    }

    pub fn export_mutations_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for r in &self.records {
            let target = format!("node_{}", r.target_node_id);
            let attr = r.attribute_name.as_deref().unwrap_or("subtree");
            triples.push(NdaTriple::new(&target, 140, attr));
        }
        triples
    }
}
