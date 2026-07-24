use crate::nda::NdaTriple;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct IndexedDbRecord {
    pub key: String,
    pub payload_json: String,
}

pub struct IndexedDbStorage {
    pub db_name: String,
    pub object_stores: HashMap<String, Vec<IndexedDbRecord>>,
}

impl IndexedDbStorage {
    pub fn new(db_name: &str) -> Self {
        Self {
            db_name: db_name.to_string(),
            object_stores: HashMap::new(),
        }
    }

    pub fn put_item(&mut self, store_name: &str, key: &str, payload_json: &str) {
        let store = self.object_stores.entry(store_name.to_string()).or_default();
        if let Some(existing) = store.iter_mut().find(|r| r.key == key) {
            existing.payload_json = payload_json.to_string();
        } else {
            store.push(IndexedDbRecord {
                key: key.to_string(),
                payload_json: payload_json.to_string(),
            });
        }
    }

    pub fn export_indexeddb_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for (store, records) in &self.object_stores {
            for r in records {
                let subject = format!("{}:{}", store, r.key);
                triples.push(NdaTriple::new(&subject, 160, &r.payload_json));
            }
        }
        triples
    }
}
