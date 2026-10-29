//! Workflow composer: sequential + branching multi-step automation.
//!
//! A [`Workflow`] is an ordered list of [`WorkflowStep`]s. [`Workflow::execute`]
//! runs them in order, reusing the MCP tool dispatch
//! ([`crate::registry::dispatch::call_tool_in_workspace`]) for `Tool` steps and
//! the headless agent runtime for `AgentTask` steps. A `Condition` step inspects
//! the previous step's [`StepOutcome`] and short-circuits the remaining steps
//! when its requirement is not met. Each step is captured in a [`StepRecord`],
//! and the whole run produces a [`WorkflowRun`] for the run log and governance
//! audit.
//!
//! Workflows persist as individual JSON files under `.velocity/workflows/`, one
//! file per workflow id, loaded/saved by [`WorkflowRegistry`].
//!
//! ## Module hierarchy
//!
//! - `mod.rs` — core workflow types and registry
//! - `ai.rs` — AI-powered natural language workflow generation
//! - `canvas.rs` — visual node-based workflow editor
//! - `templates.rs` — pre-built workflow templates
//! - `version.rs` — workflow version history and rollback

pub mod ai;
pub mod canvas;
pub mod templates;
pub mod version;

// Re-export for backward compatibility
pub use ai::*;
pub use canvas::*;
pub use templates::*;
pub use version::*;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const WORKFLOWS_DIR: &str = "workflows";

/// A single step in a workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkflowStep {
    /// Dispatch a free-form prompt to a headless agent (optionally a named team).
    AgentTask {
        prompt: String,
        team: Option<String>,
    },
    /// Invoke a registered MCP tool with JSON arguments.
    Tool {
        name: String,
        args: serde_json::Value,
    },
    /// Invoke a configured connector by id (wired in Pillar 2).
    Connector { id: String, req: serde_json::Value },
    /// Continue only if the previous step's outcome matches `require` AND, when
    /// `on_text` is set, the previous step's *output text* satisfies the verdict
    /// predicate; otherwise short-circuit the remaining steps. When the verdict
    /// is missing, an optional bounded [`Recheck`] re-*observes* the page (never
    /// re-acts on it) before the gate halts.
    Condition {
        require: StepOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_text: Option<TextPredicate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recheck: Option<Recheck>,
    },
    /// Deterministic fan-out: run `body` once per element of the JSON array found
    /// at `ctx[source]` (or, when `source == "lastOutput"`, parsed from the prior
    /// step's output). Each element is bound into a child context under `as_var`,
    /// so `{{invoice.id}}` inside a body step's args resolves to the current
    /// item. Any iteration whose sub-run is not `Success` marks the whole
    /// ForEach step Failed and halts the parent workflow (via the existing
    /// short-circuit rule) — no partial-completion drift. N approvals → N actions
    /// with N audit receipts, without a single AI inference.
    ForEach {
        source: String,
        as_var: String,
        body: Vec<WorkflowStep>,
    },
}

/// Bounded re-verification attached to a verdict gate. Real pages settle late:
/// a post that has not rendered yet and a post that never landed look identical
/// on the first read. A recheck re-runs an **observation-only** action (a fresh
/// read) up to `attempts` times with `settle_ms` between tries and re-tests the
/// predicate against what it returns. If the verdict never arrives the gate
/// halts exactly as before — and because the engine refuses an outward tool, a
/// retry can never replay the action whose verdict is missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recheck {
    /// Must name an observation-only tool; see [`recheck_is_observation_only`].
    pub tool: String,
    #[serde(default)]
    pub args: serde_json::Value,
    /// Extra observations after the first failing verdict (minimum 1).
    pub attempts: u32,
    /// Deterministic settle pause between observations, in milliseconds.
    #[serde(default)]
    pub settle_ms: u64,
}

/// Tools that are outward by nature: they change state somewhere else. A recheck
/// may never name one, so a missing verdict can never be "retried" by performing
/// the action again.
const OUTWARD_TOOLS: &[&str] = &[
    "request_org_action",
    "approve_org_action",
    "reject_org_action",
    "dispatch_org_role",
    "run_org_playbook",
    "resume_org_playbook",
    "run_due_org_playbooks",
    "execute_org_credential_rotation_after_approval",
    "grant_org_credential_elevation",
    "consume_org_credential_elevation",
    "write_file",
    "edit_file",
    "create_file",
    "delete_file",
];

/// Tools that can go either way: the same executor that reads a page can submit
/// a form. Allowed for a recheck only under a closed-world reading of the args.
const GUARDED_TOOLS: &[&str] = &[
    "execute_org_browser_task",
    "execute_org_browser_task_for_request",
    "execute_org_browser_broadcast",
];

/// Arg keys that turn a browser call into an *action*. Anything else present on a
/// guarded recheck is an observation; the list is closed-world so an unlisted
/// future verb defaults to refusing the recheck rather than replaying it.
const MUTATING_BROWSER_KEYS: &[&str] = &[
    "form_fields",
    "form_submit_selector",
    "extra_actions",
    "upload_files",
    "upload_selector",
    "click_selector",
    "press_key",
    "type_text",
    "download_selector",
    "dialog_action",
    "set_cookie",
    "set_storage",
    "drag_and_drop",
    "right_click",
    "select_option",
    "navigate",
    "platforms",
    "message",
];

/// Whether a recheck is guaranteed to be a fresh observation with no side
/// effect: not an outward tool, and for the browser executors a `read_script`
/// with no action keys at all.
pub fn recheck_is_observation_only(name: &str, args: &serde_json::Value) -> bool {
    if OUTWARD_TOOLS.contains(&name) {
        return false;
    }
    if GUARDED_TOOLS.contains(&name) {
        let reads = args
            .get("read_script")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        let acts = MUTATING_BROWSER_KEYS.iter().any(|k| args.get(*k).is_some());
        return reads && !acts;
    }
    true
}

/// A deterministic, secret-free substring predicate evaluated against the
/// *output text* of the previous step. Lets a playbook branch on a semantic
/// verdict — a browser `VERIFY FAIL` / `ABSENT FAIL`, an approval gate's
/// `DENIED` / `NEEDS APPROVAL`, a JIT elevation-deny — instead of only on
/// whether the tool call itself returned an error. No inference: plain
/// containment (`contains_all` must all appear, `contains_none` must not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TextPredicate {
    /// Every one of these substrings must be present in the prior output.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contains_all: Vec<String>,
    /// None of these substrings may be present in the prior output.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contains_none: Vec<String>,
}

impl TextPredicate {
    /// Whether `text` satisfies the predicate (empty predicate trivially holds).
    pub fn holds(&self, text: &str) -> bool {
        self.contains_all
            .iter()
            .all(|needle| text.contains(needle.as_str()))
            && self
                .contains_none
                .iter()
                .all(|needle| !text.contains(needle.as_str()))
    }
}

impl WorkflowStep {
    /// Short human label describing the step kind.
    pub fn kind_label(&self) -> String {
        match self {
            WorkflowStep::AgentTask { .. } => "agent".to_string(),
            WorkflowStep::Tool { name, .. } => format!("tool:{name}"),
            WorkflowStep::Connector { id, .. } => format!("connector:{id}"),
            WorkflowStep::Condition { require, .. } => format!("condition=={}", require.label()),
            WorkflowStep::ForEach { source, body, .. } => {
                format!("foreach[{source}]×{}", body.len())
            }
        }
    }
}

/// Outcome of executing a single step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOutcome {
    Ok,
    Failed,
    Skipped,
}

impl StepOutcome {
    pub fn label(self) -> &'static str {
        match self {
            StepOutcome::Ok => "ok",
            StepOutcome::Failed => "failed",
            StepOutcome::Skipped => "skipped",
        }
    }
}

/// The recorded result of one executed step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    pub index: usize,
    pub kind: String,
    pub outcome: StepOutcome,
    pub output: String,
}

/// Overall status of a completed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Success,
    Partial,
    Failed,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            RunStatus::Success => "success",
            RunStatus::Partial => "partial",
            RunStatus::Failed => "failed",
        }
    }
}

/// Per-run metering of the deterministic-vs-inference split. The company
/// objective says *"playbook wiring prefers deterministic workflow steps over
/// AI inference to cut cost and raise reliability"* — this struct turns that
/// design intent into a measurable per-run fact. `agent_steps` counts
/// [`WorkflowStep::AgentTask`] invocations (the only variant that routes to a
/// headless LLM); every other variant (`Tool`, `Connector`, `Condition`,
/// `ForEach`) is deterministic. ForEach sub-runs are folded into the parent's
/// tally, so a N-item fan-out over a 2-tool body contributes `2·N` tool_calls
/// and 1 foreach_step — that is what makes the ratio honest for real SOPs
/// rather than only counting top-level steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeterminismReport {
    pub agent_steps: usize,
    pub tool_calls: usize,
    pub connector_calls: usize,
    pub condition_checks: usize,
    pub foreach_steps: usize,
}

impl DeterminismReport {
    /// Total executed step invocations, deterministic + agent.
    pub fn total(&self) -> usize {
        self.agent_steps
            + self.tool_calls
            + self.connector_calls
            + self.condition_checks
            + self.foreach_steps
    }
    /// Share of executed steps that were deterministic (0.0–1.0). A pure
    /// `AgentTask` workflow scores 0.0; a pure Tool / Condition / ForEach
    /// workflow scores 1.0. A run with no executed steps returns 0.0 so a
    /// skipped/empty SOP cannot masquerade as 100% deterministic.
    pub fn deterministic_ratio(&self) -> f64 {
        let t = self.total();
        if t == 0 {
            0.0
        } else {
            (t - self.agent_steps) as f64 / t as f64
        }
    }
    /// One-line human summary used in the verdict ledger + org dashboard.
    pub fn summary_line(&self) -> String {
        let pct = (self.deterministic_ratio() * 100.0).round() as i32;
        format!(
            "deterministic {pct}% — agent={} tool={} connector={} condition={} foreach={}",
            self.agent_steps,
            self.tool_calls,
            self.connector_calls,
            self.condition_checks,
            self.foreach_steps,
        )
    }
    /// Fold another report in (used to aggregate ForEach sub-runs so inner
    /// tool/agent calls are not lost behind the outer foreach step).
    pub fn merge(&mut self, other: &DeterminismReport) {
        self.agent_steps += other.agent_steps;
        self.tool_calls += other.tool_calls;
        self.connector_calls += other.connector_calls;
        self.condition_checks += other.condition_checks;
        self.foreach_steps += other.foreach_steps;
    }
}

/// The record of a single workflow execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub workflow_id: String,
    pub status: RunStatus,
    pub steps: Vec<StepRecord>,
    pub started_at: u64,
    pub finished_at: u64,
    /// Metering of inference-vs-deterministic executed steps. `#[serde(
    /// default)]` so historical run logs without this field still load.
    #[serde(default)]
    pub determinism: DeterminismReport,
}

impl WorkflowRun {
    /// Number of steps that completed successfully.
    pub fn ok_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.outcome == StepOutcome::Ok)
            .count()
    }
}

/// A named, ordered sequence of steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

impl Workflow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            steps: Vec::new(),
        }
    }

    /// Execute the workflow sequentially against `workspace_root`, producing a
    /// [`WorkflowRun`]. A failed `Condition` step short-circuits the remainder
    /// (recorded as `Skipped`). Non-condition failures are recorded but do not
    /// halt execution — use a `Condition` to gate downstream steps.
    pub fn execute(&self, workspace_root: &Path) -> WorkflowRun {
        self.execute_with_context(workspace_root, &serde_json::Value::Null)
    }

    /// Like [`Workflow::execute`], but substitutes `{{key}}` placeholders inside
    /// every [`WorkflowStep::Tool`] `args` from `ctx` (a JSON object of named
    /// values) and `{{lastOutput}}` with the previous step's captured output
    /// before running the step. This lets a caller thread live per-run values —
    /// e.g. an approved action `requestId` or an agent-drafted body — into
    /// otherwise-static playbook steps without hard-coding them. Non-string leaf
    /// values and unknown placeholders are left untouched, so an empty `ctx`
    /// behaves exactly like [`Workflow::execute`].
    pub fn execute_with_context(
        &self,
        workspace_root: &Path,
        ctx: &serde_json::Value,
    ) -> WorkflowRun {
        let started_at = now_secs();
        let mut records = Vec::with_capacity(self.steps.len());
        let mut det = DeterminismReport::default();
        let mut short_circuit = false;
        let mut prior: Option<StepOutcome> = None;
        let mut prior_output: Option<String> = None;
        let mut last_output = String::new();

        for (index, step) in self.steps.iter().enumerate() {
            if short_circuit {
                records.push(StepRecord {
                    index,
                    kind: step.kind_label(),
                    outcome: StepOutcome::Skipped,
                    output: "skipped (prior condition not met)".to_string(),
                });
                continue;
            }

            // Resolve any {{placeholders}} in a Tool step's args from the live
            // context before dispatching it (a Condition's recheck reads with the
            // same context, so its args are bound too).
            let bound;
            let step = match step {
                WorkflowStep::Tool { name, args } => {
                    let mut a = args.clone();
                    substitute_ctx(&mut a, ctx, &last_output);
                    bound = WorkflowStep::Tool {
                        name: name.clone(),
                        args: a,
                    };
                    &bound
                }
                WorkflowStep::Condition {
                    require,
                    on_text,
                    recheck: Some(r),
                } => {
                    let mut a = r.args.clone();
                    substitute_ctx(&mut a, ctx, &last_output);
                    bound = WorkflowStep::Condition {
                        require: *require,
                        on_text: on_text.clone(),
                        recheck: Some(Recheck {
                            tool: r.tool.clone(),
                            args: a,
                            attempts: r.attempts,
                            settle_ms: r.settle_ms,
                        }),
                    };
                    &bound
                }
                other => other,
            };

            let (outcome, output) = if let WorkflowStep::ForEach {
                source,
                as_var,
                body,
            } = step
            {
                // Fan-out: resolve the source array (either from `ctx[source]` or,
                // for `source == "lastOutput"`, by JSON-parsing the prior step's
                // output). Bind each element under `as_var` in a child ctx and run
                // the body as its own Workflow so Conditions/Rechecks inside behave
                // the same as at the top level.
                let arr: Vec<serde_json::Value> = ctx
                    .get(source)
                    .and_then(|v| v.as_array())
                    .cloned()
                    .or_else(|| {
                        if source == "lastOutput" {
                            serde_json::from_str::<Vec<serde_json::Value>>(&last_output).ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                if arr.is_empty() {
                    (
                        StepOutcome::Ok,
                        format!("foreach '{source}': 0 item(s) \u{2014} nothing to fan out"),
                    )
                } else {
                    let mut sub = Workflow::new(format!("{}_foreach_{}", self.id, index), "body");
                    sub.steps = body.clone();
                    let mut lines: Vec<String> = Vec::with_capacity(arr.len());
                    let mut any_failed = false;
                    for (i, item) in arr.iter().enumerate() {
                        let child_ctx = match ctx.clone() {
                            serde_json::Value::Object(mut m) => {
                                m.insert(as_var.clone(), item.clone());
                                serde_json::Value::Object(m)
                            }
                            _ => serde_json::json!({ as_var: item }),
                        };
                        let sub_run = sub.execute_with_context(workspace_root, &child_ctx);
                        // Fold the body's metering up: an inner AgentTask is
                        // real inference cost even though the outer step is a
                        // deterministic ForEach. Without this the ratio would
                        // overstate determinism for any SOP that fans out.
                        det.merge(&sub_run.determinism);
                        if sub_run.status != RunStatus::Success {
                            any_failed = true;
                        }
                        lines.push(format!(
                            "  [{}/{}] {:?} \u{2014} {}",
                            i + 1,
                            arr.len(),
                            sub_run.status,
                            item
                        ));
                        // A failing iteration halts the whole fan-out — no partial
                        // completion drift between sibling items.
                        if any_failed {
                            break;
                        }
                    }
                    let outcome = if any_failed {
                        StepOutcome::Failed
                    } else {
                        StepOutcome::Ok
                    };
                    (
                        outcome,
                        format!(
                            "foreach {} item(s) from '{source}':\n{}",
                            arr.len(),
                            lines.join("\n")
                        ),
                    )
                }
            } else {
                run_step(workspace_root, step, prior, prior_output.as_deref())
            };
            // Tally the executed step for determinism metering. Skipped steps
            // (short-circuit tail) never reach here, so a halted SOP does not
            // inflate its deterministic ratio with dead branches.
            match step {
                WorkflowStep::AgentTask { .. } => det.agent_steps += 1,
                WorkflowStep::Tool { .. } => det.tool_calls += 1,
                WorkflowStep::Connector { .. } => det.connector_calls += 1,
                WorkflowStep::Condition { .. } => det.condition_checks += 1,
                WorkflowStep::ForEach { .. } => det.foreach_steps += 1,
            }
            // A failed condition or a failed fan-out halts the rest of the workflow.
            if matches!(
                step,
                WorkflowStep::Condition { .. } | WorkflowStep::ForEach { .. }
            ) && outcome == StepOutcome::Failed
            {
                short_circuit = true;
            }
            // Only a successful content step advances `lastOutput`; conditions
            // carry no payload to forward.
            if !matches!(step, WorkflowStep::Condition { .. }) && outcome == StepOutcome::Ok {
                last_output = output.clone();
            }
            prior = Some(outcome);
            // The immediately-preceding step's output is what a verdict-condition
            // inspects next, whether it succeeded or failed.
            prior_output = Some(output.clone());
            records.push(StepRecord {
                index,
                kind: step.kind_label(),
                outcome,
                output,
            });
        }

        let has_failed = records.iter().any(|r| r.outcome == StepOutcome::Failed);
        let has_skipped = records.iter().any(|r| r.outcome == StepOutcome::Skipped);
        let status = if has_failed {
            RunStatus::Failed
        } else if has_skipped {
            RunStatus::Partial
        } else {
            RunStatus::Success
        };

        WorkflowRun {
            workflow_id: self.id.clone(),
            status,
            steps: records,
            started_at,
            finished_at: now_secs(),
            determinism: det,
        }
    }
}

/// Recursive: substitute `{{key}}` from `ctx` and `{{lastOutput}}` in string
/// leaves. When the ctx value is not a string (e.g. an object bound by a
/// `ForEach` iteration), it is JSON-stringified so `{{item.field}}`-style
/// tokens on plain strings still interpolate usefully. Unknown placeholders are
/// left as-is.
fn substitute_ctx(value: &mut serde_json::Value, ctx: &serde_json::Value, last_output: &str) {
    match value {
        serde_json::Value::String(s) => {
            if s.contains("{{lastOutput}}") {
                *s = s.replace("{{lastOutput}}", last_output);
            }
            if let Some(obj) = ctx.as_object() {
                for (k, v) in obj {
                    let needle = format!("{{{{{k}}}}}");
                    if s.contains(&needle) {
                        let repl = match v {
                            serde_json::Value::String(inner) => inner.clone(),
                            other => other.to_string(),
                        };
                        *s = s.replace(&needle, &repl);
                    }
                }
            }
            // Dotted-path interpolation: `{{invoice.id}}` resolves by walking
            // `ctx` along the segments. This is what makes a ForEach body
            // genuinely useful — items are typically objects with several
            // fields, and each field must be addressable without a flatten
            // step. Unknown paths are left untouched (the raw token survives).
            //
            // Collect tokens first, then replace each by needle string —
            // mutating `s` while cursor-walking would invalidate the offsets
            // the moment a substitution changed the length.
            let mut dotted: Vec<String> = Vec::new();
            {
                let mut scan = 0usize;
                while let Some(open) = s[scan..].find("{{") {
                    let abs_open = scan + open;
                    let after_open = abs_open + 2;
                    let Some(rel_close) = s[after_open..].find("}}") else {
                        break;
                    };
                    let token = &s[after_open..after_open + rel_close];
                    if token.contains('.') && !dotted.iter().any(|t| t == token) {
                        dotted.push(token.to_string());
                    }
                    scan = after_open + rel_close + 2;
                }
            }
            for token in dotted {
                let mut node: Option<&serde_json::Value> = Some(ctx);
                for seg in token.split('.') {
                    node = node.and_then(|n| n.get(seg));
                }
                if let Some(v) = node {
                    let repl = match v {
                        serde_json::Value::String(inner) => inner.clone(),
                        other => other.to_string(),
                    };
                    let needle = format!("{{{{{token}}}}}");
                    *s = s.replace(&needle, &repl);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for e in arr.iter_mut() {
                substitute_ctx(e, ctx, last_output);
            }
        }
        serde_json::Value::Object(map) => {
            for e in map.values_mut() {
                substitute_ctx(e, ctx, last_output);
            }
        }
        _ => {}
    }
}

/// Run a single step and return its outcome plus a captured output string.
fn run_step(
    workspace_root: &Path,
    step: &WorkflowStep,
    prior: Option<StepOutcome>,
    prior_output: Option<&str>,
) -> (StepOutcome, String) {
    match step {
        WorkflowStep::Tool { name, args } => {
            match crate::registry::dispatch::call_tool_in_workspace(workspace_root, name, args) {
                Ok(out) => (StepOutcome::Ok, out),
                Err(e) => (StepOutcome::Failed, e.to_string()),
            }
        }
        WorkflowStep::AgentTask { prompt, team } => {
            let (provider, model) = agent_route(workspace_root, team.as_deref());
            let request = crate::agent::HeadlessSubAgentRequest {
                workspace_root: workspace_root.to_path_buf(),
                provider,
                model: model.clone(),
                thinking: false,
                prompt: prompt.clone(),
                cancel_rx: None,
                progress: None,
                scoped_files: None,
                max_turns: None,
                // A workflow runs unattended and this step has no cancel
                // channel, so without a budget a stalled provider call would
                // hang the whole workflow forever — which is why the loop's
                // stall window stays on even when the operator removes the
                // wall-clock backstop.
                budget: crate::agent::unattended_budget(),
                requires_artifact: false,
                // A workflow step edits, runs and records; the far families stay
                // one `request_tool_groups` call away.
                tool_surface: crate::registry::ToolSurface::code_and_workspace(),
                ramble: crate::agent::RambleLedger::default(),
            };
            let result = crate::agent::run_headless_subagent(request);
            // Success is the loop's structured outcome, not a substring hunt
            // through its prose: a completed agent step quotes cargo output and
            // tool errors routinely, so scanning for `Error:` failed steps that
            // had already done the work (the same misreading that discarded a
            // finished LiteLLM port in team_dispatch).
            let failed = !result.outcome.is_completed();
            let outcome = if failed {
                StepOutcome::Failed
            } else {
                StepOutcome::Ok
            };
            let blocked = if failed {
                format!(" | blocked: {}", result.outcome.reason())
            } else {
                String::new()
            };
            let excerpt = transcript_snippet(&result.transcript, 600);
            (
                outcome,
                format!(
                    "{} → {} | {} status update(s){blocked} | {excerpt}",
                    provider.slug(),
                    model,
                    result.status_updates.len()
                ),
            )
        }
        WorkflowStep::Connector { id, req: _ } => (
            StepOutcome::Skipped,
            format!("connector '{id}' execution wired in Pillar 2"),
        ),
        WorkflowStep::ForEach { source, body, .. } => (
            StepOutcome::Failed,
            format!(
                "foreach[{source}]×{} must be dispatched via Workflow::execute_with_context",
                body.len()
            ),
        ),
        WorkflowStep::Condition {
            require,
            on_text,
            recheck,
        } => match prior {
            Some(o) if o == *require => match on_text {
                Some(p) if !p.contains_all.is_empty() || !p.contains_none.is_empty() => {
                    let hay = prior_output.unwrap_or("");
                    if p.holds(hay) {
                        (
                            StepOutcome::Ok,
                            format!(
                                "condition met: prior == {} & verdict clean",
                                require.label()
                            ),
                        )
                    } else {
                        // The verdict is missing. A gate may re-observe a bounded
                        // number of times before halting — and only ever with an
                        // observation-only action, so a missing verdict can never
                        // be "retried" by performing the action again.
                        match recheck {
                            None => (
                                StepOutcome::Failed,
                                format!(
                                    "condition failed: verdict predicate not satisfied (prior {})",
                                    require.label()
                                ),
                            ),
                            Some(r) if !recheck_is_observation_only(&r.tool, &r.args) => (
                                StepOutcome::Failed,
                                format!(
                                    "condition failed: verdict predicate not satisfied and recheck refused ('{}' is an outward action)",
                                    r.tool
                                ),
                            ),
                            Some(r) => {
                                let tries = r.attempts.max(1);
                                let mut note = String::new();
                                let mut clean = false;
                                for attempt in 1..=tries {
                                    if r.settle_ms > 0 {
                                        std::thread::sleep(std::time::Duration::from_millis(
                                            r.settle_ms,
                                        ));
                                    }
                                    let observed = match crate::registry::dispatch::call_tool_in_workspace(
                                        workspace_root, &r.tool, &r.args,
                                    ) {
                                        Ok(out) => out,
                                        Err(e) => e.to_string(),
                                    };
                                    if p.holds(&observed) {
                                        clean = true;
                                        note = format!(
                                            ", re-observed clean on recheck {attempt}/{tries} [{}]",
                                            r.tool
                                        );
                                        break;
                                    }
                                    note = format!(
                                        ", recheck {attempt}/{tries} [{}] still not satisfied",
                                        r.tool
                                    );
                                }
                                if clean {
                                    (
                                        StepOutcome::Ok,
                                        format!(
                                            "condition met: prior == {} & verdict clean{}",
                                            require.label(),
                                            note
                                        ),
                                    )
                                } else {
                                    (
                                        StepOutcome::Failed,
                                        format!(
                                            "condition failed: verdict predicate not satisfied (prior {}){}",
                                            require.label(),
                                            note
                                        ),
                                    )
                                }
                            }
                        }
                    }
                }
                _ => (
                    StepOutcome::Ok,
                    format!("condition met: prior == {}", require.label()),
                ),
            },
            Some(o) => (
                StepOutcome::Failed,
                format!(
                    "condition failed: prior {} != {}",
                    o.label(),
                    require.label()
                ),
            ),
            None => (
                StepOutcome::Failed,
                "condition failed: no prior step".to_string(),
            ),
        },
    }
}

/// Resolve the provider + model for an `AgentTask` step so workflow agents
/// honour workspace configuration instead of a hardcoded provider:
/// 1. `team` given and found → first member's fallback chain head (same
///    routing `team_dispatch` uses, including pinned model ids).
/// 2. Otherwise → the provider + model the user selected in the IDE
///    (`.velocity/workspace-preferences.json`).
/// 3. Otherwise → first *configured* provider in the workspace with its
///    default model.
/// 4. Last resort → Cloudflare Workers AI default (legacy behaviour).
fn agent_route(root: &Path, team: Option<&str>) -> (crate::agent::AiProvider, String) {
    use crate::agent::provider::default_provider_model;
    use crate::agent::AiProvider;
    if let Some(name) = team {
        let lower = name.to_lowercase();
        let found = crate::editor::expert_team::load_expert_teams(root)
            .into_iter()
            .find(|t| {
                t.name.to_lowercase() == lower
                    || t.id.to_lowercase() == lower
                    || t.slug().to_lowercase() == lower
            });
        if let Some(t) = found {
            if let Some(member) = t.members.first() {
                if let Some(route) = crate::registry::team_tools::build_fallback_chain(member, root)
                    .into_iter()
                    .next()
                {
                    return route;
                }
                return (member.provider, member.model_id.clone());
            }
        }
    }
    if let Some((provider, model)) = workspace_selected_route(root) {
        return (provider, model);
    }
    if let Some((provider, _)) = crate::registry::team_tools::configured_providers(root)
        .into_iter()
        .find(|(_, ok)| *ok)
    {
        return (provider, default_provider_model(provider));
    }
    (
        AiProvider::CloudflareWorkersAi,
        default_provider_model(AiProvider::CloudflareWorkersAi),
    )
}

/// The provider + model the user picked in the IDE, as persisted by
/// `save_workspace_preferences`. Empty/invalid values fall through.
fn workspace_selected_route(root: &Path) -> Option<(crate::agent::AiProvider, String)> {
    let raw =
        std::fs::read_to_string(root.join(".velocity").join("workspace-preferences.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    let provider = crate::agent::AiProvider::from_label(json["provider"].as_str()?)?;
    let model = json["selected_model"].as_str()?.trim();
    if model.is_empty() {
        return None;
    }
    Some((provider, model.to_string()))
}

/// First `max_chars` of the transcript, flattened to one line for step logs.
fn transcript_snippet(transcript: &str, max_chars: usize) -> String {
    let flat = transcript.trim().replace('\n', " ⏎ ");
    if flat.chars().count() <= max_chars {
        flat
    } else {
        let head: String = flat.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

/// In-memory set of workflows, backed by `.velocity/workflows/<id>.json`.
#[derive(Debug, Clone, Default)]
pub struct WorkflowRegistry {
    pub workflows: Vec<Workflow>,
}

impl WorkflowRegistry {
    /// Load every `*.json` workflow file from `.velocity/workflows/`. Missing
    /// directory or unreadable/corrupt files are skipped.
    pub fn load(workspace_root: &Path) -> Self {
        let dir = workspace_root.join(".velocity").join(WORKFLOWS_DIR);
        let mut workflows = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Ok(wf) = serde_json::from_slice::<Workflow>(&bytes) {
                        workflows.push(wf);
                    }
                }
            }
        }
        workflows.sort_by(|a, b| a.name.cmp(&b.name));
        Self { workflows }
    }

    /// Persist all current workflows to `.velocity/workflows/`, pruning any
    /// orphaned `*.json` files whose id is no longer present.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity").join(WORKFLOWS_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create workflows dir: {e}"))?;

        for wf in &self.workflows {
            let json = serde_json::to_vec_pretty(wf)
                .map_err(|e| format!("workflow serialize failed: {e}"))?;
            std::fs::write(dir.join(format!("{}.json", wf.id)), json)
                .map_err(|e| format!("cannot write workflow: {e}"))?;
        }

        // Prune files for removed workflows.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if !self.workflows.iter().any(|w| w.id == stem) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }

    pub fn add(&mut self, workflow: Workflow) {
        self.workflows.push(workflow);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.workflows.len();
        self.workflows.retain(|w| w.id != id);
        self.workflows.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&Workflow> {
        self.workflows.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Workflow> {
        self.workflows.iter_mut().find(|w| w.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.workflows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.workflows.len()
    }
}

/// Current wall-clock time as Unix epoch seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_step(rel: &str) -> WorkflowStep {
        WorkflowStep::Tool {
            name: "write_file".to_string(),
            args: json!({ "relativeFilePath": rel, "content": "hello" }),
        }
    }

    #[test]
    fn context_substitution_replaces_named_and_last_output() {
        let mut v = json!({
            "request_id": "{{requestId}}",
            "body": "{{lastOutput}}",
            "nested": ["{{requestId}}"],
            "count": 5,
            "unknown": "{{nope}}"
        });
        substitute_ctx(&mut v, &json!({ "requestId": "req_42" }), "drafted copy");
        assert_eq!(v["request_id"], "req_42");
        assert_eq!(v["body"], "drafted copy");
        assert_eq!(v["nested"][0], "req_42");
        assert_eq!(v["count"], 5); // non-string leaf untouched
        assert_eq!(v["unknown"], "{{nope}}"); // unknown placeholder preserved
    }

    #[test]
    fn two_step_workflow_executes_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf1", "two steps");
        wf.steps.push(write_step("a.txt"));
        wf.steps.push(write_step("b.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.status, RunStatus::Success);
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0].index, 0);
        assert_eq!(run.steps[1].index, 1);
        assert_eq!(run.steps[0].outcome, StepOutcome::Ok);
        assert_eq!(run.ok_count(), 2);
        assert!(tmp.path().join("a.txt").exists());
        assert!(tmp.path().join("b.txt").exists());
    }

    #[test]
    fn condition_short_circuits_remaining_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf2", "gated");
        wf.steps.push(write_step("first.txt")); // Ok
        wf.steps.push(WorkflowStep::Condition {
            require: StepOutcome::Failed,
            on_text: None,
            recheck: None,
        }); // prior Ok != Failed -> fails, short-circuits
        wf.steps.push(write_step("never.txt")); // Skipped

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[0].outcome, StepOutcome::Ok);
        assert_eq!(run.steps[1].outcome, StepOutcome::Failed);
        assert_eq!(run.steps[2].outcome, StepOutcome::Skipped);
        assert_eq!(run.status, RunStatus::Failed);
        assert!(!tmp.path().join("never.txt").exists());
    }

    #[test]
    fn condition_met_allows_continuation() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf3", "passes gate");
        wf.steps.push(write_step("one.txt")); // Ok
        wf.steps.push(WorkflowStep::Condition {
            require: StepOutcome::Ok,
            on_text: None,
            recheck: None,
        }); // prior Ok == Ok -> passes
        wf.steps.push(write_step("two.txt")); // runs

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[1].outcome, StepOutcome::Ok);
        assert_eq!(run.steps[2].outcome, StepOutcome::Ok);
        assert_eq!(run.status, RunStatus::Success);
        assert!(tmp.path().join("two.txt").exists());
    }

    #[test]
    fn failed_tool_step_is_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf4", "bad tool");
        wf.steps.push(WorkflowStep::Tool {
            name: "does_not_exist".to_string(),
            args: json!({}),
        });
        let run = wf.execute(tmp.path());
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].outcome, StepOutcome::Failed);
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.ok_count(), 0);
    }

    #[test]
    fn foreach_body_runs_once_per_item_with_item_bound() {
        // Three invoices in the source array -> three files, each named by the
        // item's `id` field. This is the deterministic fan-out contract: a single
        // approval gates N downstream actions with no inference in the loop.
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf_fe", "pay each");
        wf.steps.push(WorkflowStep::ForEach {
            source: "invoices".to_string(),
            as_var: "invoice".to_string(),
            body: vec![WorkflowStep::Tool {
                name: "write_file".to_string(),
                args: json!({
                    "relativeFilePath": "inv_{{invoice.id}}.txt",
                    "content": "amount={{invoice.amount}}"
                }),
            }],
        });
        let run = wf.execute_with_context(
            tmp.path(),
            &json!({
                "invoices": [
                    {"id": "a1", "amount": 100},
                    {"id": "b2", "amount": 250},
                    {"id": "c3", "amount": 40}
                ]
            }),
        );
        assert_eq!(run.status, RunStatus::Success, "{:#?}", run);
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].outcome, StepOutcome::Ok);
        assert!(tmp.path().join("inv_a1.txt").exists());
        assert!(tmp.path().join("inv_b2.txt").exists());
        assert!(tmp.path().join("inv_c3.txt").exists());
        // Body ran with the item bound: the file contents must reflect the amount.
        let body = std::fs::read_to_string(tmp.path().join("inv_b2.txt")).unwrap();
        assert!(body.contains("amount=250"), "{body}");
    }

    #[test]
    fn foreach_halt_on_body_failure_short_circuits_parent() {
        // Iteration 2 uses a nonexistent tool -> sub-run fails -> the whole
        // ForEach fails -> any following parent step is Skipped. This proves
        // there is no silent partial-completion drift between siblings.
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf_fe_h", "halt mid");
        wf.steps.push(WorkflowStep::ForEach {
            source: "items".to_string(),
            as_var: "item".to_string(),
            body: vec![WorkflowStep::Tool {
                name: "does_not_exist".to_string(),
                args: json!({ "x": "{{item}}" }),
            }],
        });
        wf.steps.push(WorkflowStep::Tool {
            name: "write_file".to_string(),
            args: json!({ "relativeFilePath": "never.txt", "content": "x" }),
        });
        let run = wf.execute_with_context(tmp.path(), &json!({ "items": [1, 2, 3] }));
        assert_eq!(run.steps[0].outcome, StepOutcome::Failed);
        assert_eq!(run.steps[1].outcome, StepOutcome::Skipped);
        assert_eq!(run.status, RunStatus::Failed);
        assert!(!tmp.path().join("never.txt").exists());
    }

    #[test]
    fn substitute_ctx_stringifies_non_string_values() {
        // A ForEach body sees `{{item}}` where the ctx value is an object. The
        // interpolator must stringify so the substitution is observable on plain
        // string leaves (not silently preserved as an unresolved token).
        let mut v = json!({ "line": "row {{item}}" });
        substitute_ctx(&mut v, &json!({ "item": { "id": "x" } }), "");
        let s = v["line"].as_str().unwrap();
        assert!(s.contains("\"id\":\"x\""), "{s}");
        assert!(!s.contains("{{item}}"), "{s}");
    }

    #[test]
    fn text_predicate_holds_by_plain_containment_only() {
        let p = TextPredicate {
            contains_all: vec!["VERIFY: PASS".into(), "ABSENT: PASS".into()],
            contains_none: vec!["VERIFY: FAIL".into()],
        };
        assert!(p.holds("run ok\nVERIFY: PASS ('x' present).\nABSENT: PASS\n"));
        // A required marker missing -> refuse.
        assert!(!p.holds("VERIFY: PASS\n"));
        // A forbidden marker present -> refuse, even alongside the good ones.
        assert!(!p.holds("VERIFY: PASS\nABSENT: PASS\nVERIFY: FAIL"));
        // Empty predicate trivially holds (back-compat with plain conditions).
        assert!(TextPredicate::default().holds(""));
    }

    fn read_step(rel: &str) -> WorkflowStep {
        WorkflowStep::Tool {
            name: "read_file".to_string(),
            args: json!({ "relativeFilePath": rel }),
        }
    }

    fn verdict_gate() -> WorkflowStep {
        WorkflowStep::Condition {
            require: StepOutcome::Ok,
            on_text: Some(TextPredicate {
                contains_all: vec!["VERIFY: PASS".into()],
                contains_none: vec!["VERIFY: FAIL".into(), "ABSENT: FAIL".into()],
            }),
            recheck: None,
        }
    }

    /// The same gate, but allowed to re-read a file before declaring failure —
    /// standing in for re-reading a page that settles late.
    fn verdict_gate_with_recheck(rel: &str, attempts: u32) -> WorkflowStep {
        WorkflowStep::Condition {
            require: StepOutcome::Ok,
            on_text: Some(TextPredicate {
                contains_all: vec!["VERIFY: PASS".into()],
                contains_none: vec!["VERIFY: FAIL".into()],
            }),
            recheck: Some(Recheck {
                tool: "read_file".to_string(),
                args: json!({ "relativeFilePath": rel }),
                attempts,
                settle_ms: 0,
            }),
        }
    }

    #[test]
    fn recheck_rescues_a_gate_when_the_observation_settles_late() {
        let tmp = tempfile::tempdir().unwrap();
        // The first read reports the post as not landed; the recheck's fresh read
        // sees it land. Nothing is re-published — only re-observed.
        std::fs::write(
            tmp.path().join("late.txt"),
            "EXTRACTED: still rendering\nVERIFY: FAIL (expected 'marker', not found.).\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("settled.txt"),
            "EXTRACTED: post visible now\nVERIFY: PASS ('marker' present).\n",
        )
        .unwrap();
        let mut wf = Workflow::new("rc1", "recheck rescues");
        wf.steps.push(read_step("late.txt"));
        wf.steps.push(verdict_gate_with_recheck("settled.txt", 2));
        wf.steps.push(write_step("dispatched.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[1].outcome, StepOutcome::Ok, "{:?}", run.steps[1]);
        assert!(
            run.steps[1]
                .output
                .contains("re-observed clean on recheck 1/2"),
            "{}",
            run.steps[1].output
        );
        assert_eq!(run.status, RunStatus::Success);
        assert!(tmp.path().join("dispatched.txt").exists());
    }

    #[test]
    fn exhausted_rechecks_still_halt_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("never.txt"),
            "EXTRACTED: nothing landed\nVERIFY: FAIL\n",
        )
        .unwrap();
        let mut wf = Workflow::new("rc2", "recheck exhausted");
        std::fs::write(
            tmp.path().join("late.txt"),
            "EXTRACTED: not yet\nVERIFY: FAIL\n",
        )
        .unwrap();
        wf.steps.push(read_step("late.txt"));
        wf.steps.push(verdict_gate_with_recheck("never.txt", 3));
        wf.steps.push(write_step("must_not_run.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[1].outcome, StepOutcome::Failed);
        assert!(
            run.steps[1]
                .output
                .contains("recheck 3/3 [read_file] still not satisfied"),
            "{}",
            run.steps[1].output
        );
        assert_eq!(run.steps[2].outcome, StepOutcome::Skipped);
        assert!(!tmp.path().join("must_not_run.txt").exists());
    }

    #[test]
    fn a_recheck_may_never_replay_the_outward_action() {
        // The safety property: re-verification observes, it does not act. A gate
        // configured to "recheck" by requesting the action again is refused, so a
        // missing verdict can never become a duplicate post, deploy or payment.
        assert!(!recheck_is_observation_only(
            "request_org_action",
            &json!({ "description": "publish" })
        ));
        assert!(!recheck_is_observation_only(
            "execute_org_browser_task",
            &json!({ "read_script": "return 1", "form_submit_selector": "button" })
        ));
        assert!(!recheck_is_observation_only(
            "execute_org_browser_task",
            &json!({ "click_selector": "a.publish" })
        ));
        // A pure read of the same page is fine, and so is reading a file.
        assert!(recheck_is_observation_only(
            "execute_org_browser_task_for_request",
            &json!({ "request_id": "req_1", "read_script": "return document.body.innerText", "verify_expect": "marker" })
        ));
        assert!(recheck_is_observation_only(
            "read_file",
            &json!({ "relativeFilePath": "x.txt" })
        ));
    }

    #[test]
    fn a_refused_recheck_halts_without_calling_anything() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("verdict.txt"), "VERIFY: FAIL\n").unwrap();
        let mut wf = Workflow::new("rc3", "refused recheck");
        wf.steps.push(read_step("verdict.txt"));
        wf.steps.push(WorkflowStep::Condition {
            require: StepOutcome::Ok,
            on_text: Some(TextPredicate {
                contains_all: vec!["VERIFY: PASS".into()],
                contains_none: vec![],
            }),
            recheck: Some(Recheck {
                tool: "request_org_action".to_string(),
                args: json!({ "role_id": "social_operator", "resource_id": "social_x" }),
                attempts: 2,
                settle_ms: 0,
            }),
        });
        wf.steps.push(write_step("must_not_run.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[1].outcome, StepOutcome::Failed);
        assert!(
            run.steps[1]
                .output
                .contains("recheck refused ('request_org_action' is an outward action)"),
            "{}",
            run.steps[1].output
        );
        assert!(!tmp.path().join("must_not_run.txt").exists());
    }

    #[test]
    fn verdict_condition_halts_dispatch_on_failed_verdict() {
        let tmp = tempfile::tempdir().unwrap();
        // A read whose output carries the browser's FAIL verdict: the step
        // itself *returns* fine (Ok), so only a verdict-aware gate can stop a
        // downstream outward action from acting on a page that didn't verify.
        std::fs::write(
            tmp.path().join("verdict.txt"),
            "EXTRACTED: page text\nVERIFY: FAIL (expected 'marker', not found).\n",
        )
        .unwrap();
        let mut wf = Workflow::new("wfv1", "halt on fail");
        wf.steps.push(read_step("verdict.txt"));
        wf.steps.push(verdict_gate());
        wf.steps.push(write_step("must_not_run.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[0].outcome, StepOutcome::Ok);
        assert_eq!(run.steps[1].outcome, StepOutcome::Failed);
        assert_eq!(run.steps[2].outcome, StepOutcome::Skipped);
        assert_eq!(run.status, RunStatus::Failed);
        assert!(!tmp.path().join("must_not_run.txt").exists());
        assert!(run.steps[1]
            .output
            .contains("verdict predicate not satisfied"));
    }

    #[test]
    fn verdict_condition_continues_on_clean_verdict() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("verdict.txt"),
            "EXTRACTED: page text\nVERIFY: PASS ('marker' present).\n",
        )
        .unwrap();
        let mut wf = Workflow::new("wfv2", "continue on pass");
        wf.steps.push(read_step("verdict.txt"));
        wf.steps.push(verdict_gate());
        wf.steps.push(write_step("dispatched.txt"));

        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[1].outcome, StepOutcome::Ok);
        assert_eq!(run.steps[2].outcome, StepOutcome::Ok);
        assert_eq!(run.status, RunStatus::Success);
        assert!(tmp.path().join("dispatched.txt").exists());
    }

    #[test]
    fn connector_step_is_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wf = Workflow::new("wf5", "connector");
        wf.steps.push(WorkflowStep::Connector {
            id: "gh".to_string(),
            req: json!({}),
        });
        let run = wf.execute(tmp.path());
        assert_eq!(run.steps[0].outcome, StepOutcome::Skipped);
        assert_eq!(run.status, RunStatus::Partial);
    }

    #[test]
    fn registry_round_trip_and_prune() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reg = WorkflowRegistry::default();
        let mut wf = Workflow::new("keep", "Keep Me");
        wf.steps.push(write_step("x.txt"));
        reg.add(wf);
        reg.add(Workflow::new("drop", "Drop Me"));
        reg.save(tmp.path()).expect("save");

        let loaded = WorkflowRegistry::load(tmp.path());
        assert_eq!(loaded.len(), 2);
        // Sorted by name: "Drop Me" < "Keep Me".
        assert_eq!(loaded.workflows[0].id, "drop");
        assert_eq!(loaded.get("keep").unwrap().steps.len(), 1);

        // Remove one and re-save: its file should be pruned.
        let mut reg2 = loaded;
        assert!(reg2.remove("drop"));
        reg2.save(tmp.path()).expect("save2");
        let reloaded = WorkflowRegistry::load(tmp.path());
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded.get("drop").is_none());
        assert!(reloaded.get("keep").is_some());
    }

    #[test]
    fn agent_task_uses_workspace_routing_not_hardcoded_provider() {
        // Bug #8 regression: an unknown (or absent) team must resolve via the
        // workspace's configured providers, and both paths must agree.
        let tmp = tempfile::tempdir().unwrap();
        let none = agent_route(tmp.path(), None);
        let unknown = agent_route(tmp.path(), Some("no-such-team"));
        assert_eq!(none.0, unknown.0);
        assert_eq!(none.1, unknown.1);
        assert!(!none.1.is_empty());
        // The chosen provider must actually be configured in this workspace
        // (or be the explicit last-resort Cloudflare).
        let configured = crate::registry::team_tools::configured_providers(tmp.path());
        let is_cloudflare_last_resort = none.0 == crate::agent::AiProvider::CloudflareWorkersAi
            && !configured.iter().any(|(_, ok)| *ok);
        assert!(configured.iter().any(|(p, ok)| *ok && *p == none.0) || is_cloudflare_last_resort);
    }

    #[test]
    fn agent_task_prefers_ide_selected_provider_and_model() {
        // Bug #8/#9 regression: with no team, the route must be exactly what
        // the user selected in the IDE — even when the preferences file has
        // a Windows-style UTF-8 BOM.
        let tmp = tempfile::tempdir().unwrap();
        let vel = tmp.path().join(".velocity");
        std::fs::create_dir_all(&vel).unwrap();
        std::fs::write(
            vel.join("workspace-preferences.json"),
            "\u{feff}{\"provider\":\"Alibaba Qwen\",\"selected_model\":\"qwen3.8-flash\"}",
        )
        .unwrap();
        let (provider, model) = agent_route(tmp.path(), None);
        assert_eq!(provider, crate::agent::AiProvider::AlibabaQwen);
        assert_eq!(model, "qwen3.8-flash");
    }

    #[test]
    fn transcript_snippet_flattens_and_caps() {
        assert_eq!(transcript_snippet("a\nb", 10), "a \u{23ce} b");
        assert_eq!(transcript_snippet("  padded  ", 10), "padded");
        let long = "x".repeat(50);
        let s = transcript_snippet(&long, 10);
        assert_eq!(s.chars().count(), 11); // 10 chars + ellipsis
        assert!(s.ends_with('\u{2026}'));
    }

    /// Phase 74: DeterminismReport unit test — verifies the struct's
    /// `deterministic_ratio()`, `merge()`, and `summary_line()` contracts.
    #[test]
    fn determinism_report_ratio_merge_and_summary() {
        // Empty: 0 steps -> 0.0 (can't masquerade as fully deterministic).
        let empty = DeterminismReport::default();
        assert_eq!(empty.total(), 0);
        assert_eq!(empty.deterministic_ratio(), 0.0);

        // Pure tool: all deterministic -> 1.0.
        let pure = DeterminismReport {
            tool_calls: 4,
            condition_checks: 2,
            ..Default::default()
        };
        assert_eq!(pure.total(), 6);
        assert_eq!(pure.deterministic_ratio(), 1.0);

        // Pure agent: all inference -> 0.0.
        let agent_only = DeterminismReport {
            agent_steps: 3,
            ..Default::default()
        };
        assert_eq!(agent_only.deterministic_ratio(), 0.0);

        // Mixed: 1 agent + 3 tools + 1 condition -> 4/5 = 0.8.
        let mixed = DeterminismReport {
            agent_steps: 1,
            tool_calls: 3,
            condition_checks: 1,
            ..Default::default()
        };
        assert_eq!(mixed.total(), 5);
        assert!(
            (mixed.deterministic_ratio() - 0.8).abs() < f64::EPSILON,
            "got {}",
            mixed.deterministic_ratio()
        );

        // merge: ForEach sub-runs fold into parent.
        let mut parent = DeterminismReport {
            foreach_steps: 1,
            tool_calls: 2,
            ..Default::default()
        };
        let child = DeterminismReport {
            agent_steps: 1,
            tool_calls: 3,
            ..Default::default()
        };
        parent.merge(&child);
        assert_eq!(parent.agent_steps, 1);
        assert_eq!(parent.tool_calls, 5);
        assert_eq!(parent.foreach_steps, 1);
        assert_eq!(parent.total(), 7);
        // 6 deterministic / 7 total
        let ratio = parent.deterministic_ratio();
        assert!((ratio - 6.0 / 7.0).abs() < 1e-10, "merged ratio: {ratio}");

        // summary_line: format contract.
        let s = parent.summary_line();
        assert!(s.starts_with("deterministic "), "{s}");
        assert!(s.contains("agent=1"), "{s}");
        assert!(s.contains("tool=5"), "{s}");
        assert!(s.contains("foreach=1"), "{s}");
    }
}
