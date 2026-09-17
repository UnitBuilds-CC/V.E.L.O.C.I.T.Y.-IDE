//! Session memory with knowledge persistence for agent context across sessions.
//!
//! Provides [`SessionMemory`], a keyword-searchable, importance-weighted memory
//! store that agents can use to remember facts, decisions, code patterns, and
//! user preferences within a session and persist them across restarts via JSON
//! serialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

// ---------------------------------------------------------------------------
// MemoryKind
// ---------------------------------------------------------------------------

/// Categorises what type of knowledge a [`MemoryEntry`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryKind {
    /// A general factual statement.
    Fact,
    /// Context about a specific file or source range.
    FileContext,
    /// A decision that was made (and why).
    Decision,
    /// An error or failure the agent encountered.
    Error,
    /// A user preference or configuration choice.
    UserPreference,
    /// A recurring code pattern or idiom.
    CodePattern,
}

// ---------------------------------------------------------------------------
// MemoryEntry
// ---------------------------------------------------------------------------

/// A single piece of knowledge remembered by the agent.
///
/// `created` and `last_accessed` are stored as `SystemTime` (serialisable as
/// UNIX-epoch durations) rather than `Instant` so that the full struct can be
/// round-tripped through JSON for cross-session persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Monotonically-increasing identifier within a session.
    pub id: u64,
    /// What kind of knowledge this entry represents.
    pub kind: MemoryKind,
    /// Human-readable content of the memory.
    pub content: String,
    /// Optional source file that this memory relates to.
    pub source_file: Option<String>,
    /// Importance weight in the range `0.0..=1.0`.
    pub importance: f64,
    /// When the entry was created (serialisation-safe).
    pub created: SystemTime,
    /// When the entry was last accessed / recalled.
    pub last_accessed: SystemTime,
    /// How many times this entry has been recalled.
    pub access_count: u32,
    /// Free-form tags for filtering.
    pub tags: Vec<String>,
}

impl MemoryEntry {
    /// Age of this entry relative to now.
    pub fn age(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.created)
            .unwrap_or(Duration::ZERO)
    }

    /// Duration since this entry was last accessed.
    pub fn idle_time(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.last_accessed)
            .unwrap_or(Duration::ZERO)
    }
}

// ---------------------------------------------------------------------------
// MemoryStats
// ---------------------------------------------------------------------------

/// Summary statistics about the current contents of a [`SessionMemory`].
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Total number of entries currently stored.
    pub total_entries: usize,
    /// Breakdown of entry count by [`MemoryKind`].
    pub by_kind: HashMap<MemoryKind, usize>,
    /// Age of the oldest entry in seconds (0 when empty).
    pub oldest_entry_age_secs: u64,
    /// Mean importance across all entries (0.0 when empty).
    pub avg_importance: f64,
}

// ---------------------------------------------------------------------------
// SessionMemory
// ---------------------------------------------------------------------------

/// In-session knowledge store with keyword-based recall and JSON persistence.
///
/// # Capacity management
///
/// When the number of entries exceeds `max_entries` the [`consolidate`]
/// method prunes low-importance, stale entries to make room.
///
/// # Persistence
///
/// Call [`to_json`] / [`from_json`] to serialise the full memory to / from a
/// JSON string suitable for writing to disk.
pub struct SessionMemory {
    /// All remembered entries, keyed by insertion order.
    entries: Vec<MemoryEntry>,
    /// Maximum number of entries before consolidation kicks in.
    max_entries: usize,
    /// Opaque session identifier (e.g. a UUID or timestamp string).
    session_id: String,
    /// Counter for generating unique entry IDs.
    next_id: u64,
}

impl SessionMemory {
    /// Default capacity.
    const DEFAULT_MAX_ENTRIES: usize = 1000;

    /// Create a new, empty session memory.
    pub fn new(session_id: String) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: Self::DEFAULT_MAX_ENTRIES,
            session_id,
            next_id: 1,
        }
    }

    // -- mutators -----------------------------------------------------------

    /// Store a new piece of knowledge and return its unique ID.
    ///
    /// `importance` is clamped to `0.0..=1.0`.  If the store is at capacity
    /// [`consolidate`] is called automatically before inserting.
    pub fn remember(
        &mut self,
        kind: MemoryKind,
        content: String,
        importance: f64,
        tags: Vec<String>,
    ) -> u64 {
        if self.entries.len() >= self.max_entries {
            self.consolidate();
        }

        let now = SystemTime::now();
        let id = self.next_id;
        self.next_id += 1;

        let entry = MemoryEntry {
            id,
            kind,
            content,
            source_file: None,
            importance: importance.clamp(0.0, 1.0),
            created: now,
            last_accessed: now,
            access_count: 0,
            tags,
        };
        self.entries.push(entry);
        id
    }

    /// Store a new piece of knowledge linked to a source file.
    pub fn remember_with_file(
        &mut self,
        kind: MemoryKind,
        content: String,
        importance: f64,
        source_file: String,
        tags: Vec<String>,
    ) -> u64 {
        let id = self.remember(kind, content, importance, tags);
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.source_file = Some(source_file);
        }
        id
    }

    /// Remove an entry by ID.  No-op if the ID is not found.
    pub fn forget(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    /// Prune low-importance, stale entries when at capacity.
    ///
    /// Strategy:
    /// 1. Remove entries with `importance < 0.2` that are older than 1 hour.
    /// 2. If still at capacity, remove the lowest-importance half.
    pub fn consolidate(&mut self) {
        let one_hour = Duration::from_secs(3600);
        let now = SystemTime::now();

        // Phase 1: drop low-importance, old entries.
        self.entries.retain(|e| {
            let age = now.duration_since(e.created).unwrap_or(Duration::ZERO);
            !(e.importance < 0.2 && age > one_hour)
        });

        // Phase 2: if still at/over capacity, sort by importance and keep the
        // top half (leaving room for at least one new insertion).
        if self.entries.len() >= self.max_entries {
            self.entries.sort_by(|a, b| {
                b.importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let keep = (self.max_entries / 2).min(self.max_entries.saturating_sub(1));
            self.entries.truncate(keep);
        }
    }

    // -- queries ------------------------------------------------------------

    /// Recall the most relevant entries matching `query` via keyword overlap.
    ///
    /// Scoring:
    /// - For each entry, compute the Jaccard-like keyword overlap with the
    ///   query (intersection / union of lowercased word sets).
    /// - Multiply by `importance` and a recency decay factor.
    /// - Return up to `max_results` entries sorted by descending score.
    pub fn recall(&mut self, query: &str, max_results: usize) -> Vec<&MemoryEntry> {
        let query_words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
            })
            .filter(|w| !w.is_empty())
            .collect();

        if query_words.is_empty() {
            return Vec::new();
        }

        let now = SystemTime::now();

        // Score every entry.
        let mut scored: Vec<(usize, f64)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let entry_words: std::collections::HashSet<String> = entry
                    .content
                    .to_lowercase()
                    .split_whitespace()
                    .map(|w| {
                        w.chars()
                            .filter(|c| c.is_alphanumeric())
                            .collect::<String>()
                    })
                    .filter(|w| !w.is_empty())
                    .collect();

                let query_set: std::collections::HashSet<&str> =
                    query_words.iter().map(|s| s.as_str()).collect();

                let intersection = entry_words
                    .iter()
                    .filter(|w| query_set.contains(w.as_str()))
                    .count() as f64;
                let union = entry_words.len() as f64 + query_set.len() as f64 - intersection;
                let keyword_score = if union > 0.0 {
                    intersection / union
                } else {
                    0.0
                };

                // Recency: exponential decay with half-life of 24 hours.
                let age_secs = now
                    .duration_since(entry.last_accessed)
                    .unwrap_or(Duration::ZERO)
                    .as_secs_f64();
                let recency = (-age_secs * std::f64::consts::LN_2 / 86400.0).exp();

                let score = keyword_score * entry.importance.max(0.01) * recency;
                (idx, score)
            })
            .collect();

        // Sort by descending score.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top N and bump access counters.
        let top: Vec<usize> = scored
            .iter()
            .take(max_results)
            .map(|(idx, _)| *idx)
            .collect();
        for &idx in &top {
            self.entries[idx].access_count += 1;
            self.entries[idx].last_accessed = SystemTime::now();
        }

        top.iter().map(|&idx| &self.entries[idx]).collect()
    }

    /// Recall entries filtered to a specific [`MemoryKind`].
    pub fn recall_by_kind(&mut self, kind: MemoryKind, max_results: usize) -> Vec<&MemoryEntry> {
        let mut matches: Vec<&MemoryEntry> =
            self.entries.iter().filter(|e| e.kind == kind).collect();
        // Sort by importance descending.
        matches.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(max_results);
        matches
    }

    /// Recall entries that contain **all** of the given `tags`.
    pub fn recall_by_tags(&self, tags: &[&str], max_results: usize) -> Vec<&MemoryEntry> {
        let mut matches: Vec<&MemoryEntry> = self
            .entries
            .iter()
            .filter(|e| tags.iter().all(|t| e.tags.iter().any(|et| et == t)))
            .collect();
        matches.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(max_results);
        matches
    }

    /// Borrow an entry by ID.
    pub fn get(&self, id: u64) -> Option<&MemoryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The session ID this memory belongs to.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Maximum entry capacity.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Set a custom capacity limit.
    pub fn set_max_entries(&mut self, max: usize) {
        self.max_entries = max;
    }

    // -- stats --------------------------------------------------------------

    /// Compute summary statistics.
    pub fn stats(&self) -> MemoryStats {
        let now = SystemTime::now();
        let total = self.entries.len();

        let mut by_kind: HashMap<MemoryKind, usize> = HashMap::new();
        let mut oldest_age = Duration::ZERO;
        let mut importance_sum = 0.0;

        for e in &self.entries {
            *by_kind.entry(e.kind).or_default() += 1;
            let age = now.duration_since(e.created).unwrap_or(Duration::ZERO);
            if age > oldest_age {
                oldest_age = age;
            }
            importance_sum += e.importance;
        }

        MemoryStats {
            total_entries: total,
            by_kind,
            oldest_entry_age_secs: oldest_age.as_secs(),
            avg_importance: if total > 0 {
                importance_sum / total as f64
            } else {
                0.0
            },
        }
    }

    // -- persistence --------------------------------------------------------

    /// Serialise the full memory to a JSON string.
    pub fn to_json(&self) -> String {
        let snapshot = SessionSnapshot {
            session_id: self.session_id.clone(),
            max_entries: self.max_entries,
            next_id: self.next_id,
            entries: self.entries.clone(),
        };
        serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
    }

    /// Deserialise from a JSON string produced by [`to_json`].
    pub fn from_json(json: &str) -> Self {
        match serde_json::from_str::<SessionSnapshot>(json) {
            Ok(snap) => Self {
                entries: snap.entries,
                max_entries: snap.max_entries,
                session_id: snap.session_id,
                next_id: snap.next_id,
            },
            Err(_) => Self::new("recovered".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Serde helper – mirrors SessionMemory fields for JSON round-tripping.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct SessionSnapshot {
    session_id: String,
    max_entries: usize,
    next_id: u64,
    entries: Vec<MemoryEntry>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn make_memory() -> SessionMemory {
        SessionMemory::new("test-session".to_string())
    }

    // -- basic remember / recall --------------------------------------------

    #[test]
    fn test_remember_returns_incrementing_ids() {
        let mut mem = make_memory();
        let id1 = mem.remember(MemoryKind::Fact, "a".into(), 0.5, vec![]);
        let id2 = mem.remember(MemoryKind::Fact, "b".into(), 0.5, vec![]);
        assert!(id2 > id1);
    }

    #[test]
    fn test_remember_and_recall_basic() {
        let mut mem = make_memory();
        mem.remember(
            MemoryKind::Fact,
            "Rust is a systems programming language".into(),
            0.8,
            vec!["rust".into()],
        );
        let results = mem.recall("Rust language", 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
    }

    #[test]
    fn test_recall_keyword_accuracy() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "The quick brown fox".into(), 0.5, vec![]);
        mem.remember(MemoryKind::Fact, "A lazy dog sleeps".into(), 0.5, vec![]);
        mem.remember(
            MemoryKind::Fact,
            "Fox and hound adventure".into(),
            0.5,
            vec![],
        );

        let results = mem.recall("fox", 10);
        // Both entries mentioning "fox" should rank higher than the dog one.
        assert!(results.len() >= 2);
        assert!(results[0].content.to_lowercase().contains("fox"));
    }

    #[test]
    fn test_importance_weighted_recall() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "alpha beta gamma".into(), 0.1, vec![]);
        mem.remember(MemoryKind::Fact, "alpha beta gamma".into(), 1.0, vec![]);
        let results = mem.recall("alpha beta", 2);
        assert_eq!(results.len(), 2);
        // The higher-importance entry should be first.
        assert!(results[0].importance > results[1].importance);
    }

    #[test]
    fn test_recall_empty_query_returns_nothing() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "something".into(), 0.5, vec![]);
        let results = mem.recall("", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_recall_on_empty_memory() {
        let mut mem = make_memory();
        let results = mem.recall("anything", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_max_results_respected() {
        let mut mem = make_memory();
        for i in 0..20 {
            mem.remember(
                MemoryKind::Fact,
                format!("entry number {i} with keyword"),
                0.5,
                vec![],
            );
        }
        let results = mem.recall("keyword", 5);
        assert_eq!(results.len(), 5);
    }

    // -- forget -------------------------------------------------------------

    #[test]
    fn test_forget_removes_entry() {
        let mut mem = make_memory();
        let id = mem.remember(MemoryKind::Fact, "to forget".into(), 0.5, vec![]);
        assert_eq!(mem.len(), 1);
        mem.forget(id);
        assert_eq!(mem.len(), 0);
    }

    #[test]
    fn test_forget_nonexistent_is_noop() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "keep".into(), 0.5, vec![]);
        mem.forget(9999);
        assert_eq!(mem.len(), 1);
    }

    // -- consolidation ------------------------------------------------------

    #[test]
    fn test_consolidation_removes_low_importance_old_entries() {
        let mut mem = make_memory();
        mem.set_max_entries(4);

        // Insert entries with very low importance.
        for i in 0..5 {
            mem.remember(
                MemoryKind::Fact,
                format!("low importance entry {i}"),
                0.05,
                vec![],
            );
        }
        // After consolidation the store should be under capacity.
        assert!(mem.len() <= mem.max_entries());
    }

    #[test]
    fn test_consolidation_keeps_high_importance() {
        let mut mem = make_memory();
        mem.set_max_entries(4);

        for i in 0..6 {
            mem.remember(
                MemoryKind::Decision,
                format!("important decision {i}"),
                0.9,
                vec![],
            );
        }
        // High-importance entries should survive (they get halved but not
        // removed by phase 1).
        assert!(!mem.is_empty());
        assert!(mem.len() <= mem.max_entries());
    }

    // -- JSON round-trip ----------------------------------------------------

    #[test]
    fn test_json_serialization_roundtrip() {
        let mut mem = make_memory();
        mem.remember(
            MemoryKind::CodePattern,
            "Use RAII for resource management".into(),
            0.9,
            vec!["rust".into(), "raii".into()],
        );
        mem.remember(
            MemoryKind::UserPreference,
            "User prefers dark theme".into(),
            0.7,
            vec!["ui".into()],
        );

        let json = mem.to_json();
        let restored = SessionMemory::from_json(&json);

        assert_eq!(restored.len(), 2);
        assert_eq!(restored.session_id(), "test-session");
        assert_eq!(
            restored.entries[0].content,
            "Use RAII for resource management"
        );
        assert_eq!(restored.entries[1].kind, MemoryKind::UserPreference);
    }

    #[test]
    fn test_from_json_invalid_returns_empty() {
        let mem = SessionMemory::from_json("not valid json{{{");
        assert!(mem.is_empty());
        assert_eq!(mem.session_id(), "recovered");
    }

    // -- capacity enforcement -----------------------------------------------

    #[test]
    fn test_capacity_enforcement_triggers_consolidation() {
        let mut mem = make_memory();
        mem.set_max_entries(5);

        for i in 0..10 {
            mem.remember(MemoryKind::Fact, format!("entry {i}"), 0.5, vec![]);
        }
        assert!(mem.len() <= mem.max_entries());
    }

    // -- tag-based filtering ------------------------------------------------

    #[test]
    fn test_recall_by_tags_single() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "a".into(), 0.5, vec!["rust".into()]);
        mem.remember(MemoryKind::Fact, "b".into(), 0.5, vec!["python".into()]);
        mem.remember(
            MemoryKind::Fact,
            "c".into(),
            0.5,
            vec!["rust".into(), "web".into()],
        );

        let results = mem.recall_by_tags(&["rust"], 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_recall_by_tags_multiple() {
        let mut mem = make_memory();
        mem.remember(
            MemoryKind::Fact,
            "a".into(),
            0.5,
            vec!["rust".into(), "web".into()],
        );
        mem.remember(MemoryKind::Fact, "b".into(), 0.5, vec!["rust".into()]);

        let results = mem.recall_by_tags(&["rust", "web"], 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "a");
    }

    #[test]
    fn test_recall_by_tags_empty_returns_all() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "x".into(), 0.5, vec![]);
        mem.remember(MemoryKind::Fact, "y".into(), 0.5, vec![]);
        // No tag filter → everything matches.
        let results = mem.recall_by_tags(&[], 10);
        assert_eq!(results.len(), 2);
    }

    // -- recall_by_kind -----------------------------------------------------

    #[test]
    fn test_recall_by_kind() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Error, "segfault in parser".into(), 0.8, vec![]);
        mem.remember(
            MemoryKind::Fact,
            "parser is in parser.rs".into(),
            0.5,
            vec![],
        );
        mem.remember(
            MemoryKind::Error,
            "null pointer in lexer".into(),
            0.6,
            vec![],
        );

        let errors = mem.recall_by_kind(MemoryKind::Error, 10);
        assert_eq!(errors.len(), 2);
        // Higher importance first.
        assert!(errors[0].importance >= errors[1].importance);
    }

    // -- stats --------------------------------------------------------------

    #[test]
    fn test_stats_empty() {
        let mem = make_memory();
        let s = mem.stats();
        assert_eq!(s.total_entries, 0);
        assert_eq!(s.oldest_entry_age_secs, 0);
        assert_eq!(s.avg_importance, 0.0);
    }

    #[test]
    fn test_stats_populated() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "a".into(), 0.4, vec![]);
        mem.remember(MemoryKind::Error, "b".into(), 0.8, vec![]);
        mem.remember(MemoryKind::Fact, "c".into(), 0.6, vec![]);

        let s = mem.stats();
        assert_eq!(s.total_entries, 3);
        assert_eq!(s.by_kind.get(&MemoryKind::Fact), Some(&2));
        assert_eq!(s.by_kind.get(&MemoryKind::Error), Some(&1));
        let expected_avg = (0.4 + 0.8 + 0.6) / 3.0;
        assert!((s.avg_importance - expected_avg).abs() < 1e-9);
    }

    // -- get ---------------------------------------------------------------

    #[test]
    fn test_get_existing() {
        let mut mem = make_memory();
        let id = mem.remember(MemoryKind::Decision, "use async".into(), 0.7, vec![]);
        let entry = mem.get(id).unwrap();
        assert_eq!(entry.content, "use async");
    }

    #[test]
    fn test_get_missing() {
        let mem = make_memory();
        assert!(mem.get(42).is_none());
    }

    // -- remember_with_file ------------------------------------------------

    #[test]
    fn test_remember_with_file() {
        let mut mem = make_memory();
        let id = mem.remember_with_file(
            MemoryKind::FileContext,
            "main loop is here".into(),
            0.9,
            "src/main.rs".into(),
            vec!["entry".into()],
        );
        let entry = mem.get(id).unwrap();
        assert_eq!(entry.source_file.as_deref(), Some("src/main.rs"));
    }

    // -- access_count bumped on recall -------------------------------------

    #[test]
    fn test_recall_bumps_access_count() {
        let mut mem = make_memory();
        mem.remember(MemoryKind::Fact, "recalled fact".into(), 0.5, vec![]);
        let _ = mem.recall("recalled", 10);
        let entry = mem
            .entries
            .iter()
            .find(|e| e.content == "recalled fact")
            .unwrap();
        assert_eq!(entry.access_count, 1);
    }

    // -- importance clamping -----------------------------------------------

    #[test]
    fn test_importance_clamped() {
        let mut mem = make_memory();
        let id_over = mem.remember(MemoryKind::Fact, "over".into(), 5.0, vec![]);
        let id_under = mem.remember(MemoryKind::Fact, "under".into(), -1.0, vec![]);
        assert!((mem.get(id_over).unwrap().importance - 1.0).abs() < f64::EPSILON);
        assert!((mem.get(id_under).unwrap().importance - 0.0).abs() < f64::EPSILON);
    }

    // -- len / is_empty ----------------------------------------------------

    #[test]
    fn test_len_and_is_empty() {
        let mut mem = make_memory();
        assert!(mem.is_empty());
        assert_eq!(mem.len(), 0);
        mem.remember(MemoryKind::Fact, "x".into(), 0.5, vec![]);
        assert!(!mem.is_empty());
        assert_eq!(mem.len(), 1);
    }

    // -- MemoryKind serialisation round-trip --------------------------------

    #[test]
    fn test_memory_kind_serde_roundtrip() {
        let kinds = vec![
            MemoryKind::Fact,
            MemoryKind::FileContext,
            MemoryKind::Decision,
            MemoryKind::Error,
            MemoryKind::UserPreference,
            MemoryKind::CodePattern,
        ];
        for k in kinds {
            let json = serde_json::to_string(&k).unwrap();
            let back: MemoryKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, k);
        }
    }
}
