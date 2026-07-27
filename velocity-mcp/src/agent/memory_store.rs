//! Persistent agent memory backed by NDA files.
//!
//! Enables cross-session learning: the agent remembers successful strategies,
//! failed approaches, and domain-specific knowledge between runs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single memory entry with metadata for retrieval and scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique key (e.g., "tool:write_file:success" or "site:github.com:login").
    pub key: String,
    /// Human-readable content of the memory.
    pub content: String,
    /// Tags for filtering (e.g., ["tool", "file_io", "success"]).
    pub tags: Vec<String>,
    /// Relevance/success score (0.0–1.0). Higher = more useful.
    pub score: f64,
    /// Number of times this memory has been accessed.
    pub access_count: u32,
    /// Unix timestamp of creation.
    pub created_at: u64,
    /// Unix timestamp of last access.
    pub last_accessed: u64,
}

/// A search result from memory recall.
#[derive(Debug, Clone)]
pub struct MemoryHit {
    /// The memory entry.
    pub entry: MemoryEntry,
    /// Cosine similarity score to the query (0.0–1.0).
    pub similarity: f64,
}

/// Persistent memory store backed by a JSON file in `.velocity/memory.nda`.
pub struct PersistentMemory {
    /// Path to the memory file.
    file_path: PathBuf,
    /// In-memory index of all entries.
    entries: HashMap<String, MemoryEntry>,
    /// Whether the store has unsaved changes.
    dirty: bool,
    /// Maximum number of entries before pruning low-score items.
    max_entries: usize,
}

impl PersistentMemory {
    /// Open or create a memory store at the given workspace root.
    pub fn open(workspace_root: &Path) -> Self {
        let dir = workspace_root.join(".velocity");
        let file_path = dir.join("memory.nda");

        let entries = if file_path.exists() {
            Self::load_from_file(&file_path)
        } else {
            HashMap::new()
        };

        Self {
            file_path,
            entries,
            dirty: false,
            max_entries: 1000,
        }
    }

    /// Store a new memory or update an existing one.
    pub fn remember(&mut self, key: &str, content: &str, tags: &[&str], score: f64) {
        let now = current_timestamp();
        let tags_vec: Vec<String> = tags.iter().map(|t| t.to_string()).collect();

        if let Some(existing) = self.entries.get_mut(key) {
            // Update existing: merge content, boost score
            existing.content = content.to_string();
            existing.score = (existing.score + score) / 2.0;
            existing.tags = tags_vec;
            existing.last_accessed = now;
            existing.access_count += 1;
        } else {
            self.entries.insert(
                key.to_string(),
                MemoryEntry {
                    key: key.to_string(),
                    content: content.to_string(),
                    tags: tags_vec,
                    score: score.clamp(0.0, 1.0),
                    access_count: 1,
                    created_at: now,
                    last_accessed: now,
                },
            );
        }

        self.dirty = true;
        self.prune_if_needed();
    }

    /// Recall memories relevant to a query using TF-IDF cosine similarity.
    pub fn recall(&self, query: &str, limit: usize) -> Vec<MemoryHit> {
        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let mut hits: Vec<MemoryHit> = self
            .entries
            .values()
            .map(|entry| {
                let entry_terms = tokenize(&entry.content);
                let tag_terms: Vec<String> = entry.tags.iter().flat_map(|t| tokenize(t)).collect();
                let all_terms: Vec<String> = entry_terms
                    .into_iter()
                    .chain(tag_terms)
                    .chain(tokenize(&entry.key))
                    .collect();
                let similarity = cosine_similarity(&query_terms, &all_terms);
                MemoryHit {
                    entry: entry.clone(),
                    similarity,
                }
            })
            .filter(|h| h.similarity > 0.01)
            .collect();

        // Sort by combined score: similarity * entry.score
        hits.sort_by(|a, b| {
            let score_a = a.similarity * a.entry.score;
            let score_b = b.similarity * b.entry.score;
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        hits.truncate(limit);
        hits
    }

    /// Remove a specific memory by key.
    pub fn forget(&mut self, key: &str) -> bool {
        let removed = self.entries.remove(key).is_some();
        if removed {
            self.dirty = true;
        }
        removed
    }

    /// Reinforce or penalize a memory's score.
    pub fn reinforce(&mut self, key: &str, delta: f64) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.score = (entry.score + delta).clamp(0.0, 1.0);
            entry.last_accessed = current_timestamp();
            self.dirty = true;
        }
    }

    /// Get total number of stored memories.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Save to disk if there are unsaved changes.
    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| format!("Serialize failed: {}", e))?;
        std::fs::write(&self.file_path, json)
            .map_err(|e| format!("Write failed: {}", e))?;
        self.dirty = false;
        Ok(())
    }

    // ─── Internal ────────────────────────────────────────────────────────────

    fn load_from_file(path: &Path) -> HashMap<String, MemoryEntry> {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn prune_if_needed(&mut self) {
        if self.entries.len() <= self.max_entries {
            return;
        }
        // Remove lowest-score entries
        let mut keys: Vec<(String, f64)> = self
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), v.score))
            .collect();
        keys.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let to_remove = self.entries.len() - self.max_entries;
        for (key, _) in keys.into_iter().take(to_remove) {
            self.entries.remove(&key);
        }
    }
}

impl Drop for PersistentMemory {
    fn drop(&mut self) {
        let _ = self.save();
    }
}

// ─── TF-IDF Helpers ──────────────────────────────────────────────────────────

/// Tokenize text into lowercase terms.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() > 1)
        .map(String::from)
        .collect()
}

/// Compute cosine similarity between two term lists.
fn cosine_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    // Build term frequency maps
    let mut freq_a: HashMap<&str, f64> = HashMap::new();
    for term in a {
        *freq_a.entry(term.as_str()).or_default() += 1.0;
    }
    let mut freq_b: HashMap<&str, f64> = HashMap::new();
    for term in b {
        *freq_b.entry(term.as_str()).or_default() += 1.0;
    }

    // Dot product
    let mut dot = 0.0;
    for (term, count_a) in &freq_a {
        if let Some(count_b) = freq_b.get(term) {
            dot += count_a * count_b;
        }
    }

    // Magnitudes
    let mag_a: f64 = freq_a.values().map(|v| v * v).sum::<f64>().sqrt();
    let mag_b: f64 = freq_b.values().map(|v| v * v).sum::<f64>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_and_recall() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());

        mem.remember("tool:write_file", "Successfully wrote src/main.rs", &["tool", "file"], 0.9);
        mem.remember("tool:read_file", "Read Cargo.toml for dependencies", &["tool", "file"], 0.8);
        mem.remember("site:github", "Login form has username and password fields", &["web", "auth"], 0.7);

        let hits = mem.recall("write file to disk", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].entry.key, "tool:write_file");
    }

    #[test]
    fn reinforce_and_forget() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());

        mem.remember("test_key", "test content", &["test"], 0.5);
        mem.reinforce("test_key", 0.3);
        assert_eq!(mem.entries["test_key"].score, 0.8);

        mem.reinforce("test_key", -0.9);
        assert_eq!(mem.entries["test_key"].score, 0.0);

        assert!(mem.forget("test_key"));
        assert!(!mem.forget("test_key"));
    }

    #[test]
    fn persistence_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut mem = PersistentMemory::open(dir.path());
            mem.remember("persist_test", "hello world", &["test"], 0.9);
            mem.save().unwrap();
        }
        // Re-open
        let mem2 = PersistentMemory::open(dir.path());
        assert_eq!(mem2.len(), 1);
        let hits = mem2.recall("hello", 1);
        assert_eq!(hits[0].entry.key, "persist_test");
    }

    #[test]
    fn cosine_sim_identical() {
        let a = vec!["hello".to_string(), "world".to_string()];
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_sim_disjoint() {
        let a = vec!["hello".to_string()];
        let b = vec!["world".to_string()];
        let sim = cosine_similarity(&a, &b);
        assert!(sim < 0.001);
    }

    #[test]
    fn recall_ranks_by_relevance_and_filters_noise() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("k_auth", "oauth login authentication flow tokens", &["auth"], 0.9);
        mem.remember("k_db", "database connection pooling migrations", &["db"], 0.9);
        mem.remember("k_unrelated", "cooking pasta recipes basil", &["food"], 0.9);

        let hits = mem.recall("authentication login tokens", 5);
        // The auth memory must rank first.
        assert_eq!(hits[0].entry.key, "k_auth");
        // The unrelated cooking memory shares no terms and is filtered out.
        assert!(!hits.iter().any(|h| h.entry.key == "k_unrelated"));
    }

    #[test]
    fn recall_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        for i in 0..10 {
            mem.remember(
                &format!("k{i}"),
                &format!("shared term number {i}"),
                &["tag"],
                0.5,
            );
        }
        let hits = mem.recall("shared term", 3);
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn retention_prunes_lowest_score_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.max_entries = 5;
        // Insert 6 entries with strictly increasing scores.
        for i in 0..6 {
            let score = 0.1 + (i as f64) * 0.1; // 0.1 .. 0.6
            mem.remember(&format!("k{i}"), &format!("content {i}"), &["t"], score);
        }
        assert_eq!(mem.len(), 5);
        // The lowest-score entry (k0) was pruned; the highest (k5) survives.
        assert!(!mem.entries.contains_key("k0"));
        assert!(mem.entries.contains_key("k5"));
    }

    #[test]
    fn recall_empty_query_returns_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("k", "some content here", &["t"], 0.5);
        assert!(mem.recall("   ", 5).is_empty());
    }
}
