//! Spine-3 — the causal audit chain (see
//! `docs/MULTINODE_COLLAB_GOVERNANCE_SPEC.md` §3).
//!
//! Velocity keeps two audit stores that today do not talk to each other:
//! the codebase decision trail (`registry::event_store`) and the org verdict
//! trail (`editor::company_org`). The spec's rule is **link by id, never
//! physically merge** — merging would break `is_high_signal` / `file_verdict`,
//! which depend on the current shapes.
//!
//! This module is the *link*: given a governance `request_id` (a
//! `company_org::ActionRequest.id`), it reconstructs "who said what → what the
//! system actually did" by walking the op-log from the ops that directly cite
//! that request (`Op::authorized_by`) out to every op that transitively depends
//! on them (`Op::causal_deps`). That is what lets a second manager see *why*
//! the system did ZYX after the first asked for XYZ — the objective's audit
//! requirement — without either store having to absorb the other.
//!
//! Like the rest of the spine this is a pure function over an [`OpSet`]: it
//! needs no live executor wiring, and it returns ops in a deterministic
//! (`OpId`-sorted) order so two nodes rendering the same log produce the same
//! audit.

use super::oplog::{Op, OpSet};
use super::principal::OpId;
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Every op a governance request authorized, transitively.
///
/// Seeds are the ops whose `authorized_by` equals `request_id`; from those we
/// follow the *reverse* of `causal_deps` (an op that depends on a caused op is
/// itself caused) to a fixed point. The result is de-duplicated and sorted by
/// `OpId`, so it is stable across permutations of the log (INV-7). Cycle-safe:
/// a malformed `causal_deps` loop cannot cause an infinite walk.
pub fn caused_by(set: &OpSet, request_id: &str) -> Vec<OpId> {
    let ops = set.ops();

    // dep -> [ops that depend on it]; the reverse edges of the causal DAG.
    let mut dependents: HashMap<OpId, Vec<OpId>> = HashMap::new();
    for op in ops {
        for dep in &op.causal_deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(op.op_id.clone());
        }
    }

    // Seed from the ops the request directly authorized.
    let mut visited: BTreeSet<OpId> = BTreeSet::new();
    let mut queue: VecDeque<OpId> = VecDeque::new();
    for op in ops {
        if op.authorized_by.as_deref() == Some(request_id) && visited.insert(op.op_id.clone()) {
            queue.push_back(op.op_id.clone());
        }
    }

    // Transitive closure over the reverse edges.
    while let Some(cur) = queue.pop_front() {
        if let Some(children) = dependents.get(&cur) {
            for child in children {
                if visited.insert(child.clone()) {
                    queue.push_back(child.clone());
                }
            }
        }
    }

    visited.into_iter().collect()
}

/// The ops [`caused_by`] a request authorized, resolved to their bodies, in the
/// same deterministic order — the "what was done" half of an engagement record.
///
/// The "who said what" half lives in `company_org` and is joined by the caller
/// via the shared `request_id` (link, do not merge): a caller looks up the
/// `ActionRequest` once and pairs it with this op list to render the trail.
pub fn engagement<'a>(set: &'a OpSet, request_id: &str) -> Vec<&'a Op> {
    let index: HashMap<OpId, &Op> = set.ops().iter().map(|op| (op.op_id.clone(), op)).collect();
    caused_by(set, request_id)
        .iter()
        .filter_map(|id| index.get(id).copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::oplog::OpEffect;
    use crate::automation::principal::NodeId;
    use crate::automation::principal::{Principal, VectorClock};
    use crate::registry::event_store::EventOutcome;

    /// Build a bare op carrying only the identity/causation/authorization the
    /// audit walk reads; effect/outcome are inert placeholders.
    fn op(node: &str, counter: u64, deps: &[(&str, u64)], authorized_by: Option<&str>) -> Op {
        Op {
            op_id: OpId::new(NodeId::new(node), counter),
            clock: VectorClock::new(),
            principal: Principal::agent(NodeId::new(node), counter, None, None),
            causal_deps: deps
                .iter()
                .map(|(n, c)| OpId::new(NodeId::new(*n), *c))
                .collect(),
            intent: format!("intent-{}#{}", node, counter),
            effect: OpEffect::Legacy,
            outcome: EventOutcome::Success,
            authorized_by: authorized_by.map(|s| s.to_string()),
        }
    }

    fn set_of(ops: Vec<Op>) -> OpSet {
        let mut s = OpSet::new();
        for o in ops {
            s.push(o);
        }
        s
    }

    #[test]
    fn walks_the_transitive_causal_closure() {
        // R authorizes a1; b1 depends on a1; c1 depends on b1. x1 is unrelated.
        let set = set_of(vec![
            op("a", 1, &[], Some("R")),
            op("b", 1, &[("a", 1)], None),
            op("c", 1, &[("b", 1)], None),
            op("x", 1, &[], None),
        ]);

        let caused = caused_by(&set, "R");
        assert_eq!(
            caused,
            vec![
                OpId::new(NodeId::new("a"), 1),
                OpId::new(NodeId::new("b"), 1),
                OpId::new(NodeId::new("c"), 1),
            ]
        );
        // The unrelated op is not pulled in (INV-7 precision).
        assert!(!caused.contains(&OpId::new(NodeId::new("x"), 1)));
    }

    #[test]
    fn seed_must_be_directly_authorized_not_merely_referenced() {
        // Nothing cites R, so even a connected component yields an empty result.
        let set = set_of(vec![op("a", 1, &[], None), op("b", 1, &[("a", 1)], None)]);
        assert_eq!(caused_by(&set, "R"), vec![]);
    }

    #[test]
    fn only_ops_for_the_named_request_seed() {
        let set = set_of(vec![
            op("a", 1, &[], Some("R1")),
            op("b", 1, &[], Some("R2")),
        ]);
        assert_eq!(caused_by(&set, "R1"), vec![OpId::new(NodeId::new("a"), 1)]);
        assert_eq!(caused_by(&set, "R2"), vec![OpId::new(NodeId::new("b"), 1)]);
    }

    #[test]
    fn is_cycle_safe() {
        // A malformed cycle (a<->b) must terminate and still include both seeds'
        // closure.
        let set = set_of(vec![
            op("a", 1, &[("b", 1)], Some("R")),
            op("b", 1, &[("a", 1)], None),
        ]);
        let caused = caused_by(&set, "R");
        assert_eq!(caused.len(), 2);
        assert!(caused.contains(&OpId::new(NodeId::new("a"), 1)));
        assert!(caused.contains(&OpId::new(NodeId::new("b"), 1)));
    }

    #[test]
    fn result_is_deterministic_across_permutations() {
        let ordered = set_of(vec![
            op("a", 1, &[], Some("R")),
            op("b", 1, &[("a", 1)], None),
            op("c", 1, &[("b", 1)], None),
        ]);
        // Same three ops, inserted in a different arrival order.
        let shuffled = set_of(vec![
            op("c", 1, &[("b", 1)], None),
            op("a", 1, &[], Some("R")),
            op("b", 1, &[("a", 1)], None),
        ]);
        // Output is sorted by OpId, so it does not depend on insertion order.
        assert_eq!(caused_by(&ordered, "R"), caused_by(&shuffled, "R"));
    }

    #[test]
    fn engagement_returns_ops_in_causal_order() {
        let set = set_of(vec![
            op("a", 1, &[], Some("R")),
            op("b", 1, &[("a", 1)], None),
        ]);
        let trail = engagement(&set, "R");
        assert_eq!(trail.len(), 2);
        assert_eq!(trail[0].op_id, OpId::new(NodeId::new("a"), 1));
        assert_eq!(trail[1].op_id, OpId::new(NodeId::new("b"), 1));
        // Bodies are the real ops, so the renderer can read principal + intent.
        assert_eq!(trail[0].intent, "intent-a#1");
        assert_eq!(trail[0].authorized_by.as_deref(), Some("R"));
    }
}
