//! TLS 1.3 cryptographic foundation (from scratch, no third-party crates).
//!
//! This module implements the verified building blocks of the TLS 1.3 key
//! schedule: a real SHA-256, HMAC-SHA256 (RFC 2104), HKDF (RFC 5869), and the
//! TLS 1.3 `HKDF-Expand-Label` / `Derive-Secret` functions (RFC 8446 §7.1).
//!
//! These are unit-tested against the published RFC test vectors. They replace
//! the previous placeholder "SHA-256" (which was a non-cryptographic hash).
//! The remaining TLS 1.3 pieces - X25519 key exchange, an AEAD cipher, X.509
//! certificate parsing/validation, and the handshake state machine - build on
//! this foundation and are tracked as subsequent work. Nothing here fabricates
//! a completed handshake.

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 (FIPS 180-4).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Lowercase hex encoding.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);

    let mut outer = Vec::with_capacity(96);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// HKDF-Extract (RFC 5869).
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    hmac_sha256(salt, ikm)
}

/// HKDF-Expand (RFC 5869).
pub fn hkdf_expand(prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let mut okm = Vec::with_capacity(len);
    let mut t: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while okm.len() < len {
        let mut data = Vec::with_capacity(t.len() + info.len() + 1);
        data.extend_from_slice(&t);
        data.extend_from_slice(info);
        data.push(counter);
        let block = hmac_sha256(prk, &data);
        t = block.to_vec();
        okm.extend_from_slice(&block);
        counter = counter.wrapping_add(1);
    }
    okm.truncate(len);
    okm
}

/// TLS 1.3 `HKDF-Expand-Label` (RFC 8446 §7.1).
pub fn hkdf_expand_label(secret: &[u8], label: &str, context: &[u8], len: usize) -> Vec<u8> {
    let full_label = format!("tls13 {}", label);
    let mut info = Vec::new();
    info.extend_from_slice(&(len as u16).to_be_bytes());
    info.push(full_label.len() as u8);
    info.extend_from_slice(full_label.as_bytes());
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    hkdf_expand(secret, &info, len)
}

/// TLS 1.3 `Derive-Secret` (RFC 8446 §7.1).
pub fn derive_secret(secret: &[u8], label: &str, messages: &[u8]) -> [u8; 32] {
    let transcript_hash = sha256(messages);
    let out = hkdf_expand_label(secret, label, &transcript_hash, 32);
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}

// ===== TLS 1.3 key schedule (RFC 8446 §7.1) ================================
//
// The schedule is a chain of HKDF-Extract / Derive-Secret steps that turns the
// (EC)DHE shared secret into the traffic secrets, then per-record key + IV.
// All steps below compose the verified primitives above; they are unit-tested
// against the RFC 8448 example handshake trace.

/// Early Secret = HKDF-Extract(0, PSK). With no PSK, the IKM is a string of
/// `Hash.length` zero bytes (RFC 8446 §7.1).
pub fn derive_early_secret(psk: Option<&[u8]>) -> [u8; 32] {
    let zeros = [0u8; 32];
    let ikm = psk.unwrap_or(&zeros);
    hkdf_extract(&zeros, ikm)
}

/// Handshake Secret = HKDF-Extract(Derive-Secret(early, "derived", ""), ECDHE).
pub fn derive_handshake_secret(early_secret: &[u8; 32], ecdhe: &[u8]) -> [u8; 32] {
    let derived = derive_secret(early_secret, "derived", &[]);
    hkdf_extract(&derived, ecdhe)
}

/// Master Secret = HKDF-Extract(Derive-Secret(handshake, "derived", ""), 0).
pub fn derive_master_secret(handshake_secret: &[u8; 32]) -> [u8; 32] {
    let derived = derive_secret(handshake_secret, "derived", &[]);
    let zeros = [0u8; 32];
    hkdf_extract(&derived, &zeros)
}

/// Derive the per-record AEAD write key and IV from a traffic secret
/// (RFC 8446 §7.3). `key_len` is 16 for AES-128-GCM, 32 for ChaCha20-Poly1305;
/// the IV is always 12 bytes.
pub fn traffic_key_iv(traffic_secret: &[u8], key_len: usize) -> (Vec<u8>, Vec<u8>) {
    let key = hkdf_expand_label(traffic_secret, "key", &[], key_len);
    let iv = hkdf_expand_label(traffic_secret, "iv", &[], 12);
    (key, iv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn sha256_abc_vector() {
        assert_eq!(
            to_hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_empty_vector() {
        assert_eq!(
            to_hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_multiblock_vector() {
        // 56-byte message forces a second padding block.
        let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            to_hex(&sha256(msg)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_case1() {
        let key = [0x0bu8; 20];
        let mac = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            to_hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hkdf_rfc5869_case1() {
        let ikm = [0x0bu8; 22];
        let salt = from_hex("000102030405060708090a0b0c");
        let info = from_hex("f0f1f2f3f4f5f6f7f8f9");
        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(
            to_hex(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );
        let okm = hkdf_expand(&prk, &info, 42);
        assert_eq!(
            to_hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn expand_label_is_deterministic_and_sized() {
        let secret = [0x01u8; 32];
        let a = hkdf_expand_label(&secret, "key", b"", 16);
        let b = hkdf_expand_label(&secret, "key", b"", 16);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        // Different labels must yield different key material.
        let c = hkdf_expand_label(&secret, "iv", b"", 16);
        assert_ne!(a, c);
    }

    // ===== RFC 8448 (Example Handshake Traces for TLS 1.3) =================

    #[test]
    fn rfc8448_early_secret() {
        // Early Secret with no PSK is a fixed, well-known constant.
        assert_eq!(
            to_hex(&derive_early_secret(None)),
            "33ad0a1c607ec03b09e6cd9893680ce210adf300aa1f2660e1b22e10f170f92a"
        );
    }

    #[test]
    fn rfc8448_derived_secret() {
        // Derive-Secret(early, "derived", "") from RFC 8448 §3.
        let early = derive_early_secret(None);
        assert_eq!(
            to_hex(&derive_secret(&early, "derived", &[])),
            "6f2615a108c702c5678f54fc9dbab69716c076189c48250cebeac3576c3611ba"
        );
    }

    #[test]
    fn rfc8448_handshake_secret() {
        // Handshake Secret = HKDF-Extract(derived, ECDHE), RFC 8448 §3.
        let early = derive_early_secret(None);
        let ecdhe = from_hex("8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d");
        assert_eq!(
            to_hex(&derive_handshake_secret(&early, &ecdhe)),
            "1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac"
        );
    }

    #[test]
    fn rfc8448_server_handshake_traffic_key_iv() {
        // Given the server handshake traffic secret from RFC 8448 §3, the
        // derived AES-128-GCM write key and IV must match the trace.
        let s_hs_traffic =
            from_hex("b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38");
        let (key, iv) = traffic_key_iv(&s_hs_traffic, 16);
        assert_eq!(to_hex(&key), "3fce516009c21727d0f2e4e86ee403bc");
        assert_eq!(to_hex(&iv), "5d313eb2671276ee13000b30");
    }
}
