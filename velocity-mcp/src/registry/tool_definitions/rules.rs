//! The rules lifecycle surface: read, create, adjust, retire, bypass.
//!
//! Deliberately missing: approve and reject. A loosening or removal only takes
//! effect when a human or a team supervisor signs it off in the Settings → Rules
//! panel, so the tool that could file the request must not be the tool that can
//! grant it.

use crate::registry::types::Tool;
use serde_json::json;

pub fn get_rules_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "rules_list".to_string(),
            description: "List the deterministic rules enforced on every tool call in this \
             workspace, grouped by scope (task, workspace, global). Each entry shows its id, \
             kind, live parameters and the intent a refusal will quote. Rules are re-read on \
             every call, so what you see here is what the next call is judged against."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "include_retired": { "type": "boolean", "description": "Also list retired rules. Defaults to false." }
                }
            }),
        },
        Tool {
            name: "rule_show".to_string(),
            description: "Show one rule in full: current parameters with their allowed ranges, \
             the witness pair that proves it fires, its version history, and any live bypass \
             grant. Call this before rule_adjust so you change parameters you know the bounds of."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ruleId": { "type": "string", "description": "The rule id, as returned by rules_list." }
                },
                "required": ["ruleId"]
            }),
        },
        Tool {
            name: "rule_kinds".to_string(),
            description: "Describe every rule kind and the parameters it takes, including the \
             ranges the editor enforces. Call this first if you are unsure which kind fits, or \
             what a parameter has to hold for the rule to be able to fire at all."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "rule_create".to_string(),
            description: "Create a rule that binds tool calls from the next call onward, in \
             every agent sharing this workspace. Use it when the user says 'never do that \
             again': the constraint belongs here, not in your notes, because a rule refuses \
             the call deterministically whether or not you remember. A rule MUST have a \
             description stating the intent (it is quoted in the refusal the agent reads) and \
             a remedy naming the sanctioned way to do the job, so the refusal is directional \
             instead of a wall. The engine synthesizes a witness pair and refuses to store a \
             rule that cannot actually fire, so a rule that matches nothing is rejected here \
             rather than silently doing nothing later."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Short handle shown in the refusal, e.g. 'live-db-off-limits'." },
                    "description": { "type": "string", "description": "Why the rule exists, in one or two sentences. Quoted verbatim in every refusal, so it must say what the banned thing IS." },
                    "kind": { "type": "string", "enum": ["tool_ban", "egress_ban", "path_ban", "command_pattern", "tool_budget"], "description": "What the rule refuses. See rule_kinds for the parameters each expects." },
                    "params": {
                        "type": "object",
                        "description": "The rule's knobs, keyed by the parameter names rule_kinds lists for the kind. Lists for bans ('targets': [\"192.168.1.100\"]), whole numbers for budgets ('tool_calls': 100).",
                        "additionalProperties": true
                    },
                    "scope": { "type": "string", "enum": ["task", "workspace", "global"], "description": "task: this run only, in memory. workspace: this project, committed with it. global: every workspace and agent on this machine." },
                    "remedy": { "type": "string", "description": "What to do instead, e.g. 'use the dev database at 192.168.0.2'. Strongly recommended: without it the refusal can only say no." },
                    "provenance": { "type": "string", "description": "What made this rule necessary, e.g. 'turn 7: user said never query the live db'." },
                    "denyTool": { "type": "string", "description": "Optional witness: a tool call that MUST be blocked, e.g. 'run_command'. Required for command_pattern." },
                    "denyArguments": { "type": "object", "description": "Optional witness arguments for denyTool.", "additionalProperties": true },
                    "allowTool": { "type": "string", "description": "Optional witness: a near-miss tool call that MUST still be allowed." },
                    "allowArguments": { "type": "object", "description": "Optional witness arguments for allowTool.", "additionalProperties": true }
                },
                "required": ["name", "description", "kind", "params", "scope", "provenance"]
            }),
        },
        Tool {
            name: "rule_adjust".to_string(),
            description: "Change a rule's name, description, parameters or remedy. On a task \
             rule this applies at once. On a workspace or global rule an agent may only file \
             the change for approval — it is applied when a human or the team supervisor \
             approves it in Settings → Rules. Loosening a rule you are being held to is the \
             one thing this cannot do by itself."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ruleId": { "type": "string", "description": "The rule to change." },
                    "name": { "type": "string", "description": "Optional new handle." },
                    "description": { "type": "string", "description": "Optional new intent text." },
                    "params": {
                        "type": "object",
                        "description": "Optional replacement parameter set. Send the whole set, not a delta: a missing key is a missing parameter and will be rejected.",
                        "additionalProperties": true
                    },
                    "remedy": { "type": "string", "description": "Optional new remedy." },
                    "reason": { "type": "string", "description": "Why the change is needed. Recorded on the request the approver reads." }
                },
                "required": ["ruleId", "reason"]
            }),
        },
        Tool {
            name: "rule_retire".to_string(),
            description: "Retire a rule — a tombstone, never an erase: its history stays and \
             the refusal trail can still name it. Use it when a rule is broken (it blocks \
             work it should not) or no longer relevant. Task rules retire at once; workspace \
             and global rules need an approver."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ruleId": { "type": "string", "description": "The rule to retire." },
                    "reason": { "type": "string", "description": "Why it should stop applying." }
                },
                "required": ["ruleId", "reason"]
            }),
        },
        Tool {
            name: "rule_bypass".to_string(),
            description: "Ask for a bypass of one rule instead of working around it. 'event' \
             releases exactly one matching call; 'session' and 'workspace' release matches for \
             a number of minutes (max 10080). An agent can only request; the grant is issued \
             from Settings → Rules. Prefer this to retrying a blocked call — a bypass leaves a \
             record, a workaround leaves nothing."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ruleId": { "type": "string", "description": "The rule to bypass." },
                    "bypassScope": { "type": "string", "enum": ["event", "session", "workspace"], "description": "event: the next single matching call. session/workspace: a window of minutes." },
                    "minutes": { "type": "integer", "description": "Window length for session and workspace bypasses. Ignored for event." },
                    "reason": { "type": "string", "description": "Why this call must go through. Required." }
                },
                "required": ["ruleId", "bypassScope", "reason"]
            }),
        },
        Tool {
            name: "rules_pending".to_string(),
            description: "List the change requests waiting on an approver — adjustments, \
             retirements and bypasses an agent filed against a durable rule. The rules \
             themselves are unchanged until one is approved, so this is also how you check \
             that a loosening you asked for has not happened yet."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}
