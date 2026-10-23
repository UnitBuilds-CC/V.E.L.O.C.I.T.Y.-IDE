//! Company organization model for an AI-staffed operating company.
//!
//! This layer sits *on top of* the existing [`ExpertTeam`] roster and turns a
//! flat list of AI members into a governed corporate org: departments, roles in
//! an reporting hierarchy, external systems ("resources") such as source
//! control, email, banking and social platforms, per-role **capability grants**
//! over those resources, and **approval policies** that decide when an action
//! must be signed off before it runs.
//!
//! Design principles (mirroring the user's requirements):
//!
//! 1. **Deny by default.** A role may only touch a resource for a capability it
//!    has an explicit [`CredentialGrant`] for. Everything else is denied.
//! 2. **Deterministic over inferential.** The permission decision is a pure
//!    function ([`check_permission`]) over stored data — no LLM call is needed
//!    to decide "may this role do X, and who must approve". This cuts cost and
//!    makes the gate reliable and auditable.
//! 3. **Secrets never live here.** A [`Resource`] stores only a `secret_ref`
//!    pointer (an env-var name or keychain label). Raw banking/email/GitHub
//!    credentials are resolved later by the credential vault and scrubbed after
//!    use (see `velocity-ide::credential_guard`); they are never persisted in
//!    the org document.
//! 4. **Sensitive actions gate on humans.** Actions on a `sensitive` resource
//!    that mutate outward (write/deploy/spend/communicate) always require an
//!    approval decision, even when no explicit numeric threshold is hit.
//!
//! Persistence follows the expert-team convention: the document is serialized
//! as JSON and sealed with the workspace key into `.velocity/company_org.nda`.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Core enums
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// A discrete authority a role can hold over a resource. Ordered from benign to
/// privileged so policy checks read naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    /// Observe data (read repo, view inbox, read balances).
    Read,
    /// Produce content into the org (draft, commit to a branch, write docs).
    Write,
    /// Send something outward to third parties (email, social post, chat reply).
    Communicate,
    /// Move money / incur cost (payouts, ad spend, cloud purchase).
    Spend,
    /// Push running systems (deploy, restart, rotate keys, change infra).
    Deploy,
    /// Sign off on another role's pending action.
    Approve,
    /// Direct / reassign other roles (management orchestration).
    Manage,
    /// Engage another team's agents in a shared transcript (System C, spec §6:
    /// "AI department may engage Manufacturing agents" as a `CredentialGrant`).
    Collaborate,
    /// Dispatch / run headless sub-agents — the mission/team_dispatch spend.
    /// Outward: it commits model money and mutates the workspace through others.
    DispatchTo,
}
impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::Read => "read",
            Capability::Write => "write",
            Capability::Communicate => "communicate",
            Capability::Spend => "spend",
            Capability::Deploy => "deploy",
            Capability::Approve => "approve",
            Capability::Manage => "manage",
            Capability::Collaborate => "collaborate",
            Capability::DispatchTo => "dispatch_to",
        }
    }
    pub fn parse(s: &str) -> Option<Capability> {
        match s.trim().to_lowercase().as_str() {
            "read" | "view" | "readonly" => Some(Capability::Read),
            "write" | "edit" | "commit" => Some(Capability::Write),
            "communicate" | "send" | "post" | "message" => Some(Capability::Communicate),
            "spend" | "pay" | "purchase" | "disburse" => Some(Capability::Spend),
            "deploy" | "release" | "infra" => Some(Capability::Deploy),
            "approve" | "signoff" | "sign_off" => Some(Capability::Approve),
            "manage" | "orchestrate" | "direct" => Some(Capability::Manage),
            "collaborate" | "engage" => Some(Capability::Collaborate),
            "dispatch" | "dispatch_to" | "staff" => Some(Capability::DispatchTo),
            _ => None,
        }
    }

    /// Capabilities that act outward on the world and therefore trip the
    /// sensitive-resource human-approval gate. `DispatchTo` counts: running a
    /// headless sub-agent spends model money and edits through another pair of
    /// hands. `Collaborate` does not: engaging a shared transcript is ingress.
    fn is_outward(&self) -> bool {
        matches!(
            self,
            Capability::Write
                | Capability::Communicate
                | Capability::Spend
                | Capability::Deploy
                | Capability::DispatchTo
        )
    }
}
/// The kind of external system a [`Resource`] represents. Used for grouping and
/// as a fallback for default sensitivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    SourceControl,
    Email,
    Chat,
    Banking,
    Social,
    Cloud,
    Analytics,
    /// Meta-resource covering the credential vault itself. Deploy on this
    /// resource is the ONLY sanctioned way to swap a stored `secret_ref` —
    /// rotation goes through the same dual-control gate as spend, and the
    /// raw secret never enters the org document (only its pointer does).
    Credentials,
    Other,
}
impl ResourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceKind::SourceControl => "source_control",
            ResourceKind::Email => "email",
            ResourceKind::Chat => "chat",
            ResourceKind::Banking => "banking",
            ResourceKind::Social => "social",
            ResourceKind::Cloud => "cloud",
            ResourceKind::Analytics => "analytics",
            ResourceKind::Credentials => "credentials",
            ResourceKind::Other => "other",
        }
    }
    pub fn parse(s: &str) -> ResourceKind {
        match s.trim().to_lowercase().as_str() {
            "source_control" | "git" | "github" | "scm" => ResourceKind::SourceControl,
            "email" | "mail" | "smtp" => ResourceKind::Email,
            "chat" | "im" | "slack" | "discord" => ResourceKind::Chat,
            "banking" | "bank" | "payments" | "stripe" | "finance" => ResourceKind::Banking,
            "social" | "social_media" | "twitter" | "linkedin" | "reddit" => ResourceKind::Social,
            "cloud" | "infra" | "hosting" | "deploy" => ResourceKind::Cloud,
            "analytics" | "metrics" | "telemetry" => ResourceKind::Analytics,
            "credentials" | "vault" | "secrets" | "keys" => ResourceKind::Credentials,
            _ => ResourceKind::Other,
        }
    }

    /// Whether this kind is inherently sensitive (mutating actions need a human).
    fn default_sensitive(&self) -> bool {
        matches!(
            self,
            ResourceKind::Banking
                | ResourceKind::SourceControl
                | ResourceKind::Cloud
                | ResourceKind::Email
                | ResourceKind::Credentials
        )
    }
}
/// How much independent authority a role is trusted with by default. This is a
/// hint for tooling and prompts; the hard gate is always [`check_permission`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutonomyTier {
    /// Always propose, never execute without a human/manager sign-off.
    Assisted,
    /// Ordinary day-to-day autonomy within grants and thresholds.
    #[default]
    Standard,
    /// Senior roles: higher spend/deploy thresholds before approval kicks in.
    Trusted,
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Structures
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// A functional department (Executive, Engineering, Marketing, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Department {
    pub id: String,
    pub name: String,
    /// One-line mission used in prompts and the org panel.
    #[serde(default)]
    pub charter: String,
}
/// A named position in the org chart. Ties a person/agent to a reporting line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    /// Display title, e.g. "CTO".
    pub title: String,
    pub department_id: String,
    /// Role id this role reports to. `None` == top of the org.
    #[serde(default)]
    pub reports_to: Option<String>,
    #[serde(default)]
    pub tier: AutonomyTier,
    /// Slug of the [`crate::editor::expert_team::ExpertTeam`] that AI-staffs this
    /// role. Empty means the role is bound to the org's own team
    /// ([`org_team_slug`]); this is the bridge that turns the chart into a
    /// dispatchable workforce via `team_dispatch`.
    #[serde(default)]
    pub agent_team: String,
    /// Real human identities (`Principal.user_id`) that staff this role. This is
    /// the human half of the identity bridge, symmetric with `agent_team`: it is
    /// what lets a write reached under a peer's shared-secret pairing resolve to
    /// an org `role_id` and therefore be *scoped* by RBAC rather than merely
    /// denied. Empty means no human is bound to the role yet.
    #[serde(default)]
    pub humans: Vec<String>,
}
/// An external system the org can act upon. Stores a *pointer* to its secret,
/// never the secret itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: Option<ResourceKind>,
    /// Env-var name or keychain label the vault resolves at execution time.
    /// Empty means the resource needs no credential (public/analytics only).
    #[serde(default)]
    pub secret_ref: String,
    /// If true, outward actions always require an approval decision regardless
    /// of thresholds. Defaults from `kind` when omitted on import.
    #[serde(default)]
    pub sensitive: bool,
}
/// Grants a role a set of capabilities over one resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialGrant {
    pub role_id: String,
    pub resource_id: String,
    pub capabilities: Vec<Capability>,
    /// Maximum currency units this role may [`Capability::Spend`] in a single
    /// action before an approval is forced. `0.0` = no independent spend.
    #[serde(default)]
    pub spend_limit_per_action: f64,
}
/// A deterministic rule deciding when an action must be approved and by whom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    pub id: String,
    /// Empty = applies to every resource; otherwise matched exactly.
    #[serde(default)]
    pub resource_id: String,
    pub capability: Capability,
    /// When set, the policy only fires for actions whose `amount` is at or above
    /// this value (spend total, or count for batch sends). `None` = always fires.
    #[serde(default)]
    pub threshold: Option<f64>,
    /// Role ids whose approval counts toward quorum. Empty = the human operator.
    #[serde(default)]
    pub required_approvers: Vec<String>,
    /// How many of `required_approvers` must approve. Min 1.
    #[serde(default = "default_quorum")]
    pub quorum: usize,
    #[serde(default)]
    pub description: String,
}
fn default_quorum() -> usize {
    1
}
/// The whole company org. One document per workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyOrg {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub departments: Vec<Department>,
    #[serde(default)]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub grants: Vec<CredentialGrant>,
    #[serde(default)]
    pub policies: Vec<ApprovalPolicy>,
    /// Deterministic, permission-gated procedures owned by a role.
    #[serde(default)]
    pub playbooks: Vec<Playbook>,
}
impl CompanyOrg {
    pub fn find_role(&self, id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == id)
    }
    pub fn find_resource(&self, id: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.id == id)
    }
    pub fn find_department(&self, id: &str) -> Option<&Department> {
        self.departments.iter().find(|d| d.id == id)
    }
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Permission engine (deterministic, inference-free)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// Outcome of a permission check. This is the *only* thing the enforcement
/// layer needs: it can be evaluated with zero LLM calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Role may act now, unaided.
    Allowed,
    /// Role may act only after the listed approvers sign off. An empty
    /// `approvers` list means the human operator is the approver.
    RequiresApproval {
        policy_ids: Vec<String>,
        approvers: Vec<String>,
        quorum: usize,
        reason: String,
    },
    /// Role may not act at all (deny by default, or below a hard floor).
    Denied { reason: String },
}
/// Evaluate whether `role_id` may perform `capability` on `resource_id`.
//
//
// `amount` is a dimensionless magnitude the caller supplies: for
/// [`Capability::Spend`] it is the currency total, for a broadcast it can be the
/// recipient count, otherwise pass `0.0`.
//
//
// The evaluation is deterministic and side-effect free:
/// 1. Deny unless an explicit grant covers (role, resource, capability).
/// 2. Collect every policy whose capability (+resource +threshold) matches.
/// 3. If no policy fires but the action is outward on a sensitive resource, or
///    spend exceeds the grant's per-action limit, still require human approval.
/// 4. Otherwise allow.
pub fn check_permission(
    org: &CompanyOrg,
    role_id: &str,
    resource_id: &str,
    capability: Capability,
    amount: f64,
) -> Decision {
    // 1. Deny by default — an explicit grant is required.
    let grant = match org.grants.iter().find(|g| {
        g.role_id == role_id && g.resource_id == resource_id && g.capabilities.contains(&capability)
    }) {
        Some(g) => g,
        None => {
            return Decision::Denied {
                reason: format!(
                    "role '{}' has no '{}' grant on resource '{}'",
                    role_id,
                    capability.as_str(),
                    resource_id
                ),
            }
        }
    };
    let resource = org.find_resource(resource_id);

    // 2. Matching approval policies.
    let mut policy_ids: Vec<String> = Vec::new();
    let mut approvers: Vec<String> = Vec::new();
    let mut quorum = 1usize;
    for p in org.policies.iter().filter(|p| {
        p.capability == capability && (p.resource_id.is_empty() || p.resource_id == resource_id)
    }) {
        let fires = match p.threshold {
            None => true,
            Some(t) => amount >= t,
        };
        if fires {
            policy_ids.push(p.id.clone());
            for a in &p.required_approvers {
                if !approvers.contains(a) {
                    approvers.push(a.clone());
                }
            }
            quorum = quorum.max(p.quorum.max(1));
        }
    }

    // 3. Implicit human gates for sensitive / over-limit actions.
    let mut forced = Vec::new();
    if let Some(r) = resource {
        if r.sensitive && capability.is_outward() {
            forced.push(format!(
                "'{}' is a sensitive resource; outward '{}' requires sign-off",
                r.name,
                capability.as_str()
            ));
        }
    }
    if capability == Capability::Spend && amount > grant.spend_limit_per_action.max(0.0) {
        forced.push(format!(
            "spend {} exceeds role limit {}",
            amount, grant.spend_limit_per_action
        ));
    }
    if !policy_ids.is_empty() || !forced.is_empty() {
        let reason = if forced.is_empty() {
            format!("matched {} approval policy(ies)", policy_ids.len())
        } else {
            forced.join("; ")
        };
        return Decision::RequiresApproval {
            policy_ids,
            approvers,
            quorum,
            reason,
        };
    }

    // 4. Nothing to approve.
    Decision::Allowed
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Persistence (sealed NDA, mirroring the expert-team convention)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
const ORG_LABEL: &[u8] = b"company_org";
fn org_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".velocity").join("company_org.nda")
}
/// Load the org from `.velocity/company_org.nda`, if present and parseable.
pub fn load_company_org(workspace_root: &Path) -> Option<CompanyOrg> {
    let path = org_path(workspace_root);
    let bytes = fs::read(&path).ok()?;
    let plain = crate::agent::crypto::open(workspace_root, ORG_LABEL, &bytes);
    let text = String::from_utf8_lossy(&plain);
    serde_json::from_str::<CompanyOrg>(&text).ok()
}
/// Persist the org, sealed with the workspace key. Returns success.
pub fn save_company_org(workspace_root: &Path, org: &CompanyOrg) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(org) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, ORG_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(org_path(workspace_root), bytes).is_ok()
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// UnitBuilds CC preset org
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// The full Google-scale-style, AI-staffed org for UnitBuilds CC. This is a
/// starting skeleton: departments, roles with reporting lines, external
/// resources (secret *references* only), per-role capability grants, and the
/// approval policies that keep high-leverage actions gated.
pub fn default_company_org() -> CompanyOrg {
    let sensitive = |kind: ResourceKind| Resource {
        id: String::new(),
        name: String::new(),
        kind: Some(kind),
        secret_ref: String::new(),
        sensitive: kind.default_sensitive(),
    };
    let res = |id: &str, name: &str, kind: ResourceKind, secret_ref: &str| {
        let mut r = sensitive(kind);
        r.id = id.to_string();
        r.name = name.to_string();
        r.secret_ref = secret_ref.to_string();
        r
    };
    let resources = vec![
        res(
            "github",
            "UnitBuilds GitHub org",
            ResourceKind::SourceControl,
            "UNITBUILDS_GITHUB_TOKEN",
        ),
        res(
            "email",
            "support@unitbuilds.com mailbox",
            ResourceKind::Email,
            "UNITBUILDS_EMAIL_TOKEN",
        ),
        res(
            "chat",
            "Live user chat / Discord",
            ResourceKind::Chat,
            "UNITBUILDS_CHAT_TOKEN",
        ),
        res(
            "banking",
            "Banking / payment provider",
            ResourceKind::Banking,
            "UNITBUILDS_BANKING_TOKEN",
        ),
        res(
            "social_x",
            "X / Twitter account",
            ResourceKind::Social,
            "UNITBUILDS_X_TOKEN",
        ),
        res(
            "social_linkedin",
            "LinkedIn company page",
            ResourceKind::Social,
            "UNITBUILDS_LINKEDIN_TOKEN",
        ),
        res(
            "cloud",
            "Cloud / hosting control plane",
            ResourceKind::Cloud,
            "UNITBUILDS_CLOUD_TOKEN",
        ),
        res(
            "analytics",
            "Product & revenue analytics",
            ResourceKind::Analytics,
            "",
        ),
        // Meta-resource for the credential vault. Deploy on this surface is the
        // ONLY sanctioned way to swap a stored `secret_ref` (rotate an API key,
        // change a bank password, re-seed a OAuth token). Rotation is
        // irreversible and lockout-capable, so the preset wires it through
        // dual-control (Operations initiator + CEO + CTO approvers).        res
        res(
            "credentials",
            "Credential vault (secret_ref pointers)",
            ResourceKind::Credentials,
            "UNITBUILDS_VAULT_MASTER_KEY",
        ),
    ];
    let dept = |id: &str, name: &str, charter: &str| Department {
        id: id.to_string(),
        name: name.to_string(),
        charter: charter.to_string(),
    };
    let departments = vec![
        dept(
            "executive",
            "Executive",
            "Business direction, strategy, final sign-off.",
        ),
        dept(
            "management",
            "Management",
            "Orchestrate departments, keep work in line and on schedule.",
        ),
        dept(
            "engineering",
            "Engineering",
            "Build and ship patches and new features continuously.",
        ),
        dept(
            "testing",
            "Testing",
            "Verify features and patches are stable; record demos.",
        ),
        dept(
            "scouting",
            "Scouting",
            "Research candidate features and high-traction highlights.",
        ),
        dept(
            "marketing",
            "Marketing",
            "Advertise and write about capabilities across platforms.",
        ),
        dept(
            "support",
            "Support",
            "Answer email and chat, support users.",
        ),
        dept(
            "finance",
            "Finance",
            "Keep margins healthy without stifling experimentation.",
        ),
        dept(
            "legal",
            "Legal",
            "Ensure lawful operation and compliant marketing.",
        ),
        dept(
            "operations",
            "Operations",
            "Key rotation, uptime, reporting, running the business.",
        ),
    ];
    let role = |id: &str,
                title: &str,
                department_id: &str,
                reports_to: Option<&str>,
                tier: AutonomyTier| Role {
        id: id.to_string(),
        title: title.to_string(),
        department_id: department_id.to_string(),
        reports_to: reports_to.map(|s| s.to_string()),
        tier,
        // Empty => bound to the org's own team at sync time (see org_team_slug).
        agent_team: String::new(),
        humans: Vec::new(),
    };
    let roles = vec![
        role("ceo", "CEO", "executive", None, AutonomyTier::Trusted),
        role(
            "cto",
            "CTO",
            "executive",
            Some("ceo"),
            AutonomyTier::Trusted,
        ),
        role(
            "coo",
            "Chief of Staff",
            "management",
            Some("ceo"),
            AutonomyTier::Trusted,
        ),
        role(
            "pm",
            "Program Manager",
            "management",
            Some("coo"),
            AutonomyTier::Standard,
        ),
        role(
            "dev_lead",
            "Engineering Lead",
            "engineering",
            Some("cto"),
            AutonomyTier::Trusted,
        ),
        role(
            "dev_backend",
            "Backend Developer",
            "engineering",
            Some("dev_lead"),
            AutonomyTier::Standard,
        ),
        role(
            "dev_frontend",
            "Frontend Developer",
            "engineering",
            Some("dev_lead"),
            AutonomyTier::Standard,
        ),
        role(
            "dev_platform",
            "Platform / Release Engineer",
            "engineering",
            Some("dev_lead"),
            AutonomyTier::Standard,
        ),
        role(
            "qa_lead",
            "QA Lead",
            "testing",
            Some("cto"),
            AutonomyTier::Standard,
        ),
        role(
            "qa_engineer",
            "QA Engineer",
            "testing",
            Some("qa_lead"),
            AutonomyTier::Standard,
        ),
        role(
            "demo_recorder",
            "Demo Recorder",
            "testing",
            Some("qa_lead"),
            AutonomyTier::Standard,
        ),
        role(
            "scout",
            "Product Scout",
            "scouting",
            Some("cto"),
            AutonomyTier::Standard,
        ),
        role(
            "researcher",
            "Market Researcher",
            "scouting",
            Some("scout"),
            AutonomyTier::Standard,
        ),
        role(
            "mkt_lead",
            "Marketing Lead",
            "marketing",
            Some("ceo"),
            AutonomyTier::Standard,
        ),
        role(
            "writer",
            "Content Writer",
            "marketing",
            Some("mkt_lead"),
            AutonomyTier::Assisted,
        ),
        role(
            "social_operator",
            "Social Media Operator",
            "marketing",
            Some("mkt_lead"),
            AutonomyTier::Assisted,
        ),
        role(
            "support_lead",
            "Support Lead",
            "support",
            Some("coo"),
            AutonomyTier::Standard,
        ),
        role(
            "support_agent",
            "Support Agent",
            "support",
            Some("support_lead"),
            AutonomyTier::Assisted,
        ),
        role("cfo", "CFO", "finance", Some("ceo"), AutonomyTier::Trusted),
        role(
            "accountant",
            "Accountant",
            "finance",
            Some("cfo"),
            AutonomyTier::Standard,
        ),
        role(
            "counsel",
            "General Counsel",
            "legal",
            Some("ceo"),
            AutonomyTier::Trusted,
        ),
        role(
            "compliance",
            "Compliance Reviewer",
            "legal",
            Some("counsel"),
            AutonomyTier::Standard,
        ),
        role(
            "ops_lead",
            "Operations Lead",
            "operations",
            Some("coo"),
            AutonomyTier::Trusted,
        ),
        role(
            "sysadmin",
            "Systems Administrator",
            "operations",
            Some("ops_lead"),
            AutonomyTier::Standard,
        ),
        role(
            "analyst",
            "Reporting Analyst",
            "operations",
            Some("ops_lead"),
            AutonomyTier::Standard,
        ),
    ];
    let grant =
        |role_id: &str, resource_id: &str, caps: &[Capability], spend_limit: f64| CredentialGrant {
            role_id: role_id.to_string(),
            resource_id: resource_id.to_string(),
            capabilities: caps.to_vec(),
            spend_limit_per_action: spend_limit,
        };
    use Capability::*;
    let grants = vec![
        // Executive — broad, high limits, approver authority.        grant
        grant("ceo", "banking", &[Read, Spend, Approve], 25_000.0),
        grant("ceo", "github", &[Read, Approve], 0.0),
        grant("ceo", "email", &[Read, Communicate], 0.0),
        grant("cto", "github", &[Read, Write, Deploy, Approve], 0.0),
        // CTO signs off on production deploys (prod_deploy_gate), so must hold
        // Approve authority on the cloud control plane, not just Deploy.        grant
        grant("cto", "cloud", &[Read, Deploy, Approve], 0.0),
        grant("cto", "analytics", &[Read], 0.0),
        // Management — orchestrate + read across, no spend/deploy.        grant
        grant("coo", "email", &[Read, Communicate], 0.0),
        grant("coo", "analytics", &[Read], 0.0),
        grant("coo", "github", &[Read], 0.0),
        grant("pm", "email", &[Read, Communicate], 0.0),
        grant("pm", "github", &[Read], 0.0),
        // Engineering — code + read analytics; platform can deploy staging.        grant
        grant("dev_lead", "github", &[Read, Write, Deploy, Approve], 0.0),
        grant("dev_lead", "cloud", &[Read, Deploy], 0.0),
        grant("dev_backend", "github", &[Read, Write], 0.0),
        grant("dev_frontend", "github", &[Read, Write], 0.0),
        grant("dev_platform", "github", &[Read, Write], 0.0),
        grant("dev_platform", "cloud", &[Read, Deploy], 0.0),
        grant("dev_backend", "analytics", &[Read], 0.0),
        grant("dev_frontend", "analytics", &[Read], 0.0),
        // Testing — read code, record demos, deploy nothing.        grant
        grant("qa_lead", "github", &[Read, Write], 0.0),
        grant("qa_engineer", "github", &[Read, Write], 0.0),
        // QA verifies a shipped patch by reading the public staging/analytics
        // surface (no secret) and asserting it rendered the expected build —
        // so analytics-Read is the gate for the verify_staging_build SOP.        grant
        grant("qa_engineer", "analytics", &[Read], 0.0),
        grant("qa_engineer", "cloud", &[Read], 0.0),
        grant("demo_recorder", "github", &[Read], 0.0),
        // Demo recording captures the public product/analytics surface for
        // Marketing; that surface needs no secret, so analytics-Read is the gate.        grant
        grant("demo_recorder", "analytics", &[Read], 0.0),
        // Scouting — read public signals + analytics, write findings.        grant
        grant("scout", "analytics", &[Read], 0.0),
        grant("scout", "social_x", &[Read], 0.0),
        grant("researcher", "analytics", &[Read], 0.0),
        // Marketing — publish to social, write content, needs legal sign-off.        grant
        grant("mkt_lead", "social_x", &[Read, Write, Communicate], 0.0),
        grant(
            "mkt_lead",
            "social_linkedin",
            &[Read, Write, Communicate],
            0.0,
        ),
        grant("writer", "social_x", &[Read, Write], 0.0),
        grant("writer", "social_linkedin", &[Read, Write], 0.0),
        grant("social_operator", "social_x", &[Read, Communicate], 0.0),
        grant(
            "social_operator",
            "social_linkedin",
            &[Read, Communicate],
            0.0,
        ),
        grant("mkt_lead", "banking", &[Read, Spend], 5_000.0),
        // ad budget
        // Support — email + chat outward, no code, no money.        grant
        grant("support_lead", "email", &[Read, Communicate], 0.0),
        grant("support_lead", "chat", &[Read, Communicate], 0.0),
        grant("support_agent", "email", &[Read, Communicate], 0.0),
        grant("support_agent", "chat", &[Read, Communicate], 0.0),
        // Finance — read + spend within tiers, approve spend.        grant
        grant("cfo", "banking", &[Read, Spend, Approve], 10_000.0),
        grant("cfo", "analytics", &[Read], 0.0),
        grant("accountant", "banking", &[Read], 0.0),
        grant("accountant", "analytics", &[Read], 0.0),
        // Legal — approve outbound comms/marketing, read marketing surfaces.        grant
        grant("counsel", "social_x", &[Read, Approve], 0.0),
        grant("counsel", "email", &[Read, Approve], 0.0),
        grant("compliance", "social_x", &[Read], 0.0),
        grant("compliance", "social_linkedin", &[Read], 0.0),
        // Operations — run systems, rotate keys, deploy infra, report.        grant
        grant("ops_lead", "cloud", &[Read, Deploy], 0.0),
        grant("ops_lead", "github", &[Read], 0.0),
        grant("ops_lead", "analytics", &[Read], 0.0),
        // Rotation authority: ops_lead may *initiate* a credential-vault deploy
        // (the tool-side primitive). It still has to clear the dual-control
        // policy gate below before the pointer actually swaps.        grant
        grant("ops_lead", "credentials", &[Read, Deploy], 0.0),
        // CEO and CTO are the designated approvers for rotation — each holds
        // Approve on the vault so the queue routes to them, not the operator.        grant
        grant("ceo", "credentials", &[Read, Approve], 0.0),
        grant("cto", "credentials", &[Read, Approve], 0.0),
        grant("sysadmin", "cloud", &[Read, Deploy], 0.0),
        grant("sysadmin", "github", &[Read], 0.0),
        grant("analyst", "analytics", &[Read], 0.0),
        grant("analyst", "banking", &[Read], 0.0),
    ];
    let policy = |id: &str,
                  resource_id: &str,
                  capability: Capability,
                  threshold: Option<f64>,
                  approvers: &[&str],
                  quorum: usize,
                  desc: &str| {
        ApprovalPolicy {
            id: id.to_string(),
            resource_id: resource_id.to_string(),
            capability,
            threshold,
            required_approvers: approvers.iter().map(|s| s.to_string()).collect(),
            quorum,
            description: desc.to_string(),
        }
    };
    let policies = vec![
        policy(
            "spend_cfo_gate",
            "banking",
            Spend,
            Some(0.0),
            &["cfo"],
            1,
            "Any banking spend needs CFO sign-off.",
        ),
        policy(
            "spend_ceo_gate",
            "banking",
            Spend,
            Some(1_000.0),
            &["cfo", "ceo"],
            2,
            "Spend at/above 1,000 needs CFO and CEO (two approvals).",
        ),
        policy(
            "prod_deploy_gate",
            "cloud",
            Deploy,
            Some(0.0),
            &["cto"],
            1,
            "Production deploys need CTO sign-off.",
        ),
        policy(
            "github_deploy_gate",
            "github",
            Deploy,
            Some(0.0),
            &["dev_lead", "cto"],
            1,
            "Release/deploy tags need Engineering Lead or CTO.",
        ),
        policy(
            "social_comms_gate",
            "social_x",
            Communicate,
            Some(0.0),
            &["counsel"],
            1,
            "Public posts need Legal review for compliance.",
        ),
        policy(
            "linkedin_comms_gate",
            "social_linkedin",
            Communicate,
            Some(0.0),
            &["counsel"],
            1,
            "Public posts need Legal review for compliance.",
        ),
        policy(
            "email_broadcast_gate",
            "email",
            Communicate,
            Some(50.0),
            &["counsel"],
            1,
            "Broadcasts to many recipients need Legal review.",
        ),
        // Credential-vault rotation: any Deploy on `credentials` fires a
        // dual-control gate — both CEO and CTO must sign off before the
        // secret_ref pointer swaps. A rogue rotation without two-party
        // control would lock the whole company out of its own systems.        policy
        policy(
            "credential_rotation_gate",
            "credentials",
            Deploy,
            Some(0.0),
            &["ceo", "cto"],
            2,
            "Rotating a stored credential needs both CEO and CTO (two approvals).",
        ),
    ];
    CompanyOrg {
        id: "unitbuilds-cc".to_string(),
        name: "UnitBuilds CC".to_string(),
        departments,
        roles,
        resources,
        grants,
        policies,
        playbooks: seed_playbooks(),
    }
}
/// Deterministic cross-SOP handoff chains shipped with the preset org, as
/// `(upstream, downstream)` playbook ids. An edge is queued only after the
/// upstream run finishes clean, and always through the *downstream* SOP's own
/// gate — so "Engineering shipped → Testing verifies → a demo is recorded →/// Marketing is handed the publish job → the post is confirmed live" runs as
/// structure rather than inference, while granting nobody extra authority.
pub const PRESET_SOP_CHAINS: &[(&str, &str)] = &[
    ("ship_github_patch", "verify_staging_build"),
    ("verify_staging_build", "record_feature_demo"),
    ("record_feature_demo", "post_product_announcement"),
    ("post_product_announcement", "confirm_announcement_landed"),
    ("sweep_competitor_changelogs", "plan_feature_roadmap"),
    ("read_support_chats", "reply_to_support_chat"),
    ("report_revenue_to_finance", "review_finance_audit_trail"),
    ("review_campaign_compliance", "post_product_announcement"),
];
/// Attach [`PRESET_SOP_CHAINS`] to freshly seeded playbooks. Missing ids are
/// skipped rather than fatal, so deleting an SOP can't break the preset.
fn apply_preset_chains(list: &mut [Playbook]) {
    for (from, to) in PRESET_SOP_CHAINS {
        if let Some(pb) = list.iter_mut().find(|p| p.id == *from) {
            pb.on_success_handoff = Some((*to).to_string());
        }
    }
}
/// Standard-operating-procedure playbooks shipped with the preset org.
//
//
// Each is a deterministic, ordered procedure owned by a role and gated on that
/// role's `(capability, resource)` grant: the steps only run once the approval
/// engine clears the intent. Where a step genuinely needs language generation
/// it is an explicit [`WorkflowStep::AgentTask`]; everything else (verifying the
/// credential resolves, requesting the outward action, driving the browser) is a
/// deterministic tool node — realising the "deterministic over inference" goal.
/// Chained duties are wired afterwards by [`apply_preset_chains`].
fn seed_playbooks() -> Vec<Playbook> {
    use crate::editor::workflow::{StepOutcome, WorkflowStep};
    let tool = |name: &str, args: serde_json::Value| WorkflowStep::Tool {
        name: name.to_string(),
        args,
    };
    let agent = |prompt: &str| WorkflowStep::AgentTask {
        prompt: prompt.to_string(),
        team: Some("unitbuilds-cc".to_string()),
    };
    let proceed = || WorkflowStep::Condition {
        require: StepOutcome::Ok,
        on_text: None,
        recheck: None,
    };

    // Verdict-gated continuation: proceed only if the prior step's *output text*
    // proves the semantic verdict succeeded (e.g. a browser VERIFY PASS and no
    // VERIFY FAIL / ABSENT FAIL), not merely that the tool call returned. A
    // failed verdict short-circuits the rest of the SOP so an outward dispatch or
    // financial step never runs on a page that didn't actually land.
    let verdict_gate = |all: &[&str], none: &[&str]| WorkflowStep::Condition {
        require: StepOutcome::Ok,
        on_text: Some(crate::editor::workflow::TextPredicate {
            contains_all: all.iter().map(|s| s.to_string()).collect(),
            contains_none: none.iter().map(|s| s.to_string()).collect(),
        }),
        recheck: None,
    };

    // Bounded re-verification. A page that renders late is indistinguishable
    // from a post that never landed on the first read, so the gate is allowed to
    // re-*observe* (a fresh read after a settle) a bounded number of times before
    // halting. The engine refuses any recheck whose tool could mutate state, so
    // an absent verdict is never "retried" by repeating the outward action.
    let verdict_gate_recheck = |all: &[&str],
                                none: &[&str],
                                tool_name: &str,
                                args: serde_json::Value,
                                attempts: u32,
                                settle_ms: u64|
     -> WorkflowStep {
        WorkflowStep::Condition {
            require: StepOutcome::Ok,
            on_text: Some(crate::editor::workflow::TextPredicate {
                contains_all: all.iter().map(|s| s.to_string()).collect(),
                contains_none: none.iter().map(|s| s.to_string()).collect(),
            }),
            recheck: Some(crate::editor::workflow::Recheck {
                tool: tool_name.to_string(),
                args,
                attempts,
                settle_ms,
            }),
        }
    };
    let pb = |id: &str,
              name: &str,
              role_id: &str,
              resource_id: &str,
              capability: Capability,
              description: &str,
              steps: Vec<WorkflowStep>| Playbook {
        id: id.to_string(),
        name: name.to_string(),
        role_id: role_id.to_string(),
        resource_id: resource_id.to_string(),
        capability,
        amount: 0.0,
        description: description.to_string(),
        steps,
        on_success_handoff: None,
    };
    let mut list = vec![        pb(            "ship_github_patch",            "Ship a backend patch",            "dev_backend",            "github",            Capability::Write,            "Verify GitHub access, prepare the patch and open the PR.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "dev_backend", "resource_id": "github", "capability": "write"
}
),                ),                proceed(),                agent("Summarize the pending backend change, list every file it touches and draft a Conventional-Commit message."),                tool(                    "request_org_action",                    serde_json::json!({
"role_id": "dev_backend", "resource_id": "github", "capability": "write", "description": "Open PR with the prepared backend patch"
}
),                ),            ],        ),
// Development drives the browser to open a pull request on GitHub: sign
// in, navigate to the compare view, capture the base and head commit
// SHAs from the page into plan-local vars, then fill the PR title and
// description deterministically (with the SHAs interpolated), attach
// the patch diff, and submit. One gated Write action; no AI re-inference
// between browser steps — the fastest path from a hotfix branch to a
// reviewable PR.        pb
pb(            "open_github_pr",            "Open a GitHub pull request via the browser",            "dev_backend",            "github",            Capability::Write,            "Sign into GitHub, open the compare view for the pending branch, capture the base/head SHAs from the page, fill the PR title and description deterministically (referencing the captured SHAs), attach the patch diff and submit the PR.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "dev_backend", "resource_id": "github", "capability": "write"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "dev_backend", "resource_id": "github", "capability": "write", "login_url": "https://github.com/login", "username_selector": "input[name=\"login\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "wait_for_url": "/login", "captures": [{
"name": "baseSha", "selector": "[data-testid='base-ref']"
}
, {
"name": "headSha", "selector": "[data-testid='head-ref']"
}
], "extra_actions": [{
"tool": "runtime_session_navigate", "args": {
"url": "https://github.com/unitbuilds/app/compare/main...hotfix"
}
}
, {
"tool": "runtime_session_click", "args": {
"selector": "a[href$='/pull/new']"
}
}
], "form_fields": [{
"selector": "input[name=\"pull_request[title]\"]", "value": "fix(hotfix): {{vars.headSha}} over {{vars.baseSha}}"
}
, {
"selector": "textarea[name=\"pull_request[body]\"]", "value": "Automated SOP open. Base={{vars.baseSha}} Head={{vars.headSha}}."
}
], "upload_selector": "input[type=file][name=\"call[commit_uploads]\"]", "upload_files": ["patches/hotfix.diff"], "form_submit_selector": "button[type=\"submit\"][name=\"pull_request\"]", "settle_ms": 2500
}
),                ),            ],        ),        pb(            "post_product_announcement",            "Post a product announcement",            "social_operator",            "social_x",            Capability::Communicate,            "Draft compliant copy and publish it to X via the browser (Legal sign-off required).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "social_operator", "resource_id": "social_x", "capability": "communicate"
}
),                ),                proceed(),                agent("Draft a compliant sub-240-character X post announcing the latest feature. No unsubstantiated claims."),                tool(                    "execute_org_browser_task_for_request",                    serde_json::json!({
"request_id": "{{requestId}}", "role_id": "social_operator", "resource_id": "social_x", "capability": "communicate", "login_url": "https://x.com/i/flow/login", "username_selector": "[name=\"username\"]", "password_selector": "[name=\"password\"]", "submit_selector": "div[data-testid='LoginForm_Login_Button']", "composer_selector": "[data-testid='tweetTextarea_0']", "composer_submit_selector": "div[data-testid='tweetButton']", "settle_ms": 4000, "composer_text": "{{lastOutput}}"
}
),                ),            ],        ),        pb(            "support_email_reply",            "Answer a support email",            "support_agent",            "email",            Capability::Communicate,            "Sign into the webmail surface, read the newest unanswered email (deterministic read), draft a courteous reply (the one genuine language step), then send it via the webmail reply composer — full-page screenshot for the audit trail, no dispatch handoff.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "email", "capability": "communicate"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "email", "capability": "read", "login_url": "https://mail.unitbuilds.com/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "wait_for_selector": "[data-thread][data-unread='true']", "extra_actions": [{
"tool": "runtime_session_click", "args": {
"selector": "[data-thread][data-unread='true']:first-of-type"
}
}
], "read_script": "return document.querySelector('[data-thread][data-unread=\"true\"]')?.innerText?.slice(0, 4000) || ''", "settle_ms": 1500
}
),                ),                proceed(),                agent("Draft a helpful, courteous reply under 200 words to the customer email above. Stay on-brand, no unsubstantiated claims."),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "email", "capability": "communicate", "login_url": "https://mail.unitbuilds.com/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "composer_selector": "textarea[name=\"reply\"]", "composer_submit_selector": "button[aria-label=\"Send reply\"]", "composer_text": "{{lastOutput}}", "screenshot_full_page": true, "settle_ms": 2000
}
),                ),            ],        ),        pb(            "rotate_infra_secrets",            "Rotate infrastructure secrets",            "ops_lead",            "cloud",            Capability::Deploy,            "Rotate the cloud credential pointer through the vault gate (dual-control CEO+CTO).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "ops_lead", "resource_id": "cloud", "capability": "deploy"
}
),                ),                proceed(),                tool(                    "rotate_org_resource_credential",                    serde_json::json!({
"role_id": "ops_lead", "resource_id": "cloud", "new_secret_ref": "UNITBUILDS_CLOUD_TOKEN_V2"
}
),                ),            ],        ),        pb(            "weekly_metrics_report",            "Weekly metrics report",            "analyst",            "analytics",            Capability::Read,            "Compile user-count and revenue deltas and route the summary to Finance.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "analyst", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                agent("Compile this week's user-count and revenue deltas into a one-page report for the CFO."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "analyst", "task": "Deliver the weekly metrics report to Finance."
}
),                ),            ],        ),        pb(            "read_support_chats",            "Triage inbound support chats",            "support_agent",            "chat",            Capability::Read,            "Sign into the live-chat surface and extract the newest unanswered threads for triage (read-only, no outward reply).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "read", "login_url": "https://app.unitbuilds.com/support/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "extra_actions": [{
"tool": "runtime_session_click", "args": {
"selector": ".inbox [data-thread]:first-of-type"
}
}
], "read_script": "return [...document.querySelectorAll('[data-thread]')].map(t => t.innerText).join('\\n---\\n').slice(0, 4000)"
}
),                ),            ],        ),
// Support sweeps the whole open-ticket queue, not just the newest page:
// read the current page, click "Next", read again — a bounded, deterministic
// pagination crawl (no inference, predictable step count = max_pages reads).        pb
pb(            "sweep_open_ticket_queue",            "Sweep the full open-ticket queue page by page",            "support_agent",            "chat",            Capability::Read,            "Sign into the support inbox and crawl the open-ticket list across up to 5 pages, extracting every ticket body for triage (read-only, no outward reply).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "read", "login_url": "https://app.unitbuilds.com/support/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "read_script": "return [...document.querySelectorAll('[data-ticket]')].map(t => t.innerText).join('\\n---\\n').slice(0, 4000)", "next_selector": "button[aria-label=\"Next page\"]", "max_pages": 5, "settle_ms": 1500
}
),                ),            ],        ),
// Structured form-fill: file an internal ticket with a multi-field form in
// ONE gated action. The declarative `form_fields` list (subject / priority
// / description) becomes ordered fill steps, then a submit — no hand-built
// per-field extra_actions, deterministic, and the ticket body can carry a
// {{lastOutput}} from a prior draft step in a larger playbook.
        pb(            "create_support_ticket",            "File a support ticket from a structured form",            "support_agent",            "chat",            Capability::Communicate,            "Sign into the support desk and file a new ticket by filling the subject, priority and description fields, then submitting the form (deterministic multi-field fill).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "communicate"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "communicate", "login_url": "https://app.unitbuilds.com/support/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "captures": [{
"name": "topTicketId", "selector": "[data-ticket][data-priority=\"urgent\"]"
}
], "extra_actions": [{
"tool": "runtime_session_click", "args": {
"selector": "a[href='/support/new']"
}
}
], "form_fields": [{
"selector": "input[name=\"subject\"]", "value": "Escalation from chat triage (ref {{vars.topTicketId}})"
}
, {
"selector": "select[name=\"priority\"]", "value": "high"
}
, {
"selector": "textarea[name=\"description\"]", "value": "Customer reports login failure on staging build; see queue sweep."
}
], "upload_selector": "input[type=file]", "upload_files": ["logs/staging_login_failure.txt"], "form_submit_selector": "button[type=\"submit\"]", "settle_ms": 1500
}
),                ),            ],        ),
// Support closes the inbound loop end-to-end: read the newest open thread
// (deterministic browser read), draft a reply (the one genuine language
// step), then post it through the chat composer — an individual reply is
// Allowed without an approval kick (chat is non-sensitive + Communicate),
// so the customer gets a fast answer while a broadcast still routes Legal.        pb
pb(            "reply_to_support_chat",            "Read and reply to a live support chat",            "support_agent",            "chat",            Capability::Communicate,            "Sign into the live-chat surface, read the newest open thread, draft an on-brand reply and publish it via the chat composer (publish-button click, deterministic settle between actions).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "communicate"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "read", "login_url": "https://app.unitbuilds.com/support/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "extra_actions": [{
"tool": "runtime_session_click", "args": {
"selector": ".inbox [data-thread]:first-of-type"
}
}
], "read_script": "return document.querySelector('[data-thread]:first-of-type')?.innerText || ''"
}
),                ),                proceed(),                agent("Draft a helpful, on-brand reply to the customer's question in the chat thread above, under 120 words."),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "support_agent", "resource_id": "chat", "capability": "communicate", "login_url": "https://app.unitbuilds.com/support/login", "username_selector": "input[name=\"email\"]", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "composer_selector": "[data-testid=\"reply-box\"]", "composer_submit_selector": "button[aria-label=\"Send\"]", "composer_text": "{{lastOutput}}", "settle_ms": 2000
}
),                ),            ],        ),        pb(            "record_feature_demo",            "Record a feature demo for Marketing",            "demo_recorder",            "analytics",            Capability::Read,            "Capture the live product surface into the browser artifact model and hand the recording to Marketing (read-only, secret-free).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "demo_recorder", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "demo_recorder", "resource_id": "analytics", "capability": "read", "login_url": "https://app.unitbuilds.com/features/latest", "read_script": "return document.querySelector('[data-demo-summary]')?.innerText ?? document.title", "capture_demo": true, "screenshot_selector": "[data-demo-stage]"
}
),                ),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "demo_recorder", "task": "Deliver the captured feature demo to the Marketing lead for the showcase."
}
),                ),            ],        ),        pb(            "scout_competitor_features",            "Scout competitor feature signals",            "scout",            "analytics",            Capability::Read,            "Read a public competitor/changelog page (no login, secret-free), summarize the traction signal and route it to the CTO.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "scout", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "scout", "resource_id": "analytics", "capability": "read", "login_url": "https://example.com/competitor/changelog", "read_script": "return [...document.querySelectorAll('[data-feature], h2, h3')].map(h => h.innerText).join('\\n').slice(0, 4000)"
}
),                ),                agent("From the scraped changelog, list the 3 features with the highest likely traction for UnitBuilds CC and a one-line rationale each."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "scout", "task": "Deliver the scouted feature-opportunity brief to the CTO."
}
),                ),            ],        ),
// Multi-URL read crawl: sweep several competitor changelogs in ONE session
// (navigate + read per URL), so scouting N rivals costs one gated read task
// instead of N — deterministic, bounded, secret-free.        pb
pb(            "sweep_competitor_changelogs",            "Sweep several competitor changelogs in one crawl",            "scout",            "analytics",            Capability::Read,            "Visit a fixed list of public competitor changelog pages in one browser session, extract each feature heading, and route the aggregate brief to the CTO (read-only, no login, secret-free).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "scout", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "scout", "resource_id": "analytics", "capability": "read", "login_url": "https://example.com/competitor/changelog", "read_script": "return [...document.querySelectorAll('[data-feature], h2, h3')].map(h => h.innerText).join('\\n').slice(0, 4000)", "read_urls": ["https://example.com/competitor/changelog", "https://example.com/rival/changelog", "https://example.com/other/releases"]
}
),                ),                agent("Across the crawled changelogs, rank the recurring feature themes by likely traction for UnitBuilds CC and give a one-line rationale each."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "scout", "task": "Deliver the aggregate competitor-feature brief to the CTO."
}
),                ),            ],        ),
// A financial action with two-party control. banking Spend is `sensitive`
// and, at/above the 1,000 threshold, BOTH the CFO and CEO gates fire, so
// the request stalls at AwaitingApproval until two distinct approvers
// clear it — a deterministic human-in-the-loop, not an inference call.
// After clearance the transfer itself is driven through the request-gated
// browser executor (which additionally demands a JIT elevation, Phase 62b)
// and the audit receipt dispatch fires ONLY if the executor's own output
// verdicts an executed-and-verified run: DENIED / NEEDS APPROVAL /
// MISSING / PREVIEW / VERIFY: FAIL all halt the SOP before any record.
        Playbook {
            amount: 1_000.0,            ..pb(                "pay_vendor_invoice",                "Pay a vendor invoice (dual control)",                "cfo",                "banking",                Capability::Spend,                "Payments at/above 1,000 need both CFO and CEO; the transfer then runs only with an active JIT elevation, and the verdict gate halts before the audit receipt unless the execution itself verified.",                vec![                    tool(                        "verify_org_credential",                        serde_json::json!({
"role_id": "cfo", "resource_id": "banking", "capability": "spend"
}
),                    ),                    proceed(),                    agent("Summarize the vendor invoice, the service received, and the payment justification for the audit record."),                    tool(                        "execute_org_browser_task_for_request",                        serde_json::json!({
"request_id": "{{requestId}}", "role_id": "cfo", "resource_id": "banking", "capability": "spend", "amount": 1000.0, "login_url": "https://bank.unitbuilds.com/login", "username_selector": "input[name=\"user\"]", "password_selector": "input[name=\"pass\"]", "submit_selector": "button[type=\"submit\"]", "form_fields": [{
"selector": "[name=payee]", "value": "Acme Hosting Ltd"
}
, {
"selector": "[name=amount]", "value": "1000.00"
}
], "form_submit_selector": "button[data-testid=send-wire]", "read_script": "return document.querySelector('[data-receipt]')?.innerText?.slice(0, 2000) || ''", "verify_expect": "Confirmation number", "verify_absent": "Insufficient funds", "settle_ms": 3000
}
),                    ),
// Spend-path verdict gate: require the executor to state an
// actual run with a verified receipt, and forbid every
// blocked/preview/fail verdict — so a payment that never
// happened can never be recorded as if it did.                    verdict_gate
verdict_gate(                        &["Browser run for session", "VERIFY: PASS"],                        &[                            "DENIED",                            "NEEDS APPROVAL",                            "MISSING",                            "PREVIEW",                            "VERIFY: FAIL",                            "ABSENT: FAIL",                        ],                    ),                    tool(                        "dispatch_org_role",                        serde_json::json!({
"role_id": "cfo", "task": "Payment EXECUTED with a verified receipt (verdict gate cleared): record the provider transaction id from the verified receipt against the vendor invoice in the audit ledger."
}
),                    ),                ],            )
}
,
// Frame/iframe read: many banking surfaces hide the pending-transfer
// confirmation behind a 3-D-Secure iframe. The driver cannot see into
// the frame until it switches context, so this SOP pins the workflow
// on `frame_selector` — the accountant signs in, enters the secure
// frame, and reads the challenge text deterministically (Allowed gate,
// no inference, no dispatch handoff).        pb
pb(            "review_pending_secure_transfer",            "Read a pending 3-D-Secure transfer from the bank portal",            "accountant",            "banking",            Capability::Read,            "Sign into the bank portal, switch the driver into the 3-D-Secure iframe, and read the pending transfer's challenge text — deterministic frame-scoped extraction, no AI re-inference.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "accountant", "resource_id": "banking", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "accountant", "resource_id": "banking", "capability": "read", "login_url": "https://bank.unitbuilds.com/login", "username_selector": "input[name=\"user\"]", "password_selector": "input[name=\"pass\"]", "submit_selector": "button[type=\"submit\"]", "frame_selector": "iframe[name=\"three-d-secure\"]", "dialog_action": "accept", "wait_for_selector": "[data-challenge]", "read_script": "return document.querySelector('[data-challenge]')?.innerText?.slice(0, 2000) || ''", "settle_ms": 1500
}
),                ),            ],        ),
// Legal drives the browser to read a campaign's public surface and gate
// publishing on a compliance verdict — a deterministic review step, not an
// inference call, so every outward marketing campaign is law-checked first.        pb
pb(            "review_campaign_compliance",            "Review a campaign for legal compliance",            "compliance",            "social_x",            Capability::Read,            "Read the scheduled campaign posts from the public surface (no login, secret-free), assess them against advertising law, and route the verdict to General Counsel before publishing.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "compliance", "resource_id": "social_x", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "compliance", "resource_id": "social_x", "capability": "read", "login_url": "https://x.com/UnitBuilds", "read_script": "return [...document.querySelectorAll('article')].map(a => a.innerText).join('\\n---\\n').slice(0, 4000)"
}
),                ),                agent("Assess the scraped campaign/marketing copy for legal risk: unsubstantiated claims, missing disclosures, IP or trademark issues. List each flag with a one-line fix."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "compliance", "task": "Deliver the compliance verdict to General Counsel before the campaign publishes."
}
),                ),            ],        ),
// Management orchestrates the whole company by reading the live cross-team
// status board (secret-free browser read) and dispatching reallocation
// directives — keeping every team in line without hand-holding.        pb
pb(            "sync_team_capacity",            "Orchestrate cross-team capacity",            "coo",            "analytics",            Capability::Read,            "Read the live team status board (secret-free), identify departments that are blocked or over-loaded, and dispatch reallocation directives to the affected leads.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "coo", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "coo", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/teams", "read_script": "return [...document.querySelectorAll('[data-team]')].map(t => t.innerText).join('\\n---\\n').slice(0, 4000)"
}
),                ),                agent("From the team status board, flag which departments are blocked or over-loaded and recommend where to reallocate capacity this cycle."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "coo", "task": "Send the capacity reallocation directives to the affected department leads."
}
),                ),            ],        ),
// Operations owns uptime monitoring: it reads the public status/uptime
// surface over the browser (secret-free, Read-only so no approval kicks),
// then routes any incident to the Reporting Analyst for the Finance feed.        pb
pb(            "check_service_uptime",            "Check service uptime and open incidents",            "sysadmin",            "cloud",            Capability::Read,            "Read the live status/uptime board (no login, secret-free), summarize degradations or incidents, and hand the report to the Operations Analyst for revenue-impact tracking.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "sysadmin", "resource_id": "cloud", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "sysadmin", "resource_id": "cloud", "capability": "read", "login_url": "https://status.unitbuilds.com", "read_script": "return [...document.querySelectorAll('[data-component], [data-incident]')].map(e => e.innerText).join('\\n---\\n').slice(0, 4000)"
}
),                ),                agent("From the status board, list any degraded services, ongoing incidents, and the 24h uptime percentage; flag anything needing action."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "sysadmin", "task": "Deliver the uptime snapshot to the Reporting Analyst for the Finance revenue-impact report."
}
),                ),            ],        ),
// Technical direction closes the loop the objective calls for: the CTO
// reads the scouted feature/roadmap surface over the browser (secret-free,
// Read-only), ranks candidates and dispatches the chosen scope to
// Engineering — turning scouting signals into concrete dev work.        pb
pb(            "plan_feature_roadmap",            "Set the feature roadmap for Engineering",            "cto",            "analytics",            Capability::Read,            "Read the scouted feature-request and traction board (no login, secret-free), rank candidates by traction vs cost, and dispatch the next sprint's scope to the Engineering Lead.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "cto", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "cto", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/roadmap", "read_script": "return [...document.querySelectorAll('[data-feature]')].map(f => f.innerText).join('\\n---\\n').slice(0, 4000)"
}
),                ),                agent("Rank the top 5 candidate features by expected traction versus engineering cost and pick the next sprint's scope."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "cto", "task": "Open backend/frontend/platform tickets for the chosen scope and assign them to the Engineering Lead."
}
),                ),            ],        ),
// The Testing team's core loop: verify a shipped patch actually works by
// reading the public staging surface over the browser and asserting, with
// a deterministic verify_expect containment check (NO inference), that the
// expected build rendered. A pass records a demo for Marketing; a fail
// routes the regression back to Engineering.        pb
pb(            "verify_staging_build",            "Verify a staging build rendered the shipped patch",            "qa_engineer",            "analytics",            Capability::Read,            "Read the public staging build surface (no login, secret-free), deterministically assert the expected version marker is present (verify_expect), record a passing demo, or escalate a failed verification to the QA Lead.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "analytics", "capability": "read", "login_url": "https://staging.unitbuilds.com", "read_script": "return document.querySelector('[data-build-version]')?.innerText || document.title", "verify_expect": "build: green", "verify_absent": "Internal Server Error", "screenshot_selector": "[data-build-version]", "settle_ms": 2500
}
),                ),
// Verdict gate: the acceptance dispatch runs only if the browser
// run's own output proves VERIFY + ABSENT both PASSED. A failed
// verification halts the SOP here — no demo is recorded, nothing
// is marked verified on a page that didn't actually render.                verdict_gate_recheck
verdict_gate_recheck(                    &["VERIFY: PASS", "ABSENT: PASS"],                    &["VERIFY: FAIL", "ABSENT: FAIL"],                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "analytics", "capability": "read", "login_url": "https://staging.unitbuilds.com", "read_script": "return document.querySelector('[data-build-version]')?.innerText || document.title", "verify_expect": "build: green", "verify_absent": "Internal Server Error", "settle_ms": 2500
}
),                    2,                    2500,                ),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "qa_engineer", "task": "Verification PASSED (verdict gate cleared): record the acceptance demo for Marketing and mark the patch verified."
}
),                ),            ],        ),
// Phase 57: QA downloads a verified staging build artifact from the CI
// portal (cloud Read) — full browser flow: login, read, verify, screenshot,
// then download. No inference, deterministic end-to-end.        pb
pb(            "download_staging_artifact",            "Download verified staging build artifact",            "qa_engineer",            "cloud",            Capability::Read,            "Sign into CI, verify the build marker, capture evidence, then download the zip artifact for local testing.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "cloud", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "cloud", "capability": "read", "login_url": "https://ci.unitbuilds.com/builds/staging", "username_selector": "input[name=user]", "password_selector": "input[name=pass]", "submit_selector": "button.login", "read_script": "return document.querySelector('[data-artifact]')?.textContent", "verify_expect": "velocity", "screenshot_full_page": true, "download_selector": "a[data-download=zip]", "download_dir": "artifacts/", "download_timeout_ms": 30000, "settle_ms": 2000
}
),                ),            ],        ),
// Operations closes the loop the objective names — user-count and revenue
// reporting to Finance — as a deterministic browser read of the analytics
// surface (no login, secret-free, Read-only so no approval kicks) that is
// then handed to the CFO for margin review. The analyst narrows to the
// MRR/churn panels with extra_actions before reading, proving the generic
// click-then-read sequence end to end.        pb
pb(            "report_revenue_to_finance",            "Report user count and revenue to Finance",            "analyst",            "analytics",            Capability::Read,            "Read the live user-count and revenue (MRR/churn) dashboards over the browser (no login, secret-free), then deliver the snapshot to the CFO so Finance can keep margins healthy without guessing.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "analyst", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "analyst", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/revenue", "extra_actions": [{
"tool": "runtime_session_click", "args": {
"selector": "[data-tab='mrr']"
}
}
], "wait_for_selector": "[data-panel='loaded']", "scroll_to": "[data-metric='mrr']", "read_script": "return [...document.querySelectorAll('[data-metric]')].map(m => m.innerText).join('\\n---\\n').slice(0, 4000)", "settle_ms": 2000
}
),                ),                agent("From the revenue and user-count snapshot, compute MRR, net user growth and churn for the period and flag any margin risk."),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "analyst", "task": "Deliver the user-count and revenue snapshot to the CFO for the margin review."
}
),                ),            ],        ),
// Marketing closes its own loop deterministically: after a campaign posts,
// the operator reads back the live public profile (no login, secret-free,
// Read-only so no approval kick) and pins the campaign marker with
// verify_expect while verify_absent guards against an error/rate-limit
// page — proving the outward post actually landed before reporting it.        pb
pb(            "confirm_announcement_landed",            "Confirm a published announcement is live",            "social_operator",            "social_x",            Capability::Read,            "Read back the live public profile after a campaign posts (no login, secret-free), deterministically confirm the announcement marker is present and no error/rate-limit page appeared, then report the verified result to the Marketing Lead.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "social_operator", "resource_id": "social_x", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "social_operator", "resource_id": "social_x", "capability": "read", "login_url": "https://x.com/UnitBuilds", "read_script": "return [...document.querySelectorAll('article')].map(a => a.innerText).join('\\n---\\n').slice(0, 4000)", "verify_expect": "#UnitBuilds", "verify_absent": "Something went wrong", "extract_attribute_selector": "article a[href*='/status/']", "extract_attribute_name": "href", "settle_ms": 3000
}
),                ),
// Verdict gate: "report the verified post" fires only when the
// read-back's own output carries VERIFY: PASS + ABSENT: PASS. If
// the campaign didn't land (or an error page showed), the SOP
// halts before any outward report is dispatched.                verdict_gate_recheck
verdict_gate_recheck(                    &["VERIFY: PASS", "ABSENT: PASS"],                    &["VERIFY: FAIL", "ABSENT: FAIL"],                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "social_operator", "resource_id": "social_x", "capability": "read", "login_url": "https://x.com/UnitBuilds", "read_script": "return [...document.querySelectorAll('article')].map(a => a.innerText).join('\\n---\\n').slice(0, 4000)", "verify_expect": "#UnitBuilds", "verify_absent": "Something went wrong", "settle_ms": 3000
}
),                    2,                    3000,                ),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "social_operator", "task": "Report the verified post to the Marketing Lead — the verdict gate confirmed VERIFY: PASS + ABSENT: PASS, so the campaign is live."
}
),                ),            ],        ),
// Engineering sprint board management: the dev_lead opens the GitHub
// Projects board, clicks a card that opens in a new tab, switches to it,
// drags the card from Backlog to In Progress — fully deterministic, no
// inference, one gated Write action on the github resource.        pb
pb(            "move_sprint_card",            "Move a kanban card on the sprint board",            "dev_lead",            "github",            Capability::Write,            "Open the sprint board, click a card to focus it in a new tab, switch to it, then drag the card from Backlog to In Progress (deterministic project management).",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "dev_lead", "resource_id": "github", "capability": "write"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "dev_lead", "resource_id": "github", "capability": "write", "login_url": "https://github.com/login", "username_selector": "input[name=\"login\"]", "username": "bot@unitbuilds.com", "password_selector": "input[name=\"password\"]", "submit_selector": "button[type=\"submit\"]", "extra_actions": [{
"tool": "runtime_session_navigate", "args": {
"url": "https://github.com/orgs/unitbuilds/projects/1"
}
}
], "wait_for_selector": "[data-board]", "switch_tab": false, "drag_from": "[data-card='sprint-next']", "drag_to": "[data-column='in-progress']", "settle_ms": 2000
}
),                ),            ],        ),
// Phase 59: Operations exports the user-count/revenue report as a CSV from
// the tenant-scoped analytics dashboard. This is the first SOP to exercise
// the three newest browser verbs together — request_headers (a tenant id so
// the export endpoint authorizes the org without embedding a secret in the
// plan), wait_for_network_idle (the async export finishes rendering before
// we read), then a bounded download of the generated artifact. Fully
// deterministic: read + verify_expect gate the download trigger.        pb
pb(            "export_analytics_report",            "Export the analytics report for Finance",            "ops_lead",            "analytics",            Capability::Read,            "Open the tenant-scoped analytics export with the org request headers, wait for the export network activity to settle, verify the report marker is present, then download the CSV artifact into reports/ for the Finance review.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "ops_lead", "resource_id": "analytics", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "ops_lead", "resource_id": "analytics", "capability": "read", "login_url": "https://analytics.unitbuilds.com/export", "request_headers": [{
"name": "X-Tenant-ID", "value": "unitbuilds-cc"
}
], "wait_for_network_idle": true, "read_script": "return document.querySelector('[data-export-state]')?.textContent", "verify_expect": "ready", "download_selector": "a[data-download=csv]", "download_dir": "reports/", "download_timeout_ms": 45000, "settle_ms": 6000
}
),                ),            ],        ),
// Phase 59: Finance reads the bank/ledger audit trail over the browser to
// keep margins honest without guessing. Same tenant-header + network-idle
// discipline as Operations' export, but read-only evidence capture (a full
// page screenshot) rather than a download — the accountant verifies the
// reconciliation marker is present before the snapshot is handed to the CFO.        pb
pb(            "review_finance_audit_trail",            "Review the ledger audit trail",            "accountant",            "banking",            Capability::Read,            "Open the tenant-scoped ledger audit trail with the org request headers, wait for the reconciliation network activity to settle, verify the ledger is balanced, then capture a full-page screenshot as evidence for the CFO margin review.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "accountant", "resource_id": "banking", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "accountant", "resource_id": "banking", "capability": "read", "login_url": "https://bank.unitbuilds.com/audit-trail", "request_headers": [{
"name": "X-Tenant-ID", "value": "unitbuilds-cc"
}
], "wait_for_network_idle": true, "read_script": "return document.querySelector('[data-reconciliation]')?.textContent", "verify_expect": "balanced", "screenshot_full_page": true, "settle_ms": 5000
}
),                ),
// Verdict gate: the CFO handoff fires only if the ledger read
// actually verified as balanced. An unbalanced/unreadable trail
// halts the SOP — Finance never receives a snapshot that lied.                verdict_gate
verdict_gate(&["VERIFY: PASS"], &["VERIFY: FAIL"]),                tool(                    "dispatch_org_role",                    serde_json::json!({
"role_id": "accountant", "task": "Ledger verified balanced with full-page evidence captured: deliver the audit-trail snapshot to the CFO for the margin review."
}
),                ),            ],        ),
// Phase 60: Operations reaches a token-auth internal service dashboard by
// bootstrapping the session with an SSO cookie + localStorage token read
// from the vault — no password form is driven, so the whole run stays
// secret-free (the plan references the secret only by vault pointer at run
// time). This is how the org manages systems that use cookie/session auth
// (status pages, admin consoles) rather than username/password logins.        pb
pb(            "check_internal_service_status",            "Check the internal service status dashboard",            "ops_lead",            "cloud",            Capability::Read,            "Bootstrap the session with an SSO cookie and a localStorage auth token (both vault pointers, no password form), open the internal status dashboard, wait for it to settle, and verify all services report healthy.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "ops_lead", "resource_id": "cloud", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "ops_lead", "resource_id": "cloud", "capability": "read", "login_url": "https://status.unitbuilds.internal/services", "cookies": [{
"name": "sso_session", "value": "{{vault:CLOUD_SSO_COOKIE}}"
}
], "local_storage": [{
"key": "auth.token", "value": "{{vault:CLOUD_AUTH_TOKEN}}"
}
], "wait_for_network_idle": true, "read_script": "return [...document.querySelectorAll('[data-service-state]')].map(s => s.textContent).join('\\n').slice(0, 4000)", "verify_expect": "healthy", "settle_ms": 4000
}
),                ),            ],        ),
// Phase 61: QA exercises the staging site as a mobile device — HTTP
// basic-auth to reach the protected staging realm, an iPhone user-agent and
// a 390x844 viewport so the responsive layout actually re-renders, then a
// deterministic verify that the mobile nav is present and a full-page
// screenshot as the visual regression artifact for the marketing demo team.        pb
pb(            "test_mobile_responsive_layout",            "Test the mobile responsive layout on staging",            "qa_engineer",            "cloud",            Capability::Read,            "Authenticate to the basic-auth staging realm, emulate an iPhone user-agent and 390x844 viewport, load the app, verify the mobile navigation is present, and capture a full-page screenshot as the responsive-regression evidence.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "cloud", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "qa_engineer", "resource_id": "cloud", "capability": "read", "login_url": "https://staging.unitbuilds.com/app", "basic_auth_user": "staging-bot", "basic_auth_pass": "{{vault:STAGING_BASIC_PASS}}", "user_agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)", "viewport_width": 390, "viewport_height": 844, "wait_for_network_idle": true, "read_script": "return document.querySelector('[data-mobile-nav]')?.textContent", "verify_expect": "menu", "screenshot_full_page": true, "settle_ms": 3000
}
),                ),            ],        ),
// Phase 73: deterministic fan-out. The CFO reads the pending-invoices
// queue once (a `Read` intent — auto-approved against CFO's banking
// grant), then the ForEach body proposes one *individually-gated*
// Spend action per extracted invoice, carrying the item's `vendor`
// and `amount` fields into the request's description. N invoices →
// N gated actions with N audit receipts, all reached without a
// single AI inference — the objective's "deterministic over        // inference" property, now realized over a whole work-queue.        pb
pb(            "queue_invoice_payments",            "Queue one gated payment per pending invoice",            "cfo",            "banking",            Capability::Read,            "Read the bank's pending-invoice queue over the browser (no inference), then submit a dual-control Spend request per invoice with the vendor and amount bound from the extraction — one approval per payment, no batch-wide override.",            vec![                tool(                    "verify_org_credential",                    serde_json::json!({
"role_id": "cfo", "resource_id": "banking", "capability": "read"
}
),                ),                proceed(),                tool(                    "execute_org_browser_task",                    serde_json::json!({
"role_id": "cfo", "resource_id": "banking", "capability": "read", "login_url": "https://bank.unitbuilds.com/invoices/pending", "username_selector": "input[name=\"user\"]", "password_selector": "input[name=\"pass\"]", "submit_selector": "button[type=\"submit\"]", "wait_for_network_idle": true, "read_script": "return JSON.stringify([...document.querySelectorAll('[data-invoice]')].map(el => ({vendor: el.getAttribute('data-vendor'), amount: el.getAttribute('data-amount'), ref: el.getAttribute('data-ref')})))", "verify_expect": "[", "settle_ms": 3000
}
),                ),                verdict_gate(&["["], &["VERIFY: FAIL"]),                WorkflowStep::ForEach {
                    source: "lastOutput".to_string(),                    as_var: "invoice".to_string(),                    body: vec![tool(                        "request_org_action",                        serde_json::json!({
                            "role_id": "cfo",                            "resource_id": "banking",                            "capability": "spend",                            "amount": "{{invoice.amount}}",                            "description": "Pay {{invoice.vendor}} for ref {{invoice.ref}} (\u{2014} {{invoice.amount}})"
}
),                    )],
}
,            ],        ),    ];
    apply_preset_chains(&mut list);
    list
}
/// Ensure a workspace has an org: load the stored one, or seed the UnitBuilds CC
/// preset and persist it so subsequent edits are durable.
pub fn ensure_default_org(workspace_root: &Path) -> CompanyOrg {
    if let Some(org) = load_company_org(workspace_root) {
        return org;
    }
    let org = default_company_org();
    save_company_org(workspace_root, &org);
    org
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Action-request / approval queue (deterministic lifecycle)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// Lifecycle state of a proposed action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionStatus {
    /// Waiting on one or more approvals.
    Pending,
    /// Cleared to execute (either auto-allowed or quorum reached).
    Approved,
    /// Denied outright or rejected by an approver.
    Rejected,
    /// Carried out.
    Executed,
}
/// A recorded intent to perform a capability action, with its approval trail.
/// Persisted so the pipeline survives restarts and can be audited.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub id: String,
    pub role_id: String,
    pub resource_id: String,
    pub capability: Capability,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub description: String,
    pub status: ActionStatus,
    #[serde(default)]
    pub required_approvers: Vec<String>,
    #[serde(default)]
    pub quorum: usize,
    /// Ids (role ids or the human operator label) that have approved.
    #[serde(default)]
    pub approvals: Vec<String>,
    #[serde(default)]
    pub decision_reason: String,
    #[serde(default)]
    pub created_at: u64,
    /// The playbook that raised this intent, when it came from one. The
    /// (role, resource, capability) triple alone cannot identify a SOP — two
    /// playbooks may legitimately share one gate — so an approved request needs
    /// this to resume the very SOP that queued it.
    #[serde(default)]
    pub playbook_id: Option<String>,
}
const REQUESTS_LABEL: &[u8] = b"org_approvals";
/// The synthetic identity that stands in for the human operator when a policy
/// has no named approver roles (deny-to-human default).
pub const HUMAN_OPERATOR: &str = "human-operator";
fn requests_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".velocity").join("org_approvals.nda")
}
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn gen_request_id(org: &CompanyOrg, role_id: &str, resource_id: &str) -> String {
    // Deterministic-ish unique id from time + counts; collisions are impossible
    // in practice because the counter grows with each stored request.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!(
        "req_{}_{:x}{:x}",
        org.id.replace('-', "_").trim_matches('_'),
        now_secs(),
        nanos ^ (role_id.len() * 31 + resource_id.len()) as u32
    )
}
/// Load all recorded action requests (oldest→newest by created_at).
pub fn load_requests(workspace_root: &Path) -> Vec<ActionRequest> {
    let path = requests_path(workspace_root);
    if let Ok(bytes) = fs::read(&path) {
        let plain = crate::agent::crypto::open(workspace_root, REQUESTS_LABEL, &bytes);
        let text = String::from_utf8_lossy(&plain);
        if let Ok(list) = serde_json::from_str::<Vec<ActionRequest>>(&text) {
            return list;
        }
    }
    Vec::new()
}
/// Persist the full request ledger, sealed.
pub fn save_requests(workspace_root: &Path, requests: &[ActionRequest]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(requests) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, REQUESTS_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(requests_path(workspace_root), bytes).is_ok()
}
/// Propose an action. The permission decision is computed deterministically:
/// - `Allowed`    -> the request is stored `Approved` (ready to run).
/// - `RequiresApproval` -> stored `Pending` with the required approvers/quorum.
/// - `Denied`     -> stored `Rejected`; caller should not proceed.
///
/// Returns the stored request. The caller executes only when `status`
/// transitions to [`ActionStatus::Approved`].
pub fn submit_action(
    workspace_root: &Path,
    org: &CompanyOrg,
    role_id: &str,
    resource_id: &str,
    capability: Capability,
    amount: f64,
    description: &str,
) -> ActionRequest {
    submit_action_for_playbook(
        workspace_root,
        org,
        role_id,
        resource_id,
        capability,
        amount,
        description,
        None,
    )
}
/// As [`submit_action`], but stamps the request with the playbook that raised
/// it, so an approval resumes exactly that SOP rather than whatever else shares
/// the same gate.
pub fn submit_action_for_playbook(
    workspace_root: &Path,
    org: &CompanyOrg,
    role_id: &str,
    resource_id: &str,
    capability: Capability,
    amount: f64,
    description: &str,
    playbook_id: Option<&str>,
) -> ActionRequest {
    let decision = check_permission(org, role_id, resource_id, capability, amount);
    let (status, required_approvers, quorum, reason) = match decision {
        Decision::Allowed => (
            ActionStatus::Approved,
            Vec::new(),
            0,
            "allowed by policy".to_string(),
        ),
        Decision::Denied { reason } => (ActionStatus::Rejected, Vec::new(), 0, reason),
        Decision::RequiresApproval {
            approvers,
            quorum,
            reason,
            ..
        } => {
            // An empty approver list means the human operator must sign off.
            let reqs = if approvers.is_empty() {
                vec![HUMAN_OPERATOR.to_string()]
            } else {
                approvers
            };
            (ActionStatus::Pending, reqs, quorum, reason)
        }
    };
    let request = ActionRequest {
        id: gen_request_id(org, role_id, resource_id),
        role_id: role_id.to_string(),
        resource_id: resource_id.to_string(),
        capability,
        amount,
        description: description.to_string(),
        status,
        required_approvers,
        quorum,
        approvals: Vec::new(),
        decision_reason: reason,
        created_at: now_secs(),
        playbook_id: playbook_id.map(|s| s.to_string()),
    };
    let mut all = load_requests(workspace_root);
    all.push(request.clone());
    save_requests(workspace_root, &all);
    request
}
/// Record an approval from `approver_id` on `request_id`, advancing it to
/// `Approved` once quorum is reached. Errors if the approver is not designated,
/// lacks Approve authority on the resource, or the request is not pending.
pub fn approve_action(
    workspace_root: &Path,
    org: &CompanyOrg,
    request_id: &str,
    approver_id: &str,
) -> Result<ActionRequest, String> {
    let mut all = load_requests(workspace_root);
    let req = all
        .iter_mut()
        .find(|r| r.id == request_id)
        .ok_or_else(|| format!("request '{request_id}' not found"))?;
    if req.status != ActionStatus::Pending {
        return Err(format!(
            "request '{request_id}' is {:?}, not pending",
            req.status
        ));
    }
    if !req.required_approvers.contains(&approver_id.to_string()) {
        return Err(format!(
            "'{approver_id}' is not a designated approver for '{request_id}'"
        ));
    }

    // A role-based approver must actually hold Approve on the resource; the
    // human operator label is always permitted.
    if approver_id != HUMAN_OPERATOR
        && org.find_role(approver_id).is_some()
        && !org.grants.iter().any(|g| {
            g.role_id == approver_id
                && g.resource_id == req.resource_id
                && g.capabilities.contains(&Capability::Approve)
        })
    {
        return Err(format!(
            "approver '{approver_id}' lacks Approve authority on '{}'",
            req.resource_id
        ));
    }
    if !req.approvals.contains(&approver_id.to_string()) {
        req.approvals.push(approver_id.to_string());
    }
    let needed = req.quorum.max(1);
    if req.approvals.len() >= needed {
        req.status = ActionStatus::Approved;
        req.decision_reason = format!("quorum reached ({}/{})", req.approvals.len(), needed);
    }
    let out = req.clone();
    save_requests(workspace_root, &all);
    Ok(out)
}
/// Reject a pending request outright.
pub fn reject_action(
    workspace_root: &Path,
    request_id: &str,
    by: &str,
    reason: &str,
) -> Result<ActionRequest, String> {
    let mut all = load_requests(workspace_root);
    let req = all
        .iter_mut()
        .find(|r| r.id == request_id)
        .ok_or_else(|| format!("request '{request_id}' not found"))?;
    if req.status != ActionStatus::Pending {
        return Err(format!(
            "request '{request_id}' is {:?}, not pending",
            req.status
        ));
    }
    req.status = ActionStatus::Rejected;
    req.decision_reason = format!("rejected by {by}: {reason}");
    let out = req.clone();
    save_requests(workspace_root, &all);
    Ok(out)
}
/// Mark an approved request as executed (audit trail close-out).
pub fn mark_executed(workspace_root: &Path, request_id: &str) -> Result<ActionRequest, String> {
    let mut all = load_requests(workspace_root);
    let req = all
        .iter_mut()
        .find(|r| r.id == request_id)
        .ok_or_else(|| format!("request '{request_id}' not found"))?;
    if req.status != ActionStatus::Approved {
        return Err(format!(
            "request '{request_id}' is {:?}, not approved",
            req.status
        ));
    }
    req.status = ActionStatus::Executed;
    let out = req.clone();
    save_requests(workspace_root, &all);
    Ok(out)
}
/// Deterministic escalation routing: the pending actions a given approver is
/// designated to clear, in submission order. This is the primitive that turns
/// "notify the right human/manager when a gate needs a decision" into a plain
/// ledger query - no inference, no polling loop. A request is routed to
/// `approver_id` iff it is [`ActionStatus::Pending`] *and* `approver_id` is in
/// its `required_approvers` (so an approver never sees - and cannot act on -
/// items outside their designation). Pass [`HUMAN_OPERATOR`] to fetch the
/// operator's queue.
pub fn pending_for_approver(workspace_root: &Path, approver_id: &str) -> Vec<ActionRequest> {
    load_requests(workspace_root)
        .into_iter()
        .filter(|a| a.status == ActionStatus::Pending)
        .filter(|a| a.required_approvers.iter().any(|r| r == approver_id))
        .collect()
}
/// The duty whose clean run queued this one, when the request came from a
/// chain hop: the playbook declaring `on_success_handoff == pb_id`. A plain
/// edge lookup over stored declarations — no inference.
fn upstream_of<'a>(org: &'a CompanyOrg, pb_id: &str) -> Option<&'a Playbook> {
    org.playbooks
        .iter()
        .find(|p| p.on_success_handoff.as_deref() == Some(pb_id))
}
/// Secret-free chain context for one pending request: what a signature actually
/// runs, whether the request arrived as part of a chain, and what waits on the
/// far side of the gate. An approver of an irreversible action should see the
/// shape of the work, not only the (role, capability, resource) triple — that
/// difference is what makes "approve the post" a decision instead of a guess.
pub fn escalation_context(org: &CompanyOrg, req: &ActionRequest) -> String {
    match &req.playbook_id {
        // Raised directly (a browser intent, a one-off request): approving it is
        // a decision about that single action, with no script behind it.
        None => "manual action \u{2014} no stored steps to run".to_string(),
        Some(id) => match playbook_for_request(org, req) {
            None => format!("playbook '{id}' no longer exists \u{2014} nothing to run"),
            Some(pb) => {
                let hop = match upstream_of(org, id) {
                    Some(up) => format!(" (hop from '{}')", up.id),
                    None => String::new(),
                };
                let next = match &pb.on_success_handoff {
                    Some(t) if org.playbooks.iter().any(|x| x.id == *t) => {
                        format!(" \u{2192} then '{t}' awaits its own gate")
                    }
                    Some(t) => format!(" \u{2192} then '{t}' (DANGLING)"),
                    None => String::new(),
                };
                format!("signing runs '{id}'{hop}{next}")
            }
        },
    }
}
/// Escalation summary for the whole org: one line per pending request grouped
/// under each designated approver that still owes a decision (i.e. has not yet
/// approved). Deterministic and secret-free, so it can surface in the GUI queue
/// or an MCP response to show exactly who must act on what.
pub fn escalation_roster(workspace_root: &Path) -> String {
    let org = load_company_org(workspace_root);
    let pending: Vec<ActionRequest> = load_requests(workspace_root)
        .into_iter()
        .filter(|a| a.status == ActionStatus::Pending)
        .collect();
    if pending.is_empty() {
        return "No pending escalations: every gated action is cleared or denied.".to_string();
    }

    // Collect the set of approvers who still owe a decision, preserving order.
    let mut owed: Vec<String> = Vec::new();
    for a in &pending {
        for r in &a.required_approvers {
            if !a.approvals.contains(r) && !owed.contains(r) {
                owed.push(r.clone());
            }
        }
    }
    let mut out = format!("{} pending escalation(s).\n", pending.len());
    for who in &owed {
        let mine = pending_for_approver(workspace_root, who);
        out.push_str(&format!(
            "  {who}: {} action(s) await your decision\n",
            mine.len()
        ));
        for a in &mine {
            let amt = if a.amount > 0.0 {
                format!(" amount={:.0}", a.amount)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "    {} {} -[{}]-> {}{} (need {}/{})\n",
                a.id,
                a.role_id,
                a.capability.as_str(),
                a.resource_id,
                amt,
                a.approvals.len(),
                a.quorum.max(1),
            ));

            // The chain context rides under the request, so whoever is named
            // above can see what their signature is really for.
            if let Some(o) = org.as_ref() {
                out.push_str(&format!("        \u{2514} {}\n", escalation_context(o, a)));
            }
        }
    }
    if owed.is_empty() {
        out.push_str("  (awaiting co-approver quorum)\n");
    }
    out
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Just-in-time credential elevation (TTL-bounded) + secret-free usage audit
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
//
// The static permission matrix grants *standing* access (Read surfaces, low-value
// Communicate). High-value, irreversible actions (banking Spend above the
// dual-control threshold, a cloud Deploy, a credential-vault rotation) must NOT
// rely on a standing grant an agent could reuse forever. Instead: a request is
// approved through the quorum pipeline, then a *time-bounded* elevation is minted
// scoped to exactly that approved request's (role, resource, capability). It
// auto-expires on a TTL and is single-use, and every transition is written to an
// append-only audit ledger that never carries a secret — only who/what/when
/// outcome. This is the deterministic replacement for "the agent decided it was// allowed": elevation is a data fact with a clock, queryable and revocable.
/// A single-use, TTL-bounded elevation minted from an approved request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevationGrant {
    pub id: String,
    pub role_id: String,
    pub resource_id: String,
    pub capability: Capability,
    /// The approved action-request this elevation was minted from.
    pub request_id: String,
    /// Unix seconds at which the grant was minted.
    pub granted_at: u64,
    /// Unix seconds at which the grant stops being active.
    pub expires_at: u64,
    /// True once consumed — an elevation authorizes exactly one execution.
    #[serde(default)]
    pub consumed: bool,
}
impl ElevationGrant {
    /// Whether the grant is still usable at time `now` (unexpired and unconsumed).
    pub fn is_active_at(&self, now: u64) -> bool {
        !self.consumed && now < self.expires_at
    }
}
/// Outcome of a credential-use decision, recorded in the audit ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    /// An elevation was minted from an approved request.
    Granted,
    /// An active elevation was consumed to authorize one execution.
    Used,
    /// A requested use was refused (no active grant / wrong scope).
    Denied,
    /// A grant was presented after its TTL elapsed and refused.
    Expired,
}
/// One append-only ledger line. Deliberately secret-free: it records *that* a
/// capability was elevated or used, never any credential value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialAudit {
    pub seq: u64,
    pub at: u64,
    pub role_id: String,
    pub resource_id: String,
    pub capability: Capability,
    #[serde(default)]
    pub request_id: String,
    pub outcome: AuditOutcome,
    #[serde(default)]
    pub detail: String,
}
const ELEVATIONS_LABEL: &[u8] = b"org_elevations";
const AUDIT_LABEL: &[u8] = b"org_credential_audit";
fn elevations_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".velocity").join("org_elevations.nda")
}
fn audit_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root
        .join(".velocity")
        .join("org_credential_audit.nda")
}
fn load_elevations(workspace_root: &Path) -> Vec<ElevationGrant> {
    let path = elevations_path(workspace_root);
    if let Ok(bytes) = fs::read(&path) {
        let plain = crate::agent::crypto::open(workspace_root, ELEVATIONS_LABEL, &bytes);
        let text = String::from_utf8_lossy(&plain);
        if let Ok(list) = serde_json::from_str::<Vec<ElevationGrant>>(&text) {
            return list;
        }
    }
    Vec::new()
}
fn save_elevations(workspace_root: &Path, grants: &[ElevationGrant]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(grants) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, ELEVATIONS_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(elevations_path(workspace_root), bytes).is_ok()
}
fn load_audits(workspace_root: &Path) -> Vec<CredentialAudit> {
    let path = audit_path(workspace_root);
    if let Ok(bytes) = fs::read(&path) {
        let plain = crate::agent::crypto::open(workspace_root, AUDIT_LABEL, &bytes);
        let text = String::from_utf8_lossy(&plain);
        if let Ok(list) = serde_json::from_str::<Vec<CredentialAudit>>(&text) {
            return list;
        }
    }
    Vec::new()
}
fn save_audits(workspace_root: &Path, entries: &[CredentialAudit]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(entries) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, AUDIT_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(audit_path(workspace_root), bytes).is_ok()
}
/// Append one secret-free line to the credential-use ledger.
fn push_audit(
    workspace_root: &Path,
    role_id: &str,
    resource_id: &str,
    capability: Capability,
    request_id: &str,
    outcome: AuditOutcome,
    detail: &str,
    at: u64,
) {
    let mut all = load_audits(workspace_root);
    let seq = all.len() as u64 + 1;
    all.push(CredentialAudit {
        seq,
        at,
        role_id: role_id.to_string(),
        resource_id: resource_id.to_string(),
        capability,
        request_id: request_id.to_string(),
        outcome,
        detail: detail.to_string(),
    });
    save_audits(workspace_root, &all);
}
/// Mint a TTL-bounded elevation from an *approved* request. Deterministic:
/// the elevation's scope is copied verbatim from the request (role, resource,
/// capability), so an agent can never mint broader access than what was cleared.
/// Errors if the request is missing or not [`ActionStatus::Approved`]. Records a
/// `Granted` audit line on success and a `Denied` line on refusal.
pub fn grant_jit_elevation(
    workspace_root: &Path,
    request_id: &str,
    ttl_secs: u64,
    now: u64,
) -> Result<ElevationGrant, String> {
    let req = load_requests(workspace_root)
        .into_iter()
        .find(|r| r.id == request_id)
        .ok_or_else(|| format!("request '{request_id}' not found"))?;
    if req.status != ActionStatus::Approved {
        push_audit(
            workspace_root,
            &req.role_id,
            &req.resource_id,
            req.capability,
            request_id,
            AuditOutcome::Denied,
            &format!("elevation refused: request is {:?}", req.status),
            now,
        );
        return Err(format!(
            "request '{request_id}' is {:?}, not Approved — cannot elevate",
            req.status
        ));
    }

    // Clamp the window so a mistyped/oversized TTL can't mint standing access.
    let ttl = ttl_secs.clamp(1, 3600);
    let grant = ElevationGrant {
        id: format!("elev_{request_id}"),
        role_id: req.role_id.clone(),
        resource_id: req.resource_id.clone(),
        capability: req.capability,
        request_id: request_id.to_string(),
        granted_at: now,
        expires_at: now.saturating_add(ttl),
        consumed: false,
    };
    let mut all = load_elevations(workspace_root);
    all.retain(|g| g.id != grant.id);
    all.push(grant.clone());
    save_elevations(workspace_root, &all);
    push_audit(
        workspace_root,
        &grant.role_id,
        &grant.resource_id,
        grant.capability,
        request_id,
        AuditOutcome::Granted,
        &format!("TTL {ttl}s until {}", grant.expires_at),
        now,
    );
    Ok(grant)
}
/// The active (unexpired, unconsumed) elevation for a role's (resource,
/// capability) at time `now`, if any. This is the deterministic gate an elevated
/// action consults — no inference, a plain ledger query.
pub fn find_active_elevation(
    workspace_root: &Path,
    role_id: &str,
    resource_id: &str,
    capability: Capability,
    now: u64,
) -> Option<ElevationGrant> {
    load_elevations(workspace_root).into_iter().find(|g| {
        g.role_id == role_id
            && g.resource_id == resource_id
            && g.capability == capability
            && g.is_active_at(now)
    })
}
/// Consume an active elevation to authorize exactly one execution. Errors (and
/// records `Expired`/`Denied`) if the grant is past its TTL or already spent.
pub fn consume_jit_elevation(
    workspace_root: &Path,
    grant_id: &str,
    now: u64,
) -> Result<ElevationGrant, String> {
    let mut all = load_elevations(workspace_root);
    let g = all
        .iter_mut()
        .find(|g| g.id == grant_id)
        .ok_or_else(|| format!("elevation '{grant_id}' not found"))?;
    if g.consumed {
        return Err(format!("elevation '{grant_id}' already consumed"));
    }
    if now >= g.expires_at {
        push_audit(
            workspace_root,
            &g.role_id,
            &g.resource_id,
            g.capability,
            &g.request_id,
            AuditOutcome::Expired,
            &format!("expired at {}, presented at {now}", g.expires_at),
            now,
        );
        return Err(format!("elevation '{grant_id}' expired"));
    }
    g.consumed = true;
    let out = g.clone();
    save_elevations(workspace_root, &all);
    push_audit(
        workspace_root,
        &out.role_id,
        &out.resource_id,
        out.capability,
        &out.request_id,
        AuditOutcome::Used,
        "single-use elevation consumed",
        now,
    );
    Ok(out)
}
/// The most recent `limit` audit lines (newest last), for GUI/MCP surfacing.
/// Secret-free by construction — the ledger never stores a credential value.
pub fn credential_audit_trail(workspace_root: &Path, limit: usize) -> Vec<CredentialAudit> {
    let all = load_audits(workspace_root);
    let start = all.len().saturating_sub(limit);
    all[start..].to_vec()
}
/// Render the audit trail as a compact, secret-free report.
pub fn describe_credential_audit(entries: &[CredentialAudit]) -> String {
    if entries.is_empty() {
        return "No credential elevations or uses recorded yet.".to_string();
    }
    let mut out = format!(
        "CREDENTIAL AUDIT ({} entr{})\n",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    );
    for e in entries {
        out.push_str(&format!(
            "  #{} t={} {} -[{}]-> {} {:?} req={} {}\n",
            e.seq,
            e.at,
            e.role_id,
            e.capability.as_str(),
            e.resource_id,
            e.outcome,
            if e.request_id.is_empty() {
                "-"
            } else {
                &e.request_id
            },
            e.detail
        ));
    }
    out
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Deterministic, permission-gated playbooks
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// A named, ordered procedure owned by a role. Its steps run through the
/// existing deterministic [`crate::editor::workflow::Workflow`] engine, so
/// repeatable operations cost nothing in inference — LLM calls happen only at
/// explicit [`crate::editor::workflow::WorkflowStep::AgentTask`] nodes.
//
//
// A playbook first submits its `capability` intent to the approval engine; the
/// steps run *only* if that action clears. This makes "no outward action without/// a cleared gate" a structural property, not a prompt instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub id: String,
    pub name: String,
    pub role_id: String,
    pub resource_id: String,
    pub capability: Capability,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<crate::editor::workflow::WorkflowStep>,
    /// Optional cross-SOP handoff: the id of the playbook to queue when this one
    /// finishes **clean** (every step ok). The handoff re-submits the downstream
    /// playbook's intent through *its own* gate, so a chain moves *work* forward
    /// without ever moving *authority*: a gated downstream SOP still halts at
    /// `AwaitingApproval` and an outward SOP still needs its own credential.
    #[serde(default)]
    pub on_success_handoff: Option<String>,
}
/// Result of running a playbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybookOutcome {
    /// The gate cleared and the deterministic steps ran.
    Executed {
        request_id: String,
        status: String,
        ok_steps: usize,
        total_steps: usize,
        /// Secret-free rendering of the cross-SOP hop this clean run queued, if
        /// the playbook declares one and the run finished clean. `None` for an
        /// unchained SOP, and for a halted run (a failed SOP hands off nothing).
        handoff: Option<String>,
    },
    /// The gate needs sign-off; steps were NOT run (no side effects, no spend).
    AwaitingApproval {
        request_id: String,
        approvers: Vec<String>,
        quorum: usize,
    },
    /// The gate denied the action outright.
    Denied { reason: String },
}
/// Parse one playbook step from its JSON form. Accepted shapes:
/// - `{"tool": "name", "args": {...}}`
/// - `{"agent": "prompt", "team": "slug"?}`
/// - `{"connector": "id", "req": {...}}`
/// - `{"condition": "ok" | "failed" | "skipped", "text": {"all": [..], "none": [..]}?,///    "recheck": {"tool": .., "args": {..}, "attempts": n?, "settle_ms": n?}?}` — the
///   optional `text` makes it a verdict gate on the prior step's output, and the
///   optional `recheck` lets that gate re-*observe* before halting.
/// - `{"foreach": {"from": "ctxKey|lastOutput", "as": "itemName",///    "body": [ <step>, ... ]}}` — deterministic fan-out; runs `body` once per
///   element of the JSON array found at `ctx[from]` (or parsed from the prior
///   step's output when `from == "lastOutput"`), binding each item under `as`.
///   A failing iteration halts the whole fan-out (parent short-circuits).
pub fn parse_step(v: &serde_json::Value) -> Result<crate::editor::workflow::WorkflowStep, String> {
    use crate::editor::workflow::{StepOutcome, WorkflowStep};
    if let Some(name) = v.get("tool").and_then(|x| x.as_str()) {
        Ok(WorkflowStep::Tool {
            name: name.to_string(),
            args: v.get("args").cloned().unwrap_or(serde_json::json!({})),
        })
    } else if let Some(prompt) = v.get("agent").and_then(|x| x.as_str()) {
        Ok(WorkflowStep::AgentTask {
            prompt: prompt.to_string(),
            team: v
                .get("team")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        })
    } else if let Some(id) = v.get("connector").and_then(|x| x.as_str()) {
        Ok(WorkflowStep::Connector {
            id: id.to_string(),
            req: v.get("req").cloned().unwrap_or(serde_json::json!({})),
        })
    } else if let Some(cond) = v.get("condition").and_then(|x| x.as_str()) {
        let require = match cond.to_lowercase().as_str() {
            "failed" => StepOutcome::Failed,
            "skipped" => StepOutcome::Skipped,
            _ => StepOutcome::Ok,
        };

        // Optional verdict predicate: {"condition":"ok",
        //   "text":{"all":["VERIFY: PASS"],"none":["VERIFY: FAIL","ABSENT: FAIL"]}}
        let on_text = v.get("text").and_then(|t| {
            let to_vec = |k: &str| {
                t.get(k)
                    .and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|s| s.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let all = to_vec("all");
            let none = to_vec("none");
            if all.is_empty() && none.is_empty() {
                None
            } else {
                Some(crate::editor::workflow::TextPredicate {
                    contains_all: all,
                    contains_none: none,
                })
            }
        });

        // Optional bounded re-verification on the gate:
        //   "recheck":{"tool":"execute_org_browser_task","args":{...read-only...},
        //              "attempts":2,"settle_ms":1500}

        // The engine refuses a recheck that could mutate state, so an absent
        // verdict can never be retried by replaying the action itself.
        let recheck = v.get("recheck").and_then(|r| {
            let tool = r.get("tool").and_then(|x| x.as_str())?;
            Some(crate::editor::workflow::Recheck {
                tool: tool.to_string(),
                args: r.get("args").cloned().unwrap_or(serde_json::json!({})),
                attempts: r.get("attempts").and_then(|x| x.as_u64()).unwrap_or(1) as u32,
                settle_ms: r.get("settle_ms").and_then(|x| x.as_u64()).unwrap_or(0),
            })
        });
        Ok(WorkflowStep::Condition {
            require,
            on_text,
            recheck,
        })
    } else if let Some(fe) = v.get("foreach") {
        // Deterministic fan-out: {"foreach":{"from":"invoices","as":"invoice",
        //   "body":[{...step...}, ...]}}
        // `from` names a key on the run's ctx (or
        // the literal "lastOutput" to parse the prior step's JSON array), `as`
        // binds each element inside the body, `body` is recursively parsed the
        // same way as the top-level playbook steps array.
        let source = fe
            .get("from")
            .and_then(|x| x.as_str())
            .ok_or("foreach.from must be a string")?
            .to_string();
        let as_var = fe
            .get("as")
            .and_then(|x| x.as_str())
            .ok_or("foreach.as must be a string")?
            .to_string();
        let body_arr = fe
            .get("body")
            .and_then(|x| x.as_array())
            .ok_or("foreach.body must be an array of steps")?;
        let mut body = Vec::with_capacity(body_arr.len());
        for sub in body_arr {
            body.push(parse_step(sub)?);
        }
        Ok(WorkflowStep::ForEach {
            source,
            as_var,
            body,
        })
    } else {
        Err("step must be one of {tool, agent, connector, condition, foreach}".to_string())
    }
}
/// Validate that every `Tool` step in the playbook references a registered
/// tool name. See [`validate_step_references`] for the shared implementation.
pub fn validate_playbook_tool_names(pb: &Playbook) -> Result<(), String> {
    // Playbook call sites don't all carry a workspace root, so connector ids
    // are only validated on the paths that pass one (workflow_list/run).
    validate_step_references(None, "playbook", &pb.id, &pb.steps)
}

/// Shared machinery behind [`validate_playbook_tool_names`] and the
/// `workflow_list`/`workflow_run` MCP tools: check every `WorkflowStep::Tool`
/// name (recursing through ForEach bodies) against the live tool registry, and
/// — when a `workspace_root` is supplied — every `Connector` id against
/// `.velocity/connectors.json`. Returns `Err` with a human-readable message
/// for the first unknown reference encountered (positional info included).
/// AgentTasks are skipped (prompt dispatch, different namespace). `kind`
/// only labels the error message, so playbooks and plain workflows share
/// one code path.
pub fn validate_step_references(
    workspace_root: Option<&std::path::Path>,
    kind: &str,
    id: &str,
    steps: &[crate::editor::workflow::WorkflowStep],
) -> Result<(), String> {
    use crate::editor::workflow::WorkflowStep;

    /// (namespace, location_label, reference_name)
    fn collect_refs(steps: &[WorkflowStep], base: &str, out: &mut Vec<(&str, String, String)>) {
        for (i, s) in steps.iter().enumerate() {
            let loc = if base.is_empty() {
                i.to_string()
            } else {
                format!("{base}[{i}]")
            };
            match s {
                WorkflowStep::Tool { name, .. } => out.push(("tool", loc, name.clone())),
                WorkflowStep::Connector { id, .. } => out.push(("connector", loc, id.clone())),
                WorkflowStep::ForEach { body, .. } => {
                    collect_refs(body, &format!("{loc}>foreach"), out);
                }
                _ => {}
            }
        }
    }

    let mut refs = Vec::new();
    collect_refs(steps, "", &mut refs);
    if refs.is_empty() {
        return Ok(());
    }

    // Resolve lazily: registry and connector store are only read if the
    // steps actually reference them.
    let mut tools: Option<std::collections::HashSet<String>> = None;
    let mut connectors: Option<std::collections::HashSet<String>> = None;
    for (ns, loc, name) in &refs {
        let (known, missing_label): (&mut std::collections::HashSet<String>, &str) = match *ns {
            "tool" => (
                tools.get_or_insert_with(|| {
                    crate::registry::get_tools()
                        .into_iter()
                        .map(|t| t.name)
                        .collect()
                }),
                "tool",
            ),
            _ => {
                let Some(root) = workspace_root else { continue };
                (
                    connectors.get_or_insert_with(|| {
                        crate::connectors::ConnectorRegistry::load(root)
                            .connectors
                            .into_iter()
                            .map(|c| c.id)
                            .collect()
                    }),
                    "connector",
                )
            }
        };
        if !known.contains(name.as_str()) {
            return Err(format!(
                "{kind} '{id}': unknown {missing_label} '{name}' at step {loc}"
            ));
        }
    }
    Ok(())
}

/// Run a playbook: submit its intent to the approval engine, and execute the
/// deterministic steps only if the action clears. Returns the outcome.
pub fn run_playbook(
    workspace_root: &Path,
    org: &CompanyOrg,
    playbook_id: &str,
) -> Result<PlaybookOutcome, String> {
    let pb = org
        .playbooks
        .iter()
        .find(|p| p.id == playbook_id)
        .ok_or_else(|| format!("playbook '{playbook_id}' not found"))?;
    // Fail fast if any Tool step names an unregistered tool, or any Connector
    // step names an id missing from `.velocity/connectors.json`.
    validate_step_references(Some(workspace_root), "playbook", playbook_id, &pb.steps)?;
    let req = submit_action_for_playbook(
        workspace_root,
        org,
        &pb.role_id,
        &pb.resource_id,
        pb.capability,
        pb.amount,
        &pb.description,
        Some(&pb.id),
    );
    match req.status {
        ActionStatus::Approved => Ok(run_playbook_steps(workspace_root, org, pb, &req.id, 0)),
        ActionStatus::Pending => Ok(PlaybookOutcome::AwaitingApproval {
            request_id: req.id,
            approvers: req.required_approvers,
            quorum: req.quorum.max(1),
        }),
        _ => Ok(PlaybookOutcome::Denied {
            reason: req.decision_reason,
        }),
    }
}
/// Resolve the SOP an approval request belongs to. A request that names its own
/// playbook wins: several SOPs can legitimately share one gate (broadcast, then
/// confirm it landed), and the (role, resource, capability) triple alone would
/// then resume whichever was authored *first* — not the one a human approved.
/// Requests raised outside a playbook (a direct browser action) carry no id and
/// still resolve by their triple.
pub fn playbook_for_request<'a>(org: &'a CompanyOrg, req: &ActionRequest) -> Option<&'a Playbook> {
    match &req.playbook_id {
        Some(id) => org.playbooks.iter().find(|p| p.id == *id),
        None => org.playbooks.iter().find(|p| {
            p.role_id == req.role_id
                && p.resource_id == req.resource_id
                && p.capability == req.capability
        }),
    }
}
/// Run a gated playbook's deterministic steps *after* its approval request has
/// cleared. [`run_playbook`] halts a gated playbook at `AwaitingApproval` and does
/// not run any step (no side effects). Once a designated approver advances that
/// request to [`ActionStatus::Approved`] (via `approve_org_action`), this resumes
/// it: the request id is the proof the gate cleared, so we run the stored steps
/// against the live engine, threading `{{requestId}}` into outward-action steps
/// (e.g. the browser publish) and marking the request `Executed` so it cannot be
/// replayed. Returns `AwaitingApproval` if the request is still pending and
/// `Denied` if it was rejected — in both cases no step runs.
pub fn resume_playbook_after_approval(
    workspace_root: &Path,
    org: &CompanyOrg,
    request_id: &str,
) -> Result<PlaybookOutcome, String> {
    let req = load_requests(workspace_root)
        .into_iter()
        .find(|r| r.id == request_id)
        .ok_or_else(|| format!("request '{request_id}' not found"))?;
    let pb = playbook_for_request(org, &req).ok_or_else(|| match &req.playbook_id {
        Some(id) => format!("request '{request_id}' names playbook '{id}', which no longer exists"),
        None => format!(
            "no playbook matches request '{request_id}' ({} -[{}] -> {})",
            req.role_id,
            req.capability.as_str(),
            req.resource_id
        ),
    })?;
    match req.status {
        // A human resumption restarts the chain budget at depth 0: the approval
        // itself is the checkpoint, so a long pipeline of *gated* SOPs can never
        // deadlock on the depth guard, while an ungated cycle still stops itself.
        ActionStatus::Approved => Ok(run_playbook_steps(workspace_root, org, pb, &req.id, 0)),
        ActionStatus::Pending => Ok(PlaybookOutcome::AwaitingApproval {
            request_id: req.id,
            approvers: if req.required_approvers.is_empty() {
                vec![HUMAN_OPERATOR.to_string()]
            } else {
                req.required_approvers.clone()
            },
            quorum: req.quorum.max(1),
        }),
        _ => Ok(PlaybookOutcome::Denied {
            reason: req.decision_reason,
        }),
    }
}
/// Sign a request and, when that signature reaches quorum, run the SOP that
/// asked for it — one deterministic step instead of two calls that a human (or
/// an agent) can forget. This is what the dashboard's APPROVE button does:
/// an approval that never executes is only half a decision, because the gated
/// duty would otherwise sit `Approved` forever with its side effect unwritten.
//
//
// The run goes through [`resume_playbook_after_approval`], so it inherits every
/// guarantee of the two-step path: `{{requestId}}` threading into outward steps,
/// the [`ActionStatus::Executed`] no-replay mark, and a success handoff that
/// re-enters the *next* SOP's own gate rather than laundering authority.
//
//
// Deliberately narrow, so a signature never invents work:
/// * quorum not reached → nothing runs, and the line names who is still missing;
/// * a request raised outside a playbook (a direct browser intent) carries no
///   stored steps, so the approval stands but no script is guessed for it;
/// * a request whose SOP has since been deleted reports that loss instead of
///   silently swallowing the approval.
pub fn approve_and_resume(
    workspace_root: &Path,
    org: &CompanyOrg,
    request_id: &str,
    approver_id: &str,
) -> Result<String, String> {
    let req = approve_action(workspace_root, org, request_id, approver_id)?;
    if req.status != ActionStatus::Approved {
        let needed = req.quorum.max(1);
        let missing: Vec<String> = req
            .required_approvers
            .iter()
            .filter(|w| !req.approvals.contains(w))
            .cloned()
            .collect();
        return Ok(format!(            "'{request_id}' signed by {approver_id} ({} of {needed}). Still waiting for [{}] — nothing ran.",            req.approvals.len(),            missing.join(", ")        ));
    }

    // A SOP-raised intent names the playbook whose steps are waiting on this
    // signature. Anything else was requested directly: approving it is a
    // decision about the single action, not a licence to run some script.
    let Some(pb_id) = req.playbook_id.clone() else {
        return Ok(format!(            "'{request_id}' approved by {approver_id} (quorum reached). It was not raised by a playbook, so there are no stored steps to run."        ));
    };
    if playbook_for_request(org, &req).is_none() {
        return Ok(format!(            "'{request_id}' approved by {approver_id}, but playbook '{pb_id}' no longer exists — the steps it was waiting on are gone."        ));
    }
    let outcome = resume_playbook_after_approval(workspace_root, org, request_id)?;
    let trail = match &outcome {
        PlaybookOutcome::Executed {
            handoff: Some(trail),
            ..
        } => format!("\nChain:\n{trail}"),
        _ => String::new(),
    };
    Ok(format!(
        "'{request_id}' approved by {approver_id} (quorum reached). Ran '{pb_id}': {}{trail}",
        sop_run_digest(&outcome)
    ))
}
/// Approved requests that still cannot execute because they need a just-in-time
/// credential elevation and none is live: high-value Spend/Deploy gates hold a
/// second factor beyond the signature, so a human who only approved the request
/// sees it here rather than watching the executor refuse it. A request whose
/// elevation is already minted and waiting is excluded — this is the actionable
/// queue, not a restatement of the ledger.
pub fn approved_awaiting_elevation(workspace_root: &Path, now: u64) -> Vec<ActionRequest> {
    load_requests(workspace_root)
        .into_iter()
        .filter(|r| r.status == ActionStatus::Approved)
        .filter(|r| capability_requires_elevation(r.capability))
        .filter(|r| {
            find_active_elevation(
                workspace_root,
                &r.role_id,
                &r.resource_id,
                r.capability,
                now,
            )
            .is_none()
        })
        .collect()
}
/// Complete an approved irreversible action: mint the single-use elevation the
/// scope requires and run the playbook waiting on that request in one
/// deterministic step. This is the second half of the human-in-the-loop pipeline
/// for financial and infrastructure duties — approving is the decision, the
/// elevation is the key, and neither can be skipped.
//
//
// * the elevation is minted by [`grant_jit_elevation`], which copies the scope
///   verbatim from the request and refuses anything that is not `Approved`, so a
///   click can never mint broader access than the quorum cleared;
/// * the run spends it: [`ensure_elevation_cleared`] inside the executor consumes
///   the grant, so one click authorizes exactly one execution;
/// * capabilities that need no elevation simply run.
pub fn elevate_and_resume(
    workspace_root: &Path,
    org: &CompanyOrg,
    request_id: &str,
    ttl_secs: u64,
    now: u64,
) -> Result<String, String> {
    let req = load_requests(workspace_root)
        .into_iter()
        .find(|r| r.id == request_id)
        .ok_or_else(|| format!("request '{request_id}' not found"))?;
    if req.status != ActionStatus::Approved {
        return Err(format!(            "request '{request_id}' is {:?}, not Approved — nothing to elevate. Clear the approval gate first.",            req.status        ));
    }
    let minted = if capability_requires_elevation(req.capability) {
        let grant = grant_jit_elevation(workspace_root, request_id, ttl_secs, now)?;
        Some(format!(
            "Minted elevation '{}' for {} -[{}]-> {} (single-use, active until t={}).",
            grant.id,
            grant.role_id,
            grant.capability.as_str(),
            grant.resource_id,
            grant.expires_at
        ))
    } else {
        None
    };
    let outcome = resume_playbook_after_approval(workspace_root, org, request_id)?;
    let pb_label = playbook_for_request(org, &req)
        .map(|p| p.id.clone())
        .unwrap_or_else(|| request_id.to_string());
    let trail = match &outcome {
        PlaybookOutcome::Executed {
            handoff: Some(trail),
            ..
        } => format!("\nChain:\n{trail}"),
        _ => String::new(),
    };
    Ok(format!(
        "{}Ran '{pb_label}': {}{trail}",
        minted
            .as_deref()
            .map(|m| format!("{m}\n"))
            .unwrap_or_default(),
        sop_run_digest(&outcome)
    ))
}
/// Shared tail for both playbook entry points: build the workflow, run its steps
/// with `{{requestId}}` bound to the cleared request, advance the ledger to
/// `Executed`, fire the success handoff (if any) and fold the run into a
/// [`PlaybookOutcome`]. `chain_depth` is how many handoff hops led here — 0 for
/// a run an operator or the scheduler started directly.
fn run_playbook_steps(
    workspace_root: &Path,
    org: &CompanyOrg,
    pb: &Playbook,
    request_id: &str,
    chain_depth: usize,
) -> PlaybookOutcome {
    let wf = crate::editor::workflow::Workflow {
        id: pb.id.clone(),
        name: pb.name.clone(),
        steps: pb.steps.clone(),
    };
    let run = wf.execute_with_context(
        workspace_root,
        &serde_json::json!({
         "requestId": request_id
        }
        ),
    );

    // Cross-SOP verdict memory: a run that halted on a Condition (or failed a
    // step without one) is persisted to the playbook ledger, so approvers can
    // see *which* SOP is stuck and why after the run itself has ended. A
    // clean Success records nothing — silence means healthy.    record_playbook_run_verdict
    record_playbook_run_verdict(workspace_root, &pb.id, request_id, &run);
    let _ = mark_executed(workspace_root, request_id);

    // Cross-SOP handoff: only a *clean* run advances the chain, on purpose. A
    // run that halted mid-way did not finish its duty, so the next department's
    // work is left unqueued rather than sprung from a half-done premise.
    let handoff = if run.status == crate::editor::workflow::RunStatus::Success {
        fire_sop_handoff(workspace_root, org, pb, request_id, chain_depth)
    } else {
        None
    };
    PlaybookOutcome::Executed {
        request_id: request_id.to_string(),
        status: run.status.label().to_string(),
        ok_steps: run.ok_count(),
        total_steps: run.steps.len(),
        handoff,
    }
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Cross-SOP verdict memory (secret-free ledger of halted/failed playbook runs)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
//
// Phase 63's verdict gates halt a playbook deterministically — but a halt that
// nobody can see is a silently stuck SOP. Every non-clean playbook run is
// recorded here: which playbook, which request, which step halted it, and the
// engine's own condition message. Deliberately secret-free: for a halted
// verdict-gate the detail is the Condition's payload-free message; for any
// other failure only the step's *kind label* is stored, never its output (tool
// output can carry page-extracted content).
/// One recorded playbook-run verdict: a halt or failure an approver should see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookVerdict {
    pub seq: u64,
    pub at: u64,
    pub playbook_id: String,
    pub request_id: String,
    /// "halted" (a verdict/outcome Condition short-circuited the run) or
    /// "failed" (a step failed without a gating Condition).
    pub outcome: String,
    /// Index of the step that stopped the run.
    pub step_index: usize,
    /// Short human label of that step (e.g. "condition==ok" or "tool:read_file").
    pub step_kind: String,
    /// Engine-provided, secret-free explanation (a Condition message or the
    /// failed step's kind only).
    pub detail: String,
}
const VERDICTS_LABEL: &[u8] = b"org_playbook_verdicts";
fn verdicts_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root
        .join(".velocity")
        .join("org_playbook_verdicts.nda")
}
fn load_verdicts(workspace_root: &Path) -> Vec<PlaybookVerdict> {
    let path = verdicts_path(workspace_root);
    if let Ok(bytes) = fs::read(&path) {
        let plain = crate::agent::crypto::open(workspace_root, VERDICTS_LABEL, &bytes);
        let text = String::from_utf8_lossy(&plain);
        if let Ok(list) = serde_json::from_str::<Vec<PlaybookVerdict>>(&text) {
            return list;
        }
    }
    Vec::new()
}
fn save_verdicts(workspace_root: &Path, entries: &[PlaybookVerdict]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(entries) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, VERDICTS_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(verdicts_path(workspace_root), bytes).is_ok()
}
/// Fold one finished [`crate::editor::workflow::WorkflowRun`] into the verdict
/// ledger. Returns the recorded line when the run was NOT a clean success;
/// `None` (nothing written) when it was. Pure bookkeeping — never fails a run.
pub fn record_playbook_run_verdict(
    workspace_root: &Path,
    playbook_id: &str,
    request_id: &str,
    run: &crate::editor::workflow::WorkflowRun,
) -> Option<PlaybookVerdict> {
    use crate::editor::workflow::{RunStatus, StepOutcome};
    if run.status == RunStatus::Success {
        return None;
    }
    let halt = run
        .steps
        .iter()
        .find(|s| s.outcome == StepOutcome::Failed && s.kind.starts_with("condition"));
    let (step, outcome, detail) = match halt {
        // A verdict gate's own message carries no payload: "condition failed:        // verdict predicate not satisfied (prior ok)" is safe to persist.        Some
        Some(h) => (h.index, "halted", h.output.clone()),
        None => {
            // Any other stopper records only the step's KIND, never its output
            // (tool output can carry page-extracted content). A run with no
            // failed step at all (e.g. a Skipped connector making it Partial)
            // is not a stuck SOP — don't page anyone.
            let f = run
                .steps
                .iter()
                .find(|s| s.outcome == StepOutcome::Failed)?;
            (
                f.index,
                "failed",
                format!(
                    "step '{}' failed (output withheld, secret-free ledger)",
                    f.kind
                ),
            )
        }
    };
    let mut all = load_verdicts(workspace_root);
    let entry = PlaybookVerdict {
        seq: all.len() as u64 + 1,
        at: now_secs(),
        playbook_id: playbook_id.to_string(),
        request_id: request_id.to_string(),
        outcome: outcome.to_string(),
        step_index: step,
        step_kind: run.steps[step].kind.clone(),
        detail,
    };
    all.push(entry.clone());
    save_verdicts(workspace_root, &all);
    Some(entry)
}
/// The most recent `limit` playbook verdicts (newest last), secret-free.
pub fn playbook_verdict_trail(workspace_root: &Path, limit: usize) -> Vec<PlaybookVerdict> {
    let all = load_verdicts(workspace_root);
    let start = all.len().saturating_sub(limit);
    all[start..].to_vec()
}
/// Render the verdict trail as a compact escalation briefing for approvers:
/// which SOPs are stuck, at which step, and from which cleared request.
pub fn describe_playbook_verdicts(entries: &[PlaybookVerdict]) -> String {
    if entries.is_empty() {
        return "No playbook halts or failures recorded — every SOP run finished clean."
            .to_string();
    }
    let mut out = format!(
        "PLAYBOOK VERDICTS ({} stuck run{}):\n",
        entries.len(),
        if entries.len() == 1 { "" } else { "s" }
    );
    for e in entries {
        out.push_str(&format!(
            "  #{} t={} '{}' {} at step {} [{}] req={}: {}\n",
            e.seq,
            e.at,
            e.playbook_id,
            e.outcome,
            e.step_index,
            e.step_kind,
            e.request_id,
            e.detail
        ));
    }
    out.push_str(
        "  (a halted SOP re-runs only after the underlying page, gate or elevation is fixed)\n",
    );
    out
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Cross-SOP handoff chains (deterministic work routing, never authority routing)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
//
// A playbook normally ends where its own steps end, so the next duty in a real
// workflow — research landed → write the copy → publish → confirm it landed —
// has to be *remembered* by somebody, which is exactly the kind of loose thread
// an inference step would be asked to patch. Chains make that handoff
// deterministic instead: a playbook names one downstream playbook that is
// queued when, and only when, this run finished clean.
//
// The property that makes this safe to automate is that a chain carries *work*
// forward without carrying *authority* forward. Each hop re-submits the
// downstream SOP's intent through [`submit_action`] under the downstream's own
// (role, resource, capability, amount), so a gated hop still halts at
// `AwaitingApproval` for its own approvers and a denied hop never runs a step.
// [`MAX_SOP_CHAIN_DEPTH`] bounds the walk, so a mis-declared cycle (A → B → A)
// terminates with a recorded refusal rather than recursing until the stack dies.
/// One recorded chain hop: which SOP handed off to which, under which requests,
/// and what the downstream gate decided. Secret-free by construction — playbook
/// ids, request ids, a closed-set outcome label and a hop count; no step output
/// and no page content ever reaches this ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopHandoff {
    pub seq: u64,
    pub at: u64,
    pub from_playbook: String,
    pub to_playbook: String,
    /// The cleared request whose successful run fired this hop.
    pub source_request_id: String,
    /// The downstream request the gate decision belongs to. Empty when the hop
    /// was refused before a request could be submitted (missing target, depth).
    pub spawned_request_id: String,
    /// Closed set: `"ran"` (the gate cleared and the downstream was dispatched),
    /// `"awaiting-approval"`, `"denied"`, `"not-found"` or `"depth-limit"`.
    pub outcome: String,
    /// How many hops into the chain this one is (0 = the first handoff).
    pub chain_depth: usize,
}
/// Hard bound on chain walking: deep enough for any real cross-department
/// handoff, shallow enough that a cycle stops deterministically.
pub const MAX_SOP_CHAIN_DEPTH: usize = 8;
const HANDOFFS_LABEL: &[u8] = b"org_sop_handoffs";
fn handoffs_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root
        .join(".velocity")
        .join("org_sop_handoffs.nda")
}
fn load_handoffs(workspace_root: &Path) -> Vec<SopHandoff> {
    let path = handoffs_path(workspace_root);
    if let Ok(bytes) = fs::read(&path) {
        let plain = crate::agent::crypto::open(workspace_root, HANDOFFS_LABEL, &bytes);
        let text = String::from_utf8_lossy(&plain);
        if let Ok(list) = serde_json::from_str::<Vec<SopHandoff>>(&text) {
            return list;
        }
    }
    Vec::new()
}
fn save_handoffs(workspace_root: &Path, entries: &[SopHandoff]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(entries) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, HANDOFFS_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(handoffs_path(workspace_root), bytes).is_ok()
}
/// Append one hop to the chain ledger. Bookkeeping only — a failed write can
/// never fail the run that produced it.
fn record_handoff(
    workspace_root: &Path,
    from: &str,
    to: &str,
    source_request_id: &str,
    spawned_request_id: &str,
    outcome: &str,
    chain_depth: usize,
) -> SopHandoff {
    let mut all = load_handoffs(workspace_root);
    let entry = SopHandoff {
        seq: all.len() as u64 + 1,
        at: now_secs(),
        from_playbook: from.to_string(),
        to_playbook: to.to_string(),
        source_request_id: source_request_id.to_string(),
        spawned_request_id: spawned_request_id.to_string(),
        outcome: outcome.to_string(),
        chain_depth,
    };
    all.push(entry.clone());
    save_handoffs(workspace_root, &all);
    entry
}
/// The most recent `limit` chain hops (newest last), secret-free.
pub fn sop_handoff_trail(workspace_root: &Path, limit: usize) -> Vec<SopHandoff> {
    let all = load_handoffs(workspace_root);
    let start = all.len().saturating_sub(limit);
    all[start..].to_vec()
}
/// A one-line, secret-free digest of a nested playbook run for the chain trail.
fn sop_run_digest(outcome: &PlaybookOutcome) -> String {
    match outcome {
        PlaybookOutcome::Executed {
            status,
            ok_steps,
            total_steps,
            ..
        } => format!("{status} ({ok_steps}/{total_steps} steps ok)"),
        PlaybookOutcome::AwaitingApproval {
            approvers, quorum, ..
        } => format!(
            "queued for approval — needs {quorum} from [{}]",
            approvers.join(", ")
        ),
        PlaybookOutcome::Denied { reason } => format!("denied ({reason})"),
    }
}
/// Fire `pb`'s declared success handoff. Returns the rendered hop trail when a
/// hop actually happened, `None` when the playbook is unchained. Called only
/// from [`run_playbook_steps`] on a clean run.
fn fire_sop_handoff(
    workspace_root: &Path,
    org: &CompanyOrg,
    pb: &Playbook,
    source_request_id: &str,
    chain_depth: usize,
) -> Option<String> {
    let to_id = pb.on_success_handoff.as_deref()?;

    // Indent grows with depth so a multi-hop trail reads as a chain.
    let pad = "  ".repeat(chain_depth + 1);
    if chain_depth >= MAX_SOP_CHAIN_DEPTH {
        let hop = record_handoff(
            workspace_root,
            &pb.id,
            to_id,
            source_request_id,
            "",
            "depth-limit",
            chain_depth,
        );
        return Some(format!(            "{pad}→ '{to_id}' NOT queued: chain depth {} exceeded (hop #{}). Break the cycle with chain_org_playbooks.",            hop.chain_depth, hop.seq        ));
    }
    let Some(next) = org.playbooks.iter().find(|p| p.id == to_id) else {
        record_handoff(
            workspace_root,
            &pb.id,
            to_id,
            source_request_id,
            "",
            "not-found",
            chain_depth,
        );
        return Some(format!(
            "{pad}→ '{to_id}' NOT queued: no such playbook (dangling handoff)"
        ));
    };

    // The downstream SOP is gated on ITS own triple — a chain cannot inherit a
    // clearance belonging to the SOP that started it.
    let req = submit_action_for_playbook(
        workspace_root,
        org,
        &next.role_id,
        &next.resource_id,
        next.capability,
        next.amount,
        &format!("SOP handoff '{}' → '{}'", pb.id, next.id),
        Some(&next.id),
    );
    match req.status {
        ActionStatus::Approved => {
            // Recorded BEFORE the nested run, so the trail reads in the order
            // the work actually moved: this hop, then any hop it springs. The
            // outcome describes the *routing* decision — whether the downstream
            // run then halted is the verdict ledger's business, and the rendered
            // line carries the nested digest either way.            record_handoff
            record_handoff(
                workspace_root,
                &pb.id,
                to_id,
                source_request_id,
                &req.id,
                "ran",
                chain_depth,
            );
            let nested = run_playbook_steps(workspace_root, org, next, &req.id, chain_depth + 1);
            let mut line = format!(
                "{pad}→ '{}' ran (req={}): {}",
                next.id,
                req.id,
                sop_run_digest(&nested)
            );
            if let PlaybookOutcome::Executed {
                handoff: Some(deeper),
                ..
            } = nested
            {
                line.push('\n');
                line.push_str(&deeper);
            }
            Some(line)
        }
        ActionStatus::Pending => {
            record_handoff(
                workspace_root,
                &pb.id,
                to_id,
                source_request_id,
                &req.id,
                "awaiting-approval",
                chain_depth,
            );
            Some(format!(
                "{pad}→ '{}' queued, steps did NOT run: awaiting {} approval(s) from [{}] (req={})",
                next.id,
                req.quorum.max(1),
                if req.required_approvers.is_empty() {
                    vec![HUMAN_OPERATOR.to_string()]
                } else {
                    req.required_approvers.clone()
                }
                .join(", "),
                req.id
            ))
        }
        _ => {
            record_handoff(
                workspace_root,
                &pb.id,
                to_id,
                source_request_id,
                &req.id,
                "denied",
                chain_depth,
            );
            Some(format!(
                "{pad}→ '{}' blocked by policy: {}",
                next.id, req.decision_reason
            ))
        }
    }
}
/// Declare (or, with `downstream` of `None`, clear) a success handoff edge and
/// persist the org. Refuses unknown ids and self-loops, which would otherwise
/// spend the whole depth budget on a one-node cycle before anything useful ran.
pub fn chain_playbooks(
    workspace_root: &Path,
    org: &mut CompanyOrg,
    upstream_id: &str,
    downstream: Option<&str>,
) -> Result<String, String> {
    let upstream = org
        .playbooks
        .iter()
        .position(|p| p.id == upstream_id)
        .ok_or_else(|| format!("playbook '{upstream_id}' not found"))?;
    let resolved = match downstream {
        None => {
            org.playbooks[upstream].on_success_handoff = None;
            save_company_org(workspace_root, org);
            return Ok(format!(
                "Uncoupled '{}' — it no longer queues a downstream SOP.",
                org.playbooks[upstream].id
            ));
        }
        Some(target) => {
            if target == upstream_id {
                return Err(format!("'{upstream_id}' cannot hand off to itself"));
            }
            if !org.playbooks.iter().any(|p| p.id == target) {
                return Err(format!("playbook '{target}' not found"));
            }
            Some(target.to_string())
        }
    };
    org.playbooks[upstream].on_success_handoff = resolved.clone();
    save_company_org(workspace_root, org);
    Ok(format!(
        "Chained '{}' → '{}': when '{}' finishes clean, '{}' is queued through its own gate.",
        org.playbooks[upstream].id,
        resolved.as_deref().unwrap_or("-"),
        org.playbooks[upstream].id,
        resolved.as_deref().unwrap_or("-")
    ))
}
/// Render the chain ledger as a trail an approver can read: every hop, in order,
/// with the gate's own verdict on the downstream SOP.
pub fn describe_sop_handoffs(entries: &[SopHandoff]) -> String {
    if entries.is_empty() {
        return "No cross-SOP handoffs recorded yet — every SOP has been run on its own."
            .to_string();
    }
    let mut out = format!(
        "SOP HANDOFFS ({} hop{}):\n",
        entries.len(),
        if entries.len() == 1 { "" } else { "s" }
    );
    for e in entries {
        out.push_str(&format!(
            "  #{} t={} depth{} '{}' → '{}' {} req={}→{}\n",
            e.seq,
            e.at,
            e.chain_depth,
            e.from_playbook,
            e.to_playbook,
            e.outcome,
            e.source_request_id,
            e.spawned_request_id
        ));
    }
    out.push_str(&format!(        "  (a handoff queues work only; each hop is still gated on its own role/capability, max depth {MAX_SOP_CHAIN_DEPTH})\n"    ));
    out
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Recurring SOP scheduler (cadence state + deterministic tick)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
//
// An SOP nobody triggers is a duty nobody performs: uptime checks, staging
// verification and weekly reviews are *periodic* work. A [`SopSchedule`] attaches
// a cadence to a playbook id, and [`run_due_sops`] fires each due entry through
// the SAME gated [`run_playbook`] entry point an operator calls — so a scheduled
// payment or outward post still queues for approval instead of firing
// autonomously. The clock is injected rather than read from a hidden `now`, which
// makes the tick reproducible, testable, and able to backfill work that fell due
// while the machine slept exactly once (advance-then-run, never replayed N times).
// Stored state is ids, intervals and timestamps only — no page content, no secrets.
/// One recurring duty: run `playbook_id` every `every_secs`, next at `next_due_at`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopSchedule {
    pub playbook_id: String,
    pub every_secs: u64,
    pub next_due_at: u64,
    /// 0 until this cadence has fired at least once.
    pub last_run_at: u64,
    pub enabled: bool,
}
/// Faster than this is a busy-loop, not a duty: a sub-minute cadence would flood
/// the approval queue with requests a human cannot clear.
pub const MIN_SOP_INTERVAL_SECS: u64 = 60;
const SCHEDULES_LABEL: &[u8] = b"org_sop_schedules";
fn schedules_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root
        .join(".velocity")
        .join("org_sop_schedules.nda")
}
fn load_schedules(workspace_root: &Path) -> Vec<SopSchedule> {
    let path = schedules_path(workspace_root);
    if let Ok(bytes) = fs::read(&path) {
        let plain = crate::agent::crypto::open(workspace_root, SCHEDULES_LABEL, &bytes);
        let text = String::from_utf8_lossy(&plain);
        if let Ok(list) = serde_json::from_str::<Vec<SopSchedule>>(&text) {
            return list;
        }
    }
    Vec::new()
}
fn save_schedules(workspace_root: &Path, entries: &[SopSchedule]) -> bool {
    let dir = workspace_root.join(".velocity");
    let _ = fs::create_dir_all(&dir);
    let serialized = match serde_json::to_string_pretty(entries) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let bytes = crate::agent::crypto::seal(workspace_root, SCHEDULES_LABEL, serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(schedules_path(workspace_root), bytes).is_ok()
}
/// Arm (or re-arm) a cadence for an existing playbook. Re-arming replaces the old
/// interval and resets the due clock, so changing a cadence never leaves two
/// entries competing for the same duty.
pub fn schedule_sop(
    workspace_root: &Path,
    org: &CompanyOrg,
    playbook_id: &str,
    every_secs: u64,
) -> Result<SopSchedule, String> {
    if !org.playbooks.iter().any(|p| p.id == playbook_id) {
        return Err(format!(
            "No playbook '{playbook_id}' to schedule — add it first (add_org_playbook)."
        ));
    }
    if every_secs < MIN_SOP_INTERVAL_SECS {
        return Err(format!(            "Cadence must be at least {MIN_SOP_INTERVAL_SECS}s — a faster repeat would flood the approval queue."        ));
    }
    let entry = SopSchedule {
        playbook_id: playbook_id.to_string(),
        every_secs,
        next_due_at: now_secs() + every_secs,
        last_run_at: 0,
        enabled: true,
    };
    let mut all = load_schedules(workspace_root);
    all.retain(|s| s.playbook_id != playbook_id);
    all.push(entry.clone());
    save_schedules(workspace_root, &all);
    Ok(entry)
}
/// Retire a cadence entirely.
pub fn unschedule_sop(workspace_root: &Path, playbook_id: &str) -> bool {
    let mut all = load_schedules(workspace_root);
    let before = all.len();
    all.retain(|s| s.playbook_id != playbook_id);
    if all.len() == before {
        return false;
    }
    save_schedules(workspace_root, &all);
    true
}
/// Pause or resume a cadence without losing its interval. A paused duty keeps its
/// `next_due_at`, so resuming later does not instantly dump a backlog of runs.
pub fn set_sop_schedule_enabled(workspace_root: &Path, playbook_id: &str, enabled: bool) -> bool {
    let mut all = load_schedules(workspace_root);
    let mut hit = false;
    for s in all.iter_mut() {
        if s.playbook_id == playbook_id {
            s.enabled = enabled;
            hit = true;
        }
    }
    hit && save_schedules(workspace_root, &all)
}
/// Every armed cadence, in the order they were scheduled.
pub fn sop_schedules(workspace_root: &Path) -> Vec<SopSchedule> {
    load_schedules(workspace_root)
}
/// Enabled cadences whose `next_due_at` has arrived at `now`.
pub fn due_sops(workspace_root: &Path, now: u64) -> Vec<SopSchedule> {
    load_schedules(workspace_root)
        .into_iter()
        .filter(|s| s.enabled && s.next_due_at <= now)
        .collect()
}
/// Fire every due cadence through the gated playbook entry point. Returns one
/// `(playbook_id, outcome)` per duty that was due; nothing due means nothing run.
pub fn run_due_sops(
    workspace_root: &Path,
    org: &CompanyOrg,
    now: u64,
) -> Vec<(String, PlaybookOutcome)> {
    let mut fired = Vec::new();
    for s in due_sops(workspace_root, now) {
        // Advance BEFORE running: the same duty can never be in flight on two
        // ticks, and an SOP that halts on a verdict gate waits a full interval
        // instead of hammering the queue every tick.
        let mut all = load_schedules(workspace_root);
        for e in all.iter_mut() {
            if e.playbook_id == s.playbook_id {
                e.next_due_at = now.saturating_add(e.every_secs);
                e.last_run_at = now;
            }
        }
        save_schedules(workspace_root, &all);
        let outcome = match run_playbook(workspace_root, org, &s.playbook_id) {
            Ok(o) => o,
            // The playbook may have been removed since it was armed: report the
            // reason as a Denied outcome so the tick output stays uniform.            Err
            Err(reason) => PlaybookOutcome::Denied { reason },
        };
        fired.push((s.playbook_id.clone(), outcome));
    }
    fired
}
/// Render the cadences for the dashboard/MCP: what repeats, when it next fires.
pub fn describe_sop_schedules(entries: &[SopSchedule], now: u64) -> String {
    if entries.is_empty() {
        return "No recurring SOPs armed — every playbook waits for an explicit run.\n".to_string();
    }
    let mut out = format!("RECURRING SOPs ({} armed):\n", entries.len());
    for s in entries {
        let every = if s.every_secs % 60 == 0 {
            format!("{}min", s.every_secs / 60)
        } else {
            format!("{}s", s.every_secs)
        };
        let timing = if s.next_due_at <= now {
            format!("OVERDUE by {}s (fires on next tick)", now - s.next_due_at)
        } else {
            format!("due in {}s", s.next_due_at - now)
        };
        let last = if s.last_run_at == 0 {
            "never run".to_string()
        } else {
            format!("last ran at {}", s.last_run_at)
        };
        out.push_str(&format!(
            "  - '{}' every {every} | {timing} | {last} | {}\n",
            s.playbook_id,
            if s.enabled { "armed" } else { "PAUSED" }
        ));
    }
    out.push_str("  (a due cadence runs through the approval gate — never around it)\n");
    out
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Credential-vault rotation (Operations key-rotation primitive)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// The result of an approved rotation request: the pointer that was replaced
/// (never the raw secret) and the pointer that is now live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationApplied {
    pub resource_id: String,
    pub previous_ref: String,
    pub new_ref: String,
}
/// Outcome of [`propose_credential_rotation`]. Rotation on the credentials
/// meta-resource is dual-control by default (preset policy: CEO + CTO), so
/// most proposals return [`RotationResult::Pending`] and only seal after both
/// approvers sign off via `approve_org_action`. The `description` field on the
/// sealed ledger entry carries `"rotate:<resource_id>|<new_ref>"` so
/// [`execute_rotation_after_approval`] can replay it atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationResult {
    /// Gate cleared (`Allowed`) — pointer swapped in-memory.
    Sealed(RotationApplied),
    /// Gate held (`RequiresApproval`) — a Pending request was recorded and the
    /// caller must obtain `quorum` approvals before the swap takes effect.
    Pending {
        request_id: String,
        approvers: Vec<String>,
        quorum: usize,
    },
    /// Gate denied outright — role lacks the capability.
    Denied { reason: String },
}
/// Rotate a resource's `secret_ref` pointer through the credential-vault
/// gate. Never touches the raw secret — only its pointer label changes on
/// the org document, and the vault resolves the new value at run time. The
/// proposal is routed through [`check_permission`] on the `"credentials"`
/// meta-resource with [`Capability::Deploy`] (deny-by-default): ungranted
/// roles get `Denied`, and the preset wires a dual-control policy so a live
/// rotation needs both CEO and CTO before the swap seals.
pub fn propose_credential_rotation(
    workspace_root: &Path,
    org: &mut CompanyOrg,
    role_id: &str,
    resource_id: &str,
    new_secret_ref: &str,
) -> RotationResult {
    if resource_id == "credentials" {
        return RotationResult::Denied {
            reason: "the credentials meta-resource cannot self-rotate; rotate a concrete resource (banking/github/cloud/…)".to_string(),
}
;
    }
    if org.find_resource(resource_id).is_none() {
        return RotationResult::Denied {
            reason: format!("unknown resource '{resource_id}'"),
        };
    }
    if new_secret_ref.is_empty() {
        return RotationResult::Denied {
            reason: "new_secret_ref is empty — refusing to clear a live credential pointer"
                .to_string(),
        };
    }

    // Gate: Deploy on the credentials meta-resource, evaluated at amount 0.0
    // so the preset dual-control policy (threshold Some(0.0)) always fires.
    let decision = check_permission(org, role_id, "credentials", Capability::Deploy, 0.0);
    match decision {
        Decision::Denied { reason } => RotationResult::Denied { reason },
        Decision::RequiresApproval { .. } => {
            let desc = format!("rotate:{resource_id}|{new_secret_ref}");
            let req = submit_action(
                workspace_root,
                org,
                role_id,
                "credentials",
                Capability::Deploy,
                0.0,
                &desc,
            );
            match req.status {
                ActionStatus::Pending => RotationResult::Pending {
                    request_id: req.id,
                    approvers: req.required_approvers,
                    quorum: req.quorum.max(1),
                },
                ActionStatus::Rejected => RotationResult::Denied {
                    reason: req.decision_reason,
                },
                _ => RotationResult::Denied {
                    reason: format!("unexpected post-submit status: {:?}", req.status),
                },
            }
        }
        Decision::Allowed => match apply_rotation(org, resource_id, new_secret_ref) {
            Ok(applied) => RotationResult::Sealed(applied),
            Err(reason) => RotationResult::Denied { reason },
        },
    }
}
/// Execute a rotation whose approval request has already cleared. Mirrors
/// [`resume_playbook_after_approval`]: the request id is the proof, the
/// ledger entry is marked [`ActionStatus::Executed`] so the rotation cannot
/// be replayed, and the pointer swap is applied atomically. Returns
/// `Denied` if the request is unknown, not yet Approved, or was recorded
/// against a different resource than the caller is now passing.
pub fn execute_rotation_after_approval(
    workspace_root: &Path,
    org: &mut CompanyOrg,
    request_id: &str,
) -> Result<RotationApplied, String> {
    let mut all = load_requests(workspace_root);
    let idx = all
        .iter()
        .position(|r| r.id == request_id)
        .ok_or_else(|| format!("no request '{request_id}'"))?;
    if all[idx].status != ActionStatus::Approved {
        return Err(format!(
            "request '{request_id}' is {:?}, not Approved",
            all[idx].status
        ));
    }
    if all[idx].capability != Capability::Deploy || all[idx].resource_id != "credentials" {
        return Err(format!(
            "request '{request_id}' is not a credential rotation ({}/{:?})",
            all[idx].resource_id, all[idx].capability
        ));
    }
    let (rid, new_ref) = parse_rotation_payload(&all[idx].description)
        .ok_or_else(|| format!("request '{request_id}' has no valid rotation payload"))?;
    let applied = apply_rotation(org, &rid, &new_ref)?;
    all[idx].status = ActionStatus::Executed;
    save_requests(workspace_root, &all);
    Ok(applied)
}
fn parse_rotation_payload(desc: &str) -> Option<(String, String)> {
    let rest = desc.strip_prefix("rotate:")?;
    let (rid, nr) = rest.split_once('|')?;
    if rid.is_empty() || nr.is_empty() {
        return None;
    }
    Some((rid.to_string(), nr.to_string()))
}
fn apply_rotation(
    org: &mut CompanyOrg,
    resource_id: &str,
    new_secret_ref: &str,
) -> Result<RotationApplied, String> {
    let res = org
        .resources
        .iter_mut()
        .find(|r| r.id == resource_id)
        .ok_or_else(|| format!("unknown resource '{resource_id}'"))?;
    let previous_ref = std::mem::take(&mut res.secret_ref);
    res.secret_ref = new_secret_ref.to_string();
    Ok(RotationApplied {
        resource_id: resource_id.to_string(),
        previous_ref,
        new_ref: new_secret_ref.to_string(),
    })
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Credential vault resolution (secret_ref → scoped secret, zeroized after use)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// A live handle to one resolved secret. The raw value is reachable only
/// through [`ScopedSecret::expose`], is masked in logs and `Debug`, and is
/// zeroized when the handle is dropped (backed by
/// [`velocity_ide::credential_guard::CredentialScope`]).
//
//
// This is the enforcement point for "who has access to what": a secret is only
/// ever released when [`check_permission`] returns `Allowed` (or an action
/// request has already cleared to `Approved`). Gates and denials never see the
/// value, and it never lands in the org document — only the `secret_ref`
/// pointer is stored there.
pub struct ScopedSecret {
    scope: velocity_ide::credential_guard::CredentialScope,
    label: String,
    resource_name: String,
}
impl ScopedSecret {
    /// Transient access to the raw secret for the executor that must actually
    /// authenticate to the external system. Returns `None` for resources that
    /// need no credential (empty `secret_ref`).
    pub fn expose(&self) -> Option<&str> {
        self.scope.get(0).map(|s| s.as_str())
    }

    /// Log-safe masked preview (never the raw value).
    pub fn masked(&self) -> String {
        self.scope
            .get(0)
            .map(|s| s.masked())
            .unwrap_or_else(|| "(none)".to_string())
    }

    /// The resolved `secret_ref` pointer (env var name / keychain label).
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Human name of the resource the secret belongs to.
    pub fn resource_name(&self) -> &str {
        &self.resource_name
    }
}
impl std::fmt::Debug for ScopedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedSecret")
            .field("label", &self.label)
            .field("resource", &self.resource_name)
            .field("present", &self.scope.get(0).is_some())
            .finish()
    }
}
/// Why a credential could not be released to a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialBlocked {
    /// Role has no grant (or the resource is unknown) — deny by default.
    Denied { reason: String },
    /// The action must clear an approval gate first; the secret stays sealed.
    NeedsApproval {
        approvers: Vec<String>,
        quorum: usize,
        reason: String,
    },
    /// Permission is fine but the operator hasn't provisioned the secret.
    Missing { secret_ref: String },
}
impl CredentialBlocked {
    /// A single log-safe line describing the block (never the raw value).
    pub fn reason_line(&self) -> String {
        match self {
            CredentialBlocked::Denied {
 reason
}
 => format!("DENIED: {reason}"),            CredentialBlocked::NeedsApproval {
                approvers,                quorum,                reason,
}
 => {
                let who = if approvers.is_empty() {
                    HUMAN_OPERATOR.to_string()
}
 else {
                    approvers.join(", ")
}
;
                format!("NEEDS APPROVAL (quorum {quorum} of {who}): {reason}")
}
            CredentialBlocked::Missing {
 secret_ref
}
 => format!(                "MISSING: vault reference '{secret_ref}' is not provisioned; set the environment variable to grant access"            ),
}
    }
}
/// Load a resource's `secret_ref` into a zeroizing scope. An empty `secret_ref`
/// yields a valid handle with no secret (for no-credential resources).
fn load_secret_into_scope(
    secret_ref: &str,
    resource_name: &str,
) -> Result<ScopedSecret, CredentialBlocked> {
    let mut scope = velocity_ide::credential_guard::CredentialScope::new();
    if secret_ref.is_empty() {
        return Ok(ScopedSecret {
            scope,
            label: String::new(),
            resource_name: resource_name.to_string(),
        });
    }
    if scope.load_env(secret_ref).is_some() {
        Ok(ScopedSecret {
            scope,
            label: secret_ref.to_string(),
            resource_name: resource_name.to_string(),
        })
    } else {
        Err(CredentialBlocked::Missing {
            secret_ref: secret_ref.to_string(),
        })
    }
}
/// Resolve a credential for `role_id` performing `capability` on `resource_id`.
//
//
// The permission decision is computed first; only an `Allowed` decision
/// releases the secret. `RequiresApproval` and `Denied` keep the secret sealed,
/// so the vault itself is the enforcement boundary — an executor cannot read a
/// token for an action the gate has not cleared.
pub fn resolve_credential(
    org: &CompanyOrg,
    role_id: &str,
    resource_id: &str,
    capability: Capability,
    amount: f64,
) -> Result<ScopedSecret, CredentialBlocked> {
    let resource = match org.find_resource(resource_id) {
        Some(r) => r,
        None => {
            return Err(CredentialBlocked::Denied {
                reason: format!("unknown resource '{resource_id}'"),
            })
        }
    };
    match check_permission(org, role_id, resource_id, capability, amount) {
        Decision::Allowed => load_secret_into_scope(&resource.secret_ref, &resource.name),
        Decision::Denied { reason } => Err(CredentialBlocked::Denied { reason }),
        Decision::RequiresApproval {
            approvers,
            quorum,
            reason,
            ..
        } => Err(CredentialBlocked::NeedsApproval {
            approvers,
            quorum: quorum.max(1),
            reason,
        }),
    }
}
/// Resolve a credential for an *already-approved* action request. This is the
/// path an executor uses after the human/manager gate has cleared: the request
/// must be stored with [`ActionStatus::Approved`], otherwise the secret stays
/// sealed. The gate is trusted because the request id was produced by
/// [`submit_action`] and only advanced by [`approve_action`].
pub fn resolve_credential_for_request(
    org: &CompanyOrg,
    request: &ActionRequest,
) -> Result<ScopedSecret, CredentialBlocked> {
    if request.status != ActionStatus::Approved {
        return Err(CredentialBlocked::NeedsApproval {
            approvers: if request.required_approvers.is_empty() {
                vec![HUMAN_OPERATOR.to_string()]
            } else {
                request.required_approvers.clone()
            },
            quorum: request.quorum.max(1),
            reason: format!(
                "request '{}' is {:?}, not Approved",
                request.id, request.status
            ),
        });
    }
    let resource = match org.find_resource(&request.resource_id) {
        Some(r) => r,
        None => {
            return Err(CredentialBlocked::Denied {
                reason: format!("unknown resource '{}'", request.resource_id),
            })
        }
    };
    load_secret_into_scope(&resource.secret_ref, &resource.name)
}
/// High-value, irreversible capabilities must never run on a standing grant
/// alone. For these, an [`ActionStatus::Approved`] request is *additionally*
/// required to carry a live, single-use JIT elevation for the exact
/// (role, resource, capability) scope before the vault unseals. `Spend`
/// (banking) and `Deploy` (cloud, credential rotation) are the elevation-gated
/// capabilities; read-only and low-value Communicate/Write run on the standing
/// matrix as before. This is the enforcement half of the Phase 62 elevation
/// layer: the elevation is consulted for real, not kept as bookkeeping.
pub fn capability_requires_elevation(cap: Capability) -> bool {
    matches!(cap, Capability::Spend | Capability::Deploy)
}
/// Enforce the elevation factor for an already-Approved request. A no-op for
/// capabilities that don't require elevation. Otherwise it finds an active
/// elevation for the request's scope and *consumes* it (spending it for exactly
/// one execution), returning a secret-free [`CredentialBlocked`] when there is
/// no live elevation to spend (never minted, already spent, or expired).
/// Consulted by the request-gated executor BEFORE any browser session or secret
/// release, so an approved-but-unelevated irreversible action cannot even be
/// rehearsed against the live vault.
pub fn ensure_elevation_cleared(
    workspace_root: &Path,
    request: &ActionRequest,
    now: u64,
) -> Result<(), CredentialBlocked> {
    if !capability_requires_elevation(request.capability) {
        return Ok(());
    }
    let grant = find_active_elevation(        workspace_root,        &request.role_id,        &request.resource_id,        request.capability,        now,    )    .ok_or_else(|| CredentialBlocked::Denied {
        reason: format!(            "capability '{}' on '{}' is elevation-gated: request '{}' is Approved but has no active JIT elevation — mint one with grant_org_credential_elevation before executing",            request.capability.as_str(),            request.resource_id,            request.id,        ),
}
)?;
    consume_jit_elevation(workspace_root, &grant.id, now).map_err(|e| {
        CredentialBlocked::Denied {
            reason: format!("elevation '{}' could not be spent: {e}", grant.id),
        }
    })?;
    Ok(())
}
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Cross-platform browser task planning (gated + secret-free)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
/// Sentinel prefix used inside a browser plan to reference a vault pointer
/// rather than a literal secret. When a live browser session executes the plan,
/// a value of the form `vault:SECRET_REF` is resolved through
/// [`resolve_credential`] *at run time*; the plan document itself never carries
/// the secret, so it is safe to persist, diff and log.
pub const VAULT_VALUE_PREFIX: &str = "vault:";
/// A single browser action in a plan: the runtime tool to call and its
/// arguments. Secret-bearing fields (passwords, message bodies authored from a
/// template) use the [`VAULT_VALUE_PREFIX`] sentinel, never a literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserAction {
    /// Registered browser tool name, e.g. `runtime_session_navigate`.
    pub tool: String,
    pub args: serde_json::Value,
}
/// A deterministic, gated, secret-free plan for driving the browser on behalf of
/// an org role: log into a platform and perform one outward action (post a
/// status, send a message). Produced only after [`check_permission`] clears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserLoginPlan {
    pub role_id: String,
    pub resource_id: String,
    pub capability: Capability,
    /// The vault pointer the executor must resolve for the password field.
    pub secret_ref: String,
    pub actions: Vec<BrowserAction>,
}
impl BrowserLoginPlan {
    /// A log-safe rendering: every `vault:REF` value is masked to its pointer
    /// (the pointer is not a secret; the resolved value never appears here).
    pub fn describe(&self) -> String {
        let mut out = format!(
            "Browser plan for '{}' -> {} '{}' (secret via {})\n",
            self.role_id,
            self.capability.as_str(),
            self.resource_id,
            if self.secret_ref.is_empty() {
                "(none)"
            } else {
                &self.secret_ref
            }
        );
        for (i, a) in self.actions.iter().enumerate() {
            out.push_str(&format!("  {}. {} {}\n", i + 1, a.tool, a.args));
        }
        out
    }
}
/// Inputs describing a cross-platform login + one outward action.
#[derive(Debug, Clone)]
pub struct BrowserTaskSpec<'a> {
    pub role_id: &'a str,
    pub resource_id: &'a str,
    pub capability: Capability,
    pub amount: f64,
    pub login_url: &'a str,
    pub username_selector: &'a str,
    pub username: &'a str,
    pub password_selector: &'a str,
    pub submit_selector: &'a str,
    /// Optional composer URL + selector for the outward action (e.g. a post box).
    pub composer_url: Option<&'a str>,
    pub composer_selector: Option<&'a str>,
    /// The message/status text (non-secret marketing copy). Empty skips the step.
    pub composer_text: &'a str,
    /// Selector of the composer's publish/send control. When set, a
    /// `runtime_session_click` fires on it after the text is filled — the only
    /// reliable way to post on platforms whose send button is not a form submit
    /// (e.g. X's `[data-testid='tweetButton']`). Empty falls back to
    /// `runtime_session_submit`.
    pub composer_submit_selector: &'a str,
    /// Optional JavaScript to evaluate on the logged-in page via
    /// `runtime_session_evaluate`, extracting inbound content (a support ticket,
    /// a chat thread, an analytics panel). Read-only: the returned text is
    /// surfaced on [`BrowserRun::extracted`], never in the log-safe rendering.
    pub read_script: Option<&'a str>,
    /// When true, append a `runtime_capture_session` step to snapshot the current
    /// page into the browser artifact model — how the Testing team records a demo
    /// for Marketing. Read-only, no secret needed.
    pub capture_demo: bool,
    /// Ordered extra steps spliced in *after* login and *before* the
    /// composer/read/capture block — how a Support agent clicks into a specific
    /// ticket, a tester opens a feature tab before capturing a demo, or a scout
    /// expands "show more" before reading. Each entry is a registered runtime
    /// browser tool (`runtime_session_click`, `runtime_session_navigate`,
    /// `runtime_session_press_key`, …) with its `{{sessionId}}`-templated args.
    pub extra_actions: Vec<BrowserAction>,
    /// Post-action settle timeout (ms) attached to every interactive step when
    /// non-zero, so async pages render before the next action fires. 0 = no wait.
    pub settle_ms: u64,
    /// Deterministic acceptance check: a required substring the read/extract
    /// content MUST contain for the run to be considered verified. Empty = no
    /// check. This is how the Testing team confirms "the patch works / the page    /// rendered X" without an inference call — a plain containment assertion on
    /// whatever [`BrowserRun::extraction`] returned.
    pub verify_expect: &'a str,
    /// Deterministic regression guard: a substring that MUST NOT appear in the
    /// read/extract content for the run to be considered clean. Empty = no check.
    /// This is how the Testing team catches a broken page ("Internal Server    /// Error", "undefined", "NaN") without an inference call — the mirror of
    /// [`Self::verify_expect`].
    pub verify_absent: &'a str,
    /// Pagination crawl: selector of the "next page" control. When non-empty
    /// *and* [`Self::max_pages`] > 1, the `read_script` is evaluated once per
    /// page, clicking this selector between pages to walk a ticket queue, a
    /// social feed, or a paged admin table. Deterministic and bounded — no
    /// inference, no unbounded loop. Empty = single-page read (back-compat).
    pub next_selector: &'a str,
    /// Upper bound on pages for a pagination crawl (see [`Self::next_selector`]).
    /// `0` or `1` = read the current page only. The crawl runs `max_pages`
    /// reads with `next_selector` clicks interleaved, so cost is predictable.
    pub max_pages: usize,
    /// Multi-URL read crawl: a fixed list of pages to visit and read in one
    /// session (a scout sweeping N competitor changelogs, marketing verifying N
    /// campaign posts landed). For each URL the plan emits a
    /// `runtime_session_navigate` then a `runtime_session_evaluate` of the
    /// `read_script`, each read labelled by its URL so the extraction is
    /// attributable. Takes precedence over [`Self::next_selector`] when set.
    pub read_urls: Vec<String>,
    /// Structured form-fill: an ordered list of `(selector, value)` pairs the
    /// planner turns into `runtime_session_fill` steps *after* login and *before*
    /// the composer/read block. Marketing campaign/ad-platform forms and Support
    /// ticket-creation screens have many fields; authoring them as a declarative
    /// list (rather than a hand-built `extra_actions` fill per field) keeps the
    /// SOP deterministic and compact. Values are non-secret copy or
    /// [`VAULT_VALUE_PREFIX`] pointers resolved at run time by drive_plan.
    pub form_fields: Vec<(String, String)>,
    /// Deterministic wait: selector to poll until visible before proceeding to
    /// form/composer/read. More robust than blind `settle_ms` on SPAs — the plan
    /// advances as soon as the element appears. Empty = no wait step. Timeout
    /// inherits `settle_ms` (or 5000 ms default) so slow pages do not fail spuriously.
    pub wait_for_selector: &'a str,
    /// Scroll-to-element: selector the browser scrolls into view after the
    /// optional `wait_for_selector` step and before interactive fills/clicks.
    /// Needed on long pages (analytics dashboards, social feeds) where the
    /// target element is below the fold. Empty = no scroll step.
    pub scroll_to: &'a str,
    /// When true, emit a `runtime_session_switch_tab` step (position: "latest")
    /// after wait/scroll — used when an extra_action click opens a new tab
    /// (e.g. a GitHub PR "Open in new tab", a support ticket in a new window)
    /// and subsequent fills/reads must target that new tab.
    pub switch_tab: bool,
    /// Drag-and-drop source selector: when both `drag_from` and `drag_to` are
    /// non-empty, emit a `runtime_session_drag` step. This is how the CTO or
    /// dev_lead moves a card on a kanban board (Backlog → In Progress),
    /// reorders a sprint backlog, or drags a file into an upload zone.
    pub drag_from: &'a str,
    /// Drag-and-drop target selector (drop zone).
    pub drag_to: &'a str,
    /// Wait-for-navigation: URL pattern the page must match before proceeding.
    /// After login/form submit, this confirms the redirect landed (e.g. "/dashboard"
    /// or "/confirmation"). Emits `runtime_session_wait_url {sessionId, urlPattern, timeout}`.
    /// Empty = no URL wait. Timeout inherits `settle_ms` (or 8000 ms default).
    pub wait_for_url: &'a str,
    /// Right-click (context menu): selector to right-click after form_fields and
    /// before composer/read — e.g. open a file manager context menu, copy a cell
    /// value. Emits `runtime_session_right_click {sessionId, selector}`. Empty = skip.
    pub right_click_selector: &'a str,
    /// Select a dropdown by visible option text: (selector, option_text) pair
    /// emitted after form_fields — for custom dropdowns (React-Select, Headless UI)
    /// that don't respond to a simple fill. Emits `runtime_session_select {sessionId, selector, optionText}`.
    /// Empty selector = skip.
    pub select_option: (&'a str, &'a str),
    /// Form submit: selector of the submit button clicked *after* form_fields and
    /// select_option, closing the multi-field fill → submit loop for standalone
    /// forms (Support ticket creation, ad-campaign setup, expense claims). This
    /// is distinct from the composer block, which posts text via its own send
    /// control. Emits `runtime_session_click {sessionId, selector}`. Empty = skip
    /// (a form that auto-submits, or one finished by the composer path).
    pub form_submit_selector: &'a str,
    /// Deterministic text-readiness wait: a fragment that MUST appear in the
    /// page body before the next step fires. Complements [`Self::wait_for_selector`]
    /// (element presence) with *content* presence — how a Support agent waits for
    /// "Ticket #1234 loaded" or a scout waits for a rendered price, on async SPAs
    /// where the node exists before its data streams in. Emits
    /// `runtime_session_wait_text {sessionId, text, timeout}`. Empty = skip.
    /// Timeout inherits `settle_ms` (or 6000 ms default).
    pub wait_for_text: &'a str,
    /// Hover (mouse-enter) selector: bring up a dropdown/menu/tooltip before a
    /// subsequent fill/click/read targets its contents. Emits
    /// `runtime_session_hover {sessionId, selector}`. Empty = skip.
    pub hover_selector: &'a str,
    /// File upload: a `(selector, paths)` pair — the `<input type=file>` (or drop
    /// zone) selector and the workspace-relative file paths to attach. Emits
    /// `runtime_session_upload {sessionId, selector, files}` after form_fields and
    /// before select/form-submit. This is how Support attaches a crash log to a
    /// ticket, Marketing uploads a demo image to a campaign, or Testing attaches a
    /// recorded artifact. Empty selector or no paths = skip.
    pub upload: (&'a str, Vec<String>),
    /// Element attribute/text extract: a `(selector, attribute)` pair the planner
    /// turns into a *generated* `runtime_session_evaluate` (no hand-written JS).
    /// Reads one element's attribute (or, for an empty attribute, its text/value) —
    /// how Marketing confirms the permalink of a posted announcement, Support reads
    /// a ticket status, Finance reads an invoice number. The result is attributed to
    /// `selector=attribute` on [`BrowserRun::extracted`]. Empty selector = skip.
    pub extract_attribute: (&'a str, &'a str),
    /// Per-element screenshot: selector captured as visual evidence (a rendered
    /// chart, a UI regression, a campaign placement). Emits
    /// `runtime_session_screenshot {sessionId, selector}` near the demo-capture
    /// step. Empty = skip.
    pub screenshot_selector: &'a str,
    /// Full-viewport screenshot: when true, emit an additional
    /// `runtime_session_screenshot {sessionId, full: true}` (no selector) so the
    /// whole rendered page is captured as a single visual artifact — the shape
    /// a legal audit trail or a marketing campaign-placement review wants.
    /// Independent of [`Self::screenshot_selector`]; both may fire on the same run.
    pub screenshot_full_page: bool,
    /// Frame/iframe entry: selector of an `<iframe>` (or `<frame>`) to switch
    /// the driver into *before* any other interactive step (except login).
    /// Required on banking 3-D-Secure challenges, embedded payment sheets,
    /// admin dashboards and most enterprise portals whose interactive content
    /// lives inside a cross-origin frame — without this, every subsequent
    /// selector resolves against the outer document and misses the target.
    /// Emits `runtime_session_enter_frame {sessionId, selector}`. Empty = skip.
    pub frame_selector: &'a str,
    /// Deterministic value capture + later-step interpolation. Each `(name,    /// selector)` pair emits a generated `runtime_session_evaluate` that reads
    /// the element's value/text and stores it under `name` in the plan's
    /// runtime variable map. Any subsequent step's args may reference it as
    /// `{{vars.NAME}}` — drive_plan substitutes the live value right before
    /// dispatch. This is how one browser run reads an invoice ID from a list
    /// row, then clicks *that specific* row's "Pay" button, without any AI
    /// re-inference between steps. Empty list = no capture.
    pub captures: Vec<(String, String)>,
    /// JS dialog auto-handling: when a script fires `window.alert()`,
    /// `window.confirm()` or `window.prompt()` (common on legacy banking
    /// 3-D-Secure flows, delete/undo dialogs, admin panels), the driver blocks
    /// on a native modal that no CSS selector can reach — every subsequent
    /// step would silently time out. Setting `dialog_action` to `"accept"` or
    /// `"dismiss"` emits a `runtime_session_dialog {sessionId, action,    /// promptText?}` step immediately after frame entry so the modal is cleared
    /// deterministically. Empty/`"none"` = skip (default). `dialog_prompt_text`
    /// supplies the input value for `prompt()` dialogs (leave empty for
    /// `alert`/`confirm`).
    pub dialog_action: &'a str,
    /// Optional text to feed a `window.prompt()` dialog when
    /// [`Self::dialog_action`] is `"accept"`. Ignored for `dismiss` and empty
    /// dialogs. Empty = no promptText field emitted.
    pub dialog_prompt_text: &'a str,
    /// Download-triggering selector: clicking this element starts a file
    /// download (e.g. "Export CSV", "Download build artifact", bank statement
    /// link). Emits `runtime_session_download {sessionId, selector, dir,    /// timeoutMs}` as the last interactive step. Empty = no download.
    pub download_selector: &'a str,
    /// Directory where the downloaded file is saved on disk. Empty =
    /// driver-default (usually the browser's temp download folder).
    pub download_dir: &'a str,
    /// Maximum time (ms) to wait for the download to begin after the
    /// triggering click. 0 = driver default. Useful on slow CI artifact pages
    /// or large report exports that take time to generate.
    pub download_timeout_ms: u64,
    /// Wait for network idle: after navigation/login, emit a
    /// `runtime_session_network_idle` step so the driver blocks until no XHR /
    /// fetch requests are in-flight (SPA dashboard, analytics panel). Uses
    /// `settle_ms` as the timeout (default 5000 if unset). false = skip.
    pub wait_for_network_idle: bool,
    /// Custom HTTP request headers injected into the session context *before*
    /// the first navigation. Each (name, value) pair becomes a
    /// `runtime_session_set_headers` step — useful for feature flags, tenant
    /// isolation headers, API tokens for header-auth endpoints. Empty = no
    /// header step emitted.
    pub request_headers: Vec<(String, String)>,
    /// Session bootstrap via cookies: each (name, value) pair is injected as a
    /// `runtime_session_set_cookie` step *before* the first navigation, so a
    /// token-auth system (a session cookie, an SSO ticket) is reachable without
    /// driving a password form. Values come from the vault pointer at run time —
    /// the plan itself stays secret-free. Empty = no cookie step emitted.
    pub cookies: Vec<(String, String)>,
    /// Session bootstrap via localStorage: each (key, value) pair is injected as
    /// a `runtime_session_set_storage` step *before* navigation — the idiom SPA
    /// dashboards use to read an auth token from local storage. Empty = no
    /// storage step emitted.
    pub local_storage: Vec<(String, String)>,
    /// HTTP basic-auth realm credential: when non-empty a
    /// `runtime_session_basic_auth {sessionId, username, password}` step is
    /// emitted before navigation, so a basic-auth protected system (an admin
    /// console, a staging site) is reachable. The password is a vault pointer at
    /// run time, so the plan stays secret-free. Empty = no basic-auth step.
    pub basic_auth_user: &'a str,
    /// Password for [`Self::basic_auth_user`]. Empty = no basic-auth step.
    pub basic_auth_pass: &'a str,
    /// User-agent override for device/compatibility testing: a
    /// `runtime_session_set_user_agent` step is emitted before navigation so QA
    /// can exercise a page as a specific browser/robot. Empty = no override.
    pub user_agent: &'a str,
    /// Viewport width for responsive/device emulation testing (0 = no override).
    /// Emitted together with [`Self::viewport_height`] as a single
    /// `runtime_session_set_viewport` step before navigation.
    pub viewport_width: u32,
    /// Viewport height for responsive/device emulation testing (0 = no override).
    pub viewport_height: u32,
}
/// Build a gated, secret-free browser plan for a role. The approval gate is
/// enforced FIRST: if the action is denied or awaits approval, no plan (and no
/// credential) is produced, so an ungated role can never obtain a runnable
/// cross-platform automation. On `Allowed`, the emitted plan references the
/// credential only by pointer via [`VAULT_VALUE_PREFIX`].
pub fn plan_org_browser_login(
    org: &CompanyOrg,
    spec: &BrowserTaskSpec,
) -> Result<BrowserLoginPlan, CredentialBlocked> {
    let resource =
        org.find_resource(spec.resource_id)
            .ok_or_else(|| CredentialBlocked::Denied {
                reason: format!("unknown resource '{}'", spec.resource_id),
            })?;

    // Gate: only an Allowed decision yields a runnable plan. RequiresApproval and
    // Denied leave the browser (and the vault) untouched.
    match check_permission(
        org,
        spec.role_id,
        spec.resource_id,
        spec.capability,
        spec.amount,
    ) {
        Decision::Denied { reason } => return Err(CredentialBlocked::Denied { reason }),
        Decision::RequiresApproval {
            approvers,
            quorum,
            reason,
            ..
        } => {
            return Err(CredentialBlocked::NeedsApproval {
                approvers,
                quorum: quorum.max(1),
                reason,
            })
        }
        Decision::Allowed => {}
    }
    Ok(assemble_login_plan(resource, spec))
}
/// Build the ordered browser action list for a spec. The password is emitted as
/// a vault POINTER (never a literal), so the plan is secret-free and safe to
/// persist or log. This performs NO permission check — callers gate first
/// ([`plan_org_browser_login`] or the approved-request path in
/// [`execute_org_browser_plan_for_request`]).
fn assemble_login_plan(resource: &Resource, spec: &BrowserTaskSpec) -> BrowserLoginPlan {
    // Build a fresh session id placeholder; the executor substitutes the real id
    // after `runtime_create_session` returns.
    let sid = "{{sessionId}}".to_string();
    let mut actions = Vec::new();

    // Custom request headers are set BEFORE navigation so they're present on
    // the very first request (feature flags, API tokens, tenant isolation).
    if !spec.request_headers.is_empty() {
        let hdr_map: serde_json::Map<String, serde_json::Value> = spec
            .request_headers
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::json!(v)))
            .collect();
        actions.push(BrowserAction {
            tool: "runtime_session_set_headers".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "headers": serde_json::Value::Object(hdr_map)
            }
            ),
        });
    }

    // Session-bootstrap cookies are injected BEFORE navigation so the very first
    // request carries the auth cookie / SSO ticket (token-auth systems reachable
    // without driving a password form).
    for (name, value) in &spec.cookies {
        actions.push(BrowserAction {
            tool: "runtime_session_set_cookie".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "name": name, "value": value
            }
            ),
        });
    }

    // localStorage tokens are likewise set before navigation — the idiom SPA
    // dashboards use to read an auth token from local storage on boot.
    for (key, value) in &spec.local_storage {
        actions.push(BrowserAction {
            tool: "runtime_session_set_storage".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "key": key, "value": value
            }
            ),
        });
    }

    // HTTP basic-auth realm credential set before navigation so the first request
    // authenticates a basic-auth protected system (admin console, staging site).
    if !spec.basic_auth_user.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_basic_auth".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "username": spec.basic_auth_user, "password": spec.basic_auth_pass
            }
            ),
        });
    }

    // Device/compatibility overrides for QA: user-agent and viewport are applied
    // before navigation so the page renders under the emulated profile.
    if !spec.user_agent.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_set_user_agent".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "userAgent": spec.user_agent
            }
            ),
        });
    }
    if spec.viewport_width > 0 && spec.viewport_height > 0 {
        actions.push(BrowserAction {
            tool: "runtime_session_set_viewport".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "width": spec.viewport_width, "height": spec.viewport_height
            }
            ),
        });
    }
    actions.push(BrowserAction {
        tool: "runtime_session_navigate".to_string(),
        args: serde_json::json!({
         "sessionId": sid, "url": spec.login_url
        }
        ),
    });
    if !spec.username_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_fill".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": spec.username_selector,                "value": spec.username,                "clear": true
}
),
}
);
    }
    if !spec.password_selector.is_empty() {
        // The password is a vault POINTER, never the literal secret.
        let pointer = if resource.secret_ref.is_empty() {
            String::new()
        } else {
            format!("{VAULT_VALUE_PREFIX}{}", resource.secret_ref)
        };
        actions.push(BrowserAction {
            tool: "runtime_session_fill".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": spec.password_selector,                "value": pointer,                "clear": true
}
),
}
);
    }

    // Submit the login form only when there *is* one. A pure read/capture against
    // a public page (no username/password/submit selectors) must not emit a stray
    // submit — it would target nothing and just add a failing step to the run.
    let has_login_form = !spec.username_selector.is_empty() || !spec.password_selector.is_empty();
    if has_login_form || !spec.submit_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_submit".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": if spec.submit_selector.is_empty() {
 serde_json::Value::Null
}
 else {
 serde_json::Value::String(spec.submit_selector.to_string())
}

}
),
}
);
    }

    // Network-idle wait: after login, block until no XHR/fetch are in-flight.
    // This is the SPA equivalent of settle_ms — more precise because it
    // resolves as soon as the page is truly done loading (not after a fixed
    // timeout). Uses settle_ms as the upper bound (default 5000).
    if spec.wait_for_network_idle {
        let timeout = if spec.settle_ms > 0 {
            spec.settle_ms
        } else {
            5000
        };
        actions.push(BrowserAction {
            tool: "runtime_session_network_idle".to_string(),
            args: serde_json::json!({
            "sessionId": sid, "timeoutMs": timeout
            }
            ),
        });
    }

    // Ordered caller-supplied steps (click/navigate/press) run once we are logged
    // in — e.g. click into a specific ticket or open a feature tab. `sessionId`
    // is normalized to the placeholder so drive_plan binds the live id at run.
    // Value captures run *before* extra_actions so a later click/fill/read can
    // reference `{{vars.NAME}}` in its args (drive_plan substitutes right before
    // dispatch — see [`substitute_captured_vars`]). Each pair emits a generated,
    // quote-safe `runtime_session_evaluate` marked with `captureAs`.
    //
    // Frame entry runs BEFORE captures so a capture selector can target an
    // element inside the iframe (banking 3-D-Secure, embedded payment sheets,
    // admin dashboards). Empty selector = stay on the outer document.
    if !spec.frame_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_enter_frame".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "selector": spec.frame_selector,
            }
            ),
        });
    }

    // JS dialogs (alert/confirm/prompt) block the driver on a native modal no
    // CSS selector can reach — clear one proactively right after frame entry
    // when the SOP declares a `dialog_action`. `dismiss` cancels the modal
    // (equivalent to Cancel), `accept` confirms it (with `promptText` when a
    // `window.prompt()` needs input). Empty / `"none"` = skip.
    if !spec.dialog_action.is_empty() && spec.dialog_action != "none" {
        let mut args = serde_json::json!({
                    "sessionId": sid,            "action": spec.dialog_action,
        }
        );
        if !spec.dialog_prompt_text.is_empty() {
            args["promptText"] = serde_json::json!(spec.dialog_prompt_text);
        }
        actions.push(BrowserAction {
            tool: "runtime_session_dialog".to_string(),
            args,
        });
    }
    for (var_name, sel_str) in &spec.captures {
        if var_name.is_empty() || sel_str.is_empty() {
            continue;
        }
        let sel = serde_json::Value::String(sel_str.clone()).to_string();
        let script = format!(            "(()=>{{const e=document.querySelector({sel});return e?(e.value??e.innerText??e.textContent??''):'';}})()"        );
        actions.push(BrowserAction {
            tool: "runtime_session_evaluate".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "script": script,                "label": format!("capture:{var_name}"),                "captureAs": var_name,
}
),
}
);
    }
    for a in &spec.extra_actions {
        let mut args = a.args.clone();
        if let Some(obj) = args.as_object_mut() {
            let needs_sid = obj
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(|s| s.is_empty() || s == "{{sessionId}}")
                .unwrap_or(true);
            if needs_sid {
                obj.insert(
                    "sessionId".to_string(),
                    serde_json::Value::String(sid.clone()),
                );
            }
        }
        actions.push(BrowserAction {
            tool: a.tool.clone(),
            args,
        });
    }

    // Deterministic wait-for-element: poll until the selector is visible,
    // advancing as soon as it appears (more robust than blind settle_ms).
    if !spec.wait_for_selector.is_empty() {
        let timeout = if spec.settle_ms > 0 {
            spec.settle_ms
        } else {
            5000
        };
        actions.push(BrowserAction {
            tool: "runtime_session_wait".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": spec.wait_for_selector,                "timeout": timeout
}
),
}
);
    }

    // Deterministic wait-for-text: poll the rendered body until a content fragment
    // appears. Where wait_for_selector confirms the node exists, this confirms its
    // data streamed in — the readiness signal a Support/scout SOP needs on an
    // async SPA before reading.
    if !spec.wait_for_text.is_empty() {
        let timeout = if spec.settle_ms > 0 {
            spec.settle_ms
        } else {
            6000
        };
        actions.push(BrowserAction {
            tool: "runtime_session_wait_text".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "text": spec.wait_for_text,                "timeout": timeout
}
),
}
);
    }

    // Scroll-to-element: bring the target into view before interacting (long
    // dashboards, social feeds where the CTA is below the fold).
    if !spec.scroll_to.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_scroll".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "selector": spec.scroll_to
            }
            ),
        });
    }

    // Wait-for-navigation: confirm the page URL changed to a target pattern after
    // a submit or click-triggered navigation. More reliable than settle_ms alone.
    if !spec.wait_for_url.is_empty() {
        let timeout = if spec.settle_ms > 0 {
            spec.settle_ms
        } else {
            8000
        };
        actions.push(BrowserAction {
            tool: "runtime_session_wait_url".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "urlPattern": spec.wait_for_url,                "timeout": timeout
}
),
}
);
    }

    // Switch-to-newest-tab: when an extra_action click opened a new tab,
    // subsequent steps must target it. Emits one switch step.
    if spec.switch_tab {
        actions.push(BrowserAction {
            tool: "runtime_session_switch_tab".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "position": "latest"
            }
            ),
        });
    }

    // Drag-and-drop: move a source element to a target drop zone (kanban card
    // reordering, file upload, sprint board column moves). Only emitted when both
    // selectors are provided.
    if !spec.drag_from.is_empty() && !spec.drag_to.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_drag".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "sourceSelector": spec.drag_from,                "targetSelector": spec.drag_to
}
),
}
);
    }

    // Hover (mouse-enter): reveal a dropdown/menu/tooltip so a subsequent
    // fill/click/read can target its contents. Emitted after drag and before
    // right-click — both are element-scoped mouse interactions.
    if !spec.hover_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_hover".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "selector": spec.hover_selector
            }
            ),
        });
    }

    // Right-click (context menu): open a context menu on the target element before
    // form fill — used for copy/download/rename in file managers, data grids, etc.
    if !spec.right_click_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_right_click".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "selector": spec.right_click_selector
            }
            ),
        });
    }

    // Declarative form-fill: turn each (selector, value) into a clear-fill step,
    // in order, after login/extra actions and before composer/read. A value that
    // is a `vault:REF` pointer is resolved at run time by drive_plan (never a
    // literal secret in the plan).
    for (selector, value) in &spec.form_fields {
        if selector.is_empty() {
            continue;
        }
        actions.push(BrowserAction {
            tool: "runtime_session_fill".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": selector,                "value": value,                "clear": true
}
),
}
);
    }

    // File upload: attach workspace files to a `<input type=file>` / drop zone
    // after the text fields are filled and before the dropdown/submit — the order
    // a Support/Marketing/Testing form is actually completed in. Only emitted when
    // both a selector and at least one path are present.
    if !spec.upload.0.is_empty() && !spec.upload.1.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_upload".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": spec.upload.0,                "files": spec.upload.1
}
),
}
);
    }

    // Select dropdown by visible option text: after form fields, handles custom
    // dropdowns (React-Select, Headless UI) that don't respond to fill alone.
    if !spec.select_option.0.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_select".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "selector": spec.select_option.0,                "optionText": spec.select_option.1
}
),
}
);
    }

    // Form submit: click the submit button after the multi-field fill + select
    // sequence, closing the fill → submit loop for standalone forms (Support
    // ticket creation, ad-campaign setup, expense claims). Separate from the
    // composer block, which posts text via its own send control.
    if !spec.form_submit_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_click".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "selector": spec.form_submit_selector
            }
            ),
        });
    }

    // Composer block fires whenever a composer field is targeted and there is
    // copy to post. `composer_url` is optional: inline composers (e.g. X's home
    // timeline box) are filled on the current page after login, while a platform
    // with a dedicated compose page navigates there first.
    if let Some(csel) = spec.composer_selector {
        if !spec.composer_text.is_empty() {
            if let Some(curl) = spec.composer_url {
                actions.push(BrowserAction {
                    tool: "runtime_session_navigate".to_string(),
                    args: serde_json::json!({
                     "sessionId": sid, "url": curl
                    }
                    ),
                });
            }
            actions.push(BrowserAction {
                tool: "runtime_session_fill".to_string(),                args: serde_json::json!({
                    "sessionId": sid,                    "selector": csel,                    "value": spec.composer_text,                    "clear": true
}
),
}
);
            if spec.composer_submit_selector.is_empty() {
                actions.push(BrowserAction {
                    tool: "runtime_session_submit".to_string(),
                    args: serde_json::json!({
                     "sessionId": sid
                    }
                    ),
                });
            } else {
                actions.push(BrowserAction {
                    tool: "runtime_session_click".to_string(),                    args: serde_json::json!({
                        "sessionId": sid,                        "selector": spec.composer_submit_selector
}
),
}
);
            }
        }
    }

    // Element attribute/text extract: a generated, selector-safe read (no
    // hand-written JS). The selector/attribute are embedded as JSON string
    // literals, which are also valid, correctly-escaped JS string literals, so a
    // selector containing quotes (input[name="x"]) cannot break out of the script.
    if !spec.extract_attribute.0.is_empty() {
        let sel = serde_json::Value::String(spec.extract_attribute.0.to_string()).to_string();
        let attr = serde_json::Value::String(spec.extract_attribute.1.to_string()).to_string();
        let script = format!(            "(()=>{{const e=document.querySelector({sel});return e?(e.getAttribute({attr})??e.innerText??e.value??''):'';}})()"        );
        actions.push(BrowserAction {
            tool: "runtime_session_evaluate".to_string(),            args: serde_json::json!({
                "sessionId": sid,                "script": script,                "label": format!("{}={}", spec.extract_attribute.0, spec.extract_attribute.1)
}
),
}
);
    }

    // Value captures were emitted earlier (right after the login block) so
    // every subsequent step in this plan may reference `{{vars.NAME}}`.
    // Read/extract step: evaluate a script on the logged-in page to pull inbound
    // content (support ticket, chat thread, analytics). Read-only, no secret.
    // With a `next_selector` and `max_pages` > 1 this becomes a bounded,
    // deterministic pagination crawl: read the current page, click "next", read
    // again — up to `max_pages` times. A missing/failed next click is logged,
    // not fatal, so the crawl naturally stops at the last page.
    if let Some(script) = spec.read_script {
        if !script.is_empty() {
            if !spec.read_urls.is_empty() {
                // Multi-URL crawl: visit each page and read it in one session.
                // Each read is labelled by its URL so the extraction is
                // attributable to the source page. Deterministic, bounded by the
                // list length — no inference, no unbounded navigation.
                for url in &spec.read_urls {
                    if url.is_empty() {
                        continue;
                    }
                    actions.push(BrowserAction {
                        tool: "runtime_session_navigate".to_string(),
                        args: serde_json::json!({
                         "sessionId": sid, "url": url
                        }
                        ),
                    });
                    actions.push(BrowserAction {
                        tool: "runtime_session_evaluate".to_string(),
                        args: serde_json::json!({
                         "sessionId": sid, "script": script, "label": url.clone()
                        }
                        ),
                    });
                }
            } else {
                let pages = if spec.next_selector.is_empty() {
                    1
                } else {
                    spec.max_pages.max(1)
                };
                for page in 0..pages {
                    actions.push(BrowserAction {
                        tool: "runtime_session_evaluate".to_string(),
                        args: serde_json::json!({
                         "sessionId": sid, "script": script, "label": format!("page {}", page + 1)
                        }
                        ),
                    });

                    // Click "next" between pages, but never after the final read.
                    if !spec.next_selector.is_empty() && page + 1 < pages {
                        actions.push(BrowserAction {
                            tool: "runtime_session_click".to_string(),                            args: serde_json::json!({
                                "sessionId": sid,                                "selector": spec.next_selector
}
),
}
);
                    }
                }
            }
        }
    }

    // Capture step: snapshot the current page into the artifact model — the
    // Testing team's demo recording for Marketing. Read-only, no secret.
    if spec.capture_demo {
        actions.push(BrowserAction {
            tool: "runtime_capture_session".to_string(),
            args: serde_json::json!({
             "sessionId": sid
            }
            ),
        });
    }

    // Per-element screenshot: capture one region as visual evidence (a rendered
    // chart, a UI regression, a campaign placement) — narrower than a full-page
    // demo capture. Emitted after the whole-page capture so both coexist.
    if !spec.screenshot_selector.is_empty() {
        actions.push(BrowserAction {
            tool: "runtime_session_screenshot".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "selector": spec.screenshot_selector
            }
            ),
        });
    }

    // Full-viewport screenshot (audit trail / campaign-placement review): the
    // same tool with `full: true` and no selector — the runtime captures the
    // entire rendered page. Independent of `screenshot_selector` so an SOP can
    // grab both a targeted chart AND the whole page in one gated run.
    if spec.screenshot_full_page {
        actions.push(BrowserAction {
            tool: "runtime_session_screenshot".to_string(),
            args: serde_json::json!({
                            "sessionId": sid,                "full": true
            }
            ),
        });
    }
    if !spec.download_selector.is_empty() {
        let mut da = serde_json::json!({
                    "sessionId": sid,            "selector": spec.download_selector,
        }
        );
        if !spec.download_dir.is_empty() {
            da["dir"] = serde_json::json!(spec.download_dir);
        }
        if spec.download_timeout_ms > 0 {
            da["timeoutMs"] = serde_json::json!(spec.download_timeout_ms);
        }
        actions.push(BrowserAction {
            tool: "runtime_session_download".to_string(),
            args: da,
        });
    }

    // Deterministic settle: when requested, attach a post-action wait timeout to
    // every interactive step so a slow page has time to render before the next
    // action fires — reliability without an inference-driven re-check loop. A
    // caller who already set `waitTimeoutMs` on an extra step is left untouched.
    if spec.settle_ms > 0 {
        const SETTLE: [&str; 5] = [
            "runtime_session_navigate",
            "runtime_session_click",
            "runtime_session_fill",
            "runtime_session_submit",
            "runtime_session_evaluate",
        ];
        for a in actions.iter_mut() {
            if SETTLE.contains(&a.tool.as_str()) {
                if let Some(obj) = a.args.as_object_mut() {
                    obj.entry("waitTimeoutMs".to_string())
                        .or_insert(serde_json::json!(spec.settle_ms));
                }
            }
        }
    }
    BrowserLoginPlan {
        role_id: spec.role_id.to_string(),
        resource_id: spec.resource_id.to_string(),
        capability: spec.capability,
        secret_ref: resource.secret_ref.clone(),
        actions,
    }
}
/// Outcome of running a gated browser plan against the live browser engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserRun {
    pub session_id: String,
    pub steps_total: usize,
    pub steps_ok: usize,
    /// One line per action, `"<tool> -> ok"` or `"<tool> -> ERR: ..."`. Never
    /// contains the resolved secret or the substituted fill values.
    pub log: Vec<String>,
    /// Content returned by read/extract steps (`runtime_session_evaluate`,
    /// `runtime_capture_session`) on a live run. Kept separate from [`log`] so
    /// [`BrowserRun::describe`] stays log-safe; surfaced explicitly via
    /// [`BrowserRun::extraction`].
    pub extracted: Vec<String>,
}
impl BrowserRun {
    /// Deterministic pass/fail on the extracted content: true iff `needle` is a
    /// non-empty substring of the joined extraction. No browser, no inference —
    /// just a containment assertion the Testing team pins a build against.
    pub fn verify(&self, needle: &str) -> bool {
        !needle.is_empty() && self.extraction().contains(needle)
    }

    /// Deterministic regression guard: true iff `needle` is a non-empty substring
    /// that does NOT appear in the joined extraction. Catches a broken page
    /// ("Internal Server Error", "undefined", "NaN") with no inference.
    pub fn verify_absent(&self, needle: &str) -> bool {
        !needle.is_empty() && !self.extraction().contains(needle)
    }

    /// Log-safe rendering. `dry` marks a preview that never touched the browser.
    pub fn describe(&self, dry: bool) -> String {
        let head = if dry {
            format!(                "Browser plan PREVIEW (dry-run) for session '{}': {}/{} steps ready. No browser touched, no secret resolved.",                self.session_id, self.steps_ok, self.steps_total            )
        } else {
            format!(
                "Browser run for session '{}': {}/{} steps ok.",
                self.session_id, self.steps_ok, self.steps_total
            )
        };
        let mut out = head + "\n";
        for (i, l) in self.log.iter().enumerate() {
            out.push_str(&format!("  {}. {l}\n", i + 1));
        }
        out
    }

    /// The content pulled by read/extract steps, joined for the caller. This is
    /// the whole point of a read task (a ticket body, a chat thread, a captured
    /// page summary), so it is surfaced explicitly — but it is kept out of
    /// [`BrowserRun::describe`]'s log-safe rendering.
    pub fn extraction(&self) -> String {
        self.extracted.join("\n")
    }
}
/// Recursively replace `{{sessionId}}` and `vault:REF` sentinel strings inside a
/// step's args. `secret` is substituted only for a pointer whose ref matches the
/// resolved resource, so a wrong-pointer value is never injected.
fn substitute_step_args(value: &mut serde_json::Value, session_id: &str, secret: Option<&str>) {
    match value {
        serde_json::Value::String(s) => {
            if s.contains("{{sessionId}}") {
                *s = s.replace("{{sessionId}}", session_id);
            }
            if let Some(v) = secret {
                if let Some(rest) = s.strip_prefix(VAULT_VALUE_PREFIX) {
                    // The pointer's ref was already validated at resolve time;
                    // swap the whole pointer for the live value.
                    let _ = rest;
                    *s = v.to_string();
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for e in arr.iter_mut() {
                substitute_step_args(e, session_id, secret);
            }
        }
        serde_json::Value::Object(map) => {
            for e in map.values_mut() {
                substitute_step_args(e, session_id, secret);
            }
        }
        _ => {}
    }
}
/// Recursively substitute `{{vars.NAME}}` tokens inside a step's args with the
/// live value stored under `NAME` in the plan's runtime variable map (populated
/// by earlier `captureAs` steps — see [`BrowserTaskSpec::captures]). Unknown
/// tokens are left untouched so a missing capture surfaces as a tool error,
/// never a silent empty selector.
fn substitute_captured_vars(
    value: &mut serde_json::Value,
    vars: &std::collections::HashMap<String, String>,
) {
    match value {
        serde_json::Value::String(s) => {
            if !s.contains("{{vars.") {
                return;
            }
            for (name, val) in vars {
                let needle = format!("{{{{vars.{name}}}}}");
                if s.contains(&needle) {
                    *s = s.replace(&needle, val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for e in arr.iter_mut() {
                substitute_captured_vars(e, vars);
            }
        }
        serde_json::Value::Object(map) => {
            for e in map.values_mut() {
                substitute_captured_vars(e, vars);
            }
        }
        _ => {}
    }
}

/// Drive a cross-platform browser task on behalf of a role, end to end: gate the
/// action, resolve the credential through the vault, then execute the plan's
/// deterministic browser steps against the live engine. This is the seam the
/// marketing and testing teams use to actually operate platforms.
///
/// Safety properties:
/// - The permission gate runs first (via [`plan_org_browser_login`]); a denied
///   or not-yet-approved action never opens a browser session.
/// - The secret is resolved only at execution and is substituted in-memory; the
///   returned [`BrowserRun`] log never carries it, and the scope is zeroized on
///   drop.
/// - `dry_run` builds and placeholder-substitutes the full step list but calls
///   no browser tool and resolves no secret, so plans can be previewed safely.
pub fn execute_org_browser_plan(
    workspace_root: &Path,
    org: &CompanyOrg,
    spec: &BrowserTaskSpec,
    dry_run: bool,
) -> Result<BrowserRun, CredentialBlocked> {
    let plan = plan_org_browser_login(org, spec)?;

    // Resolve the live secret only when not previewing; scope zeroizes on drop.
    let secret_handle = if !dry_run && !plan.secret_ref.is_empty() {
        Some(resolve_credential(
            org,
            spec.role_id,
            spec.resource_id,
            spec.capability,
            spec.amount,
        )?)
    } else {
        None
    };
    let secret: Option<&str> = secret_handle.as_ref().and_then(|h| h.expose());

    let run = drive_plan(workspace_root, &plan, spec.login_url, secret, dry_run);
    // `secret_handle` drops here -> CredentialScope zeroizes the value.
    Ok(run)
}

/// Execute a browser plan for an *already-approved* action request. Gated outward
/// comms (a marketing post that needs counsel sign-off, a support email that
/// awaits a manager, etc.) never clear the stateless [`check_permission`] gate on
/// their own — they clear only once a human/manager advances the request to
/// [`ActionStatus::Approved`]. This is the path that then actually runs the plan:
/// the approved request *is* the gate, so the stateless gate is not re-evaluated.
/// Safety properties:
/// - The request id must exist and be `Approved`; a `Pending`/`Rejected` request
///   returns [`CredentialBlocked::NeedsApproval`] and nothing runs (even in
///   preview), so an unapproved intent can never be rehearsed as if cleared.
/// - The secret resolves through [`resolve_credential_for_request`] only at
///   execution and is substituted in-memory; the [`BrowserRun`] log never carries
///   it, and the scope is zeroized on drop.
/// - On a live (non-dry) run the request is advanced to
///   [`ActionStatus::Executed`] via [`mark_executed`], so an approved action is
///   replayed at most once — a deterministic audit trail.
pub fn execute_org_browser_plan_for_request(
    workspace_root: &Path,
    org: &CompanyOrg,
    request_id: &str,
    spec: &BrowserTaskSpec,
    dry_run: bool,
) -> Result<BrowserRun, CredentialBlocked> {
    let request = load_requests(workspace_root)
        .into_iter()
        .find(|r| r.id == request_id)
        .ok_or_else(|| CredentialBlocked::Denied {
            reason: format!("unknown request '{request_id}'"),
        })?;
    // The approved request is the gate: enforce it here, independent of the
    // stateless permission matrix, and before touching the browser or the vault.
    if request.status != ActionStatus::Approved {
        return Err(CredentialBlocked::NeedsApproval {
            approvers: if request.required_approvers.is_empty() {
                vec![HUMAN_OPERATOR.to_string()]
            } else {
                request.required_approvers.clone()
            },
            quorum: request.quorum.max(1),
            reason: format!(
                "request '{}' is {:?}, not Approved",
                request.id, request.status
            ),
        });
    }
    // Elevation gate (Phase 62b): a Spend/Deploy on an approved-but-
    // unelevated scope is refused BEFORE the vault or browser is touched.
    ensure_elevation_cleared(workspace_root, &request, now_secs())?;
    let resource =
        org.find_resource(spec.resource_id)
            .ok_or_else(|| CredentialBlocked::Denied {
                reason: format!("unknown resource '{}'", spec.resource_id),
            })?;
    // Build the plan without re-gating — approval already cleared it.
    let plan = assemble_login_plan(resource, spec);

    let secret_handle = if !dry_run && !plan.secret_ref.is_empty() {
        Some(resolve_credential_for_request(org, &request)?)
    } else {
        None
    };
    let secret: Option<&str> = secret_handle.as_ref().and_then(|h| h.expose());

    let run = drive_plan(workspace_root, &plan, spec.login_url, secret, dry_run);
    drop(secret_handle);

    // A live run consumed the approved intent: mark it executed so it can't be
    // replayed. Best-effort — the browser already ran; the ledger is bookkeeping.
    if !dry_run {
        let _ = mark_executed(workspace_root, request_id);
    }
    Ok(run)
}

/// Shared executor: open a session at `login_url`, replay `plan.actions` with the
/// real session id (and the resolved secret) substituted in-memory, then close the
/// session. `dry_run` builds and placeholder-substitutes the full step list without
/// calling any browser tool, so plans can be previewed safely. The returned log
/// never carries the secret or the substituted fill values.
fn drive_plan(
    workspace_root: &Path,
    plan: &BrowserLoginPlan,
    login_url: &str,
    secret: Option<&str>,
    dry_run: bool,
) -> BrowserRun {
    // Deterministic, collision-resistant session id (sessionId is caller-supplied).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let session_id = format!(
        "org-{}-{}-{:x}",
        crate::editor::expert_team::slugify(&plan.role_id),
        now_secs(),
        nanos
    );

    let mut log: Vec<String> = Vec::new();
    let mut extracted: Vec<String> = Vec::new();
    let mut ok = 0usize;
    // Runtime variable map for deterministic step-to-step data flow. Populated
    // when a step carries `captureAs: NAME` in its args and its evaluate output
    // comes back; consulted right before each later dispatch to substitute
    // `{{vars.NAME}}` tokens in string args. Never leaks into the log.
    let mut vars: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    if !dry_run {
        // Open the session pointed at the login page before replaying actions.
        let created = crate::registry::dispatch::call_tool_in_workspace(
            workspace_root,
            "runtime_create_session",
            &serde_json::json!({ "sessionId": session_id, "startUrl": login_url }),
        );
        match created {
            Ok(_) => ok += 1,
            Err(e) => log.push(format!("runtime_create_session -> ERR: {e}")),
        }
    }

    for a in &plan.actions {
        let mut args = a.args.clone();
        substitute_step_args(&mut args, &session_id, secret);
        if dry_run {
            // Log the tool name only — never the substituted (secret) args.
            log.push(format!("{} -> ready", a.tool));
            ok += 1;
            continue;
        }
        // Resolve plan-local `{{vars.NAME}}` tokens against values captured by
        // earlier steps (see `BrowserTaskSpec::captures`). Unresolved tokens are
        // left as-is so a missing capture fails loudly in the tool call,
        // not silently with an empty selector.
        substitute_captured_vars(&mut args, &vars);
        let res = crate::registry::dispatch::call_tool_in_workspace(workspace_root, &a.tool, &args);
        match res {
            Ok(out) => {
                log.push(format!("{} -> ok", a.tool));
                ok += 1;
                // A read/extract step's payload is the reason the task ran; capture
                // it (bounded) into `extracted`, kept out of the log-safe render.
                if matches!(
                    a.tool.as_str(),
                    "runtime_session_evaluate" | "runtime_capture_session"
                ) {
                    let clipped: String = out.chars().take(4000).collect();
                    // Attribute the chunk to its step label (e.g. a pagination
                    // crawl's "page 2") when present, else the tool name.
                    let tag = a
                        .args
                        .get("label")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(a.tool.as_str());
                    extracted.push(format!("[{tag}] {clipped}"));
                    // If this step was declared a variable capture, stash the
                    // raw value (whitespace-trimmed) for later {{vars.*}} refs.
                    if let Some(name) = a.args.get("captureAs").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            vars.insert(name.to_string(), clipped.trim().to_string());
                        }
                    }
                }
            }
            Err(e) => log.push(format!("{} -> ERR: {e}", a.tool)),
        }
    }

    if !dry_run {
        let _ = crate::registry::dispatch::call_tool_in_workspace(
            workspace_root,
            "runtime_close_session",
            &serde_json::json!({ "sessionId": session_id }),
        );
    }

    BrowserRun {
        session_id,
        steps_total: plan.actions.len(),
        steps_ok: ok,
        log,
        extracted,
    }
}

/// One owned per-platform login descriptor used for multi-platform broadcasts.
/// Owned fields keep the marketing tool schema simple and avoid borrow juggling
/// when a single campaign is fanned out across several social resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformLogin {
    pub resource_id: String,
    pub login_url: String,
    #[serde(default)]
    pub username_selector: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password_selector: String,
    #[serde(default)]
    pub submit_selector: String,
    #[serde(default)]
    pub composer_url: Option<String>,
    #[serde(default)]
    pub composer_selector: Option<String>,
    #[serde(default)]
    pub composer_text: String,
    /// Selector of this platform's publish/send control (see
    /// [`BrowserTaskSpec::composer_submit_selector`]). Empty falls back to a
    /// generic form submit.
    #[serde(default)]
    pub composer_submit_selector: String,
    /// Optional settle timeout (ms) applied to every interactive step for this
    /// platform (see [`BrowserTaskSpec::settle_ms`]). 0 = no wait.
    #[serde(default)]
    pub settle_ms: u64,
}

/// Outcome per platform for a broadcast: either a runnable, secret-free plan or
/// the reason the platform's gate blocked it.
pub type PlatformPlan = (String, Result<BrowserLoginPlan, CredentialBlocked>);

/// Fan a single outward campaign out across many platforms, gating EACH one
/// independently. A role may hold Communicate on some accounts and not others;
/// each platform is checked on its own so a partial grant yields plans only for
/// the cleared accounts and blocks (never leaks) for the rest.
pub fn plan_org_browser_broadcast(
    org: &CompanyOrg,
    role_id: &str,
    capability: Capability,
    amount: f64,
    platforms: &[PlatformLogin],
) -> Vec<PlatformPlan> {
    platforms
        .iter()
        .map(|p| {
            let spec = spec_for_platform(role_id, capability, amount, p);
            let outcome = plan_org_browser_login(org, &spec);
            (p.resource_id.clone(), outcome)
        })
        .collect()
}

/// Borrow a [`PlatformLogin`] descriptor into a [`BrowserTaskSpec`] for a single
/// platform. Broadcast fans outward posts only, so the read/capture/extra fields
/// stay empty (reads use `execute_org_browser_task`, not the fan-out).
fn spec_for_platform<'a>(
    role_id: &'a str,
    capability: Capability,
    amount: f64,
    p: &'a PlatformLogin,
) -> BrowserTaskSpec<'a> {
    BrowserTaskSpec {
        role_id,
        resource_id: &p.resource_id,
        capability,
        amount,
        login_url: &p.login_url,
        username_selector: &p.username_selector,
        username: &p.username,
        password_selector: &p.password_selector,
        submit_selector: &p.submit_selector,
        composer_url: p.composer_url.as_deref(),
        composer_selector: p.composer_selector.as_deref(),
        composer_text: &p.composer_text,
        composer_submit_selector: &p.composer_submit_selector,
        read_script: None,
        capture_demo: false,
        extra_actions: Vec::new(),
        settle_ms: p.settle_ms,
        verify_expect: "",
        verify_absent: "",
        next_selector: "",
        max_pages: 0,
        read_urls: Vec::new(),
        form_fields: Vec::new(),
        wait_for_selector: "",
        scroll_to: "",
        switch_tab: false,
        drag_from: "",
        drag_to: "",
        wait_for_url: "",
        right_click_selector: "",
        select_option: ("", ""),
        form_submit_selector: "",
        wait_for_text: "",
        hover_selector: "",
        upload: ("", Vec::new()),
        extract_attribute: ("", ""),
        screenshot_selector: "",
        captures: Vec::new(),
        screenshot_full_page: false,
        frame_selector: "",
        dialog_action: "",
        dialog_prompt_text: "",
        download_selector: "",
        download_dir: "",
        download_timeout_ms: 0,
        wait_for_network_idle: false,
        request_headers: Vec::new(),
        cookies: Vec::new(),
        local_storage: Vec::new(),
        basic_auth_user: "",
        basic_auth_pass: "",
        user_agent: "",
        viewport_width: 0,
        viewport_height: 0,
    }
}

/// Log-safe, secret-free summary of a broadcast: how many platforms cleared
/// (and would run) versus how many are blocked, with per-platform reasons.
pub fn describe_broadcast(results: &[PlatformPlan]) -> String {
    let cleared = results.iter().filter(|(_, r)| r.is_ok()).count();
    let mut out = format!(
        "Broadcast across {} platform(s): {} cleared, {} blocked.\n",
        results.len(),
        cleared,
        results.len() - cleared
    );
    for (rid, res) in results {
        match res {
            Ok(plan) => out.push_str(&format!(
                "  - {rid}: CLEARED ({} action(s), secret via {})\n",
                plan.actions.len(),
                if plan.secret_ref.is_empty() {
                    "(none)"
                } else {
                    &plan.secret_ref
                }
            )),
            Err(blocked) => out.push_str(&format!("  - {rid}: {}\n", blocked.reason_line())),
        }
    }
    out
}

/// Drive a multi-platform broadcast end to end. For each platform the gate is
/// resolved one of two ways: if an approved action-request id is supplied for
/// that platform, the request *is* the gate ([`execute_org_browser_plan_for_request`],
/// so counsel-approved comms post and the request is advanced to `Executed`);
/// otherwise the stateless permission matrix decides ([`execute_org_browser_plan`],
/// which only runs on `Allowed`). Platforms are driven independently, so a
/// campaign fans out across exactly the accounts the gate clears and blocks —
/// never leaking a credential — for the rest.
pub fn execute_org_browser_broadcast(
    workspace_root: &Path,
    org: &CompanyOrg,
    role_id: &str,
    capability: Capability,
    amount: f64,
    platforms: &[(PlatformLogin, Option<String>)],
    dry_run: bool,
) -> Vec<(String, Result<BrowserRun, CredentialBlocked>)> {
    platforms
        .iter()
        .map(|(p, rid)| {
            let spec = spec_for_platform(role_id, capability, amount, p);
            let outcome = match rid.as_deref().filter(|s| !s.is_empty()) {
                Some(request_id) => execute_org_browser_plan_for_request(
                    workspace_root,
                    org,
                    request_id,
                    &spec,
                    dry_run,
                ),
                None => execute_org_browser_plan(workspace_root, org, &spec, dry_run),
            };
            (p.resource_id.clone(), outcome)
        })
        .collect()
}

/// Log-safe, secret-free summary of a broadcast run: how many platforms executed
/// and, per platform, either the driven [`BrowserRun`] or the block reason.
pub fn describe_broadcast_runs(
    results: &[(String, Result<BrowserRun, CredentialBlocked>)],
    dry: bool,
) -> String {
    let cleared = results.iter().filter(|(_, r)| r.is_ok()).count();
    let mut out = format!(
        "Broadcast run across {} platform(s): {} {}, {} blocked.\n",
        results.len(),
        cleared,
        if dry { "previewed" } else { "executed" },
        results.len() - cleared
    );
    for (rid, res) in results {
        match res {
            Ok(run) => out.push_str(&format!(
                "  - {rid}: {}/{} steps ok\n",
                run.steps_ok, run.steps_total
            )),
            Err(blocked) => out.push_str(&format!("  - {rid}: {}\n", blocked.reason_line())),
        }
    }
    out
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Org â†” ExpertTeam staffing bridge (makes the AI company executable)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Slug of the [`crate::editor::expert_team::ExpertTeam`] that AI-staffs this
/// org. Derived from the org name with the same
/// [`crate::editor::expert_team::slugify`] the roster uses, so it always matches
/// the team [`sync_org_to_expert_team`] upserts.
pub fn org_team_slug(org: &CompanyOrg) -> String {
    crate::editor::expert_team::slugify(&org.name)
}

/// Summary of an org -> team sync (for tool output and tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgTeamSync {
    pub team_slug: String,
    pub member_count: usize,
    pub new_members: usize,
    pub carried_over: usize,
}

/// Project the org chart into Velocity's expert-team roster so the AI company is
/// actually runnable. Each [`Role`] becomes an
/// [`crate::editor::expert_team::ExpertMember`] whose persona is drawn from its
/// title, department charter and capability grants. The team is upserted by slug
/// while every other team in the roster is preserved.
///
/// Capability isolation is enforced at execution time by [`check_permission`]
/// (the org gate), not by restricting the member tool surface — a synced member
/// keeps the full registry, but every gated outward action it attempts is still
/// checked against the role's grants. Playbook `AgentTask` steps can name this
/// team slug to route work to the right role via `team_dispatch`.
pub fn sync_org_to_expert_team(workspace_root: &Path, org: &CompanyOrg) -> OrgTeamSync {
    use crate::editor::expert_team::{ExpertMember, ExpertTeam};
    let slug = org_team_slug(org);

    let mut members: Vec<ExpertMember> = Vec::with_capacity(org.roles.len());
    for r in &org.roles {
        let charter = org
            .find_department(&r.department_id)
            .map(|d| d.charter.clone())
            .unwrap_or_default();
        let mut skills: Vec<String> = vec![r.department_id.clone()];
        for g in org.grants.iter().filter(|g| g.role_id == r.id) {
            for c in &g.capabilities {
                skills.push(format!("{}:{}", g.resource_id, c.as_str()));
            }
        }
        let team_for_role = if r.agent_team.is_empty() {
            slug.clone()
        } else {
            r.agent_team.clone()
        };
        let wf = format!(
            "Staff the '{}' role of {}. Charter: {}. Route every outward/sensitive action through the org approval gate; act only within your capability grants. Home team: {}.",
            r.title, org.name, charter, team_for_role
        );
        let skill_refs: Vec<&str> = skills.iter().map(|s| s.as_str()).collect();
        members.push(ExpertMember::new(
            &r.id,
            &r.title,
            &r.title,
            crate::agent::AiProvider::LocalOllama,
            "",
            skill_refs,
            Vec::new(),
            &wf,
        ));
    }

    let team = ExpertTeam::new(
        &slug,
        &org.name,
        &format!("AI-staffed operating company: {}", org.name),
        members,
        false,
    );

    // Upsert by slug, preserving other teams; report new vs carried-over members.
    let mut teams = crate::editor::expert_team::load_expert_teams(workspace_root);
    let prior_ids: std::collections::HashSet<String> = teams
        .iter()
        .find(|t| t.slug() == slug)
        .map(|t| t.members.iter().map(|m| m.id.clone()).collect())
        .unwrap_or_default();
    let new_members = team
        .members
        .iter()
        .filter(|m| !prior_ids.contains(&m.id))
        .count();
    let carried_over = team.members.len() - new_members;
    let member_count = team.members.len();

    if let Some(existing) = teams.iter_mut().find(|t| t.slug() == slug) {
        *existing = team;
    } else {
        teams.push(team);
    }
    crate::editor::expert_team::save_expert_teams(workspace_root, &teams);

    OrgTeamSync {
        team_slug: slug,
        member_count,
        new_members,
        carried_over,
    }
}

/// Resolve which [`Role`] should staff a task using title / department keyword
/// matching, so a dispatcher can name the right AI member before calling
/// `team_dispatch`. Returns the matched role id, or `None` to fall back to the
/// team's own routing.
pub fn route_org_role_for_task(org: &CompanyOrg, task: &str) -> Option<String> {
    let task_lower = task.to_lowercase();
    for r in &org.roles {
        if task_lower.contains(&r.id) || task_lower.contains(&r.title.to_lowercase()) {
            return Some(r.id.clone());
        }
    }
    for r in &org.roles {
        if let Some(d) = org.find_department(&r.department_id) {
            if task_lower.contains(&d.name.to_lowercase()) {
                return Some(r.id.clone());
            }
        }
    }
    None
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// GUI dashboard rendering (pure, inference-free)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// A compact, human-readable dashboard of the org for the GUI activity rail:
/// departments with headcount, the capability grant matrix (who may do what on
/// each resource) and the pending approval queue. This is pure formatting over
/// stored data — the same deterministic source the MCP tools read — so the panel
/// can never disagree with the enforcement engine.
pub fn org_dashboard(workspace_root: &Path) -> String {
    let Some(org) = load_company_org(workspace_root) else {
        return "No company org yet \u{2014} run init_company_org to seed UnitBuilds CC."
            .to_string();
    };
    let mut out = String::new();
    out.push_str(&format!(
        "{} \u{00b7} {} dept(s) \u{00b7} {} role(s) \u{00b7} {} resource(s)\n",
        org.name,
        org.departments.len(),
        org.roles.len(),
        org.resources.len()
    ));

    out.push_str("\nDEPARTMENTS\n");
    for d in &org.departments {
        let count = org.roles.iter().filter(|r| r.department_id == d.id).count();
        out.push_str(&format!(
            "  \u{2022} {} \u{2014} {} role(s)\n",
            d.name, count
        ));
    }

    out.push_str("\nCAPABILITY MATRIX\n");
    for r in &org.roles {
        let grants: Vec<String> = org
            .grants
            .iter()
            .filter(|g| g.role_id == r.id)
            .map(|g| {
                let caps = g
                    .capabilities
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                if g.spend_limit_per_action > 0.0 {
                    format!(
                        "{}[{caps}\u{00a3}{}]",
                        g.resource_id, g.spend_limit_per_action
                    )
                } else {
                    format!("{}[{caps}]", g.resource_id)
                }
            })
            .collect();
        if !grants.is_empty() {
            out.push_str(&format!(
                "  {} ({})\n    {}\n",
                r.title,
                r.id,
                grants.join("  ")
            ));
        }
    }

    let reqs = load_requests(workspace_root);
    let pending: Vec<&ActionRequest> = reqs
        .iter()
        .filter(|a| a.status == ActionStatus::Pending)
        .collect();
    out.push_str(&format!("\nPENDING APPROVALS ({})\n", pending.len()));
    if pending.is_empty() {
        out.push_str("  (none)\n");
    }
    for a in pending {
        let approvers = if a.required_approvers.is_empty() {
            HUMAN_OPERATOR.to_string()
        } else {
            a.required_approvers.join(", ")
        };
        out.push_str(&format!(
            "  {} | {} -[{}]-> {} amt {} | needs {} of [{}]\n",
            a.id,
            a.role_id,
            a.capability.as_str(),
            a.resource_id,
            a.amount,
            a.quorum.max(1),
            approvers
        ));
    }

    out.push_str(&format!("\nPLAYBOOKS ({})\n", org.playbooks.len()));
    if org.playbooks.is_empty() {
        out.push_str("  (none)\n");
    }
    for p in &org.playbooks {
        let gate = match check_permission(&org, &p.role_id, &p.resource_id, p.capability, p.amount)
        {
            Decision::Allowed => "ready",
            Decision::RequiresApproval { .. } => "gated",
            Decision::Denied { .. } => "denied",
        };
        let mut tools = 0u32;
        let mut conds = 0u32;
        let mut agents = 0u32;
        let mut foreaches = 0u32;
        for s in &p.steps {
            match s {
                crate::editor::workflow::WorkflowStep::Tool { .. } => tools += 1,
                crate::editor::workflow::WorkflowStep::Condition { .. } => conds += 1,
                crate::editor::workflow::WorkflowStep::AgentTask { .. } => agents += 1,
                crate::editor::workflow::WorkflowStep::ForEach { .. } => foreaches += 1,
                crate::editor::workflow::WorkflowStep::Connector { .. } => tools += 1,
            }
        }
        out.push_str(&format!(
            "  \u{2022} {} ({} step(s): T{} C{} A{} F{}) [{}]\n      {} -[{}]-> {}\n",
            p.id,
            p.steps.len(),
            tools,
            conds,
            agents,
            foreaches,
            gate,
            p.role_id,
            p.capability.as_str(),
            p.resource_id
        ));
    }

    // Validation summary: surface broken tool references so operators can spot
    // stale SOPs without running them.
    let issues: Vec<String> = org
        .playbooks
        .iter()
        .filter_map(|p| {
            validate_step_references(Some(workspace_root), "playbook", &p.id, &p.steps).err()
        })
        .collect();
    if issues.is_empty() {
        out.push_str("  \u{2713} All playbook step references valid\n");
    } else {
        for e in &issues {
            out.push_str(&format!("  \u{26a0} {e}\n"));
        }
    }

    let slug = org_team_slug(&org);
    let roster = crate::editor::expert_team::load_expert_teams(workspace_root);
    let staffed = roster
        .iter()
        .find(|t| t.slug() == slug)
        .map(|t| t.members.len());
    out.push_str("\nSTAFFING\n  ");
    match staffed {
        Some(n) => out.push_str(&format!(
            "Team '{slug}' in roster: {n}/{} role(s) staffed.\n",
            org.roles.len()
        )),
        None => out.push_str(&format!(
            "Team '{slug}' not synced yet \u{2014} run sync_org_team to make the org runnable.\n"
        )),
    }
    out
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Tests
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

#[cfg(test)]
mod tests {
    use super::*;

    fn org() -> CompanyOrg {
        default_company_org()
    }

    #[test]
    fn preset_has_every_department_and_key_roles() {
        let o = org();
        for d in [
            "executive",
            "management",
            "engineering",
            "testing",
            "scouting",
            "marketing",
            "support",
            "finance",
            "legal",
            "operations",
        ] {
            assert!(o.find_department(d).is_some(), "missing dept {d}");
        }
        for r in [
            "ceo",
            "cto",
            "cfo",
            "counsel",
            "dev_lead",
            "support_agent",
            "sysadmin",
        ] {
            assert!(o.find_role(r).is_some(), "missing role {r}");
        }
    }

    #[test]
    fn deny_by_default_for_unganted_capability() {
        let o = org();
        // Support agent has no banking grant at all.
        let d = check_permission(&o, "support_agent", "banking", Capability::Spend, 10.0);
        assert!(
            matches!(d, Decision::Denied { .. }),
            "expected denial, got {d:?}"
        );
    }

    #[test]
    fn read_only_grant_allows_read_without_approval() {
        let o = org();
        let d = check_permission(&o, "accountant", "banking", Capability::Read, 0.0);
        assert_eq!(d, Decision::Allowed);
    }

    #[test]
    fn small_spend_still_gated_by_cfo_policy() {
        let o = org();
        // CFO spending 500: within limit but spend policy at 0 fires CFO gate;
        // CFO is itself an approver, and it's a self-gate — still RequiresApproval.
        let d = check_permission(&o, "cfo", "banking", Capability::Spend, 500.0);
        match d {
            Decision::RequiresApproval { approvers, .. } => {
                assert!(approvers.contains(&"cfo".to_string()));
            }
            other => panic!("expected approval, got {other:?}"),
        }
    }

    #[test]
    fn large_spend_requires_two_approvers() {
        let o = org();
        let d = check_permission(&o, "accountant", "banking", Capability::Spend, 5_000.0);
        // accountant has no spend grant -> denied.
        assert!(matches!(d, Decision::Denied { .. }), "got {d:?}");
    }

    #[test]
    fn ceo_large_spend_requires_quorum_two() {
        let o = org();
        // CEO spend 5000 > per-action limit 25000? No, within limit, but policy
        // spend_ceo_gate at 1000 fires requiring CFO+CEO quorum 2.
        let d = check_permission(&o, "ceo", "banking", Capability::Spend, 5_000.0);
        match d {
            Decision::RequiresApproval {
                quorum, approvers, ..
            } => {
                assert_eq!(quorum, 2);
                assert!(
                    approvers.contains(&"cfo".to_string())
                        && approvers.contains(&"ceo".to_string())
                );
            }
            other => panic!("expected two-approver gate, got {other:?}"),
        }
    }

    #[test]
    fn sensitive_outward_action_requires_human_when_no_policy() {
        let o = org();
        // dev_lead write to github: sensitive resource, outward, no write policy
        // on github -> RequiresApproval with empty approvers (human operator).
        let d = check_permission(&o, "dev_lead", "github", Capability::Write, 0.0);
        match d {
            Decision::RequiresApproval { approvers, .. } => {
                assert!(
                    approvers.is_empty(),
                    "expect human-operator gate, got {approvers:?}"
                );
            }
            other => panic!("expected human approval gate, got {other:?}"),
        }
    }

    #[test]
    fn social_post_requires_legal() {
        let o = org();
        let d = check_permission(
            &o,
            "social_operator",
            "social_x",
            Capability::Communicate,
            0.0,
        );
        match d {
            Decision::RequiresApproval { approvers, .. } => {
                assert!(approvers.contains(&"counsel".to_string()));
            }
            other => panic!("expected legal gate, got {other:?}"),
        }
    }

    #[test]
    fn deploy_cloud_requires_cto() {
        let o = org();
        let d = check_permission(&o, "sysadmin", "cloud", Capability::Deploy, 0.0);
        match d {
            Decision::RequiresApproval { approvers, .. } => {
                assert!(approvers.contains(&"cto".to_string()));
            }
            other => panic!("expected CTO gate, got {other:?}"),
        }
    }

    #[test]
    fn org_round_trips_through_sealed_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        assert!(save_company_org(root, &o));
        let loaded = load_company_org(root).expect("org should reload");
        assert_eq!(loaded.id, o.id);
        assert_eq!(loaded.grants.len(), o.grants.len());
        assert_eq!(loaded.policies.len(), o.policies.len());
    }

    #[test]
    fn submit_allowed_read_is_auto_approved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        let r = submit_action(
            root,
            &o,
            "accountant",
            "banking",
            Capability::Read,
            0.0,
            "view balances",
        );
        assert_eq!(r.status, ActionStatus::Approved);
    }

    #[test]
    fn submit_pending_requires_human_when_no_named_approver() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        // dev_lead write github: sensitive, no policy -> human-operator approver.
        let r = submit_action(
            root,
            &o,
            "dev_lead",
            "github",
            Capability::Write,
            0.0,
            "push fix",
        );
        assert_eq!(r.status, ActionStatus::Pending);
        assert_eq!(r.required_approvers, vec![HUMAN_OPERATOR.to_string()]);
    }

    #[test]
    fn approval_lifecycle_reaches_quorum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        // social_operator communicate -> counsel gate (quorum 1).
        let r = submit_action(
            root,
            &o,
            "social_operator",
            "social_x",
            Capability::Communicate,
            0.0,
            "post",
        );
        assert_eq!(r.status, ActionStatus::Pending);
        // counsel lacks Approve grant on social_x? It has [Read, Approve] -> ok.
        let approved = approve_action(root, &o, &r.id, "counsel").unwrap();
        assert_eq!(approved.status, ActionStatus::Approved);
        let executed = mark_executed(root, &r.id).unwrap();
        assert_eq!(executed.status, ActionStatus::Executed);
    }

    #[test]
    fn two_approver_quorum_needs_both() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        // ceo spend 5000 -> CFO+CEO quorum 2.
        let r = submit_action(
            root,
            &o,
            "ceo",
            "banking",
            Capability::Spend,
            5_000.0,
            "vendor payment",
        );
        assert_eq!(r.status, ActionStatus::Pending);
        assert_eq!(r.quorum, 2);
        let after_one = approve_action(root, &o, &r.id, "cfo").unwrap();
        assert_eq!(
            after_one.status,
            ActionStatus::Pending,
            "one of two not enough"
        );
        let after_two = approve_action(root, &o, &r.id, "ceo").unwrap();
        assert_eq!(after_two.status, ActionStatus::Approved);
    }

    #[test]
    fn non_designated_approver_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        let r = submit_action(
            root,
            &o,
            "social_operator",
            "social_x",
            Capability::Communicate,
            0.0,
            "post",
        );
        // accountant is not in required_approvers ([counsel]).
        let err = approve_action(root, &o, &r.id, "accountant").unwrap_err();
        assert!(err.contains("not a designated approver"), "{err}");
    }

    #[test]
    fn denied_action_is_recorded_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        let r = submit_action(
            root,
            &o,
            "support_agent",
            "banking",
            Capability::Spend,
            10.0,
            "refund",
        );
        assert_eq!(r.status, ActionStatus::Rejected);
    }

    #[test]
    fn request_ledger_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        let _ = submit_action(root, &o, "ceo", "banking", Capability::Spend, 5_000.0, "x");
        let _ = submit_action(
            root,
            &o,
            "accountant",
            "banking",
            Capability::Read,
            0.0,
            "y",
        );
        let all = load_requests(root);
        assert_eq!(all.len(), 2);
    }

    fn playbook(
        id: &str,
        role: &str,
        resource: &str,
        cap: Capability,
        step: serde_json::Value,
    ) -> Playbook {
        Playbook {
            id: id.to_string(),
            name: id.to_string(),
            role_id: role.to_string(),
            resource_id: resource.to_string(),
            capability: cap,
            amount: 0.0,
            description: "test".to_string(),
            steps: vec![parse_step(&step).unwrap()],
            on_success_handoff: None,
        }
    }

    #[test]
    fn allowed_playbook_runs_deterministic_steps() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut o = org();
        // accountant read analytics -> Allowed (read, non-sensitive, no policy).
        o.playbooks.push(playbook(
            "pb_read",
            "accountant",
            "analytics",
            Capability::Read,
            serde_json::json!({ "tool": "get_company_org" }),
        ));
        let out = run_playbook(root, &o, "pb_read").unwrap();
        match out {
            PlaybookOutcome::Executed {
                ok_steps,
                total_steps,
                ..
            } => {
                assert_eq!(total_steps, 1);
                assert_eq!(ok_steps, 1, "the tool step should have run");
            }
            other => panic!("expected executed, got {other:?}"),
        }
    }

    #[test]
    fn gated_playbook_halts_before_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut o = org();
        // social_operator communicate social_x -> counsel gate -> Pending.
        o.playbooks.push(playbook(
            "pb_post",
            "social_operator",
            "social_x",
            Capability::Communicate,
            serde_json::json!({ "tool": "get_company_org" }),
        ));
        let out = run_playbook(root, &o, "pb_post").unwrap();
        match out {
            PlaybookOutcome::AwaitingApproval {
                approvers, quorum, ..
            } => {
                assert_eq!(quorum, 1);
                assert!(approvers.contains(&"counsel".to_string()));
            }
            other => panic!("expected awaiting-approval, got {other:?}"),
        }
        // The gated step must NOT have executed: only the submitted request
        // exists, and it is still Pending (not Executed).
        let all = load_requests(root);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, ActionStatus::Pending);
    }

    #[test]
    fn denied_playbook_reports_denial() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut o = org();
        // support_agent spend banking -> no grant -> Denied.
        o.playbooks.push(playbook(
            "pb_bad",
            "support_agent",
            "banking",
            Capability::Spend,
            serde_json::json!({ "tool": "get_company_org" }),
        ));
        let out = run_playbook(root, &o, "pb_bad").unwrap();
        assert!(matches!(out, PlaybookOutcome::Denied { .. }), "{out:?}");
    }

    #[test]
    fn parse_step_rejects_unknown_shape() {
        assert!(parse_step(&serde_json::json!({ "bogus": 1 })).is_err());
    }

    #[test]
    fn validate_playbook_accepts_known_tools() {
        use crate::editor::workflow::WorkflowStep;
        let pb = Playbook {
            id: "vb_ok".into(),
            name: "valid".into(),
            description: String::new(),
            role_id: "cfo".into(),
            resource_id: "banking".into(),
            capability: Capability::Read,
            amount: 0.0,
            steps: vec![WorkflowStep::Tool {
                name: "verify_org_credential".into(),
                args: serde_json::json!({}),
            }],
            on_success_handoff: None,
        };
        assert!(validate_playbook_tool_names(&pb).is_ok());
    }

    #[test]
    fn validate_playbook_rejects_unknown_tool() {
        use crate::editor::workflow::WorkflowStep;
        let pb = Playbook {
            id: "vb_bad".into(),
            name: "invalid".into(),
            description: String::new(),
            role_id: "cfo".into(),
            resource_id: "banking".into(),
            capability: Capability::Read,
            amount: 0.0,
            steps: vec![WorkflowStep::Tool {
                name: "nonexistent_tool_xyz".into(),
                args: serde_json::json!({}),
            }],
            on_success_handoff: None,
        };
        let err = validate_playbook_tool_names(&pb).unwrap_err();
        assert!(err.contains("nonexistent_tool_xyz"), "{err}");
        assert!(err.contains("step 0"), "{err}");
    }

    #[test]
    fn validate_playbook_checks_foreach_body() {
        use crate::editor::workflow::WorkflowStep;
        let pb = Playbook {
            id: "vb_fe".into(),
            name: "foreach bad".into(),
            description: String::new(),
            role_id: "cfo".into(),
            resource_id: "banking".into(),
            capability: Capability::Read,
            amount: 0.0,
            steps: vec![WorkflowStep::ForEach {
                source: "{{lastOutput}}".into(),
                as_var: "item".into(),
                body: vec![WorkflowStep::Tool {
                    name: "ghost_tool".into(),
                    args: serde_json::json!({}),
                }],
            }],
            on_success_handoff: None,
        };
        let err = validate_playbook_tool_names(&pb).unwrap_err();
        assert!(err.contains("ghost_tool"), "{err}");
    }

    #[test]
    fn preset_org_ships_gated_sop_playbooks() {
        let o = default_company_org();
        assert!(o.playbooks.len() >= 20, "got {}", o.playbooks.len());
        for p in &o.playbooks {
            assert!(o.find_role(&p.role_id).is_some(), "role {}", p.role_id);
            assert!(
                o.find_resource(&p.resource_id).is_some(),
                "resource {}",
                p.resource_id
            );
            // A seeded SOP must never be outright Denied by the gate (it may be
            // Allowed or await approval, but the role holds the capability).
            let d = check_permission(&o, &p.role_id, &p.resource_id, p.capability, p.amount);
            assert!(
                !matches!(d, Decision::Denied { .. }),
                "playbook {} gate denied: {:?}",
                p.id,
                d
            );
            assert!(!p.steps.is_empty(), "playbook {} has no steps", p.id);
        }
        // The marketing SOP exercises the gated browser executor as a tool node.
        let mkt = o
            .playbooks
            .iter()
            .find(|p| p.id == "post_product_announcement")
            .expect("marketing playbook seeded");
        assert!(mkt
            .steps
            .iter()
            .any(|s| matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. } if name == "execute_org_browser_task_for_request")));
        // The marketing SOP posts for real: it names the publish button and a
        // deterministic settle so the outward flow is reliable, not inference-gated.
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            mkt.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task_for_request")
            })
        {
            assert_eq!(
                args["composer_submit_selector"].as_str(),
                Some("div[data-testid='tweetButton']"),
                "marketing SOP must click a publish control"
            );
            assert!(
                args["settle_ms"].as_u64().unwrap_or(0) > 0,
                "marketing SOP should settle between steps"
            );
        } else {
            panic!("marketing SOP missing the request-gated browser node");
        }
        // The Testing SOP records a demo via a read/capture browser node, and the
        // Support SOP triages chats via a read-only browser node — both gate Allowed
        // (no approval, no secret for the analytics capture).
        for id in [
            "record_feature_demo",
            "read_support_chats",
            "sweep_open_ticket_queue",
            "create_support_ticket",
            "scout_competitor_features",
            "sweep_competitor_changelogs",
            "review_campaign_compliance",
            "sync_team_capacity",
            "check_service_uptime",
            "plan_feature_roadmap",
            "verify_staging_build",
            "report_revenue_to_finance",
            "confirm_announcement_landed",
            "review_pending_secure_transfer",
        ] {
            let p = o
                .playbooks
                .iter()
                .find(|x| x.id == id)
                .unwrap_or_else(|| panic!("{id} playbook seeded"));
            assert!(
                p.steps.iter().any(|s| matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. } if name == "execute_org_browser_task")),
                "{id} drives the browser"
            );
            assert!(
                matches!(
                    check_permission(&o, &p.role_id, &p.resource_id, p.capability, p.amount),
                    Decision::Allowed
                ),
                "{id} gate should be Allowed (deterministic read)"
            );
        }
        // The Testing verify SOP pins its acceptance on a deterministic verify_expect
        // assertion (not an inference call) plus a settle so the page renders first.
        let qa = o
            .playbooks
            .iter()
            .find(|x| x.id == "verify_staging_build")
            .expect("verify_staging_build seeded");
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            qa.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["verify_expect"].as_str().unwrap_or("").is_empty(),
                "QA verify SOP must pin a deterministic acceptance target"
            );
            assert!(
                !args["verify_absent"].as_str().unwrap_or("").is_empty(),
                "QA verify SOP must pin a deterministic regression guard"
            );
            assert!(
                args["settle_ms"].as_u64().unwrap_or(0) > 0,
                "QA verify SOP should settle before reading"
            );
        } else {
            panic!("verify_staging_build missing the browser node");
        }
        // The Support sweep SOP drives the bounded pagination crawl: a next-page
        // selector and a page bound > 1, so it walks the queue deterministically.
        let sweep = o
            .playbooks
            .iter()
            .find(|x| x.id == "sweep_open_ticket_queue")
            .expect("sweep_open_ticket_queue seeded");
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            sweep.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["next_selector"].as_str().unwrap_or("").is_empty(),
                "sweep SOP must name the next-page control to crawl"
            );
            assert!(
                args["max_pages"].as_u64().unwrap_or(0) > 1,
                "sweep SOP must bound the crawl at > 1 page"
            );
        } else {
            panic!("sweep_open_ticket_queue missing the browser node");
        }
        // Phase 50: SOPs now drive the richer verb surface — Support attaches a
        // crash log via upload, Testing captures a build-marker screenshot, QA
        // demo captures a stage screenshot, Marketing extracts the permalink
        // for the legal audit trail. Assert each field is present so the
        // planner emits them on live runs (not just in dry-run preview tests).
        let ticket = o
            .playbooks
            .iter()
            .find(|x| x.id == "create_support_ticket")
            .expect("create_support_ticket seeded");
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            ticket.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["upload_selector"].as_str().unwrap_or("").is_empty(),
                "ticket SOP must name a file-input control to attach logs"
            );
            assert!(
                args["upload_files"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false),
                "ticket SOP must attach at least one file"
            );
            assert!(
                args["captures"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false),
                "ticket SOP must capture at least one value (the escalated ticket id) for deterministic cross-step threading"
            );
        } else {
            panic!("create_support_ticket missing the browser node");
        }
        let demo = o
            .playbooks
            .iter()
            .find(|x| x.id == "record_feature_demo")
            .expect("record_feature_demo seeded");
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            demo.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["screenshot_selector"]
                    .as_str()
                    .unwrap_or("")
                    .is_empty(),
                "demo SOP must pin a screenshot target for the recording"
            );
        } else {
            panic!("record_feature_demo missing the browser node");
        }
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            qa.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["screenshot_selector"]
                    .as_str()
                    .unwrap_or("")
                    .is_empty(),
                "QA verify SOP must capture visual evidence of the build marker"
            );
        }
        let landed = o
            .playbooks
            .iter()
            .find(|x| x.id == "confirm_announcement_landed")
            .expect("confirm_announcement_landed seeded");
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            landed.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["extract_attribute_selector"]
                    .as_str()
                    .unwrap_or("")
                    .is_empty(),
                "landing SOP must pin an element to extract the permalink from"
            );
            assert!(
                !args["extract_attribute_name"]
                    .as_str()
                    .unwrap_or("")
                    .is_empty(),
                "landing SOP must name the attribute (href) for the permalink"
            );
        } else {
            panic!("confirm_announcement_landed missing the browser node");
        }
        // Development now ships a fully browser-driven PR open: captures the
        // base/head SHAs from the compare view, threads them into the PR title
        // and body via `{{vars.*}}`, uploads the patch diff and submits.
        // The gate must NOT be Denied (dev_backend holds Write on github); the
        // SOP drives `execute_org_browser_task` with captures + upload.
        let pr = o
            .playbooks
            .iter()
            .find(|x| x.id == "open_github_pr")
            .expect("open_github_pr seeded");
        assert!(
            !matches!(
                check_permission(&o, &pr.role_id, &pr.resource_id, pr.capability, pr.amount),
                Decision::Denied { .. }
            ),
            "dev_backend must hold github Write for the PR SOP"
        );
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            pr.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                args["captures"]
                    .as_array()
                    .map(|a| a.len() >= 2)
                    .unwrap_or(false),
                "PR SOP must capture at least the base and head SHAs"
            );
            assert!(
                !args["upload_selector"].as_str().unwrap_or("").is_empty(),
                "PR SOP must attach the patch diff via an upload control"
            );
            let body_val = args["form_fields"][1]["value"].as_str().unwrap_or("");
            assert!(
                body_val.contains("{{vars.baseSha}}") && body_val.contains("{{vars.headSha}}"),
                "PR body must reference the captured SHAs (got: {body_val})"
            );
        } else {
            panic!("open_github_pr missing the browser node");
        }
        // Phase 53: Support email is now a browser SOP end-to-end — a read step
        // pulls the newest unanswered thread (Allowed, no approval), the one
        // genuine agent step drafts the reply, and a second execute_org_browser_task
        // publishes through the reply composer with a full-page screenshot
        // captured for the audit trail. Assert both browser nodes + the
        // full-page flag are present.
        let em = o
            .playbooks
            .iter()
            .find(|x| x.id == "support_email_reply")
            .expect("support_email_reply seeded");
        let browser_nodes: Vec<_> = em
            .steps
            .iter()
            .filter(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
            .collect();
        assert!(
            browser_nodes.len() >= 2,
            "support_email_reply must drive the browser for both read and reply, got {}",
            browser_nodes.len()
        );
        // At least one browser node must set screenshot_full_page: the audit
        // trail needs a whole-page artifact after sending, not only a chart.
        let any_full_shot = browser_nodes.iter().any(|s| {
            matches!(
                s,
                crate::editor::workflow::WorkflowStep::Tool { args, .. }
                    if args["screenshot_full_page"].as_bool().unwrap_or(false)
            )
        });
        assert!(
            any_full_shot,
            "support_email_reply must capture a full-page screenshot for the audit trail"
        );
        // Phase 54: Finance ships a 3-D-Secure-scoped read that pins the
        // `frame_selector` — the driver switches into the bank's secure iframe
        // before reading. Without frame entry the read_script would run
        // against the outer document and return an empty string.
        let secure = o
            .playbooks
            .iter()
            .find(|x| x.id == "review_pending_secure_transfer")
            .expect("review_pending_secure_transfer seeded");
        if let Some(crate::editor::workflow::WorkflowStep::Tool { args, .. }) =
            secure.steps.iter().find(|s| {
                matches!(s, crate::editor::workflow::WorkflowStep::Tool { name, .. }
                    if name == "execute_org_browser_task")
            })
        {
            assert!(
                !args["frame_selector"].as_str().unwrap_or("").is_empty(),
                "secure-transfer SOP must pin the 3-D-Secure iframe selector"
            );
            assert!(
                !args["dialog_action"].as_str().unwrap_or("").is_empty(),
                "secure-transfer SOP must clear the native 3-D-Secure confirm modal"
            );
            assert!(
                !args["read_script"].as_str().unwrap_or("").is_empty(),
                "secure-transfer SOP must frame-scope a read_script"
            );
        } else {
            panic!("review_pending_secure_transfer missing the browser node");
        }
        // Phase 73: queue_invoice_payments carries a ForEach fan-out over the
        // browser-extracted invoice array, binding item fields via dotted-path
        // interpolation. The gate auto-allows (Read), so the workflow reaches the
        // ForEach in production; in tests it halts at the verdict gate because no
        // browser is attached, but the structural contract must still hold.
        let qip = o
            .playbooks
            .iter()
            .find(|x| x.id == "queue_invoice_payments")
            .expect("queue_invoice_payments seeded");
        let foreach = qip
            .steps
            .iter()
            .find(|s| matches!(s, crate::editor::workflow::WorkflowStep::ForEach { .. }))
            .expect("queue_invoice_payments must contain a ForEach step");
        if let crate::editor::workflow::WorkflowStep::ForEach {
            source,
            as_var,
            body,
        } = foreach
        {
            assert_eq!(
                source, "lastOutput",
                "ForEach reads extracted invoices from lastOutput"
            );
            assert_eq!(as_var, "invoice", "ForEach binds each item as 'invoice'");
            assert_eq!(body.len(), 1, "body is one request_org_action per item");
            if let crate::editor::workflow::WorkflowStep::Tool { name, args } = &body[0] {
                assert_eq!(name, "request_org_action");
                assert!(
                    args["amount"]
                        .as_str()
                        .unwrap_or("")
                        .contains("{{invoice.amount}}"),
                    "ForEach body must interpolate the item amount"
                );
                assert!(
                    args["description"]
                        .as_str()
                        .unwrap_or("")
                        .contains("{{invoice.vendor}}"),
                    "ForEach body must interpolate the item vendor"
                );
                assert!(
                    args["description"]
                        .as_str()
                        .unwrap_or("")
                        .contains("{{invoice.ref}}"),
                    "ForEach body must interpolate the item ref"
                );
            } else {
                panic!("ForEach body[0] must be a Tool step");
            }
        } else {
            unreachable!();
        }
    }

    #[test]
    fn banking_spend_escalates_to_dual_control_at_threshold() {
        let o = default_company_org();
        // Below the 1,000 threshold only the CFO gate fires (quorum 1, [cfo]).
        let small = check_permission(&o, "cfo", "banking", Capability::Spend, 500.0);
        match &small {
            Decision::RequiresApproval {
                approvers, quorum, ..
            } => {
                assert_eq!(approvers, &vec!["cfo".to_string()], "{small:?}");
                assert_eq!(*quorum, 1, "{small:?}");
            }
            d => panic!("expected CFO-only gate at 500, got {d:?}"),
        }
        // At/above 1,000 BOTH gates fire: approvers union {cfo, ceo}, quorum 2.
        let big = check_permission(&o, "cfo", "banking", Capability::Spend, 2_500.0);
        match &big {
            Decision::RequiresApproval {
                approvers, quorum, ..
            } => {
                assert!(approvers.contains(&"cfo".to_string()), "{big:?}");
                assert!(approvers.contains(&"ceo".to_string()), "{big:?}");
                assert_eq!(*quorum, 2, "two-party control at/above threshold");
            }
            d => panic!("expected dual-control gate at 2,500, got {d:?}"),
        }
        // The seeded pay_vendor_invoice SOP carries the escalating amount.
        let pb = o
            .playbooks
            .iter()
            .find(|p| p.id == "pay_vendor_invoice")
            .expect("spend SOP seeded");
        assert!(
            pb.amount >= 1_000.0,
            "spend SOP must clear the dual threshold"
        );
    }

    #[test]
    fn every_operating_department_ships_a_playbook() {
        let o = default_company_org();
        // Which departments have at least one seeded SOP, via the SOP's role.
        let mut covered: Vec<&str> = Vec::new();
        for p in &o.playbooks {
            let dept = o
                .find_role(&p.role_id)
                .map(|r| r.department_id.as_str())
                .unwrap_or("<missing>");
            if !covered.contains(&dept) {
                covered.push(dept);
            }
        }
        // Every team the objective calls for must be able to run a deterministic
        // SOP — including the Executive tier, which now sets technical direction
        // through a read/dispatch roadmap workflow (no irreversible action).
        for dept in [
            "executive",
            "management",
            "engineering",
            "testing",
            "scouting",
            "marketing",
            "support",
            "finance",
            "legal",
            "operations",
        ] {
            assert!(
                covered.contains(&dept),
                "department {dept} has no SOP playbook (covered: {covered:?})"
            );
        }
    }

    #[test]
    fn capability_parsing_round_trips() {
        for c in [
            Capability::Read,
            Capability::Write,
            Capability::Communicate,
            Capability::Spend,
            Capability::Deploy,
            Capability::Approve,
            Capability::Manage,
        ] {
            assert_eq!(Capability::parse(c.as_str()), Some(c));
        }
    }

    // â”€â”€ Credential vault resolution â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn resolve_credential_denies_without_grant() {
        let o = org();
        // No banking grant for support_agent -> the vault never even looks.
        let r = resolve_credential(&o, "support_agent", "banking", Capability::Spend, 10.0);
        assert!(
            matches!(r, Err(CredentialBlocked::Denied { .. })),
            "got {r:?}"
        );
    }

    #[test]
    fn resolve_credential_keeps_secret_sealed_until_approval() {
        let o = org();
        // CEO spend 5000 fires the two-approver gate -> NeedsApproval, sealed.
        let r = resolve_credential(&o, "ceo", "banking", Capability::Spend, 5_000.0);
        match r {
            Err(CredentialBlocked::NeedsApproval { quorum, .. }) => assert_eq!(quorum, 2),
            other => panic!("expected needs-approval, got {other:?}"),
        }
    }

    #[test]
    fn resolve_credential_allowed_but_unprovisioned_reports_missing() {
        let o = org();
        // accountant read banking is Allowed, but UNITBUILDS_BANKING_TOKEN is
        // not set in the test env -> Missing (provisioned pointer, no value).
        let r = resolve_credential(&o, "accountant", "banking", Capability::Read, 0.0);
        match r {
            Err(CredentialBlocked::Missing { secret_ref }) => {
                assert_eq!(secret_ref, "UNITBUILDS_BANKING_TOKEN")
            }
            other => panic!("expected missing, got {other:?}"),
        }
    }

    #[test]
    fn resolve_credential_no_secret_resource_yields_empty_handle() {
        let o = org();
        // analytics has an empty secret_ref (no credential needed) and read is
        // allowed -> a valid handle whose expose() is None.
        let r = resolve_credential(&o, "accountant", "analytics", Capability::Read, 0.0);
        // accountant may not have an analytics read grant; if denied that's a
        // valid deterministic outcome too — accept either Allowed-Empty or Denied,
        // but never leak a value.
        match r {
            Ok(h) => assert!(h.expose().is_none()),
            Err(CredentialBlocked::Denied { .. }) => {}
            Err(other) => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn resolve_from_provisioned_env_returns_masked_only() {
        // Use a dedicated env var + rewritten pointer so this test cannot race
        // the "Missing" tests (which rely on UNITBUILDS_BANKING_TOKEN being unset).
        let mut o = org();
        let var = "UNITBUILDS_TEST_VAULT_SECRET";
        if let Some(r) = o.resources.iter_mut().find(|r| r.id == "banking") {
            r.secret_ref = var.to_string();
        }
        std::env::set_var(var, "super-secret-banking-value-123456");
        let r = resolve_credential(&o, "accountant", "banking", Capability::Read, 0.0);
        let handle = r.expect("provisioned env should resolve");
        assert_eq!(handle.expose(), Some("super-secret-banking-value-123456"));
        // masked() must NOT contain the raw value.
        assert!(!handle.masked().contains("secret-banking"));
        // Debug must NOT contain the raw value.
        let dbg = format!("{handle:?}");
        assert!(!dbg.contains("super-secret"), "{dbg}");
        std::env::remove_var(var);
    }

    #[test]
    fn resolve_for_request_requires_approved_status() {
        let o = org();
        let pending = ActionRequest {
            id: "req_x".to_string(),
            role_id: "ceo".to_string(),
            resource_id: "banking".to_string(),
            capability: Capability::Spend,
            amount: 5_000.0,
            description: String::new(),
            status: ActionStatus::Pending,
            required_approvers: vec!["cfo".to_string(), "ceo".to_string()],
            quorum: 2,
            approvals: Vec::new(),
            decision_reason: String::new(),
            created_at: 0,
            playbook_id: None,
        };
        let r = resolve_credential_for_request(&o, &pending);
        assert!(
            matches!(r, Err(CredentialBlocked::NeedsApproval { .. })),
            "got {r:?}"
        );

        let approved = ActionRequest {
            status: ActionStatus::Approved,
            ..pending
        };
        // Approved, but env not provisioned -> Missing (gate cleared, value absent).
        std::env::remove_var("UNITBUILDS_BANKING_TOKEN");
        let r2 = resolve_credential_for_request(&o, &approved);
        assert!(
            matches!(r2, Err(CredentialBlocked::Missing { .. })),
            "got {r2:?}"
        );
    }

    // â”€â”€ Cross-platform browser planning â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn spec<'a>(
        role_id: &'a str,
        resource_id: &'a str,
        capability: Capability,
        amount: f64,
    ) -> BrowserTaskSpec<'a> {
        BrowserTaskSpec {
            role_id,
            resource_id,
            capability,
            amount,
            login_url: "https://example.test/login",
            username_selector: "#user",
            username: "ops@unitbuilds.com",
            password_selector: "#pass",
            submit_selector: "button[type=submit]",
            composer_url: None,
            composer_selector: None,
            composer_text: "",
            composer_submit_selector: "",
            read_script: None,
            capture_demo: false,
            extra_actions: Vec::new(),
            settle_ms: 0,
            verify_expect: "",
            verify_absent: "",
            next_selector: "",
            max_pages: 0,
            read_urls: Vec::new(),
            form_fields: Vec::new(),
            wait_for_selector: "",
            scroll_to: "",
            switch_tab: false,
            drag_from: "",
            drag_to: "",
            wait_for_url: "",
            right_click_selector: "",
            select_option: ("", ""),
            form_submit_selector: "",
            wait_for_text: "",
            hover_selector: "",
            upload: ("", Vec::new()),
            extract_attribute: ("", ""),
            screenshot_selector: "",
            captures: Vec::new(),
            screenshot_full_page: false,
            frame_selector: "",
            dialog_action: "",
            dialog_prompt_text: "",
            download_selector: "",
            download_dir: "",
            download_timeout_ms: 0,
            wait_for_network_idle: false,
            request_headers: Vec::new(),
            cookies: Vec::new(),
            local_storage: Vec::new(),
            basic_auth_user: "",
            basic_auth_pass: "",
            user_agent: "",
            viewport_width: 0,
            viewport_height: 0,
        }
    }

    #[test]
    fn browser_plan_denied_for_unganted_role() {
        let o = org();
        let r = plan_org_browser_login(
            &o,
            &spec("support_agent", "banking", Capability::Spend, 10.0),
        );
        assert!(
            matches!(r, Err(CredentialBlocked::Denied { .. })),
            "got {r:?}"
        );
    }

    #[test]
    fn browser_plan_appends_read_and_capture_steps() {
        let o = org();
        // analyst Read on analytics is auto-allowed and needs no secret.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.read_script = Some("document.querySelector('.metric')?.innerText");
        s.capture_demo = true;
        let plan = plan_org_browser_login(&o, &s).expect("analyst read on analytics is allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert!(
            tools.contains(&"runtime_session_evaluate"),
            "read step missing: {tools:?}"
        );
        assert!(
            tools.contains(&"runtime_capture_session"),
            "capture step missing: {tools:?}"
        );
        // The read script is non-secret copy and passes through verbatim; the
        // plan carries no secret (analytics has an empty secret_ref).
        assert!(plan.secret_ref.is_empty(), "analytics needs no credential");
        let eval = plan
            .actions
            .iter()
            .find(|a| a.tool == "runtime_session_evaluate")
            .unwrap();
        assert_eq!(
            eval.args["script"],
            "document.querySelector('.metric')?.innerText"
        );
    }

    #[test]
    fn browser_plan_pagination_crawl_is_bounded_and_deterministic() {
        let o = org();
        // Support agent walks a 3-page ticket queue: read, next, read, next, read.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.read_script = Some("document.querySelectorAll('.ticket')");
        s.next_selector = "button[aria-label='Next']";
        s.max_pages = 3;
        let plan = plan_org_browser_login(&o, &s).expect("analyst read is allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        let evals = tools
            .iter()
            .filter(|t| **t == "runtime_session_evaluate")
            .count();
        let nexts = tools
            .iter()
            .filter(|t| **t == "runtime_session_click")
            .count();
        // Exactly `max_pages` reads, and one fewer next-click (never after the
        // last page) — a bounded crawl, not an unbounded loop.
        assert_eq!(evals, 3, "expected 3 page reads: {tools:?}");
        assert_eq!(
            nexts, 2,
            "expected 2 next-clicks between 3 pages: {tools:?}"
        );
        // Each read is labelled by page so the extraction is attributable.
        let labels: Vec<String> = plan
            .actions
            .iter()
            .filter(|a| a.tool == "runtime_session_evaluate")
            .map(|a| a.args["label"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(labels, vec!["page 1", "page 2", "page 3"], "{labels:?}");
    }

    #[test]
    fn browser_plan_single_page_read_when_no_next_selector() {
        let o = org();
        // Back-compat: no next_selector means exactly one read, even if
        // max_pages is set large (there is nothing to advance on).
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.read_script = Some("document.title");
        s.max_pages = 5;
        let plan = plan_org_browser_login(&o, &s).expect("analyst read is allowed");
        let evals = plan
            .actions
            .iter()
            .filter(|a| a.tool == "runtime_session_evaluate")
            .count();
        assert_eq!(evals, 1, "no next_selector must read a single page");
    }

    #[test]
    fn browser_plan_multi_url_crawl_reads_each_page_labelled() {
        let o = org();
        // Scout sweeps 3 public changelog pages in one session: navigate(login)
        // then per-URL navigate + read, each read labelled by its URL.
        let mut s = spec("scout", "analytics", Capability::Read, 0.0);
        s.username_selector = "";
        s.username = "";
        s.password_selector = "";
        s.submit_selector = "";
        s.read_script = Some("return document.title");
        s.read_urls = vec![
            "https://a.test/changelog".to_string(),
            "https://b.test/releases".to_string(),
            "https://c.test/news".to_string(),
        ];
        let plan = plan_org_browser_login(&o, &s).expect("scout read analytics is allowed");
        let evals: Vec<&str> = plan
            .actions
            .iter()
            .filter(|a| a.tool == "runtime_session_evaluate")
            .map(|a| a.args["label"].as_str().unwrap_or(""))
            .collect();
        // One read per URL, each attributable to its source page.
        assert_eq!(
            evals,
            vec![
                "https://a.test/changelog",
                "https://b.test/releases",
                "https://c.test/news"
            ],
            "{evals:?}"
        );
        // navigate(login) + one navigate per URL = 4 navigations.
        let navs = plan
            .actions
            .iter()
            .filter(|a| a.tool == "runtime_session_navigate")
            .count();
        assert_eq!(navs, 4, "login + 3 crawl navigations");
        // read_urls takes precedence over next_selector pagination.
        assert!(plan.secret_ref.is_empty(), "public read needs no secret");
    }

    #[test]
    fn browser_plan_form_fields_become_ordered_fill_steps() {
        let o = org();
        // A support ticket form: three declarative fields turn into three ordered
        // fill steps, interleaved after login and before the composer/read block.
        let mut s = spec("support_agent", "chat", Capability::Communicate, 0.0);
        s.form_fields = vec![
            ("input[name=subject]".to_string(), "Escalation".to_string()),
            ("select[name=priority]".to_string(), "high".to_string()),
            (
                "textarea[name=description]".to_string(),
                "login fails on staging".to_string(),
            ),
        ];
        let plan = plan_org_browser_login(&o, &s).expect("support communicate chat is allowed");
        let fills: Vec<&str> = plan
            .actions
            .iter()
            .filter(|a| a.tool == "runtime_session_fill")
            .map(|a| a.args["selector"].as_str().unwrap_or(""))
            .collect();
        // Order preserved: username, password (from login), then the 3 form fields.
        assert_eq!(
            fills,
            vec![
                "#user",
                "#pass",
                "input[name=subject]",
                "select[name=priority]",
                "textarea[name=description]"
            ],
            "{fills:?}"
        );
    }

    #[test]
    fn browser_plan_wait_and_scroll_emit_ordered_steps() {
        let o = org();
        // A read task with wait_for_selector + scroll_to: after login, the plan
        // emits runtime_session_wait then runtime_session_scroll before the read.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.read_script = Some("document.body.innerText");
        s.wait_for_selector = "[data-panel='loaded']";
        s.scroll_to = "[data-metric='mrr']";
        s.settle_ms = 3000;
        let plan = plan_org_browser_login(&o, &s).expect("analyst read analytics");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        // navigate -> fill(user) -> fill(pass) -> submit -> wait -> scroll -> evaluate
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_wait",
                "runtime_session_scroll",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        // wait step carries the selector + timeout
        let wait = &plan.actions[4];
        assert_eq!(wait.args["selector"], "[data-panel='loaded']");
        assert_eq!(wait.args["timeout"], 3000);
        // scroll step carries the target selector
        let scroll = &plan.actions[5];
        assert_eq!(scroll.args["selector"], "[data-metric='mrr']");
    }

    #[test]
    fn browser_plan_switch_tab_and_drag_emit_steps() {
        let o = org();
        // An analyst read task with switch_tab + drag: tests the assembler emits
        // the steps in the right order (after extra_actions/wait/scroll, before form/read).
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.extra_actions = vec![BrowserAction {
            tool: "runtime_session_click".to_string(),
            args: serde_json::json!({ "selector": "a[target=_blank]" }),
        }];
        s.switch_tab = true;
        s.drag_from = "[data-card='bug']";
        s.drag_to = "[data-column='done']";
        s.read_script = Some("document.title");
        let plan = plan_org_browser_login(&o, &s).expect("analyst read analytics is allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_click",
                "runtime_session_switch_tab",
                "runtime_session_drag",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        let drag = &plan.actions[6];
        assert_eq!(drag.args["sourceSelector"], "[data-card='bug']");
        assert_eq!(drag.args["targetSelector"], "[data-column='done']");
    }

    #[test]
    fn browser_plan_wait_url_right_click_select_emit_steps() {
        let o = org();
        // Exercises wait_for_url, right_click_selector, and select_option together:
        // navigate -> fill -> fill -> submit -> wait_url -> right_click -> form_fill -> select -> evaluate
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.wait_for_url = "/dashboard";
        s.right_click_selector = "[data-cell]";
        s.form_fields = vec![("input[name=q]".to_string(), "revenue".to_string())];
        s.select_option = ("[role=listbox]", "Q3 2026");
        s.read_script = Some("document.body.innerText");
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_wait_url",
                "runtime_session_right_click",
                "runtime_session_fill",
                "runtime_session_select",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        let wait_url = &plan.actions[4];
        assert_eq!(wait_url.args["urlPattern"], "/dashboard");
        assert_eq!(wait_url.args["timeout"], 8000);
        let sel = &plan.actions[7];
        assert_eq!(sel.args["selector"], "[role=listbox]");
        assert_eq!(sel.args["optionText"], "Q3 2026");
    }

    #[test]
    fn browser_plan_form_submit_closes_fill_loop() {
        let o = org();
        // The fill -> submit loop: form_fields, then select_option, then a click
        // on the form_submit_selector, then the read. form_submit is emitted AFTER
        // select and BEFORE composer/read — the terminal action of a standalone form.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.form_fields = vec![
            (
                "input[name=subject]".to_string(),
                "Login failure".to_string(),
            ),
            ("select[name=priority]".to_string(), "high".to_string()),
        ];
        s.select_option = ("[role=listbox]", "Critical");
        s.form_submit_selector = "button[type=submit]";
        s.read_script = Some("document.body.innerText");
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_select",
                "runtime_session_click",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        // The form-submit click targets the submit button, and is distinct from
        // the login submit (index 3): index 7 is the terminal form submit.
        let submit = &plan.actions[7];
        assert_eq!(submit.tool, "runtime_session_click");
        assert_eq!(submit.args["selector"], "button[type=submit]");
    }

    #[test]
    fn browser_plan_wait_text_and_hover_emit_ordered_steps() {
        let o = org();
        // Element-readiness -> content-readiness -> reveal-menu -> read. The order
        // matters: wait_for_text fires after wait_for_selector, hover after drag /
        // before right-click, so a menu is open before its items are targeted.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.wait_for_selector = "[data-panel]";
        s.wait_for_text = "Loaded";
        s.hover_selector = "[data-menu-trigger]";
        s.read_script = Some("document.body.innerText");
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_wait",
                "runtime_session_wait_text",
                "runtime_session_hover",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        let wt = &plan.actions[5];
        assert_eq!(wt.args["text"], "Loaded");
        assert_eq!(wt.args["timeout"], 6000);
        assert_eq!(plan.actions[6].args["selector"], "[data-menu-trigger]");
    }

    #[test]
    fn browser_plan_upload_emits_between_fields_and_select() {
        let o = org();
        // Attach order on a real form: text fields, then the file upload, then the
        // dropdown, then submit. The upload step carries the selector + file list.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.form_fields = vec![("input[name=title]".to_string(), "Crash".to_string())];
        s.upload = (
            "input[type=file]",
            vec!["logs/crash.txt".to_string(), "shots/repro.png".to_string()],
        );
        s.select_option = ("[role=listbox]", "Critical");
        s.read_script = Some("document.body.innerText");
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_fill",
                "runtime_session_upload",
                "runtime_session_select",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        let up = &plan.actions[5];
        assert_eq!(up.args["selector"], "input[type=file]");
        assert_eq!(
            up.args["files"],
            serde_json::json!(["logs/crash.txt", "shots/repro.png"])
        );
    }

    #[test]
    fn browser_plan_extract_attribute_and_screenshot() {
        let o = org();
        // A generated, quote-safe attribute read + a per-element screenshot for
        // evidence. The selector carries embedded double quotes; they must be
        // escaped inside the generated JS so the script stays syntactically valid.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.extract_attribute = ("input[name=\"subject\"]", "value");
        s.screenshot_selector = "[data-chart='mrr']";
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_evaluate",
                "runtime_session_screenshot",
            ],
            "{tools:?}"
        );
        let eval = &plan.actions[4];
        let script = eval.args["script"].as_str().unwrap();
        // The selector is JSON-escaped inside the script (its quotes become \"),
        // which is what keeps a quote-bearing selector from breaking out of the JS.
        assert!(script.contains(r#"input[name=\"subject\"]"#), "{script}");
        assert!(script.contains("querySelector"), "{script}");
        assert!(script.contains("getAttribute"), "{script}");
        assert_eq!(eval.args["label"], "input[name=\"subject\"]=value");
        assert_eq!(plan.actions[5].args["selector"], "[data-chart='mrr']");
    }

    #[test]
    fn browser_plan_captures_emit_evaluate_with_captureas() {
        let o = org();
        // Two value captures emit one generated evaluate per pair, each marked
        // with `captureAs` so drive_plan knows to store the read result. Label
        // is `capture:NAME` (attributable in `extracted`) and the script uses
        // the same quote-safe pattern as extract_attribute so a selector
        // containing embedded quotes cannot break out of the JS.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.captures = vec![
            ("invoiceId".to_string(), "tr.row[data-id]".to_string()),
            (
                "amountDue".to_string(),
                "input[name=\"amount\"]".to_string(),
            ),
        ];
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        // Order: navigate -> fill -> fill -> submit -> [capture x2].
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_evaluate",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        let first = &plan.actions[4];
        assert_eq!(first.args["captureAs"], "invoiceId");
        assert_eq!(first.args["label"], "capture:invoiceId");
        let second = &plan.actions[5];
        assert_eq!(second.args["captureAs"], "amountDue");
        let script = second.args["script"].as_str().unwrap();
        // JSON-escaped selector stays safe inside the JS string literal.
        assert!(script.contains(r#"input[name=\"amount\"]"#), "{script}");
        assert!(script.contains("querySelector"), "{script}");
    }

    #[test]
    fn browser_plan_screenshot_full_page_emits_after_selector_shot() {
        let o = org();
        // Both a per-element chart shot AND a whole-page audit shot fire, in
        // that order, after the read. Two screenshots + one evaluate + one
        // navigate + login (2 fills + submit) = 7 total steps.
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.read_script = Some("return document.title");
        s.screenshot_selector = "[data-chart='mrr']";
        s.screenshot_full_page = true;
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_evaluate",
                "runtime_session_screenshot",
                "runtime_session_screenshot",
            ],
            "{tools:?}"
        );
        // The first screenshot carries a selector; the second is full-page.
        assert_eq!(
            plan.actions[5].args["selector"], "[data-chart='mrr']",
            "selector screenshot fires first"
        );
        assert!(
            plan.actions[6].args["full"].as_bool().unwrap_or(false),
            "second screenshot must be flagged full-page"
        );
        assert!(
            plan.actions[6].args.get("selector").is_none(),
            "full-page screenshot must NOT carry a selector"
        );
    }

    #[test]
    fn browser_plan_frame_selector_emits_before_captures() {
        // Frame entry fires IMMEDIATELY after login (nav + user/pass + submit)
        // and BEFORE the value-capture evaluates, so a capture selector can
        // target an element *inside* the iframe (banking 3-D-Secure sheet,
        // embedded payment widget, admin dashboard). Ordering is the whole
        // point of the test — a swap would silently capture null.
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.frame_selector = "iframe[name=\"secure-payment\"]";
        s.captures = vec![("rowId".to_string(), "tr.top".to_string())];
        s.extra_actions = vec![crate::editor::company_org::BrowserAction {
            tool: "runtime_session_click".to_string(),
            args: serde_json::json!({ "selector": "button.open" }),
        }];
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_enter_frame",
                "runtime_session_evaluate",
                "runtime_session_click",
            ],
            "{tools:?}"
        );
        // Frame step carries the exact selector (raw, not JSON-escaped) so the
        // engine can address the iframe directly.
        assert_eq!(
            plan.actions[4].args["selector"], "iframe[name=\"secure-payment\"]",
            "frame selector surfaced verbatim"
        );
        // The capture evaluate runs INSIDE the frame context (frame entry
        // precedes it) and holds its `captureAs` marker for drive_plan to bind.
        assert_eq!(
            plan.actions[5].args["captureAs"], "rowId",
            "capture marker survives planner"
        );
    }

    #[test]
    fn browser_plan_dialog_accept_emits_after_frame_entry() {
        // 3-D-Secure-style flow: sign in -> enter secure iframe -> clear the
        // native `confirm()` modal the bank pops on frame load -> capture the
        // challenge code that only renders once the modal is dismissed.
        // Ordering is the whole point — if the dialog step lands AFTER the
        // capture, the evaluate will block on the native modal and the plan
        // silently hangs. Emit order: enter_frame -> dialog -> evaluate.
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.frame_selector = "iframe[name=\"secure\"]";
        s.dialog_action = "accept";
        s.captures = vec![("otpCode".to_string(), "input[name=otp]".to_string())];
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_enter_frame",
                "runtime_session_dialog",
                "runtime_session_evaluate",
            ],
            "{tools:?}"
        );
        // Dialog step carries the action verb ("accept" = OK / confirm) and
        // omits promptText when the SOP did not declare one.
        assert_eq!(
            plan.actions[5].args["action"], "accept",
            "dialog action surfaced verbatim"
        );
        assert!(
            plan.actions[5].args.get("promptText").is_none(),
            "empty dialog_prompt_text must NOT emit a promptText field"
        );
    }

    #[test]
    fn browser_plan_dialog_dismiss_and_prompt_text() {
        // Two complementary shapes: (a) `dismiss` alone cancels the modal,
        // (b) `accept` + a non-empty promptText threads the input into the
        // emitted args so the engine can type it into a `window.prompt()`.
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.dialog_action = "dismiss";
        s.dialog_prompt_text = "ignored";
        let plan = plan_org_browser_login(&o, &s).expect("analyst read");
        let d = plan
            .actions
            .iter()
            .find(|a| a.tool == "runtime_session_dialog")
            .expect("dialog step");
        assert_eq!(d.args["action"], "dismiss");
        assert_eq!(
            d.args["promptText"], "ignored",
            "promptText is emitted verbatim when non-empty (engine routes it only for prompt())"
        );
        // `none` sentinel is honored as skip — no dialog step for the same
        // spec with dialog_action="none".
        let mut s2 = spec("analyst", "analytics", Capability::Read, 0.0);
        s2.dialog_action = "none";
        let plan2 = plan_org_browser_login(&o, &s2).expect("analyst read");
        assert!(
            !plan2
                .actions
                .iter()
                .any(|a| a.tool == "runtime_session_dialog"),
            "dialog_action=none must not emit a dialog step"
        );
    }

    #[test]
    fn substitute_captured_vars_walks_nested_args() {
        // Pure-function check: `{{vars.NAME}}` tokens resolve anywhere inside a
        // step's args (nested objects, arrays, string values), and unknown
        // tokens are left untouched so a missing capture surfaces loudly in the
        // tool call instead of silently producing an empty selector.
        use std::collections::HashMap;
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("invoiceId".to_string(), "INV-42".to_string());
        vars.insert("tab".to_string(), "paid".to_string());
        let mut args = serde_json::json!({
            "sessionId": "sess-1",
            "selector": "tr[data-id='{{vars.invoiceId}}'] button.pay",
            "nested": { "url": "https://x/{{vars.tab}}?ref={{vars.unknown}}" },
            "list": [ "{{vars.invoiceId}}", 42, "{{vars.nothere}}" ],
        });
        substitute_captured_vars(&mut args, &vars);
        assert_eq!(
            args["selector"], "tr[data-id='INV-42'] button.pay",
            "{{vars.*}} in a top-level string resolves"
        );
        let url = args["nested"]["url"].as_str().unwrap();
        assert!(url.contains("/paid?ref="), "{url}");
        assert!(url.contains("{{vars.unknown}}"), "{url}");
        assert_eq!(args["list"][0], "INV-42");
        assert_eq!(args["list"][1], 42);
        assert_eq!(args["list"][2], "{{vars.nothere}}");
    }

    #[test]
    fn public_read_capture_plan_has_no_login_submit() {
        let o = org();
        // demo_recorder Read on analytics, driven against a public page: no login
        // selectors, just navigate -> read -> capture. No stray submit step.
        let mut s = spec("demo_recorder", "analytics", Capability::Read, 0.0);
        s.username_selector = "";
        s.username = "";
        s.password_selector = "";
        s.submit_selector = "";
        s.read_script = Some("document.title");
        s.capture_demo = true;
        let plan = plan_org_browser_login(&o, &s).expect("demo_recorder read analytics is allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_evaluate",
                "runtime_capture_session"
            ],
            "public capture must be navigate->read->capture, no submit: {tools:?}"
        );
        assert!(plan.secret_ref.is_empty(), "analytics needs no credential");
    }

    #[test]
    fn extra_actions_splice_after_login_before_read() {
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        // spec() carries a full login form; add two ordered steps then a read.
        s.extra_actions = vec![
            BrowserAction {
                tool: "runtime_session_click".to_string(),
                args: serde_json::json!({ "selector": ".tab-releases" }),
            },
            BrowserAction {
                tool: "runtime_session_navigate".to_string(),
                args: serde_json::json!({ "url": "https://app.unitbuilds.com/releases" }),
            },
        ];
        s.read_script = Some("document.title");
        let plan = plan_org_browser_login(&o, &s).expect("analyst read analytics is allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate", // login page
                "runtime_session_fill",     // username
                "runtime_session_fill",     // password
                "runtime_session_submit",   // login
                "runtime_session_click",    // extra: open tab
                "runtime_session_navigate", // extra: go to releases
                "runtime_session_evaluate", // read
            ],
            "unexpected order: {tools:?}"
        );
        // The extra steps get the {{sessionId}} placeholder injected so the
        // executor binds the live session id (the caller omitted sessionId).
        for a in plan
            .actions
            .iter()
            .filter(|a| a.tool == "runtime_session_click" || a.tool == "runtime_session_navigate")
        {
            assert_eq!(
                a.args["sessionId"].as_str(),
                Some("{{sessionId}}"),
                "{} missing bound sessionId",
                a.tool
            );
        }
    }

    #[test]
    fn composer_publish_click_fires_when_submit_selector_given() {
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.composer_url = Some("https://x.com/compose");
        s.composer_selector = Some("#post");
        s.composer_text = "Ship it";
        s.composer_submit_selector = "div[data-testid='tweetButton']";
        let plan = plan_org_browser_login(&o, &s).expect("analyst read analytics is allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        // After the login block: navigate to composer, fill copy, then a CLICK on
        // the publish control (not a bare form submit) — a real outward post.
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate", // login page
                "runtime_session_fill",     // username
                "runtime_session_fill",     // password
                "runtime_session_submit",   // login
                "runtime_session_navigate", // compose
                "runtime_session_fill",     // composer copy
                "runtime_session_click",    // publish
            ],
            "unexpected order: {tools:?}"
        );
        let publish = plan
            .actions
            .iter()
            .find(|a| a.tool == "runtime_session_click")
            .expect("publish click present");
        assert_eq!(
            publish.args["selector"].as_str(),
            Some("div[data-testid='tweetButton']")
        );
        assert_eq!(publish.args["sessionId"].as_str(), Some("{{sessionId}}"));
    }

    #[test]
    fn composer_falls_back_to_form_submit_without_publish_selector() {
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.composer_url = Some("https://x.com/compose");
        s.composer_selector = Some("#post");
        s.composer_text = "Ship it";
        // Empty publish selector keeps the legacy form-submit behaviour.
        s.composer_submit_selector = "";
        let plan = plan_org_browser_login(&o, &s).expect("allowed");
        let tools: Vec<&str> = plan.actions.iter().map(|a| a.tool.as_str()).collect();
        assert_eq!(
            tools,
            vec![
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_fill",
                "runtime_session_submit",
                "runtime_session_navigate",
                "runtime_session_fill",
                "runtime_session_submit", // composer submit
            ],
            "unexpected order: {tools:?}"
        );
        assert!(!tools.contains(&"runtime_session_click"));
    }

    #[test]
    fn settle_ms_attaches_wait_timeout_to_interactive_steps() {
        let o = org();
        let mut s = spec("analyst", "analytics", Capability::Read, 0.0);
        s.read_script = Some("document.title");
        // A caller-set wait on an extra step must win over the blanket settle.
        s.extra_actions = vec![BrowserAction {
            tool: "runtime_session_click".to_string(),
            args: serde_json::json!({ "selector": ".x", "waitTimeoutMs": 999 }),
        }];
        s.settle_ms = 750;
        let plan = plan_org_browser_login(&o, &s).expect("allowed");
        for a in &plan.actions {
            match a.tool.as_str() {
                "runtime_session_navigate"
                | "runtime_session_fill"
                | "runtime_session_evaluate" => {
                    assert_eq!(
                        a.args["waitTimeoutMs"].as_u64(),
                        Some(750),
                        "{} missing settle",
                        a.tool
                    );
                }
                "runtime_session_click" => {
                    assert_eq!(
                        a.args["waitTimeoutMs"].as_u64(),
                        Some(999),
                        "explicit caller wait must be preserved"
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn browser_run_verify_is_a_containment_assertion() {
        let run = BrowserRun {
            session_id: "s1".to_string(),
            steps_total: 3,
            steps_ok: 3,
            log: vec!["runtime_session_evaluate -> ok".to_string()],
            extracted: vec!["Staging build 2.4.1".to_string(), "ready".to_string()],
        };
        // No needle => never a false positive.
        assert!(!run.verify(""));
        // Present in the joined extraction (across two read chunks).
        assert!(run.verify("build 2.4.1"));
        assert!(run.verify("ready"));
        // Absent => deterministic FAIL, no inference.
        assert!(!run.verify("2.5.0"));
        // Regression guard is the mirror: a marker NOT present passes.
        assert!(run.verify_absent("Internal Server Error"));
        assert!(run.verify_absent("undefined"));
        // A marker that IS present fails the guard (a caught regression).
        assert!(!run.verify_absent("ready"));
        // Empty needle is never a pass.
        assert!(!run.verify_absent(""));
    }

    #[test]
    fn browser_plan_gated_when_approval_required() {
        let o = org();
        // social_operator communicate social_x -> counsel gate -> no plan at all.
        let r = plan_org_browser_login(
            &o,
            &spec("social_operator", "social_x", Capability::Communicate, 0.0),
        );
        match r {
            Err(CredentialBlocked::NeedsApproval { approvers, .. }) => {
                assert!(approvers.contains(&"counsel".to_string()));
            }
            other => panic!("expected needs-approval, got {other:?}"),
        }
    }

    #[test]
    fn browser_plan_uses_vault_pointer_never_literal_secret() {
        let o = org();
        // accountant read banking is Allowed and banking has a secret_ref.
        let mut s = spec("accountant", "banking", Capability::Read, 0.0);
        s.composer_url = Some("https://example.test/new");
        s.composer_selector = Some("#body");
        s.composer_text = "Q4 results are live!";
        let plan = plan_org_browser_login(&o, &s).expect("allowed plan");
        assert_eq!(plan.secret_ref, "UNITBUILDS_BANKING_TOKEN");
        let rendered = plan.describe();
        // The password step must reference the vault pointer, not any secret.
        assert!(
            rendered.contains("vault:UNITBUILDS_BANKING_TOKEN"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("super"),
            "plan leaked a literal secret: {rendered}"
        );
        // Login + composer sequence should produce multiple browser actions.
        assert!(
            plan.actions.len() >= 5,
            "got {} actions",
            plan.actions.len()
        );
        assert!(plan
            .actions
            .iter()
            .any(|a| a.tool == "runtime_session_submit"));
    }

    #[test]
    fn execute_dry_run_blocked_when_approval_required() {
        let dir = tempfile::tempdir().unwrap();
        let o = org();
        // social_operator communicate social_x awaits counsel -> no run at all.
        let r = execute_org_browser_plan(
            dir.path(),
            &o,
            &spec("social_operator", "social_x", Capability::Communicate, 0.0),
            true,
        );
        assert!(
            matches!(r, Err(CredentialBlocked::NeedsApproval { .. })),
            "got {r:?}"
        );
    }

    #[test]
    fn execute_dry_run_substitutes_session_and_never_leaks_secret() {
        // NOTE: dry_run never resolves the vault, so UNITBUILDS_BANKING_TOKEN is
        // deliberately left unset here to avoid colliding with the MISSING tests.
        let dir = tempfile::tempdir().unwrap();
        let o = org();
        let run = execute_org_browser_plan(
            dir.path(),
            &o,
            &spec("accountant", "banking", Capability::Read, 0.0),
            true,
        )
        .expect("allowed dry run");
        let rendered = run.describe(true);
        // Every step is previewed as ready; no placeholders remain.
        assert_eq!(run.steps_ok, run.steps_total, "{rendered}");
        assert!(!rendered.contains("{{sessionId}}"), "{rendered}");
        assert!(run.session_id.starts_with("org-accountant-"), "{rendered}");
        // A dry run must not even expose the vault pointer as a live value.
        assert!(
            !rendered.contains("super-secret-value"),
            "preview leaked secret: {rendered}"
        );
        assert!(rendered.contains("runtime_session_navigate"), "{rendered}");
    }

    fn pl(id: &str) -> PlatformLogin {
        PlatformLogin {
            resource_id: id.to_string(),
            login_url: format!("https://{id}.test/login"),
            username_selector: "#u".to_string(),
            username: "ops@unitbuilds.com".to_string(),
            password_selector: "#p".to_string(),
            submit_selector: "button".to_string(),
            composer_url: None,
            composer_selector: None,
            composer_text: String::new(),
            composer_submit_selector: String::new(),
            settle_ms: 0,
        }
    }

    #[test]
    fn broadcast_gates_each_platform_independently() {
        let o = org();
        // social_operator communicates on both social accounts -> each awaits counsel.
        let res = plan_org_browser_broadcast(
            &o,
            "social_operator",
            Capability::Communicate,
            0.0,
            &[pl("social_x"), pl("social_linkedin")],
        );
        let d = describe_broadcast(&res);
        assert!(d.contains("0 cleared, 2 blocked"), "{d}");
        assert!(d.contains("NEEDS APPROVAL"), "{d}");
    }

    #[test]
    fn broadcast_mixes_denied_and_awaiting_by_grant() {
        let o = org();
        // social_operator has Communicate on social_x (gated) but none on github (denied).
        let res = plan_org_browser_broadcast(
            &o,
            "social_operator",
            Capability::Communicate,
            0.0,
            &[pl("social_x"), pl("github")],
        );
        let d = describe_broadcast(&res);
        assert!(d.contains("0 cleared, 2 blocked"), "{d}");
        assert!(d.contains("DENIED"), "{d}");
        assert!(d.contains("NEEDS APPROVAL"), "{d}");
    }

    #[test]
    fn broadcast_clears_when_read_is_allowed() {
        let o = org();
        // scout holds Read on analytics and social_x -> both clear (no secret needed).
        let res = plan_org_browser_broadcast(
            &o,
            "scout",
            Capability::Read,
            0.0,
            &[pl("analytics"), pl("social_x")],
        );
        let d = describe_broadcast(&res);
        assert!(d.contains("2 cleared, 0 blocked"), "{d}");
    }

    // â”€â”€ GUI dashboard â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn org_dashboard_reports_empty_without_org() {
        let dir = tempfile::tempdir().unwrap();
        let dash = org_dashboard(dir.path());
        assert!(dash.contains("No company org"), "{dash}");
    }

    #[test]
    fn org_dashboard_lists_departments_matrix_and_pending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        assert!(save_company_org(root, &o));
        // Queue a gated social post so the pending section is non-empty.
        submit_action(
            root,
            &o,
            "social_operator",
            "social_x",
            Capability::Communicate,
            0.0,
            "launch tweet",
        );
        let dash = org_dashboard(root);
        assert!(dash.contains("UnitBuilds CC"), "{dash}");
        assert!(
            dash.contains("DEPARTMENTS") && dash.contains("Executive"),
            "{dash}"
        );
        assert!(dash.contains("CAPABILITY MATRIX"), "{dash}");
        assert!(dash.contains("PENDING APPROVALS (1)"), "{dash}");
        assert!(
            dash.contains("social_operator") && dash.contains("counsel"),
            "{dash}"
        );
        // Seeded SOP playbooks and the staffing line are surfaced too.
        assert!(dash.contains("PLAYBOOKS (29)"), "{dash}");
        assert!(dash.contains("ship_github_patch"), "{dash}");
        assert!(dash.contains("open_github_pr"), "{dash}");
        assert!(dash.contains("record_feature_demo"), "{dash}");
        assert!(dash.contains("STAFFING"), "{dash}");
        assert!(dash.contains("not synced yet"), "{dash}");
        // After a sync, the roster reflects the staffed team.
        sync_org_to_expert_team(root, &o);
        let dash2 = org_dashboard(root);
        assert!(dash2.contains("unitbuilds-cc"), "{dash2}");
        assert!(dash2.contains("role(s) staffed"), "{dash2}");
    }

    // â”€â”€ Org â†” ExpertTeam staffing bridge â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn org_team_slug_matches_roster_slugify() {
        let o = org();
        assert_eq!(org_team_slug(&o), "unitbuilds-cc");
    }

    #[test]
    fn sync_projects_org_into_dispatchable_team_and_is_idempotent() {
        use crate::editor::expert_team::load_expert_teams;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let o = org();
        assert!(save_company_org(root, &o));

        let first = sync_org_to_expert_team(root, &o);
        assert_eq!(first.team_slug, "unitbuilds-cc");
        assert_eq!(first.member_count, o.roles.len());
        assert_eq!(first.new_members, o.roles.len());
        assert_eq!(first.carried_over, 0);

        // The team now exists in the roster with members keyed by role id.
        let teams = load_expert_teams(root);
        let team = teams
            .iter()
            .find(|t| t.slug() == "unitbuilds-cc")
            .expect("org team present");
        assert_eq!(team.members.len(), o.roles.len());
        assert!(team.members.iter().any(|m| m.id == "ceo"));
        assert!(team.members.iter().any(|m| m.id == "cfo"));
        // Skills encode grants (resource:capability).
        let cto = team.members.iter().find(|m| m.id == "cto").unwrap();
        assert!(cto.skills.iter().any(|s| s.contains(':')));

        // Re-sync is idempotent: no new members, all carried over.
        let second = sync_org_to_expert_team(root, &o);
        assert_eq!(second.new_members, 0);
        assert_eq!(second.carried_over, o.roles.len());
    }

    #[test]
    fn route_org_role_matches_title_or_department() {
        let o = org();
        assert_eq!(
            route_org_role_for_task(&o, "ask the CFO to review").as_deref(),
            Some("cfo")
        );
        assert_eq!(
            route_org_role_for_task(&o, "engineering should ship a patch").as_deref(),
            Some("dev_lead") // first engineering role matched by department name
        );
        assert_eq!(route_org_role_for_task(&o, "unrelated nonsense"), None);
    }

    /// Seeds the persistent GUI-test workspace with a Pending Spend request so
    /// the IDE org_dashboard shows an APPROVE button for the live click-through.
    /// Run: `cargo test --lib -- --ignored --nocapture seed_p36v_workspace`
    #[test]
    #[ignore]
    fn seed_p36v_workspace() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(".velocity_testing")
            .join("p36v_ws");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join(".velocity")).unwrap();

        let org = ensure_default_org(&root);
        assert!(org.playbooks.len() >= 20);

        let outcome = run_playbook(&root, &org, "pay_vendor_invoice").unwrap();
        match &outcome {
            PlaybookOutcome::AwaitingApproval { request_id, .. } => {
                println!(
                    "SEEDED pending request '{}' on: {}",
                    request_id,
                    root.display()
                );
            }
            other => panic!("expected AwaitingApproval, got {other:?}"),
        }
    }
}
