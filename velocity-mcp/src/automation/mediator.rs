use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::sync::Mutex;
use velocity_ide::site_map::SiteMap;

#[derive(Clone, Debug)]
pub struct EditLock {
    pub file_path: PathBuf,
    pub line_range: (usize, usize),
    pub agent_id: String,
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub file_path: PathBuf,
    pub existing_lock: EditLock,
    pub requested_lock: EditLock,
    pub is_semantic: bool, // true if caught via callers/dependencies coupling
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
    pub fn acquire_lock(
        &self,
        file_path: PathBuf,
        line_range: (usize, usize),
        agent_id: String,
        site_map: &SiteMap,
    ) -> Result<(), Conflict> {
        let mut locks_guard = self.locks.lock().unwrap();

        let requested = EditLock {
            file_path: file_path.clone(),
            line_range,
            agent_id: agent_id.clone(),
            timestamp: Instant::now(),
        };

        // 1. Direct line overlap check in the same file
        if let Some(file_locks) = locks_guard.get(&file_path) {
            for existing in file_locks {
                if existing.agent_id != agent_id {
                    let (req_start, req_end) = line_range;
                    let (exist_start, exist_end) = existing.line_range;
                    if req_start <= exist_end && req_end >= exist_start {
                        return Err(Conflict {
                            file_path: file_path.clone(),
                            existing_lock: existing.clone(),
                            requested_lock: requested,
                            is_semantic: false,
                        });
                    }
                }
            }
        }

        // 2. Semantic coupling check across different files/methods.
        // Use canonical workspace-relative path identities so same-named files in
        // different directories do not collide.
        let file_identity_hash = path_identity_hash(&file_path);
        let callers = site_map.get_callers(file_identity_hash);

        for (other_path, other_locks) in locks_guard.iter() {
            if other_path != &file_path {
                for existing in other_locks {
                    if existing.agent_id != agent_id {
                        let other_identity_hash = path_identity_hash(other_path);
                        if callers.contains(&other_identity_hash) {
                            return Err(Conflict {
                                file_path: file_path.clone(),
                                existing_lock: existing.clone(),
                                requested_lock: requested,
                                is_semantic: true,
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
        if let Some(file_locks) = locks_guard.get_mut(file_path) {
            file_locks.retain(|lock| lock.agent_id != agent_id);
        }
    }

    /// Get active locks for a specific file.
    pub fn get_locks_for_file(&self, file_path: &Path) -> Vec<EditLock> {
        let locks_guard = self.locks.lock().unwrap();
        locks_guard.get(file_path).cloned().unwrap_or_default()
    }

    pub fn active_locks(&self) -> Vec<EditLock> {
        let locks_guard = self.locks.lock().unwrap();
        locks_guard.values().flat_map(|locks| locks.iter().cloned()).collect()
    }

    /// Generate an adapter or contract resolution for a conflict.
    pub fn resolve_conflict(&self, conflict: &Conflict) -> String {
        if conflict.is_semantic {
            format!(
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
            )
        } else {
            format!(
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
            )
        }
    }
}

fn path_identity_hash(path: &Path) -> u64 {
    let canonical = canonicalize_scope_path(path);
    hash_str(&canonical)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use velocity_ide::site_map::NdaNode;

    #[test]
    fn test_mediator_locks_and_conflicts() {
        let arena = MediatorArena::new();
        let file_a = PathBuf::from("src/main.rs");
        let file_b = PathBuf::from("src/lib.rs");

        let temp_dir = TempDir::new().unwrap();
        let mut sm = SiteMap::open(temp_dir.path(), 0).unwrap();

        // 1. Check direct line overlap lock block
        arena.acquire_lock(file_a.clone(), (10, 20), "AgentAlice".to_string(), &sm).unwrap();

        // Second lock request on same range by AgentBob should fail
        let res = arena.acquire_lock(file_a.clone(), (15, 25), "AgentBob".to_string(), &sm);
        assert!(res.is_err());
        let conflict = res.err().unwrap();
        assert_eq!(conflict.is_semantic, false);
        assert_eq!(conflict.existing_lock.agent_id, "AgentAlice");

        // Requesting non-overlapping range on same file succeeds
        arena.acquire_lock(file_a.clone(), (30, 40), "AgentBob".to_string(), &sm).unwrap();

        // 2. Check semantic coupling lock block
        // Setup SiteMap: "src/lib.rs" calls "src/main.rs"
        let lib_hash = path_identity_hash(&file_b);
        let main_hash = path_identity_hash(&file_a);
        let triple = NdaNode::Triple { subject_hash: lib_hash, predicate_id: 2, object_hash: main_hash };
        sm.put_node(&triple).unwrap();

        // Lock file_b (lib.rs) by AgentAlice
        arena.acquire_lock(file_b.clone(), (1, 10), "AgentAlice".to_string(), &sm).unwrap();

        // Lock file_a (main.rs) by AgentBob should trigger semantic coupling conflict
        // because main.rs is called by lib.rs (which is held by AgentAlice)
        let res_sem = arena.acquire_lock(file_a.clone(), (1, 9), "AgentBob".to_string(), &sm);
        assert!(res_sem.is_err());
        let conflict_sem = res_sem.err().unwrap();
        assert_eq!(conflict_sem.is_semantic, true);
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
        sm.put_node(&NdaNode::Triple {
            subject_hash: path_identity_hash(&caller),
            predicate_id: 2,
            object_hash: path_identity_hash(&callee),
        })
        .unwrap();

        arena.acquire_lock(caller.clone(), (1, 5), "AgentAlice".to_string(), &sm).unwrap();

        let conflict = arena.acquire_lock(callee.clone(), (1, 5), "AgentBob".to_string(), &sm);
        assert!(conflict.is_err());
        assert!(conflict.as_ref().err().unwrap().is_semantic);

        arena.release_lock(&caller, "AgentAlice");

        let unrelated_result = arena.acquire_lock(unrelated.clone(), (1, 5), "AgentBob".to_string(), &sm);
        assert!(unrelated_result.is_ok());
    }
}
