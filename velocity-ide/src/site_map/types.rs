use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcTriple {
    pub subject_hash: u64,
    pub predicate_id: u16,
    pub object_hash: u64,
}

/// Metadata record stored in index.json for one site-map entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SiteMapEntry {
    pub kind: EntryKind,
    pub hash: u64,
    /// Path relative to site_map root.
    pub file: String,
    /// SHA-256 of the raw file bytes — second integrity check on top of Merkle.
    pub file_sha: String,
    /// Size in bytes.
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntryKind {
    Kv,       // token K/V pair
    Node,     // NDA program node
    Program,  // complete NDA program (root reference)
    Snapshot, // overwriteable file-scoped live semantic snapshot
}

#[derive(Debug, Serialize)]
pub struct SiteMapStats {
    pub kv: usize,
    pub nodes: usize,
    pub programs: usize,
    pub snapshots: usize,
    pub total_bytes: u64,
    pub root: u64,
    pub weight_root: u64,
    /// Number of KV records currently cached in RAM.
    pub kv_cache_size: usize,
    /// Number of unique strings in the dictionary.
    pub string_dict_size: usize,
    /// Total number of entries in the index (all kinds).
    pub total_entries: usize,
}

impl std::fmt::Display for SiteMapStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SiteMap: {} KV, {} nodes, {} programs, {} snapshots | {:.1} KB on disk | {} strings | cache={} | root={:016x} | weight_root={:016x}",
            self.kv,
            self.nodes,
            self.programs,
            self.snapshots,
            self.total_bytes as f64 / 1024.0,
            self.string_dict_size,
            self.kv_cache_size,
            self.root,
            self.weight_root,
        )
    }
}

// ─── Diagnostics ───────────────────────────────────────────────────────────────

/// Serializable diagnostic for a VcTriple.
#[derive(Debug, Clone, Serialize)]
pub struct VcTripleInfo {
    pub subject_hash: String,
    pub predicate_id: u16,
    pub object_hash: String,
    pub validation_issues: Vec<String>,
}

impl VcTriple {
    /// Return a diagnostic snapshot of this triple.
    pub fn info(&self) -> VcTripleInfo {
        VcTripleInfo {
            subject_hash: format!("{:016x}", self.subject_hash),
            predicate_id: self.predicate_id,
            object_hash: format!("{:016x}", self.object_hash),
            validation_issues: self.validate(),
        }
    }

    /// Validate the triple.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.subject_hash == 0 {
            issues.push("subject_hash is zero (null reference)".to_string());
        }
        if self.object_hash == 0 {
            issues.push("object_hash is zero (null reference)".to_string());
        }
        if self.subject_hash == self.object_hash {
            issues.push("subject and object have same hash (self-reference)".to_string());
        }
        issues
    }
}

/// Validate a batch of VcTriples for consistency.
pub fn validate_triples(triples: &[VcTriple]) -> Vec<String> {
    let mut issues = Vec::new();
    for (i, t) in triples.iter().enumerate() {
        for issue in t.validate() {
            issues.push(format!("triple[{}]: {}", i, issue));
        }
    }
    // Check for duplicate triples
    let mut seen = std::collections::HashSet::new();
    for (i, t) in triples.iter().enumerate() {
        let key = (t.subject_hash, t.predicate_id, t.object_hash);
        if !seen.insert(key) {
            issues.push(format!("triple[{}] is a duplicate", i));
        }
    }
    issues
}

/// Diagnostic summary of EntryKind distribution.
#[derive(Debug, Clone, Serialize)]
pub struct EntryKindDistribution {
    pub kv: usize,
    pub node: usize,
    pub program: usize,
    pub snapshot: usize,
    pub total: usize,
}

/// Count entry kinds in a slice of entries.
pub fn entry_kind_distribution(entries: &[SiteMapEntry]) -> EntryKindDistribution {
    let mut dist = EntryKindDistribution {
        kv: 0,
        node: 0,
        program: 0,
        snapshot: 0,
        total: entries.len(),
    };
    for e in entries {
        match e.kind {
            EntryKind::Kv => dist.kv += 1,
            EntryKind::Node => dist.node += 1,
            EntryKind::Program => dist.program += 1,
            EntryKind::Snapshot => dist.snapshot += 1,
        }
    }
    dist
}

/// Validate a SiteMapEntry.
pub fn validate_entry(entry: &SiteMapEntry) -> Vec<String> {
    let mut issues = Vec::new();
    if entry.hash == 0 {
        issues.push("entry hash is zero".to_string());
    }
    if entry.file.is_empty() {
        issues.push("entry file path is empty".to_string());
    }
    if entry.file_sha.is_empty() {
        issues.push("entry file_sha is empty (integrity check unavailable)".to_string());
    }
    if entry.size == 0 {
        issues.push("entry size is zero".to_string());
    }
    issues
}

/// Stricter validation for SiteMapEntry (checks format details).
pub fn validate_entry_full(entry: &SiteMapEntry) -> Vec<String> {
    let mut issues = validate_entry(entry);
    // Check file_sha looks like a hex digest
    if !entry.file_sha.is_empty() && entry.file_sha.len() < 8 {
        issues.push("file_sha too short to be a valid hash".to_string());
    }
    if !entry.file_sha.is_empty() && !entry.file_sha.chars().all(|c| c.is_ascii_hexdigit()) {
        issues.push("file_sha contains non-hex characters".to_string());
    }
    // Check file path doesn't contain parent traversal
    if entry.file.contains("..") {
        issues.push("file path contains parent traversal '..'".to_string());
    }
    issues
}

/// In-memory triple index for fast lookups by subject, predicate, or object.
#[derive(Debug, Clone, Serialize)]
pub struct TripleIndex {
    by_subject: std::collections::HashMap<u64, Vec<usize>>,
    by_object: std::collections::HashMap<u64, Vec<usize>>,
    by_predicate: std::collections::HashMap<u16, Vec<usize>>,
    pub triple_count: usize,
}

impl TripleIndex {
    /// Build an index from a slice of triples.
    pub fn build(triples: &[VcTriple]) -> Self {
        let mut by_subject: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
        let mut by_object: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
        let mut by_predicate: std::collections::HashMap<u16, Vec<usize>> = std::collections::HashMap::new();

        for (i, t) in triples.iter().enumerate() {
            by_subject.entry(t.subject_hash).or_default().push(i);
            by_object.entry(t.object_hash).or_default().push(i);
            by_predicate.entry(t.predicate_id).or_default().push(i);
        }

        TripleIndex {
            by_subject,
            by_object,
            by_predicate,
            triple_count: triples.len(),
        }
    }

    /// Find triple indices by subject hash.
    pub fn by_subject(&self, hash: u64) -> &[usize] {
        self.by_subject.get(&hash).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Find triple indices by object hash.
    pub fn by_object(&self, hash: u64) -> &[usize] {
        self.by_object.get(&hash).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Find triple indices by predicate id.
    pub fn by_predicate(&self, pred: u16) -> &[usize] {
        self.by_predicate.get(&pred).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of unique subjects in the index.
    pub fn unique_subjects(&self) -> usize {
        self.by_subject.len()
    }

    /// Number of unique objects in the index.
    pub fn unique_objects(&self) -> usize {
        self.by_object.len()
    }

    /// Number of unique predicates in the index.
    pub fn unique_predicates(&self) -> usize {
        self.by_predicate.len()
    }

    /// Diagnostic info about the index.
    pub fn info(&self) -> TripleIndexInfo {
        TripleIndexInfo {
            triple_count: self.triple_count,
            unique_subjects: self.unique_subjects(),
            unique_objects: self.unique_objects(),
            unique_predicates: self.unique_predicates(),
        }
    }
}

/// Diagnostic info about a triple index.
#[derive(Debug, Clone, Serialize)]
pub struct TripleIndexInfo {
    pub triple_count: usize,
    pub unique_subjects: usize,
    pub unique_objects: usize,
    pub unique_predicates: usize,
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vc_triple_validate_clean() {
        let t = VcTriple {
            subject_hash: 0xAAAA,
            predicate_id: 1,
            object_hash: 0xBBBB,
        };
        assert!(t.validate().is_empty());
    }

    #[test]
    fn vc_triple_validate_null_subject() {
        let t = VcTriple {
            subject_hash: 0,
            predicate_id: 1,
            object_hash: 0xBBBB,
        };
        assert!(t.validate().iter().any(|i| i.contains("subject_hash")));
    }

    #[test]
    fn vc_triple_validate_self_reference() {
        let t = VcTriple {
            subject_hash: 0xAAAA,
            predicate_id: 1,
            object_hash: 0xAAAA,
        };
        let issues = t.validate();
        assert!(issues.iter().any(|i| i.contains("self-reference")));
    }

    #[test]
    fn vc_triple_info() {
        let t = VcTriple {
            subject_hash: 0xFF,
            predicate_id: 42,
            object_hash: 0xABCD,
        };
        let info = t.info();
        assert_eq!(info.predicate_id, 42);
        assert!(info.subject_hash.contains("ff"));
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn validate_triples_clean() {
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 },
            VcTriple { subject_hash: 2, predicate_id: 1, object_hash: 3 },
        ];
        assert!(validate_triples(&triples).is_empty());
    }

    #[test]
    fn validate_triples_duplicate() {
        let t = VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 };
        let triples = vec![t.clone(), t];
        let issues = validate_triples(&triples);
        assert!(issues.iter().any(|i| i.contains("duplicate")));
    }

    #[test]
    fn entry_kind_distribution_test() {
        let entries = vec![
            SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "a".into(), file_sha: "x".into(), size: 10 },
            SiteMapEntry { kind: EntryKind::Node, hash: 2, file: "b".into(), file_sha: "y".into(), size: 20 },
            SiteMapEntry { kind: EntryKind::Kv, hash: 3, file: "c".into(), file_sha: "z".into(), size: 30 },
        ];
        let dist = entry_kind_distribution(&entries);
        assert_eq!(dist.kv, 2);
        assert_eq!(dist.node, 1);
        assert_eq!(dist.program, 0);
        assert_eq!(dist.snapshot, 0);
        assert_eq!(dist.total, 3);
    }

    #[test]
    fn validate_entry_clean() {
        let e = SiteMapEntry {
            kind: EntryKind::Kv,
            hash: 1,
            file: "test.kv".into(),
            file_sha: "abc123".into(),
            size: 452,
        };
        assert!(validate_entry(&e).is_empty());
    }

    #[test]
    fn validate_entry_zero_hash() {
        let e = SiteMapEntry {
            kind: EntryKind::Kv,
            hash: 0,
            file: "test.kv".into(),
            file_sha: "abc".into(),
            size: 100,
        };
        assert!(validate_entry(&e).iter().any(|i| i.contains("hash is zero")));
    }

    #[test]
    fn validate_entry_empty_file() {
        let e = SiteMapEntry {
            kind: EntryKind::Node,
            hash: 1,
            file: "".into(),
            file_sha: "abc".into(),
            size: 100,
        };
        assert!(validate_entry(&e).iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn sitemap_stats_display() {
        let stats = SiteMapStats {
            kv: 10,
            nodes: 5,
            programs: 2,
            snapshots: 1,
            total_bytes: 2048,
            root: 0xDEAD,
            weight_root: 0xBEEF,
            kv_cache_size: 8,
            string_dict_size: 100,
            total_entries: 18,
        };
        let display = format!("{}", stats);
        assert!(display.contains("10 KV"));
        assert!(display.contains("5 nodes"));
        assert!(display.contains("2.0 KB"));
    }

    #[test]
    fn entry_kind_distribution_serializable() {
        let dist = entry_kind_distribution(&[]);
        let json = serde_json::to_string(&dist).unwrap();
        assert!(json.contains("total"));
    }

    #[test]
    fn vc_triple_info_serializable() {
        let t = VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 };
        let info = t.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("predicate_id"));
    }

    // ─── Block 80: new tests ──────────────────────────────────────────────

    #[test]
    fn validate_entry_full_clean() {
        let e = SiteMapEntry {
            kind: EntryKind::Kv,
            hash: 1,
            file: "test.kv".into(),
            file_sha: "abcdef1234567890".into(),
            size: 100,
        };
        assert!(validate_entry_full(&e).is_empty());
    }

    #[test]
    fn validate_entry_full_bad_sha_short() {
        let e = SiteMapEntry {
            kind: EntryKind::Kv,
            hash: 1,
            file: "test.kv".into(),
            file_sha: "abc".into(),
            size: 100,
        };
        let issues = validate_entry_full(&e);
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn validate_entry_full_bad_sha_non_hex() {
        let e = SiteMapEntry {
            kind: EntryKind::Kv,
            hash: 1,
            file: "test.kv".into(),
            file_sha: "xyznotahexvalue!".into(),
            size: 100,
        };
        let issues = validate_entry_full(&e);
        assert!(issues.iter().any(|i| i.contains("non-hex")));
    }

    #[test]
    fn validate_entry_full_parent_traversal() {
        let e = SiteMapEntry {
            kind: EntryKind::Node,
            hash: 1,
            file: "../../../etc/passwd".into(),
            file_sha: "abcdef1234567890".into(),
            size: 100,
        };
        let issues = validate_entry_full(&e);
        assert!(issues.iter().any(|i| i.contains("parent traversal")));
    }

    #[test]
    fn triple_index_build_empty() {
        let idx = TripleIndex::build(&[]);
        assert_eq!(idx.triple_count, 0);
        assert_eq!(idx.unique_subjects(), 0);
        assert_eq!(idx.unique_objects(), 0);
        assert_eq!(idx.unique_predicates(), 0);
    }

    #[test]
    fn triple_index_build_basic() {
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 },
            VcTriple { subject_hash: 1, predicate_id: 1, object_hash: 3 },
            VcTriple { subject_hash: 2, predicate_id: 0, object_hash: 3 },
        ];
        let idx = TripleIndex::build(&triples);
        assert_eq!(idx.triple_count, 3);
        assert_eq!(idx.unique_subjects(), 2);
        assert_eq!(idx.unique_objects(), 2);
        assert_eq!(idx.unique_predicates(), 2);
    }

    #[test]
    fn triple_index_by_subject_lookup() {
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 },
            VcTriple { subject_hash: 1, predicate_id: 1, object_hash: 3 },
            VcTriple { subject_hash: 2, predicate_id: 0, object_hash: 3 },
        ];
        let idx = TripleIndex::build(&triples);
        assert_eq!(idx.by_subject(1).len(), 2);
        assert_eq!(idx.by_subject(2).len(), 1);
        assert_eq!(idx.by_subject(999).len(), 0);
    }

    #[test]
    fn triple_index_by_object_lookup() {
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 3 },
            VcTriple { subject_hash: 2, predicate_id: 0, object_hash: 3 },
            VcTriple { subject_hash: 3, predicate_id: 0, object_hash: 4 },
        ];
        let idx = TripleIndex::build(&triples);
        assert_eq!(idx.by_object(3).len(), 2);
        assert_eq!(idx.by_object(4).len(), 1);
    }

    #[test]
    fn triple_index_by_predicate_lookup() {
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 },
            VcTriple { subject_hash: 2, predicate_id: 0, object_hash: 3 },
            VcTriple { subject_hash: 3, predicate_id: 1, object_hash: 4 },
        ];
        let idx = TripleIndex::build(&triples);
        assert_eq!(idx.by_predicate(0).len(), 2);
        assert_eq!(idx.by_predicate(1).len(), 1);
        assert_eq!(idx.by_predicate(99).len(), 0);
    }

    #[test]
    fn triple_index_info_serializes() {
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 0, object_hash: 2 },
        ];
        let idx = TripleIndex::build(&triples);
        let info = idx.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"triple_count\":1"));
        assert!(json.contains("\"unique_subjects\":1"));
    }

    #[test]
    fn triple_index_info_basic() {
        let idx = TripleIndex::build(&[]);
        let info = idx.info();
        assert_eq!(info.triple_count, 0);
        assert_eq!(info.unique_subjects, 0);
    }
}
