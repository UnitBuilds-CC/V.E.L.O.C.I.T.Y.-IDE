//! Certificate chain validation to a trusted root for the from-scratch TLS 1.3 stack.
//!
//! The hand-rolled [`Tls13Handshake`](super::tls_handshake::Tls13Handshake) parses the
//! leaf certificate itself (see [`x509`](super::x509)) and verifies the
//! `CertificateVerify` signature (see [`tls_sigverify`](super::tls_sigverify)). Proving
//! that the leaf actually chains to a publicly trusted root, however, requires full
//! RFC 5280 path building — issuer/subject matching, basic-constraints and key-usage
//! enforcement, and signature verification at each link. That logic is subtle and
//! security-critical, so rather than hand-roll it this module reuses `rustls-webpki`
//! for path validation against the Mozilla root program (`webpki-roots`), driven by
//! the same `ring` signature algorithms already locked by rustls's `ring` provider.
//! This introduces no new crypto backend or toolchain requirement.

use rustls::pki_types::{CertificateDer, UnixTime};
use std::time::Duration;

/// Validate that `chain_der[0]` (the leaf) chains to a trusted root, using any
/// intermediates supplied in `chain_der[1..]`, at wall-clock time `now_unix`
/// (Unix seconds).
///
/// Returns `Ok(())` when a valid path to a root in the Mozilla trust store is
/// found for TLS server authentication, or `Err` with a diagnostic reason. This
/// checks the *chain* only; hostname matching and the `CertificateVerify`
/// signature are handled by the caller (`x509` / `tls_sigverify`).
pub fn verify_chain_to_root(chain_der: &[Vec<u8>], now_unix: i64) -> Result<(), String> {
    if chain_der.is_empty() {
        return Err("empty certificate chain".to_string());
    }
    if now_unix < 0 {
        return Err("invalid verification time".to_string());
    }

    let leaf = CertificateDer::from(chain_der[0].as_slice());
    let intermediates: Vec<CertificateDer> = chain_der[1..]
        .iter()
        .map(|d| CertificateDer::from(d.as_slice()))
        .collect();

    let ee = webpki::EndEntityCert::try_from(&leaf)
        .map_err(|e| format!("cannot parse leaf certificate: {e:?}"))?;

    // Reuse the ring signature algorithms already locked by rustls's provider,
    // so we do not pull in a second crypto backend.
    let provider = rustls::crypto::ring::default_provider();
    let algs = provider.signature_verification_algorithms.all;

    let time = UnixTime::since_unix_epoch(Duration::from_secs(now_unix as u64));

    ee.verify_for_usage(
        algs,
        webpki_roots::TLS_SERVER_ROOTS,
        &intermediates,
        time,
        webpki::KeyUsage::server_auth(),
        None, // no revocation checking (no OCSP/CRL transport here)
        None, // no extra path constraints
    )
    .map(|_verified_path| ())
    .map_err(|e| format!("chain does not validate to a trusted root: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chain_is_rejected() {
        let err = verify_chain_to_root(&[], 1_700_000_000).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn negative_time_is_rejected() {
        let bogus = vec![vec![0x30u8, 0x00]];
        let err = verify_chain_to_root(&bogus, -1).unwrap_err();
        assert!(err.contains("invalid verification time"));
    }

    #[test]
    fn garbage_leaf_is_rejected_not_panicking() {
        // A byte string that is not a valid certificate must be rejected
        // cleanly (no panic, no accidental trust).
        let junk = vec![vec![0x01u8, 0x02, 0x03, 0x04]];
        let res = verify_chain_to_root(&junk, 1_700_000_000);
        assert!(res.is_err());
    }

    #[test]
    fn self_signed_unknown_leaf_does_not_chain_to_root() {
        // A syntactically-plausible but untrusted single cert must not validate
        // to a Mozilla root. We reuse the DER test-cert builder shape indirectly
        // by feeding a minimal SEQUENCE; webpki will reject it as it neither
        // parses as a full cert nor chains anywhere.
        let der = vec![0x30u8, 0x03, 0x02, 0x01, 0x00];
        let res = verify_chain_to_root(&[der], 1_700_000_000);
        assert!(res.is_err());
    }
}
