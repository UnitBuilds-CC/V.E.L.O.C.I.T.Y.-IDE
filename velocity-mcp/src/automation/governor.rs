//! System C (governor) — org RBAC keyed by [`Principal`], with denials in the
//! durable trail (spec §6).
//!
//! `company_org::check_permission` was already a deterministic, deny-by-default
//! pure function, but it ran only behind the credential-vault paths and the
//! `check_org_permission` *tool* — an agent could simply not call the tool.
//! Spec §6 turns it into a hard gate at the executor chokepoints, and this
//! module is the single seam that makes that possible without every caller
//! having to know how to load an org document, map a principal to a role, and
//! append an audit op:
//!
//! * **Identity join.** [`Governor::principal_for_team`] resolves the org role
//!   that staffs a team through the existing `Role::agent_team` bridge, so the
//!   dispatch gate asks "may *this role* spend on *this team*" — the same
//!   `role_id` the Governance GUI edits, now consulted on the live path.
//! * **Deny by default under governance (INV-6).** A workspace with no org
//!   document is *ungoverned* and the gate is inert — that is what keeps the
//!   spec's unattended-safety rule ("must not silently block an unattended
//!   mission on a missing approver") honest: governance is opt-in per
//!   workspace, never an accident of a missing file. Once an org *does* exist,
//!   a principal carrying no `role_id` cannot act, and every denial is
//!   appended to the durable op-log as its own audit op — the trail shows what
//!   was refused, by whom, and why, not only what was done.
//! * **One append path.** The denial op rides the same [`OpLogStore`] as
//!   attribution (System B) with an inert [`OpEffect::Legacy`] body, so it
//!   advances the shared node clock without polluting the `(path, digest)`
//!   dedupe set — a deny is not a content change.
//!
//! Resource convention (documented so grants can be authored against it): an
//! agent-dispatch target is the resource `team/<slugified-team-id>`, and
//! in-repo writes are the resource [`WORKSPACE_RESOURCE`].

use super::op_log_store::OpLogStore;
use super::oplog::{Op, OpEffect};
use super::principal::{NodeId, OpId, Principal, VectorClock};
use crate::editor::company_org::{
    check_permission, load_company_org, org_team_slug, Capability, CompanyOrg, Decision,
};
use crate::editor::expert_team::slugify;
use crate::registry::event_store::EventOutcome;
use std::path::{Path, PathBuf};

/// The canonical resource id for in-repo work (write-gate checks, spec §6
/// chokepoint 4). Grants against it are what admit file edits in a governed
/// workspace.
pub const WORKSPACE_RESOURCE: &str = "workspace";

/// The resource id an agent-dispatch check is evaluated against.
pub fn team_resource(team_id: &str) -> String {
    format!("team/{}", slugify(team_id))
}

/// The workspace's governance gate: an org document if one exists, plus the
/// durable log that denial ops are appended to.
pub struct Governor {
    workspace: PathBuf,
    org: Option<CompanyOrg>,
}

impl Governor {
    /// Load the governance state of `workspace` (`.velocity/company_org.nda`).
    pub fn open(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            org: load_company_org(workspace),
        }
    }

    /// Build a governor over an already-loaded org (embedded callers, tests).
    pub fn with_org(workspace: &Path, org: Option<CompanyOrg>) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            org,
        }
    }

    /// Whether this workspace is governed at all. Ungoverned workspaces pass
    /// every check — governance is opt-in, never an accident (see module doc).
    pub fn governed(&self) -> bool {
        self.org.is_some()
    }

    /// The loaded org document, when governed.
    pub fn org(&self) -> Option<&CompanyOrg> {
        self.org.as_ref()
    }

    /// The agent [`Principal`] that staffs `team_id`: its `role_id` is the org
    /// role whose `agent_team` bridge names the team (the same bridge
    /// `team_dispatch` uses to turn the chart into a workforce). A role with an
    /// empty `agent_team` staffs the org's own team (`org_team_slug`). When no
    /// role is bound the principal carries `role_id: None`, which a governed
    /// workspace denies (INV-6) rather than guessing an identity.
    pub fn principal_for_team(&self, team_id: &str) -> Principal {
        let role_id = self.org.as_ref().and_then(|org| {
            let slug = slugify(team_id);
            let own = org_team_slug(org);
            org.roles
                .iter()
                .find(|r| {
                    let bound = if r.agent_team.is_empty() {
                        own.as_str()
                    } else {
                        r.agent_team.as_str()
                    };
                    bound == slug || bound == team_id
                })
                .map(|r| r.id.clone())
        });
        Principal::agent(NodeId::local(), 0, role_id, Some(team_id.to_string()))
    }

    /// Evaluate `principal` for `capability` on `resource_id` — the Principal-
    /// keyed front to `check_permission`.
    pub fn check(
        &self,
        principal: &Principal,
        resource_id: &str,
        capability: Capability,
        amount: f64,
    ) -> Decision {
        let Some(org) = &self.org else {
            // Ungoverned workspace: the gate is inert by design.
            return Decision::Allowed;
        };
        let Some(role_id) = principal.role_id.as_deref() else {
            return Decision::Denied {
                reason: format!(
                    "principal on node '{}' (kind {}) carries no org role_id; a governed workspace admits only identified roles",
                    principal.node_id,
                    principal.kind.label()
                ),
            };
        };
        check_permission(org, role_id, resource_id, capability, amount)
    }

    /// [`Self::check`] plus the INV-6 audit: every denial is appended to the
    /// durable op-log before it is returned, so "what was refused, by whom,
    /// and why" is retrievable from the same trail as "what was done".
    pub fn check_recorded(
        &self,
        principal: &Principal,
        resource_id: &str,
        capability: Capability,
        amount: f64,
    ) -> Decision {
        let decision = self.check(principal, resource_id, capability, amount);
        if let Decision::Denied { reason } = &decision {
            self.record_denial(principal, resource_id, capability, reason);
        }
        decision
    }

    /// The dispatch verdict as a pass/fail result — the one seam both §6.2
    /// chokepoints (`run_mission` and `team_dispatch`) consult, so the two
    /// paths cannot drift into enforcing different rules. Denials are appended
    /// to the durable trail (INV-6) before the `Err` is returned.
    pub fn require_dispatch(&self, team_id: &str) -> Result<(), String> {
        let dispatcher = self.principal_for_team(team_id);
        let resource = team_resource(team_id);
        match self.check_recorded(&dispatcher, &resource, Capability::DispatchTo, 0.0) {
            Decision::Allowed => Ok(()),
            Decision::Denied { reason } => Err(format!(
                "rbac: dispatch to team '{team_id}' denied — {reason}"
            )),
            Decision::RequiresApproval {
                reason,
                approvers,
                quorum,
                ..
            } => Err(format!(
                "rbac: dispatch to team '{team_id}' requires org approval — {reason} (approvers {approvers:?}, quorum {quorum}); clear an approval request in the org, or run dry_run=true to preview without spending."
            )),
        }
    }

    /// The human [`Principal`] identified by `user_id`: its `role_id` is the org
    /// role whose `humans` bridge lists that identity (the human half of the
    /// identity join, symmetric with [`Self::principal_for_team`]). This is what
    /// turns a peer reached under shared-secret pairing into a *scopable* actor
    /// rather than the `HUMAN_OPERATOR` sentinel — an unbound user yields
    /// `role_id: None`, which a governed workspace denies (INV-6) rather than
    /// guessing, and which an ungoverned one passes (opt-in governance).
    pub fn principal_for_human(&self, user_id: &str, node: NodeId) -> Principal {
        let role_id = self.org.as_ref().and_then(|org| {
            org.roles
                .iter()
                .find(|r| r.humans.iter().any(|h| h == user_id))
                .map(|r| r.id.clone())
        });
        Principal::human(node, user_id, role_id)
    }

    /// The write verdict as a pass/fail result — the seam the §6.4 unified
    /// WriteGate consults, mirroring [`Self::require_dispatch`] so the two
    /// chokepoints cannot drift. It takes the acting [`Principal`] directly
    /// (resolved by the caller via [`Self::principal_for_human`] or
    /// [`Self::principal_for_team`]) because a write is attributed to whoever
    /// holds the edit lock, not to a team. Denials are appended to the durable
    /// trail (INV-6) before the `Err` is returned.
    pub fn require_write(&self, principal: &Principal) -> Result<(), String> {
        match self.check_recorded(principal, WORKSPACE_RESOURCE, Capability::Write, 0.0) {
            Decision::Allowed => Ok(()),
            Decision::Denied { reason } => Err(format!(
                "rbac: write to the workspace denied — {reason}"
            )),
            Decision::RequiresApproval {
                reason,
                approvers,
                quorum,
                ..
            } => Err(format!(
                "rbac: write to the workspace requires org approval — {reason} (approvers {approvers:?}, quorum {quorum}); clear an approval request in the org to proceed."
            )),
        }
    }

    /// Append a denial op to the durable trail. The clock is re-derived from
    /// the log itself (same durable-refresh pattern as `ChangeAttributor`), so
    /// a Governor never mints a colliding `op_id` with any other producer on
    /// this node, present or future.
    fn record_denial(
        &self,
        principal: &Principal,
        resource_id: &str,
        capability: Capability,
        reason: &str,
    ) {
        let store = OpLogStore::open(&self.workspace);
        let mut clock = VectorClock::new();
        for op in store.load().ops() {
            clock.merge(&op.clock);
        }
        let counter = clock.increment(&principal.node_id);
        let op = Op {
            op_id: OpId::new(principal.node_id.clone(), counter),
            clock,
            principal: principal.clone(),
            causal_deps: Vec::new(),
            intent: format!(
                "rbac-deny: {} on {resource_id} — {reason}",
                capability.as_str()
            ),
            // Inert for materialize *and* for the attribution dedupe set: a
            // denial is not a content change (module doc).
            effect: OpEffect::Legacy,
            outcome: EventOutcome::Failure,
            authorized_by: None,
        };
        if let Err(e) = store.append(&op) {
            log::warn!("governor: failed to append denial op: {e}");
        }
    }
}

/// A two-role org for gate tests: `ai_lead` staffs team `rewrite-team` with a
/// `DispatchTo` grant on its team resource and a `Write` grant on the workspace
/// (bound to human `alice`), so both the dispatch and write seams have a
/// grantee and an ungrantable stranger to test against.
#[cfg(test)]
pub(crate) fn test_governed_org() -> CompanyOrg {
    use crate::editor::company_org::{AutonomyTier, CredentialGrant, Resource, ResourceKind, Role};
    CompanyOrg {
        id: "test-org".into(),
        name: "Test Org".into(),
        departments: vec![],
        roles: vec![Role {
            id: "ai_lead".into(),
            title: "AI Lead".into(),
            department_id: "eng".into(),
            reports_to: None,
            tier: AutonomyTier::default(),
            agent_team: "rewrite-team".into(),
            humans: vec!["alice".into()],
        }],
        resources: vec![
            Resource {
                id: team_resource("rewrite-team"),
                name: "Rewrite team".into(),
                kind: Some(ResourceKind::Other),
                secret_ref: String::new(),
                sensitive: false,
            },
            Resource {
                id: WORKSPACE_RESOURCE.into(),
                name: "Workspace".into(),
                kind: Some(ResourceKind::Other),
                secret_ref: String::new(),
                sensitive: false,
            },
        ],
        grants: vec![
            CredentialGrant {
                role_id: "ai_lead".into(),
                resource_id: team_resource("rewrite-team"),
                capabilities: vec![Capability::DispatchTo],
                spend_limit_per_action: 0.0,
            },
            CredentialGrant {
                role_id: "ai_lead".into(),
                resource_id: WORKSPACE_RESOURCE.into(),
                capabilities: vec![Capability::Write],
                spend_limit_per_action: 0.0,
            },
        ],
        policies: vec![],
        playbooks: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ungoverned(dir: &Path) -> Governor {
        Governor::with_org(dir, None)
    }

    #[test]
    fn ungoverned_workspace_passes_even_an_unidentified_principal() {
        // Unattended safety: governance is opt-in. No org document → the gate
        // is inert and never blocks a mission (spec §6).
        let dir = tempfile::tempdir().unwrap();
        let g = ungoverned(dir.path());
        assert!(!g.governed());
        let nobody = Principal::agent(NodeId::local(), 0, None, None);
        assert_eq!(
            g.check(&nobody, &team_resource("t"), Capability::DispatchTo, 0.0),
            Decision::Allowed
        );
    }

    #[test]
    fn governed_workspace_denies_a_principal_without_a_role() {
        // INV-6: under a governed org, an unidentified principal cannot act.
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let nobody = Principal::agent(NodeId::local(), 0, None, Some("rewrite-team".into()));
        match g.check(
            &nobody,
            &team_resource("rewrite-team"),
            Capability::DispatchTo,
            0.0,
        ) {
            Decision::Denied { reason } => assert!(reason.contains("role_id"), "{reason}"),
            other => panic!("expected denial, got {other:?}"),
        }
    }

    #[test]
    fn role_is_resolved_through_the_agent_team_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let p = g.principal_for_team("rewrite-team");
        assert_eq!(p.role_id.as_deref(), Some("ai_lead"));
        // An unbound team yields role_id: None → denied, never guessed.
        let unbound = g.principal_for_team("ghost-team");
        assert_eq!(unbound.role_id, None);
    }

    #[test]
    fn granted_dispatch_is_allowed_and_ungranted_is_denied() {
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let lead = g.principal_for_team("rewrite-team");
        assert_eq!(
            g.check(
                &lead,
                &team_resource("rewrite-team"),
                Capability::DispatchTo,
                0.0
            ),
            Decision::Allowed
        );
        // Same role, a resource it holds no grant on → deny by default.
        match g.check(&lead, "banking", Capability::Spend, 1.0) {
            Decision::Denied { reason } => assert!(reason.contains("no 'spend' grant"), "{reason}"),
            other => panic!("expected denial, got {other:?}"),
        }
    }

    #[test]
    fn denial_is_appended_to_the_durable_trail() {
        // INV-6: the denial itself is an audit op, readable from the same log.
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let nobody = Principal::agent(NodeId::local(), 0, None, None);
        let d = g.check_recorded(&nobody, "anything", Capability::DispatchTo, 0.0);
        assert!(matches!(d, Decision::Denied { .. }));

        let log = OpLogStore::open(dir.path()).load();
        assert_eq!(log.ops().len(), 1);
        let op = &log.ops()[0];
        assert!(
            op.intent.contains("rbac-deny: dispatch_to"),
            "{}",
            op.intent
        );
        assert_eq!(op.outcome, EventOutcome::Failure);
        assert_eq!(op.principal.kind, crate::automation::PrincipalKind::Agent);
    }

    #[test]
    fn allow_appends_nothing_and_denials_do_not_pollute_attribution_dedupe() {
        // Denial ops are Legacy-effect: they advance the clock but are not
        // `(path, digest)` entries — the System-B dedupe set must not see them.
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let lead = g.principal_for_team("rewrite-team");
        let _ = g.check_recorded(
            &lead,
            &team_resource("rewrite-team"),
            Capability::DispatchTo,
            0.0,
        );
        assert_eq!(
            OpLogStore::open(dir.path()).load().ops().len(),
            0,
            "allows are not denials"
        );

        let denied = g.check_recorded(&lead, "nowhere", Capability::Collaborate, 0.0);
        assert!(matches!(denied, Decision::Denied { .. }));
        let mut attributor = crate::automation::ChangeAttributor::open_human(dir.path(), "alice");
        // The very same (path, digest) the deny op never touched records fine.
        assert!(attributor
            .record(
                "src/lib.rs",
                &crate::automation::ChangeAttributor::digest_of_content("x"),
                "edit"
            )
            .is_some());
        // …and the attributor's clock absorbed the denial op's counter, so the
        // two producers on different nodes can never collide on an `op_id`.
        let log = attributor.store().load();
        assert_eq!(log.ops().len(), 2);
        assert_ne!(log.ops()[0].op_id, log.ops()[1].op_id);
        assert!(
            log.ops()[1].clock.get(&NodeId::local()) >= 1,
            "the attribution clock must have merged the denial's node counter"
        );
    }

    #[test]
    fn new_capabilities_parse_and_round_trip() {
        for (text, want) in [
            ("collaborate", Capability::Collaborate),
            ("engage", Capability::Collaborate),
            ("dispatch", Capability::DispatchTo),
            ("dispatch_to", Capability::DispatchTo),
            ("staff", Capability::DispatchTo),
        ] {
            assert_eq!(Capability::parse(text), Some(want), "{text}");
            assert_eq!(Capability::parse(want.as_str()), Some(want));
        }
    }

    #[test]
    fn open_reads_the_saved_org_document() {
        // End-to-end through the real sealed persistence, not just with_org.
        let dir = tempfile::tempdir().unwrap();
        assert!(!Governor::open(dir.path()).governed());
        crate::editor::company_org::save_company_org(dir.path(), &test_governed_org());
        let g = Governor::open(dir.path());
        assert!(g.governed());
        let lead = g.principal_for_team("rewrite-team");
        assert_eq!(
            g.check(
                &lead,
                &team_resource("rewrite-team"),
                Capability::DispatchTo,
                0.0
            ),
            Decision::Allowed
        );
    }

    #[test]
    fn require_dispatch_passes_granted_and_records_only_denials() {
        let dir = tempfile::tempdir().unwrap();
        // Granted: Ok, and no audit ops — an allow is not a denial.
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        assert!(g.require_dispatch("rewrite-team").is_ok());
        assert!(OpLogStore::open(dir.path()).load().ops().is_empty());
        // Ungranted: a named error, and the denial lands in the trail (INV-6).
        let mut org = test_governed_org();
        org.grants.clear();
        let g = Governor::with_org(dir.path(), Some(org));
        let e = g.require_dispatch("rewrite-team").unwrap_err();
        assert!(e.contains("rbac") && e.contains("denied"), "{e}");
        assert_eq!(OpLogStore::open(dir.path()).load().ops().len(), 1);
    }

    #[test]
    fn require_dispatch_refuses_approval_scoped_and_points_at_the_preview() {
        use crate::editor::company_org::ApprovalPolicy;
        let dir = tempfile::tempdir().unwrap();
        let mut org = test_governed_org();
        org.policies.push(ApprovalPolicy {
            id: "dispatch_gate".into(),
            resource_id: team_resource("rewrite-team"),
            capability: Capability::DispatchTo,
            threshold: None,
            required_approvers: vec!["ai_lead".into()],
            quorum: 1,
            description: String::new(),
        });
        let g = Governor::with_org(dir.path(), Some(org));
        let e = g.require_dispatch("rewrite-team").unwrap_err();
        assert!(e.contains("approval"), "{e}");
        assert!(e.contains("dry_run=true"), "{e}");
        // An approval request is not a denial (INV-6): no op is minted.
        assert!(OpLogStore::open(dir.path()).load().ops().is_empty());
    }

    #[test]
    fn org_own_team_roles_resolve_through_the_empty_bridge() {
        use crate::editor::company_org::{AutonomyTier, Role};
        let dir = tempfile::tempdir().unwrap();
        let mut org = test_governed_org();
        org.roles.push(Role {
            id: "own_lead".into(),
            title: "Own Lead".into(),
            department_id: "eng".into(),
            reports_to: None,
            tier: AutonomyTier::default(),
            agent_team: String::new(), // staffs the org's own team by convention
            humans: Vec::new(),
        });
        let g = Governor::with_org(dir.path(), Some(org));
        // Org name "Test Org" → own-team slug "test-org" (org_team_slug).
        assert_eq!(
            g.principal_for_team("test-org").role_id.as_deref(),
            Some("own_lead")
        );
        // An unrelated team still resolves to nothing; it is never forced.
        assert_eq!(g.principal_for_team("other-team").role_id, None);
    }

    #[test]
    fn bound_human_resolves_through_the_humans_bridge_and_writes() {
        // The identity join that unblocks §6.4: a real user_id maps to an org
        // role, so its Write grant *scopes* the edit rather than denying it.
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let alice = g.principal_for_human("alice", NodeId::local());
        assert_eq!(alice.role_id.as_deref(), Some("ai_lead"));
        assert!(alice.is_human());
        assert!(g.require_write(&alice).is_ok());
        // An allow mints no audit op.
        assert!(OpLogStore::open(dir.path()).load().ops().is_empty());
    }

    #[test]
    fn unbound_human_is_denied_write_and_the_denial_is_audited() {
        // INV-6 under governance: a user with no role binding cannot write, and
        // the refusal lands in the durable trail (never a silent block).
        let dir = tempfile::tempdir().unwrap();
        let g = Governor::with_org(dir.path(), Some(test_governed_org()));
        let bob = g.principal_for_human("bob", NodeId::local());
        assert_eq!(bob.role_id, None, "bob is not in any role's humans list");
        let e = g.require_write(&bob).unwrap_err();
        assert!(e.contains("rbac") && e.contains("denied"), "{e}");
        assert_eq!(OpLogStore::open(dir.path()).load().ops().len(), 1);
    }

    #[test]
    fn ungoverned_workspace_passes_write_for_an_unbound_principal() {
        // Opt-in governance: with no org document the write seam is inert, so
        // the manual-edit watcher path is never blocked by a missing binding.
        let dir = tempfile::tempdir().unwrap();
        let g = ungoverned(dir.path());
        let nobody = Principal::human(NodeId::local(), "whoever", None);
        assert!(g.require_write(&nobody).is_ok());
    }
}
