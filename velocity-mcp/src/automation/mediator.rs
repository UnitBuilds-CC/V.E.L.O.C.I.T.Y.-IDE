//! Write-admission facade for the orchestrator and GUI call sites.
//!
//! The authoritative lock table now lives in
//! [`crate::automation::write_gate::WriteGate`]. [`MediatorArena`] is a thin
//! wrapper over it, kept so that existing `crate::automation::mediator::*`
//! imports and method names (`acquire_lock`, `release_lock`, …) continue to
//! resolve unchanged. Both the mediator path (range + semantic check) and the
//! coordination-bus whole-file claim now share the *same* [`WriteGate`] table,
//! which is what closes the two-gates-cannot-see-each-other defect (spec D5).

use crate::automation::write_gate::WriteGate;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use velocity_ide::site_map::SiteMap;

// Re-export the shared lock vocabulary so `crate::automation::mediator::{EditLock,
// Conflict, ConflictKind}` keep resolving for the GUI renderer and orchestrator scope.
pub use crate::automation::write_gate::{Conflict, ConflictKind, EditLock};

pub struct MediatorArena {
    gate: Arc<WriteGate>,
}

impl Default for MediatorArena {
    fn default() -> Self {
        Self::new()
    }
}

impl MediatorArena {
    pub fn new() -> Self {
        Self {
            gate: Arc::new(WriteGate::new()),
        }
    }

    /// Build an arena that shares an existing gate instance. This is how the app's
    /// one arena and the executor's coordination bus come to consult the *same*
    /// lock table (spec D5 runtime unify).
    pub fn from_gate(gate: Arc<WriteGate>) -> Self {
        Self { gate }
    }

    /// The underlying shared table.
    pub fn gate(&self) -> &WriteGate {
        &self.gate
    }

    /// A cloneable handle to the shared gate, for handing to the coordination bus
    /// so both write paths admit through one instance rather than one *type*.
    pub fn gate_arc(&self) -> Arc<WriteGate> {
        self.gate.clone()
    }

    /// Try to acquire an edit lock. Returns `Err(Conflict)` if an overlap or coupling
    /// conflict is found. Always runs the semantic check (a site map is supplied).
    pub fn acquire_lock(
        &self,
        file_path: PathBuf,
        line_range: (usize, usize),
        agent_id: String,
        site_map: &SiteMap,
    ) -> Result<(), Conflict> {
        self.gate
            .acquire(file_path, line_range, agent_id, Some(site_map))
    }

    /// Release locks held by an agent on a file.
    pub fn release_lock(&self, file_path: &Path, agent_id: &str) {
        self.gate.release(file_path, agent_id);
    }

    /// Release every lock held by a specific agent across all tracked paths.
    pub fn release_locks_for_agent(&self, agent_id: &str) {
        self.gate.release_for_agent(agent_id);
    }

    /// Drop locks older than the provided age threshold.
    pub fn prune_stale_locks(&self, max_age: Duration) {
        self.gate.prune_stale(max_age);
    }

    /// Get active locks for a specific file.
    pub fn get_locks_for_file(&self, file_path: &Path) -> Vec<EditLock> {
        self.gate.get_locks_for_file(file_path)
    }

    /// Generate an adapter or contract resolution for a conflict.
    pub fn resolve_conflict(&self, conflict: &Conflict) -> String {
        self.gate.resolve_conflict(conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::write_gate::path_identity_hash;
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
        let sm = SiteMap::open(temp_dir.path(), 0).unwrap();

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
    fn facade_claim_shares_the_mediator_table() {
        // Prove the facade's gate is the one table: a claim through gate() is
        // visible to a subsequent acquire_lock and vice versa.
        let arena = MediatorArena::new();
        let temp_dir = TempDir::new().unwrap();
        let sm = SiteMap::open(temp_dir.path(), 0).unwrap();
        let file = PathBuf::from("src/main.rs");

        assert!(arena.gate().claim_whole_file("chat-agent", &file));
        let blocked = arena.acquire_lock(file.clone(), (1, 5), "orchestrator".to_string(), &sm);
        assert!(blocked.is_err());
        assert_eq!(blocked.err().unwrap().kind, ConflictKind::ScopeOverlap);
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
        assert_eq!(arena.gate().active_locks().len(), 2);

        arena.release_locks_for_agent("AgentAlice");
        assert!(arena.gate().active_locks().is_empty());
    }
}
