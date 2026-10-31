//! Data model for the runtime rules engine.
//!
//! A [`RuleRecord`] owns one rule's immutable version history. A [`Rule`] is a
//! single version: the matcher, the intent, the remedy, and the witness pair that
//! proves the matcher fires. Nothing here knows about files or tool calls — see
//! [`super::store`], [`super::engine`], and [`super::lifecycle`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when the on-disk shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

/// Where a rule lives and how far it reaches.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    /// Process memory only; the narrowest scope, evaluated first.
    #[default]
    Task,
    Workspace,
    Global,
}

impl RuleScope {
    pub const ALL: [RuleScope; 3] = [RuleScope::Task, RuleScope::Workspace, RuleScope::Global];

    pub fn label(self) -> &'static str {
        match self {
            RuleScope::Task => "task",
            RuleScope::Workspace => "workspace",
            RuleScope::Global => "global",
        }
    }

    /// Parse the tool/UI spelling ("task", "workspace", "global"); also accepts
    /// "session" as a synonym for task-scoped work.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "task" | "session" => Some(RuleScope::Task),
            "workspace" | "project" => Some(RuleScope::Workspace),
            "global" | "user" => Some(RuleScope::Global),
            _ => None,
        }
    }

    /// Lower sorts first: the most specific rule wins a tie.
    pub fn rank(self) -> u8 {
        match self {
            RuleScope::Task => 0,
            RuleScope::Workspace => 1,
            RuleScope::Global => 2,
        }
    }
}

/// The kind of call a rule inspects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// Refuse a tool by name.
    #[default]
    ToolBan,
    /// Refuse calls whose arguments name a banned network target.
    EgressBan,
    /// Refuse calls whose path arguments fall under a banned prefix.
    PathBan,
    /// Refuse command/script arguments containing banned text.
    CommandPattern,
    /// Refuse calls once a rolling call count exceeds a ceiling.
    ToolBudget,
}

impl RuleKind {
    pub const ALL: [RuleKind; 5] = [
        RuleKind::ToolBan,
        RuleKind::EgressBan,
        RuleKind::PathBan,
        RuleKind::CommandPattern,
        RuleKind::ToolBudget,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RuleKind::ToolBan => "tool ban",
            RuleKind::EgressBan => "network target ban",
            RuleKind::PathBan => "path ban",
            RuleKind::CommandPattern => "command text ban",
            RuleKind::ToolBudget => "tool-call budget",
        }
    }

    /// The identifier used in JSON, tool parameters, and the rule file.
    pub fn id(self) -> &'static str {
        match self {
            RuleKind::ToolBan => "tool_ban",
            RuleKind::EgressBan => "egress_ban",
            RuleKind::PathBan => "path_ban",
            RuleKind::CommandPattern => "command_pattern",
            RuleKind::ToolBudget => "tool_budget",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "tool_ban" | "tool" => Some(RuleKind::ToolBan),
            "egress_ban" | "egress" | "network_ban" => Some(RuleKind::EgressBan),
            "path_ban" | "path" => Some(RuleKind::PathBan),
            "command_pattern" | "command" => Some(RuleKind::CommandPattern),
            "tool_budget" | "budget" | "turn_limit" => Some(RuleKind::ToolBudget),
            _ => None,
        }
    }

    /// What the description field should explain, shown as a hint in the editor.
    pub fn intent_hint(self) -> &'static str {
        match self {
            RuleKind::ToolBan => "Why this tool must not be used.",
            RuleKind::EgressBan => "What this server is, and why it is off limits.",
            RuleKind::PathBan => "What this path holds and why it must not be touched.",
            RuleKind::CommandPattern => "Which command this forbids and why it is unsafe.",
            RuleKind::ToolBudget => "Why the run must stop after this many calls.",
        }
    }

    /// The parameters this kind reads, in display order.
    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            RuleKind::ToolBan => &[ParamSpec {
                key: "tools",
                label: "Banned tools",
                kind: ParamKind::List("One tool name per entry, e.g. run_command"),
            }],
            RuleKind::EgressBan => &[ParamSpec {
                key: "targets",
                label: "Banned targets",
                kind: ParamKind::List("IPv4, CIDR (10.0.0.0/8) or hostname"),
            }],
            RuleKind::PathBan => &[ParamSpec {
                key: "paths",
                label: "Banned paths",
                kind: ParamKind::List("Path prefix relative to the workspace"),
            }],
            RuleKind::CommandPattern => &[ParamSpec {
                key: "patterns",
                label: "Banned text",
                kind: ParamKind::List("Case-insensitive fragment of a command"),
            }],
            RuleKind::ToolBudget => &[
                ParamSpec {
                    key: "tool_calls",
                    label: "Tool calls",
                    kind: ParamKind::Int { min: 1, max: 100_000, unit: "" },
                },
                ParamSpec {
                    key: "window_minutes",
                    label: "Window",
                    kind: ParamKind::Int { min: 1, max: 10_080, unit: "min" },
                },
            ],
        }
    }
}

/// One editable parameter of a kind.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
}

/// How the UI renders a parameter.
#[derive(Debug, Clone, Copy)]
pub enum ParamKind {
    /// Whole number within bounds; `unit` is appended in the editor.
    Int { min: i64, max: i64, unit: &'static str },
    /// A list of strings, shown as a comma-separated field.
    List(&'static str),
}

/// A typed parameter value. Untagged so the JSON is the natural shape:
/// `"tools": ["run_command"]`, `"tool_calls": 100`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Int(i64),
    Text(String),
    List(Vec<String>),
}

impl ParamValue {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ParamValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[String]> {
        match self {
            ParamValue::List(v) => Some(v),
            _ => None,
        }
    }
}

/// The witness pair that proves a rule can fire and can also stand aside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Witnesses {
    /// A call the rule must allow.
    pub allow: WitnessCall,
    /// A call the rule must block.
    pub deny: WitnessCall,
}

/// One synthetic tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WitnessCall {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

impl WitnessCall {
    pub fn new(tool: impl Into<String>, arguments: Value) -> Self {
        Self {
            tool: tool.into(),
            arguments,
        }
    }
}

/// A single immutable version of a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// Required. The intent appears verbatim in every refusal this rule produces.
    pub description: String,
    pub kind: RuleKind,
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
    /// The alternative to use instead, e.g. `use "192.168.0.2" instead`.
    #[serde(default)]
    pub remedy: Option<String>,
    /// Who created this version: "agent", "user", or a supervisor label.
    pub owner: String,
    /// The turn or instruction this rule came from.
    #[serde(default)]
    pub provenance: String,
    pub version: u32,
    pub created_at: u64,
    pub updated_at: u64,
    /// Proof that the matcher fires. Required for every kind except budgets,
    /// whose parameters are the proof.
    #[serde(default)]
    pub witnesses: Option<Witnesses>,
}

impl Rule {
    pub fn list(&self, key: &str) -> Vec<&str> {
        self.params
            .get(key)
            .and_then(ParamValue::as_list)
            .map(|items| items.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.params.get(key).and_then(ParamValue::as_int)
    }

    /// One line for a list row: the primary parameter, comma-joined.
    pub fn summary(&self) -> String {
        match self.kind {
            RuleKind::ToolBudget => format!(
                "{} calls / {} min",
                self.int("tool_calls").unwrap_or(0),
                self.int("window_minutes").unwrap_or(0)
            ),
            _ => {
                let key = self.kind.params().first().map(|p| p.key).unwrap_or("");
                let items = self.list(key);
                if items.is_empty() {
                    "(no entries)".to_string()
                } else {
                    items.join(", ")
                }
            }
        }
    }
}

/// A rule and its full version history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleRecord {
    pub id: String,
    pub scope: RuleScope,
    pub current: Rule,
    #[serde(default)]
    pub history: Vec<Rule>,
    /// Set when the rule was retired; the record stays for audit.
    #[serde(default)]
    pub retired_at: Option<u64>,
}

impl RuleRecord {
    pub fn new(scope: RuleScope, rule: Rule) -> Self {
        Self {
            id: rule.id.clone(),
            scope,
            current: rule,
            history: Vec::new(),
            retired_at: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.retired_at.is_none()
    }

    /// Replace the current version, keeping the old one in history. The caller
    /// guarantees `next.version == current.version + 1`.
    pub fn supersede(&mut self, next: Rule) {
        let previous = std::mem::replace(&mut self.current, next);
        self.history.push(previous);
    }
}

/// What an approval request asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ChangeAction {
    Adjust { patch: RulePatch },
    Retire,
    Bypass { spec: BypassSpec },
}

/// A partial edit. `None` fields keep their current value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RulePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Option<BTreeMap<String, ParamValue>>,
    #[serde(default)]
    pub remedy: Option<String>,
    #[serde(default)]
    pub witnesses: Option<Witnesses>,
}

impl RulePatch {
    /// A patch that changes nothing must not file a request: the approver would
    /// see a signature request with no diff under it.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.params.is_none()
            && self.remedy.is_none()
            && self.witnesses.is_none()
    }
}

/// How long a bypass lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BypassScope {
    /// Exactly one matching call.
    #[default]
    Event,
    /// Time-boxed, for this process's work session.
    Session,
    /// Time-boxed, recorded in the workspace rule file.
    Workspace,
}

impl BypassScope {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "event" | "single" | "once" => Some(BypassScope::Event),
            "session" => Some(BypassScope::Session),
            "workspace" | "project" => Some(BypassScope::Workspace),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BypassScope::Event => "one call",
            BypassScope::Session => "session",
            BypassScope::Workspace => "workspace",
        }
    }
}

/// The request half of a bypass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BypassSpec {
    pub scope: BypassScope,
    /// Minutes until expiry for session/workspace grants; ignored for events.
    pub minutes: u32,
    pub reason: String,
}

/// A live bypass. Looked up and consumed at the gate, never read by the matcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BypassGrant {
    pub id: String,
    pub rule_id: String,
    pub scope: BypassScope,
    /// Unix expiry for time-boxed grants; `None` for single-use grants.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Remaining uses for single-use grants; `None` for time-boxed grants.
    #[serde(default)]
    pub uses_left: Option<u32>,
    pub issued_by: String,
    pub reason: String,
    pub created_at: u64,
}

impl BypassGrant {
    pub fn is_expired(&self, now: u64) -> bool {
        match self.expires_at {
            Some(at) => now >= at,
            None => self.uses_left == Some(0),
        }
    }
}

/// Status of a change request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied,
}

impl RequestStatus {
    pub fn label(self) -> &'static str {
        match self {
            RequestStatus::Pending => "pending",
            RequestStatus::Approved => "approved",
            RequestStatus::Denied => "denied",
        }
    }
}

/// An agent-filed adjust/retire/bypass awaiting a human or supervisor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeRequest {
    pub id: String,
    pub rule_id: String,
    pub scope: RuleScope,
    pub action: ChangeAction,
    pub requested_by: String,
    pub reason: String,
    pub created_at: u64,
    pub status: RequestStatus,
    #[serde(default)]
    pub decided_by: Option<String>,
    #[serde(default)]
    pub decided_at: Option<u64>,
}

impl ChangeRequest {
    pub fn action_label(&self) -> &'static str {
        match self.action {
            ChangeAction::Adjust { .. } => "adjust",
            ChangeAction::Retire => "retire",
            ChangeAction::Bypass { .. } => "bypass",
        }
    }
}

/// One scope's persisted rule file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleFile {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<RuleRecord>,
    #[serde(default)]
    pub requests: Vec<ChangeRequest>,
    #[serde(default)]
    pub grants: Vec<BypassGrant>,
}

impl Default for RuleFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            rules: Vec::new(),
            requests: Vec::new(),
            grants: Vec::new(),
        }
    }
}

impl RuleFile {
    pub fn active(&self) -> impl Iterator<Item = &RuleRecord> {
        self.rules.iter().filter(|r| r.is_active())
    }

    pub fn find(&self, id: &str) -> Option<&RuleRecord> {
        self.rules.iter().find(|r| r.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut RuleRecord> {
        self.rules.iter_mut().find(|r| r.id == id)
    }

    pub fn find_request(&self, id: &str) -> Option<&ChangeRequest> {
        self.requests.iter().find(|r| r.id == id)
    }

    pub fn find_request_mut(&mut self, id: &str) -> Option<&mut ChangeRequest> {
        self.requests.iter_mut().find(|r| r.id == id)
    }

    /// A grant for `rule_id` that has not expired, ignoring uses.
    pub fn live_grant(&self, rule_id: &str, now: u64) -> Option<&BypassGrant> {
        self.grants
            .iter()
            .find(|g| g.rule_id == rule_id && !g.is_expired(now))
    }

    pub fn live_grant_mut(&mut self, rule_id: &str, now: u64) -> Option<&mut BypassGrant> {
        self.grants
            .iter_mut()
            .find(|g| g.rule_id == rule_id && !g.is_expired(now))
    }

    /// Drop expired grants. Returns whether anything was removed.
    pub fn prune_grants(&mut self, now: u64) -> bool {
        let before = self.grants.len();
        self.grants.retain(|g| !g.is_expired(now));
        before != self.grants.len()
    }
}

/// Unix seconds, the timestamp unit used throughout the rule files.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A session-unique rule id, e.g. `rule-1f2a3b4c`.
pub fn new_rule_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("rule-{:08x}", (nanos ^ (n.wrapping_mul(0x9e37_79b9_7f4a_7c15))) as u32)
}

/// `req-<hex>` and `byp-<hex>`, same uniqueness scheme.
pub fn new_request_id() -> String {
    format!("req-{}", new_rule_id().trim_start_matches("rule-"))
}

pub fn new_grant_id() -> String {
    format!("byp-{}", new_rule_id().trim_start_matches("rule-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_rule(kind: RuleKind) -> Rule {
        Rule {
            id: "rule-00000001".to_string(),
            name: "live-db".to_string(),
            description: "The live database is off limits.".to_string(),
            kind,
            params: BTreeMap::new(),
            remedy: Some("use 192.168.0.2 instead".to_string()),
            owner: "user".to_string(),
            provenance: "turn 4".to_string(),
            version: 1,
            created_at: 1,
            updated_at: 1,
            witnesses: None,
        }
    }

    #[test]
    fn rule_file_round_trips_through_json() {
        let mut file = RuleFile::default();
        let mut rule = sample_rule(RuleKind::EgressBan);
        rule.params.insert(
            "targets".to_string(),
            ParamValue::List(vec!["192.168.1.100".to_string()]),
        );
        file.rules.push(RuleRecord::new(RuleScope::Workspace, rule));
        let text = serde_json::to_string(&file).unwrap();
        let back: RuleFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.rules[0].current.int("tool_calls"), None);
        assert_eq!(back.rules[0].current.list("targets"), vec!["192.168.1.100"]);
    }

    #[test]
    fn scopes_and_kinds_parse_their_spellings() {
        assert_eq!(RuleScope::parse("Session"), Some(RuleScope::Task));
        assert_eq!(RuleScope::parse("project"), Some(RuleScope::Workspace));
        assert_eq!(RuleScope::parse("GLOBAL"), Some(RuleScope::Global));
        assert_eq!(RuleScope::parse("elsewhere"), None);
        assert_eq!(RuleKind::parse("turn_limit"), Some(RuleKind::ToolBudget));
        assert_eq!(RuleKind::parse("egress"), Some(RuleKind::EgressBan));
        assert_eq!(RuleKind::parse("regex"), None);
    }

    #[test]
    fn scope_rank_puts_task_before_global() {
        assert!(RuleScope::Task.rank() < RuleScope::Workspace.rank());
        assert!(RuleScope::Workspace.rank() < RuleScope::Global.rank());
    }

    #[test]
    fn a_superseded_version_is_kept_in_history() {
        let mut record = RuleRecord::new(RuleScope::Workspace, sample_rule(RuleKind::ToolBan));
        let mut next = record.current.clone();
        next.version = 2;
        next.description = "A tighter statement of intent.".to_string();
        record.supersede(next);
        assert_eq!(record.current.version, 2);
        assert_eq!(record.history.len(), 1);
        assert_eq!(record.history[0].version, 1);
        assert!(record.is_active());
    }

    #[test]
    fn grants_expire_by_time_or_use() {
        let event = BypassGrant {
            id: "byp-1".into(),
            rule_id: "rule-1".into(),
            scope: BypassScope::Event,
            expires_at: None,
            uses_left: Some(0),
            issued_by: "user".into(),
            reason: "one-off".into(),
            created_at: 0,
        };
        assert!(event.is_expired(now_secs()));
        let timed = BypassGrant {
            scope: BypassScope::Session,
            expires_at: Some(100),
            uses_left: None,
            ..event.clone()
        };
        assert!(!timed.is_expired(99));
        assert!(timed.is_expired(100));
    }

    #[test]
    fn parameter_types_survive_untagged_serde() {
        let value: ParamValue = serde_json::from_value(json!(100)).unwrap();
        assert_eq!(value.as_int(), Some(100));
        let value: ParamValue = serde_json::from_value(json!(["a", "b"])).unwrap();
        assert_eq!(value.as_list().unwrap(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn budget_summary_reads_both_parameters() {
        let mut rule = sample_rule(RuleKind::ToolBudget);
        rule.params.insert("tool_calls".into(), ParamValue::Int(100));
        rule.params.insert("window_minutes".into(), ParamValue::Int(60));
        assert_eq!(rule.summary(), "100 calls / 60 min");
    }

    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = new_rule_id();
        let b = new_rule_id();
        assert!(a.starts_with("rule-"));
        assert_ne!(a, b);
        assert!(new_request_id().starts_with("req-"));
        assert!(new_grant_id().starts_with("byp-"));
    }
}
