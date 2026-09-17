//! Incremental AST update optimization and query cache for the SiteMap.
//!
//! [`SitemapQueryCache`] provides a TTL-based, LRU-evicting cache for expensive
//! SiteMap lookups (triple queries, file hashes, node lookups, reverse deps).
//!
//! [`IncrementalUpdater`] batches dirty-file triple updates so that rapid
//! successive edits can be coalesced into a single flush.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::verifier::NdaNode;

/// Batch of pending triples keyed by file hash.
pub type PendingTripleBatch = (u64, Vec<(u64, u16, u64)>);

// ─── Cache result variants ────────────────────────────────────────────────────

/// A cached query result.
#[derive(Clone, Debug)]
pub enum CacheResult {
    /// Cached triple lookup: `(subject_hash, predicate_id, object_hash)`.
    TriplesForFile(Vec<(u64, u16, u64)>),
    /// Cached file content hash.
    FileHash(u64),
    /// Cached node lookup (may be `None` if the node was absent).
    NodeLookup(Option<NdaNode>),
    /// Cached reverse-dependency list (hashes of nodes that depend on a file).
    ReverseDeps(Vec<u64>),
}

impl CacheResult {
    /// Return a tag discriminant for debugging / stats.
    pub fn kind_str(&self) -> &'static str {
        match self {
            CacheResult::TriplesForFile(_) => "TriplesForFile",
            CacheResult::FileHash(_) => "FileHash",
            CacheResult::NodeLookup(_) => "NodeLookup",
            CacheResult::ReverseDeps(_) => "ReverseDeps",
        }
    }
}

// ─── Cache entry ──────────────────────────────────────────────────────────────

/// One entry in the query cache.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    /// Hash of the query parameters (the cache key).
    pub key: u64,
    /// The cached payload.
    pub result: CacheResult,
    /// When this entry was created.
    pub created: Instant,
    /// Time-to-live in seconds.
    pub ttl_seconds: u64,
    /// How many times this entry has been looked up.
    pub access_count: u64,
}

impl CacheEntry {
    /// Returns `true` if this entry has exceeded its TTL.
    pub fn is_expired(&self) -> bool {
        self.created
            .elapsed()
            .as_secs()
            .saturating_sub(0) // keep API stable
            >= self.ttl_seconds
    }
}

// ─── Cache stats ──────────────────────────────────────────────────────────────

/// Point-in-time statistics about the query cache.
#[derive(Clone, Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    pub evictions: u64,
    pub hit_rate: f64,
}

impl std::fmt::Display for CacheStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "QueryCache: {} entries | hits={} misses={} evictions={} hit_rate={:.1}%",
            self.entries,
            self.hits,
            self.misses,
            self.evictions,
            self.hit_rate * 100.0
        )
    }
}

// ─── SitemapQueryCache ────────────────────────────────────────────────────────

/// Default maximum number of entries before LRU eviction kicks in.
const DEFAULT_MAX_ENTRIES: usize = 256;

/// Default TTL for cache entries (seconds).
const DEFAULT_TTL: u64 = 30;

/// TTL-based, LRU-evicting query cache for SiteMap lookups.
pub struct SitemapQueryCache {
    cache: HashMap<u64, CacheEntry>,
    /// Tracks access order for LRU eviction: front = least-recently used.
    lru_order: Vec<u64>,
    max_entries: usize,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl SitemapQueryCache {
    /// Create a new cache with the given maximum entry count.
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: HashMap::with_capacity(max_entries.min(1024)),
            lru_order: Vec::with_capacity(max_entries.min(1024)),
            max_entries,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// Look up a cached result by key. Returns `None` on miss or expiration.
    ///
    /// On a hit the entry's `access_count` is bumped and it is moved to the
    /// back of the LRU list (most-recently used).
    pub fn get(&mut self, key: u64) -> Option<&CacheResult> {
        // Check existence + expiration.
        let expired = match self.cache.get(&key) {
            Some(entry) if entry.is_expired() => true,
            None => {
                self.misses += 1;
                return None;
            }
            _ => false,
        };

        if expired {
            self.remove_entry(key);
            self.misses += 1;
            return None;
        }

        // Bump access count.
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.access_count += 1;
        }

        // Move to back of LRU (most recently used).
        self.touch_lru(key);

        self.hits += 1;
        self.cache.get(&key).map(|e| &e.result)
    }

    /// Insert a result into the cache, evicting the LRU entry if at capacity.
    pub fn insert(&mut self, key: u64, result: CacheResult, ttl: u64) {
        // If the key already exists, update in place.
        if self.cache.contains_key(&key) {
            self.remove_entry(key);
        }

        // Evict while at capacity.
        while self.cache.len() >= self.max_entries {
            self.evict_lru();
        }

        let entry = CacheEntry {
            key,
            result,
            created: Instant::now(),
            ttl_seconds: ttl,
            access_count: 0,
        };

        self.cache.insert(key, entry);
        self.lru_order.push(key);
    }

    /// Insert with the default TTL (30 s).
    pub fn insert_default(&mut self, key: u64, result: CacheResult) {
        self.insert(key, result, DEFAULT_TTL);
    }

    /// Invalidate all cache entries whose `CacheResult` references `file_hash`.
    ///
    /// This scans all entries and removes any that are tagged with the given
    /// file hash (for `TriplesForFile` and `FileHash` variants we compare the
    /// key directly; callers should use a deterministic key derivation).
    pub fn invalidate_file(&mut self, file_hash: u64) {
        // We use the key itself as the file-hash marker for file-scoped entries.
        self.remove_entry(file_hash);
    }

    /// Invalidate entries by scanning for keys in a provided set.
    pub fn invalidate_files(&mut self, file_hashes: &[u64]) {
        for &fh in file_hashes {
            self.remove_entry(fh);
        }
    }

    /// Clear the entire cache.
    pub fn invalidate_all(&mut self) {
        self.cache.clear();
        self.lru_order.clear();
    }

    /// Return the cache hit rate as a fraction in `[0.0, 1.0]`.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    /// Return a snapshot of cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            entries: self.cache.len(),
            evictions: self.evictions,
            hit_rate: self.hit_rate(),
        }
    }

    /// Remove all expired entries from the cache.
    pub fn prune_expired(&mut self) {
        let expired_keys: Vec<u64> = self
            .cache
            .iter()
            .filter(|(_, e)| e.is_expired())
            .map(|(&k, _)| k)
            .collect();

        for key in expired_keys {
            self.remove_entry(key);
        }
    }

    /// Current number of entries in the cache.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Maximum entry capacity.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Remove an entry from both the map and the LRU list.
    fn remove_entry(&mut self, key: u64) {
        self.cache.remove(&key);
        self.lru_order.retain(|&k| k != key);
    }

    /// Evict the least-recently used entry (front of `lru_order`).
    fn evict_lru(&mut self) {
        if let Some(&victim) = self.lru_order.first() {
            self.lru_order.remove(0);
            self.cache.remove(&victim);
            self.evictions += 1;
        }
    }

    /// Move `key` to the back of `lru_order` (most-recently used).
    fn touch_lru(&mut self, key: u64) {
        self.lru_order.retain(|&k| k != key);
        self.lru_order.push(key);
    }
}

impl Default for SitemapQueryCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES)
    }
}

// ─── IncrementalUpdater ───────────────────────────────────────────────────────

/// Batches dirty-file triple updates so that rapid successive edits can be
/// coalesced into a single flush against the SiteMap.
pub struct IncrementalUpdater {
    /// Files that have pending re-index work, keyed by file hash.
    dirty_files: HashSet<u64>,
    /// Triples queued for each dirty file.
    pending_triples: HashMap<u64, Vec<(u64, u16, u64)>>,
}

impl IncrementalUpdater {
    /// Create a new, empty incremental updater.
    pub fn new() -> Self {
        Self {
            dirty_files: HashSet::new(),
            pending_triples: HashMap::new(),
        }
    }

    /// Mark a file as dirty and queue its new triples for later flushing.
    ///
    /// If the file was already dirty the new triples are **appended** to the
    /// existing pending set (allowing coalescing of rapid edits).
    pub fn mark_dirty(&mut self, file_hash: u64, triples: Vec<(u64, u16, u64)>) {
        self.dirty_files.insert(file_hash);
        self.pending_triples
            .entry(file_hash)
            .or_default()
            .extend(triples);
    }

    /// Flush all pending updates, returning them as a batch and clearing
    /// internal state.
    pub fn flush(&mut self) -> Vec<PendingTripleBatch> {
        let result: Vec<_> = self
            .dirty_files
            .iter()
            .filter_map(|&fh| {
                self.pending_triples
                    .remove(&fh)
                    .map(|triples| (fh, triples))
            })
            .collect();
        self.dirty_files.clear();
        result
    }

    /// Number of files with pending updates.
    pub fn pending_count(&self) -> usize {
        self.dirty_files.len()
    }

    /// Check whether a specific file is currently dirty.
    pub fn is_dirty(&self, file_hash: u64) -> bool {
        self.dirty_files.contains(&file_hash)
    }

    /// Discard all pending updates without flushing.
    pub fn clear(&mut self) {
        self.dirty_files.clear();
        self.pending_triples.clear();
    }

    /// Total number of pending triples across all dirty files.
    pub fn pending_triple_count(&self) -> usize {
        self.pending_triples.values().map(|v| v.len()).sum()
    }
}

impl Default for IncrementalUpdater {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a simple CacheResult for testing.
    fn file_hash_result(val: u64) -> CacheResult {
        CacheResult::FileHash(val)
    }

    fn triples_result(n: usize) -> CacheResult {
        let triples: Vec<(u64, u16, u64)> = (0..n)
            .map(|i| (i as u64, i as u16, (i + 100) as u64))
            .collect();
        CacheResult::TriplesForFile(triples)
    }

    fn reverse_deps_result(ids: &[u64]) -> CacheResult {
        CacheResult::ReverseDeps(ids.to_vec())
    }

    // ── Basic hit / miss tracking ─────────────────────────────────────────

    #[test]
    fn test_cache_miss_on_empty() {
        let mut cache = SitemapQueryCache::new(16);
        assert!(cache.get(42).is_none());
        assert_eq!(cache.misses, 1);
        assert_eq!(cache.hits, 0);
    }

    #[test]
    fn test_cache_insert_and_hit() {
        let mut cache = SitemapQueryCache::new(16);
        cache.insert(1, file_hash_result(0xDEAD), 60);
        let result = cache.get(1);
        assert!(result.is_some());
        assert_eq!(cache.hits, 1);
        assert_eq!(cache.misses, 0);
    }

    #[test]
    fn test_cache_hit_rate_after_misses_and_hits() {
        let mut cache = SitemapQueryCache::new(16);
        cache.insert(1, file_hash_result(10), 60);

        cache.get(1); // hit
        cache.get(2); // miss
        cache.get(1); // hit
        cache.get(3); // miss

        assert_eq!(cache.hits, 2);
        assert_eq!(cache.misses, 2);
        assert!((cache.hit_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_hit_rate_zero_queries() {
        let cache = SitemapQueryCache::new(16);
        assert_eq!(cache.hit_rate(), 0.0);
    }

    // ── TTL expiration ────────────────────────────────────────────────────

    #[test]
    fn test_entry_expires_after_ttl() {
        let mut cache = SitemapQueryCache::new(16);
        // Insert with 0-second TTL → immediately expired.
        cache.insert(1, file_hash_result(10), 0);

        // The entry should be treated as a miss.
        assert!(cache.get(1).is_none());
        assert_eq!(cache.misses, 1);
    }

    #[test]
    fn test_entry_not_expired_within_ttl() {
        let mut cache = SitemapQueryCache::new(16);
        cache.insert(1, file_hash_result(10), 3600);
        assert!(cache.get(1).is_some());
    }

    #[test]
    fn test_prune_expired() {
        let mut cache = SitemapQueryCache::new(16);
        cache.insert(1, file_hash_result(1), 0); // expires immediately
        cache.insert(2, file_hash_result(2), 3600); // long TTL

        cache.prune_expired();
        assert_eq!(cache.len(), 1);
        assert!(cache.get(2).is_some());
    }

    // ── LRU eviction ──────────────────────────────────────────────────────

    #[test]
    fn test_lru_eviction_at_capacity() {
        let mut cache = SitemapQueryCache::new(3);
        cache.insert(1, file_hash_result(1), 3600);
        cache.insert(2, file_hash_result(2), 3600);
        cache.insert(3, file_hash_result(3), 3600);

        // Cache is full. Inserting a 4th should evict key 1 (LRU).
        cache.insert(4, file_hash_result(4), 3600);
        assert_eq!(cache.len(), 3);
        assert!(cache.get(1).is_none()); // evicted → miss
        assert!(cache.get(4).is_some());
    }

    #[test]
    fn test_lru_access_bumps_entry() {
        let mut cache = SitemapQueryCache::new(3);
        cache.insert(1, file_hash_result(1), 3600);
        cache.insert(2, file_hash_result(2), 3600);
        cache.insert(3, file_hash_result(3), 3600);

        // Access key 1 so it becomes most-recently used.
        let _ = cache.get(1);

        // Insert key 4 → should evict key 2 (now the LRU).
        cache.insert(4, file_hash_result(4), 3600);
        assert!(cache.get(2).is_none()); // evicted
        assert!(cache.get(1).is_some()); // still alive
    }

    #[test]
    fn test_eviction_counter_increments() {
        let mut cache = SitemapQueryCache::new(2);
        cache.insert(1, file_hash_result(1), 3600);
        cache.insert(2, file_hash_result(2), 3600);
        cache.insert(3, file_hash_result(3), 3600); // evicts 1

        let stats = cache.stats();
        assert_eq!(stats.evictions, 1);
    }

    // ── File invalidation ─────────────────────────────────────────────────

    #[test]
    fn test_invalidate_file() {
        let mut cache = SitemapQueryCache::new(16);
        cache.insert(100, file_hash_result(100), 3600);
        cache.insert(200, file_hash_result(200), 3600);

        cache.invalidate_file(100);
        assert!(cache.get(100).is_none());
        assert!(cache.get(200).is_some());
    }

    #[test]
    fn test_invalidate_files_batch() {
        let mut cache = SitemapQueryCache::new(16);
        for i in 0..10 {
            cache.insert(i, file_hash_result(i), 3600);
        }
        cache.invalidate_files(&[0, 3, 7]);
        assert_eq!(cache.len(), 7);
    }

    #[test]
    fn test_invalidate_all() {
        let mut cache = SitemapQueryCache::new(16);
        for i in 0..10 {
            cache.insert(i, file_hash_result(i), 3600);
        }
        cache.invalidate_all();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    // ── Stats ─────────────────────────────────────────────────────────────

    #[test]
    fn test_stats_reflect_state() {
        let mut cache = SitemapQueryCache::new(4);
        cache.insert(1, triples_result(3), 3600);
        cache.insert(2, reverse_deps_result(&[10, 20]), 3600);

        cache.get(1); // hit
        cache.get(2); // hit
        cache.get(99); // miss

        let s = cache.stats();
        assert_eq!(s.hits, 2);
        assert_eq!(s.misses, 1);
        assert_eq!(s.entries, 2);
        assert_eq!(s.evictions, 0);
        assert!((s.hit_rate - 2.0 / 3.0).abs() < 1e-9);
    }

    // ── CacheResult variants ──────────────────────────────────────────────

    #[test]
    fn test_triples_for_file_variant() {
        let mut cache = SitemapQueryCache::new(8);
        cache.insert(1, triples_result(5), 60);
        if let Some(CacheResult::TriplesForFile(triples)) = cache.get(1) {
            assert_eq!(triples.len(), 5);
            assert_eq!(triples[0], (0, 0, 100));
        } else {
            panic!("expected TriplesForFile");
        }
    }

    #[test]
    fn test_reverse_deps_variant() {
        let mut cache = SitemapQueryCache::new(8);
        cache.insert(1, reverse_deps_result(&[10, 20, 30]), 60);
        if let Some(CacheResult::ReverseDeps(deps)) = cache.get(1) {
            assert_eq!(deps, &[10, 20, 30]);
        } else {
            panic!("expected ReverseDeps");
        }
    }

    #[test]
    fn test_node_lookup_none_variant() {
        let mut cache = SitemapQueryCache::new(8);
        cache.insert(1, CacheResult::NodeLookup(None), 60);
        if let Some(CacheResult::NodeLookup(None)) = cache.get(1) {
            // ok
        } else {
            panic!("expected NodeLookup(None)");
        }
    }

    #[test]
    fn test_overwrite_existing_key() {
        let mut cache = SitemapQueryCache::new(8);
        cache.insert(1, file_hash_result(100), 60);
        cache.insert(1, file_hash_result(200), 60); // overwrite
        assert_eq!(cache.len(), 1);
        if let Some(CacheResult::FileHash(h)) = cache.get(1) {
            assert_eq!(*h, 200);
        } else {
            panic!("expected FileHash(200)");
        }
    }

    #[test]
    fn test_default_constructor() {
        let cache = SitemapQueryCache::default();
        assert_eq!(cache.max_entries(), DEFAULT_MAX_ENTRIES);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_access_count_increments() {
        let mut cache = SitemapQueryCache::new(8);
        cache.insert(1, file_hash_result(1), 3600);
        let _ = cache.get(1);
        let _ = cache.get(1);
        let _ = cache.get(1);
        let entry = cache.cache.get(&1).unwrap();
        assert_eq!(entry.access_count, 3);
    }

    #[test]
    fn test_cache_result_kind_str() {
        assert_eq!(file_hash_result(0).kind_str(), "FileHash");
        assert_eq!(triples_result(0).kind_str(), "TriplesForFile");
        assert_eq!(reverse_deps_result(&[]).kind_str(), "ReverseDeps");
        assert_eq!(CacheResult::NodeLookup(None).kind_str(), "NodeLookup");
    }

    // ── IncrementalUpdater ────────────────────────────────────────────────

    #[test]
    fn test_updater_mark_dirty_and_pending() {
        let mut updater = IncrementalUpdater::new();
        updater.mark_dirty(1, vec![(10, 1, 20)]);
        assert!(updater.is_dirty(1));
        assert!(!updater.is_dirty(2));
        assert_eq!(updater.pending_count(), 1);
    }

    #[test]
    fn test_updater_flush_returns_batch() {
        let mut updater = IncrementalUpdater::new();
        updater.mark_dirty(1, vec![(10, 1, 20), (11, 2, 21)]);
        updater.mark_dirty(2, vec![(30, 3, 40)]);

        let batch = updater.flush();
        assert_eq!(batch.len(), 2);
        assert_eq!(updater.pending_count(), 0);
        assert!(!updater.is_dirty(1));
    }

    #[test]
    fn test_updater_coalesces_rapid_edits() {
        let mut updater = IncrementalUpdater::new();
        updater.mark_dirty(1, vec![(10, 1, 20)]);
        updater.mark_dirty(1, vec![(11, 2, 21)]); // same file, appended

        assert_eq!(updater.pending_count(), 1); // still one dirty file
        let total_triples: usize = updater.pending_triples.values().map(|v| v.len()).sum();
        assert_eq!(total_triples, 2);
    }

    #[test]
    fn test_updater_flush_empty() {
        let mut updater = IncrementalUpdater::new();
        let batch = updater.flush();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_updater_clear() {
        let mut updater = IncrementalUpdater::new();
        updater.mark_dirty(1, vec![(10, 1, 20)]);
        updater.mark_dirty(2, vec![(30, 3, 40)]);
        updater.clear();
        assert_eq!(updater.pending_count(), 0);
        assert!(!updater.is_dirty(1));
    }

    #[test]
    fn test_updater_pending_triple_count() {
        let mut updater = IncrementalUpdater::new();
        updater.mark_dirty(1, vec![(10, 1, 20), (11, 2, 21)]);
        updater.mark_dirty(2, vec![(30, 3, 40)]);
        assert_eq!(updater.pending_triple_count(), 3);
    }

    #[test]
    fn test_updater_default() {
        let updater = IncrementalUpdater::default();
        assert_eq!(updater.pending_count(), 0);
    }

    // ── Cache + updater integration ───────────────────────────────────────

    #[test]
    fn test_cache_invalidate_after_updater_flush() {
        let mut cache = SitemapQueryCache::new(16);
        let mut updater = IncrementalUpdater::new();

        // Simulate: file 100 was cached.
        cache.insert(100, triples_result(3), 60);
        assert!(cache.get(100).is_some());

        // File 100 is edited → mark dirty, then flush and invalidate cache.
        updater.mark_dirty(100, vec![(1, 1, 2)]);
        let batch = updater.flush();
        for &(fh, _) in &batch {
            cache.invalidate_file(fh);
        }

        assert!(cache.get(100).is_none());
    }
}
