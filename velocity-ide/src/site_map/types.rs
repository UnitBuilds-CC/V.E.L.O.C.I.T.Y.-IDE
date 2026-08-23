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
