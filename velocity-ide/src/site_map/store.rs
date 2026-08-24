use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::nda_int::NdaVec;
use crate::safety::SafeMutex;

use super::kv::KvRecord;
use super::serialization::{deserialise_node, serialise_node};
use super::types::{EntryKind, SiteMapEntry, SiteMapStats, VcTriple};
use super::verifier::NdaNode;
use serde::Serialize;

/// Maximum number of token KV records kept in RAM.
const MAX_KV_CACHE_SIZE: usize = 4096;

/// Persistent, content-addressed KV store.
pub struct SiteMap {
    /// Root directory on disk.
    base: PathBuf,
    /// Hot in-RAM index: hash → entry metadata.
    pub index: HashMap<u64, SiteMapEntry>,
    /// Merkle root of the index (hash of all entry hashes, sorted).
    root: u64,
    /// Blake3-style root of the model weight files — embedded in token hashes
    /// so that weight updates automatically invalidate stale KV entries.
    weight_root: u64,
    /// In-RAM KV cache with FIFO/LRU eviction up to MAX_KV_CACHE_SIZE.
    kv_cache: HashMap<u64, (NdaVec, NdaVec)>,
    kv_cache_keys: VecDeque<u64>,
    /// In-RAM cache for file snapshot triples.
    snapshot_triples_cache: Mutex<HashMap<u64, Vec<VcTriple>>>,
    /// In-RAM cache for AST node triples.
    node_triples_cache: Mutex<HashMap<u64, Vec<VcTriple>>>,
    /// In-RAM dictionary for registered strings.
    string_dict: Mutex<HashMap<u64, String>>,
    /// Predicate index: predicate_id -> list of triples with that predicate.
    /// Built lazily on first query and invalidated when snapshots change.
    predicate_index: Mutex<Option<HashMap<u16, Vec<VcTriple>>>>,
}

impl SiteMap {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Open (or create) a site map at `base_dir`.
    pub fn open(base_dir: &Path, weight_root: u64) -> Result<Self> {
        fs::create_dir_all(base_dir.join("kv"))?;
        fs::create_dir_all(base_dir.join("nodes"))?;
        fs::create_dir_all(base_dir.join("programs"))?;

        let index_path = base_dir.join("index.json");
        let (index, root) = if index_path.exists() {
            let raw = fs::read_to_string(&index_path).context("reading site_map/index.json")?;
            let entries: Vec<SiteMapEntry> =
                serde_json::from_str(&raw).context("parsing site_map/index.json")?;
            let root = Self::compute_index_root(&entries);
            let map = entries.into_iter().map(|e| (e.hash, e)).collect();
            (map, root)
        } else {
            (HashMap::new(), 0u64)
        };
        let persisted_weight_root = Self::read_weight_root(base_dir).unwrap_or(weight_root);

        Ok(Self {
            base: base_dir.to_path_buf(),
            index,
            root,
            weight_root: persisted_weight_root,
            kv_cache: HashMap::new(),
            kv_cache_keys: VecDeque::new(),
            snapshot_triples_cache: Mutex::new(HashMap::new()),
            node_triples_cache: Mutex::new(HashMap::new()),
            string_dict: Mutex::new(HashMap::new()),
            predicate_index: Mutex::new(None),
        })
    }

    /// Compute a stable hash of all `.nda` weight files in `weight_dir`.
    pub fn hash_weight_dir(weight_dir: &Path) -> u64 {
        let mut h = Sha256::new();
        let mut entries: Vec<_> = fs::read_dir(weight_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().map(|x| x == "nda").unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in &entries {
            if let Ok(meta) = entry.metadata() {
                h.update(entry.file_name().to_string_lossy().as_bytes());
                h.update(meta.len().to_le_bytes());
                if let Ok(modified) = meta.modified() {
                    if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
                        h.update(d.as_nanos().to_le_bytes());
                    }
                }
            }
        }
        let digest = h.finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap())
    }

    pub fn read_persisted_weight_root(base_dir: &Path) -> Option<u64> {
        Self::read_weight_root(base_dir)
    }

    fn read_weight_root(base_dir: &Path) -> Option<u64> {
        Self::read_weight_root_nda(base_dir).or_else(|| Self::read_weight_root_json(base_dir))
    }

    fn read_weight_root_nda(base_dir: &Path) -> Option<u64> {
        let metadata_path = base_dir.join("metadata.nda");
        let raw = fs::read_to_string(metadata_path).ok()?;
        let mut lines = raw.lines();
        let header = lines
            .find(|line| !line.trim().is_empty())
            .map(str::trim)
            .unwrap_or("");

        if header == "metadata version 2" {
            for line in lines {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with("field_count ") {
                    continue;
                }
                let parts = line.split('\t').collect::<Vec<_>>();
                if parts.len() == 3 && parts[0] == "field" && parts[1] == "weight_root" {
                    return u64::from_str_radix(parts[2].trim_start_matches("0x"), 16).ok();
                }
            }
            return None;
        }

        for line in raw.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("weight_root ") {
                return u64::from_str_radix(value.trim_start_matches("0x"), 16).ok();
            }
        }
        None
    }

    fn read_weight_root_json(base_dir: &Path) -> Option<u64> {
        let metadata_path = base_dir.join("metadata.json");
        let raw = fs::read_to_string(metadata_path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let hex = value.get("weight_root")?.as_str()?;
        u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok()
    }

    // ── KV access (hot path) ──────────────────────────────────────────────────

    pub fn token_hash(&self, token_id: u32, layer_idx: u32) -> u64 {
        let mut h = Sha256::new();
        h.update(b"kv");
        h.update(token_id.to_le_bytes());
        h.update(layer_idx.to_le_bytes());
        h.update(self.weight_root.to_le_bytes());
        let d = h.finalize();
        u64::from_le_bytes(d[..8].try_into().unwrap())
    }

    fn enforce_kv_cache_limit(&mut self) {
        while self.kv_cache.len() >= MAX_KV_CACHE_SIZE {
            if let Some(oldest_key) = self.kv_cache_keys.pop_front() {
                self.kv_cache.remove(&oldest_key);
            } else {
                break;
            }
        }
    }

    pub fn get_kv(&mut self, token_id: u32, layer_idx: u32) -> Option<(&NdaVec, &NdaVec)> {
        let key = self.token_hash(token_id, layer_idx);

        if !self.kv_cache.contains_key(&key) {
            let entry = self.index.get(&key)?;
            let path = self.base.join(&entry.file);
            let data = fs::read(&path).ok()?;
            if !Self::verify_file_sha(&data, &entry.file_sha) {
                eprintln!("[site_map] integrity check failed for {}", path.display());
                return None;
            }
            let rec = KvRecord::deserialise(&data).ok()?;
            self.enforce_kv_cache_limit();
            self.kv_cache.insert(key, (rec.k, rec.v));
            self.kv_cache_keys.push_back(key);
        }

        let (k, v) = self.kv_cache.get(&key)?;
        Some((k, v))
    }

    pub fn put_kv(&mut self, token_id: u32, layer_idx: u32, k: NdaVec, v: NdaVec) -> Result<u64> {
        let key = self.token_hash(token_id, layer_idx);
        if self.index.contains_key(&key) {
            if !self.kv_cache.contains_key(&key) {
                self.enforce_kv_cache_limit();
                self.kv_cache.insert(key, (k, v));
                self.kv_cache_keys.push_back(key);
            }
            return Ok(key);
        }

        let rec = KvRecord {
            k: k.clone(),
            v: v.clone(),
        };
        let data = rec.serialise();
        let sha = Self::file_sha(&data);
        let name = format!("kv/{:016x}.kv", key);
        let path = self.base.join(&name);
        fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;

        let entry = SiteMapEntry {
            kind: EntryKind::Kv,
            hash: key,
            file: name,
            file_sha: sha,
            size: data.len() as u64,
        };
        self.index.insert(key, entry);
        self.enforce_kv_cache_limit();
        self.kv_cache.insert(key, (k, v));
        self.kv_cache_keys.push_back(key);
        self.recompute_root();
        Ok(key)
    }

    // ── Node access ───────────────────────────────────────────────────────────

    pub fn put_node(&mut self, node: &NdaNode) -> Result<u64> {
        let key = node.hash();
        if !self.index.contains_key(&key) {
            let data = serialise_node(node);
            let sha = Self::file_sha(&data);
            let name = format!("nodes/{:016x}.nda", key);
            let path = self.base.join(&name);
            fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;

            let entry = SiteMapEntry {
                kind: EntryKind::Node,
                hash: key,
                file: name,
                file_sha: sha,
                size: data.len() as u64,
            };
            self.index.insert(key, entry);
            self.recompute_root();
        }

        let mut triples = Vec::new();
        Self::extract_triples_recursive(node, &mut triples);
        self.node_triples_cache.lock_safe().insert(key, triples);

        Ok(key)
    }

    pub fn get_node(&self, hash: u64) -> Option<NdaNode> {
        let entry = self.index.get(&hash)?;
        if entry.kind != EntryKind::Node && entry.kind != EntryKind::Program {
            return None;
        }
        let path = self.base.join(&entry.file);
        let data = fs::read(&path).ok()?;
        if !Self::verify_file_sha(&data, &entry.file_sha) {
            return None;
        }
        if entry.kind == EntryKind::Program {
            if data.len() < 8 {
                return None;
            }
            let root_hash = u64::from_le_bytes(data[..8].try_into().unwrap());
            return self.get_node(root_hash);
        }
        let mut offset = 0;
        deserialise_node(&data, &mut offset).ok()
    }

    pub fn get_any_node_hash(&self) -> Option<u64> {
        self.index.keys().next().copied()
    }

    fn extract_triples_recursive(node: &NdaNode, triples: &mut Vec<VcTriple>) {
        match node {
            NdaNode::Triple {
                subject_hash,
                predicate_id,
                object_hash,
            } => {
                triples.push(VcTriple {
                    subject_hash: *subject_hash,
                    predicate_id: *predicate_id,
                    object_hash: *object_hash,
                });
            }
            NdaNode::Scope { children } => {
                for child in children {
                    Self::extract_triples_recursive(child, triples);
                }
            }
            NdaNode::Loop { body, .. } => {
                for child in body {
                    Self::extract_triples_recursive(child, triples);
                }
            }
            NdaNode::While { cond, body } => {
                Self::extract_triples_recursive(cond, triples);
                for child in body {
                    Self::extract_triples_recursive(child, triples);
                }
            }
            NdaNode::If {
                cond,
                then_body,
                else_body,
            } => {
                Self::extract_triples_recursive(cond, triples);
                for child in then_body {
                    Self::extract_triples_recursive(child, triples);
                }
                if let Some(eb) = else_body {
                    for child in eb {
                        Self::extract_triples_recursive(child, triples);
                    }
                }
            }
            NdaNode::Compare { lhs, rhs, .. } => {
                Self::extract_triples_recursive(lhs, triples);
                Self::extract_triples_recursive(rhs, triples);
            }
            NdaNode::Let { init, .. } => {
                Self::extract_triples_recursive(init, triples);
            }
            NdaNode::Store { value, .. } => {
                Self::extract_triples_recursive(value, triples);
            }
            NdaNode::Add { lhs, rhs } => {
                Self::extract_triples_recursive(lhs, triples);
                Self::extract_triples_recursive(rhs, triples);
            }
            NdaNode::VecOp { operand, .. } => {
                Self::extract_triples_recursive(operand, triples);
            }
            NdaNode::Print { source } => {
                Self::extract_triples_recursive(source, triples);
            }
            NdaNode::Return { value } => {
                Self::extract_triples_recursive(value, triples);
            }
            NdaNode::Bitwise { lhs, rhs, .. } => {
                Self::extract_triples_recursive(lhs, triples);
                if let Some(r) = rhs {
                    Self::extract_triples_recursive(r, triples);
                }
            }
            NdaNode::Math { lhs, rhs, .. } => {
                Self::extract_triples_recursive(lhs, triples);
                Self::extract_triples_recursive(rhs, triples);
            }
            NdaNode::MathFunc { operand, .. } => {
                Self::extract_triples_recursive(operand, triples);
            }
            NdaNode::Peek { addr } => {
                Self::extract_triples_recursive(addr, triples);
            }
            NdaNode::Poke { addr, value } => {
                Self::extract_triples_recursive(addr, triples);
                Self::extract_triples_recursive(value, triples);
            }
            NdaNode::Gemv { matrix, vector } => {
                Self::extract_triples_recursive(matrix, triples);
                Self::extract_triples_recursive(vector, triples);
            }
            NdaNode::Dot { lhs, rhs } => {
                Self::extract_triples_recursive(lhs, triples);
                Self::extract_triples_recursive(rhs, triples);
            }
            NdaNode::Syscall { args, .. } => {
                for arg in args {
                    Self::extract_triples_recursive(arg, triples);
                }
            }
            NdaNode::Atomic { addr, val, .. } => {
                Self::extract_triples_recursive(addr, triples);
                Self::extract_triples_recursive(val, triples);
            }
            NdaNode::Alloc { size } => {
                Self::extract_triples_recursive(size, triples);
            }
            NdaNode::Free { addr } => {
                Self::extract_triples_recursive(addr, triples);
            }
            NdaNode::Cast { operand, .. } => {
                Self::extract_triples_recursive(operand, triples);
            }
            NdaNode::GpuDispatch { args, .. } => {
                for arg in args {
                    Self::extract_triples_recursive(arg, triples);
                }
            }
            _ => {}
        }
    }

    pub fn find_triples(
        &self,
        subject: Option<u64>,
        predicate: Option<u16>,
        object: Option<u64>,
    ) -> Vec<VcTriple> {
        self.filter_triples(
            self.collect_historical_triples(),
            subject,
            predicate,
            object,
        )
    }

    pub fn find_live_triples(
        &self,
        subject: Option<u64>,
        predicate: Option<u16>,
        object: Option<u64>,
    ) -> Vec<VcTriple> {
        self.filter_triples(
            self.collect_live_snapshot_triples(),
            subject,
            predicate,
            object,
        )
    }

    pub fn get_callers(&self, method_hash: u64) -> Vec<u64> {
        self.find_live_triples(None, Some(2), Some(method_hash))
            .into_iter()
            .map(|t| t.subject_hash)
            .collect()
    }

    pub fn get_dependencies(&self, method_hash: u64) -> Vec<u64> {
        self.find_live_triples(Some(method_hash), None, None)
            .into_iter()
            .map(|t| t.object_hash)
            .collect()
    }

    pub fn put_file_snapshot(&mut self, file_path: &str, triples: &[VcTriple]) -> Result<u64> {
        let key = self.snapshot_hash(file_path);
        let data = serde_json::to_vec(triples)?;
        let sha = Self::file_sha(&data);
        let name = format!("snapshots/{:016x}.json", key);
        let path = self.base.join(&name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;

        let entry = SiteMapEntry {
            kind: EntryKind::Snapshot,
            hash: key,
            file: name,
            file_sha: sha,
            size: data.len() as u64,
        };
        self.index.insert(key, entry);
        self.snapshot_triples_cache
            .lock()
            .unwrap()
            .insert(key, triples.to_vec());
        self.recompute_root();
        // Auto-invalidate the predicate index since triples have changed
        self.invalidate_predicate_index();
        Ok(key)
    }

    pub fn remove_file_snapshot(&mut self, file_path: &str) -> Result<bool> {
        let key = self.snapshot_hash(file_path);
        if let Some(entry) = self.index.remove(&key) {
            let path = self.base.join(entry.file);
            if path.exists() {
                fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
            self.snapshot_triples_cache.lock_safe().remove(&key);
            self.recompute_root();
            // Auto-invalidate the predicate index since triples have changed
            self.invalidate_predicate_index();
            return Ok(true);
        }
        Ok(false)
    }

    pub fn put_program(&mut self, root_node: &NdaNode) -> Result<u64> {
        let node_hash = self.put_node(root_node)?;
        let key = self.program_hash(node_hash);
        if self.index.contains_key(&key) {
            return Ok(key);
        }
        let data = node_hash.to_le_bytes().to_vec();
        let sha = Self::file_sha(&data);
        let name = format!("programs/{:016x}.nda", key);
        let path = self.base.join(&name);
        fs::write(&path, &data)?;

        let entry = SiteMapEntry {
            kind: EntryKind::Program,
            hash: key,
            file: name,
            file_sha: sha,
            size: 8,
        };
        self.index.insert(key, entry);
        self.recompute_root();
        Ok(key)
    }

    // ── Persistence ───────────────────────────────────────────────────────────

    pub fn flush(&self) -> Result<()> {
        let entries: Vec<&SiteMapEntry> = self.index.values().collect();
        let json = serde_json::to_string_pretty(&entries)?;
        fs::write(self.base.join("index.json"), json)?;
        fs::write(
            self.base.join("metadata.nda"),
            format!(
                "metadata version 2\nfield_count 1\nfield\tweight_root\t{:016x}\n",
                self.weight_root
            ),
        )?;
        fs::write(
            self.base.join("metadata.json"),
            serde_json::to_string_pretty(
                &serde_json::json!({ "weight_root": format!("{:016x}", self.weight_root) }),
            )?,
        )?;
        Ok(())
    }

    pub fn verify(&self) -> usize {
        let mut bad = 0;
        for entry in self.index.values() {
            let path = self.base.join(&entry.file);
            match fs::read(&path) {
                Ok(data) => {
                    if !Self::verify_file_sha(&data, &entry.file_sha) {
                        eprintln!("[site_map] CORRUPT: {}", path.display());
                        bad += 1;
                    }
                }
                Err(e) => {
                    eprintln!("[site_map] MISSING: {} ({e})", path.display());
                    bad += 1;
                }
            }
        }
        bad
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn weight_root(&self) -> u64 {
        self.weight_root
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    // ── Statistics ────────────────────────────────────────────────────────────

    pub fn stats(&self) -> SiteMapStats {
        let (kv, nodes, programs, snapshots) =
            self.index
                .values()
                .fold(
                    (0usize, 0usize, 0usize, 0usize),
                    |(k, n, p, s), e| match e.kind {
                        EntryKind::Kv => (k + 1, n, p, s),
                        EntryKind::Node => (k, n + 1, p, s),
                        EntryKind::Program => (k, n, p + 1, s),
                        EntryKind::Snapshot => (k, n, p, s + 1),
                    },
                );
        let total_bytes: u64 = self.index.values().map(|e| e.size).sum();
        let string_dict_size = {
            let dict = self.string_dict.lock_safe();
            dict.len()
        };
        SiteMapStats {
            kv,
            nodes,
            programs,
            snapshots,
            total_bytes,
            root: self.root,
            weight_root: self.weight_root,
            kv_cache_size: self.kv_cache.len(),
            string_dict_size,
            total_entries: self.index.len(),
        }
    }

    pub fn register_string(&self, s: &str) -> Result<u64> {
        let hash = self.hash_string(s);
        let mut dict = self.string_dict.lock_safe();

        if dict.is_empty() {
            let dict_path = self.base.join("dictionary.json");
            if dict_path.exists() {
                if let Ok(raw) = fs::read_to_string(&dict_path) {
                    if let Ok(loaded) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                        for (k_str, v_str) in loaded {
                            if let Ok(k_num) = u64::from_str_radix(&k_str, 16) {
                                dict.insert(k_num, v_str);
                            }
                        }
                    }
                }
            }
        }

        if let std::collections::hash_map::Entry::Vacant(e) = dict.entry(hash) {
            e.insert(s.to_string());
            let dict_path = self.base.join("dictionary.json");
            let mut serializable = HashMap::new();
            for (k_num, v_str) in dict.iter() {
                serializable.insert(format!("{:016x}", k_num), v_str.clone());
            }
            if let Ok(updated) = serde_json::to_string_pretty(&serializable) {
                let _ = fs::write(&dict_path, updated);
            }
        }
        Ok(hash)
    }

    pub fn resolve_string(&self, hash: u64) -> Option<String> {
        let mut dict = self.string_dict.lock_safe();

        if dict.is_empty() {
            let dict_path = self.base.join("dictionary.json");
            if dict_path.exists() {
                if let Ok(raw) = fs::read_to_string(&dict_path) {
                    if let Ok(loaded) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                        for (k_str, v_str) in loaded {
                            if let Ok(k_num) = u64::from_str_radix(&k_str, 16) {
                                dict.insert(k_num, v_str);
                            }
                        }
                    }
                }
            }
        }

        dict.get(&hash).cloned()
    }

    pub fn hash_string(&self, s: &str) -> u64 {
        let mut hasher = Sha256::new();
        hasher.update(s.as_bytes());
        let digest = hasher.finalize();
        u64::from_le_bytes(digest[0..8].try_into().unwrap())
    }

    // ── Batch operations ─────────────────────────────────────────────────────

    /// Insert multiple KV pairs, recomputing the Merkle root only once at the end.
    /// Returns the list of keys that were inserted (or updated in cache).
    pub fn put_kv_batch(
        &mut self,
        items: &[(u32, u32, NdaVec, NdaVec)],
    ) -> Result<Vec<u64>> {
        let mut keys = Vec::with_capacity(items.len());
        for &(token_id, layer_idx, ref k, ref v) in items {
            let key = self.token_hash(token_id, layer_idx);
            if self.index.contains_key(&key) {
                if !self.kv_cache.contains_key(&key) {
                    self.enforce_kv_cache_limit();
                    self.kv_cache.insert(key, (k.clone(), v.clone()));
                    self.kv_cache_keys.push_back(key);
                }
                keys.push(key);
                continue;
            }
            let rec = KvRecord {
                k: k.clone(),
                v: v.clone(),
            };
            let data = rec.serialise();
            let sha = Self::file_sha(&data);
            let name = format!("kv/{:016x}.kv", key);
            let path = self.base.join(&name);
            fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;
            let entry = SiteMapEntry {
                kind: EntryKind::Kv,
                hash: key,
                file: name,
                file_sha: sha,
                size: data.len() as u64,
            };
            self.index.insert(key, entry);
            self.enforce_kv_cache_limit();
            self.kv_cache.insert(key, (k.clone(), v.clone()));
            self.kv_cache_keys.push_back(key);
            keys.push(key);
        }
        self.recompute_root();
        Ok(keys)
    }

    /// Insert multiple nodes, recomputing the Merkle root only once at the end.
    pub fn put_nodes_batch(&mut self, nodes: &[&NdaNode]) -> Result<Vec<u64>> {
        let mut keys = Vec::with_capacity(nodes.len());
        for &node in nodes {
            let key = node.hash();
            if !self.index.contains_key(&key) {
                let data = serialise_node(node);
                let sha = Self::file_sha(&data);
                let name = format!("nodes/{:016x}.nda", key);
                let path = self.base.join(&name);
                fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;
                let entry = SiteMapEntry {
                    kind: EntryKind::Node,
                    hash: key,
                    file: name,
                    file_sha: sha,
                    size: data.len() as u64,
                };
                self.index.insert(key, entry);
            }
            let mut triples = Vec::new();
            Self::extract_triples_recursive(node, &mut triples);
            self.node_triples_cache.lock_safe().insert(key, triples);
            keys.push(key);
        }
        self.recompute_root();
        Ok(keys)
    }

    /// Register multiple strings, writing dictionary.json only once.
    pub fn register_strings_batch(&self, strings: &[&str]) -> Result<Vec<u64>> {
        let mut dict = self.string_dict.lock_safe();
        if dict.is_empty() {
            let dict_path = self.base.join("dictionary.json");
            if dict_path.exists() {
                if let Ok(raw) = fs::read_to_string(&dict_path) {
                    if let Ok(loaded) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                        for (k_str, v_str) in loaded {
                            if let Ok(k_num) = u64::from_str_radix(&k_str, 16) {
                                dict.insert(k_num, v_str);
                            }
                        }
                    }
                }
            }
        }
        let mut hashes = Vec::with_capacity(strings.len());
        let mut any_new = false;
        for s in strings {
            let hash = self.hash_string(s);
            if let std::collections::hash_map::Entry::Vacant(e) = dict.entry(hash) {
                e.insert(s.to_string());
                any_new = true;
            }
            hashes.push(hash);
        }
        if any_new {
            let dict_path = self.base.join("dictionary.json");
            let mut serializable = HashMap::new();
            for (k_num, v_str) in dict.iter() {
                serializable.insert(format!("{:016x}", k_num), v_str.clone());
            }
            if let Ok(updated) = serde_json::to_string_pretty(&serializable) {
                let _ = fs::write(&dict_path, updated);
            }
        }
        Ok(hashes)
    }

    /// Register multiple file snapshots, recomputing root and invalidating
    /// the predicate index only once at the end.
    pub fn put_file_snapshots_batch(
        &mut self,
        snapshots: &[(&str, &[VcTriple])],
    ) -> Result<Vec<u64>> {
        let mut keys = Vec::with_capacity(snapshots.len());
        for &(file_path, triples) in snapshots {
            let key = self.snapshot_hash(file_path);
            let data = serde_json::to_vec(triples)?;
            let sha = Self::file_sha(&data);
            let name = format!("snapshots/{:016x}.json", key);
            let path = self.base.join(&name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &data).with_context(|| format!("writing {}", path.display()))?;
            let entry = SiteMapEntry {
                kind: EntryKind::Snapshot,
                hash: key,
                file: name,
                file_sha: sha,
                size: data.len() as u64,
            };
            self.index.insert(key, entry);
            self.snapshot_triples_cache
                .lock()
                .unwrap()
                .insert(key, triples.to_vec());
            keys.push(key);
        }
        self.recompute_root();
        self.invalidate_predicate_index();
        Ok(keys)
    }

    /// Resolve multiple string hashes at once (loads dictionary only once).
    pub fn resolve_strings_batch(&self, hashes: &[u64]) -> Vec<Option<String>> {
        let mut dict = self.string_dict.lock_safe();
        if dict.is_empty() {
            let dict_path = self.base.join("dictionary.json");
            if dict_path.exists() {
                if let Ok(raw) = fs::read_to_string(&dict_path) {
                    if let Ok(loaded) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                        for (k_str, v_str) in loaded {
                            if let Ok(k_num) = u64::from_str_radix(&k_str, 16) {
                                dict.insert(k_num, v_str);
                            }
                        }
                    }
                }
            }
        }
        hashes.iter().map(|h| dict.get(h).cloned()).collect()
    }

    // ── Query helpers ─────────────────────────────────────────────────────────

    /// Return all entries of a specific kind.
    pub fn entries_by_kind(&self, kind: &EntryKind) -> Vec<&SiteMapEntry> {
        self.index.values().filter(|e| &e.kind == kind).collect()
    }

    /// Return the top-N largest entries by byte size.
    pub fn largest_entries(&self, n: usize) -> Vec<&SiteMapEntry> {
        let mut entries: Vec<&SiteMapEntry> = self.index.values().collect();
        entries.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        entries.truncate(n);
        entries
    }

    /// Return all string dictionary entries.
    pub fn all_strings(&self) -> HashMap<u64, String> {
        let mut dict = self.string_dict.lock_safe();
        if dict.is_empty() {
            let dict_path = self.base.join("dictionary.json");
            if dict_path.exists() {
                if let Ok(raw) = fs::read_to_string(&dict_path) {
                    if let Ok(loaded) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                        for (k_str, v_str) in loaded {
                            if let Ok(k_num) = u64::from_str_radix(&k_str, 16) {
                                dict.insert(k_num, v_str);
                            }
                        }
                    }
                }
            }
        }
        dict.clone()
    }

    // ── Internal helpers ──────────────────────────────────────────────────────────

    fn recompute_root(&mut self) {
        self.root = Self::compute_index_root(self.index.values());
    }

    fn compute_index_root<'a>(entries: impl IntoIterator<Item = &'a SiteMapEntry>) -> u64 {
        let mut hashes: Vec<u64> = entries.into_iter().map(|e| e.hash).collect();
        hashes.sort_unstable();
        let mut h = Sha256::new();
        h.update(b"IDX");
        for hash in hashes {
            h.update(hash.to_le_bytes());
        }
        let d = h.finalize();
        u64::from_le_bytes(d[..8].try_into().unwrap())
    }

    fn program_hash(&self, node_hash: u64) -> u64 {
        let mut h = Sha256::new();
        h.update(b"prog");
        h.update(node_hash.to_le_bytes());
        let d = h.finalize();
        u64::from_le_bytes(d[..8].try_into().unwrap())
    }

    fn snapshot_hash(&self, file_path: &str) -> u64 {
        let mut h = Sha256::new();
        h.update(b"snapshot");
        h.update(file_path.as_bytes());
        let d = h.finalize();
        u64::from_le_bytes(d[..8].try_into().unwrap())
    }

    fn collect_historical_triples(&self) -> Vec<VcTriple> {
        let mut all_triples = Vec::new();
        let mut cache = self.node_triples_cache.lock_safe();

        for entry in self.index.values() {
            if entry.kind != EntryKind::Node {
                continue;
            }
            if let Some(cached) = cache.get(&entry.hash) {
                all_triples.extend(cached.clone());
                continue;
            }

            if let Some(node) = self.get_node(entry.hash) {
                let mut node_triples = Vec::new();
                Self::extract_triples_recursive(&node, &mut node_triples);
                cache.insert(entry.hash, node_triples.clone());
                all_triples.extend(node_triples);
            }
        }
        all_triples
    }

    fn collect_live_snapshot_triples(&self) -> Vec<VcTriple> {
        let mut all_triples = Vec::new();
        let mut cache = self.snapshot_triples_cache.lock_safe();

        for entry in self.index.values() {
            if entry.kind != EntryKind::Snapshot {
                continue;
            }
            if let Some(cached) = cache.get(&entry.hash) {
                all_triples.extend(cached.clone());
                continue;
            }

            let path = self.base.join(&entry.file);
            let data = match fs::read(&path) {
                Ok(data) => data,
                Err(_) => continue,
            };
            if !Self::verify_file_sha(&data, &entry.file_sha) {
                continue;
            }
            if let Ok(triples) = serde_json::from_slice::<Vec<VcTriple>>(&data) {
                cache.insert(entry.hash, triples.clone());
                all_triples.extend(triples);
            }
        }
        all_triples
    }

    fn filter_triples(
        &self,
        triples: Vec<VcTriple>,
        subject: Option<u64>,
        predicate: Option<u16>,
        object: Option<u64>,
    ) -> Vec<VcTriple> {
        triples
            .into_iter()
            .filter(|t| {
                if let Some(s) = subject {
                    if t.subject_hash != s {
                        return false;
                    }
                }
                if let Some(p) = predicate {
                    if t.predicate_id != p {
                        return false;
                    }
                }
                if let Some(o) = object {
                    if t.object_hash != o {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Build the predicate index lazily. Returns a reference to the indexed triples
    /// for a specific predicate, avoiding a full scan of all triples.
    pub fn find_live_by_predicate(&self, predicate_id: u16) -> Vec<VcTriple> {
        // Check if the index is already built
        {
            let idx = self.predicate_index.lock_safe();
            if let Some(index) = idx.as_ref() {
                if let Some(triples) = index.get(&predicate_id) {
                    return triples.clone();
                }
                return Vec::new();
            }
        }
        // Build the index from all live triples
        let all = self.collect_live_snapshot_triples();
        let mut index: HashMap<u16, Vec<VcTriple>> = HashMap::new();
        for triple in &all {
            index
                .entry(triple.predicate_id)
                .or_default()
                .push(triple.clone());
        }
        let result = index.get(&predicate_id).cloned().unwrap_or_default();
        *self.predicate_index.lock_safe() = Some(index);
        result
    }

    /// Invalidate the predicate index (call when snapshots are updated).
    pub fn invalidate_predicate_index(&self) {
        *self.predicate_index.lock_safe() = None;
    }

    /// Batch query: find all triples matching any of the given subject hashes.
    /// More efficient than calling find_live_triples in a loop.
    pub fn find_live_by_subjects(&self, subjects: &[u64]) -> Vec<VcTriple> {
        let subject_set: std::collections::HashSet<u64> = subjects.iter().cloned().collect();
        self.collect_live_snapshot_triples()
            .into_iter()
            .filter(|t| subject_set.contains(&t.subject_hash))
            .collect()
    }

    /// Batch query: find all triples matching any of the given object hashes.
    pub fn find_live_by_objects(&self, objects: &[u64]) -> Vec<VcTriple> {
        let object_set: std::collections::HashSet<u64> = objects.iter().cloned().collect();
        self.collect_live_snapshot_triples()
            .into_iter()
            .filter(|t| object_set.contains(&t.object_hash))
            .collect()
    }

    fn file_sha(data: &[u8]) -> String {
        let d = Sha256::digest(data);
        format!("{:x}", d)
    }

    fn verify_file_sha(data: &[u8], expected_hex: &str) -> bool {
        let d = Sha256::digest(data);
        format!("{:x}", d) == expected_hex
    }
}

/// Diagnostic snapshot of a SiteMap.
#[derive(Debug, Clone, Serialize)]
pub struct SiteMapInfo {
    pub total_entries: usize,
    pub kv_count: usize,
    pub node_count: usize,
    pub program_count: usize,
    pub snapshot_count: usize,
    pub total_bytes: u64,
    pub root: u64,
    pub weight_root: u64,
    pub kv_cache_size: usize,
    pub kv_cache_max: usize,
    pub string_dict_size: usize,
    pub validation_issues: Vec<String>,
}

/// Detailed verification report.
#[derive(Debug, Clone, Serialize)]
pub struct SiteMapVerifyReport {
    pub total_entries: usize,
    pub checked: usize,
    pub corrupt: usize,
    pub missing: usize,
    pub valid: usize,
    pub issues: Vec<String>,
}

impl SiteMap {
    /// Return a diagnostic snapshot of this SiteMap.
    pub fn info(&self) -> SiteMapInfo {
        let stats = self.stats();
        SiteMapInfo {
            total_entries: stats.total_entries,
            kv_count: stats.kv,
            node_count: stats.nodes,
            program_count: stats.programs,
            snapshot_count: stats.snapshots,
            total_bytes: stats.total_bytes,
            root: self.root,
            weight_root: self.weight_root,
            kv_cache_size: stats.kv_cache_size,
            kv_cache_max: MAX_KV_CACHE_SIZE,
            string_dict_size: stats.string_dict_size,
            validation_issues: self.validate(),
        }
    }

    /// Validate the SiteMap for consistency.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.index.is_empty() {
            warnings.push("SiteMap index is empty".to_string());
        }

        // Check for entries with empty file paths.
        for entry in self.index.values() {
            if entry.file.is_empty() {
                warnings.push(format!("Entry {:016x} has empty file path", entry.hash));
            }
            if entry.file_sha.is_empty() {
                warnings.push(format!("Entry {:016x} has empty file_sha", entry.hash));
            }
            if entry.size == 0 {
                warnings.push(format!("Entry {:016x} has zero size", entry.hash));
            }
        }

        // Check for duplicate file paths (shouldn't happen but be safe).
        let mut file_set = std::collections::HashSet::new();
        for entry in self.index.values() {
            if !file_set.insert(&entry.file) {
                warnings.push(format!("Duplicate file path: {}", entry.file));
            }
        }

        warnings
    }

    /// Detailed verification: check every entry on disk.
    /// Returns a report with per-entry results.
    pub fn verify_detailed(&self) -> SiteMapVerifyReport {
        let mut report = SiteMapVerifyReport {
            total_entries: self.index.len(),
            checked: 0,
            corrupt: 0,
            missing: 0,
            valid: 0,
            issues: Vec::new(),
        };

        for entry in self.index.values() {
            report.checked += 1;
            let path = self.base.join(&entry.file);
            match fs::read(&path) {
                Ok(data) => {
                    if Self::verify_file_sha(&data, &entry.file_sha) {
                        report.valid += 1;
                    } else {
                        report.corrupt += 1;
                        report.issues.push(format!(
                            "CORRUPT: {} (SHA mismatch)",
                            entry.file
                        ));
                    }
                }
                Err(e) => {
                    report.missing += 1;
                    report.issues.push(format!(
                        "MISSING: {} ({})",
                        entry.file, e
                    ));
                }
            }
        }

        report
    }

    /// Compound query: find triples matching both subject and predicate.
    pub fn find_live_by_subject_and_predicate(
        &self,
        subject: u64,
        predicate: u16,
    ) -> Vec<VcTriple> {
        self.collect_live_snapshot_triples()
            .into_iter()
            .filter(|t| t.subject_hash == subject && t.predicate_id == predicate)
            .collect()
    }

    /// Compound query: find triples matching both predicate and object.
    pub fn find_live_by_predicate_and_object(
        &self,
        predicate: u16,
        object: u64,
    ) -> Vec<VcTriple> {
        self.collect_live_snapshot_triples()
            .into_iter()
            .filter(|t| t.predicate_id == predicate && t.object_hash == object)
            .collect()
    }

    /// Cache utilization ratio (0.0 to 1.0).
    pub fn cache_utilization(&self) -> f64 {
        if MAX_KV_CACHE_SIZE == 0 {
            return 0.0;
        }
        self.kv_cache.len() as f64 / MAX_KV_CACHE_SIZE as f64
    }

    /// Export a summary of the SiteMap suitable for JSON output.
    pub fn export_summary(&self) -> SiteMapSummary {
        let stats = self.stats();
        SiteMapSummary {
            stats: stats.to_string(),
            root: format!("{:016x}", self.root),
            weight_root: format!("{:016x}", self.weight_root),
            cache_utilization: format!("{:.1}%", self.cache_utilization() * 100.0),
            validation_clean: self.validate().is_empty(),
        }
    }
}

/// JSON-exportable summary of a SiteMap.
#[derive(Debug, Clone, Serialize)]
pub struct SiteMapSummary {
    pub stats: String,
    pub root: String,
    pub weight_root: String,
    pub cache_utilization: String,
    pub validation_clean: bool,
}

/// Report from a timed store operation.
#[derive(Debug, Clone, Serialize)]
pub struct StoreOperationReport {
    pub operation: String,
    pub elapsed_us: u64,
    pub entries_affected: usize,
    pub total_entries_after: usize,
}

/// Analysis of triple distribution across predicates.
#[derive(Debug, Clone, Serialize)]
pub struct TripleAnalysis {
    pub total_triples: usize,
    pub unique_predicates: usize,
    pub predicate_distribution: Vec<PredicateCount>,
    pub unique_subjects: usize,
    pub unique_objects: usize,
}

/// Count of triples for a specific predicate.
#[derive(Debug, Clone, Serialize)]
pub struct PredicateCount {
    pub predicate_id: u16,
    pub count: usize,
}

impl SiteMap {
    /// Get human-readable name for an entry kind.
    pub fn entry_kind_name(kind: &EntryKind) -> &'static str {
        match kind {
            EntryKind::Kv => "KV",
            EntryKind::Node => "Node",
            EntryKind::Program => "Program",
            EntryKind::Snapshot => "Snapshot",
        }
    }

    /// Validate a single entry for consistency.
    pub fn validate_entry(entry: &SiteMapEntry) -> Vec<String> {
        let mut issues = Vec::new();
        if entry.file.is_empty() {
            issues.push(format!("Entry {:016x} has empty file path", entry.hash));
        }
        if entry.file_sha.is_empty() {
            issues.push(format!("Entry {:016x} has empty file_sha", entry.hash));
        }
        if entry.size == 0 {
            issues.push(format!("Entry {:016x} has zero size", entry.hash));
        }
        issues
    }

    /// Analyse the distribution of triples across predicates.
    pub fn triple_analysis(&self) -> TripleAnalysis {
        let triples = self.collect_live_snapshot_triples();
        let total = triples.len();

        let mut pred_counts: HashMap<u16, usize> = HashMap::new();
        let mut subjects = std::collections::HashSet::new();
        let mut objects = std::collections::HashSet::new();

        for t in &triples {
            *pred_counts.entry(t.predicate_id).or_default() += 1;
            subjects.insert(t.subject_hash);
            objects.insert(t.object_hash);
        }

        let mut distribution: Vec<PredicateCount> = pred_counts
            .into_iter()
            .map(|(predicate_id, count)| PredicateCount { predicate_id, count })
            .collect();
        distribution.sort_by(|a, b| b.count.cmp(&a.count));

        TripleAnalysis {
            total_triples: total,
            unique_predicates: distribution.len(),
            predicate_distribution: distribution,
            unique_subjects: subjects.len(),
            unique_objects: objects.len(),
        }
    }

    /// Put a KV pair with a timing report.
    pub fn put_kv_reported(
        &mut self,
        token_id: u32,
        layer_idx: u32,
        k: NdaVec,
        v: NdaVec,
    ) -> Result<(u64, StoreOperationReport)> {
        let start = std::time::Instant::now();
        let key = self.put_kv(token_id, layer_idx, k, v)?;
        let elapsed = start.elapsed().as_micros() as u64;
        let report = StoreOperationReport {
            operation: "put_kv".to_string(),
            elapsed_us: elapsed.max(1),
            entries_affected: 1,
            total_entries_after: self.index.len(),
        };
        Ok((key, report))
    }

    /// Put a node with a timing report.
    pub fn put_node_reported(
        &mut self,
        node: &NdaNode,
    ) -> Result<(u64, StoreOperationReport)> {
        let start = std::time::Instant::now();
        let key = self.put_node(node)?;
        let elapsed = start.elapsed().as_micros() as u64;
        let report = StoreOperationReport {
            operation: "put_node".to_string(),
            elapsed_us: elapsed.max(1),
            entries_affected: 1,
            total_entries_after: self.index.len(),
        };
        Ok((key, report))
    }

    /// Flush with a timing report.
    pub fn flush_report(&self) -> Result<StoreOperationReport> {
        let start = std::time::Instant::now();
        self.flush()?;
        let elapsed = start.elapsed().as_micros() as u64;
        Ok(StoreOperationReport {
            operation: "flush".to_string(),
            elapsed_us: elapsed.max(1),
            entries_affected: self.index.len(),
            total_entries_after: self.index.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nda_int::NdaVec;
    use crate::site_map::types::{EntryKind, SiteMapEntry, VcTriple};
    use crate::site_map::verifier::NdaNode;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sitemap_store_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    fn test_ndavec(values: &[i32]) -> NdaVec {
        let f32_values: Vec<f32> = values.iter().map(|&v| v as f32).collect();
        NdaVec::from_f32_slice(&f32_values)
    }

    // ── file_sha / verify_file_sha ──────────────────────────────────────────

    #[test]
    fn file_sha_deterministic() {
        let data = b"hello world";
        let sha1 = SiteMap::file_sha(data);
        let sha2 = SiteMap::file_sha(data);
        assert_eq!(sha1, sha2);
    }

    #[test]
    fn file_sha_different_data() {
        let sha1 = SiteMap::file_sha(b"hello");
        let sha2 = SiteMap::file_sha(b"world");
        assert_ne!(sha1, sha2);
    }

    #[test]
    fn file_sha_empty_data() {
        let sha = SiteMap::file_sha(b"");
        assert!(!sha.is_empty());
    }

    #[test]
    fn verify_file_sha_valid() {
        let data = b"test data";
        let sha = SiteMap::file_sha(data);
        assert!(SiteMap::verify_file_sha(data, &sha));
    }

    #[test]
    fn verify_file_sha_invalid() {
        let data = b"test data";
        assert!(!SiteMap::verify_file_sha(data, "deadbeef"));
    }

    #[test]
    fn verify_file_sha_tampered() {
        let data = b"test data";
        let sha = SiteMap::file_sha(data);
        assert!(!SiteMap::verify_file_sha(b"tampered", &sha));
    }

    // ── compute_index_root ──────────────────────────────────────────────────

    #[test]
    fn compute_index_root_empty() {
        let root = SiteMap::compute_index_root(std::iter::empty());
        assert_ne!(root, 0);
    }

    #[test]
    fn compute_index_root_order_independent() {
        let e1 = SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "a".into(), file_sha: "x".into(), size: 1 };
        let e2 = SiteMapEntry { kind: EntryKind::Kv, hash: 2, file: "b".into(), file_sha: "y".into(), size: 2 };
        let root_ab = SiteMap::compute_index_root(vec![&e1, &e2]);
        let root_ba = SiteMap::compute_index_root(vec![&e2, &e1]);
        assert_eq!(root_ab, root_ba);
    }

    #[test]
    fn compute_index_root_different_entries() {
        let e1 = SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "a".into(), file_sha: "x".into(), size: 1 };
        let e2 = SiteMapEntry { kind: EntryKind::Kv, hash: 2, file: "b".into(), file_sha: "y".into(), size: 2 };
        let root1 = SiteMap::compute_index_root(vec![&e1]);
        let root2 = SiteMap::compute_index_root(vec![&e2]);
        assert_ne!(root1, root2);
    }

    // ── extract_triples_recursive ───────────────────────────────────────────

    #[test]
    fn extract_triples_from_triple_node() {
        let node = NdaNode::Triple { subject_hash: 10, predicate_id: 2, object_hash: 20 };
        let mut triples = Vec::new();
        SiteMap::extract_triples_recursive(&node, &mut triples);
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].subject_hash, 10);
        assert_eq!(triples[0].predicate_id, 2);
        assert_eq!(triples[0].object_hash, 20);
    }

    #[test]
    fn extract_triples_from_scope() {
        let node = NdaNode::Scope { children: vec![
            NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            NdaNode::Triple { subject_hash: 4, predicate_id: 5, object_hash: 6 },
        ]};
        let mut triples = Vec::new();
        SiteMap::extract_triples_recursive(&node, &mut triples);
        assert_eq!(triples.len(), 2);
    }

    #[test]
    fn extract_triples_from_loop_body() {
        let node = NdaNode::Loop { count: 5, body: vec![
            NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
        ]};
        let mut triples = Vec::new();
        SiteMap::extract_triples_recursive(&node, &mut triples);
        assert_eq!(triples.len(), 1);
    }

    #[test]
    fn extract_triples_from_if_branches() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 }),
            then_body: vec![NdaNode::Triple { subject_hash: 4, predicate_id: 5, object_hash: 6 }],
            else_body: Some(vec![NdaNode::Triple { subject_hash: 7, predicate_id: 8, object_hash: 9 }]),
        };
        let mut triples = Vec::new();
        SiteMap::extract_triples_recursive(&node, &mut triples);
        assert_eq!(triples.len(), 3);
    }

    #[test]
    fn extract_triples_no_triples() {
        let node = NdaNode::Int { value: 42 };
        let mut triples = Vec::new();
        SiteMap::extract_triples_recursive(&node, &mut triples);
        assert!(triples.is_empty());
    }

    #[test]
    fn extract_triples_nested() {
        let node = NdaNode::Scope { children: vec![
            NdaNode::Loop { count: 3, body: vec![
                NdaNode::If {
                    cond: Box::new(NdaNode::Int { value: 1 }),
                    then_body: vec![NdaNode::Triple { subject_hash: 1, predicate_id: 2, object_hash: 3 }],
                    else_body: None,
                },
            ]},
        ]};
        let mut triples = Vec::new();
        SiteMap::extract_triples_recursive(&node, &mut triples);
        assert_eq!(triples.len(), 1);
    }

    // ── filter_triples ──────────────────────────────────────────────────────

    #[test]
    fn filter_triples_no_filters() {
        let sm = SiteMap::open(&temp_dir("filter1"), 0).unwrap();
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            VcTriple { subject_hash: 4, predicate_id: 5, object_hash: 6 },
        ];
        let result = sm.filter_triples(triples, None, None, None);
        assert_eq!(result.len(), 2);
        cleanup(&sm.base);
    }

    #[test]
    fn filter_triples_by_subject() {
        let sm = SiteMap::open(&temp_dir("filter2"), 0).unwrap();
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            VcTriple { subject_hash: 4, predicate_id: 5, object_hash: 6 },
        ];
        let result = sm.filter_triples(triples, Some(1), None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].subject_hash, 1);
        cleanup(&sm.base);
    }

    #[test]
    fn filter_triples_by_predicate() {
        let sm = SiteMap::open(&temp_dir("filter3"), 0).unwrap();
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            VcTriple { subject_hash: 4, predicate_id: 5, object_hash: 6 },
        ];
        let result = sm.filter_triples(triples, None, Some(5), None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].predicate_id, 5);
        cleanup(&sm.base);
    }

    #[test]
    fn filter_triples_by_object() {
        let sm = SiteMap::open(&temp_dir("filter4"), 0).unwrap();
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            VcTriple { subject_hash: 4, predicate_id: 5, object_hash: 6 },
        ];
        let result = sm.filter_triples(triples, None, None, Some(6));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].object_hash, 6);
        cleanup(&sm.base);
    }

    #[test]
    fn filter_triples_combined() {
        let sm = SiteMap::open(&temp_dir("filter5"), 0).unwrap();
        let triples = vec![
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 3 },
            VcTriple { subject_hash: 1, predicate_id: 5, object_hash: 3 },
            VcTriple { subject_hash: 4, predicate_id: 2, object_hash: 6 },
        ];
        let result = sm.filter_triples(triples, Some(1), None, Some(3));
        assert_eq!(result.len(), 2);
        cleanup(&sm.base);
    }

    // ── open / basic accessors ──────────────────────────────────────────────

    #[test]
    fn open_creates_directories() {
        let dir = temp_dir("open_dirs");
        let sm = SiteMap::open(&dir, 0x1234).unwrap();
        assert!(dir.join("kv").exists());
        assert!(dir.join("nodes").exists());
        assert!(dir.join("programs").exists());
        cleanup(&dir);
    }

    #[test]
    fn open_empty_has_zero_counts() {
        let dir = temp_dir("open_empty");
        let sm = SiteMap::open(&dir, 0).unwrap();
        assert_eq!(sm.len(), 0);
        assert!(sm.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn open_preserves_weight_root() {
        let dir = temp_dir("open_weight");
        let sm = SiteMap::open(&dir, 0xABCD).unwrap();
        assert_eq!(sm.weight_root(), 0xABCD);
        cleanup(&dir);
    }

    // ── token_hash ──────────────────────────────────────────────────────────

    #[test]
    fn token_hash_deterministic() {
        let dir = temp_dir("thash1");
        let sm = SiteMap::open(&dir, 42).unwrap();
        let h1 = sm.token_hash(1, 0);
        let h2 = sm.token_hash(1, 0);
        assert_eq!(h1, h2);
        cleanup(&dir);
    }

    #[test]
    fn token_hash_varies_with_token() {
        let dir = temp_dir("thash2");
        let sm = SiteMap::open(&dir, 42).unwrap();
        let h1 = sm.token_hash(1, 0);
        let h2 = sm.token_hash(2, 0);
        assert_ne!(h1, h2);
        cleanup(&dir);
    }

    #[test]
    fn token_hash_varies_with_layer() {
        let dir = temp_dir("thash3");
        let sm = SiteMap::open(&dir, 42).unwrap();
        let h1 = sm.token_hash(1, 0);
        let h2 = sm.token_hash(1, 1);
        assert_ne!(h1, h2);
        cleanup(&dir);
    }

    #[test]
    fn token_hash_varies_with_weight_root() {
        let dir1 = temp_dir("thash4a");
        let dir2 = temp_dir("thash4b");
        let sm1 = SiteMap::open(&dir1, 1).unwrap();
        let sm2 = SiteMap::open(&dir2, 2).unwrap();
        assert_ne!(sm1.token_hash(1, 0), sm2.token_hash(1, 0));
        cleanup(&dir1);
        cleanup(&dir2);
    }

    // ── hash_string ─────────────────────────────────────────────────────────

    #[test]
    fn hash_string_deterministic() {
        let dir = temp_dir("hstr1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let h1 = sm.hash_string("hello");
        let h2 = sm.hash_string("hello");
        assert_eq!(h1, h2);
        cleanup(&dir);
    }

    #[test]
    fn hash_string_different_strings() {
        let dir = temp_dir("hstr2");
        let sm = SiteMap::open(&dir, 0).unwrap();
        assert_ne!(sm.hash_string("hello"), sm.hash_string("world"));
        cleanup(&dir);
    }

    // ── put_kv ──────────────────────────────────────────────────────────────

    #[test]
    fn put_kv_stores_entry() {
        let dir = temp_dir("putkv1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let k = test_ndavec(&[1, 2, 3]);
        let v = test_ndavec(&[4, 5, 6]);
        let key = sm.put_kv(1, 0, k, v).unwrap();
        assert_ne!(key, 0);
        assert_eq!(sm.len(), 1);
        assert!(!sm.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn put_kv_idempotent() {
        let dir = temp_dir("putkv2");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let k = test_ndavec(&[1, 2]);
        let v = test_ndavec(&[3, 4]);
        let key1 = sm.put_kv(1, 0, k.clone(), v.clone()).unwrap();
        let key2 = sm.put_kv(1, 0, k, v).unwrap();
        assert_eq!(key1, key2);
        assert_eq!(sm.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn put_kv_multiple_distinct_keys() {
        let dir = temp_dir("putkv3");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        sm.put_kv(2, 0, test_ndavec(&[3]), test_ndavec(&[4])).unwrap();
        assert_eq!(sm.len(), 2);
        cleanup(&dir);
    }

    // ── put_node / get_node ─────────────────────────────────────────────────

    #[test]
    fn put_node_stores_and_get_retrieves() {
        let dir = temp_dir("putnode1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let node = NdaNode::Int { value: 42 };
        let key = sm.put_node(&node).unwrap();
        let retrieved = sm.get_node(key);
        assert!(retrieved.is_some());
        cleanup(&dir);
    }

    #[test]
    fn get_node_missing_returns_none() {
        let dir = temp_dir("putnode2");
        let sm = SiteMap::open(&dir, 0).unwrap();
        assert!(sm.get_node(0xDEADBEEF).is_none());
        cleanup(&dir);
    }

    // ── stats ───────────────────────────────────────────────────────────────

    #[test]
    fn stats_empty() {
        let dir = temp_dir("stats1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let s = sm.stats();
        assert_eq!(s.kv, 0);
        assert_eq!(s.nodes, 0);
        assert_eq!(s.total_entries, 0);
        cleanup(&dir);
    }

    #[test]
    fn stats_after_kv() {
        let dir = temp_dir("stats2");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        let s = sm.stats();
        assert_eq!(s.kv, 1);
        assert_eq!(s.total_entries, 1);
        assert!(s.total_bytes > 0);
        cleanup(&dir);
    }

    // ── validate / info / verify ────────────────────────────────────────────

    #[test]
    fn validate_empty_index() {
        let dir = temp_dir("val1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let w = sm.validate();
        assert!(w.iter().any(|s| s.contains("empty")));
        cleanup(&dir);
    }

    #[test]
    fn info_has_correct_counts() {
        let dir = temp_dir("info1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        let info = sm.info();
        assert_eq!(info.kv_count, 1);
        assert_eq!(info.total_entries, 1);
        assert_eq!(info.kv_cache_max, MAX_KV_CACHE_SIZE);
        cleanup(&dir);
    }

    #[test]
    fn verify_clean_after_put() {
        let dir = temp_dir("verify1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        assert_eq!(sm.verify(), 0);
        cleanup(&dir);
    }

    #[test]
    fn verify_detailed_all_valid() {
        let dir = temp_dir("vdet1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        let report = sm.verify_detailed();
        assert_eq!(report.total_entries, 1);
        assert_eq!(report.valid, 1);
        assert_eq!(report.corrupt, 0);
        assert_eq!(report.missing, 0);
        cleanup(&dir);
    }

    // ── register_string / resolve_string ────────────────────────────────────

    #[test]
    fn register_and_resolve_string() {
        let dir = temp_dir("str1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let hash = sm.register_string("hello").unwrap();
        let resolved = sm.resolve_string(hash);
        assert_eq!(resolved, Some("hello".to_string()));
        cleanup(&dir);
    }

    #[test]
    fn resolve_unknown_string() {
        let dir = temp_dir("str2");
        let sm = SiteMap::open(&dir, 0).unwrap();
        assert_eq!(sm.resolve_string(0xDEADBEEF), None);
        cleanup(&dir);
    }

    // ── entries_by_kind / largest_entries ───────────────────────────────────

    #[test]
    fn entries_by_kind_kv() {
        let dir = temp_dir("kind1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        let kv_entries = sm.entries_by_kind(&EntryKind::Kv);
        assert_eq!(kv_entries.len(), 1);
        let node_entries = sm.entries_by_kind(&EntryKind::Node);
        assert!(node_entries.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn largest_entries_sorted() {
        let dir = temp_dir("largest1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        sm.put_kv(2, 0, test_ndavec(&[1, 2, 3, 4, 5]), test_ndavec(&[6, 7, 8, 9, 10])).unwrap();
        let largest = sm.largest_entries(1);
        assert_eq!(largest.len(), 1);
        cleanup(&dir);
    }

    // ── cache_utilization ───────────────────────────────────────────────────

    #[test]
    fn cache_utilization_empty() {
        let dir = temp_dir("cache1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        assert!((sm.cache_utilization() - 0.0).abs() < f64::EPSILON);
        cleanup(&dir);
    }

    // ── export_summary ──────────────────────────────────────────────────────

    #[test]
    fn export_summary_fields() {
        let dir = temp_dir("export1");
        let sm = SiteMap::open(&dir, 0xABCD).unwrap();
        let summary = sm.export_summary();
        assert!(summary.root.contains("0000"));
        assert!(summary.weight_root.contains("abcd"));
        assert!(summary.cache_utilization.contains("0.0%"));
        cleanup(&dir);
    }

    // ── entry_kind_name / validate_entry ────────────────────────────────────

    #[test]
    fn entry_kind_names() {
        assert_eq!(SiteMap::entry_kind_name(&EntryKind::Kv), "KV");
        assert_eq!(SiteMap::entry_kind_name(&EntryKind::Node), "Node");
        assert_eq!(SiteMap::entry_kind_name(&EntryKind::Program), "Program");
        assert_eq!(SiteMap::entry_kind_name(&EntryKind::Snapshot), "Snapshot");
    }

    #[test]
    fn validate_entry_clean() {
        let entry = SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "kv/1.kv".into(), file_sha: "abc".into(), size: 100 };
        assert!(SiteMap::validate_entry(&entry).is_empty());
    }

    #[test]
    fn validate_entry_empty_file() {
        let entry = SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "".into(), file_sha: "abc".into(), size: 100 };
        assert!(!SiteMap::validate_entry(&entry).is_empty());
    }

    #[test]
    fn validate_entry_empty_sha() {
        let entry = SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "a".into(), file_sha: "".into(), size: 100 };
        assert!(!SiteMap::validate_entry(&entry).is_empty());
    }

    #[test]
    fn validate_entry_zero_size() {
        let entry = SiteMapEntry { kind: EntryKind::Kv, hash: 1, file: "a".into(), file_sha: "abc".into(), size: 0 };
        assert!(!SiteMap::validate_entry(&entry).is_empty());
    }

    // ── Serialization ───────────────────────────────────────────────────────

    #[test]
    fn site_map_info_serializes() {
        let info = SiteMapInfo {
            total_entries: 1, kv_count: 1, node_count: 0, program_count: 0, snapshot_count: 0,
            total_bytes: 100, root: 0, weight_root: 0, kv_cache_size: 0, kv_cache_max: 4096,
            string_dict_size: 0, validation_issues: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"total_entries\":1"));
    }

    #[test]
    fn site_map_summary_serializes() {
        let summary = SiteMapSummary {
            stats: "test".into(), root: "0000".into(), weight_root: "abcd".into(),
            cache_utilization: "0.0%".into(), validation_clean: true,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"validation_clean\":true"));
    }

    #[test]
    fn store_operation_report_serializes() {
        let report = StoreOperationReport {
            operation: "put_kv".into(), elapsed_us: 100, entries_affected: 1, total_entries_after: 5,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"operation\":\"put_kv\""));
    }

    #[test]
    fn triple_analysis_serializes() {
        let analysis = TripleAnalysis {
            total_triples: 10, unique_predicates: 2,
            predicate_distribution: vec![PredicateCount { predicate_id: 1, count: 7 }],
            unique_subjects: 5, unique_objects: 3,
        };
        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("\"total_triples\":10"));
    }

    // ── Batch operations ────────────────────────────────────────────────────

    #[test]
    fn put_kv_batch_inserts() {
        let dir = temp_dir("batch1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let items = vec![
            (1, 0, test_ndavec(&[1]), test_ndavec(&[2])),
            (2, 0, test_ndavec(&[3]), test_ndavec(&[4])),
        ];
        let keys = sm.put_kv_batch(&items).unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(sm.len(), 2);
        cleanup(&dir);
    }

    #[test]
    fn put_nodes_batch_inserts() {
        let dir = temp_dir("batch2");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let nodes = vec![
            NdaNode::Int { value: 1 },
            NdaNode::Int { value: 2 },
        ];
        let node_refs: Vec<&NdaNode> = nodes.iter().collect();
        let keys = sm.put_nodes_batch(&node_refs).unwrap();
        assert_eq!(keys.len(), 2);
        cleanup(&dir);
    }

    #[test]
    fn register_strings_batch() {
        let dir = temp_dir("batch3");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let hashes = sm.register_strings_batch(&["hello", "world"]).unwrap();
        assert_eq!(hashes.len(), 2);
        assert_ne!(hashes[0], hashes[1]);
        cleanup(&dir);
    }

    #[test]
    fn resolve_strings_batch() {
        let dir = temp_dir("batch4");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let hashes = sm.register_strings_batch(&["hello", "world"]).unwrap();
        let resolved = sm.resolve_strings_batch(&hashes);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0], Some("hello".to_string()));
        assert_eq!(resolved[1], Some("world".to_string()));
        cleanup(&dir);
    }

    // ── get_any_node_hash ───────────────────────────────────────────────────

    #[test]
    fn get_any_node_hash_empty() {
        let dir = temp_dir("anyhash1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        assert!(sm.get_any_node_hash().is_none());
        cleanup(&dir);
    }

    #[test]
    fn get_any_node_hash_after_put() {
        let dir = temp_dir("anyhash2");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        sm.put_node(&NdaNode::Int { value: 1 }).unwrap();
        assert!(sm.get_any_node_hash().is_some());
        cleanup(&dir);
    }

    // ── predicate index ─────────────────────────────────────────────────────

    #[test]
    fn invalidate_predicate_index() {
        let dir = temp_dir("predinv1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        sm.invalidate_predicate_index();
        let idx = sm.predicate_index.lock_safe();
        assert!(idx.is_none());
        cleanup(&dir);
    }

    // ── Reported operations ─────────────────────────────────────────────────

    #[test]
    fn put_kv_reported_works() {
        let dir = temp_dir("reported1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let (key, report) = sm.put_kv_reported(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
        assert_ne!(key, 0);
        assert_eq!(report.operation, "put_kv");
        assert_eq!(report.entries_affected, 1);
        assert!(report.elapsed_us >= 1);
        cleanup(&dir);
    }

    #[test]
    fn put_node_reported_works() {
        let dir = temp_dir("reported2");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let node = NdaNode::Int { value: 42 };
        let (key, report) = sm.put_node_reported(&node).unwrap();
        assert_ne!(key, 0);
        assert_eq!(report.operation, "put_node");
        assert_eq!(report.entries_affected, 1);
        cleanup(&dir);
    }

    #[test]
    fn flush_report_works() {
        let dir = temp_dir("reported3");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let report = sm.flush_report().unwrap();
        assert_eq!(report.operation, "flush");
        assert!(report.elapsed_us >= 1);
        cleanup(&dir);
    }

    // ── Persistence round-trip ──────────────────────────────────────────────

    #[test]
    fn flush_and_reopen() {
        let dir = temp_dir("persist1");
        {
            let mut sm = SiteMap::open(&dir, 0xABCD).unwrap();
            sm.put_kv(1, 0, test_ndavec(&[1]), test_ndavec(&[2])).unwrap();
            sm.flush().unwrap();
        }
        let sm2 = SiteMap::open(&dir, 0xABCD).unwrap();
        assert_eq!(sm2.len(), 1);
        assert_eq!(sm2.weight_root(), 0xABCD);
        cleanup(&dir);
    }

    // ── put_file_snapshot / remove_file_snapshot ────────────────────────────

    #[test]
    fn put_and_remove_file_snapshot() {
        let dir = temp_dir("snapshot1");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let triples = vec![VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 3 }];
        let key = sm.put_file_snapshot("test.rs", &triples).unwrap();
        assert_ne!(key, 0);
        assert_eq!(sm.len(), 1);
        let removed = sm.remove_file_snapshot("test.rs").unwrap();
        assert!(removed);
        assert_eq!(sm.len(), 0);
        cleanup(&dir);
    }

    #[test]
    fn remove_nonexistent_snapshot() {
        let dir = temp_dir("snapshot2");
        let mut sm = SiteMap::open(&dir, 0).unwrap();
        let removed = sm.remove_file_snapshot("nonexistent.rs").unwrap();
        assert!(!removed);
        cleanup(&dir);
    }

    // ── triple_analysis ─────────────────────────────────────────────────────

    #[test]
    fn triple_analysis_empty() {
        let dir = temp_dir("analysis1");
        let sm = SiteMap::open(&dir, 0).unwrap();
        let analysis = sm.triple_analysis();
        assert_eq!(analysis.total_triples, 0);
        assert_eq!(analysis.unique_predicates, 0);
        cleanup(&dir);
    }
}
