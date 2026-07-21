use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Compact binary NDA representation for browser state & AOM layout facts
#[derive(Debug, Clone, PartialEq)]
pub struct NdaTriple {
    pub subject_hash: u64,
    pub predicate_id: u16,
    pub object_hash: u64,
}

impl NdaTriple {
    pub fn new(subject: &str, predicate: u16, object: &str) -> Self {
        Self {
            subject_hash: hash_str(subject),
            predicate_id: predicate,
            object_hash: hash_str(object),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(18);
        buf.extend_from_slice(&self.subject_hash.to_le_bytes());
        buf.extend_from_slice(&self.predicate_id.to_le_bytes());
        buf.extend_from_slice(&self.object_hash.to_le_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 18 {
            return Err("Buffer too small for NDA triple");
        }
        let subject_hash = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let predicate_id = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let object_hash = u64::from_le_bytes(bytes[10..18].try_into().unwrap());
        Ok(Self {
            subject_hash,
            predicate_id,
            object_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nda_triple_serialization() {
        let triple = NdaTriple::new("http://example.com", 1, "test_page");
        let bytes = triple.to_bytes();
        let decoded = NdaTriple::from_bytes(&bytes).unwrap();
        assert_eq!(triple, decoded);
    }
}
