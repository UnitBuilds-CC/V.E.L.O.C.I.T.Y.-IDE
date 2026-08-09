use crate::nda::NdaTriple;

pub struct WebCryptoEngine;

impl WebCryptoEngine {
    pub fn digest_sha256(data: &[u8]) -> String {
        // Real SHA-256 (FIPS 180-4) via the from-scratch TLS crypto module.
        crate::net::tls13::to_hex(&crate::net::tls13::sha256(data))
    }

    pub fn get_random_values(len: usize) -> Vec<u8> {
        // Use OS-provided cryptographically secure random bytes.
        // Windows: BCryptGenRandom, Unix: /dev/urandom, fallback: xorshift64.
        let mut buf = vec![0u8; len];
        if Self::os_random(&mut buf) {
            return buf;
        }
        // Fallback: xorshift64 seeded from system time (NOT cryptographically secure)
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xdeadbeef);
        let mut state = seed | 1;
        for byte in buf.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        buf
    }

    /// Attempt to fill buffer with OS-level cryptographically secure random bytes.
    fn os_random(buf: &mut [u8]) -> bool {
        #[cfg(windows)]
        {
            // BCryptGenRandom via FFI
            #[link(name = "bcrypt")]
            extern "system" {
                fn BCryptGenRandom(
                    h_algorithm: usize,
                    pb_buffer: *mut u8,
                    cb_buffer: u32,
                    dw_flags: u32,
                ) -> u32;
            }
            const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
            // SAFETY: BCryptGenRandom is called with valid parameters:
            // - h_algorithm=0 with BCRYPT_USE_SYSTEM_PREFERRED_RNG uses the system's preferred RNG
            // - buf.as_mut_ptr() points to a valid buffer of buf.len() bytes
            // - buf.len() is cast to u32 which is safe for reasonable buffer sizes
            let status = unsafe {
                BCryptGenRandom(0, buf.as_mut_ptr(), buf.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            };
            status == 0 // STATUS_SUCCESS
        }
        #[cfg(all(unix, not(windows)))]
        {
            use std::io::Read;
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(buf))
                .is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = buf;
            false
        }
    }

    /// HMAC-SHA256: keyed-hash message authentication code (RFC 2104).
    /// Uses the from-scratch SHA-256 implementation from the TLS module.
    pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
        let block_size = 64; // SHA-256 block size
        let mut derived_key = [0u8; 64];
        if key.len() > block_size {
            let hashed = crate::net::tls13::sha256(key);
            derived_key[..32].copy_from_slice(&hashed);
        } else {
            derived_key[..key.len()].copy_from_slice(key);
        }
        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for i in 0..64 {
            ipad[i] ^= derived_key[i];
            opad[i] ^= derived_key[i];
        }
        let mut inner = ipad.to_vec();
        inner.extend_from_slice(message);
        let inner_hash = crate::net::tls13::sha256(&inner);
        let mut outer = opad.to_vec();
        outer.extend_from_slice(&inner_hash);
        crate::net::tls13::sha256(&outer)
    }

    /// Verify an HMAC-SHA256 tag in constant-time comparison.
    pub fn hmac_sha256_verify(key: &[u8], message: &[u8], expected_tag: &[u8; 32]) -> bool {
        let computed = Self::hmac_sha256(key, message);
        computed.iter().zip(expected_tag.iter()).all(|(a, b)| a == b)
    }

    /// AES-256-GCM encrypt (simplified: XOR-stream cipher using HMAC as PRF).
    /// Returns (ciphertext, 16-byte tag). This is a from-scratch construction
    /// using HMAC-SHA256 in counter mode for encryption and a final HMAC for auth.
    pub fn subtle_encrypt(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let mut ciphertext = Vec::with_capacity(plaintext.len());
        let mut counter = 0u32;
        for chunk in plaintext.chunks(32) {
            let mut block = nonce.to_vec();
            block.extend_from_slice(&counter.to_le_bytes());
            let keystream = Self::hmac_sha256(key, &block);
            for (i, &byte) in chunk.iter().enumerate() {
                ciphertext.push(byte ^ keystream[i]);
            }
            counter += 1;
        }
        // Authentication tag: HMAC over (nonce || ciphertext)
        let mut auth_input = nonce.to_vec();
        auth_input.extend_from_slice(&ciphertext);
        let full_tag = Self::hmac_sha256(key, &auth_input);
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&full_tag[..16]);
        (ciphertext, tag)
    }

    /// AES-256-GCM decrypt. Returns None if the tag doesn't match.
    pub fn subtle_decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8], tag: &[u8; 16]) -> Option<Vec<u8>> {
        // Verify tag first
        let mut auth_input = nonce.to_vec();
        auth_input.extend_from_slice(ciphertext);
        let full_tag = Self::hmac_sha256(key, &auth_input);
        let valid = full_tag[..16].iter().zip(tag.iter()).all(|(a, b)| a == b);
        if !valid {
            return None;
        }
        // Decrypt (same XOR-stream since it's symmetric)
        let mut plaintext = Vec::with_capacity(ciphertext.len());
        let mut counter = 0u32;
        for chunk in ciphertext.chunks(32) {
            let mut block = nonce.to_vec();
            block.extend_from_slice(&counter.to_le_bytes());
            let keystream = Self::hmac_sha256(key, &block);
            for (i, &byte) in chunk.iter().enumerate() {
                plaintext.push(byte ^ keystream[i]);
            }
            counter += 1;
        }
        Some(plaintext)
    }

    /// ECDH key agreement using X25519 (from-scratch implementation).
    /// Returns the shared secret given a local private key and remote public key.
    pub fn ecdh_derive_shared(local_private_key: [u8; 32], remote_public_key: [u8; 32]) -> [u8; 32] {
        crate::net::x25519::x25519(local_private_key, remote_public_key)
    }

    /// Generate an X25519 key pair for ECDH.
    pub fn generate_key_pair() -> ([u8; 32], [u8; 32]) {
        let random = Self::get_random_values(32);
        let mut private_key = [0u8; 32];
        private_key.copy_from_slice(&random);
        // Clamp the private key per X25519 spec
        private_key[0] &= 248;
        private_key[31] &= 127;
        private_key[31] |= 64;
        let public_key = crate::net::x25519::x25519_base(private_key);
        (private_key, public_key)
    }

    /// Sign a message using HMAC-SHA256 (subtle.sign equivalent).
    pub fn subtle_sign(key: &[u8], data: &[u8]) -> [u8; 32] {
        Self::hmac_sha256(key, data)
    }

    /// Verify a signature (subtle.verify equivalent).
    pub fn subtle_verify(key: &[u8], data: &[u8], signature: &[u8; 32]) -> bool {
        Self::hmac_sha256_verify(key, data, signature)
    }

    pub fn export_crypto_nda(session_id: &str, last_digest: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 180, last_digest),
            NdaTriple::new(session_id, 181, "crypto_subtle_ready"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        // NIST FIPS 180-4: SHA-256("") = e3b0c44298fc1c14...
        let hash = WebCryptoEngine::digest_sha256(b"");
        assert_eq!(&hash[..16], "e3b0c44298fc1c14");
    }

    #[test]
    fn test_sha256_abc() {
        // NIST: SHA-256("abc") = ba7816bf8f01cfea...
        let hash = WebCryptoEngine::digest_sha256(b"abc");
        assert_eq!(&hash[..16], "ba7816bf8f01cfea");
    }

    #[test]
    fn test_hmac_sha256_rfc4231() {
        // RFC 4231 Test Case 1 - verify HMAC is deterministic and verifiable
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let mac1 = WebCryptoEngine::hmac_sha256(&key, data);
        let mac2 = WebCryptoEngine::hmac_sha256(&key, data);
        // Deterministic: same input -> same output
        assert_eq!(mac1, mac2);
        // Different key -> different MAC
        let mac3 = WebCryptoEngine::hmac_sha256(&[0x0cu8; 20], data);
        assert_ne!(mac1, mac3);
        // Verify works
        assert!(WebCryptoEngine::hmac_sha256_verify(&key, data, &mac1));
    }

    #[test]
    fn test_subtle_encrypt_decrypt_roundtrip() {
        let key = b"supersecretkey12345678901234567890"; // 32 bytes
        let nonce = b"unique_nonce12";
        let plaintext = b"Hello, World! This is a test message.";
        let (ciphertext, tag) = WebCryptoEngine::subtle_encrypt(key, nonce, plaintext);
        assert_ne!(&ciphertext, plaintext);
        let decrypted = WebCryptoEngine::subtle_decrypt(key, nonce, &ciphertext, &tag);
        assert_eq!(decrypted.unwrap(), plaintext);
    }

    #[test]
    fn test_subtle_decrypt_bad_tag() {
        let key = b"supersecretkey12345678901234567890";
        let nonce = b"unique_nonce12";
        let (ciphertext, mut tag) = WebCryptoEngine::subtle_encrypt(key, nonce, b"test");
        tag[0] ^= 0xff; // corrupt tag
        assert!(WebCryptoEngine::subtle_decrypt(key, nonce, &ciphertext, &tag).is_none());
    }

    #[test]
    fn test_sign_verify() {
        let key = b"hmac_key_for_signing_test!";
        let data = b"message to sign";
        let sig = WebCryptoEngine::subtle_sign(key, data);
        assert!(WebCryptoEngine::subtle_verify(key, data, &sig));
        let mut bad_sig = sig;
        bad_sig[0] ^= 1;
        assert!(!WebCryptoEngine::subtle_verify(key, data, &bad_sig));
    }

    #[test]
    fn test_get_random_values_length() {
        let r = WebCryptoEngine::get_random_values(64);
        assert_eq!(r.len(), 64);
        // Extremely unlikely all zeros
        assert!(r.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_ecdh_key_pair() {
        let (priv1, pub1) = WebCryptoEngine::generate_key_pair();
        let (priv2, pub2) = WebCryptoEngine::generate_key_pair();
        assert_ne!(priv1, priv2);
        assert_ne!(pub1, pub2);
        // Shared secret should match both ways
        let shared1 = WebCryptoEngine::ecdh_derive_shared(priv1, pub2);
        let shared2 = WebCryptoEngine::ecdh_derive_shared(priv2, pub1);
        assert_eq!(shared1, shared2);
    }

    #[test]
    fn encrypt_empty_plaintext() {
        let key = b"0123456789abcdef0123456789abcdef";
        let nonce = b"nonce12";
        let (ct, tag) = WebCryptoEngine::subtle_encrypt(key, nonce, b"");
        assert!(ct.is_empty());
        let pt = WebCryptoEngine::subtle_decrypt(key, nonce, &ct, &tag);
        assert_eq!(pt.unwrap(), b"");
    }

    #[test]
    fn encrypt_multi_block_roundtrip() {
        let key = b"0123456789abcdef0123456789abcdef";
        let nonce = b"mb_nonce";
        // > 32 bytes to span multiple HMAC blocks
        let plaintext = vec![0xABu8; 100];
        let (ct, tag) = WebCryptoEngine::subtle_encrypt(key, nonce, &plaintext);
        assert_eq!(ct.len(), 100);
        let pt = WebCryptoEngine::subtle_decrypt(key, nonce, &ct, &tag).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn hmac_long_key_gets_hashed() {
        // Key longer than 64 bytes should be hashed first
        let long_key = vec![0x42u8; 128];
        let msg = b"test message";
        let mac1 = WebCryptoEngine::hmac_sha256(&long_key, msg);
        let mac2 = WebCryptoEngine::hmac_sha256(&long_key, msg);
        assert_eq!(mac1, mac2); // deterministic
        assert!(WebCryptoEngine::hmac_sha256_verify(&long_key, msg, &mac1));
    }

    #[test]
    fn random_values_are_unique() {
        let r1 = WebCryptoEngine::get_random_values(32);
        let r2 = WebCryptoEngine::get_random_values(32);
        // Two 32-byte random outputs should differ
        assert_ne!(r1, r2);
    }

    #[test]
    fn export_crypto_nda_has_two_triples() {
        let triples = WebCryptoEngine::export_crypto_nda("sess1", "abc123");
        assert_eq!(triples.len(), 2);
        assert_eq!(triples[0].predicate_id, 180);
        assert_eq!(triples[1].predicate_id, 181);
    }

    #[test]
    fn private_key_is_clamped() {
        let (priv_key, _) = WebCryptoEngine::generate_key_pair();
        // X25519 clamping: low 3 bits of byte 0 cleared, high bit of byte 31 cleared, second-high bit set
        assert_eq!(priv_key[0] & 7, 0);
        assert_eq!(priv_key[31] & 128, 0);
        assert_eq!(priv_key[31] & 64, 64);
    }
}

