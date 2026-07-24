//! ChaCha20-Poly1305 AEAD (RFC 8439), from scratch.
//!
//! `TLS_CHACHA20_POLY1305_SHA256` is one of TLS 1.3's mandatory cipher suites.
//! This module implements the ChaCha20 stream cipher, the Poly1305 one-time
//! authenticator, and the combined AEAD construction, verified below against
//! the published RFC 8439 test vectors (§2.5.2, §2.6.2, §2.8.2). It is a real
//! authenticated cipher - encryption produces a genuine Poly1305 tag and
//! decryption rejects tampered ciphertext.

// ===== ChaCha20 (RFC 8439 §2) ==============================================

#[inline]
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(7);
}

fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Generate one 64-byte ChaCha20 keystream block.
fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;
    for i in 0..8 {
        state[4 + i] = read_u32_le(key, i * 4);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = read_u32_le(nonce, i * 4);
    }

    let mut working = state;
    for _ in 0..10 {
        // Column rounds.
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);
        // Diagonal rounds.
        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let v = working[i].wrapping_add(state[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encrypt (or decrypt) `data` by XORing the ChaCha20 keystream, starting at
/// the given block `counter`.
pub fn chacha20_encrypt(key: &[u8; 32], counter: u32, nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut block_counter = counter;
    for chunk in data.chunks(64) {
        let ks = chacha20_block(key, block_counter, nonce);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
        block_counter = block_counter.wrapping_add(1);
    }
    out
}

// ===== Poly1305 (RFC 8439 §2.5) ============================================

/// Compute the Poly1305 one-time authenticator tag for `msg` under `key`.
/// Uses the 5-limb (radix-2^26) reference arithmetic mod 2^130-5.
pub fn poly1305_mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    // Clamp r.
    let r0 = read_u32_le(key, 0) & 0x3ffffff;
    let r1 = (read_u32_le(key, 3) >> 2) & 0x3ffff03;
    let r2 = (read_u32_le(key, 6) >> 4) & 0x3ffc0ff;
    let r3 = (read_u32_le(key, 9) >> 6) & 0x3f03fff;
    let r4 = (read_u32_le(key, 12) >> 8) & 0x00fffff;

    let s1 = r1 * 5;
    let s2 = r2 * 5;
    let s3 = r3 * 5;
    let s4 = r4 * 5;

    let (mut h0, mut h1, mut h2, mut h3, mut h4) = (0u32, 0u32, 0u32, 0u32, 0u32);

    let full_blocks = msg.len() / 16;
    for blk in 0..full_blocks {
        process_block(
            &msg[blk * 16..blk * 16 + 16],
            1 << 24,
            (r0, r1, r2, r3, r4),
            (s1, s2, s3, s4),
            (&mut h0, &mut h1, &mut h2, &mut h3, &mut h4),
        );
    }
    let rem = msg.len() % 16;
    if rem != 0 {
        let mut buf = [0u8; 16];
        buf[..rem].copy_from_slice(&msg[full_blocks * 16..]);
        buf[rem] = 1;
        process_block(
            &buf,
            0,
            (r0, r1, r2, r3, r4),
            (s1, s2, s3, s4),
            (&mut h0, &mut h1, &mut h2, &mut h3, &mut h4),
        );
    }

    // Final reduction.
    let mut c = h1 >> 26;
    h1 &= 0x3ffffff;
    h2 += c;
    c = h2 >> 26;
    h2 &= 0x3ffffff;
    h3 += c;
    c = h3 >> 26;
    h3 &= 0x3ffffff;
    h4 += c;
    c = h4 >> 26;
    h4 &= 0x3ffffff;
    h0 += c * 5;
    c = h0 >> 26;
    h0 &= 0x3ffffff;
    h1 += c;

    // Compute h - p and select via constant-time mask.
    let mut g0 = h0 + 5;
    c = g0 >> 26;
    g0 &= 0x3ffffff;
    let mut g1 = h1 + c;
    c = g1 >> 26;
    g1 &= 0x3ffffff;
    let mut g2 = h2 + c;
    c = g2 >> 26;
    g2 &= 0x3ffffff;
    let mut g3 = h3 + c;
    c = g3 >> 26;
    g3 &= 0x3ffffff;
    let mut g4 = (h4.wrapping_add(c)).wrapping_sub(1 << 26);

    let mut mask = (g4 >> 31).wrapping_sub(1);
    g0 &= mask;
    g1 &= mask;
    g2 &= mask;
    g3 &= mask;
    g4 &= mask;
    mask = !mask;
    h0 = (h0 & mask) | g0;
    h1 = (h1 & mask) | g1;
    h2 = (h2 & mask) | g2;
    h3 = (h3 & mask) | g3;
    h4 = (h4 & mask) | g4;

    // Serialize h into four 32-bit words (done in u64 to absorb the shifts).
    let w0 = ((h0 as u64) | ((h1 as u64) << 26)) & 0xffffffff;
    let w1 = (((h1 as u64) >> 6) | ((h2 as u64) << 20)) & 0xffffffff;
    let w2 = (((h2 as u64) >> 12) | ((h3 as u64) << 14)) & 0xffffffff;
    let w3 = (((h3 as u64) >> 18) | ((h4 as u64) << 8)) & 0xffffffff;

    let pad0 = read_u32_le(key, 16) as u64;
    let pad1 = read_u32_le(key, 20) as u64;
    let pad2 = read_u32_le(key, 24) as u64;
    let pad3 = read_u32_le(key, 28) as u64;

    let mut f = w0 + pad0;
    let o0 = f as u32;
    f = w1 + pad1 + (f >> 32);
    let o1 = f as u32;
    f = w2 + pad2 + (f >> 32);
    let o2 = f as u32;
    f = w3 + pad3 + (f >> 32);
    let o3 = f as u32;

    let mut mac = [0u8; 16];
    mac[0..4].copy_from_slice(&o0.to_le_bytes());
    mac[4..8].copy_from_slice(&o1.to_le_bytes());
    mac[8..12].copy_from_slice(&o2.to_le_bytes());
    mac[12..16].copy_from_slice(&o3.to_le_bytes());
    mac
}

#[allow(clippy::too_many_arguments)]
fn process_block(
    block: &[u8],
    hibit: u32,
    r: (u32, u32, u32, u32, u32),
    s: (u32, u32, u32, u32),
    h: (&mut u32, &mut u32, &mut u32, &mut u32, &mut u32),
) {
    let (r0, r1, r2, r3, r4) = r;
    let (s1, s2, s3, s4) = s;
    let (h0, h1, h2, h3, h4) = h;

    *h0 += read_u32_le(block, 0) & 0x3ffffff;
    *h1 += (read_u32_le(block, 3) >> 2) & 0x3ffffff;
    *h2 += (read_u32_le(block, 6) >> 4) & 0x3ffffff;
    *h3 += (read_u32_le(block, 9) >> 6) & 0x3ffffff;
    *h4 += (read_u32_le(block, 12) >> 8) | hibit;

    let (a0, a1, a2, a3, a4) = (*h0 as u64, *h1 as u64, *h2 as u64, *h3 as u64, *h4 as u64);
    let (r0, r1, r2, r3, r4) = (r0 as u64, r1 as u64, r2 as u64, r3 as u64, r4 as u64);
    let (s1, s2, s3, s4) = (s1 as u64, s2 as u64, s3 as u64, s4 as u64);

    let d0 = a0 * r0 + a1 * s4 + a2 * s3 + a3 * s2 + a4 * s1;
    let mut d1 = a0 * r1 + a1 * r0 + a2 * s4 + a3 * s3 + a4 * s2;
    let mut d2 = a0 * r2 + a1 * r1 + a2 * r0 + a3 * s4 + a4 * s3;
    let mut d3 = a0 * r3 + a1 * r2 + a2 * r1 + a3 * r0 + a4 * s4;
    let mut d4 = a0 * r4 + a1 * r3 + a2 * r2 + a3 * r1 + a4 * r0;

    let mut c = (d0 >> 26) as u32;
    *h0 = (d0 as u32) & 0x3ffffff;
    d1 += c as u64;
    c = (d1 >> 26) as u32;
    *h1 = (d1 as u32) & 0x3ffffff;
    d2 += c as u64;
    c = (d2 >> 26) as u32;
    *h2 = (d2 as u32) & 0x3ffffff;
    d3 += c as u64;
    c = (d3 >> 26) as u32;
    *h3 = (d3 as u32) & 0x3ffffff;
    d4 += c as u64;
    c = (d4 >> 26) as u32;
    *h4 = (d4 as u32) & 0x3ffffff;
    *h0 += c * 5;
    c = *h0 >> 26;
    *h0 &= 0x3ffffff;
    *h1 += c;
}

// ===== AEAD_CHACHA20_POLY1305 (RFC 8439 §2.8) ==============================

/// Derive the Poly1305 one-time key from the ChaCha20 block at counter 0.
pub fn poly1305_key_gen(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
    let block = chacha20_block(key, 0, nonce);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&block[0..32]);
    otk
}

fn pad16(v: &mut Vec<u8>, len: usize) {
    let rem = len % 16;
    if rem != 0 {
        v.extend(std::iter::repeat_n(0u8, 16 - rem));
    }
}

fn aead_mac_data(aad: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut mac_data = Vec::new();
    mac_data.extend_from_slice(aad);
    pad16(&mut mac_data, aad.len());
    mac_data.extend_from_slice(ciphertext);
    pad16(&mut mac_data, ciphertext.len());
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    mac_data
}

/// AEAD encrypt: returns `(ciphertext, tag)`.
pub fn aead_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> (Vec<u8>, [u8; 16]) {
    let otk = poly1305_key_gen(key, nonce);
    let ciphertext = chacha20_encrypt(key, 1, nonce, plaintext);
    let mac_data = aead_mac_data(aad, &ciphertext);
    let tag = poly1305_mac(&otk, &mac_data);
    (ciphertext, tag)
}

/// AEAD decrypt: verifies the tag in constant time, returning the plaintext
/// only if authentication succeeds.
pub fn aead_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
) -> Option<Vec<u8>> {
    let otk = poly1305_key_gen(key, nonce);
    let mac_data = aead_mac_data(aad, ciphertext);
    let expected = poly1305_mac(&otk, &mac_data);

    let mut diff = 0u8;
    for i in 0..16 {
        diff |= expected[i] ^ tag[i];
    }
    if diff != 0 {
        return None;
    }
    Some(chacha20_encrypt(key, 1, nonce, ciphertext))
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

    fn to_hex(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for x in b {
            s.push_str(&format!("{:02x}", x));
        }
        s
    }

    #[test]
    fn poly1305_rfc8439_2_5_2() {
        let mut key = [0u8; 32];
        key.copy_from_slice(&from_hex(
            "85d6be7857556d337f4452fe42d506a8010380\
8afb0db2fd4abff6af4149f51b",
        ));
        let msg = b"Cryptographic Forum Research Group";
        assert_eq!(to_hex(&poly1305_mac(&key, msg)), "a8061dc1305136c6c22b8baf0c0127a9");
    }

    #[test]
    fn poly1305_keygen_rfc8439_2_6_2() {
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = 0x80 + i as u8;
        }
        let nonce = [0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            to_hex(&poly1305_key_gen(&key, &nonce)),
            "8ad5a08b905f81cc815040274ab29471a833b637e3fd0da508dbb8e2fdd1a646"
        );
    }

    #[test]
    fn aead_encrypt_rfc8439_2_8_2() {
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = 0x80 + i as u8;
        }
        let nonce = [0x07, 0, 0, 0, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
        let aad = from_hex("50515253c0c1c2c3c4c5c6c7");
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

        let (ciphertext, tag) = aead_encrypt(&key, &nonce, &aad, plaintext);
        assert_eq!(
            to_hex(&ciphertext),
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6\
3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36\
92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc\
3ff4def08e4b7a9de576d26586cec64b6116"
        );
        assert_eq!(to_hex(&tag), "1ae10b594f09e26a7e902ecbd0600691");
    }

    #[test]
    fn aead_round_trip_and_tamper_detection() {
        let key = [0x11u8; 32];
        let nonce = [0x22u8; 12];
        let aad = b"velocity-agent";
        let plaintext = b"the definitive browser for agents";

        let (ciphertext, tag) = aead_encrypt(&key, &nonce, aad, plaintext);
        let recovered = aead_decrypt(&key, &nonce, aad, &ciphertext, &tag).unwrap();
        assert_eq!(recovered, plaintext);

        // A single flipped ciphertext bit must fail authentication.
        let mut bad = ciphertext.clone();
        bad[0] ^= 0x01;
        assert!(aead_decrypt(&key, &nonce, aad, &bad, &tag).is_none());

        // A tampered tag must fail authentication.
        let mut bad_tag = tag;
        bad_tag[0] ^= 0x01;
        assert!(aead_decrypt(&key, &nonce, aad, &ciphertext, &bad_tag).is_none());
    }
}
