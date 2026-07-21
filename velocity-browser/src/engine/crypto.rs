use crate::nda::NdaTriple;

pub struct WebCryptoEngine;

impl WebCryptoEngine {
    pub fn digest_sha256(data: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub fn get_random_values(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 37 + 13) as u8).collect()
    }

    pub fn export_crypto_nda(session_id: &str, last_digest: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 180, last_digest),
            NdaTriple::new(session_id, 181, "crypto_subtle_ready"),
        ]
    }
}
