//! Virtual objects — Restate-style batchable state containers.
//!
//! Virtual objects encapsulate state that can be mutated by workflow steps.
//! Instead of persisting each mutation immediately (fsync per step), mutations
//! are collected and committed in batches, dramatically reducing I/O overhead.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::{VirtualObjectId, StateMutation, MutationOp};

/// A virtual object — a named, keyed state container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualObject {
    pub id: VirtualObjectId,
    pub name: String,
    state: HashMap<String, serde_json::Value>,
    version: u64,
    dirty: bool,
}

impl VirtualObject {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: VirtualObjectId::new(),
            name: name.into(),
            state: HashMap::new(),
            version: 0,
            dirty: false,
        }
    }

    pub fn with_id(mut self, id: VirtualObjectId) -> Self {
        self.id = id;
        self
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.state.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.state.insert(key.into(), value);
        self.dirty = true;
    }

    pub fn delete(&mut self, key: &str) -> Option<serde_json::Value> {
        let removed = self.state.remove(key);
        if removed.is_some() { self.dirty = true; }
        removed
    }

    pub fn increment(&mut self, key: impl Into<String>, delta: i64) -> i64 {
        let key = key.into();
        let current = self.state.get(&key)
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let new_val = current + delta;
        self.state.insert(key, serde_json::Value::from(new_val));
        self.dirty = true;
        new_val
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.state.keys()
    }

    pub fn state(&self) -> &HashMap<String, serde_json::Value> {
        &self.state
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Apply a mutation to this object.
    pub fn apply_mutation(&mut self, mutation: &StateMutation) {
        match &mutation.operation {
            MutationOp::Set { key, value } => { self.set(key.clone(), value.clone()); }
            MutationOp::Delete { key } => { self.delete(key); }
            MutationOp::Increment { key, delta } => { self.increment(key.clone(), *delta); }
            MutationOp::Append { key, value } => {
                let entry = self.state.entry(key.clone()).or_insert_with(|| serde_json::json!([]));
                if let Some(arr) = entry.as_array_mut() {
                    arr.push(value.clone());
                    self.dirty = true;
                }
            }
        }
    }

    /// Mark as persisted (called after batch commit).
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.version += 1;
    }
}

/// Registry of virtual objects for a workflow run.
#[derive(Debug, Default)]
pub struct VirtualObjectStore {
    objects: HashMap<VirtualObjectId, VirtualObject>,
}

impl VirtualObjectStore {
    pub fn new() -> Self { Self { objects: HashMap::new() } }

    pub fn register(&mut self, obj: VirtualObject) {
        self.objects.insert(obj.id.clone(), obj);
    }

    pub fn get(&self, id: &VirtualObjectId) -> Option<&VirtualObject> {
        self.objects.get(id)
    }

    pub fn get_mut(&mut self, id: &VirtualObjectId) -> Option<&mut VirtualObject> {
        self.objects.get_mut(id)
    }

    /// Apply a batch of mutations, returning the IDs of affected objects.
    pub fn apply_mutations(&mut self, mutations: &[StateMutation]) -> Vec<VirtualObjectId> {
        let mut affected = Vec::new();
        for m in mutations {
            if let Some(obj) = self.objects.get_mut(&m.object_id) {
                obj.apply_mutation(m);
                if !affected.contains(&m.object_id) {
                    affected.push(m.object_id.clone());
                }
            }
        }
        affected
    }

    /// Get all dirty objects (pending commit).
    pub fn dirty_objects(&self) -> Vec<&VirtualObject> {
        self.objects.values().filter(|o| o.is_dirty()).collect()
    }

    /// Mark all objects as clean (after batch commit).
    pub fn mark_all_clean(&mut self) {
        for obj in self.objects.values_mut() {
            obj.mark_clean();
        }
    }

    /// Number of dirty objects pending commit.
    pub fn dirty_count(&self) -> usize {
        self.objects.values().filter(|o| o.is_dirty()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_object_set_and_get() {
        let mut obj = VirtualObject::new("counter");
        obj.set("count", serde_json::json!(42));
        assert_eq!(obj.get("count").unwrap(), &serde_json::json!(42));
        assert!(obj.is_dirty());
    }

    #[test]
    fn virtual_object_increment() {
        let mut obj = VirtualObject::new("counter");
        assert_eq!(obj.increment("count", 5), 5);
        assert_eq!(obj.increment("count", 3), 8);
    }

    #[test]
    fn virtual_object_mark_clean() {
        let mut obj = VirtualObject::new("test");
        obj.set("key", serde_json::json!("value"));
        assert!(obj.is_dirty());
        obj.mark_clean();
        assert!(!obj.is_dirty());
        assert_eq!(obj.version(), 1);
    }

    #[test]
    fn store_apply_mutations() {
        let mut store = VirtualObjectStore::new();
        let obj_id = VirtualObjectId::from_str("obj-1");
        store.register(VirtualObject::new("test").with_id(obj_id.clone()));
        let mutations = vec![
            StateMutation {
                object_id: obj_id.clone(),
                operation: MutationOp::Set { key: "x".into(), value: serde_json::json!(10) },
            },
        ];
        let affected = store.apply_mutations(&mutations);
        assert_eq!(affected.len(), 1);
        assert_eq!(store.get(&obj_id).unwrap().get("x").unwrap(), &serde_json::json!(10));
        assert_eq!(store.dirty_count(), 1);
    }
}
