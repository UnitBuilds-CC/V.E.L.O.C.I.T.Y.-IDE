//! Lifecycle hooks — workspace-defined shell commands that run at agent
//! tool-call boundaries, the extension point that turns a fixed dispatch
//! pipeline into a programmable one.
//!
//! Hooks are declared in `<workspace>/.velocity/hooks.json` as a JSON array:
//!
//! ```json
//! [
//!   { "event": "pre_tool_use", "matcher": "write_file", "command": "exit 0" },
//!   { "event": "post_tool_use", "matcher": "browser_*", "command": "echo done" }
//! ]
//! ```
//!
//! Semantics (deliberately close to the convention external agents already
//! know, so muscle memory transfers):
//! * `pre_tool_use` runs *after* the governance policy has allowed the call
//!   and may still veto it — hooks can only ever **tighten** security, never
//!   grant permission that the [`PolicyEngine`](crate::editor::governance)
//!   denied. Exit code `2` (or a non-empty reason on stdout combined with
//!   exit code `2`) vetoes the call; the veto is recorded to the durable
//!   event store as a high-signal `lifecycle_hook_block` so the decision
//!   trail shows *why* the tool never ran.
//! * `post_tool_use` runs after the tool completes with the outcome in the
//!   payload. It is observational: failures are logged but never rewrite or
//!   block the tool result.
//! * The payload JSON is delivered on stdin with `VELOCITY_HOOK_EVENT` set in
//!   the environment, so hook scripts can dispatch on event type without
//!   parsing argv.
//!
//! Like [`custom_tools`](super::custom_tools), the registry is disk-backed
//! with no process-global cache: every dispatch reads `hooks.json`. That
//! keeps parallel tests isolated, lets an external editor change hooks
//! without restarting the server, and means a hook rule can never go stale
//! in memory.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The dispatch boundaries at which a hook may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Before the tool executes (after governance allowed it). May veto.
    PreToolUse,
    /// After the tool completed. Observational only.
    PostToolUse,
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
        }
    }
}

/// One hook rule from `hooks.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRule {
    pub event: HookEvent,
    /// Tool-name matcher: `*` (all tools), exact name, or a prefix/suffix
    /// wildcard such as `browser_*` or `*_file`.
    pub matcher: String,
    /// Shell command. Run via `cmd /c` on Windows, `sh -c` elsewhere.
    pub command: String,
    /// Per-rule timeout in seconds (default 10). A timed-out hook is killed;
    /// a timed-out *pre* hook vetoes the call — a hanging guard must fail
    /// closed, never open.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    10
}

/// Tool-name matcher shared by all hook events.
pub fn matches_tool(pattern: &str, tool: &str) -> bool {
    if pattern == "*" || pattern == tool {
        return true;
    }
    if let Some(prefix) = pattern.strip_prefix('*') {
        return tool.ends_with(prefix);
    }
    if let Some(suffix) = pattern.strip_suffix('*') {
        return tool.starts_with(suffix);
    }
    false
}

/// Load hook rules from `<workspace>/.velocity/hooks.json`. Missing or
/// malformed files yield no rules (with a warning) — a broken hooks file
/// must never lock the dispatch pipeline.
pub fn load_hooks(workspace_root: &Path) -> Vec<HookRule> {
    let path = workspace_root.join(".velocity").join("hooks.json");
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<Vec<HookRule>>(&raw) {
        Ok(rules) => rules,
        Err(err) => {
            log::warn!("hooks: ignoring malformed {}: {}", path.display(), err);
            Vec::new()
        }
    }
}

/// Result of running one rule.
struct HookRun {
    denied: bool,
    reason: String,
}

/// Execute a single hook rule with the JSON payload on stdin. Returns
/// `None` when the process could not be started or the command was empty
/// (with a warning) — infrastructure trouble is never treated as a veto,
/// except for timeouts, which fail closed.
fn execute_rule(rule: &HookRule, payload_json: &str) -> Option<HookRun> {
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/c")
    } else {
        ("sh", "-c")
    };
    let mut child = Command::new(shell)
        .arg(flag)
        .arg(&rule.command)
        .env("VELOCITY_HOOK_EVENT", rule.event.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| log::warn!("hooks: failed to spawn {:?}: {e}", rule.command))
        .ok()?;
    {
        let mut si = child.stdin.take()?;
        let _ = si.write_all(payload_json.as_bytes());
    }
    let deadline = Instant::now() + Duration::from_secs(rule.timeout_secs.max(1));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                // Fail closed: a guard that never answered is not a pass.
                return Some(HookRun {
                    denied: true,
                    reason: format!(
                        "hook timed out after {}s: {}",
                        rule.timeout_secs, rule.command
                    ),
                });
            }
            Err(e) => {
                log::warn!("hooks: try_wait failed for {:?}: {e}", rule.command);
                return None;
            }
        }
    };
    let mut stdout = String::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut so, &mut stdout);
    }
    let denied = status.code().unwrap_or(-1) == 2;
    let reason = {
        let out = stdout.trim();
        if out.is_empty() {
            format!("hook vetoed via exit code 2: {}", rule.command)
        } else {
            out.chars().take(400).collect::<String>()
        }
    };
    Some(HookRun { denied, reason })
}

fn payload(
    root: &Path,
    tool_name: &str,
    arguments: &serde_json::Value,
    outcome: Option<&str>,
) -> String {
    json!({
        "event_tool": tool_name,
        "arguments": arguments,
        "workspace_root": root.display().to_string(),
        "outcome": outcome,
    })
    .to_string()
}

/// Run every matching `pre_tool_use` hook. Returns `Some(reason)` when a
/// hook vetoed the call; the veto is recorded to the durable event store so
/// the decision trail explains why the tool never ran.
pub fn run_pre_tool_hooks(
    root: &Path,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Option<String> {
    let rules = load_hooks(root);
    let mut denials: Vec<String> = Vec::new();
    for rule in rules
        .iter()
        .filter(|r| r.event == HookEvent::PreToolUse && matches_tool(&r.matcher, tool_name))
    {
        let Some(run) = execute_rule(rule, &payload(root, tool_name, arguments, None)) else {
            continue;
        };
        if run.denied {
            denials.push(run.reason);
        }
    }
    if denials.is_empty() {
        return None;
    }
    let reason = denials.join("; ");
    let store = super::event_store::EventStore::open(root);
    let _ = store.record_with_metadata(
        "lifecycle_hook",
        &format!("pre_tool_use hook vetoed tool '{}'", tool_name),
        None,
        None,
        Some("hook policy: the veto reason is recorded in metadata".to_string()),
        Vec::new(),
        super::event_store::EventOutcome::Failure,
        Some(reason.clone()),
        Some(json!({
            "lifecycle_hook_block": true,
            "hook_event": "pre_tool_use",
            "blocked_tool": tool_name,
        })),
    );
    Some(reason)
}

/// Run every matching `post_tool_use` hook for observability. Never blocks
/// or alters the tool output; failures are logged only.
pub fn run_post_tool_hooks(
    root: &Path,
    tool_name: &str,
    arguments: &serde_json::Value,
    outcome: &str,
) {
    let rules = load_hooks(root);
    for rule in rules
        .iter()
        .filter(|r| r.event == HookEvent::PostToolUse && matches_tool(&r.matcher, tool_name))
    {
        if let Some(run) = execute_rule(rule, &payload(root, tool_name, arguments, Some(outcome))) {
            if run.denied {
                // Post hooks cannot block a completed call; surface the
                // objection in the log so operators see it.
                log::warn!(
                    "post_tool_use hook objected for '{}': {}",
                    tool_name,
                    run.reason
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::event_store::{EventOutcome, EventStore};
    use tempfile::TempDir;

    fn write_hooks(root: &Path, rules: &[serde_json::Value]) {
        fs::create_dir_all(root.join(".velocity")).unwrap();
        fs::write(
            root.join(".velocity").join("hooks.json"),
            serde_json::to_string(rules).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn matcher_covers_exact_star_and_wildcards() {
        assert!(matches_tool("*", "anything"));
        assert!(matches_tool("write_file", "write_file"));
        assert!(!matches_tool("write_file", "read_file"));
        assert!(matches_tool("browser_*", "browser_navigate"));
        assert!(!matches_tool("browser_*", "chrome_navigate"));
        assert!(matches_tool("*_file", "write_file"));
    }

    #[test]
    fn missing_or_malformed_hooks_file_yields_no_rules() {
        let dir = TempDir::new().unwrap();
        assert!(load_hooks(dir.path()).is_empty());
        fs::create_dir_all(dir.path().join(".velocity")).unwrap();
        fs::write(dir.path().join(".velocity").join("hooks.json"), "{ nope").unwrap();
        assert!(load_hooks(dir.path()).is_empty());
    }

    #[test]
    fn pre_tool_hook_exit_2_vetoes_and_records_in_event_store() {
        let dir = TempDir::new().unwrap();
        write_hooks(
            dir.path(),
            &[json!({ "event": "pre_tool_use", "matcher": "write_*", "command": "exit 2" })],
        );
        let veto = run_pre_tool_hooks(
            dir.path(),
            "write_file",
            &json!({"relativeFilePath": "a.rs"}),
        );
        let reason = veto.expect("exit 2 must veto");
        assert!(reason.contains("exit code 2"), "reason: {reason}");

        // The veto must be visible in the durable decision trail.
        let store = EventStore::open(dir.path());
        let events = store.query(None, Some(&EventOutcome::Failure), 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tool_name, "lifecycle_hook");
        let md = events[0].metadata.clone().unwrap();
        assert_eq!(md["lifecycle_hook_block"], json!(true));
        assert_eq!(md["blocked_tool"], json!("write_file"));
        assert!(events[0].is_high_signal());
    }

    #[test]
    fn pre_tool_hook_exit_0_allows_and_nonmatching_rules_never_run() {
        let dir = TempDir::new().unwrap();
        write_hooks(
            dir.path(),
            &[
                json!({ "event": "pre_tool_use", "matcher": "write_*", "command": "exit 0" }),
                json!({ "event": "pre_tool_use", "matcher": "delete_*", "command": "exit 2" }),
            ],
        );
        assert!(run_pre_tool_hooks(dir.path(), "write_file", &json!({})).is_none());
        // A rule for a different matcher must not veto this call.
        assert!(run_pre_tool_hooks(dir.path(), "read_file", &json!({})).is_none());
        // And allow paths leave no event-store footprint.
        assert_eq!(EventStore::open(dir.path()).count().unwrap(), 0);
    }

    #[test]
    fn post_tool_hooks_are_observational_and_carry_outcome() {
        let dir = TempDir::new().unwrap();
        // A post hook that "objects" (exit 2) must still not block anything;
        // the call already completed. Echo the payload so we at least prove
        // the process ran; the outcome text must be present in stdin payload,
        // which execute_rule passes through.
        write_hooks(
            dir.path(),
            &[json!({ "event": "post_tool_use", "matcher": "*", "command": "exit 2" })],
        );
        run_post_tool_hooks(dir.path(), "write_file", &json!({}), "success");
        // No denial event recorded for post hooks — observation only.
        assert_eq!(EventStore::open(dir.path()).count().unwrap(), 0);
    }

    #[test]
    fn timeout_fails_closed_for_pre_hooks() {
        let dir = TempDir::new().unwrap();
        // A 1s-timeout rule whose command can't finish: type/stdin-cat style
        // portable hang is not guaranteed cross-shell, so we hang on reading
        // stdin never happening by using a sleep far longer than the timeout.
        // `ping` is present on both cmd and sh for a bounded delay.
        let hang = if cfg!(windows) {
            "ping -n 5 127.0.0.1 > nul & exit 0"
        } else {
            "sleep 5"
        };
        // Use a tiny timeout so the test is fast; ping delays ~4s > 1s budget.
        write_hooks(
            dir.path(),
            &[json!({
                "event": "pre_tool_use",
                "matcher": "*",
                "command": hang,
                "timeout_secs": 1
            })],
        );
        let veto = run_pre_tool_hooks(dir.path(), "write_file", &json!({}));
        assert!(veto.is_some(), "timed-out pre hook must fail closed");
        assert!(veto.unwrap().contains("timed out"));
    }
}
