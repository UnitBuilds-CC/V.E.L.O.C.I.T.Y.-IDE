//! AES-256-GCM (RFC 5116 / NIST SP 800-38D) implemented from scratch.
//!
//! This is the NDA at-rest / in-transit AEAD. AES was chosen over
//! ChaCha20-Poly1305 because every modern x86_64 CPU carries the **AES-NI**
//! and **PCLMULQDQ** instructions, making AES-GCM effectively free in
//! hardware, whereas ChaCha is only a software-speed win. ChaCha remains
//! available as a portable fallback in [`super::chacha20poly1305`].
//!
//! Layering:
//!   * A portable **software** implementation is the correctness oracle and
//!     the fallback for CPUs without AES-NI. It is validated against the
//!     FIPS-197 AES-256 known-answer test and the NIST GCM test vectors.
//!   * A **hardware** fast-path (added on top) uses CPU intrinsics and is
//!     cross-checked to be byte-identical to the software path.
//!
//! GCM only ever uses AES *encryption* (even when decrypting), so no inverse
//! cipher is implemented.

// ---------------------------------------------------------------------------
// AES-256 block cipher (software)
// ---------------------------------------------------------------------------

/// AES S-box (FIPS-197 figure 7).
static SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// Round constants for AES-256 key expansion (only 7 are needed: i/Nk ∈ 1..=7).
static RCON: [u8; 8] = [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40];

const NR: usize = 14; // AES-256 rounds
const NK: usize = 8; // AES-256 key words

/// The 15 expanded round keys, each a 16-byte AES state.
pub struct Aes256Key {
    round_keys: [[u8; 16]; NR + 1],
}

#[inline]
fn sub_word(w: [u8; 4]) -> [u8; 4] {
    [
        SBOX[w[0] as usize],
        SBOX[w[1] as usize],
        SBOX[w[2] as usize],
        SBOX[w[3] as usize],
    ]
}

impl Aes256Key {
    /// Expand a 256-bit key into 15 round keys (FIPS-197 §5.2).
    pub fn new(key: &[u8; 32]) -> Self {
        let total_words = 4 * (NR + 1); // 60
        let mut w = [[0u8; 4]; 60];
        for i in 0..NK {
            w[i] = [key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]];
        }
        for i in NK..total_words {
            let mut temp = w[i - 1];
            if i % NK == 0 {
                // RotWord then SubWord then XOR Rcon.
                temp = [temp[1], temp[2], temp[3], temp[0]];
                temp = sub_word(temp);
                temp[0] ^= RCON[i / NK];
            } else if i % NK == 4 {
                // AES-256 only: extra SubWord at the quarter mark.
                temp = sub_word(temp);
            }
            for j in 0..4 {
                w[i][j] = w[i - NK][j] ^ temp[j];
            }
        }
        let mut round_keys = [[0u8; 16]; NR + 1];
        for r in 0..=NR {
            for c in 0..4 {
                let word = w[4 * r + c];
                round_keys[r][4 * c..4 * c + 4].copy_from_slice(&word);
            }
        }
        Aes256Key { round_keys }
    }
}

#[inline]
fn xtime(b: u8) -> u8 {
    let hi = b & 0x80;
    let shifted = b << 1;
    if hi != 0 {
        shifted ^ 0x1b
    } else {
        shifted
    }
}

#[inline]
fn add_round_key(state: &mut [u8; 16], rk: &[u8; 16]) {
    for i in 0..16 {
        state[i] ^= rk[i];
    }
}

#[inline]
fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() {
        *b = SBOX[*b as usize];
    }
}

#[inline]
fn shift_rows(state: &mut [u8; 16]) {
    // State is column-major: index = row + 4*col. Row r rotates left by r.
    let s = *state;
    for row in 1..4 {
        for col in 0..4 {
            state[row + 4 * col] = s[row + 4 * ((col + row) % 4)];
        }
    }
}

#[inline]
fn mix_columns(state: &mut [u8; 16]) {
    for c in 0..4 {
        let i = 4 * c;
        let a0 = state[i];
        let a1 = state[i + 1];
        let a2 = state[i + 2];
        let a3 = state[i + 3];
        state[i] = xtime(a0) ^ (xtime(a1) ^ a1) ^ a2 ^ a3;
        state[i + 1] = a0 ^ xtime(a1) ^ (xtime(a2) ^ a2) ^ a3;
        state[i + 2] = a0 ^ a1 ^ xtime(a2) ^ (xtime(a3) ^ a3);
        state[i + 3] = (xtime(a0) ^ a0) ^ a1 ^ a2 ^ xtime(a3);
    }
}

/// Encrypt a single 16-byte block (software AES-256).
fn aes256_encrypt_block_sw(key: &Aes256Key, block: &[u8; 16]) -> [u8; 16] {
    let mut state = *block;
    add_round_key(&mut state, &key.round_keys[0]);
    for round in 1..NR {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        add_round_key(&mut state, &key.round_keys[round]);
    }
    sub_bytes(&mut state);
    shift_rows(&mut state);
    add_round_key(&mut state, &key.round_keys[NR]);
    state
}

// ---------------------------------------------------------------------------
// Hardware AES-256 block cipher via AES-NI (x86_64)
// ---------------------------------------------------------------------------
#[cfg(target_arch = "x86_64")]
mod aesni {
    use std::arch::x86_64::*;

    #[target_feature(enable = "aes")]
    unsafe fn assist_1(temp1: __m128i, mut temp2: __m128i) -> __m128i {
        temp2 = _mm_shuffle_epi32(temp2, 0xff);
        let mut temp4 = _mm_slli_si128(temp1, 0x4);
        let mut t = _mm_xor_si128(temp1, temp4);
        temp4 = _mm_slli_si128(temp4, 0x4);
        t = _mm_xor_si128(t, temp4);
        temp4 = _mm_slli_si128(temp4, 0x4);
        t = _mm_xor_si128(t, temp4);
        _mm_xor_si128(t, temp2)
    }

    #[target_feature(enable = "aes")]
    unsafe fn assist_2(temp1: __m128i, temp3: __m128i) -> __m128i {
        let temp4x = _mm_aeskeygenassist_si128(temp1, 0x0);
        let temp2 = _mm_shuffle_epi32(temp4x, 0xaa);
        let mut temp4 = _mm_slli_si128(temp3, 0x4);
        let mut t = _mm_xor_si128(temp3, temp4);
        temp4 = _mm_slli_si128(temp4, 0x4);
        t = _mm_xor_si128(t, temp4);
        temp4 = _mm_slli_si128(temp4, 0x4);
        t = _mm_xor_si128(t, temp4);
        _mm_xor_si128(t, temp2)
    }

    #[inline]
    unsafe fn store(dst: &mut [u8; 16], v: __m128i) {
        _mm_storeu_si128(dst.as_mut_ptr() as *mut __m128i, v);
    }

    /// Expand a 256-bit key into 15 round keys using AES-NI.
    ///
    /// SAFETY: Caller must ensure AES-NI is available (checked via `is_x86_feature_detected!("aes")`).
    /// The key pointer is valid for 32 bytes (guaranteed by `&[u8; 32]`).
    /// All `_mm_*` intrinsics operate on `__m128i` values which are naturally aligned.
    #[target_feature(enable = "aes")]
    pub unsafe fn expand_key_256(key: &[u8; 32]) -> [[u8; 16]; 15] {
        let mut rks = [[0u8; 16]; 15];
        let mut temp1 = _mm_loadu_si128(key.as_ptr() as *const __m128i);
        let mut temp3 = _mm_loadu_si128(key[16..].as_ptr() as *const __m128i);
        store(&mut rks[0], temp1);
        store(&mut rks[1], temp3);

        macro_rules! round_pair {
            ($rcon:expr, $even:expr, $odd:expr) => {{
                let temp2 = _mm_aeskeygenassist_si128(temp3, $rcon);
                temp1 = assist_1(temp1, temp2);
                store(&mut rks[$even], temp1);
                temp3 = assist_2(temp1, temp3);
                store(&mut rks[$odd], temp3);
            }};
        }
        round_pair!(0x01, 2, 3);
        round_pair!(0x02, 4, 5);
        round_pair!(0x04, 6, 7);
        round_pair!(0x08, 8, 9);
        round_pair!(0x10, 10, 11);
        round_pair!(0x20, 12, 13);
        let temp2 = _mm_aeskeygenassist_si128(temp3, 0x40);
        temp1 = assist_1(temp1, temp2);
        store(&mut rks[14], temp1);
        rks
    }

    /// Encrypt one block with AES-NI given expanded round keys.
    ///
    /// SAFETY: Caller must ensure AES-NI is available. The round keys `rks` must have
    /// been produced by `expand_key_256`. The block pointer is valid for 16 bytes.
    #[target_feature(enable = "aes")]
    pub unsafe fn encrypt_block(rks: &[[u8; 16]; 15], block: &[u8; 16]) -> [u8; 16] {
        let load = |b: &[u8; 16]| _mm_loadu_si128(b.as_ptr() as *const __m128i);
        let mut m = _mm_loadu_si128(block.as_ptr() as *const __m128i);
        m = _mm_xor_si128(m, load(&rks[0]));
        for i in 1..14 {
            m = _mm_aesenc_si128(m, load(&rks[i]));
        }
        m = _mm_aesenclast_si128(m, load(&rks[14]));
        let mut out = [0u8; 16];
        store(&mut out, m);
        out
    }
}

// ---------------------------------------------------------------------------
// Backend dispatch: hardware AES-NI when available, software otherwise
// ---------------------------------------------------------------------------

enum AesBackend {
    #[cfg(target_arch = "x86_64")]
    Hardware([[u8; 16]; 15]),
    Software(Aes256Key),
}

impl AesBackend {
    fn new(key: &[u8; 32]) -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("aes") {
                // SAFETY: Runtime AES-NI detection guarantees the CPU supports AES intrinsics.
                // The `key` parameter is `&[u8; 32]`, a valid 32-byte pointer.
                let rks = unsafe { aesni::expand_key_256(key) };
                return AesBackend::Hardware(rks);
            }
        }
        AesBackend::Software(Aes256Key::new(key))
    }

    #[inline]
    fn encrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        match self {
            #[cfg(target_arch = "x86_64")]
            // SAFETY: Hardware variant is only constructed when AES-NI is detected at runtime.
            // The round keys were produced by expand_key_256, and block is a valid 16-byte pointer.
            AesBackend::Hardware(rks) => unsafe { aesni::encrypt_block(rks, block) },
            AesBackend::Software(k) => aes256_encrypt_block_sw(k, block),
        }
    }
}

// ---------------------------------------------------------------------------
// GHASH over GF(2^128) (software)
// ---------------------------------------------------------------------------

#[inline]
fn xor16(a: &mut [u8; 16], b: &[u8; 16]) {
    for i in 0..16 {
        a[i] ^= b[i];
    }
}

/// Multiply two 128-bit blocks in GF(2^128) using the GCM reduction polynomial
/// (NIST SP 800-38D §6.3). Bit 0 is the most-significant bit of byte 0.
fn gf_mult(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;
    for i in 0..128 {
        let bit = (x[i / 8] >> (7 - (i % 8))) & 1;
        if bit == 1 {
            xor16(&mut z, &v);
        }
        // v = v >> 1 in the block sense, with reduction by R = 0xe1||0^120.
        let lsb = v[15] & 1;
        let mut carry = 0u8;
        for byte in v.iter_mut() {
            let new_carry = *byte & 1;
            *byte = (*byte >> 1) | (carry << 7);
            carry = new_carry;
        }
        if lsb == 1 {
            v[0] ^= 0xe1;
        }
    }
    z
}

/// GHASH: fold each 16-byte block of `data` into the accumulator under `h`.
/// `data` must already be a multiple of 16 bytes (caller zero-pads).
fn ghash(h: &[u8; 16], data: &[u8]) -> [u8; 16] {
    let mut y = [0u8; 16];
    let mut i = 0;
    while i < data.len() {
        let mut block = [0u8; 16];
        block.copy_from_slice(&data[i..i + 16]);
        xor16(&mut y, &block);
        y = gf_mult(&y, h);
        i += 16;
    }
    y
}

/// Increment the rightmost 32 bits of a counter block (big-endian, mod 2^32).
#[inline]
fn inc32(ctr: &mut [u8; 16]) {
    let mut n = u32::from_be_bytes([ctr[12], ctr[13], ctr[14], ctr[15]]);
    n = n.wrapping_add(1);
    ctr[12..16].copy_from_slice(&n.to_be_bytes());
}

/// Append `data` to `buf`, then zero-pad up to the next 16-byte boundary.
fn push_padded(buf: &mut Vec<u8>, data: &[u8]) {
    buf.extend_from_slice(data);
    let rem = data.len() % 16;
    if rem != 0 {
        buf.extend(std::iter::repeat_n(0u8, 16 - rem));
    }
}

/// Compute the GCM authentication tag for the given ciphertext.
fn gcm_tag(
    backend: &AesBackend,
    h: &[u8; 16],
    j0: &[u8; 16],
    aad: &[u8],
    ciphertext: &[u8],
) -> [u8; 16] {
    let mut ghash_input = Vec::with_capacity(aad.len() + ciphertext.len() + 32);
    push_padded(&mut ghash_input, aad);
    push_padded(&mut ghash_input, ciphertext);
    let mut len_block = [0u8; 16];
    len_block[0..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
    len_block[8..16].copy_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());
    ghash_input.extend_from_slice(&len_block);

    let s = ghash(h, &ghash_input);
    let mut tag = backend.encrypt_block(j0);
    xor16(&mut tag, &s);
    tag
}

/// Apply GCM's CTR keystream to `data` starting at counter `start`.
fn gctr(backend: &AesBackend, start: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut ctr = *start;
    let mut i = 0;
    while i < data.len() {
        let ks = backend.encrypt_block(&ctr);
        let n = core::cmp::min(16, data.len() - i);
        for j in 0..n {
            out.push(data[i + j] ^ ks[j]);
        }
        inc32(&mut ctr);
        i += 16;
    }
    out
}

/// AES-256-GCM sealing over a backend. Returns `(ciphertext, tag)`.
fn aes256_gcm_encrypt_impl(
    backend: &AesBackend,
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> (Vec<u8>, [u8; 16]) {
    let h = backend.encrypt_block(&[0u8; 16]);
    let mut j0 = [0u8; 16];
    j0[0..12].copy_from_slice(nonce);
    j0[15] = 1;
    let mut ctr = j0;
    inc32(&mut ctr);
    let ciphertext = gctr(backend, &ctr, plaintext);
    let tag = gcm_tag(backend, &h, &j0, aad, &ciphertext);
    (ciphertext, tag)
}

/// AES-256-GCM opening over a backend. Returns the plaintext iff the tag
/// verifies.
fn aes256_gcm_decrypt_impl(
    backend: &AesBackend,
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
) -> Option<Vec<u8>> {
    let h = backend.encrypt_block(&[0u8; 16]);
    let mut j0 = [0u8; 16];
    j0[0..12].copy_from_slice(nonce);
    j0[15] = 1;
    let expected = gcm_tag(backend, &h, &j0, aad, ciphertext);
    // Constant-time tag comparison.
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= expected[i] ^ tag[i];
    }
    if diff != 0 {
        return None;
    }
    let mut ctr = j0;
    inc32(&mut ctr);
    Some(gctr(backend, &ctr, ciphertext))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Seal `plaintext` with AES-256-GCM. `nonce` (96-bit) MUST be unique per key.
/// Returns `(ciphertext, 16-byte tag)`.
pub fn aes256_gcm_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> (Vec<u8>, [u8; 16]) {
    let backend = AesBackend::new(key);
    aes256_gcm_encrypt_impl(&backend, nonce, aad, plaintext)
}

/// Open an AES-256-GCM ciphertext. Returns the plaintext iff the tag verifies
/// against `key`, `nonce`, and `aad`; `None` on any tampering or wrong key.
pub fn aes256_gcm_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
) -> Option<Vec<u8>> {
    let backend = AesBackend::new(key);
    aes256_gcm_decrypt_impl(&backend, nonce, aad, ciphertext, tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn aes256_block_matches_fips197_c3() {
        // FIPS-197 Appendix C.3 known-answer test for AES-256.
        let key: [u8; 32] = hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
            .try_into()
            .unwrap();
        let pt: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
        let expected: [u8; 16] = hex("8ea2b7ca516745bfeafc49904b496089").try_into().unwrap();
        let k = Aes256Key::new(&key);
        assert_eq!(aes256_encrypt_block_sw(&k, &pt), expected);
    }

    #[test]
    fn gcm_nist_test_case_13_empty() {
        // NIST GCM spec Test Case 13 (AES-256, empty AAD + plaintext).
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let (ct, tag) = aes256_gcm_encrypt(&key, &nonce, &[], &[]);
        assert!(ct.is_empty());
        assert_eq!(tag.to_vec(), hex("530f8afbc74536b9a963b4f1c4cb738b"));
    }

    #[test]
    fn gcm_nist_test_case_14_single_block() {
        // NIST GCM spec Test Case 14 (AES-256, 16-byte zero plaintext).
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let pt = [0u8; 16];
        let (ct, tag) = aes256_gcm_encrypt(&key, &nonce, &[], &pt);
        assert_eq!(ct, hex("cea7403d4d606b6e074ec5d3baf39d18"));
        assert_eq!(tag.to_vec(), hex("d0d1c8a799996bf0265b98b5d48ab919"));
    }

    #[test]
    fn gcm_nist_test_case_15_with_aad() {
        // NIST GCM spec Test Case 15 (AES-256, 60-byte plaintext, 20-byte AAD).
        let key: [u8; 32] = hex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308")
            .try_into()
            .unwrap();
        let nonce: [u8; 12] = hex("cafebabefacedbaddecaf888").try_into().unwrap();
        let pt = hex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let aad = hex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let (ct, tag) = aes256_gcm_encrypt(&key, &nonce, &aad, &pt);
        assert_eq!(
            ct,
            hex(
                "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa\
8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662"
            )
        );
        assert_eq!(tag.to_vec(), hex("76fc6ece0f4e1768cddf8853bb2d551b"));
    }

    #[test]
    fn gcm_round_trips_and_detects_tampering() {
        let key = derive_test_key();
        let nonce = [0x24u8; 12];
        let aad = b"nda-header";
        let pt = b"the quick brown fox jumps over the lazy dog, twice over!!";
        let (mut ct, mut tag) = aes256_gcm_encrypt(&key, &nonce, aad, pt);
        // Correct open.
        let opened = aes256_gcm_decrypt(&key, &nonce, aad, &ct, &tag).unwrap();
        assert_eq!(opened, pt);
        // Tampered ciphertext is rejected.
        ct[0] ^= 0x01;
        assert!(aes256_gcm_decrypt(&key, &nonce, aad, &ct, &tag).is_none());
        ct[0] ^= 0x01;
        // Tampered tag is rejected.
        tag[0] ^= 0x01;
        assert!(aes256_gcm_decrypt(&key, &nonce, aad, &ct, &tag).is_none());
        tag[0] ^= 0x01;
        // Wrong AAD is rejected.
        assert!(aes256_gcm_decrypt(&key, &nonce, b"other", &ct, &tag).is_none());
    }

    fn derive_test_key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        k
    }

    #[test]
    fn hardware_backend_matches_software_byte_for_byte() {
        // Whatever backend `new` selects on this CPU (AES-NI here) must produce
        // output identical to the pure-software oracle, at every length that
        // exercises partial final blocks and multi-block AAD/plaintext.
        let key = derive_test_key();
        let sw = AesBackend::Software(Aes256Key::new(&key));
        let dispatched = AesBackend::new(&key);
        for len in [0usize, 1, 15, 16, 17, 31, 32, 63, 64, 100] {
            let pt: Vec<u8> = (0..len).map(|i| (i as u8) ^ 0xa5).collect();
            let aad: Vec<u8> = (0..(len % 20)).map(|i| i as u8).collect();
            let nonce = [0x37u8; 12];
            let (c1, t1) = aes256_gcm_encrypt_impl(&sw, &nonce, &aad, &pt);
            let (c2, t2) = aes256_gcm_encrypt_impl(&dispatched, &nonce, &aad, &pt);
            assert_eq!(c1, c2, "ciphertext differs at len {len}");
            assert_eq!(t1, t2, "tag differs at len {len}");
            // And the dispatched backend must round-trip its own output.
            let opened = aes256_gcm_decrypt_impl(&dispatched, &nonce, &aad, &c2, &t2).unwrap();
            assert_eq!(opened, pt);
        }
    }
}
