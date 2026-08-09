use crate::dom::MutationRecord;
use crate::dom::MutationType;
use std::collections::HashMap;

/// Mutation batcher with deduplication, old value recording, and microtask scheduling.
pub struct MutationBatcher {
    pub pending_mutations: Vec<MutationRecord>,
    /// Old attribute values for attributeChangedCallback support.
    old_values: HashMap<(usize, String), String>,
    /// Whether to record old values for attribute mutations.
    pub record_old_values: bool,
    /// Maximum batch size before forced flush.
    pub max_batch_size: usize,
    /// Total mutations processed.
    pub total_processed: u64,
}

impl Default for MutationBatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl MutationBatcher {
    pub fn new() -> Self {
        Self {
            pending_mutations: Vec::new(),
            old_values: HashMap::new(),
            record_old_values: false,
            max_batch_size: 1000,
            total_processed: 0,
        }
    }

    /// Create with old value recording enabled.
    pub fn with_old_values() -> Self {
        let mut b = Self::new();
        b.record_old_values = true;
        b
    }

    /// Push a mutation, deduplicating attribute mutations on the same node+attribute.
    pub fn push_mutation(&mut self, record: MutationRecord) {
        // Dedup: for attribute mutations on the same node+attribute, keep only the latest
        if record.mutation_type == MutationType::Attributes {
            if let Some(ref attr_name) = record.attribute_name {
                let key = (record.target_node_id, attr_name.clone());
                // Remove existing mutation for same node+attribute
                self.pending_mutations.retain(|r| {
                    !(r.mutation_type == MutationType::Attributes
                        && r.target_node_id == record.target_node_id
                        && r.attribute_name.as_deref() == Some(attr_name.as_str()))
                });
                // Preserve old value from first mutation if we have it
                let mut new_record = record;
                if self.record_old_values {
                    if let Some(old) = self.old_values.get(&key) {
                        new_record.old_value = Some(old.clone());
                    } else {
                        // Store current value as the "original" old value
                        if let Some(ref val) = new_record.old_value {
                            self.old_values.insert(key, val.clone());
                        }
                    }
                }
                self.pending_mutations.push(new_record);
                return;
            }
        }

        // For childList mutations, merge consecutive ones on the same target
        if record.mutation_type == MutationType::ChildList {
            if let Some(last) = self.pending_mutations.last_mut() {
                if last.mutation_type == MutationType::ChildList
                    && last.target_node_id == record.target_node_id
                {
                    last.added_nodes.extend(record.added_nodes.iter());
                    last.removed_nodes.extend(record.removed_nodes.iter());
                    return;
                }
            }
        }

        self.pending_mutations.push(record);
    }

    /// Record an old attribute value before mutation.
    pub fn record_old_attribute_value(&mut self, node_id: usize, attr_name: &str, old_value: &str) {
        self.old_values
            .insert((node_id, attr_name.to_string()), old_value.to_string());
    }

    /// Flush the batch, returning all pending mutations and clearing internal state.
    pub fn flush_batch(&mut self) -> Vec<MutationRecord> {
        let mutations = std::mem::take(&mut self.pending_mutations);
        self.total_processed += mutations.len() as u64;
        mutations
    }

    /// Check if the batch is full and needs flushing.
    pub fn is_full(&self) -> bool {
        self.pending_mutations.len() >= self.max_batch_size
    }

    /// Get pending mutation count.
    pub fn pending_count(&self) -> usize {
        self.pending_mutations.len()
    }

    /// Clear old value tracking.
    pub fn clear_old_values(&mut self) {
        self.old_values.clear();
    }

    /// Get mutation statistics.
    pub fn stats(&self) -> MutationBatcherStats {
        MutationBatcherStats {
            pending: self.pending_mutations.len(),
            total_processed: self.total_processed,
            tracked_old_values: self.old_values.len(),
        }
    }
}

/// Statistics about the mutation batcher.
#[derive(Debug, Clone)]
pub struct MutationBatcherStats {
    pub pending: usize,
    pub total_processed: u64,
    pub tracked_old_values: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr_mutation(node_id: usize, attr: &str, old: Option<&str>) -> MutationRecord {
        MutationRecord {
            mutation_type: MutationType::Attributes,
            target_node_id: node_id,
            attribute_name: Some(attr.to_string()),
            old_value: old.map(|s| s.to_string()),
            added_nodes: Vec::new(),
            removed_nodes: Vec::new(),
        }
    }

    fn child_mutation(node_id: usize, added: Vec<usize>, removed: Vec<usize>) -> MutationRecord {
        MutationRecord {
            mutation_type: MutationType::ChildList,
            target_node_id: node_id,
            attribute_name: None,
            old_value: None,
            added_nodes: added,
            removed_nodes: removed,
        }
    }

    #[test]
    fn test_push_and_flush() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(attr_mutation(1, "class", Some("old")));
        batcher.push_mutation(attr_mutation(2, "id", None));
        assert_eq!(batcher.pending_count(), 2);
        let flushed = batcher.flush_batch();
        assert_eq!(flushed.len(), 2);
        assert_eq!(batcher.pending_count(), 0);
    }

    #[test]
    fn test_attribute_dedup() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(attr_mutation(1, "class", Some("a")));
        batcher.push_mutation(attr_mutation(1, "class", Some("b")));
        batcher.push_mutation(attr_mutation(1, "class", Some("c")));
        assert_eq!(batcher.pending_count(), 1); // deduped
    }

    #[test]
    fn test_different_attrs_not_deduped() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(attr_mutation(1, "class", Some("a")));
        batcher.push_mutation(attr_mutation(1, "id", Some("b")));
        assert_eq!(batcher.pending_count(), 2);
    }

    #[test]
    fn test_childlist_merge() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(child_mutation(1, vec![2], vec![]));
        batcher.push_mutation(child_mutation(1, vec![3], vec![]));
        assert_eq!(batcher.pending_count(), 1); // merged
        let flushed = batcher.flush_batch();
        assert_eq!(flushed[0].added_nodes.len(), 2);
    }

    #[test]
    fn test_is_full() {
        let mut batcher = MutationBatcher::new();
        batcher.max_batch_size = 2;
        batcher.push_mutation(attr_mutation(1, "a", None));
        assert!(!batcher.is_full());
        batcher.push_mutation(attr_mutation(2, "b", None));
        assert!(batcher.is_full());
    }

    #[test]
    fn test_total_processed() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(attr_mutation(1, "a", None));
        batcher.push_mutation(attr_mutation(2, "b", None));
        batcher.flush_batch();
        assert_eq!(batcher.stats().total_processed, 2);
    }

    #[test]
    fn test_old_value_recording() {
        let mut batcher = MutationBatcher::with_old_values();
        batcher.record_old_attribute_value(1, "class", "original");
        batcher.push_mutation(attr_mutation(1, "class", Some("changed")));
        let flushed = batcher.flush_batch();
        assert_eq!(flushed[0].old_value.as_deref(), Some("original"));
    }

    #[test]
    fn test_clear_old_values() {
        let mut batcher = MutationBatcher::with_old_values();
        batcher.record_old_attribute_value(1, "class", "original");
        batcher.record_old_attribute_value(2, "id", "foo");
        assert_eq!(batcher.stats().tracked_old_values, 2);
        batcher.clear_old_values();
        assert_eq!(batcher.stats().tracked_old_values, 0);
    }

    #[test]
    fn test_stats_reports_correctly() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(attr_mutation(1, "a", None));
        batcher.push_mutation(attr_mutation(2, "b", None));
        let stats = batcher.stats();
        assert_eq!(stats.pending, 2);
        assert_eq!(stats.total_processed, 0);
        batcher.flush_batch();
        let stats = batcher.stats();
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.total_processed, 2);
    }

    #[test]
    fn test_different_nodes_not_deduped() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(attr_mutation(1, "class", Some("a")));
        batcher.push_mutation(attr_mutation(2, "class", Some("b")));
        assert_eq!(batcher.pending_count(), 2); // different nodes, not deduped
    }

    #[test]
    fn test_childlist_merge_different_targets_not_merged() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(child_mutation(1, vec![2], vec![]));
        batcher.push_mutation(child_mutation(2, vec![3], vec![])); // different target
        assert_eq!(batcher.pending_count(), 2); // not merged
    }

    #[test]
    fn test_childlist_merge_accumulates_removed() {
        let mut batcher = MutationBatcher::new();
        batcher.push_mutation(child_mutation(1, vec![], vec![5]));
        batcher.push_mutation(child_mutation(1, vec![], vec![6]));
        assert_eq!(batcher.pending_count(), 1);
        let flushed = batcher.flush_batch();
        assert_eq!(flushed[0].removed_nodes, vec![5, 6]);
    }

    #[test]
    fn test_default_batcher() {
        let batcher = MutationBatcher::default();
        assert_eq!(batcher.max_batch_size, 1000);
        assert!(!batcher.record_old_values);
        assert_eq!(batcher.pending_count(), 0);
    }
}
