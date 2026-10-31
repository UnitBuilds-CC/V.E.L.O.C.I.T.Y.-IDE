//! Rule lifecycle: create, adjust, retire, bypass — and the approval gate that
//! separates what an agent may do from what a human or supervisor must sign off.
//!
//! Agents create rules immediately (that is how "never do that again" takes
//! effect mid-run) but may only *propose* loosening or removing one. Durable
//! proposals become [`ChangeRequest`]s; task-scoped rules are process memory and
//! apply at once. Approval lives in the interface, never on the MCP surface, so
//! the model cannot approve its own request.

use std::collections::BTreeMap;
use std::path::Path;

use super::model::*;
use super::store::{self, RulePaths};
use super::verify;

/// A rule as requested by a caller, before it has an id or a witness pair.
#[derive(Debug, Clone)]
pub struct NewRule {
    pub name: String,
    pub description: String,
    pub kind: RuleKind,
    pub params: BTreeMap<String, ParamValue>,
    pub scope: RuleScope,
    pub remedy: Option<String>,
    pub witnesses: Option<Witnesses>,
    pub owner: String,
    pub provenance: String,
}

/// Who is acting. Only these strings get immediate durable changes.
pub const AGENT: &str = "agent";
pub const USER: &str = "user";

fn may_apply_immediately(scope: RuleScope, actor: &str) -> bool {
    scope == RuleScope::Task || actor != AGENT
}

/// Create a rule and make it live immediately at the chosen scope.
pub fn create(paths: &RulePaths, root: &Path, request: NewRule) -> Result<Rule, String> {
    let name = request.name.trim();
    let description = request.description.trim();
    if name.is_empty() {
        return Err("name is required".to_string());
    }
    if description.is_empty() {
        return Err("description is required: it states the intent the refusal will quote"
            .to_string());
    }
    validate_params(request.kind, &request.params)?;
    let now = now_secs();
    let mut rule = Rule {
        id: new_rule_id(),
        name: name.to_string(),
        description: description.to_string(),
        kind: request.kind,
        params: request.params,
        remedy: request
            .remedy
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty()),
        owner: request.owner,
        provenance: request.provenance.trim().to_string(),
        version: 1,
        created_at: now,
        updated_at: now,
        witnesses: request.witnesses,
    };
    verify::prepare(&mut rule)?;
    let stored = rule.clone();
    store::update_scoped(paths, root, request.scope, |file| {
        if file.find(&stored.id).is_some() {
            return Err("rule id collision; retry".to_string());
        }
        file.rules
            .push(RuleRecord::new(request.scope, stored.clone()));
        Ok(())
    })?;
    Ok(rule)
}

/// What happened to an adjust/retire request.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeOutcome {
    /// Boxed: a record is much larger than the request id it is alternated with.
    Applied(Box<RuleRecord>),
    Pending(String),
}

/// Adjust a rule. Agents file a request on durable scopes; humans apply.
pub fn adjust(
    paths: &RulePaths,
    root: &Path,
    rule_id: &str,
    patch: RulePatch,
    actor: &str,
    reason: &str,
) -> Result<ChangeOutcome, String> {
    let scope = find_rule_scope(paths, root, rule_id)?;
    if !may_apply_immediately(scope, actor) {
        let request = file_request(
            paths,
            root,
            scope,
            rule_id,
            ChangeAction::Adjust { patch },
            actor,
            reason,
        )?;
        return Ok(ChangeOutcome::Pending(request));
    }
    let mut applied: Option<RuleRecord> = None;
    store::update_scoped(paths, root, scope, |file| {
        let record = file
            .find_mut(rule_id)
            .ok_or_else(|| format!("rule '{rule_id}' not found"))?;
        if !record.is_active() {
            return Err(format!("rule '{rule_id}' is retired"));
        }
        let candidate = candidate_for(record, patch.clone(), now_secs())?;
        record.supersede(candidate);
        applied = Some(record.clone());
        Ok(())
    })?;
    Ok(ChangeOutcome::Applied(Box::new(
        applied.ok_or("adjust produced no record")?,
    )))
}

/// Retire a rule (a tombstone, never an erase). Agents file a request.
pub fn retire(
    paths: &RulePaths,
    root: &Path,
    rule_id: &str,
    actor: &str,
    reason: &str,
) -> Result<ChangeOutcome, String> {
    let scope = find_rule_scope(paths, root, rule_id)?;
    if !may_apply_immediately(scope, actor) {
        let request = file_request(
            paths,
            root,
            scope,
            rule_id,
            ChangeAction::Retire,
            actor,
            reason,
        )?;
        return Ok(ChangeOutcome::Pending(request));
    }
    let mut applied: Option<RuleRecord> = None;
    store::update_scoped(paths, root, scope, |file| {
        let record = file
            .find_mut(rule_id)
            .ok_or_else(|| format!("rule '{rule_id}' not found"))?;
        if !record.is_active() {
            return Err(format!("rule '{rule_id}' is already retired"));
        }
        record.retired_at = Some(now_secs());
        applied = Some(record.clone());
        file.grants.retain(|g| g.rule_id != rule_id);
        Ok(())
    })?;
    Ok(ChangeOutcome::Applied(Box::new(
        applied.ok_or("retire produced no record")?,
    )))
}

/// What happened to a bypass request.
#[derive(Debug, Clone, PartialEq)]
pub enum BypassOutcome {
    Granted(BypassGrant),
    Pending(String),
}

/// Ask for, or issue, a bypass. `Event` is one call; timed grants expire.
pub fn bypass(
    paths: &RulePaths,
    root: &Path,
    rule_id: &str,
    spec: BypassSpec,
    actor: &str,
) -> Result<BypassOutcome, String> {
    if spec.reason.trim().is_empty() {
        return Err("a bypass needs a reason".to_string());
    }
    if spec.scope != BypassScope::Event && (spec.minutes < 1 || spec.minutes > MAX_BYPASS_MINUTES) {
        return Err(format!(
            "minutes must be between 1 and {MAX_BYPASS_MINUTES} for a timed bypass"
        ));
    }
    let scope = find_rule_scope(paths, root, rule_id)?;
    if !may_apply_immediately(scope, actor) {
        let request = file_request(
            paths,
            root,
            scope,
            rule_id,
            ChangeAction::Bypass { spec },
            actor,
            "bypass requested",
        )?;
        return Ok(BypassOutcome::Pending(request));
    }
    let grant = make_grant(rule_id, &spec, actor);
    let stored = grant.clone();
    store::update_scoped(paths, root, scope, |file| {
        if file.find(rule_id).is_none() {
            return Err(format!("rule '{rule_id}' not found"));
        }
        file.grants.retain(|g| g.rule_id != rule_id);
        file.grants.push(stored.clone());
        Ok(())
    })?;
    Ok(BypassOutcome::Granted(grant))
}

/// Longest timed bypass: one week. A bypass must not become a second delete.
pub const MAX_BYPASS_MINUTES: u32 = 10_080;

/// Approve a pending request and apply it. Only the interface calls this.
pub fn approve_request(
    paths: &RulePaths,
    root: &Path,
    request_id: &str,
    decided_by: &str,
) -> Result<ChangeRequest, String> {
    decide_request(paths, root, request_id, decided_by, true)
}

/// Reject a pending request. The rule is untouched.
pub fn reject_request(
    paths: &RulePaths,
    root: &Path,
    request_id: &str,
    decided_by: &str,
) -> Result<ChangeRequest, String> {
    decide_request(paths, root, request_id, decided_by, false)
}

fn decide_request(
    paths: &RulePaths,
    root: &Path,
    request_id: &str,
    decided_by: &str,
    approve: bool,
) -> Result<ChangeRequest, String> {
    let scope = find_request_scope(paths, root, request_id)?;
    let mut decided: Option<ChangeRequest> = None;
    store::update_scoped(paths, root, scope, |file| {
        let request = file
            .find_request(request_id)
            .cloned()
            .ok_or_else(|| format!("request '{request_id}' not found"))?;
        if request.status != RequestStatus::Pending {
            return Err(format!(
                "request '{request_id}' is already {}",
                request.status.label()
            ));
        }
        if approve {
            apply_request(file, &request, decided_by)?;
        }
        let stored = file.find_request_mut(request_id).expect("request located above");
        stored.status = if approve {
            RequestStatus::Approved
        } else {
            RequestStatus::Denied
        };
        stored.decided_by = Some(decided_by.to_string());
        stored.decided_at = Some(now_secs());
        decided = Some(stored.clone());
        Ok(())
    })?;
    decided.ok_or_else(|| format!("request '{request_id}' was not resolved"))
}

fn apply_request(file: &mut RuleFile, request: &ChangeRequest, decided_by: &str) -> Result<(), String> {
    match &request.action {
        ChangeAction::Adjust { patch } => {
            let record = file
                .find_mut(&request.rule_id)
                .ok_or_else(|| format!("rule '{}' no longer exists", request.rule_id))?;
            if !record.is_active() {
                return Err(format!("rule '{}' is retired", request.rule_id));
            }
            let candidate = candidate_for(record, patch.clone(), now_secs())?;
            record.supersede(candidate);
            Ok(())
        }
        ChangeAction::Retire => {
            let record = file
                .find_mut(&request.rule_id)
                .ok_or_else(|| format!("rule '{}' no longer exists", request.rule_id))?;
            record.retired_at = Some(now_secs());
            file.grants.retain(|g| g.rule_id != request.rule_id);
            Ok(())
        }
        ChangeAction::Bypass { spec } => {
            if file.find(&request.rule_id).is_none() {
                return Err(format!("rule '{}' no longer exists", request.rule_id));
            }
            let grant = make_grant(&request.rule_id, spec, decided_by);
            file.grants.retain(|g| g.rule_id != request.rule_id);
            file.grants.push(grant);
            Ok(())
        }
    }
}

fn candidate_for(record: &RuleRecord, patch: RulePatch, now: u64) -> Result<Rule, String> {
    let current = &record.current;
    let mut candidate = current.clone();
    if let Some(name) = patch.name {
        if name.trim().is_empty() {
            return Err("name cannot be empty".to_string());
        }
        candidate.name = name.trim().to_string();
    }
    if let Some(description) = patch.description {
        if description.trim().is_empty() {
            return Err("description cannot be empty".to_string());
        }
        candidate.description = description.trim().to_string();
    }
    if let Some(params) = patch.params {
        validate_params(candidate.kind, &params)?;
        candidate.params = params;
    }
    if let Some(remedy) = patch.remedy {
        candidate.remedy = Some(remedy.trim().to_string()).filter(|r| !r.is_empty());
    }
    candidate.witnesses = match patch.witnesses {
        Some(witnesses) => Some(witnesses),
        // Parameters changed but no new proof came with them: keep the old pair
        // if it still fires, otherwise synthesize a fresh one where the kind
        // allows it. A command-text rule must bring witnesses with the change.
        None if candidate.params != current.params => None,
        None => current.witnesses.clone(),
    };
    candidate.version = current.version + 1;
    candidate.updated_at = now;
    verify::prepare(&mut candidate)?;
    Ok(candidate)
}

fn file_request(
    paths: &RulePaths,
    root: &Path,
    scope: RuleScope,
    rule_id: &str,
    action: ChangeAction,
    actor: &str,
    reason: &str,
) -> Result<String, String> {
    let request = ChangeRequest {
        id: new_request_id(),
        rule_id: rule_id.to_string(),
        scope,
        action,
        requested_by: actor.to_string(),
        reason: reason.trim().to_string(),
        created_at: now_secs(),
        status: RequestStatus::Pending,
        decided_by: None,
        decided_at: None,
    };
    let id = request.id.clone();
    store::update_scoped(paths, root, scope, |file| {
        if file.find(rule_id).is_none() {
            return Err(format!("rule '{rule_id}' not found"));
        }
        file.requests.push(request.clone());
        Ok(())
    })?;
    Ok(id)
}

fn make_grant(rule_id: &str, spec: &BypassSpec, issued_by: &str) -> BypassGrant {
    let now = now_secs();
    match spec.scope {
        BypassScope::Event => BypassGrant {
            id: new_grant_id(),
            rule_id: rule_id.to_string(),
            scope: BypassScope::Event,
            expires_at: None,
            uses_left: Some(1),
            issued_by: issued_by.to_string(),
            reason: spec.reason.trim().to_string(),
            created_at: now,
        },
        scope => BypassGrant {
            id: new_grant_id(),
            rule_id: rule_id.to_string(),
            scope,
            expires_at: Some(now + spec.minutes as u64 * 60),
            uses_left: None,
            issued_by: issued_by.to_string(),
            reason: spec.reason.trim().to_string(),
            created_at: now,
        },
    }
}

fn validate_params(kind: RuleKind, params: &BTreeMap<String, ParamValue>) -> Result<(), String> {
    for key in params.keys() {
        if !kind.params().iter().any(|spec| spec.key == key) {
            return Err(format!(
                "`{key}` is not a parameter of a {} rule (expected: {})",
                kind.label(),
                kind.params()
                    .iter()
                    .map(|s| s.key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    for spec in kind.params() {
        let Some(value) = params.get(spec.key) else {
            return Err(format!(
                "{} rules need `{}`",
                kind.label(),
                spec.key
            ));
        };
        match (spec.kind, value) {
            (ParamKind::Int { min, max, .. }, ParamValue::Int(v)) => {
                if *v < min || *v > max {
                    return Err(format!(
                        "`{}` must be between {min} and {max}",
                        spec.key
                    ));
                }
            }
            (ParamKind::Int { .. }, _) => {
                return Err(format!("`{}` must be a whole number", spec.key));
            }
            (ParamKind::List(_), ParamValue::List(items)) => {
                let cleaned: Vec<&String> = items.iter().filter(|i| !i.trim().is_empty()).collect();
                if cleaned.is_empty() {
                    return Err(format!(
                        "`{}` needs at least one non-empty entry",
                        spec.key
                    ));
                }
            }
            (ParamKind::List(_), _) => {
                return Err(format!("`{}` must be a list of strings", spec.key));
            }
        }
    }
    Ok(())
}

fn find_rule_scope(paths: &RulePaths, root: &Path, rule_id: &str) -> Result<RuleScope, String> {
    for scope in RuleScope::ALL {
        let file = store::load_scoped(paths, root, scope)?;
        if file.find(rule_id).is_some() {
            return Ok(scope);
        }
    }
    Err(format!("rule '{rule_id}' not found"))
}

fn find_request_scope(paths: &RulePaths, root: &Path, request_id: &str) -> Result<RuleScope, String> {
    for scope in RuleScope::ALL {
        let file = store::load_scoped(paths, root, scope)?;
        if file.find_request(request_id).is_some() {
            return Ok(scope);
        }
    }
    Err(format!("request '{request_id}' not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::matcher::{self, ToolCall};
    use serde_json::json;

    fn paths_in(dir: &Path) -> RulePaths {
        RulePaths::isolated(dir)
    }

    fn egress(name: &str, target: &str, scope: RuleScope) -> NewRule {
        let mut params = BTreeMap::new();
        params.insert(
            "targets".to_string(),
            ParamValue::List(vec![target.to_string()]),
        );
        NewRule {
            name: name.to_string(),
            description: "The live database is off limits.".to_string(),
            kind: RuleKind::EgressBan,
            params,
            scope,
            remedy: Some("use 192.168.0.2 instead".to_string()),
            witnesses: None,
            owner: "user".to_string(),
            provenance: "turn 2".to_string(),
        }
    }

    fn call(cmd: &str) -> ToolCall {
        ToolCall::new("run_command", json!({ "command": cmd }))
    }

    #[test]
    fn a_created_rule_fires_immediately() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "192.168.1.100", RuleScope::Workspace))
            .unwrap();
        assert!(matcher::matches(&rule, &call("psql -h 192.168.1.100")).is_some());
        let hits = crate::rules::engine::evaluate(&paths, tmp.path(), &call("psql -h 192.168.1.100"))
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].scope, RuleScope::Workspace);
    }

    #[test]
    fn a_rule_without_intent_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let mut request = egress("live-db", "10.0.0.5", RuleScope::Workspace);
        request.description = "   ".into();
        let error = create(&paths, tmp.path(), request).unwrap_err();
        assert!(error.contains("description is required"), "{error}");
    }

    #[test]
    fn a_misspelled_parameter_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let mut request = egress("live-db", "10.0.0.5", RuleScope::Workspace);
        request.params.clear();
        request
            .params
            .insert("target".into(), ParamValue::List(vec!["10.0.0.5".into()]));
        let error = create(&paths, tmp.path(), request).unwrap_err();
        assert!(error.contains("not a parameter"), "{error}");
    }

    #[test]
    fn an_agent_adjust_is_pending_and_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let mut params = BTreeMap::new();
        params.insert(
            "targets".into(),
            ParamValue::List(vec!["10.0.0.6".into()]),
        );
        let outcome = adjust(
            &paths,
            tmp.path(),
            &rule.id,
            RulePatch {
                params: Some(params),
                ..Default::default()
            },
            AGENT,
            "narrow the ban",
        )
        .unwrap();
        let ChangeOutcome::Pending(_) = outcome else {
            panic!("an agent adjust must be pending");
        };
        let snapshot = paths.snapshot(tmp.path());
        let (_, record) = snapshot.find(&rule.id).unwrap();
        assert_eq!(record.current.list("targets"), vec!["10.0.0.5"]);
        assert_eq!(record.current.version, 1);
        assert_eq!(snapshot.pending_requests().len(), 1);
    }

    #[test]
    fn a_user_adjust_applies_and_keeps_the_old_version() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let mut params = BTreeMap::new();
        params.insert(
            "targets".into(),
            ParamValue::List(vec!["10.0.0.6".into()]),
        );
        let outcome = adjust(
            &paths,
            tmp.path(),
            &rule.id,
            RulePatch {
                description: Some("Use the staging server only.".into()),
                params: Some(params),
                ..Default::default()
            },
            USER,
            "the server moved",
        )
        .unwrap();
        let ChangeOutcome::Applied(record) = outcome else {
            panic!("a user adjust applies");
        };
        assert_eq!(record.current.version, 2);
        assert_eq!(record.history.len(), 1);
        assert_eq!(record.history[0].list("targets"), vec!["10.0.0.5"]);
        assert_eq!(record.current.description, "Use the staging server only.");
    }

    #[test]
    fn approving_a_request_applies_it_once() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let ChangeOutcome::Pending(request_id) = adjust(
            &paths,
            tmp.path(),
            &rule.id,
            RulePatch {
                description: Some("Tightened wording.".into()),
                ..Default::default()
            },
            AGENT,
            "clarify",
        )
        .unwrap()
        else {
            panic!("pending expected");
        };
        let request = approve_request(&paths, tmp.path(), &request_id, "supervisor").unwrap();
        assert_eq!(request.status, RequestStatus::Approved);
        assert_eq!(request.decided_by.as_deref(), Some("supervisor"));
        let record = paths.snapshot(tmp.path()).find(&rule.id).unwrap().1.clone();
        assert_eq!(record.current.description, "Tightened wording.");
        assert_eq!(record.current.version, 2);
        // A second decision is refused.
        assert!(approve_request(&paths, tmp.path(), &request_id, "supervisor").is_err());
    }

    #[test]
    fn rejecting_a_request_leaves_the_rule_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let ChangeOutcome::Pending(request_id) = retire(&paths, tmp.path(), &rule.id, AGENT, "no longer needed")
            .unwrap()
        else {
            panic!("pending expected");
        };
        reject_request(&paths, tmp.path(), &request_id, "user").unwrap();
        let record = paths.snapshot(tmp.path()).find(&rule.id).unwrap().1.clone();
        assert!(record.is_active());
    }

    #[test]
    fn retiring_is_a_tombstone_not_an_erase() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        retire(&paths, tmp.path(), &rule.id, USER, "done").unwrap();
        let snapshot = paths.snapshot(tmp.path());
        assert!(snapshot.find(&rule.id).unwrap().1.retired_at.is_some());
        assert!(snapshot.active().is_empty());
        assert_eq!(snapshot.retired().len(), 1);
    }

    #[test]
    fn a_task_rule_adjusted_by_an_agent_applies_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Task))
            .unwrap();
        let outcome = adjust(
            &paths,
            tmp.path(),
            &rule.id,
            RulePatch {
                description: Some("Session-only tightening.".into()),
                ..Default::default()
            },
            AGENT,
            "mid-run correction",
        )
        .unwrap();
        assert!(matches!(outcome, ChangeOutcome::Applied(_)));
    }

    #[test]
    fn an_agent_bypass_is_pending_until_approved() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let outcome = bypass(
            &paths,
            tmp.path(),
            &rule.id,
            BypassSpec {
                scope: BypassScope::Event,
                minutes: 0,
                reason: "one migration".into(),
            },
            AGENT,
        )
        .unwrap();
        let BypassOutcome::Pending(request_id) = outcome else {
            panic!("pending expected");
        };
        assert!(paths.snapshot(tmp.path()).live_grants().is_empty());
        approve_request(&paths, tmp.path(), &request_id, "user").unwrap();
        let snapshot = paths.snapshot(tmp.path());
        assert_eq!(snapshot.live_grants().len(), 1);
        assert_eq!(snapshot.live_grants()[0].1.uses_left, Some(1));
    }

    #[test]
    fn a_user_bypass_is_issued_with_an_expiry() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Global))
            .unwrap();
        let outcome = bypass(
            &paths,
            tmp.path(),
            &rule.id,
            BypassSpec {
                scope: BypassScope::Session,
                minutes: 30,
                reason: "debugging".into(),
            },
            USER,
        )
        .unwrap();
        let BypassOutcome::Granted(grant) = outcome else {
            panic!("grant expected");
        };
        assert!(grant.expires_at.unwrap() > now_secs());
        assert!(grant.expires_at.unwrap() <= now_secs() + 30 * 60 + 1);
    }

    #[test]
    fn a_bypass_without_a_reason_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let error = bypass(
            &paths,
            tmp.path(),
            &rule.id,
            BypassSpec {
                scope: BypassScope::Event,
                minutes: 0,
                reason: " ".into(),
            },
            USER,
        )
        .unwrap_err();
        assert!(error.contains("reason"), "{error}");
    }

    #[test]
    fn a_timed_bypass_is_capped() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let rule = create(&paths, tmp.path(), egress("live-db", "10.0.0.5", RuleScope::Workspace))
            .unwrap();
        let error = bypass(
            &paths,
            tmp.path(),
            &rule.id,
            BypassSpec {
                scope: BypassScope::Workspace,
                minutes: MAX_BYPASS_MINUTES + 1,
                reason: "forever".into(),
            },
            USER,
        )
        .unwrap_err();
        assert!(error.contains("between"), "{error}");
    }

    #[test]
    fn a_command_rule_needs_witnesses_at_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let mut params = BTreeMap::new();
        params.insert(
            "patterns".into(),
            ParamValue::List(vec!["drop table".into()]),
        );
        let request = NewRule {
            name: "no-drops".into(),
            description: "Never mutate the schema by hand.".into(),
            kind: RuleKind::CommandPattern,
            params,
            scope: RuleScope::Workspace,
            remedy: None,
            witnesses: None,
            owner: "user".into(),
            provenance: String::new(),
        };
        let error = create(&paths, tmp.path(), request).unwrap_err();
        assert!(error.contains("explicit witnesses"), "{error}");
    }

    #[test]
    fn a_command_rule_with_witnesses_is_accepted_and_fires() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let mut params = BTreeMap::new();
        params.insert(
            "patterns".into(),
            ParamValue::List(vec!["drop table".into()]),
        );
        let request = NewRule {
            name: "no-drops".into(),
            description: "Never mutate the schema by hand.".into(),
            kind: RuleKind::CommandPattern,
            params,
            scope: RuleScope::Workspace,
            remedy: Some("use a migration".into()),
            witnesses: Some(Witnesses {
                deny: WitnessCall::new(
                    "run_command",
                    json!({"command": "psql live -c 'drop table users'"}),
                ),
                allow: WitnessCall::new(
                    "run_command",
                    json!({"command": "psql dev -c 'select 1'"}),
                ),
            }),
            owner: "user".into(),
            provenance: String::new(),
        };
        let rule = create(&paths, tmp.path(), request).unwrap();
        assert!(matcher::matches(&rule, &call("psql live -c 'DROP TABLE x'")).is_some());
    }
}
