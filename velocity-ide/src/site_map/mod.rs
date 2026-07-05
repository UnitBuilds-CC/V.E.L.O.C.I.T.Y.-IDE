// site_map/mod.rs — Persistent content-addressed KV store for NDA-Zero inference
#![allow(dead_code)]
//
// REPLACES the session-scoped KV cache entirely.
//
// Why this works with ALiBi (and not RoPE):
//   ALiBi applies the position bias at attention-score time, not at K/V
//   computation time.  Therefore K and V are pure functions of (token_id,
//   model_weights) — the same token at any position always produces the
//   same K and V vectors.  We can address them by content hash rather than
//   by position, making them permanently cacheable across sessions.
//
// Layout:
//   site_map/
//     index.json          ← hot index: hash → entry metadata
//     kv/{hash16}.kv      ← K,V bitmaps for one token (NDA-packed)
//     nodes/{hash16}.nda  ← serialised NdaNode (program fragments)
//     programs/{hash16}.nda ← complete programs (root node reference)
//
// All hashes are SHA-256 truncated to u64 (first 8 bytes, hex-encoded as
// 16-char filenames).  The index.json Merkle root is the SHA-256 of all
// entry hashes sorted lexicographically, also truncated to u64.

pub mod verifier;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::nda_int::NdaVec;
#[allow(unused_imports)]
pub use verifier::{MerkleVerifier, NdaNode, NdaOpcode, CmpOp, VecOpKind, BitwiseOp, MathOp, MathFuncKind, AtomicOp, TypeKind};

// ─── Entry types ──────────────────────────────────────────────────────────────

/// Metadata record stored in index.json for one site-map entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SiteMapEntry {
    pub kind:      EntryKind,
    pub hash:      u64,
    /// Path relative to site_map root.
    pub file:      String,
    /// SHA-256 of the raw file bytes — second integrity check on top of Merkle.
    pub file_sha:  String,
    /// Size in bytes.
    pub size:      u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntryKind {
    Kv,       // token K/V pair
    Node,     // NDA program node
    Program,  // complete NDA program (root reference)
}

// ─── On-disk KV record ─────────────────────────────────────────────────────────

/// Binary layout of a `.kv` file (little-endian):
///
///   [0..2]   len:        u16   — vector length (number of elements)
///   [2]      log2_scale: i8    — shared scale for K and V
///   [3]      reserved:   u8
///   [4..]    k_sign:     ceil(len/8) bytes
///   [..]     k_extra:    ceil(len/8) bytes
///   [..]     v_sign:     ceil(len/8) bytes
///   [..]     v_extra:    ceil(len/8) bytes
///
/// Total: 4 + 4 × ceil(len/8) bytes.  For hidden=896: 4 + 448 = 452 bytes.
struct KvRecord {
    k: NdaVec,
    v: NdaVec,
}

impl KvRecord {
    fn serialise(&self) -> Vec<u8> {
        let len = self.k.len as u16;
        let mut buf = Vec::with_capacity(4 + 4 * ((self.k.len + 7) / 8));
        buf.extend_from_slice(&len.to_le_bytes());
        buf.push(self.k.log2_scale as u8);
        buf.push(0u8); // reserved
        buf.extend_from_slice(&self.k.sign);
        buf.extend_from_slice(&self.k.extra);
        buf.extend_from_slice(&self.v.sign);
        buf.extend_from_slice(&self.v.extra);
        buf
    }

    fn deserialise(data: &[u8]) -> Result<Self> {
        anyhow::ensure!(data.len() >= 4, "KV record too short");
        let len        = u16::from_le_bytes([data[0], data[1]]) as usize;
        let log2_scale = data[2] as i8;
        let bitmap_bytes = (len + 7) / 8;
        anyhow::ensure!(
            data.len() >= 4 + 4 * bitmap_bytes,
            "KV record truncated (len={len}, expected {} bytes)", 4 + 4 * bitmap_bytes
        );
        let base = 4;
        let k = NdaVec {
            len,
            log2_scale,
            sign:  data[base .. base + bitmap_bytes].to_vec().into(),
            extra: data[base + bitmap_bytes .. base + 2 * bitmap_bytes].to_vec().into(),
        };
        let v = NdaVec {
            len,
            log2_scale,
            sign:  data[base + 2 * bitmap_bytes .. base + 3 * bitmap_bytes].to_vec().into(),
            extra: data[base + 3 * bitmap_bytes .. base + 4 * bitmap_bytes].to_vec().into(),
        };
        Ok(KvRecord { k, v })
    }
}

// ─── SiteMap ──────────────────────────────────────────────────────────────────

/// Persistent, content-addressed KV store.
///
/// Hot-path reads go through the in-RAM `index` HashMap (O(1) hash lookup).
/// Cold-path reads (after a restart) load from disk on first access and warm
/// the RAM index.  Writes are write-through: RAM index updated immediately,
/// file written synchronously (async writes can be added later with a
/// background flush thread).
pub struct SiteMap {
    /// Root directory on disk.
    base:         PathBuf,
    /// Hot in-RAM index: hash → entry metadata.
    pub index:    HashMap<u64, SiteMapEntry>,
    /// Merkle root of the index (hash of all entry hashes, sorted).
    root:         u64,
    /// Blake3-style root of the model weight files — embedded in token hashes
    /// so that weight updates automatically invalidate stale KV entries.
    weight_root:  u64,
    /// In-RAM KV cache (evict-never for now; add LRU if RAM pressure matters).
    kv_cache:     HashMap<u64, (NdaVec, NdaVec)>,
}

impl SiteMap {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Open (or create) a site map at `base_dir`.
    /// `weight_root` should be the hash of all model `.nda` file sizes+mtimes —
    /// call `Self::hash_weight_dir` to compute it.
    pub fn open(base_dir: &Path, weight_root: u64) -> Result<Self> {
        fs::create_dir_all(base_dir.join("kv"))?;
        fs::create_dir_all(base_dir.join("nodes"))?;
        fs::create_dir_all(base_dir.join("programs"))?;

        let index_path = base_dir.join("index.json");
        let (index, root) = if index_path.exists() {
            let raw = fs::read_to_string(&index_path)
                .context("reading site_map/index.json")?;
            let entries: Vec<SiteMapEntry> = serde_json::from_str(&raw)
                .context("parsing site_map/index.json")?;
            let root = Self::compute_index_root(&entries);
            let map  = entries.into_iter().map(|e| (e.hash, e)).collect();
            (map, root)
        } else {
            (HashMap::new(), 0u64)
        };

        Ok(Self {
            base: base_dir.to_path_buf(),
            index,
            root,
            weight_root,
            kv_cache: HashMap::new(),
        })
    }

    /// Compute a stable hash of all `.nda` weight files in `weight_dir`.
    /// Used as `weight_root` so that token hashes automatically change when
    /// model weights are updated (invalidating stale site-map KV entries).
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
                // mtime as nanos — changes if file is re-written
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

    // ── KV access (hot path) ──────────────────────────────────────────────────

    /// Derive the site-map hash key for a token's K/V pair.
    /// Key = SHA-256(token_id_bytes ++ layer_idx_bytes ++ weight_root_bytes)[..8].
    pub fn token_hash(&self, token_id: u32, layer_idx: u32) -> u64 {
        let mut h = Sha256::new();
        h.update(b"kv");
        h.update(token_id.to_le_bytes());
        h.update(layer_idx.to_le_bytes());
        h.update(self.weight_root.to_le_bytes());
        let d = h.finalize();
        u64::from_le_bytes(d[..8].try_into().unwrap())
    }

    /// Look up K and V for `token_id` and `layer_idx`.  Returns `None` on a cold miss
    /// (token not yet in the site map).  On a warm hit, returns a reference
    /// to the in-RAM cached NdaVecs — zero allocation, zero disk I/O.
    pub fn get_kv(&mut self, token_id: u32, layer_idx: u32) -> Option<(&NdaVec, &NdaVec)> {
        let key = self.token_hash(token_id, layer_idx);

        // Fast path: already in RAM.
        if self.kv_cache.contains_key(&key) {
            let (k, v) = self.kv_cache.get(&key).unwrap();
            // Re-borrow to satisfy the borrow checker.
            return Some((
                unsafe { &*(k as *const NdaVec) },
                unsafe { &*(v as *const NdaVec) },
            ));
        }

        // Slow path: try disk.
        let entry = self.index.get(&key)?;
        let path  = self.base.join(&entry.file);
        let data  = fs::read(&path).ok()?;
        // Verify file integrity before trusting it.
        if !Self::verify_file_sha(&data, &entry.file_sha) {
            eprintln!("[site_map] integrity check failed for {}", path.display());
            return None;
        }
        let rec = KvRecord::deserialise(&data).ok()?;
        self.kv_cache.insert(key, (rec.k, rec.v));
        let (k, v) = self.kv_cache.get(&key).unwrap();
        Some((
            unsafe { &*(k as *const NdaVec) },
            unsafe { &*(v as *const NdaVec) },
        ))
    }

    /// Store K and V for `token_id` and `layer_idx`.  Write-through: RAM + disk updated
    /// immediately.  Idempotent — re-storing the same token is a no-op if the
    /// hash already exists in the index.
    pub fn put_kv(&mut self, token_id: u32, layer_idx: u32, k: NdaVec, v: NdaVec) -> Result<u64> {
        let key = self.token_hash(token_id, layer_idx);
        if self.index.contains_key(&key) {
            // Already stored — content-addressed, so it's identical.
            self.kv_cache.entry(key).or_insert_with(|| (k, v));
            return Ok(key);
        }

        let rec  = KvRecord { k: k.clone(), v: v.clone() };
        let data = rec.serialise();
        let sha  = Self::file_sha(&data);
        let name = format!("kv/{:016x}.kv", key);
        let path = self.base.join(&name);
        fs::write(&path, &data)
            .with_context(|| format!("writing {}", path.display()))?;

        let entry = SiteMapEntry {
            kind:     EntryKind::Kv,
            hash:     key,
            file:     name,
            file_sha: sha,
            size:     data.len() as u64,
        };
        self.index.insert(key, entry);
        self.kv_cache.insert(key, (k, v));
        self.recompute_root();
        Ok(key)
    }

    // ── Node access ───────────────────────────────────────────────────────────

    /// Store an NDA program node.  Returns its content hash.
    /// Idempotent — storing an identical node twice is a no-op.
    pub fn put_node(&mut self, node: &NdaNode) -> Result<u64> {
        let key  = node.hash();
        if self.index.contains_key(&key) {
            return Ok(key);
        }
        let data = Self::serialise_node(node);
        let sha  = Self::file_sha(&data);
        let name = format!("nodes/{:016x}.nda", key);
        let path = self.base.join(&name);
        fs::write(&path, &data)
            .with_context(|| format!("writing {}", path.display()))?;

        let entry = SiteMapEntry {
            kind:     EntryKind::Node,
            hash:     key,
            file:     name,
            file_sha: sha,
            size:     data.len() as u64,
        };
        self.index.insert(key, entry);
        self.recompute_root();
        Ok(key)
    }

    /// Retrieve an NDA program node by its hash (or program root hash).
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
            if data.len() < 8 { return None; }
            let root_hash = u64::from_le_bytes(data[..8].try_into().unwrap());
            return self.get_node(root_hash);
        }
        let mut offset = 0;
        Self::deserialise_node(&data, &mut offset).ok()
    }

    /// Return the hash of any stored node (used for call target placeholders).
    pub fn get_any_node_hash(&self) -> Option<u64> {
        self.index.keys().next().copied()
    }


    fn deserialise_node(data: &[u8], offset: &mut usize) -> Result<NdaNode> {
        if *offset >= data.len() {
            anyhow::bail!("EOF");
        }
        let tag = data[*offset];
        *offset += 1;
        match tag {
            b'M' => {
                if *offset + 5 > data.len() { anyhow::bail!("Truncated Matrix"); }
                let rows = u16::from_le_bytes(data[*offset..*offset+2].try_into().unwrap());
                let cols = u16::from_le_bytes(data[*offset+2..*offset+4].try_into().unwrap());
                let scale = data[*offset+4] as i8;
                *offset += 5;
                let bitmap_bytes = rows as usize * ((cols as usize + 7) / 8);
                if *offset + 2 * bitmap_bytes > data.len() { anyhow::bail!("Truncated Matrix bitmaps"); }
                let sign = data[*offset..*offset+bitmap_bytes].to_vec();
                *offset += bitmap_bytes;
                let extra = data[*offset..*offset+bitmap_bytes].to_vec();
                *offset += bitmap_bytes;
                Ok(NdaNode::Matrix { rows, cols, scale, sign, extra })
            }
            b'N' => {
                if *offset + 2 > data.len() { anyhow::bail!("Truncated Norm"); }
                let size = u16::from_le_bytes(data[*offset..*offset+2].try_into().unwrap());
                *offset += 2;
                let bitmap_bytes = (size as usize + 7) / 8;
                if *offset + 2 * bitmap_bytes > data.len() { anyhow::bail!("Truncated Norm bitmaps"); }
                let weight = data[*offset..*offset+bitmap_bytes].to_vec();
                *offset += bitmap_bytes;
                let bias = data[*offset..*offset+bitmap_bytes].to_vec();
                *offset += bitmap_bytes;
                Ok(NdaNode::Norm { size, weight, bias })
            }
            b'C' => {
                if *offset < data.len() && data[*offset] == b'M' {
                    *offset += 1;
                    if *offset >= data.len() || data[*offset] != b'P' { anyhow::bail!("Invalid Compare tag"); }
                    *offset += 1;
                    if *offset >= data.len() { anyhow::bail!("Truncated Compare op"); }
                    let op_val = data[*offset];
                    *offset += 1;
                    let op = CmpOp::from_u8(op_val).ok_or_else(|| anyhow::anyhow!("Invalid CmpOp"))?;
                    let lhs = Self::deserialise_node(data, offset)?;
                    let rhs = Self::deserialise_node(data, offset)?;
                    Ok(NdaNode::Compare { op, lhs: Box::new(lhs), rhs: Box::new(rhs) })
                } else if *offset < data.len() && data[*offset] == b'S' {
                    *offset += 1;
                    if *offset + 2 > data.len() { anyhow::bail!("Truncated Cast types"); }
                    let from_val = data[*offset];
                    let to_val = data[*offset+1];
                    *offset += 2;
                    let from_type = TypeKind::from_u8(from_val).ok_or_else(|| anyhow::anyhow!("Invalid from_type"))?;
                    let to_type = TypeKind::from_u8(to_val).ok_or_else(|| anyhow::anyhow!("Invalid to_type"))?;
                    let operand = Self::deserialise_node(data, offset)?;
                    Ok(NdaNode::Cast { from_type, to_type, operand: Box::new(operand) })
                } else {
                    if *offset + 8 > data.len() { anyhow::bail!("Truncated Call"); }
                    let target = u64::from_le_bytes(data[*offset..*offset+8].try_into().unwrap());
                    *offset += 8;
                    Ok(NdaNode::Call { target })
                }
            }
            b'I' => {
                if *offset < data.len() && data[*offset] == b'F' {
                    *offset += 1;
                    let cond = Self::deserialise_node(data, offset)?;
                    if *offset + 4 > data.len() { anyhow::bail!("Truncated If then len"); }
                    let then_len = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as usize;
                    *offset += 4;
                    let mut then_body = Vec::with_capacity(then_len);
                    for _ in 0..then_len {
                        then_body.push(Self::deserialise_node(data, offset)?);
                    }
                    let mut else_body = None;
                    if *offset < data.len() {
                        let has_else = data[*offset];
                        if has_else == 1 {
                            *offset += 1;
                            if *offset + 4 > data.len() { anyhow::bail!("Truncated If else len"); }
                            let else_len = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as usize;
                            *offset += 4;
                            let mut eb = Vec::with_capacity(else_len);
                            for _ in 0..else_len {
                                eb.push(Self::deserialise_node(data, offset)?);
                            }
                            else_body = Some(eb);
                        } else if has_else == 0 {
                            *offset += 1;
                        }
                    }
                    Ok(NdaNode::If { cond: Box::new(cond), then_body, else_body })
                } else {
                    if *offset + 4 > data.len() { anyhow::bail!("Truncated Int"); }
                    let value = i32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap());
                    *offset += 4;
                    Ok(NdaNode::Int { value })
                }
            }
            b'S' => {
                if *offset < data.len() && data[*offset] == b'T' {
                    *offset += 1;
                    if *offset + 8 > data.len() { anyhow::bail!("Truncated Store name_hash"); }
                    let name_hash = u64::from_le_bytes(data[*offset..*offset+8].try_into().unwrap());
                    *offset += 8;
                    let value = Self::deserialise_node(data, offset)?;
                    Ok(NdaNode::Store { name_hash, value: Box::new(value) })
                } else {
                    if *offset + 4 > data.len() { anyhow::bail!("Truncated Scope"); }
                    let len = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as usize;
                    *offset += 4;
                    let mut children = Vec::with_capacity(len);
                    for _ in 0..len {
                        children.push(Self::deserialise_node(data, offset)?);
                    }
                    Ok(NdaNode::Scope { children })
                }
            }
            b'L' => {
                if *offset >= data.len() { anyhow::bail!("Truncated L tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'P' => {
                        if *offset + 8 > data.len() { anyhow::bail!("Truncated Loop count/len"); }
                        let count = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap());
                        let len = u32::from_le_bytes(data[*offset+4..*offset+8].try_into().unwrap()) as usize;
                        *offset += 8;
                        let mut body = Vec::with_capacity(len);
                        for _ in 0..len {
                            body.push(Self::deserialise_node(data, offset)?);
                        }
                        Ok(NdaNode::Loop { count, body })
                    }
                    b'T' => {
                        if *offset + 8 > data.len() { anyhow::bail!("Truncated Let name_hash"); }
                        let name_hash = u64::from_le_bytes(data[*offset..*offset+8].try_into().unwrap());
                        *offset += 8;
                        let init = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Let { name_hash, init: Box::new(init) })
                    }
                    b'D' => {
                        if *offset + 8 > data.len() { anyhow::bail!("Truncated Load name_hash"); }
                        let name_hash = u64::from_le_bytes(data[*offset..*offset+8].try_into().unwrap());
                        *offset += 8;
                        Ok(NdaNode::Load { name_hash })
                    }
                    _ => anyhow::bail!("Unknown subtag L{}", sub),
                }
            }
            b'W' => {
                if *offset >= data.len() || data[*offset] != b'H' { anyhow::bail!("Invalid While tag"); }
                *offset += 1;
                let cond = Self::deserialise_node(data, offset)?;
                if *offset + 4 > data.len() { anyhow::bail!("Truncated While body len"); }
                let len = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as usize;
                *offset += 4;
                let mut body = Vec::with_capacity(len);
                for _ in 0..len {
                    body.push(Self::deserialise_node(data, offset)?);
                }
                Ok(NdaNode::While { cond: Box::new(cond), body })
            }
            b'B' => {
                if *offset >= data.len() { anyhow::bail!("Truncated B tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'K' => Ok(NdaNode::Break),
                    b'W' => {
                        if *offset + 1 > data.len() { anyhow::bail!("Truncated Bitwise op"); }
                        let op_val = data[*offset];
                        *offset += 1;
                        let op = BitwiseOp::from_u8(op_val).ok_or_else(|| anyhow::anyhow!("Invalid BitwiseOp"))?;
                        let lhs = Self::deserialise_node(data, offset)?;
                        if *offset >= data.len() { anyhow::bail!("Truncated Bitwise has_rhs"); }
                        let has_rhs = data[*offset];
                        *offset += 1;
                        let rhs = if has_rhs == 1 {
                            Some(Box::new(Self::deserialise_node(data, offset)?))
                        } else {
                            None
                        };
                        Ok(NdaNode::Bitwise { op, lhs: Box::new(lhs), rhs })
                    }
                    _ => anyhow::bail!("Unknown subtag B{}", sub),
                }
            }
            b'F' => {
                if *offset >= data.len() { anyhow::bail!("Truncated F tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'L' => {
                        if *offset + 4 > data.len() { anyhow::bail!("Truncated Float"); }
                        let value = f32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap());
                        *offset += 4;
                        Ok(NdaNode::Float { value })
                    }
                    b'R' => {
                        let addr = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Free { addr: Box::new(addr) })
                    }
                    _ => anyhow::bail!("Unknown subtag F{}", sub),
                }
            }
            b'G' => {
                if *offset >= data.len() { anyhow::bail!("Truncated G tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'M' => {
                        let matrix = Self::deserialise_node(data, offset)?;
                        let vector = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Gemv { matrix: Box::new(matrix), vector: Box::new(vector) })
                    }
                    b'D' => {
                        if *offset + 12 > data.len() { anyhow::bail!("Truncated GpuDispatch"); }
                        let shader_hash = u64::from_le_bytes(data[*offset..*offset+8].try_into().unwrap());
                        let len = u32::from_le_bytes(data[*offset+8..*offset+12].try_into().unwrap()) as usize;
                        *offset += 12;
                        let mut args = Vec::with_capacity(len);
                        for _ in 0..len {
                            args.push(Self::deserialise_node(data, offset)?);
                        }
                        Ok(NdaNode::GpuDispatch { shader_hash, args })
                    }
                    _ => anyhow::bail!("Unknown subtag G{}", sub),
                }
            }
            b'D' => {
                if *offset >= data.len() || data[*offset] != b'T' { anyhow::bail!("Invalid Dot tag"); }
                *offset += 1;
                let lhs = Self::deserialise_node(data, offset)?;
                let rhs = Self::deserialise_node(data, offset)?;
                Ok(NdaNode::Dot { lhs: Box::new(lhs), rhs: Box::new(rhs) })
            }
            b'A' => {
                if *offset >= data.len() { anyhow::bail!("Truncated A tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'D' => {
                        let lhs = Self::deserialise_node(data, offset)?;
                        let rhs = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Add { lhs: Box::new(lhs), rhs: Box::new(rhs) })
                    }
                    b'T' => {
                        if *offset + 1 > data.len() { anyhow::bail!("Truncated Atomic op"); }
                        let op_val = data[*offset];
                        *offset += 1;
                        let op = AtomicOp::from_u8(op_val).ok_or_else(|| anyhow::anyhow!("Invalid AtomicOp"))?;
                        let addr = Self::deserialise_node(data, offset)?;
                        let val = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Atomic { op, addr: Box::new(addr), val: Box::new(val) })
                    }
                    b'L' => {
                        let size = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Alloc { size: Box::new(size) })
                    }
                    _ => anyhow::bail!("Unknown subtag A{}", sub),
                }
            }
            b'V' => {
                if *offset >= data.len() || data[*offset] != b'O' { anyhow::bail!("Invalid VecOp tag"); }
                *offset += 1;
                if *offset >= data.len() { anyhow::bail!("Truncated VecOp op"); }
                let op_val = data[*offset];
                *offset += 1;
                let op = VecOpKind::from_u8(op_val).ok_or_else(|| anyhow::anyhow!("Invalid VecOpKind"))?;
                let operand = Self::deserialise_node(data, offset)?;
                Ok(NdaNode::VecOp { op, operand: Box::new(operand) })
            }
            b'P' => {
                if *offset >= data.len() { anyhow::bail!("Truncated P tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'R' => {
                        let source = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Print { source: Box::new(source) })
                    }
                    b'K' => {
                        let addr = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Peek { addr: Box::new(addr) })
                    }
                    b'O' => {
                        let addr = Self::deserialise_node(data, offset)?;
                        let value = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Poke { addr: Box::new(addr), value: Box::new(value) })
                    }
                    _ => anyhow::bail!("Unknown subtag P{}", sub),
                }
            }
            b'R' => {
                if *offset >= data.len() { anyhow::bail!("Truncated R tag"); }
                let sub = data[*offset];
                *offset += 1;
                match sub {
                    b'T' => {
                        let value = Self::deserialise_node(data, offset)?;
                        Ok(NdaNode::Return { value: Box::new(value) })
                    }
                    b'I' => {
                        if *offset + 12 > data.len() { anyhow::bail!("Truncated RegInt"); }
                        let vector = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap());
                        let handler_hash = u64::from_le_bytes(data[*offset+4..*offset+12].try_into().unwrap());
                        *offset += 12;
                        Ok(NdaNode::RegInt { vector, handler_hash })
                    }
                    _ => anyhow::bail!("Unknown subtag R{}", sub),
                }
            }
            _ => anyhow::bail!("Unknown tag {}", tag),
        }
    }


    /// Store a complete NDA program (by its root node hash).
    pub fn put_program(&mut self, root_node: &NdaNode) -> Result<u64> {
        let node_hash = self.put_node(root_node)?;
        let key  = self.program_hash(node_hash);
        if self.index.contains_key(&key) {
            return Ok(key);
        }
        // Program file is a 8-byte root node hash reference.
        let data = node_hash.to_le_bytes().to_vec();
        let sha  = Self::file_sha(&data);
        let name = format!("programs/{:016x}.nda", key);
        let path = self.base.join(&name);
        fs::write(&path, &data)?;

        let entry = SiteMapEntry {
            kind:     EntryKind::Program,
            hash:     key,
            file:     name,
            file_sha: sha,
            size:     8,
        };
        self.index.insert(key, entry);
        self.recompute_root();
        Ok(key)
    }

    // ── Persistence ───────────────────────────────────────────────────────────

    /// Flush the in-RAM index to `index.json`.  Call periodically or on shutdown.
    pub fn flush(&self) -> Result<()> {
        let entries: Vec<&SiteMapEntry> = self.index.values().collect();
        let json = serde_json::to_string_pretty(&entries)?;
        fs::write(self.base.join("index.json"), json)?;
        Ok(())
    }

    /// Verify every entry in the index: check that the file exists and its
    /// SHA matches.  Returns the number of corrupt entries found.
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

    /// Current Merkle root of the site map index.
    pub fn root(&self) -> u64 { self.root }

    /// Number of entries in the site map.
    pub fn len(&self) -> usize { self.index.len() }

    pub fn is_empty(&self) -> bool { self.index.is_empty() }

    // ── Statistics ────────────────────────────────────────────────────────────

    pub fn stats(&self) -> SiteMapStats {
        let (kv, nodes, programs) = self.index.values().fold(
            (0usize, 0usize, 0usize),
            |(k, n, p), e| match e.kind {
                EntryKind::Kv      => (k + 1, n, p),
                EntryKind::Node    => (k, n + 1, p),
                EntryKind::Program => (k, n, p + 1),
            },
        );
        let total_bytes: u64 = self.index.values().map(|e| e.size).sum();
        SiteMapStats { kv, nodes, programs, total_bytes, root: self.root }
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

    fn file_sha(data: &[u8]) -> String {
        let d = Sha256::digest(data);
        format!("{:x}", d)
    }

    fn verify_file_sha(data: &[u8], expected_hex: &str) -> bool {
        let d = Sha256::digest(data);
        format!("{:x}", d) == expected_hex
    }

    /// Minimal NDA node serialisation for disk storage.
    /// Format: 1-byte opcode tag + payload.
    fn serialise_node(node: &NdaNode) -> Vec<u8> {
        let mut buf = Vec::new();
        Self::write_node(node, &mut buf);
        buf
    }

    fn write_node(node: &NdaNode, buf: &mut Vec<u8>) {
        match node {
            NdaNode::Matrix { rows, cols, scale, sign, extra } => {
                buf.push(b'M');
                buf.extend_from_slice(&rows.to_le_bytes());
                buf.extend_from_slice(&cols.to_le_bytes());
                buf.push(*scale as u8);
                buf.extend_from_slice(sign);
                buf.extend_from_slice(extra);
            }
            NdaNode::Norm { size, weight, bias } => {
                buf.push(b'N');
                buf.extend_from_slice(&size.to_le_bytes());
                buf.extend_from_slice(weight);
                buf.extend_from_slice(bias);
            }
            NdaNode::Call { target } => {
                buf.push(b'C');
                buf.extend_from_slice(&target.to_le_bytes());
            }
            NdaNode::Int { value } => {
                buf.push(b'I');
                buf.extend_from_slice(&value.to_le_bytes());
            }
            NdaNode::Scope { children } => {
                buf.push(b'S');
                buf.extend_from_slice(&(children.len() as u32).to_le_bytes());
                for child in children {
                    Self::write_node(child, buf);
                }
            }
            // ── Language extension nodes ──────────────────────────────────
            NdaNode::Loop { count, body } => {
                buf.push(b'L'); buf.push(b'P');
                buf.extend_from_slice(&count.to_le_bytes());
                buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
                for child in body { Self::write_node(child, buf); }
            }
            NdaNode::While { cond, body } => {
                buf.push(b'W'); buf.push(b'H');
                Self::write_node(cond, buf);
                buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
                for child in body { Self::write_node(child, buf); }
            }
            NdaNode::If { cond, then_body, else_body } => {
                buf.push(b'I'); buf.push(b'F');
                Self::write_node(cond, buf);
                buf.extend_from_slice(&(then_body.len() as u32).to_le_bytes());
                for child in then_body { Self::write_node(child, buf); }
                if let Some(eb) = else_body {
                    buf.push(1u8); // has_else marker
                    buf.extend_from_slice(&(eb.len() as u32).to_le_bytes());
                    for child in eb { Self::write_node(child, buf); }
                } else {
                    buf.push(0u8); // no else
                }
            }
            NdaNode::Compare { op, lhs, rhs } => {
                buf.push(b'C'); buf.push(b'M'); buf.push(b'P');
                buf.push(*op as u8);
                Self::write_node(lhs, buf);
                Self::write_node(rhs, buf);
            }
            NdaNode::Break => {
                buf.push(b'B'); buf.push(b'K');
            }
            NdaNode::Let { name_hash, init } => {
                buf.push(b'L'); buf.push(b'T');
                buf.extend_from_slice(&name_hash.to_le_bytes());
                Self::write_node(init, buf);
            }
            NdaNode::Load { name_hash } => {
                buf.push(b'L'); buf.push(b'D');
                buf.extend_from_slice(&name_hash.to_le_bytes());
            }
            NdaNode::Store { name_hash, value } => {
                buf.push(b'S'); buf.push(b'T');
                buf.extend_from_slice(&name_hash.to_le_bytes());
                Self::write_node(value, buf);
            }
            NdaNode::Add { lhs, rhs } => {
                buf.push(b'A'); buf.push(b'D');
                Self::write_node(lhs, buf);
                Self::write_node(rhs, buf);
            }
            NdaNode::VecOp { op, operand } => {
                buf.push(b'V'); buf.push(b'O');
                buf.push(*op as u8);
                Self::write_node(operand, buf);
            }
            NdaNode::Print { source } => {
                buf.push(b'P'); buf.push(b'R');
                Self::write_node(source, buf);
            }
            NdaNode::Return { value } => {
                buf.push(b'R'); buf.push(b'T');
                Self::write_node(value, buf);
            }
            NdaNode::Bitwise { op, lhs, rhs } => {
                buf.push(b'B'); buf.push(b'W');
                buf.push(*op as u8);
                Self::write_node(lhs, buf);
                if let Some(r) = rhs {
                    buf.push(1u8);
                    Self::write_node(r, buf);
                } else {
                    buf.push(0u8);
                }
            }
            NdaNode::Float { value } => {
                buf.push(b'F'); buf.push(b'L');
                buf.extend_from_slice(&value.to_le_bytes());
            }
            NdaNode::Math { op, lhs, rhs } => {
                buf.push(b'M'); buf.push(b'H');
                buf.push(*op as u8);
                Self::write_node(lhs, buf);
                Self::write_node(rhs, buf);
            }
            NdaNode::MathFunc { func, operand } => {
                buf.push(b'M'); buf.push(b'F');
                buf.push(*func as u8);
                Self::write_node(operand, buf);
            }
            NdaNode::Peek { addr } => {
                buf.push(b'P'); buf.push(b'K');
                Self::write_node(addr, buf);
            }
            NdaNode::Poke { addr, value } => {
                buf.push(b'P'); buf.push(b'O');
                Self::write_node(addr, buf);
                Self::write_node(value, buf);
            }
            NdaNode::Gemv { matrix, vector } => {
                buf.push(b'G'); buf.push(b'M');
                Self::write_node(matrix, buf);
                Self::write_node(vector, buf);
            }
            NdaNode::Dot { lhs, rhs } => {
                buf.push(b'D'); buf.push(b'T');
                Self::write_node(lhs, buf);
                Self::write_node(rhs, buf);
            }
            NdaNode::Syscall { num, args } => {
                buf.push(b'S'); buf.push(b'C');
                buf.extend_from_slice(&num.to_le_bytes());
                buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
                for arg in args {
                    Self::write_node(arg, buf);
                }
            }
            NdaNode::Spawn { scope_hash } => {
                buf.push(b'S'); buf.push(b'W');
                buf.extend_from_slice(&scope_hash.to_le_bytes());
            }
            NdaNode::Atomic { op, addr, val } => {
                buf.push(b'A'); buf.push(b'T');
                buf.push(*op as u8);
                Self::write_node(addr, buf);
                Self::write_node(val, buf);
            }
            NdaNode::Alloc { size } => {
                buf.push(b'A'); buf.push(b'L');
                Self::write_node(size, buf);
            }
            NdaNode::Free { addr } => {
                buf.push(b'F'); buf.push(b'R');
                Self::write_node(addr, buf);
            }
            NdaNode::RegInt { vector, handler_hash } => {
                buf.push(b'R'); buf.push(b'I');
                buf.extend_from_slice(&vector.to_le_bytes());
                buf.extend_from_slice(&handler_hash.to_le_bytes());
            }
            NdaNode::Cast { from_type, to_type, operand } => {
                buf.push(b'C'); buf.push(b'S');
                buf.push(*from_type as u8);
                buf.push(*to_type as u8);
                Self::write_node(operand, buf);
            }
            NdaNode::GpuDispatch { shader_hash, args } => {
                buf.push(b'G'); buf.push(b'D');
                buf.extend_from_slice(&shader_hash.to_le_bytes());
                buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
                for arg in args {
                    Self::write_node(arg, buf);
                }
            }
        }
    }
}

// ─── Stats ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SiteMapStats {
    pub kv:          usize,
    pub nodes:       usize,
    pub programs:    usize,
    pub total_bytes: u64,
    pub root:        u64,
}

impl std::fmt::Display for SiteMapStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SiteMap: {} KV entries, {} nodes, {} programs | {:.1} KB on disk | root={:016x}",
            self.kv, self.nodes, self.programs,
            self.total_bytes as f64 / 1024.0,
            self.root
        )
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ndavec(len: usize, val: u8) -> NdaVec {
        let bytes = (len + 7) / 8;
        NdaVec {
            len,
            log2_scale: 0,
            sign:  vec![val; bytes].into(),
            extra: vec![val; bytes].into(),
        }
    }

    #[test]
    fn round_trip_kv() {
        let dir = TempDir::new().unwrap();
        let mut sm = SiteMap::open(dir.path(), 0xDEAD).unwrap();
        let k = make_ndavec(16, 0xAA);
        let v = make_ndavec(16, 0x55);
        let hash = sm.put_kv(42, 0, k.clone(), v.clone()).unwrap();
        let (kr, vr) = sm.get_kv(42, 0).unwrap();
        assert_eq!(kr.sign, k.sign);
        assert_eq!(vr.sign, v.sign);
        // Idempotent second write.
        let hash2 = sm.put_kv(42, 0, k, v).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn put_node_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let mut sm = SiteMap::open(dir.path(), 0).unwrap();
        let n = NdaNode::Int { value: 99 };
        let h1 = sm.put_node(&n).unwrap();
        let h2 = sm.put_node(&n).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(sm.len(), 1);
    }

    #[test]
    fn flush_and_reload() {
        let dir = TempDir::new().unwrap();
        {
            let mut sm = SiteMap::open(dir.path(), 0).unwrap();
            let k = make_ndavec(8, 0xFF);
            let v = make_ndavec(8, 0x00);
            sm.put_kv(7, 0, k, v).unwrap();
            sm.flush().unwrap();
        }
        // Reload from disk.
        let mut sm2 = SiteMap::open(dir.path(), 0).unwrap();
        assert_eq!(sm2.len(), 1);
        assert!(sm2.get_kv(7, 0).is_some());
    }

    #[test]
    fn verify_detects_corruption() {
        let dir = TempDir::new().unwrap();
        let mut sm = SiteMap::open(dir.path(), 0).unwrap();
        let k = make_ndavec(8, 0xAA);
        let v = make_ndavec(8, 0xBB);
        sm.put_kv(1, 0, k, v).unwrap();
        sm.flush().unwrap();
        // Corrupt the file.
        let entry = sm.index.values().next().unwrap();
        let path  = dir.path().join(&entry.file);
        let mut data = fs::read(&path).unwrap();
        data[4] ^= 0xFF;
        fs::write(&path, &data).unwrap();
        assert_eq!(sm.verify(), 1);
    }

    #[test]
    fn weight_root_change_invalidates_token_hashes() {
        let dir = TempDir::new().unwrap();
        let sm1 = SiteMap::open(dir.path(), 0x0001).unwrap();
        let sm2 = SiteMap::open(dir.path(), 0x0002).unwrap();
        // Same token_id, different weight roots → different hashes.
        assert_ne!(sm1.token_hash(42, 0), sm2.token_hash(42, 0));
    }

    #[test]
    fn stats_display() {
        let dir = TempDir::new().unwrap();
        let sm = SiteMap::open(dir.path(), 0).unwrap();
        let s = sm.stats();
        println!("{s}");
        assert_eq!(s.kv, 0);
    }
}
