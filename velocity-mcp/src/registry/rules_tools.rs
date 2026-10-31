//! The MCP surface for the rules subsystem: read, create, adjust, retire, bypass.
//!
//! Every handler works on the same files the dispatch gate reads, so a rule
//! created here is enforced by the very next call — there is no reload, no
//! restart and no per-agent cache to go stale. Approval is deliberately absent:
//! an agent may file a loosening but must not be able to grant it, so
//! `approve`/`reject` exist only behind the Settings → Rules panel.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use serde_json::{json, Value};

use crate::rules::{
    self, lifecycle,
    model::*,
    store::{self, RulePaths},
    RuleSnapshot,
};

use super::tool_definitions::rules::get_rules_tools;

const RULE_TOOL_NAMES: &[&str] = &[
    "rules_list",
    "rule_show",
    "rule_kinds",
    "rule_create",
    "rule_adjust",
    "rule_retire",
    "rule_bypass",
    "rules_pending",
];

/// True when `name` is one of the rules tools, so callers can claim it before
/// falling through to the unknown-tool path.
pub fn is_rules_tool(name: &str) -> bool {
    RULE_TOOL_NAMES.contains(&name)
}

pub fn handle_rules_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    if !is_rules_tool(name) {
        return Ok(None);
    }
    let paths = RulePaths::for_workspace(root);
    let outcome = match name {
        "rules_list" => rules_list(&paths, root, arguments),
        "rule_show" => rule_show(&paths, root, arguments),
        "rule_kinds" => Ok(rule_kinds_json()),
        "rule_create" => rule_create(&paths, root, arguments),
        "rule_adjust" => rule_adjust(&paths, root, arguments),
        "rule_retire" => rule_retire(&paths, root, arguments),
        "rule_bypass" => rule_bypass(&paths, root, arguments),
        "rules_pending" => rules_pending(&paths, root),
        _ => unreachable!("is_rules_tool admitted only the arms above"),
    };
    match outcome {
        Ok(text) => Ok(Some(text)),
        Err(e) => Ok(Some(
            json!({ "success": false, "detail": e, "tool": name }).to_string(),
        )),
    }
}

fn required_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("`{key}` is required"))
}

fn snapshot_of(paths: &RulePaths, root: &Path) -> RuleSnapshot {
    paths.snapshot(root)
}

fn rules_list(paths: &RulePaths, root: &Path, arguments: &Value) -> Result<String, String> {
    let snapshot = snapshot_of(paths, root);
    let include_retired = arguments["include_retired"].as_bool().unwrap_or(false);
    let mut scopes = Vec::new();
    for scope in RuleScope::ALL {
        let rows = snapshot
            .active()
            .into_iter()
            .filter(|(record_scope, _)| *record_scope == scope)
            .map(|(_, record)| rule_row(&record.current))
            .collect::<Vec<_>>();
        scopes.push(json!({
            "scope": scope.label(),
            "active": rows,
        }));
    }
    let mut out = json!({
        "success": true,
        "scopes": scopes,
        "pending_requests": snapshot.pending_requests().len(),
        "live_bypasses": snapshot.live_grants().len(),
        "note": "Rules are re-read on every tool call. A refusal quotes the description and appends the remedy.",
    });
    if include_retired {
        out["retired"] = json!(snapshot
            .retired()
            .into_iter()
            .map(|(scope, record)| json!({
                "scope": scope.label(),
                "rule": rule_row(&record.current),
                "retired_at": record.retired_at,
            }))
            .collect::<Vec<_>>());
    }
    if !snapshot.errors.is_empty() {
        out["errors"] = json!(snapshot.errors);
        out["detail"] = json!(
            "A rule file could not be read, so every tool call currently refuses. \
             Fix or remove the file; the engine fails closed rather than open."
        );
    }
    Ok(out.to_string())
}

fn rule_row(rule: &Rule) -> Value {
    json!({
        "id": rule.id,
        "name": rule.name,
        "kind": rule.kind.id(),
        "summary": rule.summary(),
        "description": rule.description,
        "remedy": rule.remedy,
        "version": rule.version,
        "owner": rule.owner,
    })
}

fn rule_show(paths: &RulePaths, root: &Path, arguments: &Value) -> Result<String, String> {
    let id = required_str(arguments, "ruleId")?;
    let snapshot = snapshot_of(paths, root);
    let (scope, record) = snapshot
        .find(id)
        .ok_or_else(|| format!("rule '{id}' not found in any scope"))?;
    let grant = snapshot
        .live_grants()
        .into_iter()
        .find(|(_, g)| g.rule_id == id)
        .map(|(_, g)| g.clone());
    let pending = snapshot
        .pending_requests()
        .into_iter()
        .filter(|(_, r)| r.rule_id == id)
        .map(|(_, r)| request_row(r))
        .collect::<Vec<_>>();
    Ok(json!({
        "success": true,
        "scope": scope.label(),
        "rule": record.current,
        "summary": record.current.summary(),
        "parameters": param_specs_json(record.current.kind),
        "history": record.history,
        "retired_at": record.retired_at,
        "live_bypass": grant,
        "pending_requests": pending,
        "refusal_preview": refusal_preview(record),
    })
    .to_string())
}

/// What the model would read if this rule fired right now — the cheapest way to
/// check a rule is directional before depending on it.
fn refusal_preview(record: &RuleRecord) -> Value {
    let detail = match record.current.kind {
        // A budget is not proved by a single call — it fires on the one that
        // crosses the ceiling — so the preview is that first blocked call.
        RuleKind::ToolBudget => {
            let limit = record.current.int("tool_calls").unwrap_or(0);
            rules::engine::budget_detail(
                record.current.int("window_minutes").unwrap_or(0),
                limit,
                limit + 1,
            )
        }
        _ => {
            let Some(witness) = record.current.witnesses.as_ref() else {
                return Value::Null;
            };
            let call =
                rules::ToolCall::new(witness.deny.tool.clone(), witness.deny.arguments.clone());
            match rules::matcher::matches(&record.current, &call) {
                Some(detail) => detail,
                None => return Value::Null,
            }
        }
    };
    Value::String(
        rules::engine::Hit {
            rule: record.current.clone(),
            scope: record.scope,
            detail,
        }
        .refusal(),
    )
}

fn rule_kinds_json() -> String {
    let kinds = RuleKind::ALL
        .iter()
        .copied()
        .map(|kind| {
            json!({
                "kind": kind.id(),
                "label": kind.label(),
                "intent_hint": kind.intent_hint(),
                "parameters": param_specs_json(kind),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "success": true,
        "kinds": kinds,
        "scopes": RuleScope::ALL.iter().map(|s| s.label()).collect::<Vec<_>>(),
        "note": "A rule needs a description (quoted in every refusal) and a remedy (the way to \
                 do the job instead). Witnesses prove it fires; command_pattern must bring its own.",
    })
    .to_string()
}

fn param_specs_json(kind: RuleKind) -> Vec<Value> {
    kind.params()
        .iter()
        .map(|spec| match spec.kind {
            ParamKind::Int { min, max, unit } => json!({
                "key": spec.key,
                "label": spec.label,
                "type": "integer",
                "min": min,
                "max": max,
                "unit": unit,
            }),
            ParamKind::List(hint) => json!({
                "key": spec.key,
                "label": spec.label,
                "type": "string_list",
                "hint": hint,
            }),
        })
        .collect()
}

/// JSON object → typed parameters. Numbers must be whole; arrays become lists.
fn params_from_json(value: &Value) -> Result<BTreeMap<String, ParamValue>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "`params` must be an object of key: value pairs".to_string())?;
    let mut out = BTreeMap::new();
    for (key, raw) in object {
        let parsed = match raw {
            Value::Number(number) => number
                .as_i64()
                .map(ParamValue::Int)
                .ok_or_else(|| format!("`{key}` must be a whole number"))?,
            Value::String(text) => ParamValue::Text(text.clone()),
            Value::Array(items) => {
                let mut cleaned = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Value::String(text) => cleaned.push(text.clone()),
                        other => cleaned.push(other.to_string()),
                    }
                }
                ParamValue::List(cleaned)
            }
            other => {
                return Err(format!(
                    "`{key}` must be a number, a string or a list of strings, not {other}"
                ))
            }
        };
        out.insert(key.clone(), parsed);
    }
    Ok(out)
}

fn rule_create(paths: &RulePaths, root: &Path, arguments: &Value) -> Result<String, String> {
    let kind = RuleKind::parse(required_str(arguments, "kind")?)
        .ok_or_else(|| "unknown rule kind".to_string())?;
    let scope = RuleScope::parse(required_str(arguments, "scope")?)
        .ok_or_else(|| "unknown rule scope".to_string())?;
    let witnesses = witness_pair(arguments)?;
    let request = lifecycle::NewRule {
        name: required_str(arguments, "name")?.to_string(),
        description: required_str(arguments, "description")?.to_string(),
        kind,
        params: params_from_json(&arguments["params"])?,
        scope,
        remedy: arguments["remedy"].as_str().map(|s| s.to_string()),
        witnesses,
        owner: lifecycle::AGENT.to_string(),
        provenance: arguments["provenance"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    };
    let rule = lifecycle::create(paths, root, request)?;
    let stored = snapshot_of(paths, root)
        .find(&rule.id)
        .map(|(_, record)| refusal_preview(record))
        .unwrap_or(Value::Null);
    Ok(json!({
        "success": true,
        "status": "active",
        "id": rule.id,
        "scope": scope.label(),
        "kind": rule.kind.id(),
        "enforced_from": "the next tool call, in every agent using this workspace",
        "refusal_the_agent_will_read": stored,
        "hint": "If the refusal above is null the rule has no deny-witness and does nothing; \
                 retire it and create one that fires.",
    })
    .to_string())
}

fn witness_pair(arguments: &Value) -> Result<Option<Witnesses>, String> {
    let deny_tool = arguments["denyTool"]
        .as_str()
        .filter(|s| !s.trim().is_empty());
    let allow_tool = arguments["allowTool"]
        .as_str()
        .filter(|s| !s.trim().is_empty());
    match (deny_tool, allow_tool) {
        (None, None) => Ok(None),
        (Some(deny), Some(allow)) => Ok(Some(Witnesses {
            deny: WitnessCall::new(deny.to_string(), arguments["denyArguments"].clone()),
            allow: WitnessCall::new(allow.to_string(), arguments["allowArguments"].clone()),
        })),
        _ => Err(
            "a witness pair needs both denyTool and allowTool: one call that must be blocked \
             and one near-miss that must still be allowed"
                .to_string(),
        ),
    }
}

fn rule_adjust(paths: &RulePaths, root: &Path, arguments: &Value) -> Result<String, String> {
    let rule_id = required_str(arguments, "ruleId")?;
    let reason = required_str(arguments, "reason")?;
    let mut patch = RulePatch::default();
    if let Some(name) = arguments["name"].as_str() {
        patch.name = Some(name.to_string());
    }
    if let Some(description) = arguments["description"].as_str() {
        patch.description = Some(description.to_string());
    }
    if let Some(remedy) = arguments["remedy"].as_str() {
        patch.remedy = Some(remedy.to_string());
    }
    if arguments.get("params").is_some() {
        patch.params = Some(params_from_json(&arguments["params"])?);
    }
    if let Some(witnesses) = witness_pair(arguments)? {
        patch.witnesses = Some(witnesses);
    }
    if patch.is_empty() {
        return Err("nothing to change: supply name, description, params or remedy".to_string());
    }
    let outcome = lifecycle::adjust(paths, root, rule_id, patch, lifecycle::AGENT, reason)?;
    Ok(change_outcome_json(outcome))
}

fn change_outcome_json(outcome: lifecycle::ChangeOutcome) -> String {
    match outcome {
        lifecycle::ChangeOutcome::Applied(record) => json!({
            "success": true,
            "status": "applied",
            "id": record.id,
            "version": record.current.version,
            "summary": record.current.summary(),
        })
        .to_string(),
        lifecycle::ChangeOutcome::Pending(request_id) => json!({
            "success": true,
            "status": "pending_approval",
            "request_id": request_id,
            "detail": "The rule is unchanged. A durable rule only loosens once a human or the \
                       team supervisor approves the request in Settings → Rules.",
        })
        .to_string(),
    }
}

fn rule_retire(paths: &RulePaths, root: &Path, arguments: &Value) -> Result<String, String> {
    let rule_id = required_str(arguments, "ruleId")?;
    let reason = required_str(arguments, "reason")?;
    let outcome = lifecycle::retire(paths, root, rule_id, lifecycle::AGENT, reason)?;
    Ok(change_outcome_json(outcome))
}

fn rule_bypass(paths: &RulePaths, root: &Path, arguments: &Value) -> Result<String, String> {
    let rule_id = required_str(arguments, "ruleId")?;
    let scope = BypassScope::parse(required_str(arguments, "bypassScope")?)
        .ok_or_else(|| "bypassScope must be event, session or workspace".to_string())?;
    let spec = BypassSpec {
        scope,
        minutes: arguments["minutes"].as_i64().unwrap_or(0).max(0) as u32,
        reason: arguments["reason"].as_str().unwrap_or_default().to_string(),
    };
    match lifecycle::bypass(paths, root, rule_id, spec, lifecycle::AGENT)? {
        lifecycle::BypassOutcome::Granted(grant) => Ok(json!({
            "success": true,
            "status": "granted",
            "id": grant.id,
            "rule_id": grant.rule_id,
            "scope": grant.scope.label(),
            "expires_at": grant.expires_at,
            "uses_left": grant.uses_left,
        })
        .to_string()),
        lifecycle::BypassOutcome::Pending(request_id) => Ok(json!({
            "success": true,
            "status": "pending_approval",
            "request_id": request_id,
            "detail": "A bypass of a durable rule is issued from Settings → Rules, never by the \
                       agent it releases.",
        })
        .to_string()),
    }
}

fn rules_pending(paths: &RulePaths, root: &Path) -> Result<String, String> {
    let snapshot = snapshot_of(paths, root);
    let requests = snapshot
        .pending_requests()
        .into_iter()
        .map(|(scope, request)| {
            let mut row = request_row(request);
            row["scope"] = json!(scope.label());
            row
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "success": true,
        "count": requests.len(),
        "requests": requests,
        "approve_from": "Settings → Rules → Pending. No MCP tool can approve its own request.",
    })
    .to_string())
}

fn request_row(request: &ChangeRequest) -> Value {
    json!({
        "id": request.id,
        "rule_id": request.rule_id,
        "action": request.action_label(),
        "requested_by": request.requested_by,
        "reason": request.reason,
        "created_at": request.created_at,
        "status": request.status.label(),
    })
}

/// The rules tools reach the surface through the `system` module list, which the
/// group tables walk; this keeps their definitions in one file of their own.
pub fn rules_tool_definitions() -> Vec<super::types::Tool> {
    get_rules_tools()
}

/// Snapshot helper the GUI panel and tests share, so both read exactly what the
/// gate will enforce.
pub fn rules_for(root: &Path) -> RuleSnapshot {
    store::RulePaths::for_workspace(root).snapshot(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn isolated(dir: &Path) -> RulePaths {
        // The same derivation the handler uses: a test that pointed the gate at
        // `RulePaths::isolated` would be checking a file the tools never write.
        RulePaths::for_workspace(dir)
    }

    fn create_live_db(root: &Path) -> String {
        let out = handle_rules_tool(
            root,
            "rule_create",
            &json!({
                "name": "live-db",
                "description": "192.168.1.100 is the production database, not the dev one.",
                "kind": "egress_ban",
                "params": { "targets": ["192.168.1.100"] },
                "scope": "workspace",
                "remedy": "use 192.168.0.2",
                "provenance": "turn 7: never query the live db"
            }),
        )
        .unwrap()
        .unwrap();
        serde_json::from_str::<Value>(&out).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn a_created_rule_is_live_for_the_next_dispatched_call() {
        let tmp = tempfile::tempdir().unwrap();
        // The handler and the gate must agree on where the file lives, or a rule
        // created here would be invisible to the calls that need refusing.
        let paths = isolated(tmp.path());
        let body = handle_rules_tool(
            tmp.path(),
            "rule_create",
            &json!({
                "name": "live-db",
                "description": "192.168.1.100 is the production database.",
                "kind": "egress_ban",
                "params": { "targets": ["192.168.1.100"] },
                "scope": "workspace",
                "remedy": "use 192.168.0.2",
                "provenance": "turn 7"
            }),
        )
        .unwrap()
        .unwrap();
        assert!(body.contains("\"status\":\"active\""), "{body}");
        assert!(paths.workspace.exists());
        let error = rules::gate_with_paths(
            &paths,
            tmp.path(),
            "run_command",
            &json!({"command": "psql -h 192.168.1.100 -U app"}),
        )
        .unwrap_err();
        assert!(error.contains("Remedy: use 192.168.0.2."), "{error}");
    }

    #[test]
    fn the_create_result_shows_the_refusal_the_agent_will_read() {
        let tmp = tempfile::tempdir().unwrap();
        let body = handle_rules_tool(
            tmp.path(),
            "rule_create",
            &json!({
                "name": "live-db",
                "description": "192.168.1.100 is the production database, not the dev one",
                "kind": "egress_ban",
                "params": { "targets": ["192.168.1.100"] },
                "scope": "workspace",
                "remedy": "use 192.168.0.2 instead",
                "provenance": "turn 7"
            }),
        )
        .unwrap()
        .unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        let refusal = parsed["refusal_the_agent_will_read"].as_str().unwrap();
        assert!(refusal.contains("action blocked"), "{refusal}");
        assert!(refusal.contains("192.168.1.100"), "{refusal}");
        assert!(refusal.contains("192.168.0.2"), "{refusal}");
    }

    /// A budget has no single call that proves it, which used to leave its
    /// create-result with a null refusal and the hint reading as if the rule
    /// did nothing. It fires on the call over the ceiling, so that is previewed.
    #[test]
    fn a_budget_rule_previews_the_refusal_of_its_first_blocked_call() {
        let tmp = tempfile::tempdir().unwrap();
        let body = handle_rules_tool(
            tmp.path(),
            "rule_create",
            &json!({
                "name": "shell-ceiling",
                "description": "A run may not churn the shell without reporting",
                "kind": "tool_budget",
                "params": { "tool_calls": 2, "window_minutes": 60 },
                "scope": "workspace",
                "remedy": "summarise what was learned instead",
                "provenance": "housekeeping"
            }),
        )
        .unwrap()
        .unwrap();
        let refusal = serde_json::from_str::<Value>(&body).unwrap()["refusal_the_agent_will_read"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(refusal.contains("3 calls in 60 min (limit 2)"), "{refusal}");
        assert!(
            refusal.contains("Remedy: summarise what was learned instead."),
            "{refusal}"
        );
    }

    #[test]
    fn a_half_witness_pair_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let body = handle_rules_tool(
            tmp.path(),
            "rule_create",
            &json!({
                "name": "no-probe",
                "description": "Blocks a command nobody should run.",
                "kind": "command_pattern",
                "params": { "patterns": ["drop table"] },
                "scope": "workspace",
                "provenance": "turn 3",
                "denyTool": "run_command",
                "denyArguments": { "command": "sqlcmd -Q 'drop table users'" }
            }),
        )
        .unwrap()
        .unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["success"], json!(false), "{parsed}");
        assert!(
            parsed["detail"]
                .as_str()
                .unwrap()
                .contains("both denyTool and allowTool"),
            "{parsed}"
        );
    }

    #[test]
    fn an_agent_adjust_of_a_workspace_rule_is_pending_and_blocks_anyway() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = isolated(tmp.path());
        let id = create_live_db(tmp.path());
        let body = handle_rules_tool(
            tmp.path(),
            "rule_adjust",
            &json!({
                "ruleId": id,
                "params": { "targets": ["192.168.1.101"] },
                "reason": "the ban should cover the replica instead"
            }),
        )
        .unwrap()
        .unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["status"], json!("pending_approval"), "{parsed}");
        // The un-approved change must not have relaxed anything.
        rules::gate_with_paths(
            &paths,
            tmp.path(),
            "run_command",
            &json!({"command": "psql -h 192.168.1.100"}),
        )
        .unwrap_err();
        let pending = handle_rules_tool(tmp.path(), "rules_pending", &json!({}))
            .unwrap()
            .unwrap();
        assert!(pending.contains(id.as_str()), "{pending}");
    }

    #[test]
    fn an_agent_bypass_of_a_workspace_rule_is_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let id = create_live_db(tmp.path());
        let body = handle_rules_tool(
            tmp.path(),
            "rule_bypass",
            &json!({"ruleId": id, "bypassScope": "event", "reason": "one migration"}),
        )
        .unwrap()
        .unwrap();
        assert!(body.contains("pending_approval"), "{body}");
    }

    #[test]
    fn rule_kinds_lists_every_kind_with_its_editable_parameters() {
        let body = rule_kinds_json();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        let kinds = parsed["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), RuleKind::ALL.len());
        let budget = kinds
            .iter()
            .find(|k| k["kind"] == json!("tool_budget"))
            .unwrap();
        let keys = budget["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["key"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["tool_calls", "window_minutes"], "{budget}");
    }

    #[test]
    fn a_budget_rule_is_created_from_whole_numbers_and_shows_its_window() {
        let tmp = tempfile::tempdir().unwrap();
        let body = handle_rules_tool(
            tmp.path(),
            "rule_create",
            &json!({
                "name": "throttle",
                "description": "A run may not hammer the compiler more than twice a minute",
                "kind": "tool_budget",
                "params": { "tool_calls": 2, "window_minutes": 60 },
                "scope": "task",
                "provenance": "housekeeping"
            }),
        )
        .unwrap()
        .unwrap();
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["status"], json!("active"), "{parsed}");
        let shown = handle_rules_tool(
            tmp.path(),
            "rule_show",
            &json!({"ruleId": parsed["id"].as_str().unwrap()}),
        )
        .unwrap()
        .unwrap();
        assert!(shown.contains("\"tool_calls\":2"), "{shown}");
        assert!(shown.contains("2 calls / 60 min"), "{shown}");
    }

    #[test]
    fn a_list_reports_the_rule_that_just_took_effect() {
        let tmp = tempfile::tempdir().unwrap();
        create_live_db(tmp.path());
        let body = handle_rules_tool(tmp.path(), "rules_list", &json!({}))
            .unwrap()
            .unwrap();
        assert!(body.contains("live-db"), "{body}");
        assert!(body.contains("\"scope\":\"workspace\""), "{body}");
    }

    #[test]
    fn an_unknown_rule_id_reports_failure_without_panicking() {
        let tmp = tempfile::tempdir().unwrap();
        let body = handle_rules_tool(tmp.path(), "rule_show", &json!({"ruleId": "rule_404"}))
            .unwrap()
            .unwrap();
        assert!(body.contains("\"success\":false"), "{body}");
    }

    #[test]
    fn every_rules_tool_is_discoverable_and_grouped() {
        let names = get_rules_tools()
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>();
        for name in RULE_TOOL_NAMES {
            assert!(names.iter().any(|n| n == name), "{name} has no definition");
        }
        for tool in get_rules_tools() {
            assert!(
                super::super::tool_groups::group_map().contains_key(&tool.name),
                "{} is advertised but in no group, so a narrowed surface hides it",
                tool.name
            );
        }
    }
}
