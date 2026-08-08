//! Minimal, defensive X.509 (RFC 5280) parsing for TLS peer certificates.
//!
//! Scope and honesty: this module extracts the fields needed to make *real*
//! trust decisions that do not require large-integer / elliptic-curve crypto —
//! namely the validity window (notBefore/notAfter), the subject CommonName, and
//! the subjectAltName dNSName entries — plus it captures the signature algorithm
//! OID and the raw subjectPublicKeyInfo so the signature-verification layer
//! ([`tls_sigverify`](super::tls_sigverify)) can consume them without re-parsing.
//!
//! What this module deliberately does NOT do by itself: verify the certificate
//! signature or build a chain to a trust anchor. Those steps live elsewhere —
//! the `CertificateVerify` signature is checked by
//! [`tls_sigverify`](super::tls_sigverify), and the chain is validated against
//! the Mozilla root program by [`tls_trust`](super::tls_trust). [`verify`] here
//! therefore fills in only the hostname + validity fields and leaves
//! `signature_verified`/`chain_verified`/`authenticated` for the handshake layer
//! to complete. A peer is reported `authenticated` only once hostname, validity,
//! chain, and signature have all passed; callers that require authentication
//! must fail closed otherwise.

/// Parsed subset of an X.509 certificate.
#[derive(Debug, Clone, Default)]
pub struct ParsedCertificate {
    /// notBefore as seconds since the Unix epoch.
    pub not_before: i64,
    /// notAfter as seconds since the Unix epoch.
    pub not_after: i64,
    /// Subject CommonName (CN), if present.
    pub subject_cn: Option<String>,
    /// subjectAltName dNSName entries.
    pub san_dns: Vec<String>,
    /// signatureAlgorithm OID (raw DER contents bytes).
    pub sig_alg_oid: Vec<u8>,
    /// Raw subjectPublicKeyInfo (DER TLV), for future signature verification.
    pub spki: Vec<u8>,
}

/// Parse error surface. Kept coarse on purpose — a malformed cert is simply
/// untrusted, and the reason is for diagnostics only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X509Error {
    Truncated,
    UnexpectedTag(u8),
    BadLength,
    BadTime,
    NoValidity,
}

// === DER reader =============================================================

/// A single DER TLV: tag byte + contents slice.
struct Tlv<'a> {
    tag: u8,
    contents: &'a [u8],
    /// Total bytes consumed (header + contents), for advancing the cursor.
    total: usize,
}

/// Read one DER TLV from the front of `data`.
fn read_tlv(data: &[u8]) -> Result<Tlv<'_>, X509Error> {
    if data.len() < 2 {
        return Err(X509Error::Truncated);
    }
    let tag = data[0];
    let first = data[1];
    let (len, header) = if first & 0x80 == 0 {
        // Short form.
        (first as usize, 2usize)
    } else {
        // Long form: low 7 bits = number of length octets.
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 {
            return Err(X509Error::BadLength);
        }
        if data.len() < 2 + n {
            return Err(X509Error::Truncated);
        }
        let mut len = 0usize;
        for &b in &data[2..2 + n] {
            len = (len << 8) | b as usize;
        }
        (len, 2 + n)
    };
    let end = header.checked_add(len).ok_or(X509Error::BadLength)?;
    if end > data.len() {
        return Err(X509Error::Truncated);
    }
    Ok(Tlv { tag, contents: &data[header..end], total: end })
}

const TAG_INTEGER: u8 = 0x02;
const TAG_OCTET_STRING: u8 = 0x04;
const TAG_OID: u8 = 0x06;
const TAG_UTF8: u8 = 0x0c;
const TAG_PRINTABLE: u8 = 0x13;
const TAG_IA5: u8 = 0x16;
const TAG_UTC_TIME: u8 = 0x17;
const TAG_GEN_TIME: u8 = 0x18;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_SET: u8 = 0x31;
const TAG_CTX_0: u8 = 0xa0; // [0] EXPLICIT (version)
const TAG_CTX_3: u8 = 0xa3; // [3] EXPLICIT (extensions)
const TAG_SAN_DNS: u8 = 0x82; // [2] IMPLICIT dNSName in GeneralName

// OIDs (DER contents, without tag/len).
const OID_CN: [u8; 3] = [0x55, 0x04, 0x03]; // 2.5.4.3
const OID_SAN: [u8; 3] = [0x55, 0x1d, 0x11]; // 2.5.29.17

/// Parse the subset of an X.509 certificate we can act on.
pub fn parse_certificate(der: &[u8]) -> Result<ParsedCertificate, X509Error> {
    let cert = read_tlv(der)?;
    if cert.tag != TAG_SEQUENCE {
        return Err(X509Error::UnexpectedTag(cert.tag));
    }
    // Certificate ::= SEQ { tbsCertificate, signatureAlgorithm, signatureValue }
    let tbs = read_tlv(cert.contents)?;
    if tbs.tag != TAG_SEQUENCE {
        return Err(X509Error::UnexpectedTag(tbs.tag));
    }
    // signatureAlgorithm follows the TBS within the outer SEQUENCE.
    let after_tbs = &cert.contents[tbs.total..];
    let mut out = ParsedCertificate::default();
    if let Ok(sig_alg) = read_tlv(after_tbs) {
        if sig_alg.tag == TAG_SEQUENCE {
            if let Ok(oid) = read_tlv(sig_alg.contents) {
                if oid.tag == TAG_OID {
                    out.sig_alg_oid = oid.contents.to_vec();
                }
            }
        }
    }

    // Walk the TBSCertificate fields in order.
    let mut cur = tbs.contents;
    // Optional [0] EXPLICIT version.
    let first = read_tlv(cur)?;
    if first.tag == TAG_CTX_0 {
        cur = &cur[first.total..];
    }
    // serialNumber INTEGER
    let serial = read_tlv(cur)?;
    if serial.tag != TAG_INTEGER {
        return Err(X509Error::UnexpectedTag(serial.tag));
    }
    cur = &cur[serial.total..];
    // signature AlgorithmIdentifier SEQ
    let inner_sig = read_tlv(cur)?;
    cur = &cur[inner_sig.total..];
    // issuer Name SEQ
    let issuer = read_tlv(cur)?;
    cur = &cur[issuer.total..];
    // validity SEQ { notBefore, notAfter }
    let validity = read_tlv(cur)?;
    if validity.tag != TAG_SEQUENCE {
        return Err(X509Error::NoValidity);
    }
    cur = &cur[validity.total..];
    let nb = read_tlv(validity.contents)?;
    let na = read_tlv(&validity.contents[nb.total..])?;
    out.not_before = parse_time(nb.tag, nb.contents)?;
    out.not_after = parse_time(na.tag, na.contents)?;
    // subject Name SEQ
    let subject = read_tlv(cur)?;
    cur = &cur[subject.total..];
    out.subject_cn = extract_cn(subject.contents);
    // subjectPublicKeyInfo SEQ
    let spki = read_tlv(cur)?;
    out.spki = der[..0].to_vec(); // placeholder; replaced below with real bytes
    // Reconstruct the full SPKI TLV bytes (tag+len+contents) for future use.
    {
        let start = der.len() - cur.len();
        let end = start + spki.total;
        out.spki = der[start..end].to_vec();
    }
    cur = &cur[spki.total..];
    // Optional [1] issuerUniqueID, [2] subjectUniqueID, then [3] extensions.
    while let Ok(tlv) = read_tlv(cur) {
        if tlv.tag == TAG_CTX_3 {
            out.san_dns = extract_san(tlv.contents);
            break;
        }
        if tlv.total == 0 || tlv.total > cur.len() {
            break;
        }
        cur = &cur[tlv.total..];
        if cur.is_empty() {
            break;
        }
    }

    Ok(out)
}

/// Extract the first CommonName from a Name (RDNSequence).
fn extract_cn(name_contents: &[u8]) -> Option<String> {
    // Name ::= SEQ OF RelativeDistinguishedName (SET OF AttributeTypeAndValue)
    let mut cur = name_contents;
    while let Ok(rdn) = read_tlv(cur) {
        if rdn.total == 0 || rdn.total > cur.len() {
            break;
        }
        if rdn.tag == TAG_SET {
            let mut acur = rdn.contents;
            while let Ok(atv) = read_tlv(acur) {
                if atv.total == 0 || atv.total > acur.len() {
                    break;
                }
                if atv.tag == TAG_SEQUENCE {
                    if let Ok(oid) = read_tlv(atv.contents) {
                        if oid.tag == TAG_OID && oid.contents == OID_CN {
                            if let Ok(val) = read_tlv(&atv.contents[oid.total..]) {
                                if matches!(val.tag, TAG_UTF8 | TAG_PRINTABLE | TAG_IA5) {
                                    return String::from_utf8(val.contents.to_vec()).ok();
                                }
                            }
                        }
                    }
                }
                acur = &acur[atv.total..];
            }
        }
        cur = &cur[rdn.total..];
    }
    None
}

/// Extract dNSName entries from the extensions block ([3] EXPLICIT).
fn extract_san(ext_ctx_contents: &[u8]) -> Vec<String> {
    // [3] wraps a single SEQUENCE OF Extension.
    let seq = match read_tlv(ext_ctx_contents) {
        Ok(t) if t.tag == TAG_SEQUENCE => t,
        _ => return Vec::new(),
    };
    let mut cur = seq.contents;
    while let Ok(ext) = read_tlv(cur) {
        if ext.total == 0 || ext.total > cur.len() {
            break;
        }
        if ext.tag == TAG_SEQUENCE {
            // Extension ::= SEQ { extnID OID, critical BOOL DEFAULT FALSE, extnValue OCTET STRING }
            if let Ok(oid) = read_tlv(ext.contents) {
                if oid.tag == TAG_OID && oid.contents == OID_SAN {
                    // Find the OCTET STRING (skip optional BOOLEAN).
                    let mut ecur = &ext.contents[oid.total..];
                    while let Ok(inner) = read_tlv(ecur) {
                        if inner.tag == TAG_OCTET_STRING {
                            return parse_general_names(inner.contents);
                        }
                        if inner.total == 0 || inner.total > ecur.len() {
                            break;
                        }
                        ecur = &ecur[inner.total..];
                    }
                }
            }
        }
        cur = &cur[ext.total..];
    }
    Vec::new()
}

/// Parse the GeneralNames SEQUENCE inside a SAN extnValue, returning dNSNames.
fn parse_general_names(octet_contents: &[u8]) -> Vec<String> {
    let seq = match read_tlv(octet_contents) {
        Ok(t) if t.tag == TAG_SEQUENCE => t,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    let mut cur = seq.contents;
    while let Ok(gn) = read_tlv(cur) {
        if gn.total == 0 || gn.total > cur.len() {
            break;
        }
        if gn.tag == TAG_SAN_DNS {
            if let Ok(s) = std::str::from_utf8(gn.contents) {
                out.push(s.to_string());
            }
        }
        cur = &cur[gn.total..];
    }
    out
}

/// Parse a UTCTime/GeneralizedTime into seconds since the Unix epoch.
fn parse_time(tag: u8, bytes: &[u8]) -> Result<i64, X509Error> {
    let s = std::str::from_utf8(bytes).map_err(|_| X509Error::BadTime)?;
    // Must end in 'Z' (UTC). We only support the Z form.
    let s = s.strip_suffix('Z').ok_or(X509Error::BadTime)?;
    let (year, rest) = match tag {
        TAG_UTC_TIME => {
            // YYMMDDHHMMSS — 2-digit year, pivot at 2000/1950 per RFC 5280.
            if s.len() < 10 {
                return Err(X509Error::BadTime);
            }
            let yy: i64 = s[0..2].parse().map_err(|_| X509Error::BadTime)?;
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            (year, &s[2..])
        }
        TAG_GEN_TIME => {
            if s.len() < 12 {
                return Err(X509Error::BadTime);
            }
            let year: i64 = s[0..4].parse().map_err(|_| X509Error::BadTime)?;
            (year, &s[4..])
        }
        _ => return Err(X509Error::UnexpectedTag(tag)),
    };
    let two = |sl: &str| -> Result<i64, X509Error> {
        sl.parse::<i64>().map_err(|_| X509Error::BadTime)
    };
    if rest.len() < 8 {
        return Err(X509Error::BadTime);
    }
    let month = two(&rest[0..2])?;
    let day = two(&rest[2..4])?;
    let hour = two(&rest[4..6])?;
    let min = two(&rest[6..8])?;
    let sec = if rest.len() >= 10 { two(&rest[8..10])? } else { 0 };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(X509Error::BadTime);
    }
    Ok(civil_to_unix(year, month, day, hour, min, sec))
}

/// Days-from-civil (Howard Hinnant's algorithm) → Unix seconds.
fn civil_to_unix(year: i64, month: i64, day: i64, hour: i64, min: i64, sec: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + hour * 3600 + min * 60 + sec
}

// === Verification ===========================================================

/// The outcome of examining a peer certificate. `authenticated` is the only
/// field callers should gate real trust on. [`verify`] fills in the hostname
/// and validity checks and leaves `signature_verified`/`chain_verified`/
/// `authenticated` false; the handshake layer performs the `CertificateVerify`
/// signature check ([`tls_sigverify`](super::tls_sigverify)) and the chain build
/// ([`tls_trust`](super::tls_trust)) and upgrades the verdict on success.
#[derive(Debug, Clone)]
pub struct CertVerdict {
    pub hostname_matched: bool,
    pub time_valid: bool,
    pub signature_verified: bool,
    /// True once the leaf has been shown to chain to a trusted root.
    pub chain_verified: bool,
    pub authenticated: bool,
    pub reason: String,
}

/// Verify a parsed certificate against `hostname` at time `now_unix` (seconds).
///
/// Performs the checks this function can do standalone (hostname + validity
/// window). It does not have the handshake transcript or the `CertificateVerify`
/// signature, so it leaves `signature_verified`/`authenticated` false; the
/// handshake layer performs that step and upgrades the verdict.
pub fn verify(cert: &ParsedCertificate, hostname: &str, now_unix: i64) -> CertVerdict {
    let time_valid = now_unix >= cert.not_before && now_unix <= cert.not_after;
    let hostname_matched = matches_hostname(cert, hostname);
    let signature_verified = false; // completed later by the handshake layer
    let chain_verified = false; // completed later by tls_trust
    let authenticated = signature_verified && chain_verified && time_valid && hostname_matched;

    let reason = if !time_valid {
        format!("certificate not within validity window (now={now_unix})")
    } else if !hostname_matched {
        format!("hostname '{hostname}' does not match certificate")
    } else {
        "hostname and validity OK; signature/chain not verified".to_string()
    };

    CertVerdict {
        hostname_matched,
        time_valid,
        signature_verified,
        chain_verified,
        authenticated,
        reason,
    }
}

/// True if `hostname` matches the cert's SAN dNSNames (or CN as legacy fallback).
fn matches_hostname(cert: &ParsedCertificate, hostname: &str) -> bool {
    let host = hostname.trim_end_matches('.').to_ascii_lowercase();
    if cert.san_dns.iter().any(|p| host_matches(p, &host)) {
        return true;
    }
    // Legacy CN fallback only when no SANs are present.
    if cert.san_dns.is_empty() {
        if let Some(cn) = &cert.subject_cn {
            return host_matches(cn, &host);
        }
    }
    false
}

/// Match one presented identity `pattern` against `host` (already lowercased),
/// honoring a single leftmost-label wildcard (`*.example.com`).
fn host_matches(pattern: &str, host: &str) -> bool {
    let pat = pattern.trim_end_matches('.').to_ascii_lowercase();
    if let Some(rest) = pat.strip_prefix("*.") {
        // Wildcard matches exactly one leftmost label, and only if the
        // remainder has at least two labels (no "*.com").
        if rest.split('.').count() < 2 {
            return false;
        }
        match host.split_once('.') {
            Some((_label, tail)) => tail == rest,
            None => false,
        }
    } else {
        pat == host
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tiny DER builder for constructing test certificates ---------------

    fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
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

    fn utc_time(s: &str) -> Vec<u8> {
        tlv(TAG_UTC_TIME, format!("{s}Z").as_bytes())
    }

    fn cn_rdn(name: &str) -> Vec<u8> {
        let atv = tlv(
            TAG_SEQUENCE,
            &[tlv(TAG_OID, &OID_CN), tlv(TAG_UTF8, name.as_bytes())].concat(),
        );
        tlv(TAG_SET, &atv)
    }

    fn san_ext(dns: &[&str]) -> Vec<u8> {
        let names: Vec<u8> = dns
            .iter()
            .flat_map(|d| tlv(TAG_SAN_DNS, d.as_bytes()))
            .collect();
        let general_names = tlv(TAG_SEQUENCE, &names);
        let octet = tlv(TAG_OCTET_STRING, &general_names);
        let ext = tlv(TAG_SEQUENCE, &[tlv(TAG_OID, &OID_SAN), octet].concat());
        let ext_seq = tlv(TAG_SEQUENCE, &ext);
        tlv(TAG_CTX_3, &ext_seq)
    }

    fn build_cert(nb: &str, na: &str, cn: &str, san: &[&str]) -> Vec<u8> {
        let version = tlv(TAG_CTX_0, &tlv(TAG_INTEGER, &[0x02]));
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2a, 0x86, 0x48]));
        let issuer = tlv(TAG_SEQUENCE, &cn_rdn("Test CA"));
        let validity = tlv(TAG_SEQUENCE, &[utc_time(nb), utc_time(na)].concat());
        let subject = tlv(TAG_SEQUENCE, &cn_rdn(cn));
        let spki = tlv(
            TAG_SEQUENCE,
            &[
                tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2a, 0x86, 0x48])),
                tlv(0x03, &[0x00, 0xde, 0xad, 0xbe, 0xef]),
            ]
            .concat(),
        );
        let mut tbs_body = Vec::new();
        tbs_body.extend_from_slice(&version);
        tbs_body.extend_from_slice(&serial);
        tbs_body.extend_from_slice(&sig);
        tbs_body.extend_from_slice(&issuer);
        tbs_body.extend_from_slice(&validity);
        tbs_body.extend_from_slice(&subject);
        tbs_body.extend_from_slice(&spki);
        if !san.is_empty() {
            tbs_body.extend_from_slice(&san_ext(san));
        }
        let tbs = tlv(TAG_SEQUENCE, &tbs_body);
        let outer_sig = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2a, 0x86, 0x48]));
        let sig_val = tlv(0x03, &[0x00, 0x01, 0x02]);
        tlv(TAG_SEQUENCE, &[tbs, outer_sig, sig_val].concat())
    }

    #[test]
    fn parses_validity_cn_and_san() {
        let der = build_cert(
            "240101000000",
            "260101000000",
            "example.com",
            &["example.com", "www.example.com"],
        );
        let cert = parse_certificate(&der).expect("parse");
        assert_eq!(cert.subject_cn.as_deref(), Some("example.com"));
        assert_eq!(cert.san_dns, vec!["example.com", "www.example.com"]);
        // 2024-01-01 .. 2026-01-01
        assert_eq!(cert.not_before, civil_to_unix(2024, 1, 1, 0, 0, 0));
        assert_eq!(cert.not_after, civil_to_unix(2026, 1, 1, 0, 0, 0));
        assert!(!cert.spki.is_empty());
    }

    #[test]
    fn verify_ok_within_window_and_hostname() {
        let der = build_cert("240101000000", "260101000000", "example.com", &["example.com"]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "example.com", now);
        assert!(v.time_valid);
        assert!(v.hostname_matched);
        // Never authenticated: no signature verification available.
        assert!(!v.authenticated);
        assert!(!v.signature_verified);
    }

    #[test]
    fn verify_rejects_expired() {
        let der = build_cert("200101000000", "210101000000", "example.com", &["example.com"]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "example.com", now);
        assert!(!v.time_valid);
        assert!(!v.authenticated);
    }

    #[test]
    fn verify_rejects_wrong_hostname() {
        let der = build_cert("240101000000", "260101000000", "example.com", &["example.com"]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "evil.com", now);
        assert!(!v.hostname_matched);
    }

    #[test]
    fn wildcard_matches_one_label_only() {
        assert!(host_matches("*.example.com", "www.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        assert!(!host_matches("*.example.com", "a.b.example.com"));
        assert!(!host_matches("*.com", "example.com"));
        assert!(host_matches("Example.COM", "example.com"));
    }

    #[test]
    fn san_takes_precedence_over_cn() {
        // CN says example.com but SAN only lists other.com → example.com fails.
        let der = build_cert("240101000000", "260101000000", "example.com", &["other.com"]);
        let cert = parse_certificate(&der).unwrap();
        assert!(!matches_hostname(&cert, "example.com"));
        assert!(matches_hostname(&cert, "other.com"));
    }

    #[test]
    fn malformed_is_error_not_panic() {
        assert!(parse_certificate(&[]).is_err());
        assert!(parse_certificate(&[0x30, 0x82, 0xff]).is_err());
        assert!(parse_certificate(&[0x02, 0x01, 0x00]).is_err());
    }

    #[test]
    fn verify_before_validity_window() {
        let der = build_cert("250101000000", "260101000000", "example.com", &["example.com"]);
        let cert = parse_certificate(&der).unwrap();
        // now = 2024-06-01, which is before notBefore (2025-01-01)
        let now = civil_to_unix(2024, 6, 1, 0, 0, 0);
        let v = verify(&cert, "example.com", now);
        assert!(!v.time_valid);
        assert!(v.reason.contains("validity window"));
    }

    #[test]
    fn verify_cn_fallback_when_no_san() {
        let der = build_cert("240101000000", "260101000000", "example.com", &[]);
        let cert = parse_certificate(&der).unwrap();
        assert!(cert.san_dns.is_empty());
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "example.com", now);
        assert!(v.hostname_matched); // CN fallback used
    }

    #[test]
    fn verify_cn_fallback_rejects_wrong_host() {
        let der = build_cert("240101000000", "260101000000", "example.com", &[]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "evil.com", now);
        assert!(!v.hostname_matched);
    }

    #[test]
    fn wildcard_san_matches_subdomain() {
        let der = build_cert("240101000000", "260101000000", "CA", &["*.example.com"]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "www.example.com", now);
        assert!(v.hostname_matched);
    }

    #[test]
    fn wildcard_san_rejects_bare_domain() {
        let der = build_cert("240101000000", "260101000000", "CA", &["*.example.com"]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "example.com", now);
        assert!(!v.hostname_matched);
    }

    #[test]
    fn verify_at_exact_boundaries() {
        let der = build_cert("240101000000", "260101000000", "example.com", &["example.com"]);
        let cert = parse_certificate(&der).unwrap();
        // At notBefore exactly
        let v = verify(&cert, "example.com", cert.not_before);
        assert!(v.time_valid);
        // At notAfter exactly
        let v = verify(&cert, "example.com", cert.not_after);
        assert!(v.time_valid);
    }

    #[test]
    fn civil_to_unix_known_epoch() {
        // 1970-01-01 00:00:00 UTC = Unix epoch = 0
        assert_eq!(civil_to_unix(1970, 1, 1, 0, 0, 0), 0);
    }

    #[test]
    fn civil_to_unix_known_date() {
        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(civil_to_unix(2000, 1, 1, 0, 0, 0), 946684800);
    }

    #[test]
    fn parse_time_utc_year_pivot() {
        // YY=99 → 1999, YY=00 → 2000, YY=49 → 2049
        let t99 = parse_time(TAG_UTC_TIME, b"990101000000Z").unwrap();
        let t00 = parse_time(TAG_UTC_TIME, b"000101000000Z").unwrap();
        let t49 = parse_time(TAG_UTC_TIME, b"490101000000Z").unwrap();
        assert_eq!(civil_to_unix(1999, 1, 1, 0, 0, 0), t99);
        assert_eq!(civil_to_unix(2000, 1, 1, 0, 0, 0), t00);
        assert_eq!(civil_to_unix(2049, 1, 1, 0, 0, 0), t49);
    }

    #[test]
    fn parse_time_generalized_time() {
        let t = parse_time(TAG_GEN_TIME, b"20250601120000Z").unwrap();
        assert_eq!(t, civil_to_unix(2025, 6, 1, 12, 0, 0));
    }

    #[test]
    fn parse_time_rejects_no_z_suffix() {
        let result = parse_time(TAG_UTC_TIME, b"240101000000");
        assert_eq!(result, Err(X509Error::BadTime));
    }

    #[test]
    fn parse_time_rejects_invalid_month() {
        let result = parse_time(TAG_UTC_TIME, b"241301000000Z");
        assert_eq!(result, Err(X509Error::BadTime));
    }

    #[test]
    fn parse_time_rejects_invalid_day() {
        let result = parse_time(TAG_UTC_TIME, b"240132000000Z");
        assert_eq!(result, Err(X509Error::BadTime));
    }

    #[test]
    fn host_matches_exact_and_wildcard() {
        assert!(host_matches("example.com", "example.com"));
        assert!(!host_matches("example.com", "other.com"));
        assert!(host_matches("*.example.com", "sub.example.com"));
        assert!(!host_matches("*.com", "example.com")); // single-label rest rejected
    }

    #[test]
    fn trailing_dot_stripped_from_hostname() {
        // Trailing FQDN dots should be stripped
        let der = build_cert("240101000000", "260101000000", "CA", &["example.com"]);
        let cert = parse_certificate(&der).unwrap();
        let now = civil_to_unix(2025, 6, 1, 0, 0, 0);
        let v = verify(&cert, "example.com.", now);
        assert!(v.hostname_matched);
    }
}
