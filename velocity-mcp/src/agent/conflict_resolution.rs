//! Conflict resolution for concurrent agent and user actions.
//!
//! When multiple users or agents operate on the same resources simultaneously,
//! conflicts can arise. This module provides detection, tracking, and
//! resolution strategies for these conflicts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A conflict between concurrent operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationConflict {
    /// Unique conflict ID.
    pub id: String,
    /// The resource that conflicted (e.g., file path, workflow ID).
    pub resource: String,
    /// The type of resource.
    pub resource_type: ResourceType,
    /// First operation.
    pub op_a: Operation,
    /// Second (conflicting) operation.
    pub op_b: Operation,
    /// When the conflict was detected.
    pub detected_at: u64,
    /// Resolution strategy applied.
    pub resolution: Option<Resolution>,
    /// When the conflict was resolved.
    pub resolved_at: Option<u64>,
}

/// Type of resource involved in a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    File,
    Workflow,
    AgentSession,
    KnowledgeEntry,
    Configuration,
}

impl ResourceType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Workflow => "workflow",
            Self::AgentSession => "session",
            Self::KnowledgeEntry => "knowledge",
            Self::Configuration => "config",
        }
    }
}

/// An operation that may conflict with another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Who performed this operation.
    pub actor_id: String,
    /// What kind of operation.
    pub kind: OperationKind,
    /// When the operation was performed.
    pub timestamp: u64,
    /// Operation details (serialized).
    pub payload: serde_json::Value,
}

/// Kind of operation that can conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationKind {
    Create,
    Read,
    Update,
    Delete,
    Execute,
}

impl OperationKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Read => "read",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Execute => "execute",
        }
    }
}

/// How to resolve a conflict.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Resolution {
    /// Keep the first operation's result.
    KeepFirst,
    /// Keep the second operation's result.
    KeepSecond,
    /// Keep the most recent operation.
    KeepLatest,
    /// Merge both operations (if possible).
    Merge,
    /// Discard both and start over.
    DiscardBoth,
    /// Manual resolution required.
    Manual,
}

impl Default for Resolution {
    fn default() -> Self {
        Self::KeepLatest
    }
}

impl Resolution {
    pub fn label(&self) -> &'static str {
        match self {
            Self::KeepFirst => "keep_first",
            Self::KeepSecond => "keep_second",
            Self::KeepLatest => "keep_latest",
            Self::Merge => "merge",
            Self::DiscardBoth => "discard_both",
            Self::Manual => "manual",
        }
    }
}

/// A lock on a resource held by an actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLock {
    /// The resource being locked.
    pub resource: String,
    /// Who holds the lock.
    pub holder_id: String,
    /// When the lock was acquired.
    pub acquired_at: u64,
    /// When the lock expires (0 = no expiry).
    pub expires_at: u64,
    /// Lock type.
    pub kind: LockKind,
}

/// Type of lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockKind {
    /// Exclusive lock — no other locks allowed.
    Exclusive,
    /// Shared lock — other shared locks allowed, exclusive blocked.
    Shared,
}

/// Manages conflict detection, locks, and resolution.
#[derive(Debug, Clone, Default)]
pub struct ConflictResolver {
    /// Active resource locks.
    pub locks: HashMap<String, Vec<ResourceLock>>,
    /// Conflict history.
    pub conflicts: Vec<OperationConflict>,
    /// Maximum conflicts to retain.
    pub max_conflicts: usize,
    /// Default resolution strategy.
    pub default_resolution: Resolution,
    /// Auto-lock timeout (seconds). Locks older than this are expired.
    pub lock_timeout_secs: u64,
}

impl ConflictResolver {
    pub fn new() -> Self {
        Self {
            locks: HashMap::new(),
            conflicts: Vec::new(),
            max_conflicts: 200,
            default_resolution: Resolution::KeepLatest,
            lock_timeout_secs: 300, // 5 minutes
        }
    }

    // ── Locking ──

    /// Try to acquire a lock on a resource.
    pub fn try_lock(
        &mut self,
        resource: &str,
        actor_id: &str,
        kind: LockKind,
    ) -> Result<(), LockError> {
        self.expire_old_locks();

        let now = now_secs();
        let expires_at = now + self.lock_timeout_secs;

        let existing = self.locks.get(resource);

        match (kind, existing) {
            // Exclusive lock: fails if any lock exists.
            (LockKind::Exclusive, Some(locks)) if !locks.is_empty() => {
                // If the same actor already holds it, that's OK.
                if locks.iter().any(|l| l.holder_id == actor_id) {
                    return Ok(());
                }
                Err(LockError::AlreadyLocked {
                    resource: resource.to_string(),
                    holder: locks[0].holder_id.clone(),
                })
            }
            // Shared lock: fails if an exclusive lock exists.
            (LockKind::Shared, Some(locks)) if locks.iter().any(|l| l.kind == LockKind::Exclusive) => {
                Err(LockError::AlreadyLocked {
                    resource: resource.to_string(),
                    holder: locks[0].holder_id.clone(),
                })
            }
            // No conflicts — acquire the lock.
            _ => {
                let lock = ResourceLock {
                    resource: resource.to_string(),
                    holder_id: actor_id.to_string(),
                    acquired_at: now,
                    expires_at,
                    kind,
                };
                self.locks.entry(resource.to_string())
                    .or_default()
                    .push(lock);
                Ok(())
            }
        }
    }

    /// Release a lock held by an actor.
    pub fn unlock(&mut self, resource: &str, actor_id: &str) -> bool {
        if let Some(locks) = self.locks.get_mut(resource) {
            let before = locks.len();
            locks.retain(|l| l.holder_id != actor_id);
            let after = locks.len();
            if locks.is_empty() {
                self.locks.remove(resource);
            }
            before != after
        } else {
            false
        }
    }

    /// Check if a resource is locked.
    pub fn is_locked(&self, resource: &str) -> bool {
        self.locks.get(resource).map(|l| !l.is_empty()).unwrap_or(false)
    }

    /// Get who holds the lock on a resource.
    pub fn lock_holder(&self, resource: &str) -> Option<&str> {
        self.locks.get(resource)
            .and_then(|locks| locks.first())
            .map(|l| l.holder_id.as_str())
    }

    /// Expire locks that have timed out.
    pub fn expire_old_locks(&mut self) {
        let now = now_secs();
        for locks in self.locks.values_mut() {
            locks.retain(|l| l.expires_at == 0 || l.expires_at > now);
        }
        self.locks.retain(|_, locks| !locks.is_empty());
    }

    // ── Conflict Detection ──

    /// Detect and record a conflict between two operations.
    pub fn record_conflict(
        &mut self,
        resource: &str,
        resource_type: ResourceType,
        op_a: Operation,
        op_b: Operation,
    ) -> String {
        let id = format!("conflict_{}_{}", now_secs(), self.conflicts.len());
        self.conflicts.push(OperationConflict {
            id: id.clone(),
            resource: resource.to_string(),
            resource_type,
            op_a,
            op_b,
            detected_at: now_secs(),
            resolution: None,
            resolved_at: None,
        });
        while self.conflicts.len() > self.max_conflicts {
            self.conflicts.remove(0);
        }
        id
    }

    /// Check if two operations conflict based on their kinds.
    pub fn operations_conflict(a: &OperationKind, b: &OperationKind) -> bool {
        match (a, b) {
            // Two reads never conflict.
            (OperationKind::Read, OperationKind::Read) => false,
            // Write + write conflicts.
            (OperationKind::Update, OperationKind::Update) => true,
            (OperationKind::Create, OperationKind::Create) => true,
            // Delete + anything conflicts.
            (OperationKind::Delete, _) | (_, OperationKind::Delete) => true,
            // Read + write is a read-write conflict.
            (OperationKind::Read, OperationKind::Update) => true,
            (OperationKind::Update, OperationKind::Read) => true,
            (OperationKind::Read, OperationKind::Delete) => true,
            (OperationKind::Delete, OperationKind::Read) => true,
            // Execute + update conflicts.
            (OperationKind::Execute, OperationKind::Update) => true,
            (OperationKind::Update, OperationKind::Execute) => true,
            // Other combinations don't conflict.
            _ => false,
        }
    }

    /// Resolve a conflict by ID.
    pub fn resolve(&mut self, conflict_id: &str, resolution: Resolution) -> bool {
        if let Some(conflict) = self.conflicts.iter_mut().find(|c| c.id == conflict_id) {
            conflict.resolution = Some(resolution);
            conflict.resolved_at = Some(now_secs());
            true
        } else {
            false
        }
    }

    /// Get unresolved conflicts.
    pub fn unresolved(&self) -> Vec<&OperationConflict> {
        self.conflicts.iter().filter(|c| c.resolution.is_none()).collect()
    }

    /// Get unresolved conflicts for a specific resource.
    pub fn unresolved_for(&self, resource: &str) -> Vec<&OperationConflict> {
        self.unresolved().into_iter()
            .filter(|c| c.resource == resource)
            .collect()
    }

    /// Get conflict statistics.
    pub fn stats(&self) -> ConflictStats {
        ConflictStats {
            total: self.conflicts.len(),
            unresolved: self.unresolved().len(),
            by_resource_type: self.count_by_resource_type(),
        }
    }

    fn count_by_resource_type(&self) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for conflict in &self.conflicts {
            *counts.entry(conflict.resource_type.label().to_string()).or_default() += 1;
        }
        counts
    }

    /// Save resolver state to disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let json = serde_json::to_vec_pretty(&self.conflicts)
            .map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("conflict_history.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load resolver state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut resolver = Self::new();
        let path = workspace_root.join(".velocity").join("conflict_history.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(conflicts) = serde_json::from_slice::<Vec<OperationConflict>>(&bytes) {
                resolver.conflicts = conflicts;
            }
        }
        resolver
    }
}

/// Error when acquiring a lock.
#[derive(Debug, Clone)]
pub enum LockError {
    AlreadyLocked { resource: String, holder: String },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyLocked { resource, holder } => {
                write!(f, "Resource '{}' is locked by '{}'", resource, holder)
            }
        }
    }
}

/// Statistics about conflicts.
#[derive(Debug)]
pub struct ConflictStats {
    pub total: usize,
    pub unresolved: usize,
    pub by_resource_type: HashMap<String, usize>,
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

    fn make_op(actor: &str, kind: OperationKind) -> Operation {
        Operation {
            actor_id: actor.to_string(),
            kind,
            timestamp: now_secs(),
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn exclusive_lock() {
        let mut resolver = ConflictResolver::new();
        assert!(resolver.try_lock("file.rs", "user1", LockKind::Exclusive).is_ok());
        assert!(resolver.is_locked("file.rs"));
        assert_eq!(resolver.lock_holder("file.rs"), Some("user1"));
    }

    #[test]
    fn exclusive_lock_conflict() {
        let mut resolver = ConflictResolver::new();
        resolver.try_lock("file.rs", "user1", LockKind::Exclusive).unwrap();
        let result = resolver.try_lock("file.rs", "user2", LockKind::Exclusive);
        assert!(result.is_err());
    }

    #[test]
    fn same_actor_relock() {
        let mut resolver = ConflictResolver::new();
        resolver.try_lock("file.rs", "user1", LockKind::Exclusive).unwrap();
        assert!(resolver.try_lock("file.rs", "user1", LockKind::Exclusive).is_ok());
    }

    #[test]
    fn shared_locks_coexist() {
        let mut resolver = ConflictResolver::new();
        resolver.try_lock("file.rs", "user1", LockKind::Shared).unwrap();
        assert!(resolver.try_lock("file.rs", "user2", LockKind::Shared).is_ok());
    }

    #[test]
    fn shared_lock_blocks_exclusive() {
        let mut resolver = ConflictResolver::new();
        resolver.try_lock("file.rs", "user1", LockKind::Shared).unwrap();
        let result = resolver.try_lock("file.rs", "user2", LockKind::Exclusive);
        assert!(result.is_err());
    }

    #[test]
    fn unlock_releases() {
        let mut resolver = ConflictResolver::new();
        resolver.try_lock("file.rs", "user1", LockKind::Exclusive).unwrap();
        assert!(resolver.unlock("file.rs", "user1"));
        assert!(!resolver.is_locked("file.rs"));
    }

    #[test]
    fn detect_write_write_conflict() {
        assert!(ConflictResolver::operations_conflict(
            &OperationKind::Update, &OperationKind::Update
        ));
    }

    #[test]
    fn no_read_read_conflict() {
        assert!(!ConflictResolver::operations_conflict(
            &OperationKind::Read, &OperationKind::Read
        ));
    }

    #[test]
    fn detect_delete_conflicts() {
        assert!(ConflictResolver::operations_conflict(
            &OperationKind::Delete, &OperationKind::Read
        ));
        assert!(ConflictResolver::operations_conflict(
            &OperationKind::Create, &OperationKind::Delete
        ));
    }

    #[test]
    fn record_and_resolve_conflict() {
        let mut resolver = ConflictResolver::new();
        let op_a = make_op("user1", OperationKind::Update);
        let op_b = make_op("user2", OperationKind::Update);

        let id = resolver.record_conflict("file.rs", ResourceType::File, op_a, op_b);
        assert_eq!(resolver.unresolved().len(), 1);

        resolver.resolve(&id, Resolution::KeepLatest);
        assert_eq!(resolver.unresolved().len(), 0);
    }

    #[test]
    fn unresolved_for_resource() {
        let mut resolver = ConflictResolver::new();
        resolver.record_conflict("a.rs", ResourceType::File,
            make_op("u1", OperationKind::Update),
            make_op("u2", OperationKind::Update));
        resolver.record_conflict("b.rs", ResourceType::File,
            make_op("u1", OperationKind::Update),
            make_op("u2", OperationKind::Update));

        assert_eq!(resolver.unresolved_for("a.rs").len(), 1);
        assert_eq!(resolver.unresolved_for("c.rs").len(), 0);
    }

    #[test]
    fn conflict_stats() {
        let mut resolver = ConflictResolver::new();
        resolver.record_conflict("f.rs", ResourceType::File,
            make_op("u1", OperationKind::Update),
            make_op("u2", OperationKind::Update));
        resolver.record_conflict("w1", ResourceType::Workflow,
            make_op("u1", OperationKind::Execute),
            make_op("u2", OperationKind::Update));

        let stats = resolver.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.unresolved, 2);
        assert_eq!(stats.by_resource_type["file"], 1);
        assert_eq!(stats.by_resource_type["workflow"], 1);
    }

    #[test]
    fn resource_type_labels() {
        assert_eq!(ResourceType::File.label(), "file");
        assert_eq!(ResourceType::AgentSession.label(), "session");
        assert_eq!(ResourceType::KnowledgeEntry.label(), "knowledge");
    }

    #[test]
    fn resolution_labels() {
        assert_eq!(Resolution::KeepFirst.label(), "keep_first");
        assert_eq!(Resolution::Merge.label(), "merge");
        assert_eq!(Resolution::Manual.label(), "manual");
    }

    #[test]
    fn max_conflicts_eviction() {
        let mut resolver = ConflictResolver::new();
        resolver.max_conflicts = 3;

        for i in 0..5 {
            resolver.record_conflict(
                &format!("file{}.rs", i),
                ResourceType::File,
                make_op("u1", OperationKind::Update),
                make_op("u2", OperationKind::Update),
            );
        }
        assert_eq!(resolver.conflicts.len(), 3);
    }
}
