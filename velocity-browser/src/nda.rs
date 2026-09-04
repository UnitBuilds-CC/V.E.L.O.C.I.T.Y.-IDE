use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Compact binary NDA representation for browser state & AOM layout facts
#[derive(Debug, Clone, PartialEq)]
pub struct NdaTriple {
    pub subject_hash: u64,
    pub predicate_id: u16,
    pub object_hash: u64,
}

impl NdaTriple {
    pub fn new(subject: &str, predicate: u16, object: &str) -> Self {
        Self {
            subject_hash: hash_str(subject),
            predicate_id: predicate,
            object_hash: hash_str(object),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(18);
        buf.extend_from_slice(&self.subject_hash.to_le_bytes());
        buf.extend_from_slice(&self.predicate_id.to_le_bytes());
        buf.extend_from_slice(&self.object_hash.to_le_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 18 {
            return Err("Buffer too small for NDA triple");
        }
        let subject_hash = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let predicate_id = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let object_hash = u64::from_le_bytes(bytes[10..18].try_into().unwrap());
        Ok(Self {
            subject_hash,
            predicate_id,
            object_hash,
        })
    }
}

/// The object side of a lossless fact: either an interned string (recoverable
/// text) or an inline integer literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NdaObject {
    /// Index into the owning [`NdaDocument`]'s dictionary.
    Str(u32),
    /// Inline signed integer literal (counts, scores, coordinates).
    Int(i64),
}

/// String interner: assigns stable `u32` ids to strings and de-duplicates, so
/// repeated names/roles/text cost one dictionary entry instead of many copies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NdaDictionary {
    strings: Vec<String>,
    index: std::collections::HashMap<String, u32>,
}

impl NdaDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `s`, returning its stable id. Idempotent for equal strings.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), id);
        id
    }

    /// Recover the original string for an id.
    pub fn resolve(&self, id: u32) -> Option<&str> {
        self.strings.get(id as usize).map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

/// A lossless fact: `subject predicate object` where the subject is a
/// dictionary id and the object preserves its literal value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdaFact {
    pub subject: u32,
    pub predicate: u16,
    pub object: NdaObject,
}

/// A self-contained, agent-readable state document: a string dictionary plus a
/// list of facts. Unlike [`NdaTriple`] (which hashes its object and is
/// therefore lossy), an `NdaDocument` round-trips the original strings, so an
/// agent can actually *read* the DOM/canvas content the facts describe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NdaDocument {
    pub dict: NdaDictionary,
    pub facts: Vec<NdaFact>,
}

/// Magic bytes opening a secured NDA envelope: ASCII `"NDA1"`, matching the
/// original V.E.L.O.C.I.T.Y. Neural Document Architecture header.
pub const NDA_MAGIC: [u8; 4] = *b"NDA1";

/// `Flags` bit: the body is sealed with an AES-256-GCM AEAD.
pub const NDA_FLAG_ENCRYPTED: u32 = 0x0000_0001;

/// Fixed size of the secured envelope header:
/// magic(4) + flags(4) + merkle_root(32) + fact_count(4) + nonce(12).
pub const NDA_HEADER_LEN: usize = 4 + 4 + 32 + 4 + 12;

impl NdaDocument {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a fact whose object is a string literal.
    pub fn push_str(&mut self, subject: &str, predicate: u16, object: &str) {
        let subject = self.dict.intern(subject);
        let object = NdaObject::Str(self.dict.intern(object));
        self.facts.push(NdaFact {
            subject,
            predicate,
            object,
        });
    }

    /// Append a fact whose object is an integer literal.
    pub fn push_int(&mut self, subject: &str, predicate: u16, object: i64) {
        let subject = self.dict.intern(subject);
        self.facts.push(NdaFact {
            subject,
            predicate,
            object: NdaObject::Int(object),
        });
    }

    /// Absorb every fact from `other` into `self`, re-interning its strings so
    /// dictionary ids stay valid. Lets each subsystem build its own document
    /// independently and have the session compose them losslessly.
    pub fn merge(&mut self, other: &NdaDocument) {
        for fact in &other.facts {
            let subject = match other.dict.resolve(fact.subject) {
                Some(s) => self.dict.intern(s),
                None => continue,
            };
            let object = match &fact.object {
                NdaObject::Str(id) => match other.dict.resolve(*id) {
                    Some(s) => NdaObject::Str(self.dict.intern(s)),
                    None => continue,
                },
                NdaObject::Int(n) => NdaObject::Int(*n),
            };
            self.facts.push(NdaFact {
                subject,
                predicate: fact.predicate,
                object,
            });
        }
    }

    /// Resolve a fact's subject to its original string.
    pub fn subject_str(&self, fact: &NdaFact) -> Option<&str> {
        self.dict.resolve(fact.subject)
    }

    /// Resolve a fact's object to a displayable string (ints are formatted).
    pub fn object_display(&self, fact: &NdaFact) -> Option<String> {
        match &fact.object {
            NdaObject::Str(id) => self.dict.resolve(*id).map(|s| s.to_string()),
            NdaObject::Int(n) => Some(n.to_string()),
        }
    }

    /// Resolve every fact to its `(subject, predicate, object)` string form.
    /// Facts whose dictionary ids cannot be resolved are skipped. This is the
    /// canonical view used to diff two documents into an [`crate::agent_api::NdaDelta`].
    pub fn readable_facts(&self) -> Vec<(String, u16, String)> {
        let mut out = Vec::with_capacity(self.facts.len());
        for fact in &self.facts {
            let (Some(subject), Some(object)) = (self.subject_str(fact), self.object_display(fact))
            else {
                continue;
            };
            out.push((subject.to_string(), fact.predicate, object));
        }
        out
    }

    /// Render all facts as compact `subject|predicate-name|object` lines — the
    /// token-cheapest readable serialization for LLM consumption.
    pub fn facts_text(&self) -> String {
        let facts = self.readable_facts();
        let mut out = String::with_capacity(facts.len() * 40);
        for (subject, predicate, object) in facts {
            out.push_str(&subject);
            out.push('|');
            out.push_str(crate::predicates::predicate_name(predicate));
            out.push('|');
            out.push_str(&object);
            out.push('\n');
        }
        out
    }

    /// Serialize to a self-describing binary stream (little-endian):
    /// `[dict_len u32] { [str_len u32][utf8 bytes] }* [fact_len u32]`
    /// `{ [subject u32][predicate u16][tag u8][payload] }*` where tag 0 => Str
    /// (payload: str id u32), tag 1 => Int (payload: i64).
    pub fn to_binary_stream(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.dict.strings.len() as u32).to_le_bytes());
        for s in &self.dict.strings {
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        buf.extend_from_slice(&(self.facts.len() as u32).to_le_bytes());
        for f in &self.facts {
            buf.extend_from_slice(&f.subject.to_le_bytes());
            buf.extend_from_slice(&f.predicate.to_le_bytes());
            match &f.object {
                NdaObject::Str(id) => {
                    buf.push(0u8);
                    buf.extend_from_slice(&id.to_le_bytes());
                }
                NdaObject::Int(n) => {
                    buf.push(1u8);
                    buf.extend_from_slice(&n.to_le_bytes());
                }
            }
        }
        buf
    }

    /// Parse a stream produced by [`NdaDocument::to_binary_stream`].
    pub fn from_binary_stream(stream: &[u8]) -> Result<Self, &'static str> {
        let mut pos = 0usize;
        let read_u32 = |stream: &[u8], pos: &mut usize| -> Result<u32, &'static str> {
            if *pos + 4 > stream.len() {
                return Err("unexpected end of NDA stream (u32)");
            }
            let v = u32::from_le_bytes(stream[*pos..*pos + 4].try_into().unwrap());
            *pos += 4;
            Ok(v)
        };

        let dict_len = read_u32(stream, &mut pos)? as usize;
        let mut strings = Vec::with_capacity(dict_len);
        let mut index = std::collections::HashMap::with_capacity(dict_len);
        for i in 0..dict_len {
            let slen = read_u32(stream, &mut pos)? as usize;
            if pos + slen > stream.len() {
                return Err("unexpected end of NDA stream (string bytes)");
            }
            let s = String::from_utf8(stream[pos..pos + slen].to_vec())
                .map_err(|_| "invalid utf8 in NDA dictionary")?;
            pos += slen;
            index.insert(s.clone(), i as u32);
            strings.push(s);
        }

        let fact_len = read_u32(stream, &mut pos)? as usize;
        let mut facts = Vec::with_capacity(fact_len);
        for _ in 0..fact_len {
            let subject = read_u32(stream, &mut pos)?;
            if pos + 2 > stream.len() {
                return Err("unexpected end of NDA stream (predicate)");
            }
            let predicate = u16::from_le_bytes(stream[pos..pos + 2].try_into().unwrap());
            pos += 2;
            if pos + 1 > stream.len() {
                return Err("unexpected end of NDA stream (object tag)");
            }
            let tag = stream[pos];
            pos += 1;
            let object = match tag {
                0 => NdaObject::Str(read_u32(stream, &mut pos)?),
                1 => {
                    if pos + 8 > stream.len() {
                        return Err("unexpected end of NDA stream (int payload)");
                    }
                    let n = i64::from_le_bytes(stream[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    NdaObject::Int(n)
                }
                _ => return Err("unknown NDA object tag"),
            };
            facts.push(NdaFact {
                subject,
                predicate,
                object,
            });
        }

        Ok(Self {
            dict: NdaDictionary { strings, index },
            facts,
        })
    }

    /// Compute the SHA-256 Merkle root over the document's readable facts,
    /// mirroring the original NDA design: each fact hashes to a leaf
    /// `SHA-256("subject|predicate|object")`, then leaves are pairwise-hashed
    /// up to a single 32-byte root (an odd node is promoted unchanged). An
    /// empty document has the all-zero root. Any change to any fact changes
    /// the root, making tampering detectable (integrity / anti-drift).
    pub fn merkle_root(&self) -> [u8; 32] {
        use crate::net::tls13::sha256;
        let facts = self.readable_facts();
        if facts.is_empty() {
            return [0u8; 32];
        }
        let mut level: Vec<[u8; 32]> = facts
            .iter()
            .map(|(s, p, o)| sha256(format!("{s}|{p}|{o}").as_bytes()))
            .collect();
        while level.len() > 1 {
            let mut parents = Vec::with_capacity(level.len().div_ceil(2));
            let mut i = 0;
            while i < level.len() {
                if i + 1 < level.len() {
                    let mut concat = [0u8; 64];
                    concat[..32].copy_from_slice(&level[i]);
                    concat[32..].copy_from_slice(&level[i + 1]);
                    parents.push(sha256(&concat));
                } else {
                    parents.push(level[i]);
                }
                i += 2;
            }
            level = parents;
        }
        level[0]
    }

    /// Serialize into a secured `NDA1` envelope that is both tamper-evident and
    /// confidential. The header (magic, flags, Merkle root, fact count, nonce)
    /// is authenticated as AEAD additional-data, and the body
    /// ([`Self::to_binary_stream`]) is encrypted with AES-256-GCM.
    ///
    /// Layout: `header(56) || ciphertext || tag(16)`.
    ///
    /// The `nonce` MUST be unique for a given `key` (nonce reuse breaks the
    /// AEAD). Callers that lack a secure random source should derive a fresh
    /// nonce per write (e.g. a monotonic counter) rather than a constant.
    pub fn seal(&self, key: &[u8; 32], nonce: &[u8; 12]) -> Vec<u8> {
        let body = self.to_binary_stream();
        let root = self.merkle_root();

        let mut header = Vec::with_capacity(NDA_HEADER_LEN);
        header.extend_from_slice(&NDA_MAGIC);
        header.extend_from_slice(&NDA_FLAG_ENCRYPTED.to_le_bytes());
        header.extend_from_slice(&root);
        header.extend_from_slice(&(self.facts.len() as u32).to_le_bytes());
        header.extend_from_slice(nonce);

        let (ciphertext, tag) = crate::net::aes_gcm::aes256_gcm_encrypt(key, nonce, &header, &body);

        let mut out = header;
        out.extend_from_slice(&ciphertext);
        out.extend_from_slice(&tag);
        out
    }

    /// Open a secured `NDA1` envelope produced by [`Self::seal`]: authenticate
    /// and decrypt the body, then recompute the Merkle root and confirm it
    /// matches the (authenticated) header. Returns an error on a wrong key, a
    /// tampered header/body, or an integrity mismatch.
    pub fn open(key: &[u8; 32], bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < NDA_HEADER_LEN + 16 {
            return Err("NDA envelope too small");
        }
        if bytes[0..4] != NDA_MAGIC {
            return Err("bad NDA magic");
        }
        let flags = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if flags & NDA_FLAG_ENCRYPTED == 0 {
            return Err("NDA envelope is not encrypted");
        }
        if flags & NDA_FLAG_RAW != 0 {
            return Err("NDA envelope is a raw-bytes container, not a document");
        }
        let mut root = [0u8; 32];
        root.copy_from_slice(&bytes[8..40]);
        let fact_count = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bytes[44..NDA_HEADER_LEN]);

        let header = &bytes[0..NDA_HEADER_LEN];
        let ct_and_tag = &bytes[NDA_HEADER_LEN..];
        let split = ct_and_tag.len() - 16;
        let (ciphertext, tag_slice) = ct_and_tag.split_at(split);
        let mut tag = [0u8; 16];
        tag.copy_from_slice(tag_slice);

        let body = crate::net::aes_gcm::aes256_gcm_decrypt(key, &nonce, header, ciphertext, &tag)
            .ok_or("NDA authentication failed (tampered or wrong key)")?;

        let doc = Self::from_binary_stream(&body)?;
        if doc.facts.len() as u32 != fact_count {
            return Err("NDA fact count mismatch");
        }
        if doc.merkle_root() != root {
            return Err("NDA Merkle root mismatch");
        }
        Ok(doc)
    }
}

/// Derive a 32-byte NDA encryption key from a passphrase and salt via HKDF
/// (SHA-256). This gives callers a real KDF instead of raw key bytes; the
/// passphrase/salt provisioning (env var, OS keyring, workspace secret) is an
/// operational choice made by the caller, not baked in here.
pub fn derive_nda_key(passphrase: &[u8], salt: &[u8]) -> [u8; 32] {
    let prk = crate::net::tls13::hkdf_extract(salt, passphrase);
    let okm = crate::net::tls13::hkdf_expand_label(&prk, "nda key", &[], 32);
    let mut key = [0u8; 32];
    key.copy_from_slice(&okm);
    key
}

/// `Flags` bit: the payload is opaque bytes whose integrity is a SHA-256 over
/// the plaintext (as opposed to an [`NdaDocument`], whose integrity is a Merkle
/// root over its facts). Set by [`seal_bytes`].
pub const NDA_FLAG_RAW: u32 = 0x0000_0002;

/// Seal arbitrary bytes into a secured `NDA1` envelope for encryption at rest.
/// This is the general-purpose path for persisting heterogeneous artifacts
/// (sitemaps, chat logs, transcripts) whose payloads are not themselves
/// [`NdaDocument`]s. Integrity is a SHA-256 over the plaintext payload;
/// confidentiality is AES-256-GCM, with the header bound as AEAD
/// additional-data. Layout: `header(56) || ciphertext || tag(16)`.
///
/// The `nonce` MUST be unique for a given `key` (nonce reuse breaks the AEAD).
pub fn seal_bytes(key: &[u8; 32], nonce: &[u8; 12], payload: &[u8]) -> Vec<u8> {
    let hash = crate::net::tls13::sha256(payload);
    let mut header = Vec::with_capacity(NDA_HEADER_LEN);
    header.extend_from_slice(&NDA_MAGIC);
    header.extend_from_slice(&(NDA_FLAG_ENCRYPTED | NDA_FLAG_RAW).to_le_bytes());
    header.extend_from_slice(&hash);
    header.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    header.extend_from_slice(nonce);

    let (ciphertext, tag) = crate::net::aes_gcm::aes256_gcm_encrypt(key, nonce, &header, payload);

    let mut out = header;
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&tag);
    out
}

/// Open a secured raw-bytes envelope produced by [`seal_bytes`]: authenticate
/// and decrypt, then confirm the plaintext length and SHA-256 match the
/// authenticated header. Errors on a wrong key, tampering, or an integrity
/// mismatch.
pub fn open_bytes(key: &[u8; 32], bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    if bytes.len() < NDA_HEADER_LEN + 16 {
        return Err("NDA envelope too small");
    }
    if bytes[0..4] != NDA_MAGIC {
        return Err("bad NDA magic");
    }
    let flags = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4]));
    if flags & NDA_FLAG_ENCRYPTED == 0 {
        return Err("NDA envelope is not encrypted");
    }
    if flags & NDA_FLAG_RAW == 0 {
        return Err("NDA envelope is not a raw-bytes container");
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes[8..40]);
    let payload_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap_or([0; 4])) as usize;
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&bytes[44..NDA_HEADER_LEN]);

    let header = &bytes[0..NDA_HEADER_LEN];
    let ct_and_tag = &bytes[NDA_HEADER_LEN..];
    let split = ct_and_tag.len() - 16;
    let (ciphertext, tag_slice) = ct_and_tag.split_at(split);
    let mut tag = [0u8; 16];
    tag.copy_from_slice(tag_slice);

    let payload = crate::net::aes_gcm::aes256_gcm_decrypt(key, &nonce, header, ciphertext, &tag)
        .ok_or("NDA authentication failed (tampered or wrong key)")?;

    if payload.len() != payload_len {
        return Err("NDA payload length mismatch");
    }
    if crate::net::tls13::sha256(&payload) != hash {
        return Err("NDA content hash mismatch");
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nda_triple_serialization() {
        let triple = NdaTriple::new("http://example.com", 1, "test_page");
        let bytes = triple.to_bytes();
        let decoded = NdaTriple::from_bytes(&bytes).unwrap();
        assert_eq!(triple, decoded);
    }

    #[test]
    fn facts_text_uses_predicate_names() {
        let mut doc = NdaDocument::new();
        doc.push_str("page", crate::predicates::SESSION_TITLE, "My Title");
        doc.push_int("page", crate::predicates::SESSION_LINK_COUNT, 3);
        let text = doc.facts_text();
        assert_eq!(text, "page|title|My Title\npage|links|3\n");
    }

    #[test]
    fn document_roundtrips_literals() {
        let mut doc = NdaDocument::new();
        doc.push_str("node_1", 11, "Submit");
        doc.push_int("node_1", 13, 100);
        let stream = doc.to_binary_stream();
        let decoded = NdaDocument::from_binary_stream(&stream).unwrap();
        assert_eq!(doc, decoded);
        // The literal "Submit" is recoverable, not a hash.
        let name_fact = &decoded.facts[0];
        assert_eq!(decoded.object_display(name_fact).as_deref(), Some("Submit"));
        assert_eq!(decoded.subject_str(name_fact), Some("node_1"));
    }

    #[test]
    fn dictionary_dedups_repeated_strings() {
        let mut doc = NdaDocument::new();
        doc.push_str("node_1", 10, "button");
        doc.push_str("node_2", 10, "button");
        doc.push_str("node_1", 11, "OK");
        // "node_1" and "button" each interned once; plus "node_2" and "OK".
        assert_eq!(doc.dict.len(), 4);
    }

    #[test]
    fn document_rejects_truncated_stream() {
        let mut doc = NdaDocument::new();
        doc.push_str("s", 1, "o");
        let mut stream = doc.to_binary_stream();
        stream.truncate(stream.len() - 1);
        assert!(NdaDocument::from_binary_stream(&stream).is_err());
    }

    #[test]
    fn merge_preserves_literals_and_reinterns() {
        let mut a = NdaDocument::new();
        a.push_str("node_1", 10, "button");
        let mut b = NdaDocument::new();
        b.push_str("node_2", 11, "Submit");
        b.push_int("node_2", 13, 100);
        a.merge(&b);
        assert_eq!(a.facts.len(), 3);
        let submit = &a.facts[1];
        assert_eq!(a.subject_str(submit), Some("node_2"));
        assert_eq!(a.object_display(submit).as_deref(), Some("Submit"));
        let score = &a.facts[2];
        assert_eq!(a.object_display(score).as_deref(), Some("100"));
    }

    fn sample_doc() -> NdaDocument {
        let mut doc = NdaDocument::new();
        doc.push_str("node_1", 11, "Submit");
        doc.push_int("node_1", 13, 100);
        doc.push_str("node_2", 10, "button");
        doc
    }

    #[test]
    fn empty_document_has_zero_merkle_root() {
        assert_eq!(NdaDocument::new().merkle_root(), [0u8; 32]);
    }

    #[test]
    fn merkle_root_changes_when_a_fact_changes() {
        let a = sample_doc();
        let mut b = sample_doc();
        b.push_str("node_2", 11, "Cancel");
        assert_ne!(a.merkle_root(), b.merkle_root());
        // Recomputing the same content is deterministic.
        assert_eq!(a.merkle_root(), sample_doc().merkle_root());
    }

    #[test]
    fn sealed_envelope_starts_with_magic_and_round_trips() {
        let doc = sample_doc();
        let key = [0x11u8; 32];
        let nonce = [0x22u8; 12];
        let sealed = doc.seal(&key, &nonce);
        assert_eq!(&sealed[0..4], b"NDA1");
        // Ciphertext must not expose the plaintext content.
        assert!(!sealed.windows(6).any(|w| w == b"Submit"));
        let opened = NdaDocument::open(&key, &sealed).unwrap();
        assert_eq!(doc, opened);
    }

    #[test]
    fn open_with_wrong_key_fails() {
        let doc = sample_doc();
        let sealed = doc.seal(&[0x11u8; 32], &[0x22u8; 12]);
        assert!(NdaDocument::open(&[0x99u8; 32], &sealed).is_err());
    }

    #[test]
    fn tampering_with_body_is_detected() {
        let doc = sample_doc();
        let key = [0x11u8; 32];
        let mut sealed = doc.seal(&key, &[0x22u8; 12]);
        let last = sealed.len() - 20; // flip a byte inside the ciphertext
        sealed[last] ^= 0x01;
        assert!(NdaDocument::open(&key, &sealed).is_err());
    }

    #[test]
    fn tampering_with_authenticated_header_is_detected() {
        let doc = sample_doc();
        let key = [0x11u8; 32];
        let mut sealed = doc.seal(&key, &[0x22u8; 12]);
        sealed[8] ^= 0x01; // flip a byte of the Merkle root in the header (AAD)
        assert!(NdaDocument::open(&key, &sealed).is_err());
    }

    #[test]
    fn derive_nda_key_is_deterministic_and_salt_sensitive() {
        let k1 = derive_nda_key(b"correct horse battery staple", b"velocity-nda");
        let k2 = derive_nda_key(b"correct horse battery staple", b"velocity-nda");
        let k3 = derive_nda_key(b"correct horse battery staple", b"other-salt");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn sealed_document_survives_key_derivation_round_trip() {
        let key = derive_nda_key(b"passphrase", b"salt");
        let doc = sample_doc();
        let sealed = doc.seal(&key, &[7u8; 12]);
        let opened = NdaDocument::open(&key, &sealed).unwrap();
        assert_eq!(doc, opened);
    }

    #[test]
    fn seal_bytes_round_trips_and_hides_plaintext() {
        let key = derive_nda_key(b"workspace secret", b"sitemap");
        let nonce = [0x5au8; 12];
        let payload = b"root/\n  src/main.rs (file)\n  Cargo.toml (file)\n";
        let sealed = seal_bytes(&key, &nonce, payload);
        assert_eq!(&sealed[0..4], b"NDA1");
        // The plaintext must not survive anywhere in the ciphertext.
        assert!(!sealed.windows(8).any(|w| w == b"main.rs "));
        let opened = open_bytes(&key, &sealed).unwrap();
        assert_eq!(opened, payload);
    }

    #[test]
    fn seal_bytes_handles_empty_payload() {
        let key = [0x33u8; 32];
        let sealed = seal_bytes(&key, &[1u8; 12], b"");
        assert_eq!(open_bytes(&key, &sealed).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn open_bytes_rejects_wrong_key() {
        let sealed = seal_bytes(&[0x11u8; 32], &[0x22u8; 12], b"secret log line");
        assert!(open_bytes(&[0x99u8; 32], &sealed).is_err());
    }

    #[test]
    fn open_bytes_detects_ciphertext_tampering() {
        let key = [0x11u8; 32];
        let mut sealed = seal_bytes(&key, &[0x22u8; 12], b"secret log line");
        let last = sealed.len() - 4;
        sealed[last] ^= 0x01;
        assert!(open_bytes(&key, &sealed).is_err());
    }

    #[test]
    fn open_bytes_rejects_document_envelope_and_vice_versa() {
        // A raw-bytes envelope must not be mistaken for a document envelope.
        let key = [0x44u8; 32];
        let raw = seal_bytes(&key, &[0x22u8; 12], b"payload");
        assert!(NdaDocument::open(&key, &raw).is_err());
        // ...and a document envelope must not open as raw bytes.
        let doc_sealed = sample_doc().seal(&key, &[0x22u8; 12]);
        assert!(open_bytes(&key, &doc_sealed).is_err());
    }
}
