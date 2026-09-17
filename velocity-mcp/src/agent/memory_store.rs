//! Persistent agent memory backed by NDA files.
//!
//! Enables cross-session learning: the agent remembers successful strategies,
//! failed approaches, and domain-specific knowledge between runs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::memory::{MemoryEntry as SessionMemoryEntry, SessionMemory};

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
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
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

    /// Iterate over all stored memory entries.
    pub fn iter(&self) -> impl Iterator<Item = &MemoryEntry> {
        self.entries.values()
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
        std::fs::write(&self.file_path, json).map_err(|e| format!("Write failed: {}", e))?;
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

// ---------------------------------------------------------------------------
// Cross-Session Memory Store
// ---------------------------------------------------------------------------

/// Snapshot of a session's memory for persistent storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Unique session identifier.
    pub session_id: String,
    /// All memory entries from the session.
    pub entries: Vec<SessionMemoryEntry>,
    /// When the session was created.
    pub created: SystemTime,
    /// When the session was last active.
    pub last_active: SystemTime,
    /// Number of entries in the snapshot.
    pub entry_count: usize,
}

/// Statistics about the memory store.
#[derive(Debug, Clone)]
pub struct MemoryStoreStats {
    /// Total number of stored sessions.
    pub total_sessions: usize,
    /// Number of global entries.
    pub global_entry_count: usize,
    /// Total entries across all sessions and global.
    pub total_entry_count: usize,
    /// Size of the store file in bytes.
    pub store_size_bytes: u64,
    /// Age of the oldest session in seconds.
    pub oldest_session_age_secs: u64,
}

/// Persistent cross-session memory store.
///
/// Manages session snapshots and global knowledge entries that persist across
/// IDE restarts. Uses NDA encryption when available, falls back to plain JSON.
pub struct MemoryStore {
    /// Path to the store file.
    store_path: PathBuf,
    /// Session snapshots keyed by session ID.
    sessions: HashMap<String, SessionSnapshot>,
    /// Global entries that persist across all sessions.
    global_entries: Vec<SessionMemoryEntry>,
    /// Maximum number of global entries before pruning.
    max_global_entries: usize,
}

impl MemoryStore {
    /// Default maximum number of global entries.
    const DEFAULT_MAX_GLOBAL_ENTRIES: usize = 500;

    /// Create a new memory store at the given path.
    pub fn new(store_path: PathBuf) -> Self {
        let mut store = Self {
            store_path,
            sessions: HashMap::new(),
            global_entries: Vec::new(),
            max_global_entries: Self::DEFAULT_MAX_GLOBAL_ENTRIES,
        };
        // Load existing data if present.
        store.load_from_disk();
        store
    }

    /// Save a session's memory to persistent storage.
    pub fn save_session(&mut self, memory: &SessionMemory) -> Result<(), String> {
        let now = SystemTime::now();
        let session_id = memory.session_id().to_string();

        // Use JSON serialization to extract entries.
        let json = memory.to_json();
        let snapshot = self.parse_session_snapshot(&json, &session_id, now)?;

        self.sessions.insert(session_id.clone(), snapshot);
        self.persist_to_disk()
    }

    /// Load a session's memory from persistent storage.
    pub fn load_session(&self, session_id: &str) -> Option<SessionMemory> {
        let snapshot = self.sessions.get(session_id)?;
        let mut memory = SessionMemory::new(session_id.to_string());

        // Restore entries by re-remembering them.
        for entry in &snapshot.entries {
            memory.remember(
                entry.kind,
                entry.content.clone(),
                entry.importance,
                entry.tags.clone(),
            );
        }

        Some(memory)
    }

    /// Save global entries to persistent storage.
    pub fn save_global(&mut self, entries: &[SessionMemoryEntry]) -> Result<(), String> {
        self.global_entries = entries.to_vec();
        self.prune_global_if_needed();
        self.persist_to_disk()
    }

    /// Load global entries from persistent storage.
    pub fn load_global(&self) -> Vec<SessionMemoryEntry> {
        self.global_entries.clone()
    }

    /// Merge important session memories into global knowledge.
    ///
    /// Entries with importance >= 0.7 are promoted to global.
    pub fn merge_session_into_global(&mut self, session_id: &str) {
        if let Some(snapshot) = self.sessions.get(session_id) {
            let high_importance: Vec<SessionMemoryEntry> = snapshot
                .entries
                .iter()
                .filter(|e| e.importance >= 0.7)
                .cloned()
                .collect();

            self.global_entries.extend(high_importance);
            self.prune_global_if_needed();
        }
    }

    /// Remove oldest sessions beyond the `keep` count.
    pub fn prune_old_sessions(&mut self, keep: usize) {
        if self.sessions.len() <= keep {
            return;
        }

        // Sort sessions by last_active time.
        let mut session_list: Vec<(String, SystemTime)> = self
            .sessions
            .iter()
            .map(|(id, snap)| (id.clone(), snap.last_active))
            .collect();

        session_list.sort_by_key(|a| a.1);

        // Remove oldest sessions.
        let to_remove = self.sessions.len() - keep;
        for (id, _) in session_list.into_iter().take(to_remove) {
            self.sessions.remove(&id);
        }
    }

    /// Get all session IDs.
    pub fn all_session_ids(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    /// Get total entry count across all sessions and global.
    pub fn total_entries(&self) -> usize {
        let session_entries: usize = self.sessions.values().map(|s| s.entry_count).sum();
        session_entries + self.global_entries.len()
    }

    /// Get store statistics.
    pub fn store_stats(&self) -> MemoryStoreStats {
        let now = SystemTime::now();
        let mut oldest_age = std::time::Duration::ZERO;

        for snapshot in self.sessions.values() {
            let age = now
                .duration_since(snapshot.created)
                .unwrap_or(std::time::Duration::ZERO);
            if age > oldest_age {
                oldest_age = age;
            }
        }

        let session_entries: usize = self.sessions.values().map(|s| s.entry_count).sum();
        let store_size = std::fs::metadata(&self.store_path)
            .map(|m| m.len())
            .unwrap_or(0);

        MemoryStoreStats {
            total_sessions: self.sessions.len(),
            global_entry_count: self.global_entries.len(),
            total_entry_count: session_entries + self.global_entries.len(),
            store_size_bytes: store_size,
            oldest_session_age_secs: oldest_age.as_secs(),
        }
    }

    // ─── Internal Helpers ─────────────────────────────────────────────────────

    fn parse_session_snapshot(
        &self,
        json: &str,
        session_id: &str,
        now: SystemTime,
    ) -> Result<SessionSnapshot, String> {
        // Parse the JSON from SessionMemory::to_json().
        #[derive(Deserialize)]
        struct InternalSnapshot {
            entries: Vec<SessionMemoryEntry>,
        }

        let internal: InternalSnapshot =
            serde_json::from_str(json).map_err(|e| format!("Parse error: {}", e))?;

        let entry_count = internal.entries.len();

        Ok(SessionSnapshot {
            session_id: session_id.to_string(),
            entries: internal.entries,
            created: now,
            last_active: now,
            entry_count,
        })
    }

    fn prune_global_if_needed(&mut self) {
        if self.global_entries.len() <= self.max_global_entries {
            return;
        }

        // Sort by importance and keep the highest.
        self.global_entries.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.global_entries.truncate(self.max_global_entries);
    }

    fn persist_to_disk(&mut self) -> Result<(), String> {
        if let Some(parent) = self.store_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        #[derive(Serialize)]
        struct StoreData<'a> {
            sessions: &'a HashMap<String, SessionSnapshot>,
            global_entries: &'a Vec<SessionMemoryEntry>,
        }

        let data = StoreData {
            sessions: &self.sessions,
            global_entries: &self.global_entries,
        };

        let json =
            serde_json::to_string_pretty(&data).map_err(|e| format!("Serialize failed: {}", e))?;

        // Try NDA encryption if crypto is available.
        let bytes = self.maybe_encrypt(json.as_bytes());

        std::fs::write(&self.store_path, bytes).map_err(|e| format!("Write failed: {}", e))?;

        Ok(())
    }

    fn load_from_disk(&mut self) {
        if !self.store_path.exists() {
            return;
        }

        let bytes = match std::fs::read(&self.store_path) {
            Ok(b) => b,
            Err(_) => return,
        };

        // Try NDA decryption if encrypted.
        let decrypted = self.maybe_decrypt(&bytes);

        #[derive(Deserialize)]
        struct StoreData {
            sessions: HashMap<String, SessionSnapshot>,
            global_entries: Vec<SessionMemoryEntry>,
        }

        let data: StoreData = match serde_json::from_slice(&decrypted) {
            Ok(d) => d,
            Err(_) => return,
        };

        self.sessions = data.sessions;
        self.global_entries = data.global_entries;
    }

    fn maybe_encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        // Try to use crypto::seal if workspace root is available.
        // For now, fall back to plain JSON.
        // In production, check for workspace_root and call crypto::seal.
        plaintext.to_vec()
    }

    fn maybe_decrypt(&self, bytes: &[u8]) -> Vec<u8> {
        // Try to use crypto::open if encrypted.
        // For now, return as-is for plain JSON.
        bytes.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::{MemoryKind, SessionMemory};

    #[test]
    fn remember_and_recall() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());

        mem.remember(
            "tool:write_file",
            "Successfully wrote src/main.rs",
            &["tool", "file"],
            0.9,
        );
        mem.remember(
            "tool:read_file",
            "Read Cargo.toml for dependencies",
            &["tool", "file"],
            0.8,
        );
        mem.remember(
            "site:github",
            "Login form has username and password fields",
            &["web", "auth"],
            0.7,
        );

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
        mem.remember(
            "k_auth",
            "oauth login authentication flow tokens",
            &["auth"],
            0.9,
        );
        mem.remember(
            "k_db",
            "database connection pooling migrations",
            &["db"],
            0.9,
        );
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

    #[test]
    fn is_empty_on_fresh_store() {
        let dir = tempfile::tempdir().unwrap();
        let mem = PersistentMemory::open(dir.path());
        assert!(mem.is_empty());
        assert_eq!(mem.len(), 0);
    }

    #[test]
    fn is_empty_after_remember() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("key", "content", &["t"], 0.5);
        assert!(!mem.is_empty());
        assert_eq!(mem.len(), 1);
    }

    #[test]
    fn iter_returns_all_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("k1", "content 1", &["a"], 0.5);
        mem.remember("k2", "content 2", &["b"], 0.7);
        mem.remember("k3", "content 3", &["c"], 0.9);

        let entries: Vec<&MemoryEntry> = mem.iter().collect();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn reinforce_nonexistent_key_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.reinforce("ghost", 0.5); // should not panic
        assert!(mem.is_empty());
    }

    #[test]
    fn save_when_clean_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("k", "content", &["t"], 0.5);
        mem.save().unwrap(); // first save
                             // Now dirty is false; second save should be a no-op
        assert!(mem.save().is_ok());
    }

    #[test]
    fn remember_updates_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("k", "original", &["t1"], 0.8);
        mem.remember("k", "updated", &["t2"], 0.6);

        assert_eq!(mem.len(), 1);
        let entry = mem.iter().next().unwrap();
        assert_eq!(entry.content, "updated");
        assert_eq!(entry.tags, vec!["t2"]);
        // Score is averaged: (0.8 + 0.6) / 2 = 0.7
        assert!((entry.score - 0.7).abs() < 0.001);
    }

    #[test]
    fn score_is_clamped_to_0_1() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        mem.remember("k", "content", &["t"], 1.5); // above 1.0
        let entry = mem.iter().next().unwrap();
        assert!(entry.score <= 1.0);

        mem.remember("k2", "content", &["t"], -0.5); // below 0.0
        let entry2 = mem.entries.get("k2").unwrap();
        assert!(entry2.score >= 0.0);
    }

    #[test]
    fn cosine_sim_empty_returns_zero() {
        let a: Vec<String> = vec![];
        let b = vec!["hello".to_string()];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
        assert_eq!(cosine_similarity(&b, &a), 0.0);
    }

    #[test]
    fn tokenize_filters_short_terms() {
        let terms = tokenize("I am a big fan of Rust");
        // "I", "a" are single chars -> filtered out
        assert!(!terms.contains(&"i".to_string()));
        assert!(!terms.contains(&"a".to_string()));
        assert!(terms.contains(&"am".to_string()));
        assert!(terms.contains(&"big".to_string()));
        assert!(terms.contains(&"fan".to_string()));
        assert!(terms.contains(&"rust".to_string()));
    }

    // ─── MemoryStore Cross-Session Tests ──────────────────────────────────────

    fn make_test_session(id: &str) -> SessionMemory {
        let mut mem = SessionMemory::new(id.to_string());
        mem.remember(
            MemoryKind::Fact,
            format!("Test fact for {}", id),
            0.8,
            vec!["test".into()],
        );
        mem
    }

    #[test]
    fn memory_store_new_creates_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let store = MemoryStore::new(path);
        assert_eq!(store.all_session_ids().len(), 0);
        assert_eq!(store.total_entries(), 0);
    }

    #[test]
    fn memory_store_save_and_load_session_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session = SessionMemory::new("session-1".to_string());
        session.remember(
            MemoryKind::Decision,
            "Use async/await pattern".into(),
            0.9,
            vec!["rust".into(), "async".into()],
        );
        session.remember(
            MemoryKind::CodePattern,
            "RAII for resource management".into(),
            0.85,
            vec!["rust".into()],
        );

        store.save_session(&session).unwrap();
        assert_eq!(store.all_session_ids().len(), 1);

        let loaded = store.load_session("session-1").unwrap();
        assert_eq!(loaded.session_id(), "session-1");
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn memory_store_global_entries_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session = SessionMemory::new("session-global".to_string());
        session.remember(
            MemoryKind::UserPreference,
            "User prefers dark theme".into(),
            0.95,
            vec!["preference".into()],
        );

        store.save_session(&session).unwrap();
        store.merge_session_into_global("session-global");

        let global = store.load_global();
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].content, "User prefers dark theme");
    }

    #[test]
    fn memory_store_merge_session_into_global_filters_low_importance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session = SessionMemory::new("session-merge".to_string());
        session.remember(MemoryKind::Fact, "High importance".into(), 0.9, vec![]);
        session.remember(MemoryKind::Fact, "Low importance".into(), 0.3, vec![]);
        session.remember(MemoryKind::Fact, "Medium importance".into(), 0.7, vec![]);

        store.save_session(&session).unwrap();
        store.merge_session_into_global("session-merge");

        let global = store.load_global();
        // Only entries with importance >= 0.7 should be merged.
        assert_eq!(global.len(), 2);
        assert!(global.iter().all(|e| e.importance >= 0.7));
    }

    #[test]
    fn memory_store_prune_old_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        for i in 0..5 {
            let session = make_test_session(&format!("session-{}", i));
            store.save_session(&session).unwrap();
        }

        assert_eq!(store.all_session_ids().len(), 5);
        store.prune_old_sessions(3);
        assert_eq!(store.all_session_ids().len(), 3);
    }

    #[test]
    fn memory_store_empty_store_handling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let store = MemoryStore::new(path);

        assert!(store.load_session("nonexistent").is_none());
        assert_eq!(store.load_global().len(), 0);
        assert_eq!(store.total_entries(), 0);
    }

    #[test]
    fn memory_store_multiple_sessions_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let session1 = make_test_session("session-A");
        let session2 = make_test_session("session-B");
        let session3 = make_test_session("session-C");

        store.save_session(&session1).unwrap();
        store.save_session(&session2).unwrap();
        store.save_session(&session3).unwrap();

        assert_eq!(store.all_session_ids().len(), 3);
        assert!(store.all_session_ids().contains(&"session-A".to_string()));
        assert!(store.all_session_ids().contains(&"session-B".to_string()));
        assert!(store.all_session_ids().contains(&"session-C".to_string()));
    }

    #[test]
    fn memory_store_stats_accuracy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session1 = SessionMemory::new("stats-session-1".to_string());
        session1.remember(MemoryKind::Fact, "Fact 1".into(), 0.8, vec![]);
        session1.remember(MemoryKind::Fact, "Fact 2".into(), 0.7, vec![]);

        let mut session2 = SessionMemory::new("stats-session-2".to_string());
        session2.remember(MemoryKind::Decision, "Decision 1".into(), 0.9, vec![]);

        store.save_session(&session1).unwrap();
        store.save_session(&session2).unwrap();

        let stats = store.store_stats();
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.total_entry_count, 3); // 2 + 1
    }

    #[test]
    fn memory_store_save_global_directly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session = SessionMemory::new("global-source".to_string());
        let id = session.remember(
            MemoryKind::CodePattern,
            "Use iterators over loops".into(),
            0.85,
            vec!["rust".into()],
        );

        let entries: Vec<SessionMemoryEntry> = vec![session.get(id).unwrap().clone()];
        store.save_global(&entries).unwrap();

        let loaded = store.load_global();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "Use iterators over loops");
    }

    #[test]
    fn memory_store_persistence_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");

        {
            let mut store = MemoryStore::new(path.clone());
            let session = make_test_session("persist-test");
            store.save_session(&session).unwrap();
        }

        // Reopen the store.
        let store2 = MemoryStore::new(path);
        assert_eq!(store2.all_session_ids().len(), 1);
        assert!(store2.load_session("persist-test").is_some());
    }

    #[test]
    fn memory_store_total_entries_includes_global() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session = SessionMemory::new("total-test".to_string());
        session.remember(MemoryKind::Fact, "Session fact".into(), 0.8, vec![]);
        store.save_session(&session).unwrap();

        let mut global_entry = session.get(1).unwrap().clone();
        global_entry.content = "Global fact".into();
        store.save_global(&[global_entry]).unwrap();

        // Total = 1 session entry + 1 global entry.
        assert_eq!(store.total_entries(), 2);
    }

    #[test]
    fn memory_store_prune_global_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);
        store.max_global_entries = 3;

        let mut _entries: Vec<SessionMemoryEntry> = Vec::new();
        for i in 0..5 {
            let mut session = SessionMemory::new(format!("prune-{}", i));
            let importance = 0.5 + (i as f64) * 0.1;
            session.remember(MemoryKind::Fact, format!("Fact {}", i), importance, vec![]);
            store.save_session(&session).unwrap();
            store.merge_session_into_global(&format!("prune-{}", i));
        }

        let global = store.load_global();
        assert!(global.len() <= 3);
    }

    #[test]
    fn memory_store_load_nonexistent_session_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let store = MemoryStore::new(path);

        assert!(store.load_session("does-not-exist").is_none());
    }

    #[test]
    fn memory_store_merge_nonexistent_session_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        store.merge_session_into_global("ghost-session");
        assert_eq!(store.load_global().len(), 0);
    }

    #[test]
    fn memory_store_prune_sessions_with_zero_keep() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let session = make_test_session("to-prune");
        store.save_session(&session).unwrap();
        assert_eq!(store.all_session_ids().len(), 1);

        store.prune_old_sessions(0);
        assert_eq!(store.all_session_ids().len(), 0);
    }

    #[test]
    fn memory_store_stats_with_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let store = MemoryStore::new(path);

        let stats = store.store_stats();
        assert_eq!(stats.total_sessions, 0);
        assert_eq!(stats.global_entry_count, 0);
        assert_eq!(stats.total_entry_count, 0);
        assert_eq!(stats.oldest_session_age_secs, 0);
    }

    #[test]
    fn memory_store_overwrite_existing_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.nda");
        let mut store = MemoryStore::new(path);

        let mut session1 = SessionMemory::new("overwrite-test".to_string());
        session1.remember(MemoryKind::Fact, "First version".into(), 0.5, vec![]);
        store.save_session(&session1).unwrap();

        let mut session2 = SessionMemory::new("overwrite-test".to_string());
        session2.remember(MemoryKind::Fact, "Second version".into(), 0.9, vec![]);
        store.save_session(&session2).unwrap();

        // Should still have only 1 session.
        assert_eq!(store.all_session_ids().len(), 1);

        let loaded = store.load_session("overwrite-test").unwrap();
        assert_eq!(loaded.len(), 1);
    }
}
