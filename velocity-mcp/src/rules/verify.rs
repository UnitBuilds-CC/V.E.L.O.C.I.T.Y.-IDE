//! Witness verification: a rule is only stored if it can be shown to fire.
//!
//! Each rule carries a pair of synthetic calls — one it must block, one it must
//! allow. The pair is checked against the rule **in isolation** before the
//! version is written, so a typo that makes the matcher never match is rejected
//! at creation instead of failing open for the life of the workspace.
//!
//! Most kinds can synthesize a pair from their own parameters. `command_pattern`
//! cannot: an arbitrary fragment has no derivable command, so those rules must
//! bring explicit witnesses.

use super::matcher::{self, ToolCall};
use super::model::{ParamValue, Rule, RuleKind, WitnessCall, Witnesses};

/// Fill in `rule.witnesses` if absent and prove the pair fires.
pub fn prepare(rule: &mut Rule) -> Result<(), String> {
    if rule.kind != RuleKind::ToolBudget && rule.witnesses.is_none() {
        rule.witnesses = Some(auto_witnesses(rule)?);
    }
    verify(rule)
}

/// Prove the rule blocks its deny witness and allows its allow witness.
pub fn verify(rule: &Rule) -> Result<(), String> {
    if rule.kind == RuleKind::ToolBudget {
        return verify_budget(rule);
    }
    let Some(witnesses) = &rule.witnesses else {
        return Err(format!(
            "rule '{}' has no witness calls; a rule that cannot be shown to fire is not stored",
            rule.name
        ));
    };
    let deny = to_call(&witnesses.deny);
    if matcher::matches(rule, &deny).is_none() {
        return Err(format!(
            "witness is not blocked: {} {:?} does not match this rule",
            deny.tool, deny.arguments
        ));
    }
    let allow = to_call(&witnesses.allow);
    if let Some(detail) = matcher::matches(rule, &allow) {
        return Err(format!(
            "witness allow-call is also blocked ({detail}): {} {:?} must pass this rule",
            allow.tool, allow.arguments
        ));
    }
    Ok(())
}

fn verify_budget(rule: &Rule) -> Result<(), String> {
    let calls = rule.int("tool_calls").unwrap_or(0);
    let window = rule.int("window_minutes").unwrap_or(0);
    if calls < 1 {
        return Err("tool_calls must be at least 1".to_string());
    }
    if window < 1 {
        return Err("window_minutes must be at least 1".to_string());
    }
    Ok(())
}

fn to_call(witness: &WitnessCall) -> ToolCall {
    ToolCall::new(witness.tool.clone(), witness.arguments.clone())
}

/// Synthesize a pair from the rule's primary parameter.
pub fn auto_witnesses(rule: &Rule) -> Result<Witnesses, String> {
    match rule.kind {
        RuleKind::ToolBan => {
            let banned = first_list(rule, "tools")?;
            let allow = ["read_file", "list_dir", "grep_search", "fetch_panel_data"]
                .iter()
                .find(|probe| !banned.iter().any(|b| b.eq_ignore_ascii_case(probe)))
                .ok_or_else(|| {
                    "every probe tool is banned; add a witness allow-call explicitly".to_string()
                })?;
            Ok(Witnesses {
                deny: WitnessCall::new(banned[0].clone(), serde_json::json!({})),
                allow: WitnessCall::new(*allow, serde_json::json!({})),
            })
        }
        RuleKind::EgressBan => {
            let targets = first_list(rule, "targets")?;
            let deny_host = targets[0].clone();
            let allow = ["192.0.2.10", "198.51.100.7", "203.0.113.9", "127.0.0.1"]
                .iter()
                .find_map(|probe| {
                    let call = to_call(&probe_call(probe));
                    matcher::matches(rule, &call).is_none().then_some(*probe)
                })
                .ok_or_else(|| {
                    "every probe address is banned; add a witness allow-call explicitly"
                        .to_string()
                })?;
            Ok(Witnesses {
                deny: probe_call(&deny_host),
                allow: probe_call(allow),
            })
        }
        RuleKind::PathBan => {
            let paths = first_list(rule, "paths")?;
            let prefix = paths[0].trim_end_matches(['/', '\\']);
            let deny = ToolCall::new(
                "read_file",
                serde_json::json!({ "relativeFilePath": format!("{prefix}/witness.txt") }),
            );
            let allow = ["README.md", "docs/witness.md", "src/lib.rs", "Cargo.toml"]
                .iter()
                .find_map(|probe| {
                    let call = ToolCall::new(
                        "read_file",
                        serde_json::json!({ "relativeFilePath": probe }),
                    );
                    matcher::matches(rule, &call).is_none().then_some(*probe)
                })
                .ok_or_else(|| {
                    "every probe path is banned; add a witness allow-call explicitly".to_string()
                })?;
            Ok(Witnesses {
                deny: WitnessCall::new("read_file", deny.arguments),
                allow: WitnessCall::new(
                    "read_file",
                    serde_json::json!({ "relativeFilePath": allow }),
                ),
            })
        }
        RuleKind::CommandPattern => Err(
            "a command-text rule needs explicit witnesses: supply `witnesses.deny` (a call the \
             rule blocks) and `witnesses.allow` (a near-miss it allows)"
                .to_string(),
        ),
        RuleKind::ToolBudget => Err(
            "a budget is verified by its parameters, not by a witness pair".to_string(),
        ),
    }
}

fn first_list(rule: &Rule, key: &str) -> Result<Vec<String>, String> {
    let key_spec = rule
        .kind
        .params()
        .first()
        .map(|spec| spec.key)
        .unwrap_or(key);
    match rule.params.get(key_spec) {
        Some(ParamValue::List(items)) if !items.is_empty() => Ok(items.clone()),
        _ => Err(format!(
            "rule '{}' needs at least one entry in `{key_spec}`",
            rule.name
        )),
    }
}

fn probe_call(host: &str) -> WitnessCall {
    WitnessCall::new(
        "run_command",
        serde_json::json!({ "command": format!("psql -h {host} -U witness") }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::RuleKind;
    use std::collections::BTreeMap;

    fn rule(kind: RuleKind, key: &str, values: &[&str]) -> Rule {
        let mut params = BTreeMap::new();
        params.insert(
            key.to_string(),
            ParamValue::List(values.iter().map(|v| v.to_string()).collect()),
        );
        Rule {
            id: "rule-test".into(),
            name: "test".into(),
            description: "intent".into(),
            kind,
            params,
            remedy: None,
            owner: "user".into(),
            provenance: String::new(),
            version: 1,
            created_at: 0,
            updated_at: 0,
            witnesses: None,
        }
    }

    #[test]
    fn an_egress_rule_gets_a_witness_pair_that_fires() {
        let mut rule = rule(RuleKind::EgressBan, "targets", &["192.168.1.100"]);
        prepare(&mut rule).unwrap();
        let witnesses = rule.witnesses.as_ref().unwrap();
        assert!(matcher::matches(&rule, &to_call(&witnesses.deny)).is_some());
        assert!(matcher::matches(&rule, &to_call(&witnesses.allow)).is_none());
    }

    #[test]
    fn a_path_rule_gets_a_boundary_safe_pair() {
        let mut rule = rule(RuleKind::PathBan, "paths", &["src/secrets"]);
        prepare(&mut rule).unwrap();
        let witnesses = rule.witnesses.as_ref().unwrap();
        assert!(matcher::matches(&rule, &to_call(&witnesses.deny)).is_some());
        assert!(matcher::matches(&rule, &to_call(&witnesses.allow)).is_none());
    }

    #[test]
    fn a_rule_that_bans_every_probe_is_rejected() {
        let mut rule = rule(
            RuleKind::EgressBan,
            "targets",
            &["0.0.0.0/0", "127.0.0.1", "192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24"],
        );
        let error = prepare(&mut rule).unwrap_err();
        assert!(error.contains("every probe address is banned"), "{error}");
    }

    #[test]
    fn a_command_rule_without_witnesses_is_rejected() {
        let mut rule = rule(RuleKind::CommandPattern, "patterns", &["rm -rf"]);
        let error = prepare(&mut rule).unwrap_err();
        assert!(error.contains("needs explicit witnesses"), "{error}");
    }

    #[test]
    fn a_wrong_deny_witness_is_rejected() {
        let mut rule = rule(RuleKind::ToolBan, "tools", &["run_command"]);
        rule.witnesses = Some(Witnesses {
            deny: WitnessCall::new("read_file", serde_json::json!({})),
            allow: WitnessCall::new("list_dir", serde_json::json!({})),
        });
        let error = verify(&rule).unwrap_err();
        assert!(error.contains("witness is not blocked"), "{error}");
    }

    #[test]
    fn an_allow_witness_that_also_matches_is_rejected() {
        let mut rule = rule(RuleKind::ToolBan, "tools", &["run_command"]);
        rule.witnesses = Some(Witnesses {
            deny: WitnessCall::new("run_command", serde_json::json!({})),
            allow: WitnessCall::new("run_command", serde_json::json!({})),
        });
        let error = verify(&rule).unwrap_err();
        assert!(error.contains("allow-call is also blocked"), "{error}");
    }

    #[test]
    fn a_budget_rule_is_verified_by_its_parameters() {
        let mut rule = rule(RuleKind::ToolBudget, "patterns", &[]);
        rule.params
            .insert("tool_calls".into(), ParamValue::Int(0));
        rule.params
            .insert("window_minutes".into(), ParamValue::Int(60));
        assert!(prepare(&mut rule).is_err());
        rule.params
            .insert("tool_calls".into(), ParamValue::Int(100));
        prepare(&mut rule).unwrap();
    }

    #[test]
    fn a_compliant_path_rule_keeps_its_explicit_witnesses() {
        let mut rule = rule(RuleKind::PathBan, "paths", &["secrets"]);
        let explicit = Witnesses {
            deny: WitnessCall::new(
                "read_file",
                serde_json::json!({"relativeFilePath": "secrets/x.pem"}),
            ),
            allow: WitnessCall::new(
                "read_file",
                serde_json::json!({"relativeFilePath": "src/lib.rs"}),
            ),
        };
        rule.witnesses = Some(explicit.clone());
        prepare(&mut rule).unwrap();
        assert_eq!(rule.witnesses, Some(explicit));
    }
}
