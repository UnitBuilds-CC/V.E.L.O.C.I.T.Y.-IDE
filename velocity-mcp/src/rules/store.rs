//! Persistence for the rules engine.
//!
//! Two durable files (global and workspace) plus a process-local task store.
//! Reads are cached by file stamp and revalidated on every access, so a rule
//! written by another process — or another agent in this one — is live on the
//! next tool call without a restart.
//!
//! A rule file that exists but does not parse is a hard error, never an empty
//! set: silently dropping safety rules is the one failure mode a rules engine
//! must not have.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::model::{now_secs, RuleFile, RuleScope};

/// Where the two durable files live for one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePaths {
    pub global: PathBuf,
    pub workspace: PathBuf,
}

impl RulePaths {
    /// Global rules live beside provider credentials under the user config dir;
    /// workspace rules live in the workspace and are shared with every agent
    /// that opens it.
    pub fn for_workspace(root: &Path) -> Self {
        Self {
            global: crate::usage::user_config_dir().join("rules.json"),
            workspace: root.join(".velocity").join("rules.json"),
        }
    }

    /// Both files inside one directory, for tests and harnesses.
    pub fn isolated(dir: &Path) -> Self {
        Self {
            global: dir.join("global-rules.json"),
            workspace: dir.join("workspace-rules.json"),
        }
    }

    pub fn for_scope(&self, scope: RuleScope) -> Option<&Path> {
        match scope {
            RuleScope::Task => None,
            RuleScope::Workspace => Some(&self.workspace),
            RuleScope::Global => Some(&self.global),
        }
    }

    /// Changes whenever either file's content may have changed. The UI polls
    /// this to decide whether to reload; the gate compares it internally.
    pub fn revision(&self) -> u64 {
        let mut hash = task_revision();
        for path in [&self.workspace, &self.global] {
            let stamp = file_stamp(path).unwrap_or((0, 0));
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(stamp.0 ^ stamp.1.rotate_left(17));
        }
        hash
    }
}

fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Some((mtime, meta.len()))
}

struct CachedFile {
    stamp: (u64, u64),
    file: Arc<RuleFile>,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CachedFile>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedFile>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn task_store() -> &'static Mutex<HashMap<PathBuf, RuleFile>> {
    static TASK: OnceLock<Mutex<HashMap<PathBuf, RuleFile>>> = OnceLock::new();
    TASK.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Read and parse one durable file, with the stamp check.
fn load_file(path: &Path) -> Result<Arc<RuleFile>, String> {
    let Some(stamp) = file_stamp(path) else {
        // Missing file is not an error: no rules configured for this scope.
        return Ok(Arc::new(RuleFile::default()));
    };
    if let Some(hit) = cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
    {
        if hit.stamp == stamp {
            return Ok(Arc::clone(&hit.file));
        }
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("cannot read rules file {}: {e}", path.display()))?;
    let text = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let file: RuleFile = serde_json::from_str(text)
        .map_err(|e| format!("rules file {} is not valid JSON: {e}", path.display()))?;
    let file = Arc::new(file);
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            path.to_path_buf(),
            CachedFile {
                stamp,
                file: Arc::clone(&file),
            },
        );
    Ok(file)
}

/// The task store is keyed by the canonical workspace path: the dispatch gate
/// canonicalizes its root (verbatim on Windows) while the interface holds the
/// path as typed, and both must name the same task rules.
fn task_key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

/// Bumped on every task-store mutation. Task rules are memory-only, so the file
/// stat stamp the panel polls cannot see them; without this counter a rule an
/// agent created this turn would stay invisible in the interface until a durable
/// file happened to change.
static TASK_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn task_revision() -> u64 {
    TASK_REVISION.load(std::sync::atomic::Ordering::Relaxed)
}

fn bump_task_revision() {
    TASK_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// The task-scoped rules for `root` (process memory only).
pub fn task_file(root: &Path) -> RuleFile {
    task_store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&task_key(root))
        .cloned()
        .unwrap_or_default()
}

/// Load the rules for one scope. Errors fail closed at the gate.
pub fn load_scoped(paths: &RulePaths, root: &Path, scope: RuleScope) -> Result<Arc<RuleFile>, String> {
    match scope {
        RuleScope::Task => Ok(Arc::new(task_file(root))),
        _ => load_file(
            paths
                .for_scope(scope)
                .expect("durable scopes have a path"),
        ),
    }
}

/// All three scopes plus any load errors, for display. Never panics on a
/// corrupt file — the error is reported and that scope shows as empty.
#[derive(Debug, Clone, Default)]
pub struct RuleSnapshot {
    pub task: RuleFile,
    pub workspace: RuleFile,
    pub global: RuleFile,
    pub revision: u64,
    pub errors: Vec<String>,
}

impl RuleSnapshot {
    /// Active records, most specific scope first.
    pub fn active(&self) -> Vec<(RuleScope, &super::model::RuleRecord)> {
        let mut out = Vec::new();
        for (scope, file) in [
            (RuleScope::Task, &self.task),
            (RuleScope::Workspace, &self.workspace),
            (RuleScope::Global, &self.global),
        ] {
            for record in file.rules.iter().filter(|r| r.is_active()) {
                out.push((scope, record));
            }
        }
        out
    }

    /// Retired records, for the history tab.
    pub fn retired(&self) -> Vec<(RuleScope, &super::model::RuleRecord)> {
        let mut out = Vec::new();
        for (scope, file) in [
            (RuleScope::Task, &self.task),
            (RuleScope::Workspace, &self.workspace),
            (RuleScope::Global, &self.global),
        ] {
            for record in file.rules.iter().filter(|r| !r.is_active()) {
                out.push((scope, record));
            }
        }
        out
    }

    pub fn pending_requests(&self) -> Vec<(RuleScope, &super::model::ChangeRequest)> {
        let mut out = Vec::new();
        for (scope, file) in [
            (RuleScope::Task, &self.task),
            (RuleScope::Workspace, &self.workspace),
            (RuleScope::Global, &self.global),
        ] {
            for request in file
                .requests
                .iter()
                .filter(|r| r.status == super::model::RequestStatus::Pending)
            {
                out.push((scope, request));
            }
        }
        out
    }

    pub fn live_grants(&self) -> Vec<(RuleScope, &super::model::BypassGrant)> {
        let now = now_secs();
        let mut out = Vec::new();
        for (scope, file) in [
            (RuleScope::Task, &self.task),
            (RuleScope::Workspace, &self.workspace),
            (RuleScope::Global, &self.global),
        ] {
            for grant in file.grants.iter().filter(|g| !g.is_expired(now)) {
                out.push((scope, grant));
            }
        }
        out
    }

    pub fn find(&self, id: &str) -> Option<(RuleScope, &super::model::RuleRecord)> {
        for (scope, file) in [
            (RuleScope::Task, &self.task),
            (RuleScope::Workspace, &self.workspace),
            (RuleScope::Global, &self.global),
        ] {
            if let Some(record) = file.find(id) {
                return Some((scope, record));
            }
        }
        None
    }
}

impl RulePaths {
    pub fn snapshot(&self, root: &Path) -> RuleSnapshot {
        let mut snapshot = RuleSnapshot {
            task: task_file(root),
            revision: self.revision(),
            ..Default::default()
        };
        match load_file(&self.workspace) {
            Ok(file) => snapshot.workspace = (*file).clone(),
            Err(e) => snapshot.errors.push(e),
        }
        match load_file(&self.global) {
            Ok(file) => snapshot.global = (*file).clone(),
            Err(e) => snapshot.errors.push(e),
        }
        snapshot
    }
}

/// Atomically replace one durable file: write a sibling temp, then rename.
fn write_file(path: &Path, file: &RuleFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create rules directory {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(file)
        .map_err(|e| format!("cannot serialize rules: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)
        .map_err(|e| format!("cannot write rules temp {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path)
        .map_err(|e| format!("cannot replace rules file {}: {e}", path.display()))?;
    if let Some(stamp) = file_stamp(path) {
        cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                path.to_path_buf(),
                CachedFile {
                    stamp,
                    file: Arc::new(file.clone()),
                },
            );
    }
    Ok(())
}

/// Mutate one scope's rule file. Durable scopes reload from disk first so a
/// change made by another process is never clobbered; task scope is memory.
pub fn update_scoped<F>(
    paths: &RulePaths,
    root: &Path,
    scope: RuleScope,
    f: F,
) -> Result<(), String>
where
    F: FnOnce(&mut RuleFile) -> Result<(), String>,
{
    match scope {
        RuleScope::Task => {
            let key = task_key(root);
            let mut store = task_store().lock().unwrap_or_else(|e| e.into_inner());
            let file = store.entry(key).or_default();
            f(file).inspect(|_| bump_task_revision())
        }
        _ => {
            let path = paths.for_scope(scope).expect("durable scope");
            let raw = match fs::read_to_string(path) {
                Ok(raw) => raw,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(format!("cannot read rules file {}: {e}", path.display())),
            };
            let mut file = if raw.trim().is_empty() {
                RuleFile::default()
            } else {
                let text = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
                serde_json::from_str::<RuleFile>(text)
                    .map_err(|e| format!("rules file {} is not valid JSON: {e}", path.display()))?
            };
            f(&mut file)?;
            write_file(path, &file)
        }
    }
}

/// Consume a bypass for `rule_id`, if one is live. An event grant allows exactly
/// one call and is persisted as consumed; timed grants allow every call until
/// they expire.
pub fn consume_grant(
    paths: &RulePaths,
    root: &Path,
    rule_id: &str,
) -> Result<bool, String> {
    let now = now_secs();
    if let Some(grant) = task_file(root).live_grant(rule_id, now).cloned() {
        if grant.scope == super::model::BypassScope::Event {
            update_scoped(paths, root, RuleScope::Task, |file| {
                if let Some(live) = file.live_grant_mut(rule_id, now) {
                    live.uses_left = Some(0);
                }
                Ok(())
            })?;
        }
        return Ok(true);
    }
    for scope in [RuleScope::Workspace, RuleScope::Global] {
        let file = load_scoped(paths, root, scope)?;
        if let Some(grant) = file.live_grant(rule_id, now).cloned() {
            if grant.scope == super::model::BypassScope::Event {
                update_scoped(paths, root, scope, |file| {
                    if let Some(live) = file.live_grant_mut(rule_id, now) {
                        live.uses_left = Some(0);
                    }
                    Ok(())
                })?;
            }
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::{
        BypassGrant, BypassScope, ParamValue, Rule, RuleKind, RuleRecord,
    };
    use std::collections::BTreeMap;

    fn paths_in(dir: &Path) -> RulePaths {
        RulePaths::isolated(dir)
    }

    fn a_rule() -> Rule {
        let mut params = BTreeMap::new();
        params.insert(
            "tools".into(),
            ParamValue::List(vec!["run_command".into()]),
        );
        Rule {
            id: "rule-00000001".into(),
            name: "no-shell".into(),
            description: "Shell is off limits here.".into(),
            kind: RuleKind::ToolBan,
            params,
            remedy: None,
            owner: "user".into(),
            provenance: "test".into(),
            version: 1,
            created_at: 1,
            updated_at: 1,
            witnesses: None,
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_rule_set() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let file = load_scoped(&paths, tmp.path(), RuleScope::Workspace).unwrap();
        assert!(file.rules.is_empty());
    }

    #[test]
    fn workspace_rules_round_trip_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        update_scoped(&paths, tmp.path(), RuleScope::Workspace, |file| {
            file.rules
                .push(RuleRecord::new(RuleScope::Workspace, a_rule()));
            Ok(())
        })
        .unwrap();
        // No temp file survives a successful write.
        assert!(!paths.workspace.with_extension("json.tmp").exists());
        let loaded = load_scoped(&paths, tmp.path(), RuleScope::Workspace).unwrap();
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].current.name, "no-shell");
    }

    #[test]
    fn global_and_workspace_files_are_independent() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        update_scoped(&paths, tmp.path(), RuleScope::Global, |file| {
            let mut rule = a_rule();
            rule.id = "rule-global".into();
            file.rules.push(RuleRecord::new(RuleScope::Global, rule));
            Ok(())
        })
        .unwrap();
        assert!(load_scoped(&paths, tmp.path(), RuleScope::Global)
            .unwrap()
            .find("rule-global")
            .is_some());
        assert!(load_scoped(&paths, tmp.path(), RuleScope::Workspace)
            .unwrap()
            .find("rule-global")
            .is_none());
    }

    #[test]
    fn corrupt_rules_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        fs::write(&paths.workspace, "{ not json").unwrap();
        let error = load_scoped(&paths, tmp.path(), RuleScope::Workspace).unwrap_err();
        assert!(error.contains("not valid JSON"), "{error}");
        let snapshot = paths.snapshot(tmp.path());
        assert_eq!(snapshot.errors.len(), 1);
        assert!(snapshot.workspace.rules.is_empty());
    }

    #[test]
    fn a_byte_order_mark_does_not_break_loading() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let mut file = RuleFile::default();
        file.rules
            .push(RuleRecord::new(RuleScope::Workspace, a_rule()));
        let text = format!("\u{feff}{}", serde_json::to_string(&file).unwrap());
        fs::write(&paths.workspace, text).unwrap();
        let loaded = load_scoped(&paths, tmp.path(), RuleScope::Workspace).unwrap();
        assert_eq!(loaded.rules.len(), 1);
    }

    #[test]
    fn the_revision_moves_when_a_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        update_scoped(&paths, tmp.path(), RuleScope::Workspace, |_| Ok(())).unwrap();
        let first = paths.revision();
        update_scoped(&paths, tmp.path(), RuleScope::Workspace, |file| {
            file.rules
                .push(RuleRecord::new(RuleScope::Workspace, a_rule()));
            Ok(())
        })
        .unwrap();
        assert_ne!(first, paths.revision());
    }

    #[test]
    fn task_rules_live_in_memory_per_workspace() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        let paths = paths_in(one.path());
        update_scoped(&paths, one.path(), RuleScope::Task, |file| {
            file.rules
                .push(RuleRecord::new(RuleScope::Task, a_rule()));
            Ok(())
        })
        .unwrap();
        assert_eq!(task_file(one.path()).rules.len(), 1);
        assert!(task_file(two.path()).rules.is_empty());
    }

    #[test]
    fn an_event_grant_is_consumed_exactly_once() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let grant = BypassGrant {
            id: "byp-1".into(),
            rule_id: "rule-00000001".into(),
            scope: BypassScope::Event,
            expires_at: None,
            uses_left: Some(1),
            issued_by: "user".into(),
            reason: "one-off".into(),
            created_at: now_secs(),
        };
        update_scoped(&paths, tmp.path(), RuleScope::Workspace, |file| {
            file.grants.push(grant.clone());
            Ok(())
        })
        .unwrap();
        assert!(consume_grant(&paths, tmp.path(), "rule-00000001").unwrap());
        assert!(!consume_grant(&paths, tmp.path(), "rule-00000001").unwrap());
        let file = load_scoped(&paths, tmp.path(), RuleScope::Workspace).unwrap();
        assert_eq!(file.grants[0].uses_left, Some(0));
    }

    #[test]
    fn a_timed_grant_does_not_consume() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let grant = BypassGrant {
            id: "byp-2".into(),
            rule_id: "rule-00000001".into(),
            scope: BypassScope::Session,
            expires_at: Some(now_secs() + 600),
            uses_left: None,
            issued_by: "user".into(),
            reason: "debugging".into(),
            created_at: now_secs(),
        };
        update_scoped(&paths, tmp.path(), RuleScope::Global, |file| {
            file.grants.push(grant);
            Ok(())
        })
        .unwrap();
        assert!(consume_grant(&paths, tmp.path(), "rule-00000001").unwrap());
        assert!(consume_grant(&paths, tmp.path(), "rule-00000001").unwrap());
    }

    #[test]
    fn snapshot_orders_scopes_most_specific_first() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        update_scoped(&paths, tmp.path(), RuleScope::Task, |file| {
            file.rules.push(RuleRecord::new(RuleScope::Task, a_rule()));
            Ok(())
        })
        .unwrap();
        update_scoped(&paths, tmp.path(), RuleScope::Global, |file| {
            file.rules.push(RuleRecord::new(RuleScope::Global, a_rule()));
            Ok(())
        })
        .unwrap();
        let snapshot = paths.snapshot(tmp.path());
        let scopes: Vec<RuleScope> = snapshot.active().iter().map(|(s, _)| *s).collect();
        assert_eq!(scopes, vec![RuleScope::Task, RuleScope::Global]);
    }
}
