//! Portable NDA1 document format — the browser-viewable, self-contained
//! neural-document schema.
//!
//! This is the exact binary layout the reference V.E.L.O.C.I.T.Y. NDA PWA
//! (`editor/app.js`) reads, so a document written here renders in any browser
//! viewer with zero dependencies. It is **distinct** from the encrypted
//! envelope in [`crate::nda`] (which shares the `NDA1` magic but sets the
//! `ENCRYPTED`/`RAW` flag bits and carries an opaque body); a portable
//! document has `Flags == 0`.
//!
//! Layout (little-endian):
//! * Header (48 bytes): magic u32 `"NDA1"` · flags u32 · MerkleRoot[32] ·
//!   tripleCount u32 · commandCount u16 · stringPoolOffset u16.
//! * Triples: `tripleCount` × 12 bytes (`sOffset`,`pOffset`,`oOffset` — each a
//!   byte offset into the string pool).
//! * Commands: `commandCount` × 18 bytes (`type` u8, `color` u32 RGBA,
//!   `x`/`y`/`w`/`h` u16, `contentOffset` u32, one padding byte).
//! * String pool (starts at `stringPoolOffset`): `[len u16][utf8 bytes]`*;
//!   offset 0 is always the empty string.
//!
//! History/provenance rides *inside* the triples block as ordinary semantic
//! facts, so a file's origin and every author who has touched it travel with
//! the document and are visible in any reference viewer.

use crate::net::tls13::sha256;

/// Magic bytes opening any NDA container: ASCII `"NDA1"`.
pub const NDA_MAGIC: u32 = 0x3141_444E; // "NDA1" little-endian

/// Fixed portable-document header size in bytes.
pub const PORTABLE_HEADER_LEN: usize = 48;

// --- Display command kinds -------------------------------------------------

/// Immediate-mode canvas draw op kind, matching the reference schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    DrawText = 1,
    DrawVector = 2,
    DrawRect = 3,
    DrawImage = 4,
}

impl CommandKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(CommandKind::DrawText),
            2 => Some(CommandKind::DrawVector),
            3 => Some(CommandKind::DrawRect),
            4 => Some(CommandKind::DrawImage),
            _ => None,
        }
    }
}

/// A single immediate-mode display command. `content` is interned into the
/// string pool at encode time (text, a vector point list, or an image
/// data-url, depending on `kind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayCommand {
    pub kind: u8,
    /// Packed RGBA (`0xRRGGBBAA`).
    pub color: u32,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub content: String,
}

impl DisplayCommand {
    pub fn text(content: impl Into<String>, x: u16, y: u16, color: u32) -> Self {
        Self { kind: CommandKind::DrawText as u8, color, x, y, w: 0, h: 0, content: content.into() }
    }

    pub fn rect(x: u16, y: u16, w: u16, h: u16, color: u32) -> Self {
        Self { kind: CommandKind::DrawRect as u8, color, x, y, w, h, content: String::new() }
    }

    pub fn image(data_url: impl Into<String>, x: u16, y: u16, w: u16, h: u16) -> Self {
        Self {
            kind: CommandKind::DrawImage as u8,
            color: 0xFFFF_FFFF,
            x,
            y,
            w,
            h,
            content: data_url.into(),
        }
    }
}

// --- Provenance predicate vocabulary --------------------------------------

/// Predicate: the document's human title.
pub const NDA_TITLE: &str = "nda:title";
/// Predicate: the document creation timestamp (top-level meta).
pub const NDA_CREATED: &str = "nda:created";
/// Predicate: origin marker (subject `nda:doc`, object = creating workspace).
pub const NDA_ORIGIN: &str = "nda:origin";
/// Predicate: whether the document should be sealed at rest.
pub const NDA_SEAL: &str = "nda:seal";
/// Predicate on a `rev:{n}` subject — parent revision content hash (or `genesis`).
pub const NDA_REV_PARENT: &str = "rev:parent";
/// Predicate on a `rev:{n}` subject — this revision's content hash (hex).
pub const NDA_REV_CONTENT_HASH: &str = "rev:content_hash";
/// Predicate on a `rev:{n}` subject — resolved author display name.
pub const NDA_REV_AUTHOR_NAME: &str = "rev:author_name";
/// Predicate on a `rev:{n}` subject — resolved author email.
pub const NDA_REV_AUTHOR_EMAIL: &str = "rev:author_email";
/// Predicate on a `rev:{n}` subject — which tier resolved the identity.
pub const NDA_REV_AUTHOR_SOURCE: &str = "rev:author_source";
/// Predicate on a `rev:{n}` subject — RFC3339 commit timestamp.
pub const NDA_REV_TIMESTAMP: &str = "rev:timestamp";
/// Predicate on a `rev:{n}` subject — optional commit message.
pub const NDA_REV_MESSAGE: &str = "rev:message";

/// Sentinel parent for the first (origin) revision.
pub const GENESIS: &str = "genesis";

/// True if a predicate belongs to the provenance/meta layer (excluded from the
/// content hash so the revision chain stays stable as history accumulates).
pub fn is_provenance_predicate(predicate: &str) -> bool {
    matches!(
        predicate,
        NDA_ORIGIN
            | NDA_CREATED
            | NDA_REV_PARENT
            | NDA_REV_CONTENT_HASH
            | NDA_REV_AUTHOR_NAME
            | NDA_REV_AUTHOR_EMAIL
            | NDA_REV_AUTHOR_SOURCE
            | NDA_REV_TIMESTAMP
            | NDA_REV_MESSAGE
    )
}

/// A single provenance record recovered from the document's triples.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Revision {
    pub id: String,
    pub parent: String,
    pub content_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub author_source: String,
    pub timestamp: String,
    pub message: String,
}

// --- The document ----------------------------------------------------------

/// A portable NDA document: semantic triples plus immediate-mode display
/// commands. Strings are stored inline and interned only at encode time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NdaPortableDoc {
    pub triples: Vec<(String, String, String)>,
    pub commands: Vec<DisplayCommand>,
}

impl NdaPortableDoc {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a semantic content triple.
    pub fn push_triple(&mut self, subject: impl Into<String>, predicate: impl Into<String>, object: impl Into<String>) {
        self.triples.push((subject.into(), predicate.into(), object.into()));
    }

    /// Append a display command.
    pub fn push_command(&mut self, cmd: DisplayCommand) {
        self.commands.push(cmd);
    }

    /// Set (or replace) the document title meta triple.
    pub fn set_title(&mut self, title: &str) {
        self.triples.retain(|(_, p, _)| p != NDA_TITLE);
        self.triples.insert(0, ("nda:doc".to_string(), NDA_TITLE.to_string(), title.to_string()));
    }

    /// Return the document title if present.
    pub fn title(&self) -> Option<&str> {
        self.triples.iter().find(|(_, p, _)| p == NDA_TITLE).map(|(_, _, o)| o.as_str())
    }

    /// SHA-256 over the *content* layer only (triples with non-provenance
    /// predicates + all display commands). Stable across provenance churn, so
    /// it forms the revision chain's link identity.
    pub fn content_hash(&self) -> [u8; 32] {
        let mut buf = Vec::new();
        for (s, p, o) in &self.triples {
            if is_provenance_predicate(p) {
                continue;
            }
            push_len_prefixed(&mut buf, s.as_bytes());
            push_len_prefixed(&mut buf, p.as_bytes());
            push_len_prefixed(&mut buf, o.as_bytes());
        }
        for c in &self.commands {
            buf.push(c.kind);
            buf.extend_from_slice(&c.color.to_le_bytes());
            buf.extend_from_slice(&c.x.to_le_bytes());
            buf.extend_from_slice(&c.y.to_le_bytes());
            buf.extend_from_slice(&c.w.to_le_bytes());
            buf.extend_from_slice(&c.h.to_le_bytes());
            push_len_prefixed(&mut buf, c.content.as_bytes());
        }
        sha256(&buf)
    }

    /// Highest existing `rev:{n}` index, or `None` if there is no history yet.
    fn latest_rev_index(&self) -> Option<u64> {
        self.triples
            .iter()
            .filter_map(|(s, _, _)| s.strip_prefix("rev:").and_then(|n| n.parse::<u64>().ok()))
            .max()
    }

    /// Append a new revision to the in-file history, linking it to the prior
    /// revision by content hash. The genesis revision also records an origin
    /// marker. Author fields are resolved by the caller (see
    /// `velocity-mcp`'s author resolution); this keeps the core crate pure.
    pub fn commit_revision(
        &mut self,
        author_name: &str,
        author_email: &str,
        author_source: &str,
        timestamp: &str,
        message: &str,
        origin_workspace: &str,
    ) {
        let parent = self
            .revisions()
            .last()
            .map(|r| r.content_hash.clone())
            .unwrap_or_else(|| GENESIS.to_string());
        let content_hex = hex(&self.content_hash());
        let next = self.latest_rev_index().map(|n| n + 1).unwrap_or(0);
        let subj = format!("rev:{next}");

        if next == 0 {
            self.triples.push(("nda:doc".to_string(), NDA_ORIGIN.to_string(), origin_workspace.to_string()));
            if !self.triples.iter().any(|(_, p, _)| p == NDA_CREATED) {
                self.triples.push(("nda:doc".to_string(), NDA_CREATED.to_string(), timestamp.to_string()));
            }
        }

        self.triples.push((subj.clone(), NDA_REV_PARENT.to_string(), parent));
        self.triples.push((subj.clone(), NDA_REV_CONTENT_HASH.to_string(), content_hex));
        self.triples.push((subj.clone(), NDA_REV_AUTHOR_NAME.to_string(), author_name.to_string()));
        self.triples.push((subj.clone(), NDA_REV_AUTHOR_EMAIL.to_string(), author_email.to_string()));
        self.triples.push((subj.clone(), NDA_REV_AUTHOR_SOURCE.to_string(), author_source.to_string()));
        self.triples.push((subj.clone(), NDA_REV_TIMESTAMP.to_string(), timestamp.to_string()));
        self.triples.push((subj, NDA_REV_MESSAGE.to_string(), message.to_string()));
    }

    /// Recover the ordered revision history from the document's triples.
    pub fn revisions(&self) -> Vec<Revision> {
        let mut indices: Vec<u64> = self
            .triples
            .iter()
            .filter_map(|(s, _, _)| s.strip_prefix("rev:").and_then(|n| n.parse::<u64>().ok()))
            .collect();
        indices.sort_unstable();
        indices.dedup();

        indices
            .into_iter()
            .map(|n| {
                let subj = format!("rev:{n}");
                let get = |pred: &str| -> String {
                    self.triples
                        .iter()
                        .find(|(s, p, _)| s == &subj && p == pred)
                        .map(|(_, _, o)| o.clone())
                        .unwrap_or_default()
                };
                Revision {
                    id: subj.clone(),
                    parent: get(NDA_REV_PARENT),
                    content_hash: get(NDA_REV_CONTENT_HASH),
                    author_name: get(NDA_REV_AUTHOR_NAME),
                    author_email: get(NDA_REV_AUTHOR_EMAIL),
                    author_source: get(NDA_REV_AUTHOR_SOURCE),
                    timestamp: get(NDA_REV_TIMESTAMP),
                    message: get(NDA_REV_MESSAGE),
                }
            })
            .collect()
    }

    /// Verify the revision chain: each revision's `parent` must equal the prior
    /// revision's `content_hash` (genesis links to `GENESIS`). Returns `Ok(())`
    /// or a description of the first broken link.
    pub fn verify_history(&self) -> Result<(), String> {
        let revs = self.revisions();
        let mut prev: Option<String> = None;
        for r in &revs {
            let expected = prev.clone().unwrap_or_else(|| GENESIS.to_string());
            if r.parent != expected {
                return Err(format!(
                    "revision {} parent {} does not link to {}",
                    r.id, r.parent, expected
                ));
            }
            prev = Some(r.content_hash.clone());
        }
        Ok(())
    }

    /// SHA-256 pairwise Merkle root over *all* triples (`"{s}|{p}|{o}"` leaves),
    /// matching the reference viewer. Empty doc → all-zero root.
    pub fn merkle_root(&self) -> [u8; 32] {
        if self.triples.is_empty() {
            return [0u8; 32];
        }
        let mut level: Vec<[u8; 32]> = self
            .triples
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

    /// Serialize to the portable 48-byte-header NDA1 layout (Flags = 0).
    pub fn to_portable_bytes(&self) -> Vec<u8> {
        // Build the string pool with an offset-deduplicating interner. The
        // empty string is interned first so its offset is 0.
        let mut pool: Vec<u8> = Vec::new();
        let mut offsets: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut add_string = |s: &str| -> u32 {
            if let Some(&off) = offsets.get(s) {
                return off;
            }
            let off = pool.len() as u32;
            let bytes = s.as_bytes();
            pool.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            pool.extend_from_slice(bytes);
            offsets.insert(s.to_string(), off);
            off
        };
        add_string(""); // ensure offset 0 == empty string

        let triple_offsets: Vec<(u32, u32, u32)> = self
            .triples
            .iter()
            .map(|(s, p, o)| (add_string(s), add_string(p), add_string(o)))
            .collect();
        let command_offsets: Vec<u32> =
            self.commands.iter().map(|c| add_string(&c.content)).collect();

        let triples_block = triple_offsets.len() * 12;
        let commands_block = self.commands.len() * 18;
        let string_pool_offset = PORTABLE_HEADER_LEN + triples_block + commands_block;

        let mut buf = vec![0u8; string_pool_offset];
        // Header.
        buf[0..4].copy_from_slice(&NDA_MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&0u32.to_le_bytes()); // Flags = portable
        buf[8..40].copy_from_slice(&self.merkle_root());
        buf[40..44].copy_from_slice(&(triple_offsets.len() as u32).to_le_bytes());
        buf[44..46].copy_from_slice(&(self.commands.len() as u16).to_le_bytes());
        buf[46..48].copy_from_slice(&(string_pool_offset as u16).to_le_bytes());

        // Triples block.
        let mut off = PORTABLE_HEADER_LEN;
        for (s, p, o) in &triple_offsets {
            buf[off..off + 4].copy_from_slice(&s.to_le_bytes());
            buf[off + 4..off + 8].copy_from_slice(&p.to_le_bytes());
            buf[off + 8..off + 12].copy_from_slice(&o.to_le_bytes());
            off += 12;
        }
        // Commands block.
        for (c, content_off) in self.commands.iter().zip(command_offsets) {
            buf[off] = c.kind;
            buf[off + 1..off + 5].copy_from_slice(&c.color.to_le_bytes());
            buf[off + 5..off + 7].copy_from_slice(&c.x.to_le_bytes());
            buf[off + 7..off + 9].copy_from_slice(&c.y.to_le_bytes());
            buf[off + 9..off + 11].copy_from_slice(&c.w.to_le_bytes());
            buf[off + 11..off + 13].copy_from_slice(&c.h.to_le_bytes());
            buf[off + 13..off + 17].copy_from_slice(&content_off.to_le_bytes());
            buf[off + 17] = 0; // padding
            off += 18;
        }
        // String pool.
        buf.extend_from_slice(&pool);
        buf
    }

    /// Parse a portable NDA1 buffer, validating the magic, the portable flag,
    /// bounds, and the Merkle root (tamper check).
    pub fn from_portable_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < PORTABLE_HEADER_LEN {
            return Err("NDA buffer smaller than header");
        }
        if u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != NDA_MAGIC {
            return Err("bad NDA magic");
        }
        if u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 0 {
            return Err("not a portable NDA document (flags set — sealed/raw envelope)");
        }
        let mut header_root = [0u8; 32];
        header_root.copy_from_slice(&bytes[8..40]);
        let triple_count = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        let command_count = u16::from_le_bytes(bytes[44..46].try_into().unwrap()) as usize;
        let pool_offset = u16::from_le_bytes(bytes[46..48].try_into().unwrap()) as usize;
        if pool_offset > bytes.len() {
            return Err("string pool offset out of bounds");
        }
        let pool = &bytes[pool_offset..];

        let read_string = |rel: u32| -> Result<String, &'static str> {
            let start = rel as usize;
            if start + 2 > pool.len() {
                return Err("string offset out of bounds");
            }
            let len = u16::from_le_bytes(pool[start..start + 2].try_into().unwrap()) as usize;
            let s = start + 2;
            if s + len > pool.len() {
                return Err("string length out of bounds");
            }
            String::from_utf8(pool[s..s + len].to_vec()).map_err(|_| "invalid utf8 in string pool")
        };

        let mut triples = Vec::with_capacity(triple_count);
        let mut off = PORTABLE_HEADER_LEN;
        for _ in 0..triple_count {
            if off + 12 > bytes.len() {
                return Err("triples block truncated");
            }
            let s = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
            let p = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
            let o = u32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap());
            triples.push((read_string(s)?, read_string(p)?, read_string(o)?));
            off += 12;
        }

        let mut commands = Vec::with_capacity(command_count);
        for _ in 0..command_count {
            if off + 18 > bytes.len() {
                return Err("commands block truncated");
            }
            let kind = bytes[off];
            let color = u32::from_le_bytes(bytes[off + 1..off + 5].try_into().unwrap());
            let x = u16::from_le_bytes(bytes[off + 5..off + 7].try_into().unwrap());
            let y = u16::from_le_bytes(bytes[off + 7..off + 9].try_into().unwrap());
            let w = u16::from_le_bytes(bytes[off + 9..off + 11].try_into().unwrap());
            let h = u16::from_le_bytes(bytes[off + 11..off + 13].try_into().unwrap());
            let content_off = u32::from_le_bytes(bytes[off + 13..off + 17].try_into().unwrap());
            commands.push(DisplayCommand {
                kind,
                color,
                x,
                y,
                w,
                h,
                content: read_string(content_off)?,
            });
            off += 18;
        }

        let doc = Self { triples, commands };
        if doc.merkle_root() != header_root {
            return Err("NDA Merkle root mismatch (tampered)");
        }
        Ok(doc)
    }
}

/// Lowercase hex encoding of a byte slice.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    s
}

fn push_len_prefixed(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NdaPortableDoc {
        let mut doc = NdaPortableDoc::new();
        doc.set_title("Hello");
        doc.push_triple("subject", "predicate", "object");
        doc.push_command(DisplayCommand::text("Hi", 10, 20, 0x1122_33FF));
        doc.push_command(DisplayCommand::rect(0, 0, 100, 50, 0x00FF_00FF));
        doc
    }

    #[test]
    fn roundtrips_byte_identical() {
        let doc = sample();
        let bytes = doc.to_portable_bytes();
        let parsed = NdaPortableDoc::from_portable_bytes(&bytes).unwrap();
        assert_eq!(doc, parsed);
        // Re-emitting the parsed doc yields identical bytes.
        assert_eq!(bytes, parsed.to_portable_bytes());
    }

    #[test]
    fn empty_doc_zero_root() {
        let doc = NdaPortableDoc::new();
        assert_eq!(doc.merkle_root(), [0u8; 32]);
        let bytes = doc.to_portable_bytes();
        let parsed = NdaPortableDoc::from_portable_bytes(&bytes).unwrap();
        assert_eq!(doc, parsed);
    }

    #[test]
    fn golden_header_offsets() {
        let doc = sample();
        let bytes = doc.to_portable_bytes();
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), NDA_MAGIC);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 0);
        // 1 title triple + 1 content triple = 2 triples.
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(bytes[44..46].try_into().unwrap()), 2);
        let pool_off = u16::from_le_bytes(bytes[46..48].try_into().unwrap()) as usize;
        assert_eq!(pool_off, PORTABLE_HEADER_LEN + 2 * 12 + 2 * 18);
        // Empty string interned first at pool offset 0.
        assert_eq!(u16::from_le_bytes(bytes[pool_off..pool_off + 2].try_into().unwrap()), 0);
    }

    #[test]
    fn string_pool_dedups() {
        let mut doc = NdaPortableDoc::new();
        doc.push_triple("same", "same", "same");
        let bytes = doc.to_portable_bytes();
        let pool_off = u16::from_le_bytes(bytes[46..48].try_into().unwrap()) as usize;
        // Pool holds only "" and "same" — one dedup'd entry beyond empty.
        let pool = &bytes[pool_off..];
        // "" => 2 bytes; "same" => 2 + 4 bytes; total 8.
        assert_eq!(pool.len(), 8);
    }

    #[test]
    fn tamper_detected() {
        let doc = sample();
        let mut bytes = doc.to_portable_bytes();
        // Flip a byte inside the string pool.
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(NdaPortableDoc::from_portable_bytes(&bytes).is_err());
    }

    #[test]
    fn out_of_bounds_offset_rejected() {
        let doc = sample();
        let mut bytes = doc.to_portable_bytes();
        // Corrupt the first triple's subject offset to point past the pool.
        bytes[PORTABLE_HEADER_LEN..PORTABLE_HEADER_LEN + 4]
            .copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert!(NdaPortableDoc::from_portable_bytes(&bytes).is_err());
    }

    #[test]
    fn history_chain_links_and_verifies() {
        let mut doc = NdaPortableDoc::new();
        doc.push_triple("a", "b", "c");
        doc.commit_revision("Alice", "a@x", "configured", "2026-01-01T00:00:00Z", "init", "ws");
        // A content edit + second revision.
        doc.push_triple("d", "e", "f");
        doc.commit_revision("Bob", "b@x", "git", "2026-01-02T00:00:00Z", "edit", "ws");

        let revs = doc.revisions();
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].parent, GENESIS);
        assert_eq!(revs[0].author_name, "Alice");
        assert_eq!(revs[1].parent, revs[0].content_hash);
        assert_eq!(revs[1].author_source, "git");
        assert!(doc.verify_history().is_ok());
    }

    #[test]
    fn content_hash_stable_across_provenance() {
        let mut doc = NdaPortableDoc::new();
        doc.push_triple("a", "b", "c");
        let before = doc.content_hash();
        doc.commit_revision("Alice", "a@x", "os", "2026-01-01T00:00:00Z", "init", "ws");
        // Committing appended only provenance triples; content hash unchanged.
        assert_eq!(before, doc.content_hash());
    }

    #[test]
    fn broken_history_detected() {
        let mut doc = NdaPortableDoc::new();
        doc.push_triple("a", "b", "c");
        doc.commit_revision("Alice", "a@x", "os", "2026-01-01T00:00:00Z", "init", "ws");
        // Corrupt the genesis parent link.
        for t in doc.triples.iter_mut() {
            if t.1 == NDA_REV_PARENT {
                t.2 = "bogus".to_string();
            }
        }
        assert!(doc.verify_history().is_err());
    }
}
