//! Rule evaluation at the gate: which active rules fire, and what the model is
//! told when one does.
//!
//! Evaluation reads all three scopes in specificity order (task, workspace,
//! global) so the first refusal the gate reports is the most specific one. Files
//! are revalidated by [`super::store`] on every call: a rule written a moment ago
//! — by this agent, another agent, or another process — is live on the next call.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::matcher::{self, ToolCall};
use super::model::{Rule, RuleKind, RuleScope};
use super::store::{self, RulePaths};

/// One rule that fired on a call.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub rule: Rule,
    pub scope: RuleScope,
    /// What matched, e.g. `server "192.168.1.100"`.
    pub detail: String,
}

impl Hit {
    /// The message the model receives. It must be actionable: what matched, what
    /// rule it broke, why the rule exists, and the recorded alternative.
    pub fn refusal(&self) -> String {
        let mut text = format!(
            "action blocked — {}; rule \"{}\" ({}, {}): {}",
            self.detail,
            self.rule.name,
            self.scope.label(),
            self.rule.id,
            sentence(&self.rule.description),
        );
        match &self.rule.remedy {
            Some(remedy) if !remedy.trim().is_empty() => {
                text.push_str(&format!(" Remedy: {}", sentence(remedy)));
            }
            _ => text.push_str(" No alternative is recorded for this rule."),
        }
        text
    }
}

fn sentence(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.ends_with(['.', '!', '?']) {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

/// Evaluate `call` against every active rule. `Err` means a rule file could not
/// be read: the caller must treat that as a refusal (fail closed).
pub fn evaluate(paths: &RulePaths, root: &Path, call: &ToolCall) -> Result<Vec<Hit>, String> {
    let mut hits = Vec::new();
    for scope in RuleScope::ALL {
        let file = store::load_scoped(paths, root, scope)
            .map_err(|e| format!("rules unavailable: {e}"))?;
        let mut records: Vec<_> = file.active().collect();
        records.sort_by(|a, b| a.id.cmp(&b.id));
        for record in records {
            let rule = &record.current;
            if rule.kind == RuleKind::ToolBudget {
                if let Some(detail) = budget_hit(root, rule) {
                    hits.push(Hit {
                        rule: rule.clone(),
                        scope,
                        detail,
                    });
                }
                continue;
            }
            if let Some(detail) = matcher::matches(rule, call) {
                hits.push(Hit {
                    rule: rule.clone(),
                    scope,
                    detail,
                });
            }
        }
    }
    Ok(hits)
}

// ─── Budgets ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BudgetKey {
    workspace: String,
    rule: String,
    version: u32,
}

fn budgets() -> &'static Mutex<HashMap<BudgetKey, VecDeque<Instant>>> {
    static BUDGETS: OnceLock<Mutex<HashMap<BudgetKey, VecDeque<Instant>>>> = OnceLock::new();
    BUDGETS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record this call and report whether the rolling window is over its ceiling.
/// Counting every call (not just blocked ones) keeps the window a true rate.
fn budget_hit(root: &Path, rule: &Rule) -> Option<String> {
    let limit = rule.int("tool_calls").unwrap_or(0);
    let window = rule.int("window_minutes").unwrap_or(0);
    if limit < 1 || window < 1 {
        // Invalid parameters were rejected at creation; a corrupted file could
        // still carry them. Refuse rather than silently never firing.
        return Some(format!("tool-call budget for \"{}\" is invalid", rule.name));
    }
    let key = BudgetKey {
        workspace: root.to_string_lossy().to_string(),
        rule: rule.id.clone(),
        version: rule.version,
    };
    let now = Instant::now();
    let mut store = budgets().lock().unwrap_or_else(|e| e.into_inner());
    // Bound the map: budgets are per process and per rule version, and rule
    // versions are rare; this only guards against pathological churn.
    if store.len() > 512 {
        store.clear();
    }
    let queue = store.entry(key).or_default();
    let exceeded = record_and_check(queue, now, limit, Duration::from_secs(window as u64 * 60));
    exceeded.then(|| budget_detail(window, limit, queue.len() as i64))
}

/// The wording of a budget refusal. Public so the tool surface can quote the
/// same sentence at creation time: what an agent reads when it files the rule
/// must be what it reads when the rule fires.
pub fn budget_detail(window: i64, limit: i64, count: i64) -> String {
    format!("tool-call budget exceeded: {count} calls in {window} min (limit {limit})")
}

/// Pure window arithmetic, split out so the rolling boundary is testable.
fn record_and_check(
    queue: &mut VecDeque<Instant>,
    now: Instant,
    limit: i64,
    window: Duration,
) -> bool {
    queue.push_back(now);
    let cutoff = now.checked_sub(window).unwrap_or(now);
    while queue.front().is_some_and(|instant| *instant < cutoff) {
        queue.pop_front();
    }
    queue.len() as i64 > limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::{ParamValue, RuleRecord};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn paths_in(dir: &Path) -> RulePaths {
        RulePaths::isolated(dir)
    }

    fn rule(kind: RuleKind) -> Rule {
        Rule {
            id: "rule-00000001".into(),
            name: "live-db".into(),
            description: "The live database is off limits, use the dev server".into(),
            kind,
            params: BTreeMap::new(),
            remedy: Some("use \"192.168.0.2\" instead".into()),
            owner: "user".into(),
            provenance: "turn 3".into(),
            version: 1,
            created_at: 0,
            updated_at: 0,
            witnesses: None,
        }
    }

    fn egress_rule(target: &str) -> Rule {
        let mut rule = rule(RuleKind::EgressBan);
        rule.params
            .insert("targets".into(), ParamValue::List(vec![target.to_string()]));
        rule
    }

    #[test]
    fn the_refusal_names_the_target_the_intent_and_the_remedy() {
        let hit = Hit {
            rule: egress_rule("192.168.1.100"),
            scope: RuleScope::Workspace,
            detail: "server \"192.168.1.100\"".into(),
        };
        let text = hit.refusal();
        assert!(
            text.starts_with("action blocked — server \"192.168.1.100\""),
            "{text}"
        );
        assert!(
            text.contains("rule \"live-db\" (workspace, rule-00000001)"),
            "{text}"
        );
        assert!(
            text.contains("The live database is off limits, use the dev server."),
            "{text}"
        );
        assert!(
            text.contains("Remedy: use \"192.168.0.2\" instead."),
            "{text}"
        );
    }

    #[test]
    fn a_rule_without_a_remedy_says_so_instead_of_inventing_one() {
        let mut rule = egress_rule("10.0.0.5");
        rule.remedy = None;
        let hit = Hit {
            rule,
            scope: RuleScope::Global,
            detail: "server \"10.0.0.5\"".into(),
        };
        assert!(hit.refusal().contains("No alternative is recorded"));
    }

    #[test]
    fn scopes_are_evaluated_most_specific_first() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        for (scope, id) in [
            (RuleScope::Global, "rule-global"),
            (RuleScope::Task, "rule-task"),
        ] {
            let mut r = egress_rule("10.9.9.9");
            r.id = id.into();
            store::update_scoped(&paths, tmp.path(), scope, |file| {
                file.rules.push(RuleRecord::new(scope, r.clone()));
                Ok(())
            })
            .unwrap();
        }
        let call = ToolCall::new("run_command", json!({"command": "curl http://10.9.9.9/"}));
        let hits = evaluate(&paths, tmp.path(), &call).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].scope, RuleScope::Task);
        assert_eq!(hits[1].scope, RuleScope::Global);
    }

    #[test]
    fn a_non_matching_call_produces_no_hits() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        store::update_scoped(&paths, tmp.path(), RuleScope::Workspace, |file| {
            file.rules.push(RuleRecord::new(
                RuleScope::Workspace,
                egress_rule("192.168.1.100"),
            ));
            Ok(())
        })
        .unwrap();
        let call = ToolCall::new("run_command", json!({"command": "psql -h 192.168.0.2"}));
        assert!(evaluate(&paths, tmp.path(), &call).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_rule_file_is_an_error_not_an_empty_set() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        std::fs::write(&paths.workspace, "not json").unwrap();
        let error =
            evaluate(&paths, tmp.path(), &ToolCall::new("read_file", json!({}))).unwrap_err();
        assert!(error.contains("rules unavailable"), "{error}");
    }

    #[test]
    fn a_budget_blocks_the_call_after_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut rule = rule(RuleKind::ToolBudget);
        rule.id = "rule-budget-test".into();
        rule.params.insert("tool_calls".into(), ParamValue::Int(2));
        rule.params
            .insert("window_minutes".into(), ParamValue::Int(60));
        assert!(budget_hit(root, &rule).is_none());
        assert!(budget_hit(root, &rule).is_none());
        let detail = budget_hit(root, &rule).expect("third call must exceed");
        assert!(detail.contains("limit 2"), "{detail}");
    }

    #[test]
    fn the_window_drops_calls_older_than_its_span() {
        let now = Instant::now();
        let mut queue = VecDeque::new();
        queue.push_back(now - Duration::from_secs(120));
        assert!(!record_and_check(
            &mut queue,
            now,
            1,
            Duration::from_secs(60)
        ));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn a_new_budget_version_starts_a_fresh_window() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut rule = rule(RuleKind::ToolBudget);
        rule.id = "rule-budget-versioned".into();
        rule.params.insert("tool_calls".into(), ParamValue::Int(1));
        rule.params
            .insert("window_minutes".into(), ParamValue::Int(60));
        assert!(budget_hit(root, &rule).is_none());
        assert!(budget_hit(root, &rule).is_some());
        rule.version = 2;
        assert!(budget_hit(root, &rule).is_none());
    }

    #[test]
    fn a_budget_rule_is_reported_as_a_hit_by_evaluate() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(tmp.path());
        let mut rule = rule(RuleKind::ToolBudget);
        rule.id = "rule-eval-budget".into();
        rule.params.insert("tool_calls".into(), ParamValue::Int(1));
        rule.params
            .insert("window_minutes".into(), ParamValue::Int(60));
        store::update_scoped(&paths, tmp.path(), RuleScope::Workspace, |file| {
            file.rules.push(RuleRecord::new(RuleScope::Workspace, rule));
            Ok(())
        })
        .unwrap();
        let call = ToolCall::new("read_file", json!({"relativeFilePath": "README.md"}));
        assert!(evaluate(&paths, tmp.path(), &call).unwrap().is_empty());
        let hits = evaluate(&paths, tmp.path(), &call).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].detail.contains("budget exceeded"));
    }
}
