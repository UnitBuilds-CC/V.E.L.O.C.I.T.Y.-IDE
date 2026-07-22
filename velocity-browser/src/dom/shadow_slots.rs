use crate::dom::DomTree;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SlotProjection {
    pub slot_name: String,
    pub assigned_nodes: Vec<usize>,
}

pub struct SlotProjectionEngine;

impl SlotProjectionEngine {
    pub fn project_slots(tree: &DomTree, host_node_id: usize) -> HashMap<String, SlotProjection> {
        let mut projections = HashMap::new();

        if let Some(host_node) = tree.get_node(host_node_id) {
            for &child_id in &host_node.children {
                if let Some(child_node) = tree.get_node(child_id) {
                    let slot_attr = child_node.attributes.get("slot").cloned().unwrap_or_default();
                    projections.entry(slot_attr.clone()).or_insert_with(|| SlotProjection {
                        slot_name: slot_attr,
                        assigned_nodes: Vec::new(),
                    }).assigned_nodes.push(child_id);
                }
            }
        }

        projections
    }
}
