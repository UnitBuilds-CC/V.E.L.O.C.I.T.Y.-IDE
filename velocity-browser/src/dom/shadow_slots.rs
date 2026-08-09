use crate::dom::DomTree;
use std::collections::HashMap;

/// A slot projection mapping light DOM nodes to shadow DOM slots.
#[derive(Debug, Clone)]
pub struct SlotProjection {
    pub slot_name: String,
    pub assigned_nodes: Vec<usize>,
    /// Fallback content when no nodes are assigned.
    pub fallback_content: Option<String>,
}

/// Engine for projecting light DOM nodes into shadow DOM slots.
pub struct SlotProjectionEngine;

impl SlotProjectionEngine {
    /// Project slots for a shadow host, mapping light DOM children to named slots.
    pub fn project_slots(tree: &DomTree, host_node_id: usize) -> HashMap<String, SlotProjection> {
        let mut projections = HashMap::new();

        if let Some(host_node) = tree.get_node(host_node_id) {
            for &child_id in &host_node.children {
                if let Some(child_node) = tree.get_node(child_id) {
                    let slot_name = child_node
                        .attributes
                        .get("slot")
                        .cloned()
                        .unwrap_or_default();
                    projections
                        .entry(slot_name.clone())
                        .or_insert_with(|| SlotProjection {
                            slot_name,
                            assigned_nodes: Vec::new(),
                            fallback_content: None,
                        })
                        .assigned_nodes
                        .push(child_id);
                }
            }
        }

        projections
    }

    /// Get the default slot (unnamed slot) assignments.
    pub fn default_slot(tree: &DomTree, host_node_id: usize) -> Vec<usize> {
        let mut default_nodes = Vec::new();
        if let Some(host_node) = tree.get_node(host_node_id) {
            for &child_id in &host_node.children {
                if let Some(child_node) = tree.get_node(child_id) {
                    if !child_node.attributes.contains_key("slot") {
                        default_nodes.push(child_id);
                    }
                }
            }
        }
        default_nodes
    }

    /// Set fallback content for a slot when no nodes are assigned.
    pub fn set_fallback(
        projections: &mut HashMap<String, SlotProjection>,
        slot_name: &str,
        content: &str,
    ) {
        if let Some(proj) = projections.get_mut(slot_name) {
            if proj.assigned_nodes.is_empty() {
                proj.fallback_content = Some(content.to_string());
            }
        }
    }

    /// Generate slotchange event data for slots that changed.
    pub fn detect_slot_changes(
        old: &HashMap<String, SlotProjection>,
        new: &HashMap<String, SlotProjection>,
    ) -> Vec<String> {
        let mut changed = Vec::new();
        for (name, new_proj) in new {
            let old_proj = old.get(name);
            let old_nodes = old_proj
                .map(|p| &p.assigned_nodes)
                .cloned()
                .unwrap_or_default();
            if old_nodes != new_proj.assigned_nodes {
                changed.push(name.clone());
            }
        }
        // Check for removed slots
        for name in old.keys() {
            if !new.contains_key(name) {
                changed.push(name.clone());
            }
        }
        changed
    }

    /// Get all assigned node IDs across all slots.
    pub fn all_assigned_nodes(projections: &HashMap<String, SlotProjection>) -> Vec<usize> {
        projections
            .values()
            .flat_map(|p| p.assigned_nodes.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DomTree;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_tree_with_host() -> (DomTree, usize) {
        let host_id = 0;
        let nodes = vec![
            DomNode {
                id: 0,
                node_type: NodeType::Element,
                tag_name: "my-component".to_string(),
                attributes: HashMap::new(),
                text_content: String::new(),
                children: vec![1, 2, 3],
                parent: None,
            },
            DomNode {
                id: 1,
                node_type: NodeType::Element,
                tag_name: "span".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("slot".to_string(), "header".to_string());
                    m
                },
                text_content: "Header".to_string(),
                children: Vec::new(),
                parent: Some(0),
            },
            DomNode {
                id: 2,
                node_type: NodeType::Element,
                tag_name: "div".to_string(),
                attributes: HashMap::new(),
                text_content: "Default content".to_string(),
                children: Vec::new(),
                parent: Some(0),
            },
            DomNode {
                id: 3,
                node_type: NodeType::Element,
                tag_name: "span".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("slot".to_string(), "footer".to_string());
                    m
                },
                text_content: "Footer".to_string(),
                children: Vec::new(),
                parent: Some(0),
            },
        ];
        (DomTree::new(nodes), host_id)
    }

    #[test]
    fn test_project_named_slots() {
        let (tree, host_id) = make_tree_with_host();
        let projections = SlotProjectionEngine::project_slots(&tree, host_id);
        assert!(projections.contains_key("header"));
        assert!(projections.contains_key("footer"));
        assert_eq!(projections["header"].assigned_nodes.len(), 1);
    }

    #[test]
    fn test_default_slot() {
        let (tree, host_id) = make_tree_with_host();
        let default = SlotProjectionEngine::default_slot(&tree, host_id);
        assert_eq!(default.len(), 1);
        assert_eq!(default[0], 2);
    }

    #[test]
    fn test_fallback_content() {
        let mut projections = HashMap::new();
        projections.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: Vec::new(),
                fallback_content: None,
            },
        );
        SlotProjectionEngine::set_fallback(&mut projections, "header", "Default Header");
        assert_eq!(
            projections["header"].fallback_content.as_deref(),
            Some("Default Header")
        );
    }

    #[test]
    fn test_no_fallback_when_assigned() {
        let mut projections = HashMap::new();
        projections.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: vec![1],
                fallback_content: None,
            },
        );
        SlotProjectionEngine::set_fallback(&mut projections, "header", "Default Header");
        assert!(projections["header"].fallback_content.is_none());
    }

    #[test]
    fn test_detect_changes() {
        let mut old = HashMap::new();
        old.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: vec![1],
                fallback_content: None,
            },
        );
        let mut new = HashMap::new();
        new.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: vec![1, 2],
                fallback_content: None,
            },
        );
        let changed = SlotProjectionEngine::detect_slot_changes(&old, &new);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0], "header");
    }

    #[test]
    fn test_all_assigned_nodes() {
        let mut projections = HashMap::new();
        projections.insert(
            "a".to_string(),
            SlotProjection {
                slot_name: "a".to_string(),
                assigned_nodes: vec![1, 2],
                fallback_content: None,
            },
        );
        projections.insert(
            "b".to_string(),
            SlotProjection {
                slot_name: "b".to_string(),
                assigned_nodes: vec![3],
                fallback_content: None,
            },
        );
        let all = SlotProjectionEngine::all_assigned_nodes(&projections);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_empty_host_produces_no_slots() {
        let nodes = vec![DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: "my-component".to_string(),
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        }];
        let tree = DomTree::new(nodes);
        let projections = SlotProjectionEngine::project_slots(&tree, 0);
        assert!(projections.is_empty());
    }

    #[test]
    fn test_detect_changes_removed_slot() {
        let mut old = HashMap::new();
        old.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: vec![1],
                fallback_content: None,
            },
        );
        old.insert(
            "footer".to_string(),
            SlotProjection {
                slot_name: "footer".to_string(),
                assigned_nodes: vec![2],
                fallback_content: None,
            },
        );
        let new = HashMap::new(); // all slots removed
        let changed = SlotProjectionEngine::detect_slot_changes(&old, &new);
        assert_eq!(changed.len(), 2);
    }

    #[test]
    fn test_detect_changes_no_change() {
        let mut old = HashMap::new();
        old.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: vec![1],
                fallback_content: None,
            },
        );
        let mut new = HashMap::new();
        new.insert(
            "header".to_string(),
            SlotProjection {
                slot_name: "header".to_string(),
                assigned_nodes: vec![1],
                fallback_content: None,
            },
        );
        let changed = SlotProjectionEngine::detect_slot_changes(&old, &new);
        assert!(changed.is_empty());
    }

    #[test]
    fn test_all_assigned_nodes_empty_projections() {
        let projections = HashMap::new();
        let all = SlotProjectionEngine::all_assigned_nodes(&projections);
        assert!(all.is_empty());
    }

    #[test]
    fn test_multiple_nodes_same_slot() {
        let nodes = vec![
            DomNode {
                id: 0,
                node_type: NodeType::Element,
                tag_name: "host".to_string(),
                attributes: HashMap::new(),
                text_content: String::new(),
                children: vec![1, 2],
                parent: None,
            },
            DomNode {
                id: 1,
                node_type: NodeType::Element,
                tag_name: "span".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("slot".to_string(), "header".to_string());
                    m
                },
                text_content: String::new(),
                children: Vec::new(),
                parent: Some(0),
            },
            DomNode {
                id: 2,
                node_type: NodeType::Element,
                tag_name: "div".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("slot".to_string(), "header".to_string());
                    m
                },
                text_content: String::new(),
                children: Vec::new(),
                parent: Some(0),
            },
        ];
        let tree = DomTree::new(nodes);
        let projections = SlotProjectionEngine::project_slots(&tree, 0);
        assert_eq!(projections["header"].assigned_nodes.len(), 2);
    }

    #[test]
    fn test_fallback_not_set_for_nonexistent_slot() {
        let mut projections = HashMap::new();
        // Setting fallback for a slot that doesn't exist should do nothing
        SlotProjectionEngine::set_fallback(&mut projections, "nonexistent", "fallback");
        assert!(!projections.contains_key("nonexistent"));
    }
}
