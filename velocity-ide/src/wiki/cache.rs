//! Content-keyed SQLite cache for wiki generation.
//!
//! Stores LLM-generated content keyed by content hash, so re-scanning
//! after small edits costs no API calls for untouched modules.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
/// Cache entry for a single wiki page's LLM-generated content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Content hash of the source file (for invalidation)
    pub content_hash: String,
    /// The generated detail/narration text
    pub detail: String,
    /// Timestamp when this entry was created
    pub created_at: u64,
    /// Number of times this entry has been accessed
    pub access_count: u32,
    /// Token count of the input (for statistics)
    pub input_tokens: usize,
    /// Token count of the output (for statistics)
    pub output_tokens: usize,
}

/// Statistics about cache usage.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_hits: u64,
    pub total_misses: u64,
    pub total_input_tokens: usize,
    pub total_output_tokens: usize,
    pub cache_size_bytes: usize,
}

/// File-based wiki cache (JSON format for simplicity).
/// 
/// In production, this could be backed by SQLite for better performance,
/// but JSON is sufficient for typical wiki sizes and easier to debug.
pub struct WikiCache {
    /// Path to the cache directory
    cache_dir: PathBuf,
    /// In-memory index of cache entries
    entries: HashMap<String, CacheEntry>,
    /// Cache statistics
    stats: CacheStats,
}

impl WikiCache {
    /// Open or create a cache at the given directory.
    pub fn open(cache_dir: &Path) -> Self {
        let cache_dir = cache_dir.to_path_buf();
        let _ = fs::create_dir_all(&cache_dir);
        
        let index_path = cache_dir.join("cache_index.json");
        let entries: HashMap<String, CacheEntry> = if index_path.exists() {
            fs::read_to_string(&index_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        
        let mut stats = CacheStats::default();
        stats.total_entries = entries.len();
        for entry in entries.values() {
            stats.total_input_tokens += entry.input_tokens;
            stats.total_output_tokens += entry.output_tokens;
        }
        
        // Calculate cache size
        if let Ok(metadata) = fs::metadata(&cache_dir) {
            stats.cache_size_bytes = metadata.len() as usize;
        }
        
        WikiCache {
            cache_dir,
            entries,
            stats,
        }
    }
    
    /// Open the default cache location (~/.velocity/wiki-cache/).
    pub fn open_default() -> Self {
        let cache_dir = default_cache_dir();
        Self::open(&cache_dir)
    }
    
    /// Look up a cached entry by file path and content hash.
    /// Returns None if not found or if content has changed.
    pub fn get(&mut self, file_path: &str, content_hash: &str) -> Option<String> {
        let key = cache_key(file_path);
        
        let entry = self.entries.get_mut(&key);
        if let Some(entry) = entry {
            if entry.content_hash == content_hash {
                entry.access_count += 1;
                self.stats.total_hits += 1;
                return Some(entry.detail.clone());
            } else {
                // Content changed, invalidate
                self.entries.remove(&key);
            }
        }
        
        self.stats.total_misses += 1;
        None
    }
    
    /// Store a cache entry.
    pub fn put(
        &mut self,
        file_path: &str,
        content_hash: &str,
        detail: String,
        input_tokens: usize,
        output_tokens: usize,
    ) {
        let key = cache_key(file_path);
        let now = current_timestamp();
        
        let entry = CacheEntry {
            content_hash: content_hash.to_string(),
            detail,
            created_at: now,
            access_count: 0,
            input_tokens,
            output_tokens,
        };
        
        self.entries.insert(key, entry);
        self.stats.total_entries = self.entries.len();
        self.stats.total_input_tokens += input_tokens;
        self.stats.total_output_tokens += output_tokens;
    }
    
    /// Check if a file is cached and unchanged.
    pub fn is_cached(&self, file_path: &str, content_hash: &str) -> bool {
        let key = cache_key(file_path);
        self.entries
            .get(&key)
            .map(|e| e.content_hash == content_hash)
            .unwrap_or(false)
    }
    
    /// Remove entries for files that no longer exist.
    pub fn prune_missing(&mut self, existing_files: &[&str]) {
        let existing_keys: std::collections::HashSet<String> = existing_files
            .iter()
            .map(|f| cache_key(f))
            .collect();
        
        self.entries.retain(|k, _| existing_keys.contains(k));
        self.stats.total_entries = self.entries.len();
    }
    
    /// Remove all entries older than the given age in seconds.
    pub fn prune_old(&mut self, max_age_secs: u64) {
        let cutoff = current_timestamp().saturating_sub(max_age_secs);
        self.entries.retain(|_, e| e.created_at > cutoff);
        self.stats.total_entries = self.entries.len();
    }
    
    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.stats = CacheStats::default();
        let _ = fs::remove_dir_all(&self.cache_dir);
        let _ = fs::create_dir_all(&self.cache_dir);
    }
    
    /// Persist the cache index to disk.
    pub fn save(&self) -> std::io::Result<()> {
        let index_path = self.cache_dir.join("cache_index.json");
        let json = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(index_path, json)
    }
    
    /// Get cache statistics.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
    
    /// Calculate hit rate as a percentage.
    pub fn hit_rate(&self) -> f64 {
        let total = self.stats.total_hits + self.stats.total_misses;
        if total == 0 {
            0.0
        } else {
            (self.stats.total_hits as f64 / total as f64) * 100.0
        }
    }
    
    /// Estimate token savings from cache hits.
    pub fn estimated_token_savings(&self) -> usize {
        // Each hit saves the input tokens that would have been sent to the LLM
        self.stats.total_hits as usize * (self.stats.total_input_tokens / self.stats.total_entries.max(1))
    }
}

impl Drop for WikiCache {
    fn drop(&mut self) {
        // Auto-save on drop
        let _ = self.save();
    }
}

/// Incremental regeneration state for wiki exports.
/// 
/// Tracks which pages were generated from which inputs, so re-runs
/// only regenerate pages whose source changed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RegenerationState {
    /// Map of output page path -> (input file path, content hash)
    pub pages: HashMap<String, PageState>,
    /// Last full regeneration timestamp
    pub last_full_regen: u64,
}

/// State for a single wiki page.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageState {
    /// Source file path
    pub source_path: String,
    /// Content hash at time of generation
    pub content_hash: String,
    /// When this page was generated
    pub generated_at: u64,
    /// Whether this page has LLM-generated detail
    pub has_detail: bool,
}

impl RegenerationState {
    /// Load regeneration state from a file.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    
    /// Save regeneration state to a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, json)
    }
    
    /// Check if a page needs regeneration.
    pub fn needs_regen(&self, page_path: &str, _source_path: &str, content_hash: &str) -> bool {
        match self.pages.get(page_path) {
            Some(state) => state.content_hash != content_hash,
            None => true,
        }
    }
    
    /// Mark a page as generated.
    pub fn mark_generated(&mut self, page_path: &str, source_path: &str, content_hash: &str, has_detail: bool) {
        self.pages.insert(page_path.to_string(), PageState {
            source_path: source_path.to_string(),
            content_hash: content_hash.to_string(),
            generated_at: current_timestamp(),
            has_detail,
        });
    }
    
    /// Remove pages for files that no longer exist.
    pub fn prune_missing(&mut self, existing_sources: &[&str]) {
        let existing: std::collections::HashSet<&str> = existing_sources.iter().copied().collect();
        self.pages.retain(|_, state| existing.contains(state.source_path.as_str()));
    }
    
    /// Get list of pages that need regeneration.
    pub fn pages_to_regen<'a>(
        &'a self,
        sources: &'a [(String, String, String)], // (page_path, source_path, content_hash)
    ) -> Vec<&'a (String, String, String)> {
        sources
            .iter()
            .filter(|(page, source, hash)| self.needs_regen(page, source, hash))
            .collect()
    }
}

// ─── Helper functions ──────────────────────────────────────────────────────

fn default_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".velocity").join("wiki-cache");
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("Velocity").join("wiki-cache");
    }
    PathBuf::from(".velocity").join("wiki-cache")
}

fn cache_key(file_path: &str) -> String {
    // Normalize path separators and create a safe key
    file_path
        .replace('\\', "/")
        .replace('/', "_")
        .replace('.', "_")
        .to_lowercase()
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    
    fn temp_cache_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("velocity_wiki_cache_test_{}_{}", std::process::id(), 
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&dir).expect("Failed to create temp cache dir");
        dir
    }
    
    #[test]
    fn test_cache_put_get() {
        let dir = temp_cache_dir();
        let mut cache = WikiCache::open(&dir);
        
        // Miss on empty cache
        assert!(cache.get("src/lib.rs", "hash1").is_none());
        
        // Put and get
        cache.put("src/lib.rs", "hash1", "Generated content".to_string(), 100, 50);
        assert_eq!(cache.get("src/lib.rs", "hash1"), Some("Generated content".to_string()));
        
        // Different hash = miss (content changed)
        assert!(cache.get("src/lib.rs", "hash2").is_none());
        
        let _ = fs::remove_dir_all(&dir);
    }
    
    #[test]
    fn test_cache_persistence() {
        let dir = temp_cache_dir();
        
        // Create and populate cache
        {
            let mut cache = WikiCache::open(&dir);
            cache.put("src/lib.rs", "hash1", "Content".to_string(), 100, 50);
            cache.save().unwrap();
        }
        
        // Reopen and verify
        {
            let mut cache = WikiCache::open(&dir);
            assert_eq!(cache.get("src/lib.rs", "hash1"), Some("Content".to_string()));
        }
        
        let _ = fs::remove_dir_all(&dir);
    }
    
    #[test]
    fn test_cache_stats() {
        let dir = temp_cache_dir();
        let mut cache = WikiCache::open(&dir);
        
        cache.put("a.rs", "h1", "A".to_string(), 10, 5);
        cache.put("b.rs", "h2", "B".to_string(), 20, 10);
        
        cache.get("a.rs", "h1"); // hit
        cache.get("a.rs", "h1"); // hit
        cache.get("c.rs", "h3"); // miss
        
        assert_eq!(cache.stats().total_hits, 2);
        assert_eq!(cache.stats().total_misses, 1);
        assert!(cache.hit_rate() > 60.0);
        
        let _ = fs::remove_dir_all(&dir);
    }
    
    #[test]
    fn test_regeneration_state() {
        let mut state = RegenerationState::default();
        
        // New page needs regen
        assert!(state.needs_regen("wiki/lib.md", "src/lib.rs", "hash1"));
        
        // Mark as generated
        state.mark_generated("wiki/lib.md", "src/lib.rs", "hash1", true);
        
        // Same hash = no regen needed
        assert!(!state.needs_regen("wiki/lib.md", "src/lib.rs", "hash1"));
        
        // Different hash = needs regen
        assert!(state.needs_regen("wiki/lib.md", "src/lib.rs", "hash2"));
    }
    
    #[test]
    fn test_cache_key_normalization() {
        assert_eq!(cache_key("src/lib.rs"), "src_lib_rs");
        assert_eq!(cache_key("src\\lib.rs"), "src_lib_rs");
        assert_eq!(cache_key("SRC/LIB.RS"), "src_lib_rs");
    }
}
