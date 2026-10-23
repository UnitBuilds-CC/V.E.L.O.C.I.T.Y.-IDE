//! System A — text (file-byte) convergence behind the `MergePolicy` seam (see
//! `docs/MULTINODE_COLLAB_GOVERNANCE_SPEC.md` §4, "Text convergence tiers").
//!
//! The site-map *edges* converge via [`crate::automation::oplog::materialize`] and
//! partition via [`crate::automation::partition`]; the remaining hard part is the
//! file *bytes*. The spec ships **Tier 1 = per-file last-write-wins**, and
//! reserves this module's [`MergePolicy`] trait as the seam where a sequence CRDT
//! (RGA/Fugue-style, open decision **D1**) can later be dropped in *per partition*
//! without touching the op model. So shipping LWW here is the specced default,
//! not a substitute for the undecided upgrade path — D1 governs which `MergePolicy`
//! impl a partition selects, and this trait is exactly that choice point.
//!
//! Determinism note: LWW ranks each op by `(clock magnitude, OpId)`. That pair is
//! a *total* order, so the fold is associative/commutative/idempotent — the same
//! convergence guarantees the triple fold relies on (INV-2/INV-3). Ranking by the
//! clock first honours the spec's "resolved by `clock` then `op_id`"; `OpId` is
//! the deterministic tie-break for genuinely concurrent writes.

use super::oplog::{Op, OpEffect, OpSet};
use super::principal::OpId;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The surviving head of a file after a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHead {
    pub op_id: OpId,
    pub digest: String,
}

/// Strategy for collapsing the text ops on one path to a single head.
///
/// Tier 1 is [`LastWriteWins`]; a sequence CRDT for D1 would implement this same
/// trait (merging digests/patches instead of picking one) and be selected per
/// partition, leaving every caller of [`materialize_text`] unchanged.
pub trait MergePolicy {
    fn resolve_text(&self, candidates: &[&Op]) -> Option<TextHead>;
}

/// Total rank: greater clock magnitude wins, tie broken by greater `OpId`.
fn rank(op: &Op) -> (u64, &OpId) {
    (op.clock.total(), &op.op_id)
}

/// Tier-1 convergence: the highest-ranked op owns the file's content.
pub struct LastWriteWins;

impl MergePolicy for LastWriteWins {
    fn resolve_text(&self, candidates: &[&Op]) -> Option<TextHead> {
        let mut best: Option<&Op> = None;
        for op in candidates {
            if best.is_none_or(|b| rank(op) > rank(b)) {
                best = Some(op);
            }
        }
        match best {
            Some(op) => match &op.effect {
                OpEffect::Text { digest, .. } => Some(TextHead {
                    op_id: op.op_id.clone(),
                    digest: digest.clone(),
                }),
                _ => None,
            },
            None => None,
        }
    }
}

/// Materialize per-path text heads for the whole log under `policy`.
///
/// Only [`OpEffect::Text`] ops contribute; every other effect is ignored here
/// (edges are handled by `materialize`). The result is order-independent because
/// the fold keeps the max of a total rank (INV-2) and is idempotent under
/// duplicated ops (INV-3).
pub fn materialize_text(set: &OpSet, policy: &dyn MergePolicy) -> BTreeMap<PathBuf, TextHead> {
    let mut by_path: BTreeMap<PathBuf, Vec<&Op>> = BTreeMap::new();
    for op in set.ops() {
        if let OpEffect::Text { path, .. } = &op.effect {
            by_path.entry(path.clone()).or_default().push(op);
        }
    }

    let mut heads = BTreeMap::new();
    for (path, candidates) in by_path {
        if let Some(head) = policy.resolve_text(&candidates) {
            heads.insert(path, head);
        }
    }
    heads
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::principal::NodeId;
    use crate::automation::principal::{Principal, VectorClock};
    use crate::registry::event_store::EventOutcome;

    /// Build a text op with an explicit clock + op_id so rank is controlled.
    fn text_op(
        node: &str,
        counter: u64,
        clock_totals: &[(&str, u64)],
        path: &str,
        digest: &str,
    ) -> Op {
        let mut clock = VectorClock::new();
        for (n, c) in clock_totals {
            for _ in 0..*c {
                clock.increment(&NodeId::new(*n));
            }
        }
        Op {
            op_id: OpId::new(NodeId::new(node), counter),
            clock,
            principal: Principal::service(NodeId::new(node)),
            causal_deps: Vec::new(),
            intent: String::new(),
            effect: OpEffect::Text {
                path: PathBuf::from(path),
                digest: digest.to_string(),
            },
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

    fn path(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn higher_clock_wins_even_against_op_id() {
        // A has a bigger causal history (total 3) but a smaller OpId than B.
        let a = text_op("a", 1, &[("a", 3)], "f.rs", "A");
        let b = text_op("z", 9, &[("z", 1)], "f.rs", "B");
        let heads = materialize_text(&set_of(vec![a, b]), &LastWriteWins);
        // "clock then op_id" ⇒ the greater clock magnitude (A) owns the file.
        assert_eq!(heads[&path("f.rs")].digest, "A");
    }

    #[test]
    fn concurrent_writes_tiebreak_on_op_id() {
        // Same clock magnitude, different nodes ⇒ resolved by greater OpId.
        let a = text_op("a", 1, &[("a", 1)], "f.rs", "A");
        let b = text_op("b", 1, &[("b", 1)], "f.rs", "B");
        let heads = materialize_text(&set_of(vec![a, b]), &LastWriteWins);
        // Node "b" > "a" lexicographically at equal counter.
        assert_eq!(heads[&path("f.rs")].digest, "B");
    }

    #[test]
    fn text_materialize_is_permutation_invariant() {
        let ops = vec![
            text_op("a", 1, &[("a", 1)], "f.rs", "A"),
            text_op("b", 2, &[("b", 5)], "f.rs", "B"),
            text_op("c", 3, &[("c", 2)], "g.rs", "C"),
        ];
        let forward = materialize_text(&set_of(ops.clone()), &LastWriteWins);
        let mut reversed = ops;
        reversed.reverse();
        let backward = materialize_text(&set_of(reversed), &LastWriteWins);
        assert_eq!(forward, backward); // INV-2
    }

    #[test]
    fn text_materialize_is_idempotent() {
        let op = text_op("a", 1, &[("a", 1)], "f.rs", "A");
        let mut once = OpSet::new();
        once.push(op.clone());
        let mut twice = OpSet::new();
        twice.push(op.clone());
        twice.push(op); // duplicated via gossip
                        // INV-3: replaying the same op changes nothing.
        assert_eq!(
            materialize_text(&once, &LastWriteWins),
            materialize_text(&twice, &LastWriteWins)
        );
    }

    #[test]
    fn non_text_effects_do_not_produce_heads() {
        // A Legacy/textless op must not appear in the text view.
        let mut legacy = text_op("a", 1, &[], "f.rs", "A");
        legacy.effect = OpEffect::Legacy;
        assert!(materialize_text(&set_of(vec![legacy]), &LastWriteWins).is_empty());
    }

    /// Proves the `MergePolicy` seam is real: swapping the strategy changes the
    /// resolution but not any caller — the D1 upgrade path plugs in here.
    struct FirstWriteWins;
    impl MergePolicy for FirstWriteWins {
        fn resolve_text(&self, candidates: &[&Op]) -> Option<TextHead> {
            // Inverse rank: lowest (clock, op_id) wins.
            let mut best: Option<&Op> = None;
            for op in candidates {
                if best.is_none_or(|b| rank(op) < rank(b)) {
                    best = Some(op);
                }
            }
            match best {
                Some(op) => match &op.effect {
                    OpEffect::Text { digest, .. } => Some(TextHead {
                        op_id: op.op_id.clone(),
                        digest: digest.clone(),
                    }),
                    _ => None,
                },
                None => None,
            }
        }
    }

    #[test]
    fn merge_policy_is_swappable() {
        let ops = vec![
            text_op("a", 1, &[("a", 3)], "f.rs", "A"),
            text_op("b", 1, &[("b", 1)], "f.rs", "B"),
        ];
        let set = set_of(ops);
        assert_eq!(
            materialize_text(&set, &LastWriteWins)[&path("f.rs")].digest,
            "A"
        );
        // A different policy (standing in for a future sequence CRDT) yields a
        // different head from the identical op log, with no caller changes.
        assert_eq!(
            materialize_text(&set, &FirstWriteWins)[&path("f.rs")].digest,
            "B"
        );
    }
}
