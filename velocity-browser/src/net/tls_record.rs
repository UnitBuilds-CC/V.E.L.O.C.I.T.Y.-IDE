//! TLS 1.3 record protection (RFC 8446 §5.2–5.3).
//!
//! This ties the verified primitives together into the operation the record
//! layer actually performs: given a traffic key/IV (from the key schedule) and
//! a monotonically increasing sequence number, protect or unprotect one record
//! with an AEAD.
//!
//! Only ChaCha20-Poly1305 is wired here for now. AES-128-GCM plugs into the
//! same shape once its core exists; `AeadAlg` selects between them.

use crate::net::chacha20poly1305;

/// The AEAD algorithm negotiated for the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AeadAlg {
    /// TLS_CHACHA20_POLY1305_SHA256 — 32-byte key, 16-byte tag.
    ChaCha20Poly1305,
}

/// Construct the per-record nonce (RFC 8446 §5.3):
/// the 64-bit sequence number is encoded big-endian, left-padded with zeros to
/// the IV length, then XORed with the static write IV.
pub fn per_record_nonce(write_iv: &[u8; 12], seq: u64) -> [u8; 12] {
    let mut nonce = *write_iv;
    let seq_bytes = seq.to_be_bytes(); // 8 bytes
    // XOR into the low-order (rightmost) 8 bytes of the 12-byte IV.
    for i in 0..8 {
        nonce[4 + i] ^= seq_bytes[i];
    }
    nonce
}

/// Seal one record: encrypt `plaintext` under `key`/`write_iv` at `seq`, binding
/// `additional_data` (the record header) into the tag. Returns
/// `ciphertext || tag`.
pub fn seal_record(
    alg: AeadAlg,
    key: &[u8],
    write_iv: &[u8; 12],
    seq: u64,
    additional_data: &[u8],
    plaintext: &[u8],
) -> Vec<u8> {
    let nonce = per_record_nonce(write_iv, seq);
    match alg {
        AeadAlg::ChaCha20Poly1305 => {
            let mut k = [0u8; 32];
            k.copy_from_slice(key);
            let (mut ct, tag) =
                chacha20poly1305::aead_encrypt(&k, &nonce, additional_data, plaintext);
            ct.extend_from_slice(&tag);
            ct
        }
    }
}

/// Open one record: verify + decrypt `ciphertext_and_tag` (`ciphertext || tag`).
/// Returns `None` on any authentication failure.
pub fn open_record(
    alg: AeadAlg,
    key: &[u8],
    write_iv: &[u8; 12],
    seq: u64,
    additional_data: &[u8],
    ciphertext_and_tag: &[u8],
) -> Option<Vec<u8>> {
    if ciphertext_and_tag.len() < 16 {
        return None;
    }
    let nonce = per_record_nonce(write_iv, seq);
    let split = ciphertext_and_tag.len() - 16;
    let (ct, tag_slice) = ciphertext_and_tag.split_at(split);
    match alg {
        AeadAlg::ChaCha20Poly1305 => {
            let mut k = [0u8; 32];
            k.copy_from_slice(key);
            let mut tag = [0u8; 16];
            tag.copy_from_slice(tag_slice);
            chacha20poly1305::aead_decrypt(&k, &nonce, additional_data, ct, &tag)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_at_seq_zero_is_the_static_iv() {
        let iv = [
            0x5d, 0x31, 0x3e, 0xb2, 0x67, 0x12, 0x76, 0xee, 0x13, 0x00, 0x0b, 0x30,
        ];
        assert_eq!(per_record_nonce(&iv, 0), iv);
    }

    #[test]
    fn nonce_xors_sequence_into_low_bytes() {
        let iv = [0u8; 12];
        // seq = 1 -> only the last byte flips to 0x01.
        let n1 = per_record_nonce(&iv, 1);
        let mut expected = [0u8; 12];
        expected[11] = 0x01;
        assert_eq!(n1, expected);

        // A multi-byte sequence lands big-endian in the rightmost 8 bytes.
        let n = per_record_nonce(&iv, 0x0102_0304_0506_0708);
        assert_eq!(
            n,
            [0, 0, 0, 0, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
    }

    #[test]
    fn seal_then_open_round_trips() {
        let key = [0x42u8; 32];
        let iv = [0x24u8; 12];
        let aad = [0x17, 0x03, 0x03, 0x00, 0x2a]; // application_data record header
        let plaintext = b"agent record payload";

        let sealed = seal_record(AeadAlg::ChaCha20Poly1305, &key, &iv, 7, &aad, plaintext);
        assert!(sealed.len() > plaintext.len()); // includes the 16-byte tag
        let opened =
            open_record(AeadAlg::ChaCha20Poly1305, &key, &iv, 7, &aad, &sealed).unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn wrong_sequence_number_fails_to_open() {
        let key = [0x42u8; 32];
        let iv = [0x24u8; 12];
        let aad = [0x17, 0x03, 0x03, 0x00, 0x2a];
        let sealed = seal_record(AeadAlg::ChaCha20Poly1305, &key, &iv, 7, &aad, b"secret");
        // Opening at a different sequence number changes the nonce -> auth fails.
        assert!(open_record(AeadAlg::ChaCha20Poly1305, &key, &iv, 8, &aad, &sealed).is_none());
    }

    #[test]
    fn tampered_header_fails_to_open() {
        let key = [0x42u8; 32];
        let iv = [0x24u8; 12];
        let aad = [0x17, 0x03, 0x03, 0x00, 0x2a];
        let sealed = seal_record(AeadAlg::ChaCha20Poly1305, &key, &iv, 1, &aad, b"secret");
        let bad_aad = [0x17, 0x03, 0x03, 0x00, 0x2b]; // one byte differs
        assert!(open_record(AeadAlg::ChaCha20Poly1305, &key, &iv, 1, &bad_aad, &sealed).is_none());
    }
}
