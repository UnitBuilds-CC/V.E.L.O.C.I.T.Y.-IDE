//! Spine-2 — the operation log as the source of truth (see
//! `docs/MULTINODE_COLLAB_GOVERNANCE_SPEC.md` §2).
//!
//! The existing `events.jsonl` (`registry/event_store.rs`) is *a log of tool
//! calls*. This module evolves that idea into a replicated **operation log**: the
//! site map becomes a *derived materialized view* of an append-only [`OpSet`],
//! rather than the authoritative store.
//!
//! The load-bearing property is [`materialize`]: replaying the same op set must
//! yield the same state **regardless of the order ops arrive in** (INV-2,
//! convergence) and **regardless of duplicated ops** (INV-3, idempotence). We
//! prove that here for the triple (site-map edge) effect with a
//! last-write-wins-by-`OpId` fold — `OpId` is totally ordered, so the "winner"
//! of every triple is a pure `max`, which is order-independent and idempotent by
//! construction. The `Text` effect is left as a `MergePolicy` seam (open
//! decision D1): the fold currently tracks only a content digest and does not
//! resolve byte-level merges, so text convergence is explicitly *not* claimed
//! until Phase 3.
//!
//! Back-compat (INV-4): a legacy log line has no `principal`/`clock`/`deps`; it
//! loads via `#[serde(default)]` as a [`PrincipalKind::Service`] op with an empty
//! clock and an inert [`OpEffect::Legacy`] effect, so it never contributes to the
//! materialized state — the decision trail keeps rendering exactly as before.

use super::principal::{OpId, Principal, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use velocity_ide::site_map::VcTriple;

/// Reuse the event store's outcome vocabulary — the op log and the decision
/// trail must not diverge on what "success/failure/revert/pending" mean.
pub use crate::registry::event_store::EventOutcome;

/// Default outcome for a legacy log line that predates the field: undetermined.
fn default_outcome() -> EventOutcome {
    EventOutcome::Pending
}

/// A content-addressed change to the site-map edge set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TripleEffect {
    pub file_path: String,
    /// Edges this op introduces.
    pub add: Vec<VcTriple>,
    /// Edges this op removes.
    pub rm: Vec<VcTriple>,
}

/// The effect an op has on shared state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpEffect {
    /// Site-map edge change (the CRDT-critical, convergent path).
    Triple(TripleEffect),
    /// Byte-level change to a file. Carries only a content digest for now; the
    /// merge strategy is the Phase-3 `MergePolicy` seam (open decision D1).
    Text { path: PathBuf, digest: String },
    /// Advisory cross-node lease over a scope + line range (spec §4).
    Claim {
        scope: PathBuf,
        range: (usize, usize),
    },
    /// Release a previously claimed scope.
    Release { scope: PathBuf },
    /// A governance decision, linked to a `company_org` `ActionRequest.id`.
    Approve { request_id: String, verdict: String },
    /// An inert effect for pre-refactor log lines that carry no op state.
    #[default]
    Legacy,
}

/// A single replicated operation — the atomic unit of the source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub op_id: OpId,
    #[serde(default)]
    pub clock: VectorClock,
    #[serde(default)]
    pub principal: Principal,
    /// Ops this one supersedes / reads (causal edges for dependency resolution).
    #[serde(default)]
    pub causal_deps: Vec<OpId>,
    /// The *why* — already captured today as `CodebaseEvent.context`.
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub effect: OpEffect,
    #[serde(default = "default_outcome")]
    pub outcome: EventOutcome,
    /// Links this op to the `company_org` `ActionRequest.id` that authorized it
    /// (Spine-3 causal audit).
    #[serde(default)]
    pub authorized_by: Option<String>,
}

impl Op {
    /// Construct a triple op, bumping the node's clock and stamping the `op_id`
    /// from `(node, counter)` so identity and ordering share one source.
    pub fn new_triple(
        mut clock: VectorClock,
        principal: Principal,
        file_path: impl Into<String>,
        add: Vec<VcTriple>,
        rm: Vec<VcTriple>,
        intent: impl Into<String>,
    ) -> Self {
        let counter = clock.increment(&principal.node_id);
        let op_id = OpId::new(principal.node_id.clone(), counter);
        Self {
            op_id,
            clock,
            principal,
            causal_deps: Vec::new(),
            intent: intent.into(),
            effect: OpEffect::Triple(TripleEffect {
                file_path: file_path.into(),
                add,
                rm,
            }),
            outcome: EventOutcome::Pending,
            authorized_by: None,
        }
    }

    /// Construct a byte-level text-change op — the shape a watcher emits when a
    /// file is edited (by a human or an agent). It bumps the node's clock and
    /// stamps `op_id` from `(node, counter)` like [`Op::new_triple`]. The op carries
    /// only a content digest (the byte-merge strategy is the D1 `MergePolicy` seam);
    /// it contributes to the durable trail and dependency re-flagging, not to the
    /// converged triple set.
    pub fn new_text(
        mut clock: VectorClock,
        principal: Principal,
        path: impl Into<PathBuf>,
        digest: impl Into<String>,
        intent: impl Into<String>,
    ) -> Self {
        let counter = clock.increment(&principal.node_id);
        let op_id = OpId::new(principal.node_id.clone(), counter);
        Self {
            op_id,
            clock,
            principal,
            causal_deps: Vec::new(),
            intent: intent.into(),
            effect: OpEffect::Text {
                path: path.into(),
                digest: digest.into(),
            },
            outcome: EventOutcome::Pending,
            authorized_by: None,
        }
    }
}

/// The append-only operation log. Convergence is a property of how this set is
/// [`materialize`]d, not of insertion order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpSet {
    ops: Vec<Op>,
}

impl OpSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// Append an op. Duplicate `op_id`s are accepted into the log but are
    /// neutralized by [`materialize`] (INV-3), not by rejection here — the log
    /// is append-only and gossip-safe.
    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }
}

/// A triple key derived from [`VcTriple`]; `VcTriple` itself is not `Ord`/`Hash`,
/// but its three fields are, so we order/dedupe on the tuple.
type TripleKey = (u64, u16, u64);

fn key_of(t: &VcTriple) -> TripleKey {
    (t.subject_hash, t.predicate_id, t.object_hash)
}

/// The site-map state derived from an op set: which edges are live, plus the
/// current scope claims.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterializedState {
    /// Live triple set, ordered for deterministic iteration / hashing.
    pub live_triples: std::collections::BTreeSet<TripleKey>,
    /// Highest-`OpId` claim per scope (present iff not later released).
    pub claims: BTreeMap<PathBuf, OpId>,
}

/// Fold an [`OpSet`] into a [`MaterializedState`].
///
/// For every triple we keep only the winning op — the one with the greatest
/// `OpId` that touched it (add ⇒ present, rm ⇒ absent). Because `max` is
/// associative, commutative and idempotent, the result is independent of both
/// op order and duplicate ops: INV-2 + INV-3. `Legacy`/`Text` ops do not touch
/// the triple set, so pre-refactor lines are inert (INV-4).
pub fn materialize(set: &OpSet) -> MaterializedState {
    // Winner per triple: (op that decided it, present?).
    let mut triple_winners: BTreeMap<TripleKey, (OpId, bool)> = BTreeMap::new();
    let mut claim_winners: BTreeMap<PathBuf, (OpId, bool)> = BTreeMap::new();

    for op in &set.ops {
        match &op.effect {
            OpEffect::Triple(t) => {
                for edge in &t.add {
                    record_winner(&mut triple_winners, key_of(edge), &op.op_id, true);
                }
                for edge in &t.rm {
                    record_winner(&mut triple_winners, key_of(edge), &op.op_id, false);
                }
            }
            OpEffect::Claim { scope, .. } => {
                record_winner(&mut claim_winners, scope.clone(), &op.op_id, true);
            }
            OpEffect::Release { scope } => {
                record_winner(&mut claim_winners, scope.clone(), &op.op_id, false);
            }
            // Text/Legacy/Approve do not contribute authoritative site-map state
            // at this phase (text merge is D1; approvals are audit-only).
            OpEffect::Text { .. } | OpEffect::Legacy | OpEffect::Approve { .. } => {}
        }
    }

    let live_triples = triple_winners
        .into_iter()
        .filter(|(_, (_, present))| *present)
        .map(|(key, _)| key)
        .collect();
    let claims = claim_winners
        .into_iter()
        .filter(|(_, (_, held))| *held)
        .map(|(scope, (winner, _))| (scope, winner))
        .collect();

    MaterializedState {
        live_triples,
        claims,
    }
}

/// Keep the greatest-`OpId` decision for a key (LWW).
fn record_winner<K: Ord + Clone>(
    winners: &mut BTreeMap<K, (OpId, bool)>,
    key: K,
    op_id: &OpId,
    present: bool,
) {
    match winners.get(&key) {
        Some((best_id, _)) if *best_id >= *op_id => {}
        _ => {
            winners.insert(key, (op_id.clone(), present));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::principal::{NodeId, PrincipalKind};

    fn svc_node() -> NodeId {
        NodeId::new("n1")
    }

    fn triple(subj: u64, obj: u64) -> VcTriple {
        VcTriple {
            subject_hash: subj,
            predicate_id: 2,
            object_hash: obj,
        }
    }

    fn agent(name: &str, task: u64) -> Principal {
        Principal::agent(NodeId::new(name), task, None, None)
    }

    #[test]
    fn materialize_is_permutation_invariant() {
        // Two concurrent ops touching overlapping triples.
        let mut s1 = VectorClock::new();
        s1.increment(&NodeId::new("a"));
        let add = Op::new_triple(s1, agent("a", 1), "f.rs", vec![triple(1, 2)], vec![], "add");
        let mut s2 = VectorClock::new();
        s2.increment(&NodeId::new("b"));
        let rm = Op::new_triple(s2, agent("b", 1), "f.rs", vec![], vec![triple(1, 2)], "rm");

        let mut forward = OpSet::new();
        forward.push(add.clone());
        forward.push(rm.clone());

        let mut backward = OpSet::new();
        backward.push(rm.clone());
        backward.push(add.clone());

        // b#1 > a#1, so the removal wins regardless of arrival order (INV-2).
        let f = materialize(&forward);
        let b = materialize(&backward);
        assert_eq!(f, b);
        assert!(!f.live_triples.contains(&(1, 2, 2)));
    }

    #[test]
    fn materialize_is_idempotent_under_duplicate_ops() {
        let mut s = VectorClock::new();
        s.increment(&svc_node());
        let op = Op::new_triple(s, agent("n1", 1), "f.rs", vec![triple(5, 6)], vec![], "add");

        let mut once = OpSet::new();
        once.push(op.clone());

        let mut twice = OpSet::new();
        twice.push(op.clone());
        twice.push(op.clone()); // duplicated via gossip

        // INV-3: replaying the same op_id changes nothing.
        assert_eq!(materialize(&once), materialize(&twice));
        assert!(materialize(&once).live_triples.contains(&(5, 2, 6)));
    }

    #[test]
    fn legacy_line_loads_as_inert_service_op() {
        // A pre-refactor events.jsonl line has none of the new fields.
        let legacy_json = r#"{
            "op_id": {"node": "legacy", "counter": 0}
        }"#;
        let op: Op = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(op.principal.kind, PrincipalKind::Service);
        assert_eq!(op.effect, OpEffect::Legacy);
        assert!(op.clock.total() == 0);

        let mut set = OpSet::new();
        set.push(op);
        // INV-4: a legacy op contributes no authoritative state.
        assert!(materialize(&set).live_triples.is_empty());
    }

    #[test]
    fn triple_op_round_trips_through_json() {
        let mut s = VectorClock::new();
        s.increment(&svc_node());
        let op = Op::new_triple(s, agent("n1", 1), "f.rs", vec![triple(1, 2)], vec![], "why");
        let mut set = OpSet::new();
        set.push(op.clone());
        let json = serde_json::to_string(&set).unwrap();
        let back: OpSet = serde_json::from_str(&json).unwrap();
        assert_eq!(set, back);
        assert_eq!(back.ops().len(), 1);
    }

    #[test]
    fn new_triple_stamps_op_id_from_node_and_clock() {
        let mut s = VectorClock::new();
        s.increment(&NodeId::new("a")); // pre-existing counter 1
        let op = Op::new_triple(s, agent("a", 9), "f.rs", vec![triple(1, 1)], vec![], "why");
        // increment bumps "a" to 2, and op_id reflects that counter.
        assert_eq!(op.op_id, OpId::new(NodeId::new("a"), 2));
        assert_eq!(op.clock.get(&NodeId::new("a")), 2);
    }
}
