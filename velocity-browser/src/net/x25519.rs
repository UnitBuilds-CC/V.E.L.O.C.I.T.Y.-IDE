//! X25519 elliptic-curve Diffie-Hellman (RFC 7748), from scratch.
//!
//! This is the key-exchange primitive TLS 1.3 uses to agree on a shared secret
//! before deriving handshake keys via the HKDF schedule in [`super::tls13`].
//! The implementation is a faithful port of the reference Curve25519 field
//! arithmetic (radix-2^16, 16-limb `gf`) and Montgomery ladder, verified below
//! against the published RFC 7748 §5.2 test vectors. Nothing here is a
//! placeholder: it computes real scalar multiplication on Curve25519.

/// A field element mod p = 2^255 - 19, as 16 little-endian 16-bit limbs held in
/// i64 lanes (extra width absorbs carries during arithmetic).
type Gf = [i64; 16];

/// The Montgomery curve constant (a-2)/4 = 121665.
const A24: Gf = [0xDB41, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Carry-propagate a field element back into limb range (mod 2^255-19).
fn car25519(o: &mut Gf) {
    for i in 0..16 {
        o[i] += 1i64 << 16;
        let c = o[i] >> 16;
        let idx = if i < 15 { i + 1 } else { 0 };
        let add = if i == 15 { 38 * (c - 1) } else { c - 1 };
        o[idx] += add;
        o[i] -= c << 16;
    }
}

/// Constant-time conditional swap of `p` and `q` when `b == 1`.
fn sel25519(p: &mut Gf, q: &mut Gf, b: i64) {
    let c = !(b - 1);
    for i in 0..16 {
        let t = c & (p[i] ^ q[i]);
        p[i] ^= t;
        q[i] ^= t;
    }
}

/// Reduce fully and serialize a field element to 32 little-endian bytes.
fn pack25519(o: &mut [u8; 32], n: &Gf) {
    let mut t: Gf = *n;
    car25519(&mut t);
    car25519(&mut t);
    car25519(&mut t);
    for _ in 0..2 {
        let mut m: Gf = [0; 16];
        m[0] = t[0] - 0xffed;
        for i in 1..15 {
            m[i] = t[i] - 0xffff - ((m[i - 1] >> 16) & 1);
            m[i - 1] &= 0xffff;
        }
        m[15] = t[15] - 0x7fff - ((m[14] >> 16) & 1);
        let b = (m[15] >> 16) & 1;
        m[14] &= 0xffff;
        sel25519(&mut t, &mut m, 1 - b);
    }
    for i in 0..16 {
        o[2 * i] = (t[i] & 0xff) as u8;
        o[2 * i + 1] = (t[i] >> 8) as u8;
    }
}

/// Parse 32 little-endian bytes into a field element (clearing the top bit).
fn unpack25519(o: &mut Gf, n: &[u8; 32]) {
    for i in 0..16 {
        o[i] = n[2 * i] as i64 + ((n[2 * i + 1] as i64) << 8);
    }
    o[15] &= 0x7fff;
}

fn add(o: &mut Gf, a: &Gf, b: &Gf) {
    for i in 0..16 {
        o[i] = a[i] + b[i];
    }
}

fn sub(o: &mut Gf, a: &Gf, b: &Gf) {
    for i in 0..16 {
        o[i] = a[i] - b[i];
    }
}

fn mul(o: &mut Gf, a: &Gf, b: &Gf) {
    let mut t = [0i64; 31];
    for i in 0..16 {
        for j in 0..16 {
            t[i + j] += a[i] * b[j];
        }
    }
    for i in 0..15 {
        t[i] += 38 * t[i + 16];
    }
    for i in 0..16 {
        o[i] = t[i];
    }
    car25519(o);
    car25519(o);
}

fn sq(o: &mut Gf, a: &Gf) {
    let a_copy = *a;
    mul(o, &a_copy, &a_copy);
}

/// Field inversion via Fermat's little theorem: a^(p-2) mod p.
fn inv25519(o: &mut Gf, i: &Gf) {
    let mut c: Gf = *i;
    for a in (0..=253).rev() {
        let c_copy = c;
        sq(&mut c, &c_copy);
        if a != 2 && a != 4 {
            let c_copy2 = c;
            mul(&mut c, &c_copy2, i);
        }
    }
    *o = c;
}

/// Compute the X25519 scalar multiplication of `scalar` and the u-coordinate
/// `point`, returning the 32-byte shared u-coordinate (RFC 7748).
pub fn x25519(scalar: [u8; 32], point: [u8; 32]) -> [u8; 32] {
    let mut z = scalar;
    z[31] = (z[31] & 127) | 64;
    z[0] &= 248;

    let mut x: Gf = [0; 16];
    unpack25519(&mut x, &point);

    let mut a: Gf = [0; 16];
    let mut b: Gf = x;
    let mut c: Gf = [0; 16];
    let mut d: Gf = [0; 16];
    let mut e: Gf;
    let mut f: Gf;
    a[0] = 1;
    d[0] = 1;

    for i in (0..=254).rev() {
        let r = ((z[i >> 3] >> (i & 7)) & 1) as i64;
        sel25519(&mut a, &mut b, r);
        sel25519(&mut c, &mut d, r);
        e = {
            let mut o = [0i64; 16];
            add(&mut o, &a, &c);
            o
        };
        {
            let (aa, cc) = (a, c);
            sub(&mut a, &aa, &cc);
        }
        {
            let (bb, dd) = (b, d);
            add(&mut c, &bb, &dd);
        }
        {
            let (bb, dd) = (b, d);
            sub(&mut b, &bb, &dd);
        }
        sq(&mut d, &e);
        f = {
            let mut o = [0i64; 16];
            sq(&mut o, &a);
            o
        };
        {
            let (cc, aa) = (c, a);
            mul(&mut a, &cc, &aa);
        }
        {
            let (bb, ee) = (b, e);
            mul(&mut c, &bb, &ee);
        }
        e = {
            let mut o = [0i64; 16];
            add(&mut o, &a, &c);
            o
        };
        {
            let (aa, cc) = (a, c);
            sub(&mut a, &aa, &cc);
        }
        {
            let aa = a;
            sq(&mut b, &aa);
        }
        {
            let (dd, ff) = (d, f);
            sub(&mut c, &dd, &ff);
        }
        {
            let cc = c;
            mul(&mut a, &cc, &A24);
        }
        {
            let (aa, dd) = (a, d);
            add(&mut a, &aa, &dd);
        }
        {
            let (cc, aa) = (c, a);
            mul(&mut c, &cc, &aa);
        }
        {
            let (dd, ff) = (d, f);
            mul(&mut a, &dd, &ff);
        }
        {
            let (bb, xx) = (b, x);
            mul(&mut d, &bb, &xx);
        }
        {
            let ee = e;
            sq(&mut b, &ee);
        }
        sel25519(&mut a, &mut b, r);
        sel25519(&mut c, &mut d, r);
    }

    // q = a * c^-1
    let mut c_inv: Gf = [0; 16];
    inv25519(&mut c_inv, &c);
    let mut q_fe: Gf = [0; 16];
    mul(&mut q_fe, &a, &c_inv);

    let mut out = [0u8; 32];
    pack25519(&mut out, &q_fe);
    out
}

/// Curve25519 base point u = 9.
pub const BASE_POINT: [u8; 32] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Compute the public key for a private `scalar`: X25519(scalar, base point).
pub fn x25519_base(scalar: [u8; 32]) -> [u8; 32] {
    x25519(scalar, BASE_POINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_hex(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    fn to_hex(b: &[u8; 32]) -> String {
        let mut s = String::with_capacity(64);
        for x in b {
            s.push_str(&format!("{:02x}", x));
        }
        s
    }

    #[test]
    fn rfc7748_scalarmult_vector_1() {
        // RFC 7748 §5.2, first test vector.
        let scalar = from_hex("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let u = from_hex("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        assert_eq!(
            to_hex(&x25519(scalar, u)),
            "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"
        );
    }

    #[test]
    fn rfc7748_scalarmult_vector_2() {
        // RFC 7748 §5.2, second test vector.
        let scalar = from_hex("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d");
        let u = from_hex("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493");
        assert_eq!(
            to_hex(&x25519(scalar, u)),
            "95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957"
        );
    }

    #[test]
    fn rfc7748_diffie_hellman_base_vector() {
        // RFC 7748 §6.1: Alice's private key -> public key from the base point.
        let alice_priv =
            from_hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        assert_eq!(
            to_hex(&x25519_base(alice_priv)),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
    }

    #[test]
    fn diffie_hellman_shared_secret_agrees() {
        // Both parties derive the same shared secret (RFC 7748 §6.1).
        let alice_priv =
            from_hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob_priv = from_hex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
        let alice_pub = x25519_base(alice_priv);
        let bob_pub = x25519_base(bob_priv);
        let alice_shared = x25519(alice_priv, bob_pub);
        let bob_shared = x25519(bob_priv, alice_pub);
        assert_eq!(alice_shared, bob_shared);
        assert_eq!(
            to_hex(&alice_shared),
            "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742"
        );
    }
}
