//! Shared memory and knowledge base for team collaboration.
//!
//! Provides a shared knowledge store that team members can read from and
//! write to, enabling collective intelligence across agent sessions.
//! Includes shared memories, learned patterns, and team annotations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A shared knowledge entry visible to all team members.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedKnowledge {
    /// Unique entry ID.
    pub id: String,
    /// Title/summary.
    pub title: String,
    /// Detailed content.
    pub content: String,
    /// Category of knowledge.
    pub category: KnowledgeCategory,
    /// Who created this entry.
    pub author_id: String,
    /// When this entry was created.
    pub created_at: u64,
    /// When this entry was last updated.
    pub updated_at: u64,
    /// Tags for search/discovery.
    pub tags: Vec<String>,
    /// Whether this entry is pinned (always shown).
    pub pinned: bool,
    /// Access level.
    pub access: KnowledgeAccess,
    /// Number of times this entry has been viewed.
    pub view_count: u32,
}

/// Categories of shared knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeCategory {
    /// Architecture decisions and rationale.
    Architecture,
    /// Coding conventions and standards.
    Conventions,
    /// Known issues and workarounds.
    KnownIssues,
    /// How-to guides and recipes.
    Guides,
    /// Project-specific facts.
    ProjectFacts,
    /// Agent-specific learned patterns.
    AgentPatterns,
    /// General notes.
    Notes,
}

impl KnowledgeCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::Conventions => "conventions",
            Self::KnownIssues => "known_issues",
            Self::Guides => "guides",
            Self::ProjectFacts => "project_facts",
            Self::AgentPatterns => "agent_patterns",
            Self::Notes => "notes",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "architecture" => Some(Self::Architecture),
            "conventions" => Some(Self::Conventions),
            "known_issues" => Some(Self::KnownIssues),
            "guides" => Some(Self::Guides),
            "project_facts" => Some(Self::ProjectFacts),
            "agent_patterns" => Some(Self::AgentPatterns),
            "notes" => Some(Self::Notes),
            _ => None,
        }
    }
}

/// Access level for knowledge entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeAccess {
    /// Visible to all team members.
    Public,
    /// Visible only to editors and above.
    TeamOnly,
    /// Visible only to the author.
    Private,
}

/// A team annotation on a file or code region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamAnnotation {
    /// Unique annotation ID.
    pub id: String,
    /// File path this annotation applies to.
    pub file_path: String,
    /// Line number (or start of range).
    pub line: u32,
    /// Optional end line for range annotations.
    pub end_line: Option<u32>,
    /// Annotation content.
    pub content: String,
    /// Who created this annotation.
    pub author_id: String,
    /// When created.
    pub created_at: u64,
    /// Whether this annotation is resolved.
    pub resolved: bool,
    /// Annotation type.
    pub kind: AnnotationKind,
}

/// Type of team annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationKind {
    /// A note or comment.
    Note,
    /// A warning about potential issues.
    Warning,
    /// A TODO item.
    Todo,
    /// A question for the team.
    Question,
}

/// Manages the shared knowledge base and annotations.
#[derive(Debug, Clone, Default)]
pub struct SharedMemoryStore {
    /// Knowledge entries keyed by ID.
    pub entries: HashMap<String, SharedKnowledge>,
    /// Team annotations keyed by ID.
    pub annotations: HashMap<String, TeamAnnotation>,
    /// Auto-increment counter for IDs.
    next_id: u64,
}

impl SharedMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Knowledge Entries ──

    /// Add a new knowledge entry. Returns the entry ID.
    pub fn add_entry(
        &mut self,
        title: &str,
        content: &str,
        category: KnowledgeCategory,
        author_id: &str,
        tags: Vec<String>,
    ) -> String {
        let id = self.gen_id("kn");
        let now = now_secs();
        let entry = SharedKnowledge {
            id: id.clone(),
            title: title.to_string(),
            content: content.to_string(),
            category,
            author_id: author_id.to_string(),
            created_at: now,
            updated_at: now,
            tags,
            pinned: false,
            access: KnowledgeAccess::Public,
            view_count: 0,
        };
        self.entries.insert(id.clone(), entry);
        id
    }

    /// Update an existing entry's content.
    pub fn update_entry(&mut self, id: &str, content: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.content = content.to_string();
            entry.updated_at = now_secs();
            true
        } else {
            false
        }
    }

    /// Remove a knowledge entry.
    pub fn remove_entry(&mut self, id: &str) -> bool {
        self.entries.remove(id).is_some()
    }

    /// Pin/unpin an entry.
    pub fn set_pinned(&mut self, id: &str, pinned: bool) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.pinned = pinned;
        }
    }

    /// Record a view of an entry.
    pub fn record_view(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.view_count += 1;
        }
    }

    /// Search entries by tag.
    pub fn entries_by_tag(&self, tag: &str) -> Vec<&SharedKnowledge> {
        self.entries
            .values()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// Search entries by category.
    pub fn entries_by_category(&self, category: KnowledgeCategory) -> Vec<&SharedKnowledge> {
        self.entries
            .values()
            .filter(|e| e.category == category)
            .collect()
    }

    /// Get pinned entries.
    pub fn pinned_entries(&self) -> Vec<&SharedKnowledge> {
        self.entries.values().filter(|e| e.pinned).collect()
    }

    /// Search entries by keyword in title or content.
    pub fn search(&self, query: &str) -> Vec<&SharedKnowledge> {
        let q = query.to_lowercase();
        self.entries
            .values()
            .filter(|e| {
                e.title.to_lowercase().contains(&q)
                    || e.content.to_lowercase().contains(&q)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// Get entries visible to a specific user based on access level.
    pub fn visible_to(&self, user_id: &str) -> Vec<&SharedKnowledge> {
        self.entries
            .values()
            .filter(|e| {
                match e.access {
                    KnowledgeAccess::Public => true,
                    KnowledgeAccess::TeamOnly => true, // Simplified: all team members
                    KnowledgeAccess::Private => e.author_id == user_id,
                }
            })
            .collect()
    }

    // ── Annotations ──

    /// Add a team annotation. Returns the annotation ID.
    pub fn add_annotation(
        &mut self,
        file_path: &str,
        line: u32,
        end_line: Option<u32>,
        content: &str,
        author_id: &str,
        kind: AnnotationKind,
    ) -> String {
        let id = self.gen_id("ann");
        let annotation = TeamAnnotation {
            id: id.clone(),
            file_path: file_path.to_string(),
            line,
            end_line,
            content: content.to_string(),
            author_id: author_id.to_string(),
            created_at: now_secs(),
            resolved: false,
            kind,
        };
        self.annotations.insert(id.clone(), annotation);
        id
    }

    /// Resolve an annotation.
    pub fn resolve_annotation(&mut self, id: &str) -> bool {
        if let Some(ann) = self.annotations.get_mut(id) {
            ann.resolved = true;
            true
        } else {
            false
        }
    }

    /// Remove an annotation.
    pub fn remove_annotation(&mut self, id: &str) -> bool {
        self.annotations.remove(id).is_some()
    }

    /// Get annotations for a file.
    pub fn annotations_for_file(&self, file_path: &str) -> Vec<&TeamAnnotation> {
        self.annotations
            .values()
            .filter(|a| a.file_path == file_path)
            .collect()
    }

    /// Get unresolved annotations.
    pub fn unresolved_annotations(&self) -> Vec<&TeamAnnotation> {
        self.annotations.values().filter(|a| !a.resolved).collect()
    }

    /// Get unresolved annotations for a file.
    pub fn unresolved_for_file(&self, file_path: &str) -> Vec<&TeamAnnotation> {
        self.annotations
            .values()
            .filter(|a| a.file_path == file_path && !a.resolved)
            .collect()
    }

    // ── Persistence ──

    /// Save store to disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let state = PersistedSharedMemory {
            entries: self.entries.values().cloned().collect(),
            annotations: self.annotations.values().cloned().collect(),
        };
        let json =
            serde_json::to_vec_pretty(&state).map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("shared_memory.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load store from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut store = Self::new();
        let path = workspace_root.join(".velocity").join("shared_memory.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(state) = serde_json::from_slice::<PersistedSharedMemory>(&bytes) {
                for entry in state.entries {
                    store.entries.insert(entry.id.clone(), entry);
                }
                for ann in state.annotations {
                    store.annotations.insert(ann.id.clone(), ann);
                }
            }
        }
        store
    }

    fn gen_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{}_{}_{}", prefix, now_secs(), self.next_id)
    }
}

/// Serializable persistence for shared memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSharedMemory {
    entries: Vec<SharedKnowledge>,
    annotations: Vec<TeamAnnotation>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_search_entries() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_entry(
            "API Design",
            "We use REST for all external APIs",
            KnowledgeCategory::Conventions,
            "user1",
            vec!["api".to_string(), "rest".to_string()],
        );

        assert!(store.entries.contains_key(&id));
        assert_eq!(store.search("REST").len(), 1);
        assert_eq!(store.entries_by_tag("api").len(), 1);
        assert_eq!(
            store
                .entries_by_category(KnowledgeCategory::Conventions)
                .len(),
            1
        );
    }

    #[test]
    fn update_entry() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_entry("Title", "Original", KnowledgeCategory::Notes, "u1", vec![]);
        assert!(store.update_entry(&id, "Updated content"));
        assert_eq!(store.entries[&id].content, "Updated content");
    }

    #[test]
    fn remove_entry() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_entry("Title", "Content", KnowledgeCategory::Notes, "u1", vec![]);
        assert!(store.remove_entry(&id));
        assert!(store.entries.is_empty());
    }

    #[test]
    fn pin_entries() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_entry(
            "Important",
            "Content",
            KnowledgeCategory::Notes,
            "u1",
            vec![],
        );
        store.set_pinned(&id, true);
        assert_eq!(store.pinned_entries().len(), 1);
    }

    #[test]
    fn view_counting() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_entry("Title", "Content", KnowledgeCategory::Notes, "u1", vec![]);
        store.record_view(&id);
        store.record_view(&id);
        assert_eq!(store.entries[&id].view_count, 2);
    }

    #[test]
    fn private_entries_visibility() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_entry(
            "My Note",
            "Private",
            KnowledgeCategory::Notes,
            "user1",
            vec![],
        );
        store.entries.get_mut(&id).unwrap().access = KnowledgeAccess::Private;

        let visible = store.visible_to("user1");
        assert_eq!(visible.len(), 1);

        let not_visible = store.visible_to("user2");
        assert_eq!(not_visible.len(), 0);
    }

    #[test]
    fn add_and_query_annotations() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_annotation(
            "src/main.rs",
            42,
            Some(50),
            "This function needs refactoring",
            "user1",
            AnnotationKind::Todo,
        );

        assert_eq!(store.annotations_for_file("src/main.rs").len(), 1);
        assert_eq!(store.unresolved_annotations().len(), 1);
        assert_eq!(store.unresolved_for_file("src/main.rs").len(), 1);

        store.resolve_annotation(&id);
        assert_eq!(store.unresolved_annotations().len(), 0);
    }

    #[test]
    fn remove_annotation() {
        let mut store = SharedMemoryStore::new();
        let id = store.add_annotation("file.rs", 1, None, "Note", "u1", AnnotationKind::Note);
        assert!(store.remove_annotation(&id));
        assert!(store.annotations.is_empty());
    }

    #[test]
    fn knowledge_category_labels() {
        assert_eq!(KnowledgeCategory::Architecture.label(), "architecture");
        assert_eq!(KnowledgeCategory::AgentPatterns.label(), "agent_patterns");
        assert_eq!(
            KnowledgeCategory::from_label("known_issues"),
            Some(KnowledgeCategory::KnownIssues)
        );
        assert_eq!(KnowledgeCategory::from_label("invalid"), None);
    }

    #[test]
    fn search_by_multiple_tags() {
        let mut store = SharedMemoryStore::new();
        store.add_entry(
            "Multi",
            "Content",
            KnowledgeCategory::Notes,
            "u1",
            vec!["rust".to_string(), "web".to_string()],
        );

        assert_eq!(store.entries_by_tag("rust").len(), 1);
        assert_eq!(store.entries_by_tag("web").len(), 1);
        assert_eq!(store.entries_by_tag("python").len(), 0);
    }
}
