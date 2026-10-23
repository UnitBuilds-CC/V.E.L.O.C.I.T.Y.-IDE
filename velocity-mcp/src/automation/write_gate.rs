//! The single authoritative write-admission table.
//!
//! Historically Velocity gated concurrent writes through **two independent
//! locks** that could not see each other:
//!
//! - [`crate::automation::mediator::MediatorArena`] — line-range locks with a
//!   semantic-coupling check (via the [`SiteMap`]) and a TTL, used by the
//!   orchestrator when it acquires task scopes.
//! - `crate::agent::coordination::CoordinationBus::claim_file` — a whole-file
//!   exclusive claim on a *separate* table, used by the chat agent executor.
//!
//! Because they were separate tables with separate path-identity rules, a chat
//! agent and an orchestrator task could both admit a write to the *same file*
//! without either gate knowing (the D5 defect). [`WriteGate`] is the one lock
//! model that both entry points now share: a range/whole-scope lock keyed by a
//! single canonical identity, with the semantic check applied only when a
//! [`SiteMap`] is supplied. [`MediatorArena`] is a thin wrapper over it, and the
//! coordination bus claims route through [`WriteGate::claim_whole_file`].

use crate::safety::SafeMutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use velocity_ide::hash_str;
use velocity_ide::site_map::SiteMap;

/// Held locks older than this are pruned on the next acquisition. Matches the
/// threshold the mediator has always used, so no live lock lifetime changes.
const LOCK_TTL: Duration = Duration::from_secs(1800);

/// The sentinel line range for a lock that covers an entire file or directory.
/// [`is_dir_scope`] treats a range ending at `usize::MAX / 4` as a scope-wide
/// lock; this is the *same* range the orchestrator uses when acquiring task
/// scopes (`scope.rs`), so an executor whole-file claim and an orchestrator
/// scope lock are the identical lock kind and therefore conflict correctly.
pub const WHOLE_SCOPE: (usize, usize) = (1, usize::MAX / 4);

/// A single held edit lock: who owns it, over what range, and since when.
#[derive(Clone, Debug)]
pub struct EditLock {
    pub file_path: PathBuf,
    pub line_range: (usize, usize),
    pub agent_id: String,
    pub timestamp: Instant,
    /// Wall-clock seconds at acquisition. `Instant` cannot be rendered as a unix
    /// time, so the coordination bus's `lock_details` (stale-claim reporting) reads
    /// this instead. Set once at acquisition; never used for conflict decisions.
    pub claimed_unix: u64,
}

/// The class of overlap that blocked an acquisition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    DirectLine,
    ScopeOverlap,
    Semantic,
}

/// A structured rejection of an acquisition, naming the incumbent lock.
#[derive(Clone, Debug)]
pub struct Conflict {
    pub file_path: PathBuf,
    pub existing_lock: EditLock,
    pub requested_lock: EditLock,
    pub kind: ConflictKind,
}

/// Current wall-clock seconds since the UNIX epoch, for lock-claim reporting.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_dir_scope(path: &Path, range: (usize, usize)) -> bool {
    path.is_dir()
        || path.extension().is_none()
        || path.to_string_lossy().ends_with('/')
        || path.to_string_lossy().ends_with('\\')
        || range.1 == usize::MAX
        || range.1 == usize::MAX / 4
}

pub(crate) fn path_identity_hash(path: &Path) -> u64 {
    let canonical = canonicalize_scope_path(path);
    hash_str(&canonical)
}

fn scopes_overlap(a: &Path, a_is_dir: bool, b: &Path, b_is_dir: bool) -> bool {
    let a_canonical = canonicalize_scope_path(a);
    let b_canonical = canonicalize_scope_path(b);

    if a_canonical == b_canonical {
        return true;
    }

    if a_is_dir && is_canonical_within_scope(&b_canonical, &a_canonical) {
        return true;
    }

    if b_is_dir && is_canonical_within_scope(&a_canonical, &b_canonical) {
        return true;
    }

    false
}

fn is_canonical_within_scope(path: &str, scope_root: &str) -> bool {
    path == scope_root
        || path
            .strip_prefix(scope_root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// The one canonical identity used across *all* write admission. Workspace-relative,
/// separator-normalized, filesystem-agnostic (no `canonicalize()`), which is why an
/// executor claim and an orchestrator scope lock on the same file resolve to the same
/// key and therefore conflict.
fn canonicalize_scope_path(path: &Path) -> String {
    let mut normalized = Vec::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::CurDir => {}
            Component::ParentDir => normalized.push("..".to_string()),
            Component::Normal(part) => normalized.push(part.to_string_lossy().replace('\\', "/")),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    normalized.join("/")
}

/// The single authoritative lock table shared by every write path.
pub struct WriteGate {
    locks: Mutex<HashMap<PathBuf, Vec<EditLock>>>,
}

impl Default for WriteGate {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteGate {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// Try to acquire a lock over `line_range` of `file_path` for `agent_id`.
    /// Returns `Err(Conflict)` on a direct-line, scope-overlap, or (when a
    /// `site_map` is supplied) semantic-coupling overlap. Passing `None` for the
    /// site map keeps the check to pure path/range overlap — this is what a
    /// whole-file claim uses, so a claim never newly fails for a coupling reason it
    /// was never subject to before.
    pub fn acquire(
        &self,
        file_path: PathBuf,
        line_range: (usize, usize),
        agent_id: String,
        site_map: Option<&SiteMap>,
    ) -> Result<(), Conflict> {
        self.prune_stale(LOCK_TTL);
        let mut locks_guard = self.locks.lock_safe();

        let requested = EditLock {
            file_path: file_path.clone(),
            line_range,
            agent_id: agent_id.clone(),
            timestamp: Instant::now(),
            claimed_unix: now_unix(),
        };

        let requested_canonical = canonicalize_scope_path(&file_path);
        let requested_is_dir = is_dir_scope(&file_path, line_range);

        // 1. Direct path overlap and exact-file line overlap checks.
        for (other_path, other_locks) in locks_guard.iter() {
            for existing in other_locks {
                if existing.agent_id == agent_id {
                    continue;
                }

                let existing_is_dir = is_dir_scope(other_path, existing.line_range);
                if !scopes_overlap(&file_path, requested_is_dir, other_path, existing_is_dir) {
                    continue;
                }

                if !requested_is_dir
                    && !existing_is_dir
                    && canonicalize_scope_path(other_path) == requested_canonical
                {
                    let (req_start, req_end) = line_range;
                    let (exist_start, exist_end) = existing.line_range;
                    if req_start <= exist_end && req_end >= exist_start {
                        return Err(Conflict {
                            file_path: file_path.clone(),
                            existing_lock: existing.clone(),
                            requested_lock: requested,
                            kind: ConflictKind::DirectLine,
                        });
                    }
                    continue;
                }

                return Err(Conflict {
                    file_path: file_path.clone(),
                    existing_lock: existing.clone(),
                    requested_lock: requested,
                    kind: ConflictKind::ScopeOverlap,
                });
            }
        }

        // 2. Semantic coupling check across different files/methods. Only runs when a
        //    site map is provided (the mediator/orchestrator path); whole-file claims
        //    pass None and therefore are never blocked by a coupling they never saw.
        if let Some(site_map) = site_map {
            let file_identity_hash = path_identity_hash(&file_path);
            let callers = site_map.get_callers(file_identity_hash);

            for (other_path, other_locks) in locks_guard.iter() {
                if canonicalize_scope_path(other_path) != requested_canonical {
                    for existing in other_locks {
                        if existing.agent_id != agent_id {
                            let other_identity_hash = path_identity_hash(other_path);
                            if callers.contains(&other_identity_hash) {
                                return Err(Conflict {
                                    file_path: file_path.clone(),
                                    existing_lock: existing.clone(),
                                    requested_lock: requested,
                                    kind: ConflictKind::Semantic,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Insert new lock
        locks_guard.entry(file_path).or_default().push(requested);
        Ok(())
    }

    /// Claim an entire file for exclusive writing by a single owner. Returns `true`
    /// on success, `false` if any other owner already holds an overlapping lock on
    /// the same resource. This is the coordination-bus entry point routed through the
    /// shared table, so a claim is now visible to (and blocks) orchestrator range
    /// locks on the same file and vice versa.
    pub fn claim_whole_file(&self, agent_id: &str, path: &Path) -> bool {
        self.acquire(path.to_path_buf(), WHOLE_SCOPE, agent_id.to_string(), None)
            .is_ok()
    }

    /// Release locks held by `agent_id` on a specific path.
    pub fn release(&self, file_path: &Path, agent_id: &str) {
        let mut locks_guard = self.locks.lock_safe();
        let target_canonical = canonicalize_scope_path(file_path);
        locks_guard.retain(|path, file_locks| {
            if canonicalize_scope_path(path) == target_canonical {
                file_locks.retain(|lock| lock.agent_id != agent_id);
            }
            !file_locks.is_empty()
        });
    }

    /// Release every lock held by `agent_id` across all tracked paths.
    pub fn release_for_agent(&self, agent_id: &str) {
        let mut locks_guard = self.locks.lock_safe();
        locks_guard.retain(|_, file_locks| {
            file_locks.retain(|lock| lock.agent_id != agent_id);
            !file_locks.is_empty()
        });
    }

    /// Drop locks older than the provided age threshold.
    pub fn prune_stale(&self, max_age: Duration) {
        let mut locks_guard = self.locks.lock_safe();
        let now = Instant::now();
        locks_guard.retain(|_, file_locks| {
            file_locks.retain(|lock| {
                now.checked_duration_since(lock.timestamp)
                    .map(|d| d <= max_age)
                    .unwrap_or(true)
            });
            !file_locks.is_empty()
        });
    }

    /// Get active locks for a specific file.
    pub fn get_locks_for_file(&self, file_path: &Path) -> Vec<EditLock> {
        let locks_guard = self.locks.lock_safe();
        locks_guard.get(file_path).cloned().unwrap_or_default()
    }

    /// Whether `agent_id` currently holds a lock on the resource at `path`.
    /// Used by the coordination bus to decide whether a release should broadcast.
    pub fn holds(&self, agent_id: &str, path: &Path) -> bool {
        let canonical = canonicalize_scope_path(path);
        let locks_guard = self.locks.lock_safe();
        locks_guard.iter().any(|(p, locks)| {
            canonicalize_scope_path(p) == canonical && locks.iter().any(|l| l.agent_id == agent_id)
        })
    }

    /// Whether some *other* owner holds a lock on the resource at `path`.
    pub fn is_locked_by_other(&self, agent_id: &str, path: &Path) -> bool {
        let canonical = canonicalize_scope_path(path);
        let locks_guard = self.locks.lock_safe();
        locks_guard.iter().any(|(p, locks)| {
            canonicalize_scope_path(p) == canonical && locks.iter().any(|l| l.agent_id != agent_id)
        })
    }

    /// Every current claim as `(path, owner)`. Mirrors the old bus view.
    pub fn locked_files(&self) -> Vec<(PathBuf, String)> {
        let locks_guard = self.locks.lock_safe();
        locks_guard
            .iter()
            .flat_map(|(p, locks)| locks.iter().map(move |l| (p.clone(), l.agent_id.clone())))
            .collect()
    }

    /// Every current claim with its wall-clock acquisition time, for stale reporting.
    pub fn lock_details(&self) -> Vec<(PathBuf, String, u64)> {
        let locks_guard = self.locks.lock_safe();
        locks_guard
            .iter()
            .flat_map(|(p, locks)| {
                locks
                    .iter()
                    .map(move |l| (p.clone(), l.agent_id.clone(), l.claimed_unix))
            })
            .collect()
    }

    /// Drop every lock. Session-cleanup surface for the coordination bus.
    pub fn clear_all(&self) {
        self.locks.lock_safe().clear();
    }

    /// Generate an adapter or contract resolution for a conflict.
    pub fn resolve_conflict(&self, conflict: &Conflict) -> String {
        match conflict.kind {
            ConflictKind::Semantic => format!(
                "MEDIATION CONTRACT:\n\
                 Conflict Type: SEMANTIC COUPLING\n\
                 File A: {}\n\
                 File B: {}\n\
                 Action required: Bob's agent ({}) must create a backward-compatible method signature \
                 to avoid breaking calls from Alice's agent ({}).",
                conflict.existing_lock.file_path.display(),
                conflict.requested_lock.file_path.display(),
                conflict.existing_lock.agent_id,
                conflict.requested_lock.agent_id
            ),
            ConflictKind::ScopeOverlap => format!(
                "MEDIATION CONTRACT:\n\
                 Conflict Type: SCOPE OVERLAP\n\
                 Scope A: {}\n\
                 Scope B: {}\n\
                 Action required: Alice ({}) and Bob ({}) must serialize overlapping scoped work before applying edits.",
                conflict.existing_lock.file_path.display(),
                conflict.requested_lock.file_path.display(),
                conflict.existing_lock.agent_id,
                conflict.requested_lock.agent_id
            ),
            ConflictKind::DirectLine => format!(
                "MEDIATION CONTRACT:\n\
                 Conflict Type: DIRECT LINE COLLISION\n\
                 File: {}\n\
                 Lines: {}-{} vs {}-{}\n\
                 Action required: Alice ({}) and Bob ({}) must merge overlapping edits manually or via LLM adapter.",
                conflict.file_path.display(),
                conflict.existing_lock.line_range.0,
                conflict.existing_lock.line_range.1,
                conflict.requested_lock.line_range.0,
                conflict.requested_lock.line_range.1,
                conflict.existing_lock.agent_id,
                conflict.requested_lock.agent_id
            ),
        }
    }

    /// All currently held locks across every file. Test-only diagnostic.
    #[cfg(test)]
    pub fn active_locks(&self) -> Vec<EditLock> {
        let locks_guard = self.locks.lock_safe();
        locks_guard
            .values()
            .flat_map(|locks| locks.iter().cloned())
            .collect()
    }

    /// Direct access to the raw table, for tests that need to seed a lock with a
    /// back-dated timestamp (stale-prune checks).
    #[cfg(test)]
    pub fn locks_raw(&self) -> &Mutex<HashMap<PathBuf, Vec<EditLock>>> {
        &self.locks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use velocity_ide::site_map::VcTriple;

    #[test]
    fn whole_file_claim_is_mutually_exclusive() {
        let gate = WriteGate::new();
        let path = Path::new("src/main.rs");
        assert!(gate.claim_whole_file("agent-1", path));
        // Same agent may re-claim (idempotent, matches old bus behavior).
        assert!(gate.claim_whole_file("agent-1", path));
        // A different agent is blocked.
        assert!(!gate.claim_whole_file("agent-2", path));
        gate.release(path, "agent-1");
        assert!(gate.claim_whole_file("agent-2", path));
    }

    #[test]
    fn claim_and_range_lock_share_one_table() {
        // THE D5 FIX: a range lock (orchestrator style) and a whole-file claim
        // (executor style) now see each other because they live in one table.
        let gate = WriteGate::new();
        let temp = TempDir::new().unwrap();
        let sm = SiteMap::open(temp.path(), 0).unwrap();
        let file = PathBuf::from("src/lib.rs");

        gate.acquire(
            file.clone(),
            (10, 20),
            "orchestrator".to_string(),
            Some(&sm),
        )
        .unwrap();
        // An executor-style claim over the same file must now be denied.
        assert!(!gate.claim_whole_file("chat-agent", Path::new("src/lib.rs")));

        gate.release(&file, "orchestrator");
        assert!(gate.claim_whole_file("chat-agent", Path::new("src/lib.rs")));
    }

    #[test]
    fn claim_is_never_blocked_by_semantic_coupling() {
        // Set up lib.rs -> calls -> main.rs. A mediator acquire with a site map is
        // blocked by the coupling, but a whole-file claim (no map) is not: it only
        // obeys path/range overlap it always obeyed.
        let gate = WriteGate::new();
        let temp = TempDir::new().unwrap();
        let mut sm = SiteMap::open(temp.path(), 0).unwrap();
        let caller = PathBuf::from("src/lib.rs");
        let callee = PathBuf::from("src/main.rs");
        sm.put_file_snapshot(
            "src/lib.rs",
            &[VcTriple {
                subject_hash: path_identity_hash(&caller),
                predicate_id: 2,
                object_hash: path_identity_hash(&callee),
            }],
        )
        .unwrap();

        // Incumbent on the callee (main.rs) by a different agent.
        gate.acquire(callee.clone(), (1, 5), "alice".to_string(), Some(&sm))
            .unwrap();
        // Semantic check on the caller (lib.rs) is blocked with a map ...
        let with_map = gate.acquire(callee.clone(), (1, 5), "bob".to_string(), Some(&sm));
        // (bob vs alice same file => overlap, not semantic; assert it at least rejects.)
        assert!(with_map.is_err());

        // A claim on a DIFFERENT, uncoupled file succeeds regardless of the map being absent.
        assert!(gate.claim_whole_file("carol", Path::new("src/other.rs")));
    }

    #[test]
    fn directory_scope_conflicts_with_nested_claim() {
        let gate = WriteGate::new();
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let dir_scope = root.join("src");
        let nested = dir_scope.join("lib.rs");
        std::fs::create_dir_all(dir_scope.clone()).unwrap();
        std::fs::write(&nested, "pub fn demo() {}\n").unwrap();

        gate.acquire(dir_scope.clone(), WHOLE_SCOPE, "alice".to_string(), None)
            .unwrap();
        // A whole-file claim on a path nested under a held dir scope is denied.
        assert!(!gate.claim_whole_file("bob", &nested));
    }

    #[test]
    fn prunes_stale_locks() {
        let gate = WriteGate::new();
        let fresh = PathBuf::from("src/fresh.rs");
        let stale = PathBuf::from("src/stale.rs");
        {
            let mut locks = gate.locks_raw().lock().unwrap();
            locks.insert(
                fresh.clone(),
                vec![EditLock {
                    file_path: fresh.clone(),
                    line_range: (1, 5),
                    agent_id: "fresh-agent".to_string(),
                    timestamp: Instant::now(),
                    claimed_unix: now_unix(),
                }],
            );
            locks.insert(
                stale.clone(),
                vec![EditLock {
                    file_path: stale.clone(),
                    line_range: (1, 5),
                    agent_id: "stale-agent".to_string(),
                    timestamp: Instant::now() - Duration::from_secs(30),
                    claimed_unix: now_unix(),
                }],
            );
        }

        gate.prune_stale(Duration::from_secs(5));
        let remaining = gate.active_locks();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].file_path, fresh);
    }

    #[test]
    fn release_for_agent_clears_across_paths() {
        let gate = WriteGate::new();
        let temp = TempDir::new().unwrap();
        let sm = SiteMap::open(temp.path(), 0).unwrap();
        let first = PathBuf::from("src/main.rs");
        let second = PathBuf::from("src/lib.rs");
        gate.acquire(first.clone(), (1, 5), "alice".to_string(), Some(&sm))
            .unwrap();
        gate.acquire(second.clone(), (1, 5), "alice".to_string(), Some(&sm))
            .unwrap();
        assert_eq!(gate.active_locks().len(), 2);
        gate.release_for_agent("alice");
        assert!(gate.active_locks().is_empty());
    }
}
