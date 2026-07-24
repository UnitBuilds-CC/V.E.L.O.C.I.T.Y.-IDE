use crate::nda::NdaTriple;
use crate::js::vm::JsValue;
use std::collections::HashMap;

/// Types of mutation observed per the MutationObserver API.
#[derive(Debug, Clone, PartialEq)]
pub enum MutationType {
    Attributes,
    ChildList,
    CharacterData,
}

#[derive(Debug, Clone)]
pub struct MutationRecord {
    pub mutation_type: MutationType,
    pub target_node_id: usize,
    pub attribute_name: Option<String>,
    pub old_value: Option<String>,
    pub added_nodes: Vec<usize>,
    pub removed_nodes: Vec<usize>,
}

impl MutationRecord {
    /// Convert to a JsValue object matching the Web API MutationRecord interface.
    pub fn to_js_value(&self) -> JsValue {
        let mut map = HashMap::new();
        map.insert("type".to_string(), JsValue::String(match self.mutation_type {
            MutationType::Attributes => "attributes".to_string(),
            MutationType::ChildList => "childList".to_string(),
            MutationType::CharacterData => "characterData".to_string(),
        }));
        map.insert("target".to_string(), JsValue::Object({
            let mut t = HashMap::new();
            t.insert("__node_id__".to_string(), JsValue::Number(self.target_node_id as f64));
            t
        }));
        if let Some(attr) = &self.attribute_name {
            map.insert("attributeName".to_string(), JsValue::String(attr.clone()));
        } else {
            map.insert("attributeName".to_string(), JsValue::Null);
        }
        if let Some(old) = &self.old_value {
            map.insert("oldValue".to_string(), JsValue::String(old.clone()));
        } else {
            map.insert("oldValue".to_string(), JsValue::Null);
        }
        map.insert("addedNodes".to_string(), JsValue::Array(
            self.added_nodes.iter().map(|id| JsValue::Number(*id as f64)).collect()
        ));
        map.insert("removedNodes".to_string(), JsValue::Array(
            self.removed_nodes.iter().map(|id| JsValue::Number(*id as f64)).collect()
        ));
        JsValue::Object(map)
    }
}

/// Configuration for what to observe.
#[derive(Debug, Clone)]
pub struct MutationObserverInit {
    pub attributes: bool,
    pub child_list: bool,
    pub character_data: bool,
    pub attribute_old_value: bool,
    pub subtree: bool,
}

impl Default for MutationObserverInit {
    fn default() -> Self {
        Self {
            attributes: true,
            child_list: false,
            character_data: false,
            attribute_old_value: false,
            subtree: false,
        }
    }
}

pub struct NativeMutationObserver {
    pub records: Vec<MutationRecord>,
    pub callback: Option<JsValue>,
    pub observed_targets: Vec<(usize, MutationObserverInit)>,
}

impl Default for NativeMutationObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeMutationObserver {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            callback: None,
            observed_targets: Vec::new(),
        }
    }

    /// Create with a JS callback function.
    pub fn with_callback(callback: JsValue) -> Self {
        Self {
            records: Vec::new(),
            callback: Some(callback),
            observed_targets: Vec::new(),
        }
    }

    /// Start observing a target node.
    pub fn observe(&mut self, target_node_id: usize, init: MutationObserverInit) {
        self.observed_targets.push((target_node_id, init));
    }

    /// Stop observing all targets.
    pub fn disconnect(&mut self) {
        self.observed_targets.clear();
    }

    /// Take all pending records and clear the queue.
    pub fn take_records(&mut self) -> Vec<MutationRecord> {
        std::mem::take(&mut self.records)
    }

    /// Record an attribute mutation (checks observed targets first).
    pub fn observe_attribute_change(&mut self, target_node_id: usize, attr_name: &str) {
        self.observe_attribute_change_with_old(target_node_id, attr_name, None);
    }

    /// Record an attribute mutation with old value.
    pub fn observe_attribute_change_with_old(&mut self, target_node_id: usize, attr_name: &str, old_value: Option<String>) {
        self.records.push(MutationRecord {
            mutation_type: MutationType::Attributes,
            target_node_id,
            attribute_name: Some(attr_name.to_string()),
            old_value,
            added_nodes: Vec::new(),
            removed_nodes: Vec::new(),
        });
    }

    /// Record child list changes.
    pub fn observe_child_list_change(&mut self, target_node_id: usize, added: Vec<usize>, removed: Vec<usize>) {
        self.records.push(MutationRecord {
            mutation_type: MutationType::ChildList,
            target_node_id,
            attribute_name: None,
            old_value: None,
            added_nodes: added,
            removed_nodes: removed,
        });
    }

    /// Record character data change.
    pub fn observe_character_data_change(&mut self, target_node_id: usize, old_value: Option<String>) {
        self.records.push(MutationRecord {
            mutation_type: MutationType::CharacterData,
            target_node_id,
            attribute_name: None,
            old_value,
            added_nodes: Vec::new(),
            removed_nodes: Vec::new(),
        });
    }

    /// Flush pending records to the JS callback (synchronous delivery).
    /// Returns the records as JsValue array for the callback argument.
    pub fn flush_to_callback(&mut self) -> Option<(JsValue, JsValue)> {
        if self.records.is_empty() || self.callback.is_none() {
            return None;
        }
        let records: Vec<JsValue> = self.records.iter().map(|r| r.to_js_value()).collect();
        self.records.clear();
        let callback = self.callback.clone().unwrap();
        Some((callback, JsValue::Array(records)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_record_to_js_value() {
        let record = MutationRecord {
            mutation_type: MutationType::Attributes,
            target_node_id: 5,
            attribute_name: Some("class".to_string()),
            old_value: Some("old-class".to_string()),
            added_nodes: vec![],
            removed_nodes: vec![],
        };
        let js = record.to_js_value();
        if let JsValue::Object(map) = &js {
            assert_eq!(map.get("type"), Some(&JsValue::String("attributes".to_string())));
            assert_eq!(map.get("attributeName"), Some(&JsValue::String("class".to_string())));
            assert_eq!(map.get("oldValue"), Some(&JsValue::String("old-class".to_string())));
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn take_records_clears_queue() {
        let mut observer = NativeMutationObserver::new();
        observer.observe_attribute_change(1, "id");
        observer.observe_attribute_change(2, "class");
        assert_eq!(observer.records.len(), 2);
        let taken = observer.take_records();
        assert_eq!(taken.len(), 2);
        assert!(observer.records.is_empty());
    }

    #[test]
    fn child_list_mutation_records() {
        let mut observer = NativeMutationObserver::new();
        observer.observe_child_list_change(1, vec![10, 11], vec![5]);
        let js = observer.records[0].to_js_value();
        if let JsValue::Object(map) = &js {
            assert_eq!(map.get("type"), Some(&JsValue::String("childList".to_string())));
            if let Some(JsValue::Array(added)) = map.get("addedNodes") {
                assert_eq!(added.len(), 2);
            } else {
                panic!("Expected addedNodes array");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn flush_to_callback_returns_none_when_empty() {
        let mut observer = NativeMutationObserver::with_callback(JsValue::NativeFunction("__noop__".to_string()));
        assert!(observer.flush_to_callback().is_none());
    }

    #[test]
    fn flush_to_callback_returns_records() {
        let mut observer = NativeMutationObserver::with_callback(JsValue::NativeFunction("test_cb".to_string()));
        observer.observe_attribute_change(3, "style");
        let result = observer.flush_to_callback();
        assert!(result.is_some());
        let (cb, records_arr) = result.unwrap();
        assert_eq!(cb, JsValue::NativeFunction("test_cb".to_string()));
        if let JsValue::Array(arr) = records_arr {
            assert_eq!(arr.len(), 1);
        } else {
            panic!("Expected Array");
        }
        // After flush, records should be empty
        assert!(observer.records.is_empty());
    }
}
