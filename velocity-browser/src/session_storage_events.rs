use crate::nda::NdaTriple;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct StorageEventRecord {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub url: String,
}

pub struct StorageEventBroadcaster {
    pub history: Vec<StorageEventRecord>,
}

impl StorageEventBroadcaster {
    pub fn new() -> Self {
        Self { history: Vec::new() }
    }

    pub fn set_item(&mut self, storage: &mut HashMap<String, String>, key: &str, value: &str, url: &str) {
        let old = storage.insert(key.to_string(), value.to_string());
        self.history.push(StorageEventRecord {
            key: key.to_string(),
            old_value: old,
            new_value: Some(value.to_string()),
            url: url.to_string(),
        });
    }

    pub fn export_events_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for ev in &self.history {
            let val = ev.new_value.as_deref().unwrap_or("");
            triples.push(NdaTriple::new(&ev.key, 150, val));
        }
        triples
    }
}
