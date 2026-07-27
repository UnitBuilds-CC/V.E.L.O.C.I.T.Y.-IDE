//! TLS 1.3 `CertificateVerify` signature verification.
//!
//! This is the piece the from-scratch handshake in [`crate::net::tls_handshake`]
//! previously lacked: cryptographic proof that the peer holds the private key
//! matching the certificate it presented. Without it, a parsed certificate only
//! tells you *what* the peer claims — not that the claim is authentic.
//!
//! We deliberately reuse `ring` (already locked by rustls's `ring` provider)
//! for the RSA/ECDSA primitives rather than hand-rolling bignum arithmetic:
//! re-implementing constant-time modular exponentiation buys zero agentic value
//! and is a notorious source of subtle, exploitable bugs.
//!
//! Everything here is expressed as small pure functions so the DER walking,
//! the TLS 1.3 signed-content construction, and the scheme→algorithm mapping
//! can each be unit-tested without a live handshake.

use ring::signature;

const TAG_SEQUENCE: u8 = 0x30;
const TAG_BIT_STRING: u8 = 0x03;

// TLS 1.3 SignatureScheme code points (RFC 8446 §4.2.3).
const ECDSA_SECP256R1_SHA256: [u8; 2] = [0x04, 0x03];
const ECDSA_SECP384R1_SHA384: [u8; 2] = [0x05, 0x03];
const RSA_PSS_RSAE_SHA256: [u8; 2] = [0x08, 0x04];
const RSA_PSS_RSAE_SHA384: [u8; 2] = [0x08, 0x05];
const RSA_PSS_RSAE_SHA512: [u8; 2] = [0x08, 0x06];
const RSA_PKCS1_SHA256: [u8; 2] = [0x04, 0x01];
const RSA_PKCS1_SHA384: [u8; 2] = [0x05, 0x01];
const RSA_PKCS1_SHA512: [u8; 2] = [0x06, 0x01];

/// Minimal DER TLV read: returns `(tag, contents, total_bytes_consumed)`.
fn read_tlv(data: &[u8]) -> Option<(u8, &[u8], usize)> {
    if data.len() < 2 {
        return None;
    }
    let tag = data[0];
    let first = data[1];
    let (len, header) = if first & 0x80 == 0 {
        (first as usize, 2usize)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || data.len() < 2 + n {
            return None;
        }
        let mut len = 0usize;
        for &b in &data[2..2 + n] {
            len = (len << 8) | b as usize;
        }
        (len, 2 + n)
    };
    let end = header.checked_add(len)?;
    if end > data.len() {
        return None;
    }
    Some((tag, &data[header..end], end))
}

/// Extract the raw public-key octets from a DER `SubjectPublicKeyInfo`.
///
/// `SPKI ::= SEQUENCE { AlgorithmIdentifier, subjectPublicKey BIT STRING }`.
/// The returned bytes are the BIT STRING payload with the leading "unused bits"
/// octet removed — which is exactly what `ring` expects:
/// * ECDSA: the uncompressed EC point (`0x04 || X || Y`).
/// * RSA: the DER `RSAPublicKey` (`SEQUENCE { modulus, exponent }`).
pub fn spki_public_key_bytes(spki: &[u8]) -> Option<Vec<u8>> {
    let (tag, contents, _) = read_tlv(spki)?;
    if tag != TAG_SEQUENCE {
        return None;
    }
    // First element is the AlgorithmIdentifier SEQUENCE — skip it.
    let (_atag, _acontents, atotal) = read_tlv(contents)?;
    let rest = &contents[atotal..];
    // Second element is the subjectPublicKey BIT STRING.
    let (btag, bcontents, _) = read_tlv(rest)?;
    if btag != TAG_BIT_STRING || bcontents.is_empty() {
        return None;
    }
    // bcontents[0] is the count of unused trailing bits (0 for these keys).
    Some(bcontents[1..].to_vec())
}

/// Parse the body of a `CertificateVerify` handshake message (the bytes *after*
/// the 4-byte handshake header). Returns `(signature_scheme, signature)`.
///
/// Layout: `SignatureScheme (2) || signature length (2) || signature`.
pub fn parse_certificate_verify(body: &[u8]) -> Option<([u8; 2], &[u8])> {
    if body.len() < 4 {
        return None;
    }
    let scheme = [body[0], body[1]];
    let sig_len = ((body[2] as usize) << 8) | body[3] as usize;
    let end = 4 + sig_len;
    if body.len() < end {
        return None;
    }
    Some((scheme, &body[4..end]))
}

/// Build the exact byte string a TLS 1.3 server signs for `CertificateVerify`
/// (RFC 8446 §4.4.3): 64 `0x20` octets, the context string, a `0x00` separator,
/// then the transcript hash.
pub fn tls13_server_signed_content(transcript_hash: &[u8]) -> Vec<u8> {
    const CONTEXT: &[u8] = b"TLS 1.3, server CertificateVerify";
    let mut out = Vec::with_capacity(64 + CONTEXT.len() + 1 + transcript_hash.len());
    out.extend(std::iter::repeat_n(0x20u8, 64));
    out.extend_from_slice(CONTEXT);
    out.push(0x00);
    out.extend_from_slice(transcript_hash);
    out
}

/// Map a TLS 1.3 signature scheme to the corresponding `ring` verification
/// algorithm, or `None` if unsupported.
fn ring_algorithm(scheme: [u8; 2]) -> Option<&'static dyn signature::VerificationAlgorithm> {
    Some(match scheme {
        ECDSA_SECP256R1_SHA256 => &signature::ECDSA_P256_SHA256_ASN1,
        ECDSA_SECP384R1_SHA384 => &signature::ECDSA_P384_SHA384_ASN1,
        RSA_PSS_RSAE_SHA256 => &signature::RSA_PSS_2048_8192_SHA256,
        RSA_PSS_RSAE_SHA384 => &signature::RSA_PSS_2048_8192_SHA384,
        RSA_PSS_RSAE_SHA512 => &signature::RSA_PSS_2048_8192_SHA512,
        RSA_PKCS1_SHA256 => &signature::RSA_PKCS1_2048_8192_SHA256,
        RSA_PKCS1_SHA384 => &signature::RSA_PKCS1_2048_8192_SHA384,
        RSA_PKCS1_SHA512 => &signature::RSA_PKCS1_2048_8192_SHA512,
        _ => return None,
    })
}

/// Human-readable name for a signature scheme (diagnostics only).
pub fn scheme_name(scheme: [u8; 2]) -> &'static str {
    match scheme {
        ECDSA_SECP256R1_SHA256 => "ecdsa_secp256r1_sha256",
        ECDSA_SECP384R1_SHA384 => "ecdsa_secp384r1_sha384",
        RSA_PSS_RSAE_SHA256 => "rsa_pss_rsae_sha256",
        RSA_PSS_RSAE_SHA384 => "rsa_pss_rsae_sha384",
        RSA_PSS_RSAE_SHA512 => "rsa_pss_rsae_sha512",
        RSA_PKCS1_SHA256 => "rsa_pkcs1_sha256",
        RSA_PKCS1_SHA384 => "rsa_pkcs1_sha384",
        RSA_PKCS1_SHA512 => "rsa_pkcs1_sha512",
        _ => "unknown",
    }
}

/// Verify a raw signature over `signed_content` using the public key in `spki`.
pub fn verify_signature(
    scheme: [u8; 2],
    spki: &[u8],
    signed_content: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    let alg = ring_algorithm(scheme).ok_or_else(|| {
        format!("unsupported signature scheme {:02x}{:02x}", scheme[0], scheme[1])
    })?;
    let key_bytes = spki_public_key_bytes(spki).ok_or("malformed SubjectPublicKeyInfo")?;
    let public_key = signature::UnparsedPublicKey::new(alg, key_bytes);
    public_key
        .verify(signed_content, signature)
        .map_err(|_| format!("{} signature verification failed", scheme_name(scheme)))
}

/// Full `CertificateVerify` check: parse the message, rebuild the signed
/// content from `transcript_hash`, and verify against the certificate's SPKI.
/// Returns the accepted signature scheme on success.
pub fn verify_certificate_verify(
    spki: &[u8],
    transcript_hash: &[u8],
    cert_verify_body: &[u8],
) -> Result<[u8; 2], String> {
    let (scheme, sig) =
        parse_certificate_verify(cert_verify_body).ok_or("malformed CertificateVerify")?;
    let signed = tls13_server_signed_content(transcript_hash);
    verify_signature(scheme, spki, &signed, sig)?;
    Ok(scheme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

    #[test]
    fn signed_content_has_correct_prefix_and_layout() {
        let hash = [0xabu8; 32];
        let out = tls13_server_signed_content(&hash);
        assert_eq!(out.len(), 64 + 33 + 1 + 32);
        assert!(out[..64].iter().all(|&b| b == 0x20));
        assert_eq!(&out[64..64 + 33], b"TLS 1.3, server CertificateVerify");
        assert_eq!(out[97], 0x00);
        assert_eq!(&out[98..], &hash);
    }

    #[test]
    fn parse_certificate_verify_extracts_scheme_and_sig() {
        // scheme 0x0403, len 3, sig = [1,2,3]
        let body = [0x04, 0x03, 0x00, 0x03, 0x01, 0x02, 0x03];
        let (scheme, sig) = parse_certificate_verify(&body).unwrap();
        assert_eq!(scheme, [0x04, 0x03]);
        assert_eq!(sig, &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn parse_certificate_verify_rejects_truncated() {
        assert!(parse_certificate_verify(&[0x04]).is_none());
        // Claims 5-byte signature but only 2 present.
        assert!(parse_certificate_verify(&[0x04, 0x03, 0x00, 0x05, 0x01, 0x02]).is_none());
    }

    #[test]
    fn unsupported_scheme_is_rejected() {
        let err = verify_signature([0xff, 0xff], &[], &[], &[]).unwrap_err();
        assert!(err.contains("unsupported signature scheme"));
    }

    #[test]
    fn malformed_spki_is_rejected() {
        // Valid scheme but SPKI is not a SEQUENCE.
        let err = verify_signature([0x04, 0x03], &[0x02, 0x01, 0x00], b"x", b"y").unwrap_err();
        assert!(err.contains("malformed SubjectPublicKeyInfo"));
    }

    /// End-to-end proof that the verification path works: generate a real
    /// ECDSA P-256 key with `ring`, sign the TLS 1.3 signed-content for a known
    /// transcript hash, wrap the public key in a minimal SPKI, and confirm both
    /// that a valid signature verifies and a tampered one is rejected.
    #[test]
    fn ecdsa_p256_roundtrip_verifies_and_rejects_tampering() {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                .unwrap();
        let public_point = key_pair.public_key().as_ref(); // 0x04 || X || Y

        // Minimal SPKI: SEQUENCE { AlgorithmIdentifier (skipped), BIT STRING }.
        // The AlgorithmIdentifier content is irrelevant to our extraction —
        // only its length matters — so we use a 1-byte placeholder SEQUENCE.
        let alg_id = der_seq(&[0x05, 0x00]); // SEQUENCE { NULL }
        let mut bit_string_payload = vec![0x00u8]; // 0 unused bits
        bit_string_payload.extend_from_slice(public_point);
        let bit_string = der_tlv(TAG_BIT_STRING, &bit_string_payload);
        let mut spki_inner = alg_id;
        spki_inner.extend_from_slice(&bit_string);
        let spki = der_seq(&spki_inner);

        // Extraction must recover exactly the uncompressed point.
        assert_eq!(spki_public_key_bytes(&spki).unwrap(), public_point);

        let transcript_hash = [0x5au8; 32];
        let signed = tls13_server_signed_content(&transcript_hash);
        let sig = key_pair.sign(&rng, &signed).unwrap();

        // Build a CertificateVerify body and verify end-to-end.
        let mut body = vec![0x04, 0x03];
        let sig_bytes = sig.as_ref();
        body.push((sig_bytes.len() >> 8) as u8);
        body.push((sig_bytes.len() & 0xff) as u8);
        body.extend_from_slice(sig_bytes);

        let scheme = verify_certificate_verify(&spki, &transcript_hash, &body).unwrap();
        assert_eq!(scheme, ECDSA_SECP256R1_SHA256);

        // A different transcript hash must fail.
        let wrong_hash = [0x00u8; 32];
        assert!(verify_certificate_verify(&spki, &wrong_hash, &body).is_err());

        // A flipped signature byte must fail.
        let mut tampered = body.clone();
        *tampered.last_mut().unwrap() ^= 0x01;
        assert!(verify_certificate_verify(&spki, &transcript_hash, &tampered).is_err());
    }

    // --- tiny DER builders for the roundtrip test --------------------------

    fn der_tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        let len = contents.len();
        if len < 0x80 {
            out.push(len as u8);
        } else if len < 0x100 {
            out.push(0x81);
            out.push(len as u8);
        } else {
            out.push(0x82);
            out.push((len >> 8) as u8);
            out.push(len as u8);
        }
        out.extend_from_slice(contents);
        out
    }

    fn der_seq(contents: &[u8]) -> Vec<u8> {
        der_tlv(TAG_SEQUENCE, contents)
    }
}
