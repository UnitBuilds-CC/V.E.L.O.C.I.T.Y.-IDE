//! System A Phase 3 — cross-node op replication over the peer transport
//! (spec §4: "`/peer/ops` + vector-clock shuffling are the only new surface").
//!
//! The op-log was designed gossip-safe from the start: [`materialize`] is
//! order- and duplicate-independent (INV-2/INV-3), and every producer re-derives
//! its clock by merging over the whole log before stamping an `op_id`. So
//! replication is *not* a new consistency mechanism — it is a shipping rule:
//! exchange the ops a peer may not have, append whatever arrives, and let the
//! existing fold converge both copies.
//!
//! Two primitives, both keyed only on what the log already contains:
//!
//! * **`seen` (the vector-clock shuffle).** A [`ReplicationView`] is the merged
//!   clock of everything one node has observed, plus the exact `op_id` set for
//!   tie-breaking. [`ReplicationView::needs`] decides whether an op might carry
//!   new information: its clock is not yet dominated (`happens_before` or
//!   `Concurrent`), or its `op_id` is absent from the set. An already-absorbed
//!   op (clock `After` *and* id present) is skipped — that is the bandwidth win
//!   the clock buys; correctness never depends on it, because re-sending an op
//!   the peer already holds is harmless (INV-3).
//! * **Idempotent merge.** [`merge_remote_ops`] appends only ops the local log
//!   does not already carry, and — crucially — the local *producers* pick their
//!   next counter by merging over the log after the append, so a replicated-in
//!   op from node `b` advances what this node will stamp for `b`, and a fresh
//!   local op can never collide with one it has just received.
//!
//! The wire shape ([`OpsExchangeRequest`] / [`OpsExchangeResponse`]) is exactly
//! what `peer_server`'s `("POST", "/peer/ops")` arm and `PeerManager::sync_ops_with_peer`
//! speak; keeping it a pure serde struct here is what lets the cross-node
//! convergence tests run over two tempdir workspaces without sockets.

use super::op_log_store::OpLogStore;
use super::oplog::{Op, OpSet};
use super::principal::{OpId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Everything this node has observed: the join of all known op clocks, plus the
/// exact ids (the clock is a filter, the id set is the tie-breaker).
#[derive(Debug, Clone, Default)]
pub struct ReplicationView {
    /// Component-wise max over every op's clock this node has seen.
    pub observed: VectorClock,
    /// Every `op_id` this node carries.
    pub seen_ids: HashSet<OpId>,
}

impl ReplicationView {
    /// Derive the view of a local log — the state a server answers a sync from.
    pub fn from_log(log: &OpSet) -> Self {
        let mut observed = VectorClock::new();
        let mut seen_ids = HashSet::new();
        for op in log.ops() {
            observed.merge(&op.clock);
            seen_ids.insert(op.op_id.clone());
        }
        Self { observed, seen_ids }
    }

    /// Derive the view a sync request carries: the merged local clock plus the
    /// ids listed on the wire. Ids may run ahead of the clock the sender
    /// summarizes (op counters *are* per-node history), so each claimed id
    /// raises its node's component — never lowers it.
    pub fn from_wire(clock: &VectorClock, ids: &[OpId]) -> Self {
        let mut observed = clock.clone();
        for id in ids {
            observed.set_max(&id.node, id.counter);
        }
        Self {
            observed,
            seen_ids: ids.iter().cloned().collect(),
        }
    }

    /// Would an op carrying `clock`/`op_id` be new information here?
    pub fn needs(&self, clock: &VectorClock, op_id: &OpId) -> bool {
        // An exact id match proves the peer carries this very op.
        if self.seen_ids.contains(op_id) {
            return false;
        }
        match self.observed.compare(clock) {
            // They are ahead of everything we observed: definitely new.
            super::principal::CausalOrder::Before => true,
            super::principal::CausalOrder::Concurrent => true,
            // We dominate (or match) their history: per-node counters are
            // dense — a producer stamps `op_id` from the bumped clock, so a
            // peer claiming node#N provably carries every lower counter too.
            // Otherwise the id set alone decides; and since the early return
            // already handled an exact match, a dominated-but-unlisted id is
            // still shipped — the Legacy-effect denial shape, where one op's
            // clock is absorbed by an earlier sibling of the same node.
            _ => self.observed.get(&op_id.node) < op_id.counter,
        }
    }
}

/// A sync round in one request: "here is everything I have that you may not
/// have (per your last-known clock) — give me yours." Answered by
/// `("POST", "/peer/ops")` in `agent/peer_server.rs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsExchangeRequest {
    /// Who is asking (their node id, for logs/errors only — the ops carry
    /// their own identity).
    #[serde(default)]
    pub from_node: String,
    /// The sender's observed clock — how the receiver filters its answer.
    #[serde(default)]
    pub observed: VectorClock,
    /// The sender's full local log. Append-only, so the receiver can always
    /// dedupe by `op_id`; `needs` filters which of these are worth parsing.
    #[serde(default)]
    pub ops: Vec<Op>,
    /// The ids the sender carries, so the receiver does not re-derive the set
    /// from `ops` (they are the same list in practice; this is a fast path).
    #[serde(default)]
    pub seen_ids: Vec<OpId>,
}

/// The receiver's answer: the ops its own log holds that the sender may lack.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsExchangeResponse {
    /// How many ops the receiver appended from the request.
    pub merged: usize,
    /// How many request ops it skipped as already-known.
    pub skipped: usize,
    /// Ops the sender may not have, filtered by the sender's `observed` clock.
    pub ops: Vec<Op>,
    /// The receiver's observed clock after the merge — the sender uses it to
    /// bump its own view without re-folding every op.
    pub observed: VectorClock,
}

/// Append the ops `request` carries that `store` does not already hold.
/// Returns `(appended, skipped)`. Duplicate-id ops are skipped rather than
/// written twice — the log *tolerates* duplicates (INV-3) but a growing file of
/// resends is waste, not history.
pub fn merge_remote_ops(store: &OpLogStore, request: &OpsExchangeRequest) -> (usize, usize) {
    let local = store.load();
    let mut view = ReplicationView::from_log(&local);
    let mut to_append: Vec<Op> = Vec::new();
    let mut skipped = 0usize;
    for op in &request.ops {
        if view.seen_ids.contains(&op.op_id) {
            skipped += 1;
            continue;
        }
        view.seen_ids.insert(op.op_id.clone());
        to_append.push(op.clone());
    }
    let appended = to_append.len();
    if let Err(e) = store.append_many(&to_append) {
        log::warn!("replication: failed to append {appended} remote ops: {e}");
        return (0, skipped);
    }
    (appended, skipped)
}

/// Filter `log` down to the ops the *sender's* wire view might not have —
/// the answer half of a sync round.
pub fn ops_for_peer(log: &OpSet, peer: &ReplicationView) -> Vec<Op> {
    log.ops()
        .iter()
        .filter(|op| peer.needs(&op.clock, &op.op_id))
        .cloned()
        .collect()
}

/// The full server-side half of a sync: merge what the peer sent, then answer
/// with what this node holds that the peer may lack. Pure over the workspace's
/// durable log, so `peer_server` stays transport and this stays testable.
pub fn handle_exchange(
    workspace: &std::path::Path,
    request: &OpsExchangeRequest,
) -> OpsExchangeResponse {
    let store = OpLogStore::open(workspace);
    let (merged, skipped) = merge_remote_ops(&store, request);
    let local = store.load();
    let view = ReplicationView::from_log(&local);
    let peer_view = ReplicationView::from_wire(&request.observed, &request.seen_ids);
    OpsExchangeResponse {
        merged,
        skipped,
        ops: ops_for_peer(&local, &peer_view),
        observed: view.observed,
    }
}

/// Test-only op minting, shared with the peer-transport tests (the producer
/// clock discipline lives here, so the helpers prove it rather than fake it).
#[cfg(test)]
pub(crate) mod test_util {
    use super::super::oplog::{OpEffect, TripleEffect};
    use super::*;
    use crate::automation::oplog::EventOutcome;
    use crate::automation::principal::{NodeId, Principal};
    use velocity_ide::site_map::VcTriple;

    /// A triple op stamped the way a real producer stamps one: the clock is
    /// the merge over `store`'s log with `node` bumped, and the `op_id` takes
    /// that bumped counter — so ops minted through here collide with nothing,
    /// anywhere (see the collision test below).
    pub fn stamped_triple_op(
        store: &OpLogStore,
        node: &str,
        file: &str,
        subj: u64,
        obj: u64,
    ) -> Op {
        let mut clock = VectorClock::new();
        for op in store.load().ops() {
            clock.merge(&op.clock);
        }
        let counter = clock.increment(&NodeId::new(node));
        Op {
            op_id: OpId::new(NodeId::new(node), counter),
            clock,
            principal: Principal::agent(NodeId::new(node), 1, None, None),
            causal_deps: Vec::new(),
            intent: format!("add {subj}->{obj}"),
            effect: OpEffect::Triple(TripleEffect {
                file_path: file.into(),
                add: vec![VcTriple {
                    subject_hash: subj,
                    predicate_id: 2,
                    object_hash: obj,
                }],
                rm: vec![],
            }),
            outcome: EventOutcome::Pending,
            authorized_by: None,
        }
    }

    /// A standalone triple op with a minimal one-bump clock — for tests that
    /// only need a well-formed convergent op, not a collision-free sequence.
    pub fn triple_op(node: &str, file: &str, subj: u64, obj: u64) -> Op {
        let mut clock = VectorClock::new();
        let counter = clock.increment(&NodeId::new(node));
        Op {
            op_id: OpId::new(NodeId::new(node), counter),
            clock,
            principal: Principal::agent(NodeId::new(node), 1, None, None),
            causal_deps: Vec::new(),
            intent: format!("add {subj}->{obj}"),
            effect: OpEffect::Triple(TripleEffect {
                file_path: file.into(),
                add: vec![VcTriple {
                    subject_hash: subj,
                    predicate_id: 2,
                    object_hash: obj,
                }],
                rm: vec![],
            }),
            outcome: EventOutcome::Pending,
            authorized_by: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::oplog::OpEffect;
    use crate::automation::principal::{NodeId, Principal, PrincipalKind};

    fn agent(node: &str) -> Principal {
        Principal::agent(NodeId::new(node), 1, None, None)
    }

    /// Mint the next op for `node` against `store` exactly the way a producer
    /// does: clock merged over the whole log, counter bumped, id stamped.
    fn stamp_op(store: &OpLogStore, node: &str, subj: u64, obj: u64) -> Op {
        test_util::stamped_triple_op(store, node, "f.rs", subj, obj)
    }

    /// A denial-shaped op: Text-free Legacy effect, so `needs` must fall back
    /// to the id set when the clocks are Equal.
    fn legacy_op(node: &str, counter: u64) -> Op {
        let mut clock = VectorClock::new();
        clock.increment(&NodeId::new(node));
        Op {
            op_id: OpId::new(NodeId::new(node), counter),
            clock,
            principal: agent(node),
            causal_deps: Vec::new(),
            intent: "rbac-deny: dispatch_to on team/x — nope".into(),
            effect: OpEffect::Legacy,
            outcome: crate::automation::oplog::EventOutcome::Failure,
            authorized_by: None,
        }
    }

    #[test]
    fn two_nodes_converge_to_the_same_materialized_state() {
        // The INV-2 claim at node scale: bidirectional exchange, then both
        // folds agree — regardless of which side's ops landed first anywhere.
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let a = OpLogStore::open(a_dir.path());
        let b = OpLogStore::open(b_dir.path());

        let a1 = stamp_op(&a, "node-a", 1, 2);
        a.append(&a1).unwrap();
        let a2 = stamp_op(&a, "node-a", 2, 3);
        a.append(&a2).unwrap();
        let b1 = stamp_op(&b, "node-b", 5, 6);
        b.append(&b1).unwrap();

        // A asks B; then B asks A (using A's answer to build its request).
        let req_a = OpsExchangeRequest {
            from_node: "node-a".into(),
            observed: ReplicationView::from_log(&a.load()).observed,
            ops: a.load().ops().to_vec(),
            seen_ids: a.load().ops().iter().map(|o| o.op_id.clone()).collect(),
        };
        let resp_b = handle_exchange(b_dir.path(), &req_a);
        assert_eq!(resp_b.merged, 2, "B appends both of A's ops");
        assert_eq!(resp_b.skipped, 0);
        assert_eq!(resp_b.ops.len(), 1, "A has not seen B's own op");

        let req_b = OpsExchangeRequest {
            from_node: "node-b".into(),
            observed: ReplicationView::from_log(&b.load()).observed,
            ops: b.load().ops().to_vec(),
            seen_ids: b.load().ops().iter().map(|o| o.op_id.clone()).collect(),
        };
        let resp_a = handle_exchange(a_dir.path(), &req_b);
        assert_eq!(resp_a.merged, 1);
        // A already learned of B's op from the response — filtered out.
        assert!(resp_a.ops.is_empty(), "{:?}", resp_a.ops.len());

        let state_a = a.materialize();
        let state_b = b.materialize();
        assert_eq!(state_a, state_b, "cross-node convergence (INV-2)");
        assert_eq!(state_a.live_triples.len(), 3);
    }

    #[test]
    fn re_sending_the_whole_log_changes_nothing() {
        // INV-3 at the transport seam: a full resend is skipped, not appended;
        // the file does not grow; the fold is untouched.
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let a = OpLogStore::open(a_dir.path());
        let b = OpLogStore::open(b_dir.path());
        let op = stamp_op(&a, "node-a", 1, 2);
        a.append(&op).unwrap();

        let req = OpsExchangeRequest {
            from_node: "node-a".into(),
            observed: ReplicationView::from_log(&a.load()).observed,
            ops: vec![op.clone()],
            seen_ids: vec![op.op_id.clone()],
        };
        assert_eq!(handle_exchange(b_dir.path(), &req).merged, 1);
        let raw_after_first = std::fs::read_to_string(b.ops_path()).unwrap();

        let second = handle_exchange(b_dir.path(), &req);
        assert_eq!(second.merged, 0);
        assert_eq!(second.skipped, 1);
        let raw_after_second = std::fs::read_to_string(b.ops_path()).unwrap();
        assert_eq!(
            raw_after_first, raw_after_second,
            "a resend must not grow the durable log"
        );
    }

    #[test]
    fn replicated_in_ops_prevent_local_op_id_collisions() {
        // The counter-after-replication invariant: B stamps node-a ops using
        // A's higher counters, so the fresh op neither duplicates one already
        // in B's log nor one A will stamp next on A itself.
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let a = OpLogStore::open(a_dir.path());
        let b = OpLogStore::open(b_dir.path());
        for i in 0..3u64 {
            let op = stamp_op(&a, "node-a", 10 + i, 20 + i);
            a.append(&op).unwrap();
        }
        let req = OpsExchangeRequest {
            from_node: "node-a".into(),
            observed: ReplicationView::from_log(&a.load()).observed,
            ops: a.load().ops().to_vec(),
            seen_ids: Vec::new(),
        };
        handle_exchange(b_dir.path(), &req);

        // B minting a node-a op must continue A's sequence…
        let b_next = stamp_op(&b, "node-a", 99, 100);
        assert_eq!(b_next.op_id.counter, 4);
        // …and so must A itself.
        let a_next = stamp_op(&a, "node-a", 101, 102);
        assert_eq!(a_next.op_id.counter, 4);
        assert_eq!(b_next.op_id, a_next.op_id);
        // Both sides would stamp 4 next — so the *owner* (A) mints the real
        // op#4 and ships it; B dedupes by id and its fold gains A's content,
        // never a rival op with the same identity.
        a.append(&a_next).unwrap();
        let req2 = OpsExchangeRequest {
            from_node: "node-a".into(),
            observed: ReplicationView::from_log(&a.load()).observed,
            ops: a.load().ops().to_vec(),
            seen_ids: Vec::new(),
        };
        let resp = handle_exchange(b_dir.path(), &req2);
        assert_eq!(resp.skipped, 3, "B already holds A's ops 1–3");
        assert_eq!(resp.merged, 1, "op#4 is A's to mint and B's to absorb");
        assert_eq!(a.materialize(), b.materialize());
        assert_eq!(a.materialize().live_triples.len(), 4);
    }

    #[test]
    fn equal_clocks_fall_back_to_the_id_set() {
        // Legacy-effect denial shape: two ops of the same node can carry the
        // same clock; only the id set separates "already have" from "send it".
        let node = "node-a";
        let l1 = legacy_op(node, 1);
        let l2 = legacy_op(node, 2);
        let mut view = ReplicationView::default();
        assert!(view.needs(&l1.clock, &l1.op_id));
        view.observed.merge(&l1.clock);
        view.seen_ids.insert(l1.op_id.clone());
        assert!(
            !view.needs(&l1.clock, &l1.op_id),
            "same id, same clock: skip"
        );
        assert!(
            view.needs(&l2.clock, &l2.op_id),
            "equal clock, new id: still send"
        );
    }

    #[test]
    fn from_wire_absorbs_ids_beyond_the_merged_clock() {
        // A request whose seen_ids claim a higher counter than their node's
        // summarized clock: counters are dense, so the receiver must treat
        // every lower counter of that node as carried, not just the listed id.
        let mut observed = VectorClock::new();
        observed.increment(&NodeId::new("node-a")); // a=1
        let ids = vec![OpId::new(NodeId::new("node-a"), 3)];
        let view = ReplicationView::from_wire(&observed, &ids);
        assert_eq!(view.observed.get(&NodeId::new("node-a")), 3);
        assert!(
            !view.needs(&observed, &OpId::new(NodeId::new("node-a"), 2)),
            "a#3 on the wire proves the peer carries a#2 (dense counters)"
        );
        assert!(!view.needs(&observed, &OpId::new(NodeId::new("node-a"), 3)));
        // …but a counter above what the wire claimed is still new.
        assert!(view.needs(&observed, &OpId::new(NodeId::new("node-a"), 4)));
    }

    #[test]
    fn principal_and_clock_survive_the_wire_round_trip() {
        // The exchange travels as JSON over HTTP in production; serde is the
        // whole protocol, so identity must not degrade through it.
        let mut clock = VectorClock::new();
        clock.increment(&NodeId::new("node-a"));
        let op = stamp_op(
            &OpLogStore::open(tempfile::tempdir().unwrap().path()),
            "node-a",
            1,
            2,
        );
        let req = OpsExchangeRequest {
            from_node: "node-a".into(),
            observed: clock,
            ops: vec![op.clone()],
            seen_ids: vec![op.op_id.clone()],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: OpsExchangeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ops, vec![op]);
        assert_eq!(back.ops[0].principal.kind, PrincipalKind::Agent);
        assert_eq!(back.ops[0].principal.node_id.as_str(), "node-a");
    }
}
