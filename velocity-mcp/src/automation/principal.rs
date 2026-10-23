//! Spine-1 — a single causal identity shared by the collaboration + governance
//! systems (see `docs/MULTINODE_COLLAB_GOVERNANCE_SPEC.md` §1–§2).
//!
//! Today Velocity carries three disconnected notions of "who did this":
//! `agent_id = "task-{id}"` in [`crate::orchestrator::worker::scope`], a hard-coded
//! `"Agent_Thread"` in the GUI presence handler, and the single
//! [`crate::editor::company_org::HUMAN_OPERATOR`] sentinel in the org layer. None
//! of them survive a cross-node hop, and none can answer *"which human, on which
//! machine, under whose authority?"*.
//!
//! [`Principal`] unifies them into one causal identity; [`VectorClock`] gives that
//! identity a distributed happens-before relation; [`OpId`] names an operation
//! uniquely by `(node, counter)`. This module is intentionally dependency-light
//! (std + serde only) so it can be the foundation the op-log and the site-map
//! materializer are built on, and so its ordering laws are unit-testable in
//! isolation before any consumer is wired in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ─── Node identity ──────────────────────────────────────────────────────────

/// A stable identifier for one machine/instance in the cluster.
///
/// Distinct from the workflow-canvas `NodeId` (a graph vertex): this names a
/// *physical participant* and is the key a [`VectorClock`] entry is stamped with,
/// so two nodes must never share one. Sourced from the peer/instance registry
/// (`agent/instances.rs`) when a principal is constructed on a remote node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Placeholder for a principal built before the node registry is available
    /// (single-process tests, legacy reload). Never used for clock stamping in a
    /// real multi-node deployment.
    pub fn local() -> Self {
        Self("local".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ─── Principal ──────────────────────────────────────────────────────────────

/// What class of actor produced an action. See spec §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// An autonomous agent / sub-agent run by the executor.
    Agent,
    /// A human — via the GUI editor or a manual filesystem edit.
    Human,
    /// A non-interactive service or a legacy record with no attributable human.
    Service,
}

impl PrincipalKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Human => "human",
            Self::Service => "service",
        }
    }
}

/// A single causal identity carried by every op, lock, and audit record.
///
/// The optional fields are populated per [`PrincipalKind`]:
/// - `Agent` → `task_id` (executor) **and** `role_id` (org), so the two subsystems
///   finally share a join key (spec §1 requirement 3).
/// - `Human` → `user_id` (real identity, ends the `HUMAN_OPERATOR` collapse) and a
///   `role_id` mapping that human to an org role.
/// - `Service`/legacy → neither; carries only `node_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub kind: PrincipalKind,
    /// Which machine this principal acted on — required for cross-node clocks.
    pub node_id: NodeId,
    /// Team the agent belongs to (Agent).
    #[serde(default)]
    pub team_id: Option<String>,
    /// Org role key into `company_org` roles (Agent *or* Human).
    #[serde(default)]
    pub role_id: Option<String>,
    /// Executor task id (Agent); replaces the bare `"task-{id}"` string.
    #[serde(default)]
    pub task_id: Option<u64>,
    /// Real human identity (Human); ends the `HUMAN_OPERATOR` single-sentinel model.
    #[serde(default)]
    pub user_id: Option<String>,
    /// Human-readable label for display / audit rendering.
    #[serde(default)]
    pub display: String,
}

impl Principal {
    /// An agent principal that carries both the executor `task_id` and the org
    /// `role_id`, giving mediator/scope and company_org a shared key.
    pub fn agent(
        node_id: NodeId,
        task_id: u64,
        role_id: Option<String>,
        team_id: Option<String>,
    ) -> Self {
        Self {
            kind: PrincipalKind::Agent,
            node_id,
            team_id,
            role_id,
            task_id: Some(task_id),
            user_id: None,
            display: format!("task-{task_id}"),
        }
    }

    /// A human principal identified by a real `user_id` mapped to an org role.
    pub fn human(node_id: NodeId, user_id: impl Into<String>, role_id: Option<String>) -> Self {
        let user_id = user_id.into();
        let display = user_id.clone();
        Self {
            kind: PrincipalKind::Human,
            node_id,
            team_id: None,
            role_id,
            task_id: None,
            user_id: Some(user_id),
            display,
        }
    }

    /// A service/legacy principal with no attributable human.
    pub fn service(node_id: NodeId) -> Self {
        Self {
            kind: PrincipalKind::Service,
            node_id,
            team_id: None,
            role_id: None,
            task_id: None,
            user_id: None,
            display: "service".to_string(),
        }
    }

    pub fn is_agent(&self) -> bool {
        self.kind == PrincipalKind::Agent
    }

    pub fn is_human(&self) -> bool {
        self.kind == PrincipalKind::Human
    }
}

/// Legacy/pre-refactor default: an unattributable `Service` principal on the
/// local node. Used by `#[serde(default)]` so old log lines load without a
/// `principal` field (spec §2 back-compat, INV-4).
impl Default for Principal {
    fn default() -> Self {
        Self {
            kind: PrincipalKind::Service,
            node_id: NodeId::local(),
            team_id: None,
            role_id: None,
            task_id: None,
            user_id: None,
            display: "service".to_string(),
        }
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}@{}", self.kind.label(), self.display, self.node_id)
    }
}

// ─── Operation identity ─────────────────────────────────────────────────────

/// A globally-unique operation identifier: `(node, per-node monotonic counter)`.
///
/// Content-derived and cheap to compare; used as the deterministic tie-break in
/// [`VectorClock::compare`] and in the site-map materializer so that any node
/// replaying the same op set reaches the same state (spec INV-2/INV-3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OpId {
    pub node: NodeId,
    pub counter: u64,
}

impl OpId {
    pub fn new(node: NodeId, counter: u64) -> Self {
        Self { node, counter }
    }
}

impl fmt::Display for OpId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.node, self.counter)
    }
}

// ─── Vector clock ───────────────────────────────────────────────────────────

/// The result of comparing two causal histories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalOrder {
    /// `self` strictly happens-before `other` (other sees everything self did).
    Before,
    /// `other` strictly happens-before `self`.
    After,
    /// Neither dominates: the two are concurrent (a genuine conflict to resolve).
    Concurrent,
    /// Identical causal history.
    Equal,
}

/// A version vector `node_id -> counter`, the backbone of cross-node convergence.
///
/// Backed by a [`BTreeMap`] so iteration (and therefore serialized bytes) is
/// deterministic — a hard requirement for the site-map `root()` to stay a pure
/// function of content under replay (spec §2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    entries: BTreeMap<NodeId, u64>,
}

impl VectorClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bump and return this node's counter — call it when appending a new op.
    pub fn increment(&mut self, node: &NodeId) -> u64 {
        let counter = self.entries.entry(node.clone()).or_insert(0);
        *counter += 1;
        *counter
    }

    /// Current counter for a node (0 if unseen).
    pub fn get(&self, node: &NodeId) -> u64 {
        self.entries.get(node).copied().unwrap_or(0)
    }

    /// Raise `node`'s counter to `counter` if — and only if — it is higher.
    /// Same monotonic join semantics as [`Self::merge`], for a single component:
    /// the clock-shipping seam needs to absorb an `OpId` counter that a merged
    /// clock may not carry (a peer that lists ids past its own summary).
    pub fn set_max(&mut self, node: &NodeId, counter: u64) {
        let entry = self.entries.entry(node.clone()).or_insert(0);
        if counter > *entry {
            *entry = counter;
        }
    }

    /// Total number of ops this history has observed across all nodes.
    pub fn total(&self) -> u64 {
        self.entries.values().sum()
    }

    /// Merge `other` into `self`, keeping the component-wise maximum. This is the
    /// join (`⊔`) of the two histories: idempotent, commutative, and associative —
    /// the property that makes gossip safe (INV-3).
    pub fn merge(&mut self, other: &VectorClock) {
        for (node, counter) in &other.entries {
            let entry = self.entries.entry(node.clone()).or_insert(0);
            if *counter > *entry {
                *entry = *counter;
            }
        }
    }

    /// Establish the happens-before relation between two histories.
    pub fn compare(&self, other: &VectorClock) -> CausalOrder {
        let mut self_ahead = false;
        let mut other_ahead = false;

        // Every node mentioned by either clock must be visited for a correct
        // domination test; missing entries are treated as 0.
        self.entries
            .keys()
            .chain(other.entries.keys())
            .for_each(|node| {
                let a = self.get(node);
                let b = other.get(node);
                if a > b {
                    self_ahead = true;
                } else if b > a {
                    other_ahead = true;
                }
            });

        match (self_ahead, other_ahead) {
            (false, false) => CausalOrder::Equal,
            (true, false) => CausalOrder::After,
            (false, true) => CausalOrder::Before,
            (true, true) => CausalOrder::Concurrent,
        }
    }

    /// True iff `self` happened strictly before `other` — i.e. applying `other`
    /// subsumes everything `self` observed. Used to decide whether a remote op is
    /// already reflected locally.
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), CausalOrder::Before)
    }
}

impl fmt::Display for VectorClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(n, c)| format!("{n}={c}"))
            .collect();
        write!(f, "{{{}}}", parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> NodeId {
        NodeId::new(name)
    }

    #[test]
    fn increment_is_monotonic_per_node() {
        let mut clock = VectorClock::new();
        assert_eq!(clock.increment(&node("a")), 1);
        assert_eq!(clock.increment(&node("a")), 2);
        assert_eq!(clock.increment(&node("b")), 1);
        assert_eq!(clock.get(&node("a")), 2);
        assert_eq!(clock.get(&node("b")), 1);
        assert_eq!(clock.get(&node("c")), 0);
        assert_eq!(clock.total(), 3);
    }

    #[test]
    fn merge_is_join_max_idempotent_and_commutative() {
        let mut x = VectorClock::new();
        x.increment(&node("a"));
        x.increment(&node("a")); // a=2

        let mut y = VectorClock::new();
        y.increment(&node("a")); // a=1
        y.increment(&node("b")); // b=1

        // x ⊔ y  ==  {a=2, b=1}
        let mut xy = x.clone();
        xy.merge(&y);
        assert_eq!(xy.get(&node("a")), 2);
        assert_eq!(xy.get(&node("b")), 1);

        // idempotent: merging the join into itself changes nothing (INV-3)
        let mut again = xy.clone();
        again.merge(&xy);
        assert_eq!(again, xy);

        // commutative: y ⊔ x == x ⊔ y
        let mut yx = y.clone();
        yx.merge(&x);
        assert_eq!(yx, xy);
    }

    #[test]
    fn set_max_is_monotonic_per_component() {
        let mut c = VectorClock::new();
        c.increment(&node("a")); // a=1
        c.set_max(&node("a"), 3);
        assert_eq!(c.get(&node("a")), 3);
        // Never rewinds — the join only ever moves forward.
        c.set_max(&node("a"), 2);
        assert_eq!(c.get(&node("a")), 3);
        c.set_max(&node("b"), 5);
        assert_eq!(c.get(&node("b")), 5);
    }

    #[test]
    fn compare_detects_causal_order() {
        let mut a = VectorClock::new();
        a.increment(&node("a")); // {a:1}

        let mut ab = a.clone();
        ab.increment(&node("b")); // {a:1,b:1} — a happens-before ab
        assert_eq!(a.compare(&ab), CausalOrder::Before);
        assert_eq!(ab.compare(&a), CausalOrder::After);
        assert!(a.happens_before(&ab));

        // identical histories are Equal
        assert_eq!(a.compare(&a.clone()), CausalOrder::Equal);
    }

    #[test]
    fn compare_detects_concurrency() {
        let mut c1 = VectorClock::new();
        c1.increment(&node("a")); // {a:1}
        let mut c2 = VectorClock::new();
        c2.increment(&node("b")); // {b:1}
                                  // Neither dominates the other → concurrent, must be conflict-resolved.
        assert_eq!(c1.compare(&c2), CausalOrder::Concurrent);
        assert_eq!(c2.compare(&c1), CausalOrder::Concurrent);
    }

    #[test]
    fn opid_orders_by_node_then_counter() {
        let a1 = OpId::new(node("a"), 1);
        let a2 = OpId::new(node("a"), 2);
        let b0 = OpId::new(node("b"), 0);
        assert!(a1 < a2);
        assert!(a2 < b0); // node name is the primary key for the deterministic tie-break
        assert_eq!(a1.to_string(), "a#1");
    }

    #[test]
    fn principal_agent_carries_both_join_keys() {
        let p = Principal::agent(
            node("n1"),
            42,
            Some("role-cto".to_string()),
            Some("team-ai".to_string()),
        );
        assert!(p.is_agent());
        assert_eq!(p.task_id, Some(42));
        assert_eq!(p.role_id.as_deref(), Some("role-cto"));
        assert_eq!(p.display, "task-42");
    }

    #[test]
    fn principal_human_replaces_sentinel_with_real_id() {
        let p = Principal::human(node("n1"), "alice", Some("role-manager".to_string()));
        assert!(p.is_human());
        assert_eq!(p.user_id.as_deref(), Some("alice"));
        assert_eq!(p.task_id, None);
    }

    #[test]
    fn principal_round_trips_through_json() {
        let p = Principal::agent(node("n1"), 7, None, None);
        let json = serde_json::to_string(&p).unwrap();
        let back: Principal = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
        // kind is serialized snake_case per the spec's back-compat convention.
        assert!(json.contains("\"kind\":\"agent\""));
    }

    #[test]
    fn clock_round_trips_through_json() {
        let mut c = VectorClock::new();
        c.increment(&node("a"));
        c.increment(&node("a"));
        let json = serde_json::to_string(&c).unwrap();
        let back: VectorClock = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
        assert_eq!(back.get(&node("a")), 2);
    }
}
