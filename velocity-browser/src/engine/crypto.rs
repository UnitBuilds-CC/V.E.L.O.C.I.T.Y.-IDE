use crate::nda::NdaTriple;

pub struct WebCryptoEngine;

impl WebCryptoEngine {
    pub fn digest_sha256(data: &[u8]) -> String {
        // Real SHA-256 (FIPS 180-4) via the from-scratch TLS crypto module.
        crate::net::tls13::to_hex(&crate::net::tls13::sha256(data))
    }

    pub fn get_random_values(len: usize) -> Vec<u8> {
        // Use a simple xorshift64 PRNG seeded from system time.
        // Not cryptographically secure, but far better than deterministic.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xdeadbeef);
        let mut state = seed | 1; // must be non-zero
        (0..len).map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        }).collect()
    }

    pub fn export_crypto_nda(session_id: &str, last_digest: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 180, last_digest),
            NdaTriple::new(session_id, 181, "crypto_subtle_ready"),
        ]
    }
}
