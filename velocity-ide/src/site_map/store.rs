use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::nda_int::NdaVec;

use super::kv::KvRecord;
use super::serialization::{deserialise_node, serialise_node};
use super::types::{EntryKind, SiteMapEntry, SiteMapStats, VcTriple};
use super::verifier::NdaNode;

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
        let header = lines.find(|line| !line.trim().is_empty()).map(str::trim).unwrap_or("");

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
        self.node_triples_cache.lock().unwrap().insert(key, triples);

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
        self.filter_triples(self.collect_historical_triples(), subject, predicate, object)
    }

    pub fn find_live_triples(
        &self,
        subject: Option<u64>,
        predicate: Option<u16>,
        object: Option<u64>,
    ) -> Vec<VcTriple> {
        self.filter_triples(self.collect_live_snapshot_triples(), subject, predicate, object)
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
            self.snapshot_triples_cache.lock().unwrap().remove(&key);
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
                .fold((0usize, 0usize, 0usize, 0usize), |(k, n, p, s), e| match e.kind {
                    EntryKind::Kv => (k + 1, n, p, s),
                    EntryKind::Node => (k, n + 1, p, s),
                    EntryKind::Program => (k, n, p + 1, s),
                    EntryKind::Snapshot => (k, n, p, s + 1),
                });
        let total_bytes: u64 = self.index.values().map(|e| e.size).sum();
        SiteMapStats {
            kv,
            nodes,
            programs,
            snapshots,
            total_bytes,
            root: self.root,
            weight_root: self.weight_root,
        }
    }

    pub fn register_string(&self, s: &str) -> Result<u64> {
        let hash = self.hash_string(s);
        let mut dict = self.string_dict.lock().unwrap();

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

        if !dict.contains_key(&hash) {
            dict.insert(hash, s.to_string());
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
        let mut dict = self.string_dict.lock().unwrap();

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

    // ── Internal helpers ──────────────────────────────────────────────────────

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
        let mut cache = self.node_triples_cache.lock().unwrap();

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
        let mut cache = self.snapshot_triples_cache.lock().unwrap();

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
                    if t.subject_hash != s { return false; }
                }
                if let Some(p) = predicate {
                    if t.predicate_id != p { return false; }
                }
                if let Some(o) = object {
                    if t.object_hash != o { return false; }
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
            let idx = self.predicate_index.lock().unwrap();
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
            index.entry(triple.predicate_id).or_default().push(triple.clone());
        }
        let result = index.get(&predicate_id).cloned().unwrap_or_default();
        *self.predicate_index.lock().unwrap() = Some(index);
        result
    }

    /// Invalidate the predicate index (call when snapshots are updated).
    pub fn invalidate_predicate_index(&self) {
        *self.predicate_index.lock().unwrap() = None;
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
