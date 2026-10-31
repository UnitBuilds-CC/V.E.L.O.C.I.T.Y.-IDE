//! Runtime rules: deterministic, scoped, enforced at the tool-dispatch gate.
//!
//! The engine below the model. A rule names a tool call it refuses, states the
//! intent the refusal will quote, and carries a witness pair proving it fires.
//! [`gate_tool_call`] runs from `registry::call_tool_in_workspace` — the one path
//! every MCP client and the embedded agent share — and re-reads the rule files on
//! every call, so a rule created mid-run is live for the next call without a
//! restart.

pub mod engine;
pub mod lifecycle;
pub mod matcher;
pub mod model;
pub mod store;
pub mod verify;

use std::path::Path;

pub use engine::Hit;
pub use lifecycle::{BypassOutcome, ChangeOutcome, NewRule, AGENT, USER};
pub use matcher::ToolCall;
pub use model::*;
pub use store::{RulePaths, RuleSnapshot};

/// Enforce the rules for a tool call at the dispatch choke point.
///
/// Returns the refusal text on a block — it already names the matched resource,
/// the rule, the intent, and the recorded remedy — or the load error when a rule
/// file is unreadable (fail closed).
pub fn gate_tool_call(
    workspace_root: &Path,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<(), String> {
    let paths = RulePaths::for_workspace(workspace_root);
    gate_with_paths(&paths, workspace_root, tool, arguments)
}

/// Same gate with explicit file locations, for tests and harnesses.
pub fn gate_with_paths(
    paths: &RulePaths,
    root: &Path,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<(), String> {
    let call = ToolCall::new(tool, arguments.clone());
    let hits = engine::evaluate(paths, root, &call)?;
    for hit in hits {
        match store::consume_grant(paths, root, &hit.rule.id) {
            Ok(true) => {
                log::info!(
                    "rules: bypassed '{}' for '{}' on one call",
                    hit.rule.id,
                    tool
                );
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                // A bypass lookup that cannot read its file must not silently
                // turn into a block or a pass; treat it as a block with the
                // reason stated.
                log::warn!("rules: grant lookup failed for '{}': {e}", hit.rule.id);
            }
        }
        log::warn!(
            "rules: '{}' blocked '{}': {}",
            hit.rule.id,
            tool,
            hit.detail
        );
        return Err(hit.refusal());
    }
    Ok(())
}

/// All rules across the three scopes, for the interface. Load errors are
/// reported in the snapshot instead of failing: the panel must render and show
/// what is wrong, unlike the gate, which must refuse.
pub fn snapshot(workspace_root: &Path) -> RuleSnapshot {
    RulePaths::for_workspace(workspace_root).snapshot(workspace_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn egress_request(name: &str, target: &str, scope: RuleScope) -> NewRule {
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
            owner: "agent".to_string(),
            provenance: "turn 7: never query the live db".to_string(),
        }
    }

    #[test]
    fn the_gate_refuses_a_banned_target_with_a_directional_message() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = RulePaths::isolated(tmp.path());
        lifecycle::create(
            &paths,
            tmp.path(),
            egress_request("live-db", "192.168.1.100", RuleScope::Workspace),
        )
        .unwrap();
        let error = gate_with_paths(
            &paths,
            tmp.path(),
            "run_command",
            &json!({"command": "psql -h 192.168.1.100 -U app"}),
        )
        .unwrap_err();
        assert!(error.contains("server \"192.168.1.100\""), "{error}");
        assert!(error.contains("live-db"), "{error}");
        assert!(error.contains("off limits"), "{error}");
        assert!(error.contains("Remedy: use 192.168.0.2 instead."), "{error}");
    }

    #[test]
    fn the_gate_allows_the_dev_server() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = RulePaths::isolated(tmp.path());
        lifecycle::create(
            &paths,
            tmp.path(),
            egress_request("live-db", "192.168.1.100", RuleScope::Workspace),
        )
        .unwrap();
        gate_with_paths(
            &paths,
            tmp.path(),
            "run_command",
            &json!({"command": "psql -h 192.168.0.2 -U app"}),
        )
        .unwrap();
    }

    #[test]
    fn a_single_use_grant_lets_exactly_one_call_through() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = RulePaths::isolated(tmp.path());
        let rule = lifecycle::create(
            &paths,
            tmp.path(),
            egress_request("live-db", "192.168.1.100", RuleScope::Workspace),
        )
        .unwrap();
        lifecycle::bypass(
            &paths,
            tmp.path(),
            &rule.id,
            BypassSpec {
                scope: BypassScope::Event,
                minutes: 0,
                reason: "one migration".to_string(),
            },
            USER,
        )
        .unwrap();
        let call = json!({"command": "psql -h 192.168.1.100 -c 'migrate'"});
        gate_with_paths(&paths, tmp.path(), "run_command", &call).unwrap();
        let error = gate_with_paths(&paths, tmp.path(), "run_command", &call).unwrap_err();
        assert!(error.contains("action blocked"), "{error}");
    }

    #[test]
    fn a_corrupt_rule_file_refuses_every_call() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = RulePaths::isolated(tmp.path());
        std::fs::write(&paths.workspace, "{ broken").unwrap();
        let error = gate_with_paths(&paths, tmp.path(), "read_file", &json!({})).unwrap_err();
        assert!(error.contains("rules unavailable"), "{error}");
    }

    #[test]
    fn a_global_rule_blocks_without_a_workspace_file() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = RulePaths::isolated(tmp.path());
        lifecycle::create(
            &paths,
            tmp.path(),
            egress_request("live-db", "10.1.2.3", RuleScope::Global),
        )
        .unwrap();
        let error = gate_with_paths(
            &paths,
            tmp.path(),
            "connector_call",
            &json!({"url": "https://10.1.2.3/x"}),
        )
        .unwrap_err();
        assert!(error.contains("server \"10.1.2.3\""), "{error}");
    }
}
