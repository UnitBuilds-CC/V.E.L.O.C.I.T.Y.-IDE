use crate::registry::types::Tool;
use serde_json::json;

pub fn get_wa_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "wa_create_session".to_string(),
            description: "Create a Rust-native WA semantic session artifact with NDA-backed persistence.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Stable WA session identifier." },
                    "compact": { "type": "boolean", "description": "When true, return a structured WA session creation summary instead of human-readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "wa_get_session".to_string(),
            description: "Read a persisted WA semantic session artifact.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "compact": { "type": "boolean", "description": "When true, return a structured WA session read summary instead of the raw session payload." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "wa_list_sessions".to_string(),
            description: "List persisted WA sessions using compact summaries.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionIdContains": { "type": "string", "description": "Optional case-insensitive substring filter on WA session id." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of sessions to return." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for session ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "wa_save_snapshot".to_string(),
            description: "Persist a WA semantic snapshot with compact node metadata and NDA sidecar.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Logical name for the saved snapshot." },
                    "url": { "type": "string", "description": "Source URL or logical surface identifier." },
                    "title": { "type": "string", "description": "Snapshot title or label." },
                    "focusNodeId": { "type": "string", "description": "Optional id of the focused node." },
                    "nodes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "role": { "type": "string" },
                                "name": { "type": "string" },
                                "value": { "type": "string" },
                                "actions": { "type": "array", "items": { "type": "string" } },
                                "visible": { "type": "boolean" },
                                "enabled": { "type": "boolean" },
                                "provenance": { "type": "string" },
                                "confidence": { "type": "number" }
                            },
                            "required": ["id", "role", "name"]
                        }
                    },
                    "compact": { "type": "boolean", "description": "When true, return a structured WA snapshot save summary instead of human-readable text." }
                },
                "required": ["sessionId", "snapshotName", "url", "title", "nodes"]
            }),
        },
        Tool {
            name: "wa_capture_windows_snapshot".to_string(),
            description: "Capture a live Windows accessibility snapshot via UIAutomation and persist it as a WA snapshot.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Logical name for the captured snapshot." },
                    "title": { "type": "string", "description": "Optional title override for the captured snapshot." },
                    "processId": { "type": "integer", "minimum": 1, "description": "Optional target process id. Defaults to the foreground or first named window when omitted." },
                    "windowNameContains": { "type": "string", "description": "Optional case-insensitive window title substring filter." },
                    "maxDepth": { "type": "integer", "minimum": 0, "description": "Optional maximum UIAutomation traversal depth. Defaults to 3." },
                    "maxChildrenPerNode": { "type": "integer", "minimum": 1, "description": "Optional maximum number of children to inspect per node. Defaults to 64." },
                    "compact": { "type": "boolean", "description": "When true, return a structured Windows capture report instead of human-readable text." }
                },
                "required": ["sessionId", "snapshotName"]
            }),
        },
        Tool {
            name: "wa_read_snapshot".to_string(),
            description: "Read a persisted WA semantic snapshot.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Saved snapshot name." },
                    "compact": { "type": "boolean", "description": "When true, return a structured WA snapshot read summary instead of the raw snapshot payload." }
                },
                "required": ["sessionId", "snapshotName"]
            }),
        },
        Tool {
            name: "wa_list_snapshots".to_string(),
            description: "List persisted WA snapshots using compact summaries.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Optional WA session id filter." },
                    "snapshotNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on snapshot name." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of snapshots to return." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for snapshot ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "wa_save_script".to_string(),
            description: "Persist a deterministic WA semantic script artifact with NDA sidecar.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Workflow/script name." },
                    "startUrl": { "type": "string", "description": "Optional start URL for the script." },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": { "type": "string" },
                                "nodeId": { "type": "string" },
                                "role": { "type": "string" },
                                "name": { "type": "string" },
                                "value": { "type": "string" },
                                "required": { "type": "boolean" }
                            },
                            "required": ["action"]
                        }
                    },
                    "compact": { "type": "boolean", "description": "When true, return a structured WA script save summary instead of human-readable text." }
                },
                "required": ["name", "steps"]
            }),
        },
        Tool {
            name: "wa_read_script".to_string(),
            description: "Read a saved WA semantic script artifact.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .wa.nda file relative to the workspace root. Legacy .wa.json paths are still accepted for read fallback." },
                    "compact": { "type": "boolean", "description": "When true, return a structured WA script read summary instead of the raw script payload." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "wa_list_scripts".to_string(),
            description: "List saved WA semantic script artifacts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "scriptNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on script name." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of scripts to return." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for script ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "wa_resolve_selector".to_string(),
            description: "Resolve a deterministic WA selector against a saved semantic snapshot.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "nodeId": { "type": "string", "description": "Optional exact node id." },
                    "role": { "type": "string", "description": "Optional semantic role filter." },
                    "name": { "type": "string", "description": "Optional semantic name filter." },
                    "action": { "type": "string", "description": "Optional required action capability." },
                    "compact": { "type": "boolean", "description": "When true, return a structured selector resolution report." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "wa_plan_action".to_string(),
            description: "Plan a deterministic WA action against a saved semantic snapshot without executing it.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "action": { "type": "string", "description": "Action to plan, such as click, focus, type, or submit." },
                    "nodeId": { "type": "string", "description": "Optional exact node id." },
                    "role": { "type": "string", "description": "Optional semantic role filter." },
                    "name": { "type": "string", "description": "Optional semantic name filter." },
                    "value": { "type": "string", "description": "Optional input value for type/fill style actions." },
                    "compact": { "type": "boolean", "description": "When true, return a structured action plan report instead of human-readable text." }
                },
                "required": ["sessionId", "action"]
            }),
        },
        Tool {
            name: "wa_execute_windows_action".to_string(),
            description: "Execute a deterministic Windows UIAutomation action against a saved WA snapshot.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "action": { "type": "string", "description": "Action to execute, such as click, focus, type, select, toggle, expand, or collapse." },
                    "nodeId": { "type": "string", "description": "Optional exact node id." },
                    "role": { "type": "string", "description": "Optional semantic role filter." },
                    "name": { "type": "string", "description": "Optional semantic name filter." },
                    "value": { "type": "string", "description": "Optional input value for type actions." },
                    "compact": { "type": "boolean", "description": "When true, return a structured Windows action execution report." }
                },
                "required": ["sessionId", "action"]
            }),
        },
        Tool {
            name: "wa_wait_for_windows_condition".to_string(),
            description: "Wait for a deterministic Windows UIAutomation condition against a saved WA snapshot.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "condition": { "type": "string", "description": "Condition to wait for: exists, focused, or value_equals." },
                    "nodeId": { "type": "string", "description": "Optional exact node id." },
                    "role": { "type": "string", "description": "Optional semantic role filter." },
                    "name": { "type": "string", "description": "Optional semantic name filter." },
                    "expectedValue": { "type": "string", "description": "Expected value when condition is value_equals." },
                    "timeoutMs": { "type": "integer", "minimum": 1, "description": "Maximum wait duration in milliseconds. Defaults to 3000." },
                    "pollIntervalMs": { "type": "integer", "minimum": 1, "description": "Polling interval in milliseconds. Defaults to 100." },
                    "compact": { "type": "boolean", "description": "When true, return a structured Windows wait report." }
                },
                "required": ["sessionId", "condition"]
            }),
        },
        Tool {
            name: "wa_run_script".to_string(),
            description: "Run a saved WA semantic script deterministically against the Windows automation layer.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .wa.nda file relative to the workspace root." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "startStepIndex": { "type": "integer", "minimum": 0, "description": "Optional zero-based step index to resume execution from." },
                    "compact": { "type": "boolean", "description": "When true, return a structured persisted WA script run artifact." }
                },
                "required": ["sessionId", "relativeFilePath"]
            }),
        },
        Tool {
            name: "wa_read_run".to_string(),
            description: "Read a persisted WA script run artifact.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .wa-run.nda file relative to the workspace root." },
                    "compact": { "type": "boolean", "description": "When true, return the structured persisted WA run artifact." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "wa_list_runs".to_string(),
            description: "List persisted WA script run artifacts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Optional WA session id filter." },
                    "scriptNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on script name." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of runs to return." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for run ordering. Defaults to asc." }
                }
            }),
        },
    ]
}
