use crate::nda::NdaTriple;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct SessionStorageDisk {
    pub storage_dir: String,
}

impl SessionStorageDisk {
    pub fn new(storage_dir: &str) -> Self {
        let _ = fs::create_dir_all(storage_dir);
        Self {
            storage_dir: storage_dir.to_string(),
        }
    }

    pub fn save_session_nda(&self, session_id: &str, triples: &[NdaTriple]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let file_path = format!("{}/{}.nda", self.storage_dir, session_id);
        let mut bytes = Vec::with_capacity(triples.len() * 18);
        for t in triples {
            bytes.extend_from_slice(&t.to_bytes());
        }
        fs::write(&file_path, bytes)?;
        Ok(file_path)
    }

    pub fn load_session_nda(&self, session_id: &str) -> Result<Vec<NdaTriple>, Box<dyn std::error::Error + Send + Sync>> {
        let file_path = format!("{}/{}.nda", self.storage_dir, session_id);
        let bytes = fs::read(&file_path)?;

        if bytes.len() % 18 != 0 {
            return Err("Invalid NDA binary session file length".into());
        }

        let count = bytes.len() / 18;
        let mut triples = Vec::with_capacity(count);
        for i in 0..count {
            let offset = i * 18;
            let triple = NdaTriple::from_bytes(&bytes[offset..offset + 18])?;
            triples.push(triple);
        }
        Ok(triples)
    }
}
