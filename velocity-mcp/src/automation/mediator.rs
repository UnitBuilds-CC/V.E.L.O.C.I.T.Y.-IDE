use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use velocity_ide::site_map::SiteMap;

#[derive(Clone, Debug)]
pub struct EditLock {
    pub file_path: PathBuf,
    pub line_range: (usize, usize),
    pub agent_id: String,
    pub timestamp: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    DirectLine,
    ScopeOverlap,
    Semantic,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub file_path: PathBuf,
    pub existing_lock: EditLock,
    pub requested_lock: EditLock,
    pub kind: ConflictKind,
}

fn is_dir_scope(path: &Path, range: (usize, usize)) -> bool {
    path.is_dir()
        || path.extension().is_none()
        || path.to_string_lossy().ends_with('/')
        || path.to_string_lossy().ends_with('\\')
        || range.1 == usize::MAX
        || range.1 == usize::MAX / 4
}

fn path_identity_hash(path: &Path) -> u64 {
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

fn hash_str(s: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

pub struct MediatorArena {
    locks: Mutex<HashMap<PathBuf, Vec<EditLock>>>,
}

impl MediatorArena {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// Try to acquire an edit lock. Returns Err(Conflict) if an overlap or coupling conflict is found.
    /// Try to acquire an edit lock. Returns Err(Conflict) if an overlap or coupling conflict is found.
    pub fn acquire_lock(
        &self,
        file_path: PathBuf,
        line_range: (usize, usize),
        agent_id: String,
        site_map: &SiteMap,
    ) -> Result<(), Conflict> {
        self.prune_stale_locks(Duration::from_secs(1800));
        let mut locks_guard = self.locks.lock().unwrap();

        let requested = EditLock {
            file_path: file_path.clone(),
            line_range,
            agent_id: agent_id.clone(),
            timestamp: Instant::now(),
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

        // 2. Semantic coupling check across different files/methods.
        // Use canonical workspace-relative path identities so same-named files in
        // different directories do not collide.
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

        // Insert new lock
        locks_guard.entry(file_path).or_default().push(requested);
        Ok(())
    }

    /// Release locks held by an agent on a file.
    pub fn release_lock(&self, file_path: &Path, agent_id: &str) {
        let mut locks_guard = self.locks.lock().unwrap();
        let target_canonical = canonicalize_scope_path(file_path);
        locks_guard.retain(|path, file_locks| {
            if canonicalize_scope_path(path) == target_canonical {
                file_locks.retain(|lock| lock.agent_id != agent_id);
            }
            !file_locks.is_empty()
        });
    }

    /// Release every lock held by a specific agent across all tracked paths.
    pub fn release_locks_for_agent(&self, agent_id: &str) {
        let mut locks_guard = self.locks.lock().unwrap();
        locks_guard.retain(|_, file_locks| {
            file_locks.retain(|lock| lock.agent_id != agent_id);
            !file_locks.is_empty()
        });
    }

    /// Drop locks older than the provided age threshold.
    pub fn prune_stale_locks(&self, max_age: Duration) {
        let mut locks_guard = self.locks.lock().unwrap();
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
        let locks_guard = self.locks.lock().unwrap();
        locks_guard.get(file_path).cloned().unwrap_or_default()
    }

    /// All currently held locks across every file. Test-only diagnostic.
    #[cfg(test)]
    pub fn active_locks(&self) -> Vec<EditLock> {
        let locks_guard = self.locks.lock().unwrap();
        locks_guard
            .values()
            .flat_map(|locks| locks.iter().cloned())
            .collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use velocity_ide::site_map::VcTriple;

    #[test]
    fn test_mediator_locks_and_conflicts() {
        let arena = MediatorArena::new();
        let file_a = PathBuf::from("src/main.rs");
        let file_b = PathBuf::from("src/lib.rs");

        let temp_dir = TempDir::new().unwrap();
        let mut sm = SiteMap::open(temp_dir.path(), 0).unwrap();

        // 1. Check direct line overlap lock block
        arena
            .acquire_lock(file_a.clone(), (10, 20), "AgentAlice".to_string(), &sm)
            .unwrap();

        // Second lock request on same range by AgentBob should fail
        let res = arena.acquire_lock(file_a.clone(), (15, 25), "AgentBob".to_string(), &sm);
        assert!(res.is_err());
        let conflict = res.err().unwrap();
        assert_eq!(conflict.kind, ConflictKind::DirectLine);
        assert_eq!(conflict.existing_lock.agent_id, "AgentAlice");

        // Requesting non-overlapping range on same file succeeds
        arena
            .acquire_lock(file_a.clone(), (30, 40), "AgentBob".to_string(), &sm)
            .unwrap();

        // 2. Check semantic coupling lock block
        // Setup SiteMap: "src/lib.rs" calls "src/main.rs"
        let lib_hash = path_identity_hash(&file_b);
        let main_hash = path_identity_hash(&file_a);
        sm.put_file_snapshot(
            "src/lib.rs",
            &[VcTriple {
                subject_hash: lib_hash,
                predicate_id: 2,
                object_hash: main_hash,
            }],
        )
        .unwrap();

        // Lock file_b (lib.rs) by AgentAlice
        arena
            .acquire_lock(file_b.clone(), (1, 10), "AgentAlice".to_string(), &sm)
            .unwrap();

        // Lock file_a (main.rs) by AgentBob should trigger semantic coupling conflict
        // because main.rs is called by lib.rs (which is held by AgentAlice)
        let res_sem = arena.acquire_lock(file_a.clone(), (1, 9), "AgentBob".to_string(), &sm);
        assert!(res_sem.is_err());
        let conflict_sem = res_sem.err().unwrap();
        assert_eq!(conflict_sem.kind, ConflictKind::Semantic);
        assert_eq!(conflict_sem.existing_lock.agent_id, "AgentAlice");

        // Verify conflict resolution text output
        let contract = arena.resolve_conflict(&conflict_sem);
        assert!(contract.contains("Conflict Type: SEMANTIC COUPLING"));
    }

    #[test]
    fn semantic_conflicts_use_canonical_paths_for_same_named_files() {
        let arena = MediatorArena::new();
        let caller = PathBuf::from("src/feature/a.rs");
        let callee = PathBuf::from("src/shared/a.rs");
        let unrelated = PathBuf::from("src/other/a.rs");

        let temp_dir = TempDir::new().unwrap();
        let mut sm = SiteMap::open(temp_dir.path(), 0).unwrap();
        sm.put_file_snapshot(
            "src/feature/a.rs",
            &[VcTriple {
                subject_hash: path_identity_hash(&caller),
                predicate_id: 2,
                object_hash: path_identity_hash(&callee),
            }],
        )
        .unwrap();

        arena
            .acquire_lock(caller.clone(), (1, 5), "AgentAlice".to_string(), &sm)
            .unwrap();

        let conflict = arena.acquire_lock(callee.clone(), (1, 5), "AgentBob".to_string(), &sm);
        assert!(conflict.is_err());
        assert_eq!(
            conflict.as_ref().err().unwrap().kind,
            ConflictKind::Semantic
        );

        arena.release_lock(&caller, "AgentAlice");

        let unrelated_result =
            arena.acquire_lock(unrelated.clone(), (1, 5), "AgentBob".to_string(), &sm);
        assert!(unrelated_result.is_ok());
    }

    #[test]
    fn directory_scopes_conflict_with_nested_files() {
        let arena = MediatorArena::new();
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();
        let mut sm = SiteMap::open(temp_dir.path(), 0).unwrap();

        let dir_scope = workspace_root.join("src");
        let nested_file = workspace_root.join("src").join("lib.rs");
        std::fs::create_dir_all(dir_scope.clone()).unwrap();
        std::fs::write(&nested_file, "pub fn demo() {}\n").unwrap();

        arena
            .acquire_lock(
                dir_scope.clone(),
                (1, usize::MAX / 4),
                "AgentAlice".to_string(),
                &sm,
            )
            .unwrap();

        let conflict = arena.acquire_lock(
            nested_file.clone(),
            (1, usize::MAX / 4),
            "AgentBob".to_string(),
            &sm,
        );
        assert!(conflict.is_err());
        let conflict = conflict.err().unwrap();
        assert_eq!(conflict.kind, ConflictKind::ScopeOverlap);
        assert!(arena
            .resolve_conflict(&conflict)
            .contains("Conflict Type: SCOPE OVERLAP"));
    }

    #[test]
    fn can_release_locks_for_agent_across_paths() {
        let arena = MediatorArena::new();
        let temp_dir = TempDir::new().unwrap();
        let sm = SiteMap::open(temp_dir.path(), 0).unwrap();
        let first = PathBuf::from("src/main.rs");
        let second = PathBuf::from("src/lib.rs");

        arena
            .acquire_lock(first.clone(), (1, 5), "AgentAlice".to_string(), &sm)
            .unwrap();
        arena
            .acquire_lock(second.clone(), (1, 5), "AgentAlice".to_string(), &sm)
            .unwrap();
        assert_eq!(arena.active_locks().len(), 2);

        arena.release_locks_for_agent("AgentAlice");
        assert!(arena.active_locks().is_empty());
    }

    #[test]
    fn prunes_stale_locks() {
        let arena = MediatorArena::new();
        let fresh = PathBuf::from("src/fresh.rs");
        let stale = PathBuf::from("src/stale.rs");
        {
            let mut locks = arena.locks.lock().unwrap();
            locks.insert(
                fresh.clone(),
                vec![EditLock {
                    file_path: fresh.clone(),
                    line_range: (1, 5),
                    agent_id: "fresh-agent".to_string(),
                    timestamp: Instant::now(),
                }],
            );
            locks.insert(
                stale.clone(),
                vec![EditLock {
                    file_path: stale.clone(),
                    line_range: (1, 5),
                    agent_id: "stale-agent".to_string(),
                    timestamp: Instant::now() - Duration::from_secs(30),
                }],
            );
        }

        arena.prune_stale_locks(Duration::from_secs(5));
        let remaining = arena.active_locks();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].file_path, fresh);
    }
}
