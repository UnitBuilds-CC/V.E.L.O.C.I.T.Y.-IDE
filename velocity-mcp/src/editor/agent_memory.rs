#![allow(dead_code)]
//! Agent Memory — persistent per-member knowledge store.
//!
//! Each team member accumulates learnings, patterns, and project knowledge
//! across sessions. Memories are stored as NDA-encrypted files per member ID.
//! The system loads them at session start and appends new learnings during
//! agent execution.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::nda::{decode_nda_text, encode_nda_text};

/// A single memory entry for a team member.
#[derive(Debug, Clone)]
pub struct AgentMemory {
    /// Unique ID for this memory entry.
    pub id: String,
    /// Short title summarizing the memory.
    pub title: String,
    /// Full content of the memory (knowledge, pattern, lesson).
    pub content: String,
    /// Unix timestamp when this memory was created.
    pub created_at: u64,
    /// Category: "pattern", "preference", "architecture", "lesson", "context".
    pub category: String,
    /// Relevance keywords for retrieval.
    pub keywords: Vec<String>,
}

impl AgentMemory {
    pub fn new(title: &str, content: &str, category: &str, keywords: Vec<&str>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            id: format!("mem_{:x}", now),
            title: title.to_string(),
            content: content.to_string(),
            created_at: now,
            category: category.to_string(),
            keywords: keywords.into_iter().map(String::from).collect(),
        }
    }
}

/// Per-member memory store.
#[derive(Debug, Clone, Default)]
pub struct MemberMemoryStore {
    /// Member ID this store belongs to.
    pub member_id: String,
    /// All memories for this member.
    pub memories: Vec<AgentMemory>,
}

impl MemberMemoryStore {
    pub fn new(member_id: &str) -> Self {
        Self {
            member_id: member_id.to_string(),
            memories: Vec::new(),
        }
    }

    /// Add a new memory entry.
    pub fn add(&mut self, memory: AgentMemory) {
        self.memories.push(memory);
    }

    /// Search memories by keyword overlap with query terms.
    pub fn search(&self, query: &str) -> Vec<&AgentMemory> {
        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();
        let mut scored: Vec<(usize, &AgentMemory)> = self
            .memories
            .iter()
            .map(|mem| {
                let score = terms
                    .iter()
                    .filter(|term| {
                        mem.keywords
                            .iter()
                            .any(|k| k.to_lowercase().contains(*term))
                            || mem.title.to_lowercase().contains(*term)
                            || mem.content.to_lowercase().contains(*term)
                    })
                    .count();
                (score, mem)
            })
            .filter(|(score, _)| *score > 0)
            .collect();
        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        scored.into_iter().map(|(_, mem)| mem).collect()
    }

    /// Get a memory injection prompt for the member's system prompt.
    /// Returns a formatted string of relevant memories or empty if none.
    pub fn inject_context(&self, task_description: &str) -> String {
        let relevant = self.search(task_description);
        if relevant.is_empty() {
            return String::new();
        }
        let entries: Vec<String> = relevant
            .iter()
            .take(5)
            .map(|mem| format!("- [{}] {}: {}", mem.category, mem.title, mem.content))
            .collect();
        format!(
            "\n\n<member_memory>\nRelevant knowledge from past sessions:\n{}\n</member_memory>\n",
            entries.join("\n")
        )
    }

    /// Number of stored memories.
    pub fn count(&self) -> usize {
        self.memories.len()
    }
}

/// Global agent memory manager — loads/saves all member stores.
#[derive(Debug, Clone, Default)]
pub struct AgentMemoryManager {
    /// Per-member stores indexed by member_id.
    pub stores: Vec<MemberMemoryStore>,
    /// Workspace root for persistence paths.
    workspace_root: PathBuf,
}

impl AgentMemoryManager {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            stores: Vec::new(),
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    /// Get or create a memory store for a specific member.
    pub fn get_or_create(&mut self, member_id: &str) -> &mut MemberMemoryStore {
        if let Some(pos) = self.stores.iter().position(|s| s.member_id == member_id) {
            &mut self.stores[pos]
        } else {
            self.stores.push(MemberMemoryStore::new(member_id));
            self.stores.last_mut().unwrap()
        }
    }

    /// Load all member memory stores from disk.
    pub fn load_all(&mut self) {
        let memory_dir = self.workspace_root.join(".velocity").join("agent_memory");
        if !memory_dir.exists() {
            return;
        }

        if let Ok(entries) = fs::read_dir(&memory_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("nda") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(bytes) = fs::read(&path) {
                            let plain = crate::agent::crypto::open(
                                &self.workspace_root,
                                b"agent_memory",
                                &bytes,
                            );
                            let content = String::from_utf8_lossy(&plain);
                            let store = parse_member_memory(stem, &content);
                            if !store.memories.is_empty() {
                                // Replace any existing store for this member so
                                // repeated loads never accumulate duplicates.
                                if let Some(pos) = self
                                    .stores
                                    .iter()
                                    .position(|s| s.member_id == store.member_id)
                                {
                                    self.stores[pos] = store;
                                } else {
                                    self.stores.push(store);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Save all member memory stores to disk.
    pub fn save_all(&self) {
        let memory_dir = self.workspace_root.join(".velocity").join("agent_memory");
        let _ = fs::create_dir_all(&memory_dir);

        for store in &self.stores {
            if store.memories.is_empty() {
                continue;
            }
            let serialized = serialize_member_memory(store);
            let nda_path = memory_dir.join(format!("{}.nda", store.member_id));
            let bytes = crate::agent::crypto::seal(
                &self.workspace_root,
                b"agent_memory",
                serialized.as_bytes(),
            )
            .unwrap_or_else(|| serialized.into_bytes());
            let _ = fs::write(nda_path, bytes);
        }
    }

    /// Save a single member's memory store.
    pub fn save_member(&self, member_id: &str) {
        let memory_dir = self.workspace_root.join(".velocity").join("agent_memory");
        let _ = fs::create_dir_all(&memory_dir);

        if let Some(store) = self.stores.iter().find(|s| s.member_id == member_id) {
            if store.memories.is_empty() {
                return;
            }
            let serialized = serialize_member_memory(store);
            let nda_path = memory_dir.join(format!("{}.nda", member_id));
            let bytes = crate::agent::crypto::seal(
                &self.workspace_root,
                b"agent_memory",
                serialized.as_bytes(),
            )
            .unwrap_or_else(|| serialized.into_bytes());
            let _ = fs::write(nda_path, bytes);
        }
    }

    /// Add a memory to a member and persist immediately.
    pub fn remember(&mut self, member_id: &str, memory: AgentMemory) {
        self.get_or_create(member_id).add(memory);
        self.save_member(member_id);
    }

    /// Get context injection for a member given a task.
    pub fn context_for(&self, member_id: &str, task: &str) -> String {
        self.stores
            .iter()
            .find(|s| s.member_id == member_id)
            .map(|s| s.inject_context(task))
            .unwrap_or_default()
    }
}

/// Serialize a member's memories into NDA text format.
fn serialize_member_memory(store: &MemberMemoryStore) -> String {
    let mut lines = vec![
        "agent-memory version 1".to_string(),
        format!("member_id\t{}", encode_nda_text(&store.member_id)),
        format!("memory_count\t{}", store.memories.len()),
    ];
    for (i, mem) in store.memories.iter().enumerate() {
        lines.push(format!("memory\t{}\tid\t{}", i, encode_nda_text(&mem.id)));
        lines.push(format!(
            "memory\t{}\ttitle\t{}",
            i,
            encode_nda_text(&mem.title)
        ));
        lines.push(format!(
            "memory\t{}\tcontent\t{}",
            i,
            encode_nda_text(&mem.content)
        ));
        lines.push(format!("memory\t{}\tcreated_at\t{}", i, mem.created_at));
        lines.push(format!(
            "memory\t{}\tcategory\t{}",
            i,
            encode_nda_text(&mem.category)
        ));
        for kw in &mem.keywords {
            lines.push(format!("memory_kw\t{}\t{}", i, encode_nda_text(kw)));
        }
    }
    lines.join("\n") + "\n"
}

/// Parse a member's memories from NDA text format.
fn parse_member_memory(member_id: &str, text: &str) -> MemberMemoryStore {
    if !text.trim_start().starts_with("agent-memory version 1") {
        return MemberMemoryStore::new(member_id);
    }

    #[derive(Default)]
    struct MemBuilder {
        id: String,
        title: String,
        content: String,
        created_at: u64,
        category: String,
        keywords: Vec<String>,
    }

    let mut memories: std::collections::BTreeMap<usize, MemBuilder> =
        std::collections::BTreeMap::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("memory\t") {
            let parts: Vec<&str> = rest.splitn(3, '\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(idx) = parts[0].parse::<usize>() else {
                continue;
            };
            let field = parts[1];
            let value = parts[2];
            let mem = memories.entry(idx).or_default();
            match field {
                "id" => mem.id = decode_nda_text(value),
                "title" => mem.title = decode_nda_text(value),
                "content" => mem.content = decode_nda_text(value),
                "created_at" => mem.created_at = value.trim().parse().unwrap_or(0),
                "category" => mem.category = decode_nda_text(value),
                _ => {}
            }
        } else if let Some(rest) = line.strip_prefix("memory_kw\t") {
            let parts: Vec<&str> = rest.splitn(2, '\t').collect();
            if parts.len() != 2 {
                continue;
            }
            let Ok(idx) = parts[0].parse::<usize>() else {
                continue;
            };
            let kw = decode_nda_text(parts[1]);
            memories.entry(idx).or_default().keywords.push(kw);
        }
    }

    let mut store = MemberMemoryStore::new(member_id);
    for mem in memories.into_values() {
        store.memories.push(AgentMemory {
            id: mem.id,
            title: mem.title,
            content: mem.content,
            created_at: mem.created_at,
            category: mem.category,
            keywords: mem.keywords,
        });
    }
    store
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_creation() {
        let mem = AgentMemory::new(
            "Auth pattern",
            "Use JWT with refresh tokens for API auth",
            "pattern",
            vec!["auth", "jwt", "api"],
        );
        assert_eq!(mem.title, "Auth pattern");
        assert_eq!(mem.category, "pattern");
        assert_eq!(mem.keywords.len(), 3);
    }

    #[test]
    fn store_search_finds_relevant() {
        let mut store = MemberMemoryStore::new("test_member");
        store.add(AgentMemory::new(
            "Database pattern",
            "Always use connection pooling with max 20 connections",
            "pattern",
            vec!["database", "pool", "connection"],
        ));
        store.add(AgentMemory::new(
            "Auth preference",
            "Use OAuth2 PKCE flow for SPAs",
            "preference",
            vec!["auth", "oauth", "spa"],
        ));

        let results = store.search("database connection");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Database pattern");
    }

    #[test]
    fn serialize_roundtrip() {
        let mut store = MemberMemoryStore::new("member_lead");
        store.add(AgentMemory::new(
            "Test memory",
            "Content with special\tchars\nand newlines",
            "lesson",
            vec!["test", "special"],
        ));

        let serialized = serialize_member_memory(&store);
        let parsed = parse_member_memory("member_lead", &serialized);
        assert_eq!(parsed.memories.len(), 1);
        assert_eq!(parsed.memories[0].title, "Test memory");
        assert_eq!(parsed.memories[0].keywords.len(), 2);
    }

    #[test]
    fn context_injection_with_no_memories_is_empty() {
        let store = MemberMemoryStore::new("empty");
        assert!(store.inject_context("anything").is_empty());
    }

    #[test]
    fn context_injection_formats_memories() {
        let mut store = MemberMemoryStore::new("test");
        store.add(AgentMemory::new(
            "Naming convention",
            "Use snake_case for all Rust identifiers",
            "pattern",
            vec!["naming", "rust", "convention"],
        ));
        let ctx = store.inject_context("rust naming");
        assert!(ctx.contains("<member_memory>"));
        assert!(ctx.contains("snake_case"));
    }

    #[test]
    fn manager_round_trip_is_durable_across_sessions() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut mgr = AgentMemoryManager::new(dir.path());
            mgr.remember(
                "member_a",
                AgentMemory::new(
                    "Deploy lesson",
                    "Always run migrations before deploy",
                    "lesson",
                    vec!["deploy", "migration"],
                ),
            );
            mgr.save_all();
        }
        // A fresh manager (new session) must reload the persisted memory.
        let mut mgr2 = AgentMemoryManager::new(dir.path());
        mgr2.load_all();
        let ctx = mgr2.context_for("member_a", "deploy migration");
        assert!(ctx.contains("migrations before deploy"));
    }

    #[test]
    fn manager_load_all_does_not_duplicate_stores() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut mgr = AgentMemoryManager::new(dir.path());
            mgr.remember(
                "member_b",
                AgentMemory::new("Pattern", "Use connection pooling", "pattern", vec!["db"]),
            );
            mgr.save_all();
        }
        let mut mgr2 = AgentMemoryManager::new(dir.path());
        mgr2.load_all();
        mgr2.load_all(); // repeated load must not accumulate duplicates
        let count = mgr2
            .stores
            .iter()
            .filter(|s| s.member_id == "member_b")
            .count();
        assert_eq!(count, 1);
    }
}
