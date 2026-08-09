//! Bi-directional sync engine for external services.
//!
//! Manages synchronization of data between the local workspace and external
//! services (GitHub issues, Jira tickets, Notion pages, etc.). Supports
//! conflict resolution, sync state tracking, and periodic polling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A sync rule defining what to sync and in which direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRule {
    /// Unique rule ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Connector ID to sync with.
    pub connector_id: String,
    /// Direction of sync.
    pub direction: SyncDirection,
    /// Resource type to sync (e.g., "issues", "tasks", "pages").
    pub resource_type: String,
    /// How often to poll (seconds). 0 = manual only.
    pub poll_interval_secs: u64,
    /// Last sync timestamp.
    pub last_sync: Option<u64>,
    /// Whether this rule is active.
    pub enabled: bool,
    /// Field mappings: local_field -> remote_field.
    pub field_mappings: Vec<(String, String)>,
    /// Filter expression (e.g., "labels contains 'bug'").
    pub filter: Option<String>,
}

/// Direction of data flow for a sync rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncDirection {
    /// Only pull from remote to local.
    PullOnly,
    /// Only push from local to remote.
    PushOnly,
    /// Bi-directional sync.
    BiDirectional,
}

impl SyncDirection {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PullOnly => "pull",
            Self::PushOnly => "push",
            Self::BiDirectional => "bidirectional",
        }
    }
}

/// A synced resource item tracked by the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedItem {
    /// Local identifier.
    pub local_id: String,
    /// Remote identifier (on the external service).
    pub remote_id: String,
    /// The sync rule that manages this item.
    pub rule_id: String,
    /// Local content hash (for change detection).
    pub local_hash: u64,
    /// Remote content hash (for change detection).
    pub remote_hash: u64,
    /// Last time this item was synced.
    pub last_synced: u64,
    /// Whether the local version has unsynced changes.
    pub local_dirty: bool,
    /// Whether the remote version has unsynced changes.
    pub remote_dirty: bool,
}

/// A conflict that occurred during sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    /// Unique conflict ID.
    pub id: String,
    /// The sync rule involved.
    pub rule_id: String,
    /// The item that conflicted.
    pub item_id: String,
    /// When the conflict was detected.
    pub detected_at: u64,
    /// Local version of the data.
    pub local_data: serde_json::Value,
    /// Remote version of the data.
    pub remote_data: serde_json::Value,
    /// Resolution strategy (if resolved).
    pub resolution: Option<ConflictResolution>,
}

/// How to resolve a sync conflict.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Keep the local version.
    KeepLocal,
    /// Keep the remote version.
    KeepRemote,
    /// Keep both (create duplicates).
    KeepBoth,
    /// Discard both.
    DiscardBoth,
}

/// Result of a sync operation.
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    /// Items pulled from remote.
    pub pulled: usize,
    /// Items pushed to remote.
    pub pushed: usize,
    /// Conflicts detected.
    pub conflicts: usize,
    /// Errors encountered.
    pub errors: Vec<String>,
}

impl SyncResult {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn total_changes(&self) -> usize {
        self.pulled + self.pushed
    }
}

/// The bi-directional sync engine.
#[derive(Debug, Clone, Default)]
pub struct SyncEngine {
    /// Configured sync rules.
    pub rules: HashMap<String, SyncRule>,
    /// Tracked synced items keyed by "rule_id:local_id".
    pub items: HashMap<String, SyncedItem>,
    /// Unresolved conflicts.
    pub conflicts: Vec<SyncConflict>,
    /// Maximum conflicts to retain.
    pub max_conflicts: usize,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            items: HashMap::new(),
            conflicts: Vec::new(),
            max_conflicts: 100,
        }
    }

    /// Add or update a sync rule.
    pub fn add_rule(&mut self, rule: SyncRule) {
        self.rules.insert(rule.id.clone(), rule);
    }

    /// Remove a sync rule and its tracked items.
    pub fn remove_rule(&mut self, id: &str) -> bool {
        let removed = self.rules.remove(id).is_some();
        if removed {
            self.items.retain(|key, _| !key.starts_with(&format!("{}:", id)));
            self.conflicts.retain(|c| c.rule_id != id);
        }
        removed
    }

    /// Get rules that are due for polling.
    pub fn due_rules(&self, now: u64) -> Vec<&SyncRule> {
        self.rules.values()
            .filter(|r| {
                r.enabled && r.poll_interval_secs > 0 && match r.last_sync {
                    Some(last) => now - last >= r.poll_interval_secs,
                    None => true,
                }
            })
            .collect()
    }

    /// Determine what needs to happen for a synced item.
    pub fn item_action(&self, key: &str) -> SyncAction {
        match self.items.get(key) {
            Some(item) => {
                match (item.local_dirty, item.remote_dirty) {
                    (false, false) => SyncAction::None,
                    (true, false) => {
                        // Check rule direction.
                        if let Some(rule) = self.rules.get(&item.rule_id) {
                            match rule.direction {
                                SyncDirection::PullOnly => SyncAction::SkipPull,
                                _ => SyncAction::Push,
                            }
                        } else {
                            SyncAction::None
                        }
                    }
                    (false, true) => {
                        if let Some(rule) = self.rules.get(&item.rule_id) {
                            match rule.direction {
                                SyncDirection::PushOnly => SyncAction::SkipPush,
                                _ => SyncAction::Pull,
                            }
                        } else {
                            SyncAction::None
                        }
                    }
                    (true, true) => SyncAction::Conflict,
                }
            }
            None => SyncAction::None,
        }
    }

    /// Record a new synced item.
    pub fn track_item(&mut self, item: SyncedItem) {
        let key = format!("{}:{}", item.rule_id, item.local_id);
        self.items.insert(key, item);
    }

    /// Mark a local item as dirty (has unsynced changes).
    pub fn mark_local_dirty(&mut self, rule_id: &str, local_id: &str) {
        let key = format!("{}:{}", rule_id, local_id);
        if let Some(item) = self.items.get_mut(&key) {
            item.local_dirty = true;
        }
    }

    /// Mark a remote item as dirty (has changes on the remote).
    pub fn mark_remote_dirty(&mut self, rule_id: &str, local_id: &str) {
        let key = format!("{}:{}", rule_id, local_id);
        if let Some(item) = self.items.get_mut(&key) {
            item.remote_dirty = true;
        }
    }

    /// Resolve an item as successfully synced.
    pub fn mark_synced(&mut self, rule_id: &str, local_id: &str, local_hash: u64, remote_hash: u64) {
        let key = format!("{}:{}", rule_id, local_id);
        if let Some(item) = self.items.get_mut(&key) {
            item.local_hash = local_hash;
            item.remote_hash = remote_hash;
            item.local_dirty = false;
            item.remote_dirty = false;
            item.last_synced = now_secs();
        }
    }

    /// Record a sync conflict.
    pub fn record_conflict(
        &mut self,
        rule_id: &str,
        local_id: &str,
        local_data: serde_json::Value,
        remote_data: serde_json::Value,
    ) -> String {
        let id = format!("conflict_{}_{}", now_secs(), self.conflicts.len());
        self.conflicts.push(SyncConflict {
            id: id.clone(),
            rule_id: rule_id.to_string(),
            item_id: local_id.to_string(),
            detected_at: now_secs(),
            local_data,
            remote_data,
            resolution: None,
        });
        while self.conflicts.len() > self.max_conflicts {
            self.conflicts.remove(0);
        }
        id
    }

    /// Resolve a conflict.
    pub fn resolve_conflict(&mut self, conflict_id: &str, resolution: ConflictResolution) -> bool {
        if let Some(conflict) = self.conflicts.iter_mut().find(|c| c.id == conflict_id) {
            conflict.resolution = Some(resolution);
            true
        } else {
            false
        }
    }

    /// Count unresolved conflicts.
    pub fn unresolved_conflicts(&self) -> Vec<&SyncConflict> {
        self.conflicts.iter().filter(|c| c.resolution.is_none()).collect()
    }

    /// Get sync statistics for a rule.
    pub fn rule_stats(&self, rule_id: &str) -> SyncStats {
        let items: Vec<&SyncedItem> = self.items.values()
            .filter(|i| i.rule_id == rule_id)
            .collect();

        SyncStats {
            total_items: items.len(),
            dirty_local: items.iter().filter(|i| i.local_dirty).count(),
            dirty_remote: items.iter().filter(|i| i.remote_dirty).count(),
            conflicts: self.conflicts.iter().filter(|c| c.rule_id == rule_id && c.resolution.is_none()).count(),
        }
    }

    /// Update the last_sync timestamp for a rule.
    pub fn update_last_sync(&mut self, rule_id: &str) {
        if let Some(rule) = self.rules.get_mut(rule_id) {
            rule.last_sync = Some(now_secs());
        }
    }

    /// Save engine state to disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let state = PersistedSyncState {
            rules: self.rules.values().cloned().collect(),
            items: self.items.values().cloned().collect(),
            conflicts: self.conflicts.clone(),
        };
        let json = serde_json::to_vec_pretty(&state)
            .map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("sync_state.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load engine state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut engine = Self::new();
        let path = workspace_root.join(".velocity").join("sync_state.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(state) = serde_json::from_slice::<PersistedSyncState>(&bytes) {
                for rule in state.rules {
                    engine.rules.insert(rule.id.clone(), rule);
                }
                for item in state.items {
                    let key = format!("{}:{}", item.rule_id, item.local_id);
                    engine.items.insert(key, item);
                }
                engine.conflicts = state.conflicts;
            }
        }
        engine
    }
}

/// What action to take for a synced item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// No action needed.
    None,
    /// Push local changes to remote.
    Push,
    /// Pull remote changes to local.
    Pull,
    /// Conflict — both sides changed.
    Conflict,
    /// Skip because rule is pull-only but local is dirty.
    SkipPull,
    /// Skip because rule is push-only but remote is dirty.
    SkipPush,
}

/// Statistics about sync state for a rule.
#[derive(Debug, Clone)]
pub struct SyncStats {
    pub total_items: usize,
    pub dirty_local: usize,
    pub dirty_remote: usize,
    pub conflicts: usize,
}

/// Serializable persistence for sync state.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSyncState {
    rules: Vec<SyncRule>,
    items: Vec<SyncedItem>,
    conflicts: Vec<SyncConflict>,
}

/// Create default sync rules for common services.
pub fn create_default_rules(connector_id: &str) -> Vec<SyncRule> {
    vec![
        SyncRule {
            id: format!("{}_issues", connector_id),
            name: "Sync Issues".to_string(),
            connector_id: connector_id.to_string(),
            direction: SyncDirection::BiDirectional,
            resource_type: "issues".to_string(),
            poll_interval_secs: 300,
            last_sync: None,
            enabled: false,
            field_mappings: vec![
                ("title".to_string(), "title".to_string()),
                ("body".to_string(), "body".to_string()),
                ("status".to_string(), "state".to_string()),
                ("labels".to_string(), "labels".to_string()),
            ],
            filter: None,
        },
        SyncRule {
            id: format!("{}_prs", connector_id),
            name: "Sync Pull Requests".to_string(),
            connector_id: connector_id.to_string(),
            direction: SyncDirection::PullOnly,
            resource_type: "pull_requests".to_string(),
            poll_interval_secs: 120,
            last_sync: None,
            enabled: false,
            field_mappings: vec![
                ("title".to_string(), "title".to_string()),
                ("status".to_string(), "state".to_string()),
                ("branch".to_string(), "head_ref".to_string()),
            ],
            filter: Some("state:open".to_string()),
        },
    ]
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

    fn test_rule() -> SyncRule {
        SyncRule {
            id: "rule1".to_string(),
            name: "Test Sync".to_string(),
            connector_id: "github".to_string(),
            direction: SyncDirection::BiDirectional,
            resource_type: "issues".to_string(),
            poll_interval_secs: 60,
            last_sync: None,
            enabled: true,
            field_mappings: vec![("title".to_string(), "title".to_string())],
            filter: None,
        }
    }

    fn test_item(rule_id: &str, local_id: &str) -> SyncedItem {
        SyncedItem {
            local_id: local_id.to_string(),
            remote_id: format!("remote_{}", local_id),
            rule_id: rule_id.to_string(),
            local_hash: 12345,
            remote_hash: 12345,
            last_synced: now_secs(),
            local_dirty: false,
            remote_dirty: false,
        }
    }

    #[test]
    fn add_and_remove_rule() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        assert_eq!(engine.rules.len(), 1);
        assert!(engine.remove_rule("rule1"));
        assert_eq!(engine.rules.len(), 0);
    }

    #[test]
    fn remove_rule_cleans_items() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "item1"));
        engine.track_item(test_item("rule1", "item2"));
        assert_eq!(engine.items.len(), 2);

        engine.remove_rule("rule1");
        assert_eq!(engine.items.len(), 0);
    }

    #[test]
    fn due_rules_check() {
        let mut engine = SyncEngine::new();
        let mut rule = test_rule();
        rule.poll_interval_secs = 60;
        rule.last_sync = Some(now_secs() - 120);
        engine.add_rule(rule);

        let due = engine.due_rules(now_secs());
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn item_action_no_changes() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "i1"));
        assert_eq!(engine.item_action("rule1:i1"), SyncAction::None);
    }

    #[test]
    fn item_action_local_dirty() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "i1"));
        engine.mark_local_dirty("rule1", "i1");
        assert_eq!(engine.item_action("rule1:i1"), SyncAction::Push);
    }

    #[test]
    fn item_action_remote_dirty() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "i1"));
        engine.mark_remote_dirty("rule1", "i1");
        assert_eq!(engine.item_action("rule1:i1"), SyncAction::Pull);
    }

    #[test]
    fn item_action_conflict() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "i1"));
        engine.mark_local_dirty("rule1", "i1");
        engine.mark_remote_dirty("rule1", "i1");
        assert_eq!(engine.item_action("rule1:i1"), SyncAction::Conflict);
    }

    #[test]
    fn pull_only_skips_push() {
        let mut engine = SyncEngine::new();
        let mut rule = test_rule();
        rule.direction = SyncDirection::PullOnly;
        engine.add_rule(rule);
        engine.track_item(test_item("rule1", "i1"));
        engine.mark_local_dirty("rule1", "i1");
        assert_eq!(engine.item_action("rule1:i1"), SyncAction::SkipPull);
    }

    #[test]
    fn mark_synced_clears_dirty() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "i1"));
        engine.mark_local_dirty("rule1", "i1");
        engine.mark_remote_dirty("rule1", "i1");
        engine.mark_synced("rule1", "i1", 999, 999);

        let item = &engine.items["rule1:i1"];
        assert!(!item.local_dirty);
        assert!(!item.remote_dirty);
        assert_eq!(item.local_hash, 999);
    }

    #[test]
    fn conflict_lifecycle() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());

        let id = engine.record_conflict("rule1", "i1", serde_json::json!({"a": 1}), serde_json::json!({"a": 2}));
        assert_eq!(engine.unresolved_conflicts().len(), 1);

        engine.resolve_conflict(&id, ConflictResolution::KeepLocal);
        assert_eq!(engine.unresolved_conflicts().len(), 0);
    }

    #[test]
    fn rule_stats() {
        let mut engine = SyncEngine::new();
        engine.add_rule(test_rule());
        engine.track_item(test_item("rule1", "i1"));
        engine.track_item(test_item("rule1", "i2"));
        engine.mark_local_dirty("rule1", "i1");

        let stats = engine.rule_stats("rule1");
        assert_eq!(stats.total_items, 2);
        assert_eq!(stats.dirty_local, 1);
        assert_eq!(stats.dirty_remote, 0);
    }

    #[test]
    fn default_rules_created() {
        let rules = create_default_rules("github");
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().any(|r| r.resource_type == "issues"));
        assert!(rules.iter().any(|r| r.resource_type == "pull_requests"));
    }

    #[test]
    fn sync_direction_labels() {
        assert_eq!(SyncDirection::PullOnly.label(), "pull");
        assert_eq!(SyncDirection::PushOnly.label(), "push");
        assert_eq!(SyncDirection::BiDirectional.label(), "bidirectional");
    }
}
