//! Governance: policy engine + approval queue.
//!
//! The [`PolicyEngine`] evaluates a proposed [`ActionContext`] (a tool call with
//! optional path/domain scope and the run's cumulative token/cost usage) against
//! an ordered list of [`Rule`]s and a [`Budget`], yielding a [`Decision`]:
//! `Allow`, `Deny`, or `NeedsApproval`. When no rules match, the engine falls
//! back to its `default_decision` — which defaults to `Allow`, so an unconfigured
//! workspace behaves exactly as before (advisory, non-blocking).
//!
//! Risky actions that resolve to `NeedsApproval` are parked in the
//! [`ApprovalQueue`] (persisted to `.velocity/approvals.json`) for a human to
//! approve or deny. The policy itself persists to `.velocity/policy.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

const POLICY_FILE: &str = "policy.json";
const APPROVALS_FILE: &str = "approvals.json";

/// The verdict for a proposed action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Allow,
    Deny,
    NeedsApproval,
}

impl Decision {
    pub fn label(&self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::NeedsApproval => "needs-approval",
        }
    }
}

/// The effect a matching [`Rule`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleEffect {
    Allow,
    Deny,
    RequireApproval,
}

impl RuleEffect {
    fn decision(self) -> Decision {
        match self {
            RuleEffect::Allow => Decision::Allow,
            RuleEffect::Deny => Decision::Deny,
            RuleEffect::RequireApproval => Decision::NeedsApproval,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            RuleEffect::Allow => "allow",
            RuleEffect::Deny => "deny",
            RuleEffect::RequireApproval => "require-approval",
        }
    }
}

/// A single policy rule. A rule matches an action when the tool matches (exact
/// name or `*` wildcard) and every specified scope constraint is satisfied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Tool name to match, or `*` for any tool.
    pub tool: String,
    pub effect: RuleEffect,
    /// If set, the action's path must start with this prefix to match.
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// If set, the action's domain/URL must contain this substring to match.
    #[serde(default)]
    pub domain: Option<String>,
}

impl Rule {
    fn matches(&self, action: &ActionContext) -> bool {
        if self.tool != "*" && self.tool != action.tool {
            return false;
        }
        if let Some(prefix) = &self.path_prefix {
            match &action.path {
                Some(path) if path.starts_with(prefix.as_str()) => {}
                _ => return false,
            }
        }
        if let Some(domain) = &self.domain {
            match &action.domain {
                Some(actual) if actual.contains(domain.as_str()) => {}
                _ => return false,
            }
        }
        true
    }
}

/// Per-run resource ceilings. `None` means unlimited.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost_cents: Option<u64>,
}

impl Budget {
    /// Whether the supplied cumulative usage has met or exceeded any limit.
    pub fn exhausted(&self, used_tokens: u64, used_cost_cents: u64) -> bool {
        if let Some(max) = self.max_tokens {
            if used_tokens >= max {
                return true;
            }
        }
        if let Some(max) = self.max_cost_cents {
            if used_cost_cents >= max {
                return true;
            }
        }
        false
    }
}

/// The action being evaluated by the [`PolicyEngine`].
#[derive(Debug, Clone, Default)]
pub struct ActionContext {
    pub tool: String,
    pub path: Option<String>,
    pub domain: Option<String>,
    pub used_tokens: u64,
    pub used_cost_cents: u64,
}

// Builder helpers for constructing ActionContext in tests and production code.
impl ActionContext {
    pub fn tool(name: impl Into<String>) -> Self {
        Self {
            tool: name.into(),
            ..Default::default()
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn with_usage(mut self, tokens: u64, cost_cents: u64) -> Self {
        self.used_tokens = tokens;
        self.used_cost_cents = cost_cents;
        self
    }
}

/// The ordered rule set + budget + default fallback verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEngine {
    /// Verdict when no rule matches. Defaults to [`Decision::Allow`].
    pub default_decision: Decision,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub budget: Budget,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self {
            default_decision: Decision::Allow,
            rules: Vec::new(),
            budget: Budget::default(),
        }
    }
}

impl PolicyEngine {
    /// Load from `.velocity/policy.json`, or a permissive default if absent.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".velocity").join(POLICY_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Persist to `.velocity/policy.json`.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(POLICY_FILE), text).map_err(|e| e.to_string())
    }

    /// Evaluate an action: budget first (exhaustion denies), then the first
    /// matching rule, else the default verdict.
    pub fn evaluate(&self, action: &ActionContext) -> Decision {
        if self
            .budget
            .exhausted(action.used_tokens, action.used_cost_cents)
        {
            return Decision::Deny;
        }
        for rule in &self.rules {
            if rule.matches(action) {
                return rule.effect.decision();
            }
        }
        self.default_decision
    }
}

/// Extract a filesystem path argument from common tool argument shapes.
pub fn arg_path(arguments: &serde_json::Value) -> Option<String> {
    for key in ["relativeFilePath", "path", "file", "dir", "directory"] {
        if let Some(v) = arguments.get(key).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    None
}

/// Extract a URL/domain argument from common tool argument shapes.
pub fn arg_domain(arguments: &serde_json::Value) -> Option<String> {
    for key in ["url", "base_url", "domain", "host"] {
        if let Some(v) = arguments.get(key).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    None
}

/// A short human-readable description of a tool call for the approval queue.
fn describe_action(tool: &str, arguments: &serde_json::Value) -> String {
    match (arg_path(arguments), arg_domain(arguments)) {
        (Some(path), _) => format!("{tool} on {path}"),
        (_, Some(domain)) => format!("{tool} -> {domain}"),
        _ => tool.to_string(),
    }
}

/// Enforce policy for a tool call at the dispatch chokepoint. On `NeedsApproval`
/// the request is parked in the approval queue for human review. Returns `Err`
/// (with a user-facing reason) when the call must not proceed. With no policy
/// file present the engine allows everything, so this is a no-op by default.
pub fn gate_tool_call(
    workspace_root: &Path,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<(), String> {
    let engine = PolicyEngine::load(workspace_root);
    let mut action = ActionContext::tool(tool);
    if let Some(p) = arg_path(arguments) {
        action = action.with_path(p);
    }
    if let Some(d) = arg_domain(arguments) {
        action = action.with_domain(d);
    }
    match engine.evaluate(&action) {
        Decision::Allow => Ok(()),
        Decision::Deny => Err(format!("blocked by policy: tool '{tool}' is denied")),
        Decision::NeedsApproval => {
            let mut queue = ApprovalQueue::load(workspace_root);
            queue.enqueue(tool, describe_action(tool, arguments));
            let _ = queue.save(workspace_root);
            Err(format!(
                "requires approval: tool '{tool}' queued for review (see Governance panel)"
            ))
        }
    }
}

/// Evaluate policy for a tool call with usage tracking (tokens/cost).
/// Returns the decision without enqueuing for approval.
pub fn evaluate_with_usage(
    workspace_root: &Path,
    tool: &str,
    arguments: &serde_json::Value,
    used_tokens: u64,
    used_cost_cents: u64,
) -> Decision {
    let engine = PolicyEngine::load(workspace_root);
    let action = ActionContext::tool(tool)
        .with_path(arg_path(arguments).unwrap_or_default())
        .with_domain(arg_domain(arguments).unwrap_or_default())
        .with_usage(used_tokens, used_cost_cents);
    engine.evaluate(&action)
}

/// Status of a queued approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
}

impl ApprovalStatus {
    pub fn label(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Denied => "denied",
        }
    }
}

/// A risky action parked for human review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalItem {
    pub id: String,
    pub tool: String,
    pub summary: String,
    pub created_at: u64,
    pub status: ApprovalStatus,
}

/// A persisted queue of approval requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalQueue {
    #[serde(default)]
    pub items: Vec<ApprovalItem>,
}

// Management methods for the approval queue, consumed by the Governance panel and policy gate.
impl ApprovalQueue {
    /// Load from `.velocity/approvals.json`, or an empty queue if absent.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".velocity").join(APPROVALS_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Persist to `.velocity/approvals.json`.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(APPROVALS_FILE), text).map_err(|e| e.to_string())
    }

    /// Append a new pending request and return its id.
    pub fn enqueue(&mut self, tool: impl Into<String>, summary: impl Into<String>) -> String {
        let id = format!("apr-{}", now_secs());
        // Disambiguate multiple enqueues within the same second.
        let id = if self.items.iter().any(|i| i.id == id) {
            format!("{id}-{}", self.items.len())
        } else {
            id
        };
        self.items.push(ApprovalItem {
            id: id.clone(),
            tool: tool.into(),
            summary: summary.into(),
            created_at: now_secs(),
            status: ApprovalStatus::Pending,
        });
        id
    }

    fn set_status(&mut self, id: &str, status: ApprovalStatus) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = status;
            true
        } else {
            false
        }
    }

    pub fn approve(&mut self, id: &str) -> bool {
        self.set_status(id, ApprovalStatus::Approved)
    }

    pub fn deny(&mut self, id: &str) -> bool {
        self.set_status(id, ApprovalStatus::Denied)
    }

    /// All still-pending requests.
    pub fn pending(&self) -> Vec<&ApprovalItem> {
        self.items
            .iter()
            .filter(|i| i.status == ApprovalStatus::Pending)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_allows_everything() {
        let engine = PolicyEngine::default();
        let decision = engine.evaluate(&ActionContext::tool("write_file"));
        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn deny_rule_blocks_matching_tool() {
        let mut engine = PolicyEngine::default();
        engine.rules.push(Rule {
            tool: "delete_file".to_string(),
            effect: RuleEffect::Deny,
            path_prefix: None,
            domain: None,
        });
        assert_eq!(
            engine.evaluate(&ActionContext::tool("delete_file")),
            Decision::Deny
        );
        // Non-matching tool still allowed by default.
        assert_eq!(
            engine.evaluate(&ActionContext::tool("read_file")),
            Decision::Allow
        );
    }

    #[test]
    fn path_scope_narrows_rule() {
        let mut engine = PolicyEngine::default();
        engine.rules.push(Rule {
            tool: "write_file".to_string(),
            effect: RuleEffect::RequireApproval,
            path_prefix: Some("src/".to_string()),
            domain: None,
        });
        // Inside scope → needs approval.
        assert_eq!(
            engine.evaluate(&ActionContext::tool("write_file").with_path("src/main.rs")),
            Decision::NeedsApproval
        );
        // Outside scope → default allow.
        assert_eq!(
            engine.evaluate(&ActionContext::tool("write_file").with_path("docs/readme.md")),
            Decision::Allow
        );
    }

    #[test]
    fn domain_scope_matches_substring() {
        let mut engine = PolicyEngine::default();
        engine.rules.push(Rule {
            tool: "*".to_string(),
            effect: RuleEffect::Deny,
            path_prefix: None,
            domain: Some("internal.example".to_string()),
        });
        assert_eq!(
            engine.evaluate(
                &ActionContext::tool("connector_call").with_domain("https://internal.example/api")
            ),
            Decision::Deny
        );
        assert_eq!(
            engine.evaluate(
                &ActionContext::tool("connector_call").with_domain("https://public.example/api")
            ),
            Decision::Allow
        );
    }

    #[test]
    fn first_matching_rule_wins() {
        let mut engine = PolicyEngine::default();
        engine.rules.push(Rule {
            tool: "*".to_string(),
            effect: RuleEffect::Allow,
            path_prefix: None,
            domain: None,
        });
        engine.rules.push(Rule {
            tool: "delete_file".to_string(),
            effect: RuleEffect::Deny,
            path_prefix: None,
            domain: None,
        });
        // The broad allow comes first, so delete_file is allowed.
        assert_eq!(
            engine.evaluate(&ActionContext::tool("delete_file")),
            Decision::Allow
        );
    }

    #[test]
    fn budget_exhaustion_denies() {
        let mut engine = PolicyEngine::default();
        engine.budget.max_tokens = Some(1000);
        // Under budget → allow.
        assert_eq!(
            engine.evaluate(&ActionContext::tool("x").with_usage(500, 0)),
            Decision::Allow
        );
        // At/over budget → deny regardless of rules.
        assert_eq!(
            engine.evaluate(&ActionContext::tool("x").with_usage(1000, 0)),
            Decision::Deny
        );
    }

    #[test]
    fn policy_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = PolicyEngine::default();
        engine.default_decision = Decision::NeedsApproval;
        engine.rules.push(Rule {
            tool: "write_file".to_string(),
            effect: RuleEffect::Allow,
            path_prefix: Some("src/".to_string()),
            domain: None,
        });
        engine.budget.max_cost_cents = Some(250);
        engine.save(tmp.path()).unwrap();

        let loaded = PolicyEngine::load(tmp.path());
        assert_eq!(loaded.default_decision, Decision::NeedsApproval);
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.budget.max_cost_cents, Some(250));
    }

    #[test]
    fn approval_queue_lifecycle_and_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut queue = ApprovalQueue::default();
        let id = queue.enqueue("delete_file", "delete src/old.rs");
        assert_eq!(queue.pending().len(), 1);
        assert!(queue.approve(&id));
        assert_eq!(queue.pending().len(), 0);
        assert_eq!(queue.items[0].status, ApprovalStatus::Approved);
        queue.save(tmp.path()).unwrap();

        let loaded = ApprovalQueue::load(tmp.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.items[0].tool, "delete_file");
    }

    #[test]
    fn arg_extractors_read_common_keys() {
        let v = serde_json::json!({ "relativeFilePath": "src/a.rs" });
        assert_eq!(arg_path(&v), Some("src/a.rs".to_string()));
        let v = serde_json::json!({ "base_url": "https://x.example" });
        assert_eq!(arg_domain(&v), Some("https://x.example".to_string()));
    }
}
