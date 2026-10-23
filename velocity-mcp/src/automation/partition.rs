//! System A — per-scope partitioned convergence (see
//! `docs/MULTINODE_COLLAB_GOVERNANCE_SPEC.md` §4).
//!
//! A shared workspace is partitioned so each team's scope carries its own
//! sub-op-log and materializes independently — cheap merges, and cross-team
//! work ("Manufacturing" and "AI integrations" in one repo) converges without
//! serializing on one global fold. The partition *mechanism* is required
//! regardless of granularity, so the granularity policy (open decision **D4**:
//! per-file vs per-module vs per-team) is supplied by the caller as a key
//! function; [`default_partition_key`] is the whole-file baseline.
//!
//! Correctness rests on one property: a triple edge lives in exactly one file's
//! snapshot (`put_file_snapshot` overwrites a file's edges wholesale), so every
//! op that can decide that edge shares the file's partition. Therefore the union
//! of per-partition [`materialize`] results equals the single global fold — for
//! *any* granularity that keeps a file's ops together. The tests assert exactly
//! that (and that D4 is policy, not correctness).

use super::oplog::{materialize, MaterializedState, Op, OpEffect, OpSet};
use std::collections::{BTreeMap, BTreeSet};

/// A partition identifier (a file path, module prefix, team scope, …).
pub type PartitionKey = String;

/// Ops whose effect is not scope-affined (Text/Legacy/Approve) land here so they
/// are never dropped from the fold.
const GLOBAL: &str = "";

/// Whole-file baseline granularity: a triple partitions by its file, a claim /
/// release by its scope. Callers may pass a coarser closure to implement D4.
pub fn default_partition_key(op: &Op) -> PartitionKey {
    match &op.effect {
        OpEffect::Triple(t) => t.file_path.clone(),
        OpEffect::Claim { scope, .. } | OpEffect::Release { scope } => {
            scope.to_string_lossy().into_owned()
        }
        OpEffect::Text { path, .. } => path.to_string_lossy().into_owned(),
        OpEffect::Legacy | OpEffect::Approve { .. } => GLOBAL.to_string(),
    }
}

/// Group an op set into independent per-partition sub-logs.
pub fn partition_by(
    set: &OpSet,
    key_of: impl Fn(&Op) -> PartitionKey,
) -> BTreeMap<PartitionKey, OpSet> {
    let mut map: BTreeMap<PartitionKey, OpSet> = BTreeMap::new();
    for op in set.ops() {
        map.entry(key_of(op)).or_default().push(op.clone());
    }
    map
}

/// Materialize each partition independently.
pub fn materialize_partitioned(
    set: &OpSet,
    key_of: impl Fn(&Op) -> PartitionKey,
) -> BTreeMap<PartitionKey, MaterializedState> {
    partition_by(set, key_of)
        .into_iter()
        .map(|(key, sub)| (key, materialize(&sub)))
        .collect()
}

/// The union of live triples across all partitions — for comparing against a
/// single global [`materialize`] fold.
pub fn union_live_triples(
    partitions: &BTreeMap<PartitionKey, MaterializedState>,
) -> BTreeSet<(u64, u16, u64)> {
    partitions
        .values()
        .flat_map(|state| state.live_triples.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::oplog::TripleEffect;
    use crate::automation::principal::{NodeId, Principal, VectorClock};
    use crate::registry::event_store::EventOutcome;
    use velocity_ide::site_map::VcTriple;

    fn triple(subj: u64, obj: u64) -> VcTriple {
        VcTriple {
            subject_hash: subj,
            predicate_id: 2,
            object_hash: obj,
        }
    }

    /// A triple-add op whose `op_id` counter is forced to `counter` (so LWW
    /// winners are deterministic in the tests).
    fn add_op(node: &str, counter: u64, file: &str, edges: Vec<VcTriple>, rm: Vec<VcTriple>) -> Op {
        Op {
            op_id: crate::automation::principal::OpId::new(NodeId::new(node), counter),
            clock: VectorClock::new(),
            principal: Principal::agent(NodeId::new(node), counter, None, None),
            causal_deps: Vec::new(),
            intent: String::new(),
            effect: OpEffect::Triple(TripleEffect {
                file_path: file.to_string(),
                add: edges,
                rm,
            }),
            outcome: EventOutcome::Success,
            authorized_by: None,
        }
    }

    fn set_of(ops: Vec<Op>) -> OpSet {
        let mut s = OpSet::new();
        for o in ops {
            s.push(o);
        }
        s
    }

    /// D4 as a caller seam: coarse per-module key = the first path segment.
    fn module_key(op: &Op) -> PartitionKey {
        match &op.effect {
            OpEffect::Triple(t) => t.file_path.split('/').next().unwrap_or(GLOBAL).to_string(),
            _ => default_partition_key(op),
        }
    }

    #[test]
    fn partitioned_union_equals_global_fold() {
        let set = set_of(vec![
            add_op("a", 1, "src/a.rs", vec![triple(1, 1), triple(2, 2)], vec![]),
            add_op("b", 2, "src/b.rs", vec![triple(3, 3)], vec![]),
            // b later removes one of a's edges within b's own file snapshot.
            add_op("b", 3, "src/b.rs", vec![], vec![triple(3, 3)]),
        ]);

        let global = materialize(&set);
        let union = union_live_triples(&materialize_partitioned(&set, default_partition_key));
        assert_eq!(global.live_triples, union);
    }

    #[test]
    fn granularity_is_policy_not_correctness() {
        let set = set_of(vec![
            add_op("a", 1, "src/a.rs", vec![triple(1, 1)], vec![]),
            add_op(
                "b",
                2,
                "src/mod/b.rs",
                vec![triple(2, 2), triple(4, 4)],
                vec![],
            ),
            add_op("b", 3, "src/mod/b.rs", vec![], vec![triple(4, 4)]),
        ]);

        let global = materialize(&set);
        let by_file = union_live_triples(&materialize_partitioned(&set, default_partition_key));
        let by_module = union_live_triples(&materialize_partitioned(&set, module_key));

        // Whole-file and per-module partitioning both reproduce the global set,
        // so D4 only changes merge cost, never the converged result.
        assert_eq!(global.live_triples, by_file);
        assert_eq!(global.live_triples, by_module);
    }

    #[test]
    fn each_partition_converges_under_permutation() {
        let ops = vec![
            add_op("a", 1, "src/a.rs", vec![triple(7, 7)], vec![]),
            add_op("a", 2, "src/a.rs", vec![triple(8, 8)], vec![]),
            add_op("b", 5, "src/b.rs", vec![triple(9, 9)], vec![]),
        ];
        let forward = materialize_partitioned(&set_of(ops.clone()), default_partition_key);
        let mut reversed = ops;
        reversed.reverse();
        let backward = materialize_partitioned(&set_of(reversed), default_partition_key);

        // Independent partitions converge the same regardless of arrival order.
        assert_eq!(forward, backward);
        assert_eq!(forward.len(), 2); // src/a.rs and src/b.rs
    }

    #[test]
    fn non_scope_effects_are_not_dropped() {
        // A Legacy op lands in the GLOBAL partition rather than vanishing.
        let mut legacy = add_op("a", 1, "src/a.rs", vec![], vec![]);
        legacy.effect = OpEffect::Legacy;
        let parts = partition_by(&set_of(vec![legacy]), default_partition_key);
        assert!(parts.contains_key(GLOBAL));
    }
}
