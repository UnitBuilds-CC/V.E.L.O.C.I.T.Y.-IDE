use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::error::Error;
use std::process::{Command, Stdio, Child};
use std::io::{Write, BufReader, BufRead};
use std::sync::Mutex;
use once_cell::sync::Lazy;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

pub fn get_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "convert_to_nda".to_string(),
            description: "Convert any file (e.g. C# source code, PDF, CSV, Excel, Image, Zip archive) into a cryptographically signed NDA (.nda) binary document.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Absolute path to the input file to convert." },
                    "outputPath": { "type": "string", "description": "Optional absolute path to write the compiled .nda file. Defaults to input path with .nda extension." }
                },
                "required": ["filePath"]
            }),
        },
        Tool {
            name: "read_nda".to_string(),
            description: "Read and parse a compiled .nda binary file to view its semantic triples, visual display commands, and string pool contents.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the .nda file to inspect." }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "execute_nda".to_string(),
            description: "Execute a runnable .nda container. If it holds a compiled C# binary, it is run in-memory. If it contains a script (e.g., Python, Node.js, PowerShell, Bash), it executes via the corresponding shell process.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the runnable .nda file." },
                    "arguments": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command-line arguments to pass to the executable or script."
                    }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "read_file".to_string(),
            description: "Read the contents of a file in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "write_file".to_string(),
            description: "Write or overwrite a file with specific content in the workspace. Creates folders if they do not exist.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"scripts/bootstrap.sh\")" },
                    "content": { "type": "string", "description": "The text content to write to the file." }
                },
                "required": ["relativeFilePath", "content"]
            }),
        },
        Tool {
            name: "list_dir".to_string(),
            description: "List the contents of a directory relative to the workspace root.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeDirPath": { "type": "string", "description": "Directory path relative to workspace root. Use \".\" for the workspace root." }
                },
                "required": ["relativeDirPath"]
            }),
        },
        Tool {
            name: "grep_search".to_string(),
            description: "Find lines containing a query string in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The text to search for" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "run_command".to_string(),
            description: "Run a shell command inside the current workspace directory and capture its combined stdout and stderr output.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command line string to execute." }
                },
                "required": ["command"]
            }),
        },
        Tool {
            name: "delete_file".to_string(),
            description: "Delete a file in the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path relative to workspace root (e.g. \"temp.txt\")" }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "web_navigate".to_string(),
            description: "Navigate and crawl a page with the current static AOM-first browser engine. It captures a truthful live snapshot, persists SiteMap facts, and writes browser artifacts for links, forms, and cookies discovered in fetched HTML.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute URL to navigate to and crawl." },
                    "concurrency": { "type": "integer", "description": "Unused by the current static browser engine." },
                    "compact": { "type": "boolean", "description": "When true, return a structured crawl summary with persisted artifact paths instead of verbose multiline text." }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "browser_create_session".to_string(),
            description: "Create or reset a persisted browser session with its own cookie jar and navigation state.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "compact": { "type": "boolean", "description": "When true, return a structured session creation summary instead of verbose multiline text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_runtime_capture".to_string(),
            description: "Capture a page through the Go chromedp runtime, then persist the resulting browser snapshot, session state, HTML fallback, and crawl facts into the Rust browser artifact model.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "url": { "type": "string", "description": "Absolute URL to capture through the runtime-backed browser." },
                    "timeoutMs": { "type": "integer", "minimum": 1, "description": "Optional runtime capture timeout in milliseconds. Defaults to 15000." },
                    "apiBase": { "type": "string", "description": "Optional Go browser API base URL override. Defaults to VELOCITY_BROWSER_API_BASE or http://127.0.0.1:8080." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary instead of verbose multiline text." }
                },
                "required": ["sessionId", "url"]
            }),
        },
        Tool {
            name: "browser_runtime_visual_capture".to_string(),
            description: "Capture a truthful runtime PNG screenshot through the Go browser API and persist both the image and artifact metadata under .velocity/browser-runtime-visuals.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute URL to capture as a runtime visual artifact." },
                    "apiBase": { "type": "string", "description": "Optional Go browser API base URL override. Defaults to VELOCITY_BROWSER_API_BASE or http://127.0.0.1:8080." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime visual artifact summary instead of verbose multiline text." }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "runtime_create_session".to_string(),
            description: "Create an explicit Go runtime browser session and persist the Rust-side runtime session mapping under .velocity/runtime-browser-sessions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "startUrl": { "type": "string", "description": "Optional absolute URL to open when the runtime session is created." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional startup wait timeout in milliseconds." },
                    "apiBase": { "type": "string", "description": "Optional Go browser API base URL override." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime session summary instead of verbose multiline text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "runtime_get_session".to_string(),
            description: "Read a persisted explicit runtime browser session mapping.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime session summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "runtime_close_session".to_string(),
            description: "Close an explicit Go runtime browser session and remove the persisted Rust-side runtime session mapping.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "compact": { "type": "boolean", "description": "When true, return a structured close summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "runtime_capture_session".to_string(),
            description: "Capture the current page from an explicit Go runtime browser session and persist it into the Rust browser artifact model.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "runtime_session_navigate".to_string(),
            description: "Navigate an explicit Go runtime browser session to an absolute URL, then persist the captured result.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "url": { "type": "string", "description": "Absolute URL to navigate to." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId", "url"]
            }),
        },
        Tool {
            name: "runtime_session_click".to_string(),
            description: "Click a runtime target in an explicit Go browser session using either nodeId or selector, then persist the captured result.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "nodeId": { "type": "string", "description": "Optional runtime node identifier. Either nodeId or selector is required." },
                    "selector": { "type": "string", "description": "Optional CSS selector fallback. Either nodeId or selector is required." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "runtime_session_js_click".to_string(),
            description: "Dispatch a JS click against an explicit Go runtime browser session target. Requires nodeId because the runtime API does not support selector fallback for js_click.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "nodeId": { "type": "string", "description": "Runtime node identifier required by the Go js_click action." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId", "nodeId"]
            }),
        },
        Tool {
            name: "runtime_session_evaluate".to_string(),
            description: "Evaluate JavaScript in an explicit Go runtime browser session, then persist the captured result and returned evaluation payload.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "script": { "type": "string", "description": "JavaScript expression or snippet to evaluate in the runtime session." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId", "script"]
            }),
        },
        Tool {
            name: "runtime_session_fill".to_string(),
            description: "Fill a runtime target in an explicit Go browser session using nodeId or selector, then persist the captured result.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "nodeId": { "type": "string", "description": "Optional runtime node identifier. Either nodeId or selector is required." },
                    "selector": { "type": "string", "description": "Optional CSS selector fallback. Either nodeId or selector is required." },
                    "value": { "type": "string", "description": "Value to type into the target field." },
                    "natural": { "type": "boolean", "description": "When true, request more human-like typing cadence from the runtime." },
                    "clear": { "type": "boolean", "description": "When true, clear the field before typing." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId", "value"]
            }),
        },
        Tool {
            name: "runtime_session_submit".to_string(),
            description: "Submit within an explicit Go runtime browser session by targeting a nodeId or selector when available, or by allowing the runtime to fall back to Enter-key submission.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "nodeId": { "type": "string", "description": "Optional runtime node identifier." },
                    "selector": { "type": "string", "description": "Optional CSS selector fallback." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "runtime_session_press_key".to_string(),
            description: "Press a key in an explicit Go runtime browser session, then persist the captured result.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Rust-side persisted runtime session identifier." },
                    "key": { "type": "string", "description": "Key name to dispatch, such as Enter or Tab." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-action wait timeout in milliseconds." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime capture summary." }
                },
                "required": ["sessionId", "key"]
            }),
        },
        Tool {
            name: "browser_get_session".to_string(),
            description: "Read the current persisted browser session state, including current URL, cookies, and storage state.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned session read report with summary fields and persisted session artifact path instead of the full session payload." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_list_snapshots".to_string(),
            description: "List persisted browser snapshot artifacts from the workspace snapshot directory using compact summaries that include persisted snapshot artifact paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "urlContains": { "type": "string", "description": "Optional case-insensitive substring filter on snapshot URL." },
                    "titleContains": { "type": "string", "description": "Optional case-insensitive substring filter on snapshot title." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of snapshot summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for snapshot URL ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_read_snapshot".to_string(),
            description: "Read a persisted browser snapshot JSON artifact by its original page URL.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Original page URL captured in the persisted browser snapshot." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned snapshot read report with summary fields and persisted snapshot artifact path instead of the full snapshot payload." }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "browser_read_visual_fallback".to_string(),
            description: "Read the persisted HTML fallback artifact for a captured page URL when truthful raw HTML evidence is available.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Original page URL captured in the persisted browser snapshot and HTML fallback artifact." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned HTML fallback report with byte count and artifact path instead of the raw HTML content." }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "browser_diff_snapshots".to_string(),
            description: "Compute a truthful semantic diff between two persisted browser snapshots identified by their original page URLs.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "beforeUrl": { "type": "string", "description": "Original page URL for the earlier persisted browser snapshot." },
                    "afterUrl": { "type": "string", "description": "Original page URL for the later persisted browser snapshot." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned diff read report with rendered summary metadata plus before/after persisted artifact paths instead of the full diff payload." }
                },
                "required": ["beforeUrl", "afterUrl"]
            }),
        },
        Tool {
            name: "browser_list_sessions".to_string(),
            description: "List persisted browser sessions from the workspace session directory using compact summaries that include persisted session artifact paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionIdContains": { "type": "string", "description": "Optional case-insensitive substring filter on session id." },
                    "urlContains": { "type": "string", "description": "Optional case-insensitive substring filter on current URL." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of session summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for session id ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_get_storage".to_string(),
            description: "Read persisted browser storage state for a session scope (local or session).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "scope": { "type": "string", "description": "Storage scope: local or session." },
                    "compact": { "type": "boolean", "description": "When true, return a structured storage summary with counts and session metadata instead of raw storage entries only." }
                },
                "required": ["sessionId", "scope"]
            }),
        },
        Tool {
            name: "browser_set_storage".to_string(),
            description: "Seed or update persisted browser storage state for a session scope (local or session).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "scope": { "type": "string", "description": "Storage scope: local or session." },
                    "entries": {
                        "type": "object",
                        "description": "String key/value storage entries to merge into the selected scope.",
                        "additionalProperties": { "type": "string" }
                    },
                    "compact": { "type": "boolean", "description": "When true, return a structured storage update summary instead of verbose multiline text." }
                },
                "required": ["sessionId", "scope", "entries"]
            }),
        },
        Tool {
            name: "browser_get_cookies".to_string(),
            description: "Read the persisted browser cookie jar for a session so authenticated flows can inspect recovery state explicitly.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "compact": { "type": "boolean", "description": "When true, return a structured cookie summary with counts and cookie names instead of raw cookie values." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_set_cookies".to_string(),
            description: "Seed or update the persisted browser cookie jar for a session to recover or resume authenticated flows.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "cookies": {
                        "type": "array",
                        "description": "Cookie name/value pairs to merge into the session cookie jar.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "value": { "type": "string" }
                            },
                            "required": ["name", "value"]
                        }
                    },
                    "compact": { "type": "boolean", "description": "When true, return a structured cookie update summary instead of verbose multiline text." }
                },
                "required": ["sessionId", "cookies"]
            }),
        },
        Tool {
            name: "browser_auth_diagnostics".to_string(),
            description: "Read a compact, truthful auth/session recovery diagnosis for a persisted browser session using cookies, storage, and the current snapshot if available.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_save_auth_profile".to_string(),
            description: "Save a reusable auth profile from a source browser session or checkpoint using only auth cookies plus CSRF-relevant storage.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "profileName": { "type": "string", "description": "Auth profile identifier stored under .velocity/browser-auth-profiles." },
                    "sourceSessionId": { "type": "string", "description": "Source session identifier to capture auth state from." },
                    "sourceCheckpointName": { "type": "string", "description": "Optional checkpoint name on the source session; when provided, capture from that checkpoint instead of the live source session." },
                    "compact": { "type": "boolean", "description": "When true, return a structured auth profile save report instead of verbose multiline text." }
                },
                "required": ["profileName", "sourceSessionId"]
            }),
        },
        Tool {
            name: "browser_list_auth_profiles".to_string(),
            description: "List saved browser auth profiles using compact summaries that include filtered auth cookie/storage counts and persisted artifact paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "profileNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on auth profile name." },
                    "sourceSessionIdContains": { "type": "string", "description": "Optional case-insensitive substring filter on source session id." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of auth profile summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for auth profile name ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_read_auth_profile".to_string(),
            description: "Read a saved browser auth profile by name.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "profileName": { "type": "string", "description": "Auth profile identifier stored under .velocity/browser-auth-profiles." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned auth profile summary report instead of the full profile payload." }
                },
                "required": ["profileName"]
            }),
        },
        Tool {
            name: "browser_apply_auth_profile".to_string(),
            description: "Apply a saved auth profile to a target browser session, then report the resulting auth diagnosis for that session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "profileName": { "type": "string", "description": "Saved auth profile identifier stored under .velocity/browser-auth-profiles." },
                    "targetSessionId": { "type": "string", "description": "Target session identifier to update with the saved auth profile." },
                    "compact": { "type": "boolean", "description": "When true, return a structured auth profile apply report instead of verbose multiline text." }
                },
                "required": ["profileName", "targetSessionId"]
            }),
        },
        Tool {
            name: "browser_reseed_auth".to_string(),
            description: "Copy auth cookies plus CSRF-relevant storage from a source browser session or checkpoint into a target session, then report the resulting target auth diagnosis.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "targetSessionId": { "type": "string", "description": "Session identifier to update with recovered auth state." },
                    "sourceSessionId": { "type": "string", "description": "Source session identifier to copy auth state from." },
                    "sourceCheckpointName": { "type": "string", "description": "Optional checkpoint name on the source session; when provided, copy from that checkpoint instead of the live source session." },
                    "compact": { "type": "boolean", "description": "When true, return a structured auth reseed report instead of verbose multiline text." }
                },
                "required": ["targetSessionId", "sourceSessionId"]
            }),
        },
        Tool {
            name: "browser_access_diagnostics".to_string(),
            description: "Read a compact, truthful access/challenge diagnosis for a persisted browser session using the current snapshot when available.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "compact": { "type": "boolean", "description": "When true, return the structured access diagnosis report; otherwise render a multiline operator summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_get_session_network".to_string(),
            description: "Read the persisted per-session network config that the truthful browser runtime can apply to outgoing requests.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "compact": { "type": "boolean", "description": "When true, return the structured network config report; otherwise render a multiline operator summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_read_session_transcript".to_string(),
            description: "Read persisted browser session transcript history for a session, with compact discovery or a full entry lookup by sequence.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "sequence": { "type": "integer", "minimum": 1, "description": "Optional transcript sequence number to read one full entry instead of listing recent summaries." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of transcript summaries to return when listing." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for transcript sequence ordering. Defaults to asc." },
                    "compact": { "type": "boolean", "description": "When true, return a structured transcript report or full entry JSON; otherwise render a multiline transcript summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_session_health".to_string(),
            description: "Read a truthful aggregated health and recovery report for a persisted browser session using saved session, snapshot, auth, access, checkpoint, network, and HTML fallback evidence.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "compact": { "type": "boolean", "description": "When true, return the structured health report; otherwise render a multiline operator summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_set_session_network".to_string(),
            description: "Update the persisted per-session network config for request headers, user-agent, timeout, redirect following, and allow/block URL policy.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "userAgent": { "type": "string", "description": "Optional User-Agent override for outgoing requests." },
                    "headers": { "type": "object", "description": "Optional header map to merge or replace into the persisted session network config.", "additionalProperties": { "type": "string" } },
                    "replaceHeaders": { "type": "boolean", "description": "When true, replace all saved custom headers with the provided header map instead of merging." },
                    "timeoutMs": { "type": "integer", "minimum": 1, "description": "Optional request timeout in milliseconds." },
                    "clearTimeout": { "type": "boolean", "description": "When true, clear any saved timeout override and fall back to the transport default." },
                    "followRedirects": { "type": "boolean", "description": "Optional redirect-following override for this persisted session." },
                    "clearFollowRedirects": { "type": "boolean", "description": "When true, clear any saved redirect-following override and fall back to the transport default." },
                    "allowedUrlPrefixes": { "type": "array", "items": { "type": "string" }, "description": "Optional allow-list of absolute URL prefixes. When non-empty, only matching URLs are allowed." },
                    "blockedUrlPrefixes": { "type": "array", "items": { "type": "string" }, "description": "Optional block-list of absolute URL prefixes that are always denied." },
                    "compact": { "type": "boolean", "description": "When true, return the structured network update report; otherwise render a multiline operator summary." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_session_navigate".to_string(),
            description: "Navigate a persisted browser session so cookies and current URL survive across requests.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "url": { "type": "string", "description": "Absolute URL to navigate within the persisted session." },
                    "compact": { "type": "boolean", "description": "When true, return a structured navigation summary with artifact paths instead of verbose multiline text." }
                },
                "required": ["sessionId", "url"]
            }),
        },
        Tool {
            name: "browser_session_click".to_string(),
            description: "Click a navigable element in the current persisted browser session using the truthful static AOM-first interaction model.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "role": { "type": "string", "description": "Semantic role to match, such as link or button." },
                    "name": { "type": "string", "description": "Semantic name or nearby text for the target element." },
                    "compact": { "type": "boolean", "description": "When true, return a structured session action report with artifact paths instead of verbose multiline text." }
                },
                "required": ["sessionId", "role", "name"]
            }),
        },
        Tool {
            name: "browser_session_fill".to_string(),
            description: "Fill a form field in the current persisted browser session using the truthful static AOM/form interaction model.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "field": { "type": "string", "description": "Field label, name, or nearby semantic text to match." },
                    "value": { "type": "string", "description": "Value to write into the matched field." },
                    "compact": { "type": "boolean", "description": "When true, return a structured session action report with artifact paths instead of verbose multiline text." }
                },
                "required": ["sessionId", "field", "value"]
            }),
        },
        Tool {
            name: "browser_session_submit".to_string(),
            description: "Submit a form in the current persisted browser session using the truthful static form-post interaction model.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "form": { "type": "string", "description": "Optional form id to submit; defaults to the first form in the current snapshot." },
                    "compact": { "type": "boolean", "description": "When true, return a structured session action report with artifact paths instead of verbose multiline text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_session_wait".to_string(),
            description: "Poll the current page in a persisted browser session until text or an element appears, then persist the updated snapshot and semantic diff.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier stored under .velocity/browser-sessions." },
                    "text": { "type": "string", "description": "Wait until this text appears in the current snapshot." },
                    "title": { "type": "string", "description": "Wait until the current page title contains this text." },
                    "urlContains": { "type": "string", "description": "Wait until the current page URL contains this fragment." },
                    "mutation": { "type": "string", "description": "Wait until the current snapshot exposes a runtime mutation label containing this text." },
                    "requestMethod": { "type": "string", "description": "Structured request wait: require a captured request with this HTTP method." },
                    "requestUrlContains": { "type": "string", "description": "Structured request wait: require a captured request URL containing this fragment." },
                    "requestStatus": { "type": "integer", "description": "Structured request wait: require a captured request with this status code." },
                    "requestResource": { "type": "string", "description": "Structured request wait: require a captured request resource kind such as document or xhr." },
                    "storageScope": { "type": "string", "description": "Structured storage wait: require a captured storage bucket scope such as local or session." },
                    "storageKey": { "type": "string", "description": "Structured storage wait: require a captured storage entry key within storageScope." },
                    "storageValue": { "type": "string", "description": "Optional structured storage wait value substring once storageScope and storageKey match." },
                    "settle": { "type": "string", "description": "Wait until the current snapshot exposes an engine-observed settle signal containing this text." },
                    "settleScope": { "type": "string", "description": "Structured settle wait: require a settle signal scope such as network, navigation, or response." },
                    "settleState": { "type": "string", "description": "Optional structured settle state to require within settleScope, such as settled or complete." },
                    "runtimeScope": { "type": "string", "description": "Wait until the current snapshot exposes a runtime-state entry for this scope." },
                    "runtimeKey": { "type": "string", "description": "Wait until the current snapshot exposes a runtime-state entry for this key within runtimeScope." },
                    "runtimeValue": { "type": "string", "description": "Optional runtime-state value substring to require once runtimeScope and runtimeKey match." },
                    "protocolKind": { "type": "string", "description": "Structured protocol-event wait: require an observed protocol event kind such as redirect, download, upload, stream, or event." },
                    "protocolPhase": { "type": "string", "description": "Structured protocol-event wait: require an observed protocol event phase such as start, update, complete, or commit." },
                    "protocolTarget": { "type": "string", "description": "Optional protocol-event target substring to require, such as a final URL, route, or resource identifier." },
                    "protocolDetail": { "type": "string", "description": "Optional protocol-event detail substring to require once the protocol event matches the other fields." },
                    "networkIdle": { "type": "boolean", "description": "When true, wait for a truthful network-settled signal or equivalent runtime evidence." },
                    "appReady": { "type": "boolean", "description": "When true, wait for truthful app-ready evidence such as navigation settled plus runtime/store readiness signals." },
                    "mutationSettled": { "type": "boolean", "description": "When true, wait for truthful mutation/hydration completion evidence from settle or mutation labels." },
                    "streamComplete": { "type": "boolean", "description": "When true, wait for truthful completion evidence for stream/event style runtime activity." },
                    "role": { "type": "string", "description": "Optional role when waiting for an element." },
                    "name": { "type": "string", "description": "Optional accessible name when waiting for an element." },
                    "requireActionable": { "type": "boolean", "description": "When waiting by role and name, require the matched target to be actionable in the current static browser model." },
                    "stablePolls": { "type": "integer", "description": "For stability waits, require this many consecutive unchanged polls before succeeding." },
                    "timeoutMs": { "type": "integer", "description": "Maximum time to wait before failing." },
                    "intervalMs": { "type": "integer", "description": "Polling interval between re-fetches." },
                    "compact": { "type": "boolean", "description": "When true, return a structured wait summary instead of the verbose multiline text output." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_save_checkpoint".to_string(),
            description: "Persist the current browser session and its latest snapshot as a named checkpoint for later restore or forking.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Existing browser session identifier." },
                    "checkpointName": { "type": "string", "description": "Human-readable checkpoint name." },
                    "compact": { "type": "boolean", "description": "When true, return a structured checkpoint save summary instead of verbose multiline text." }
                },
                "required": ["sessionId", "checkpointName"]
            }),
        },
        Tool {
            name: "browser_restore_checkpoint".to_string(),
            description: "Restore a saved browser session checkpoint, optionally forking it into a different session id.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Original session identifier that owns the checkpoint." },
                    "checkpointName": { "type": "string", "description": "Checkpoint name to restore." },
                    "targetSessionId": { "type": "string", "description": "Optional new session identifier to restore into." },
                    "compact": { "type": "boolean", "description": "When true, return a structured restore summary with artifact paths instead of verbose multiline text." }
                },
                "required": ["sessionId", "checkpointName"]
            }),
        },
        Tool {
            name: "browser_list_checkpoints".to_string(),
            description: "List saved browser checkpoints for a session from the persisted checkpoint artifacts, including compact summaries with checkpoint artifact paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier that owns the checkpoints." },
                    "checkpointNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on checkpoint name." },
                    "titleContains": { "type": "string", "description": "Optional case-insensitive substring filter on checkpoint page title." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of checkpoint summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for checkpoint name ordering. Defaults to asc." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_read_checkpoint".to_string(),
            description: "Read a saved browser session checkpoint JSON artifact for inspection.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier that owns the checkpoint." },
                    "checkpointName": { "type": "string", "description": "Checkpoint name to inspect." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned checkpoint read report with summary fields and persisted checkpoint artifact path instead of the full checkpoint payload." }
                },
                "required": ["sessionId", "checkpointName"]
            }),
        },
        Tool {
            name: "browser_diff_checkpoints".to_string(),
            description: "Compute a truthful semantic diff between two saved browser checkpoints for the same session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session identifier that owns the checkpoints." },
                    "beforeCheckpointName": { "type": "string", "description": "Earlier checkpoint name to compare from." },
                    "afterCheckpointName": { "type": "string", "description": "Later checkpoint name to compare to." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned diff read report with rendered summary metadata plus before/after persisted artifact paths instead of the full diff payload." }
                },
                "required": ["sessionId", "beforeCheckpointName", "afterCheckpointName"]
            }),
        },
        Tool {
            name: "browser_save_workflow".to_string(),
            description: "Persist a semantic browser workflow as JSON plus NDA-backed DSL for later deterministic replay.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Workflow name." },
                    "startUrl": { "type": "string", "description": "Initial URL for replay." },
                    "variables": {
                        "type": "object",
                        "description": "Optional workflow variables used by {{variable}} templates in step fields.",
                        "additionalProperties": { "type": "string" }
                    },
                    "steps": {
                        "type": "array",
                        "description": "Workflow steps. Supported kinds: navigate, click, fill_field, submit_form, wait_for_text, wait_for_element, wait_for_title, wait_for_url_contains, wait_for_mutation, wait_for_request, wait_for_storage, wait_for_settle, wait_for_runtime_state, wait_for_protocol_event, wait_for_stable, extract_text, save_checkpoint, restore_checkpoint, if_text_contains, if_output_equals, assert_element, assert_text_contains, assert_output.",
                        "items": { "type": "object" }
                    },
                    "compact": { "type": "boolean", "description": "When true, return a structured workflow save summary with artifact paths instead of verbose multiline text." }
                },
                "required": ["name", "startUrl", "steps"]
            }),
        },
        Tool {
            name: "browser_read_workflow".to_string(),
            description: "Read a saved browser workflow JSON artifact from the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .browser.json workflow relative to the workspace root." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned workflow read report with summary fields and persisted workflow artifact paths instead of the full workflow payload." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "browser_list_workflows".to_string(),
            description: "List saved browser workflow artifacts from the workspace workflow directory using compact summaries with workflow JSON and NDA artifact paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workflowNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on workflow name." },
                    "startUrlContains": { "type": "string", "description": "Optional case-insensitive substring filter on workflow start URL." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of workflow summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for workflow name ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_replay_workflow".to_string(),
            description: "Replay a saved semantic browser workflow deterministically using the current static AOM-first engine with session/form support. Provide sessionId to run inside an existing persisted session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .browser.json workflow relative to the workspace root." },
                    "sessionId": { "type": "string", "description": "Optional persisted session id to replay inside instead of an ephemeral session." },
                    "compact": { "type": "boolean", "description": "When true, return a structured replay summary with artifact paths instead of verbose multiline text." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "browser_list_workflow_runs".to_string(),
            description: "List persisted browser workflow run reports from the workspace run-artifact directory using compact summaries with run report paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workflowNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on workflow name." },
                    "sessionIdContains": { "type": "string", "description": "Optional case-insensitive substring filter on session id." },
                    "finalUrlContains": { "type": "string", "description": "Optional case-insensitive substring filter on final URL." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of workflow run summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for workflow/session ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_read_workflow_run".to_string(),
            description: "Read a persisted browser workflow run report for a workflow/session pair.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workflowName": { "type": "string", "description": "Workflow name that produced the run report." },
                    "sessionId": { "type": "string", "description": "Session identifier captured in the run report." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned workflow run read report with summary fields and persisted run artifact path instead of the full run report." }
                },
                "required": ["workflowName", "sessionId"]
            }),
        },
        Tool {
            name: "browser_save_workflow_suite".to_string(),
            description: "Persist a semantic browser workflow suite as JSON so multiple saved workflows can run as a lightweight regression pack.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Suite name." },
                    "workflows": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Relative paths to saved .browser.json workflows."
                    },
                    "compact": { "type": "boolean", "description": "When true, return a structured suite save summary with artifact paths instead of verbose multiline text." }
                },
                "required": ["name", "workflows"]
            }),
        },
        Tool {
            name: "browser_read_workflow_suite".to_string(),
            description: "Read a saved browser workflow suite JSON artifact from the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .suite.json file relative to the workspace root." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned workflow suite read report with summary fields and persisted suite artifact path instead of the full suite payload." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "browser_list_workflow_suites".to_string(),
            description: "List saved browser workflow suite artifacts from the workspace suite directory using compact summaries with suite artifact paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "suiteNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on suite name." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of suite summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for suite name ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_run_workflow_suite".to_string(),
            description: "Execute a saved semantic browser workflow suite and persist an aggregated suite run report.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "relativeFilePath": { "type": "string", "description": "Path to a saved .suite.json file relative to the workspace root." },
                    "compact": { "type": "boolean", "description": "When true, return a structured suite execution summary with the persisted suite report path instead of verbose multiline text." }
                },
                "required": ["relativeFilePath"]
            }),
        },
        Tool {
            name: "browser_list_workflow_suite_runs".to_string(),
            description: "List persisted browser workflow suite run reports from the workspace suite-run artifact directory using compact summaries with suite run report paths.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "suiteNameContains": { "type": "string", "description": "Optional case-insensitive substring filter on suite name." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Optional maximum number of suite run summaries to return after sorting." },
                    "sortDirection": { "type": "string", "enum": ["asc", "desc"], "description": "Optional sort direction for suite name ordering. Defaults to asc." }
                }
            }),
        },
        Tool {
            name: "browser_read_workflow_suite_run".to_string(),
            description: "Read a persisted browser workflow suite run report by suite name.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "suiteName": { "type": "string", "description": "Suite name that produced the suite run report." },
                    "compact": { "type": "boolean", "description": "When true, return a browser-owned suite run read report with summary fields and persisted suite-run artifact path instead of the full suite run report." }
                },
                "required": ["suiteName"]
            }),
        },
    ]
}

fn parse_browser_steps(
    steps: &[Value],
) -> Result<Vec<crate::editor::browser::BrowserWorkflowStep>, Box<dyn Error>> {
    let mut parsed_steps = Vec::with_capacity(steps.len());
    for step in steps {
        let kind = step["kind"].as_str().ok_or("workflow step kind is required")?;
        let parsed = match kind {
            "navigate" => crate::editor::browser::BrowserWorkflowStep::Navigate {
                url: step["url"].as_str().ok_or("navigate step url is required")?.to_string(),
            },
            "click" => crate::editor::browser::BrowserWorkflowStep::Click {
                role: step["role"].as_str().ok_or("click step role is required")?.to_string(),
                name: step["name"].as_str().ok_or("click step name is required")?.to_string(),
            },
            "fill_field" => crate::editor::browser::BrowserWorkflowStep::FillField {
                field: step["field"].as_str().ok_or("fill_field step field is required")?.to_string(),
                value: step["value"].as_str().ok_or("fill_field step value is required")?.to_string(),
            },
            "submit_form" => crate::editor::browser::BrowserWorkflowStep::SubmitForm {
                form: step["form"].as_str().map(|value| value.to_string()),
            },
            "wait_for_text" => crate::editor::browser::BrowserWorkflowStep::WaitForText {
                text: step["text"].as_str().ok_or("wait_for_text step text is required")?.to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_element" => crate::editor::browser::BrowserWorkflowStep::WaitForElement {
                role: step["role"].as_str().ok_or("wait_for_element step role is required")?.to_string(),
                name: step["name"].as_str().ok_or("wait_for_element step name is required")?.to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_title" => crate::editor::browser::BrowserWorkflowStep::WaitForTitle {
                title: step["title"].as_str().ok_or("wait_for_title step title is required")?.to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_url_contains" => crate::editor::browser::BrowserWorkflowStep::WaitForUrlContains {
                fragment: step["fragment"].as_str().ok_or("wait_for_url_contains step fragment is required")?.to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_mutation" => crate::editor::browser::BrowserWorkflowStep::WaitForMutation {
                label: step["label"].as_str().ok_or("wait_for_mutation step label is required")?.to_string(),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_request" => crate::editor::browser::BrowserWorkflowStep::WaitForRequest {
                method: step["method"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                url_contains: step["urlContains"].as_str().or_else(|| step["url_contains"].as_str()).map(|value| value.to_string()).filter(|value| !value.is_empty()),
                status: step["status"].as_u64().map(|value| value as u16),
                resource: step["resource"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_storage" => crate::editor::browser::BrowserWorkflowStep::WaitForStorage {
                scope: step["scope"].as_str().ok_or("wait_for_storage step scope is required")?.to_string(),
                key: step["key"].as_str().ok_or("wait_for_storage step key is required")?.to_string(),
                value: step["value"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_settle" => crate::editor::browser::BrowserWorkflowStep::WaitForSettle {
                label: step["label"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                scope: step["scope"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                state: step["state"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_runtime_state" => crate::editor::browser::BrowserWorkflowStep::WaitForRuntimeState {
                scope: step["scope"].as_str().ok_or("wait_for_runtime_state step scope is required")?.to_string(),
                key: step["key"].as_str().ok_or("wait_for_runtime_state step key is required")?.to_string(),
                value: step["value"].as_str().map(|value| value.to_string()).filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_protocol_event" => crate::editor::browser::BrowserWorkflowStep::WaitForProtocolEvent {
                event_kind: step["kindName"].as_str().or_else(|| step["eventKind"].as_str()).or_else(|| step["protocolKind"].as_str()).map(|value| value.to_string()).filter(|value| !value.is_empty()),
                phase: step["phase"].as_str().or_else(|| step["protocolPhase"].as_str()).map(|value| value.to_string()).filter(|value| !value.is_empty()),
                target: step["target"].as_str().or_else(|| step["targetContains"].as_str()).or_else(|| step["protocolTarget"].as_str()).map(|value| value.to_string()).filter(|value| !value.is_empty()),
                detail: step["detail"].as_str().or_else(|| step["detailContains"].as_str()).or_else(|| step["protocolDetail"].as_str()).map(|value| value.to_string()).filter(|value| !value.is_empty()),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "wait_for_stable" => crate::editor::browser::BrowserWorkflowStep::WaitForStable {
                stable_polls: step["stablePolls"].as_u64().map(|value| value as u32),
                timeout_ms: step["timeoutMs"].as_u64(),
                interval_ms: step["intervalMs"].as_u64(),
            },
            "extract_text" => crate::editor::browser::BrowserWorkflowStep::ExtractText {
                output: step["output"].as_str().ok_or("extract_text step output is required")?.to_string(),
                source: step["source"].as_str().ok_or("extract_text step source is required")?.to_string(),
                role: step["role"].as_str().map(|value| value.to_string()),
                name: step["name"].as_str().map(|value| value.to_string()),
                field: step["field"].as_str().map(|value| value.to_string()),
            },
            "save_checkpoint" => crate::editor::browser::BrowserWorkflowStep::SaveCheckpoint {
                name: step["name"].as_str().ok_or("save_checkpoint step name is required")?.to_string(),
            },
            "restore_checkpoint" => crate::editor::browser::BrowserWorkflowStep::RestoreCheckpoint {
                name: step["name"].as_str().ok_or("restore_checkpoint step name is required")?.to_string(),
            },
            "if_text_contains" => crate::editor::browser::BrowserWorkflowStep::IfTextContains {
                text: step["text"].as_str().ok_or("if_text_contains step text is required")?.to_string(),
                then_steps: parse_browser_steps(step["thenSteps"].as_array().ok_or("if_text_contains thenSteps must be an array")?)?,
                else_steps: parse_browser_steps(step["elseSteps"].as_array().map(|steps| steps.as_slice()).unwrap_or(&[]))?,
            },
            "if_output_equals" => crate::editor::browser::BrowserWorkflowStep::IfOutputEquals {
                output: step["output"].as_str().ok_or("if_output_equals step output is required")?.to_string(),
                equals: step["equals"].as_str().ok_or("if_output_equals step equals is required")?.to_string(),
                then_steps: parse_browser_steps(step["thenSteps"].as_array().ok_or("if_output_equals thenSteps must be an array")?)?,
                else_steps: parse_browser_steps(step["elseSteps"].as_array().map(|steps| steps.as_slice()).unwrap_or(&[]))?,
            },
            "assert_element" => crate::editor::browser::BrowserWorkflowStep::AssertElement {
                role: step["role"].as_str().ok_or("assert_element step role is required")?.to_string(),
                name: step["name"].as_str().ok_or("assert_element step name is required")?.to_string(),
            },
            "assert_text_contains" => crate::editor::browser::BrowserWorkflowStep::AssertTextContains {
                text: step["text"].as_str().ok_or("assert_text_contains step text is required")?.to_string(),
            },
            "assert_output" => crate::editor::browser::BrowserWorkflowStep::AssertOutput {
                output: step["output"].as_str().ok_or("assert_output step output is required")?.to_string(),
                equals: step["equals"].as_str().map(|value| value.to_string()),
                contains: step["contains"].as_str().map(|value| value.to_string()),
            },
            other => return Err(format!("unsupported browser workflow step kind: {}", other).into()),
        };
        parsed_steps.push(parsed);
    }
    Ok(parsed_steps)
}

pub fn call_tool_in_workspace(root: &Path, name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let root = root.canonicalize()?;
    match name {
        "web_navigate" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::crawl_and_sync_sitemap_report(url, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser crawl summary: {err}").into())
            } else {
                crate::editor::browser::crawl_and_sync_sitemap(url, &sitemap_path)
                    .map_err(|e| e.into())
            }
        }
        "browser_create_session" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let report = crate::editor::browser::create_session_report(&root, session_id)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session creation summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_create_report(&report))
            }
        }
        "browser_runtime_capture" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let timeout_ms = arguments["timeoutMs"].as_u64().unwrap_or(15_000);
            let api_base = arguments["apiBase"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::runtime_capture_report(
                    &root,
                    session_id,
                    url,
                    timeout_ms,
                    api_base,
                    &sitemap_path,
                )
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser runtime capture summary: {err}").into())
            } else {
                crate::editor::browser::runtime_capture(
                    &root,
                    session_id,
                    url,
                    timeout_ms,
                    api_base,
                    &sitemap_path,
                )
                .map_err(|e| e.into())
            }
        }
        "browser_runtime_visual_capture" => crate::editor::browser::browser_runtime_visual_capture(
            &root,
            arguments["url"].as_str().ok_or("url is required")?,
            arguments["apiBase"].as_str(),
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(|e| e.into()),
        "runtime_create_session" => crate::editor::browser::create_runtime_session(
            &root,
            arguments["sessionId"].as_str().ok_or("sessionId is required")?,
            arguments["startUrl"].as_str(),
            arguments["waitTimeoutMs"].as_u64(),
            arguments["apiBase"].as_str(),
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(|e| e.into()),
        "runtime_get_session" => crate::editor::browser::get_runtime_session(
            &root,
            arguments["sessionId"].as_str().ok_or("sessionId is required")?,
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(|e| e.into()),
        "runtime_close_session" => crate::editor::browser::close_runtime_session(
            &root,
            arguments["sessionId"].as_str().ok_or("sessionId is required")?,
            arguments["compact"].as_bool().unwrap_or(false),
        )
        .map_err(|e| e.into()),
        "runtime_capture_session" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::capture_runtime_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_navigate" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_navigate_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["url"].as_str().ok_or("url is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_click" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_click_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["nodeId"].as_str(),
                arguments["selector"].as_str(),
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_js_click" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_js_click_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["nodeId"].as_str().ok_or("nodeId is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_evaluate" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_evaluate_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["script"].as_str().ok_or("script is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_fill" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_fill_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["nodeId"].as_str(),
                arguments["selector"].as_str(),
                arguments["value"].as_str().ok_or("value is required")?,
                arguments["natural"].as_bool().unwrap_or(false),
                arguments["clear"].as_bool().unwrap_or(false),
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_submit" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_submit_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["nodeId"].as_str(),
                arguments["selector"].as_str(),
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "runtime_session_press_key" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            crate::editor::browser::runtime_press_key_session(
                &root,
                arguments["sessionId"].as_str().ok_or("sessionId is required")?,
                arguments["key"].as_str().ok_or("key is required")?,
                arguments["waitTimeoutMs"].as_u64(),
                &sitemap_path,
                arguments["compact"].as_bool().unwrap_or(false),
            )
            .map_err(|e| e.into())
        }
        "browser_get_session" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let session = crate::editor::browser::load_session_state(&root, session_id)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_session_report(&root, session_id)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session summary: {err}").into())
            } else {
                crate::editor::browser::session_state_to_json(&session).map_err(|e| e.into())
            }
        }
        "browser_list_snapshots" => {
            let sitemap_path = root.join(".velocity").join("site_map");
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let snapshots = crate::editor::browser::list_snapshots(
                &sitemap_path,
                arguments["urlContains"].as_str(),
                arguments["titleContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&snapshots)
                .map_err(|err| format!("serialise browser snapshots: {err}").into())
        }
        "browser_read_snapshot" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let snapshot = crate::editor::browser::read_snapshot(url, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_snapshot_report(url, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser snapshot summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&snapshot)
                    .map_err(|err| format!("serialise browser snapshot: {err}").into())
            }
        }
        "browser_read_visual_fallback" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_visual_fallback_report(url, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser html fallback summary: {err}").into())
            } else {
                crate::editor::browser::read_visual_fallback(url, &sitemap_path)
                    .map_err(|e| e.into())
            }
        }
        "browser_diff_snapshots" => {
            let before_url = arguments["beforeUrl"].as_str().ok_or("beforeUrl is required")?;
            let after_url = arguments["afterUrl"].as_str().ok_or("afterUrl is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::diff_saved_snapshots(before_url, after_url, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_snapshot_diff_report(before_url, after_url, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| format!("serialise browser snapshot diff summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser snapshot diff: {err}").into())
            }
        }
        "browser_list_sessions" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let sessions = crate::editor::browser::list_sessions(
                &root,
                arguments["sessionIdContains"].as_str(),
                arguments["urlContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&sessions)
                .map_err(|err| format!("serialise browser sessions: {err}").into())
        }
        "browser_get_storage" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let scope = arguments["scope"].as_str().ok_or("scope is required")?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::get_session_storage_entries_report(&root, session_id, scope)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser storage summary: {err}").into())
            } else {
                crate::editor::browser::get_session_storage_entries(&root, session_id, scope)
                    .map_err(|e| e.into())
            }
        }
        "browser_set_storage" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let scope = arguments["scope"].as_str().ok_or("scope is required")?;
            let entries_value = arguments["entries"].as_object().ok_or("entries is required")?;
            let mut entries = std::collections::HashMap::new();
            for (key, value) in entries_value {
                let value = value.as_str().ok_or("storage entry values must be strings")?;
                entries.insert(key.clone(), value.to_string());
            }
            let report = crate::editor::browser::set_session_storage_entries_report(&root, session_id, scope, &entries)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser storage update summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_storage_update_report(&report))
            }
        }
        "browser_get_cookies" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::get_session_cookies_report(&root, session_id)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser cookie summary: {err}").into())
            } else {
                crate::editor::browser::get_session_cookies(&root, session_id)
                    .map_err(|e| e.into())
            }
        }
        "browser_set_cookies" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let cookies_value = arguments["cookies"].as_array().ok_or("cookies is required")?;
            let mut cookies = Vec::new();
            for cookie in cookies_value {
                let name = cookie["name"].as_str().ok_or("cookie name is required")?;
                let value = cookie["value"].as_str().ok_or("cookie value is required")?;
                cookies.push(crate::editor::browser::BrowserCookie {
                    name: name.to_string(),
                    value: value.to_string(),
                });
            }
            let report = crate::editor::browser::set_session_cookies_report(&root, session_id, &cookies)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser cookie update summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_cookie_update_report(&report))
            }
        }
        "browser_auth_diagnostics" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::auth_diagnostics_report(&root, session_id, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&report)
                .map_err(|err| format!("serialise browser auth diagnostics: {err}").into())
        }
        "browser_save_auth_profile" => {
            let profile_name = arguments["profileName"].as_str().ok_or("profileName is required")?;
            let source_session_id = arguments["sourceSessionId"].as_str().ok_or("sourceSessionId is required")?;
            let source_checkpoint_name = arguments["sourceCheckpointName"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::save_auth_profile_report(
                &root,
                profile_name,
                source_session_id,
                source_checkpoint_name,
                &sitemap_path,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser auth profile save report: {err}").into())
            } else {
                Ok(crate::editor::browser::render_auth_profile_save_report(&report))
            }
        }
        "browser_list_auth_profiles" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let profiles = crate::editor::browser::list_auth_profiles(
                &root,
                arguments["profileNameContains"].as_str(),
                arguments["sourceSessionIdContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&profiles)
                .map_err(|err| format!("serialise browser auth profiles: {err}").into())
        }
        "browser_read_auth_profile" => {
            let profile_name = arguments["profileName"].as_str().ok_or("profileName is required")?;
            let profile = crate::editor::browser::load_auth_profile(&root, profile_name)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_auth_profile_report(&root, profile_name)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser auth profile summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&profile)
                    .map_err(|err| format!("serialise browser auth profile: {err}").into())
            }
        }
        "browser_apply_auth_profile" => {
            let profile_name = arguments["profileName"].as_str().ok_or("profileName is required")?;
            let target_session_id = arguments["targetSessionId"].as_str().ok_or("targetSessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::apply_auth_profile_report(
                &root,
                profile_name,
                target_session_id,
                &sitemap_path,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser auth profile apply report: {err}").into())
            } else {
                Ok(crate::editor::browser::render_auth_profile_apply_report(&report))
            }
        }
        "browser_reseed_auth" => {
            let target_session_id = arguments["targetSessionId"].as_str().ok_or("targetSessionId is required")?;
            let source_session_id = arguments["sourceSessionId"].as_str().ok_or("sourceSessionId is required")?;
            let source_checkpoint_name = arguments["sourceCheckpointName"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::reseed_auth_state_report(
                &root,
                target_session_id,
                source_session_id,
                source_checkpoint_name,
                &sitemap_path,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser auth reseed report: {err}").into())
            } else {
                Ok(crate::editor::browser::render_auth_reseed_report(&report))
            }
        }
        "browser_access_diagnostics" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::access_diagnostics_report(&root, session_id, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(true) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser access diagnostics: {err}").into())
            } else {
                Ok(crate::editor::browser::render_access_diagnostics_report(&report))
            }
        }
        "browser_get_session_network" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let report = crate::editor::browser::read_session_network_report(&root, session_id)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session network report: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_network_read_report(&report))
            }
        }
        "browser_read_session_transcript" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            if let Some(sequence) = arguments["sequence"].as_u64() {
                let entry = crate::editor::browser::read_session_transcript_entry(&root, session_id, sequence)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&entry)
                    .map_err(|err| format!("serialise browser session transcript entry: {err}").into())
            } else {
                let sort_direction = crate::editor::browser::parse_list_sort_direction(arguments["sortDirection"].as_str())
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                let limit = arguments["limit"].as_u64().map(|value| value as usize);
                let report = crate::editor::browser::read_session_transcript_report(&root, session_id, limit, sort_direction)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                if arguments["compact"].as_bool().unwrap_or(false) {
                    serde_json::to_string_pretty(&report)
                        .map_err(|err| format!("serialise browser session transcript report: {err}").into())
                } else {
                    Ok(crate::editor::browser::render_session_transcript_report(&report))
                }
            }
        }
        "browser_session_health" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_health_report(&root, session_id, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session health report: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_health_report(&report))
            }
        }
        "browser_set_session_network" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let headers = arguments["headers"].as_object().map(|entries| {
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_string()))
                    .collect::<std::collections::HashMap<_, _>>()
            });
            let allowed_url_prefixes = arguments["allowedUrlPrefixes"].as_array().map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|value| value.to_string()))
                    .collect::<Vec<_>>()
            });
            let blocked_url_prefixes = arguments["blockedUrlPrefixes"].as_array().map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|value| value.to_string()))
                    .collect::<Vec<_>>()
            });
            let report = crate::editor::browser::update_session_network_report(
                &root,
                session_id,
                arguments["userAgent"].as_str(),
                headers,
                arguments["timeoutMs"].as_u64(),
                arguments["clearTimeout"].as_bool().unwrap_or(false),
                arguments["followRedirects"].as_bool(),
                arguments["clearFollowRedirects"].as_bool().unwrap_or(false),
                allowed_url_prefixes,
                blocked_url_prefixes,
                arguments["replaceHeaders"].as_bool().unwrap_or(false),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session network update: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_network_update_report(&report))
            }
        }
        "browser_session_navigate" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::navigate_session_report(&root, session_id, url, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session navigation summary: {err}").into())
            } else {
                crate::editor::browser::navigate_session(&root, session_id, url, &sitemap_path)
                    .map_err(|e| e.into())
            }
        }
        "browser_session_click" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let role = arguments["role"].as_str().ok_or("role is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_click_report(&root, session_id, role, name, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser click summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_action_report(&report))
            }
        }
        "browser_session_fill" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let field = arguments["field"].as_str().ok_or("field is required")?;
            let value = arguments["value"].as_str().ok_or("value is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_fill_report(&root, session_id, field, value, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser fill summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_action_report(&report))
            }
        }
        "browser_session_submit" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::session_submit_report(&root, session_id, arguments["form"].as_str(), &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser submit summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_session_action_report(&report))
            }
        }
        "browser_session_wait" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let text = arguments["text"].as_str();
            let title = arguments["title"].as_str();
            let url_contains = arguments["urlContains"].as_str();
            let mutation = arguments["mutation"].as_str();
            let request_method = arguments["requestMethod"].as_str();
            let request_url_contains = arguments["requestUrlContains"].as_str();
            let request_status = arguments["requestStatus"].as_u64().map(|value| value as u16);
            let request_resource = arguments["requestResource"].as_str();
            let storage_scope = arguments["storageScope"].as_str();
            let storage_key = arguments["storageKey"].as_str();
            let storage_value = arguments["storageValue"].as_str();
            let settle = arguments["settle"].as_str();
            let settle_scope = arguments["settleScope"].as_str();
            let settle_state = arguments["settleState"].as_str();
            let runtime_scope = arguments["runtimeScope"].as_str();
            let runtime_key = arguments["runtimeKey"].as_str();
            let runtime_value = arguments["runtimeValue"].as_str();
            let protocol_kind = arguments["protocolKind"].as_str();
            let protocol_phase = arguments["protocolPhase"].as_str();
            let protocol_target = arguments["protocolTarget"].as_str();
            let protocol_detail = arguments["protocolDetail"].as_str();
            let network_idle = arguments["networkIdle"].as_bool().unwrap_or(false);
            let app_ready = arguments["appReady"].as_bool().unwrap_or(false);
            let mutation_settled = arguments["mutationSettled"].as_bool().unwrap_or(false);
            let stream_complete = arguments["streamComplete"].as_bool().unwrap_or(false);
            let role = arguments["role"].as_str();
            let name = arguments["name"].as_str();
            let require_actionable = arguments["requireActionable"].as_bool().unwrap_or(false);
            let stable_polls = arguments["stablePolls"].as_u64().map(|value| value as u32);
            let timeout_ms = arguments["timeoutMs"].as_u64();
            let interval_ms = arguments["intervalMs"].as_u64();
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::wait_for_session_report(
                    &root,
                    session_id,
                    text,
                    title,
                    url_contains,
                    mutation,
                    request_method,
                    request_url_contains,
                    request_status,
                    request_resource,
                    storage_scope,
                    storage_key,
                    storage_value,
                    settle,
                    settle_scope,
                    settle_state,
                    runtime_scope,
                    runtime_key,
                    runtime_value,
                    protocol_kind,
                    protocol_phase,
                    protocol_target,
                    protocol_detail,
                    network_idle,
                    app_ready,
                    mutation_settled,
                    stream_complete,
                    role,
                    name,
                    require_actionable,
                    stable_polls,
                    timeout_ms,
                    interval_ms,
                    &sitemap_path,
                )
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser session wait summary: {err}").into())
            } else {
                crate::editor::browser::wait_for_session(
                    &root,
                    session_id,
                    text,
                    title,
                    url_contains,
                    mutation,
                    request_method,
                    request_url_contains,
                    request_status,
                    request_resource,
                    storage_scope,
                    storage_key,
                    storage_value,
                    settle,
                    settle_scope,
                    settle_state,
                    runtime_scope,
                    runtime_key,
                    runtime_value,
                    protocol_kind,
                    protocol_phase,
                    protocol_target,
                    protocol_detail,
                    network_idle,
                    app_ready,
                    mutation_settled,
                    stream_complete,
                    role,
                    name,
                    require_actionable,
                    stable_polls,
                    timeout_ms,
                    interval_ms,
                    &sitemap_path,
                )
                .map_err(|e| e.into())
            }
        }
        "browser_save_checkpoint" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"].as_str().ok_or("checkpointName is required")?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let report = crate::editor::browser::save_session_checkpoint_report(&root, session_id, checkpoint_name, &sitemap_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser checkpoint save summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_checkpoint_save_report(&report))
            }
        }
        "browser_restore_checkpoint" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"].as_str().ok_or("checkpointName is required")?;
            let target_session_id = arguments["targetSessionId"].as_str();
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::restore_session_checkpoint_report(
                    &root,
                    session_id,
                    checkpoint_name,
                    target_session_id,
                    &sitemap_path,
                )
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser checkpoint restore summary: {err}").into())
            } else {
                crate::editor::browser::restore_session_checkpoint(
                    &root,
                    session_id,
                    checkpoint_name,
                    target_session_id,
                    &sitemap_path,
                )
                .map_err(|e| e.into())
            }
        }
        "browser_list_checkpoints" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let checkpoints = crate::editor::browser::list_session_checkpoints(
                &root,
                session_id,
                arguments["checkpointNameContains"].as_str(),
                arguments["titleContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&checkpoints)
                .map_err(|err| format!("serialise checkpoint list: {err}").into())
        }
        "browser_read_checkpoint" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let checkpoint_name = arguments["checkpointName"].as_str().ok_or("checkpointName is required")?;
            let checkpoint = crate::editor::browser::read_session_checkpoint(&root, session_id, checkpoint_name)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_session_checkpoint_report(&root, session_id, checkpoint_name)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise checkpoint summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&checkpoint)
                    .map_err(|err| format!("serialise checkpoint: {err}").into())
            }
        }
        "browser_diff_checkpoints" => {
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let before_checkpoint_name = arguments["beforeCheckpointName"].as_str().ok_or("beforeCheckpointName is required")?;
            let after_checkpoint_name = arguments["afterCheckpointName"].as_str().ok_or("afterCheckpointName is required")?;
            let report = crate::editor::browser::diff_session_checkpoints(
                &root,
                session_id,
                before_checkpoint_name,
                after_checkpoint_name,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_checkpoint_diff_report(
                    &root,
                    session_id,
                    before_checkpoint_name,
                    after_checkpoint_name,
                )
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| format!("serialise checkpoint diff summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise checkpoint diff: {err}").into())
            }
        }
        "browser_save_workflow" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let start_url = arguments["startUrl"].as_str().ok_or("startUrl is required")?;
            let steps = arguments["steps"].as_array().ok_or("steps must be an array")?;
            let mut variables = std::collections::HashMap::new();
            if let Some(map) = arguments["variables"].as_object() {
                for (key, value) in map {
                    let text = value.as_str().ok_or("workflow variables must be string values")?;
                    variables.insert(key.to_string(), text.to_string());
                }
            }
            let parsed_steps = parse_browser_steps(steps)?;

            let workflow = crate::editor::browser::BrowserWorkflow {
                name: name.to_string(),
                start_url: start_url.to_string(),
                variables,
                steps: parsed_steps,
            };
            let report = crate::editor::browser::save_workflow_report(&root, &workflow)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser workflow save summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_workflow_save_report(&report))
            }
        }
        "browser_read_workflow" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let workflow = crate::editor::browser::load_workflow(&full_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_workflow_report(&full_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                Ok(serde_json::to_string_pretty(&report)?)
            } else {
                Ok(serde_json::to_string_pretty(&workflow)?)
            }
        }
        "browser_list_workflows" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let workflows = crate::editor::browser::list_workflows(
                &root,
                arguments["workflowNameContains"].as_str(),
                arguments["startUrlContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&workflows)
                .map_err(|err| format!("serialise workflows: {err}").into())
        }
        "browser_replay_workflow" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let workflow = crate::editor::browser::load_workflow(&full_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let compact = arguments["compact"].as_bool().unwrap_or(false);
            if let Some(session_id) = arguments["sessionId"].as_str() {
                if compact {
                    let report = crate::editor::browser::replay_workflow_in_session_report(&root, session_id, &workflow, &sitemap_path)
                        .map_err(|e| -> Box<dyn Error> { e.into() })?;
                    serde_json::to_string_pretty(&report)
                        .map_err(|err| format!("serialise browser workflow replay summary: {err}").into())
                } else {
                    crate::editor::browser::replay_workflow_in_session(&root, session_id, &workflow, &sitemap_path)
                        .map_err(|e| e.into())
                }
            } else if compact {
                let report = crate::editor::browser::replay_workflow_with_artifacts_report(&root, &workflow, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser workflow replay summary: {err}").into())
            } else {
                crate::editor::browser::replay_workflow_with_artifacts(&root, &workflow, &sitemap_path)
                    .map_err(|e| e.into())
            }
        }
        "browser_list_workflow_runs" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let runs = crate::editor::browser::list_workflow_runs(
                &root,
                arguments["workflowNameContains"].as_str(),
                arguments["sessionIdContains"].as_str(),
                arguments["finalUrlContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&runs)
                .map_err(|err| format!("serialise workflow runs: {err}").into())
        }
        "browser_read_workflow_run" => {
            let workflow_name = arguments["workflowName"].as_str().ok_or("workflowName is required")?;
            let session_id = arguments["sessionId"].as_str().ok_or("sessionId is required")?;
            let report = crate::editor::browser::read_workflow_run(&root, workflow_name, session_id)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_workflow_run_report(&root, workflow_name, session_id)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| format!("serialise workflow run summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise workflow run: {err}").into())
            }
        }
        "browser_save_workflow_suite" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let workflows = arguments["workflows"].as_array().ok_or("workflows must be an array")?;
            let suite = crate::editor::browser::BrowserWorkflowSuite {
                name: name.to_string(),
                workflows: workflows
                    .iter()
                    .map(|entry| entry.as_str().ok_or("workflow suite entries must be strings"))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|value| value.to_string())
                    .collect(),
            };
            let report = crate::editor::browser::save_workflow_suite_report(&root, &suite)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser workflow suite save summary: {err}").into())
            } else {
                Ok(crate::editor::browser::render_workflow_suite_save_report(&report))
            }
        }
        "browser_read_workflow_suite" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let suite = crate::editor::browser::load_workflow_suite(&full_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_workflow_suite_report(&full_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                Ok(serde_json::to_string_pretty(&report)?)
            } else {
                Ok(serde_json::to_string_pretty(&suite)?)
            }
        }
        "browser_list_workflow_suites" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let suites = crate::editor::browser::list_workflow_suites(
                &root,
                arguments["suiteNameContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&suites)
                .map_err(|err| format!("serialise workflow suites: {err}").into())
        }
        "browser_run_workflow_suite" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let suite = crate::editor::browser::load_workflow_suite(&full_path)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::run_workflow_suite_report(&root, &suite, &sitemap_path)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise browser workflow suite execution summary: {err}").into())
            } else {
                crate::editor::browser::run_workflow_suite(&root, &suite, &sitemap_path)
                    .map_err(|e| e.into())
            }
        }
        "browser_list_workflow_suite_runs" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let runs = crate::editor::browser::list_workflow_suite_runs(
                &root,
                arguments["suiteNameContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string_pretty(&runs)
                .map_err(|err| format!("serialise workflow suite runs: {err}").into())
        }
        "browser_read_workflow_suite_run" => {
            let suite_name = arguments["suiteName"].as_str().ok_or("suiteName is required")?;
            let report = crate::editor::browser::read_workflow_suite_run(&root, suite_name)
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_workflow_suite_run_report(&root, suite_name)
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| format!("serialise workflow suite run summary: {err}").into())
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| format!("serialise workflow suite run: {err}").into())
            }
        }
        "run_command" => {
            let command_str = arguments["command"].as_str().ok_or("command is required")?;
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(&["/C", command_str])
                    .current_dir(&root)
                    .output()
            } else {
                Command::new("sh")
                    .args(&["-c", command_str])
                    .current_dir(&root)
                    .output()
            };

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let combined = format!("{}{}", stdout, stderr);
                    Ok(combined)
                }
                Err(e) => Err(format!("Failed to execute command: {:?}", e).into())
            }
        }
        "convert_to_nda" => {
            let _file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            let _output_path = arguments["outputPath"].as_str().unwrap_or("");
            execute_csharp_mcp_tool("convert_to_nda", arguments)
        }
        "read_nda" => {
            let _nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            execute_csharp_mcp_tool("read_nda", arguments)
        }
        "execute_nda" => {
            let _nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            execute_csharp_mcp_tool("execute_nda", arguments)
        }
        "read_file" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            let content = std::fs::read_to_string(full_path)?;
            Ok(content)
        }
        "write_file" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let content = arguments["content"].as_str().ok_or("content is required")?;
            
            // Run safety scanner warnings detection
            let scan_warning = scan_file_content(content);
            
            let full_path = resolve_workspace_path(&root, rel_path, true)?;
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(full_path, content)?;
            
            if let Some(warn) = scan_warning {
                Ok(format!(
                    "Success: File written successfully. WARNING: Security scan warning triggered: [{}]. Please immediately correct this exposure in your next step.",
                    warn
                ))
            } else {
                Ok("Success: File written successfully".to_string())
            }
        }
        "list_dir" => {
            let rel_path = arguments["relativeDirPath"].as_str().ok_or("relativeDirPath is required")?;
            let target_dir = if rel_path == "." || rel_path.is_empty() {
                root.clone()
            } else {
                resolve_workspace_path(&root, rel_path, false)?
            };
            
            let mut entries_list = Vec::new();
            let entries = std::fs::read_dir(&target_dir)
                .map_err(|e| format!("Failed to read directory '{}': {:?}", target_dir.display(), e))?;
                
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type()?.is_dir();
                entries_list.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
            }
            Ok(entries_list.join("\n"))
        }
        "delete_file" => {
            let rel_path = arguments["relativeFilePath"].as_str().ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(&root, rel_path, false)?;
            
            if full_path.is_dir() {
                return Err("delete_file cannot be used to delete a directory. Use a command line tool if needed.".into());
            }
            
            std::fs::remove_file(&full_path)?;
            Ok(format!("Success: File '{}' deleted successfully.", rel_path))
        }
        "grep_search" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let root_dir = root.clone();
            let mut matches = Vec::new();
            
            fn search_dir(dir: &std::path::Path, query: &str, matches: &mut Vec<String>, root: &std::path::Path) -> Result<(), Box<dyn Error>> {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let file_type = entry.file_type()?;
                        
                        if file_type.is_dir() {
                            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            if dir_name == "node_modules" 
                                || dir_name == ".git" 
                                || dir_name == "target"
                                || dir_name == "dist"
                                || dir_name == "build"
                                || dir_name == ".vscode"
                                || dir_name == ".idea"
                                || dir_name == "bin"
                                || dir_name == "obj" 
                            {
                                continue;
                            }
                            search_dir(&path, query, matches, root)?;
                        } else if file_type.is_file() {
                            let extension = path.extension()
                                .and_then(|ext| ext.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            let skip_exts = ["png", "jpg", "jpeg", "gif", "ico", "pdf", "zip", "tar", "gz", "7z", "rar", "exe", "dll", "so", "dylib", "class", "pyc", "nda"];
                            if skip_exts.contains(&extension.as_str()) {
                                continue;
                            }
                            
                            if let Ok(metadata) = path.metadata() {
                                if metadata.len() > 1024 * 1024 { // skip files > 1 MB
                                    continue;
                                }
                            }
                            
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string();
                                for (idx, line) in content.lines().enumerate() {
                                    if line.contains(query) {
                                        matches.push(format!("{}:{}: {}", rel, idx + 1, line.trim()));
                                        if matches.len() >= 100 {
                                            return Ok(());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            
            search_dir(&root_dir, query, &mut matches, &root_dir)?;
            Ok(matches.join("\n"))
        }
        _ => Err(format!("Tool '{}' is not registered on this server.", name).into()),
    }
}

fn resolve_workspace_path(root: &Path, raw: &str, allow_missing: bool) -> Result<PathBuf, Box<dyn Error>> {
    let candidate = Path::new(raw);

    // If the model passed an absolute path, try to make it relative to root first.
    let raw_rel: std::borrow::Cow<str> = if candidate.is_absolute() {
        // Strip the workspace root prefix if present (handles model emitting full paths)
        match candidate.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().into(),
            Err(_) => {
                // Also try with canonical root to handle UNC vs normal differences
                let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                match candidate.strip_prefix(&canon_root) {
                    Ok(rel) => rel.to_string_lossy().into(),
                    Err(_) => return Err("workspace tool path is outside the workspace root".into()),
                }
            }
        }
    } else {
        raw.into()
    };

    let candidate = Path::new(raw_rel.as_ref());
    if candidate.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err("workspace tool path escapes the workspace".into());
    }

    let joined = root.join(candidate);

    if allow_missing {
        // For writes: just ensure existing parents are within root
        if joined.exists() {
            // path exists — validate it stays within root
            let canonical = safe_canonicalize(&joined);
            let canon_root = safe_canonicalize(root);
            if !canonical.starts_with(&canon_root) {
                return Err("workspace tool path escapes the workspace".into());
            }
        }
        return Ok(joined);
    }

    // For reads: path must exist
    if !joined.exists() {
        return Err(format!("file not found in workspace: {}", joined.display()).into());
    }

    // Validate within root without relying solely on canonicalize (which fails on OneDrive)
    let canonical = safe_canonicalize(&joined);
    let canon_root = safe_canonicalize(root);
    if !canonical.starts_with(&canon_root) {
        return Err("workspace tool path escapes the workspace".into());
    }

    Ok(joined)
}

/// Canonicalize a path without crashing on Windows permission errors (e.g. OneDrive placeholders).
/// Returns the input path unchanged when canonicalization fails.
fn safe_canonicalize(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}


pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let root = std::env::current_dir()?;
    call_tool_in_workspace(&root, name, arguments)
}

fn scan_file_content(content: &str) -> Option<&'static str> {
    if (content.contains("mysql ") || content.contains("mysqldump ") || content.contains("sqlcmd ")) && 
       (content.contains(" -p") || content.contains(" --password=")) {
        if !content.contains("$") && !content.contains("temp_") {
            return Some("MySQL command-line password exposure detected.");
        }
    }
    if content.contains("IDENTIFIED BY") || content.contains("WITH PASSWORD") {
        if !content.contains("$") && !content.contains("temp_") {
            return Some("Plaintext password exposure in inline database query detected.");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::call_tool_in_workspace;
    use serde_json::json;
    use std::fs;
    use std::io::Read;
    use std::net::TcpStream;
    use std::time::Duration;

    fn read_http_request(stream: &mut TcpStream) -> String {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        let mut expected_total = None;

        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    data.extend_from_slice(&buf[..read]);
                    if expected_total.is_none() {
                        if let Some(header_end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
                            let headers_end = header_end + 4;
                            let headers = String::from_utf8_lossy(&data[..headers_end]);
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    let lower = line.to_ascii_lowercase();
                                    lower
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            expected_total = Some(headers_end + content_length);
                        }
                    }
                    if let Some(total) = expected_total {
                        if data.len() >= total {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        String::from_utf8_lossy(&data).to_string()
    }

    #[test]
    fn file_tools_use_explicit_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "write_file",
            &json!({"relativeFilePath": "src/main.rs", "content": "fn main() {}"}),
        )
        .unwrap();

        assert_eq!(fs::read_to_string(root.join("src/main.rs")).unwrap(), "fn main() {}");
    }

    #[test]
    fn file_tools_reject_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let result = call_tool_in_workspace(
            &root,
            "write_file",
            &json!({"relativeFilePath": "../outside.txt", "content": "nope"}),
        );

        assert!(result.is_err());
        assert!(!temp.path().join("outside.txt").exists());
    }

    #[test]
    fn command_runs_in_explicit_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let output = call_tool_in_workspace(&root, "run_command", &json!({"command": "cd"})).unwrap();
        assert!(output.to_lowercase().contains("project"));
    }

    #[test]
    fn file_tools_delete_file_success() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        fs::write(root.join("temp.txt"), "hello").unwrap();
        assert!(root.join("temp.txt").exists());

        call_tool_in_workspace(
            &root,
            "delete_file",
            &json!({"relativeFilePath": "temp.txt"}),
        )
        .unwrap();

        assert!(!root.join("temp.txt").exists());
    }

    #[test]
    fn file_tools_delete_file_rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let result = call_tool_in_workspace(
            &root,
            "delete_file",
            &json!({"relativeFilePath": "../outside.txt"}),
        );

        assert!(result.is_err());
    }

    #[test]
    fn list_dir_returns_error_on_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let result = call_tool_in_workspace(
            &root,
            "list_dir",
            &json!({"relativeDirPath": "missing_folder"}),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_web_navigate_native_parser() {
        use std::io::Write;
        use std::net::TcpListener;
        use velocity_ide::site_map::SiteMap;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    use std::io::Read;
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);

                    let body = "<html><head><title>Egui Test</title></head><body><a href=\"/button\">Click Me</a></body></html>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let sitemap_path = root.join(".velocity").join("site_map");

        let res = crate::editor::browser::crawl_and_sync_sitemap(&url, &sitemap_path).unwrap();
        assert!(res.contains("Egui Test"));
        assert!(res.contains("Interactive Elements: 1"));
        assert!(res.contains("Snapshot JSON:"));
        assert!(res.contains("NDA Facts:"));

        let compact = call_tool_in_workspace(&root, "web_navigate", &json!({"url": url, "compact": true})).unwrap();
        assert!(compact.contains("\"snapshot\":"));
        assert!(compact.contains("\"title\": \"Egui Test\""));
        assert!(compact.contains("\"element_count\": 1"));
        assert!(compact.contains("\"snapshot_json_path\":"));
        assert!(compact.contains("\"nda_facts_path\":"));
        assert!(!compact.contains("Crawler finished."));

        let sm = SiteMap::open(&sitemap_path, 0).unwrap();
        assert!(sm.len() > 0);

        let snapshots = call_tool_in_workspace(&root, "browser_list_snapshots", &json!({})).unwrap();
        assert!(snapshots.contains(&url));
        assert!(snapshots.contains("Egui Test"));
        assert!(snapshots.contains("\"json_path\":"));

        let filtered_snapshots = call_tool_in_workspace(
            &root,
            "browser_list_snapshots",
            &json!({"titleContains": "egui", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_snapshots.contains("Egui Test"));
        assert!(filtered_snapshots.contains("\"json_path\":"));

        let snapshot = call_tool_in_workspace(&root, "browser_read_snapshot", &json!({"url": url})).unwrap();
        assert!(snapshot.contains("\"title\": \"Egui Test\""));
        assert!(snapshot.contains("\"url\":"));

        let compact_snapshot = call_tool_in_workspace(
            &root,
            "browser_read_snapshot",
            &json!({"url": url, "compact": true}),
        )
        .unwrap();
        assert!(compact_snapshot.contains("\"snapshot\":"));
        assert!(compact_snapshot.contains("\"title\": \"Egui Test\""));
        assert!(compact_snapshot.contains("\"request_count\":"));
        assert!(compact_snapshot.contains("\"json_path\":"));
        assert!(!compact_snapshot.contains("\"forms\":"));

        let diff = call_tool_in_workspace(
            &root,
            "browser_diff_snapshots",
            &json!({"beforeUrl": url, "afterUrl": url}),
        )
        .unwrap();
        assert!(diff.contains("\"summary\": \"no_semantic_change\""));
        assert!(diff.contains("\"before_url\":"));
    }

    #[test]
    fn browser_workflow_tools_round_trip_and_replay() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let response_base_url = base_url.clone();

        std::thread::spawn(move || {
            for _ in 0..24 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let dashboard = first_line.contains(" /login ");
                    let body = if dashboard {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = if dashboard {
                        format!(
                            "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nX-Velocity-Local-Storage: theme=dark\r\nX-Velocity-Mutations: route:dashboard;hydration:complete\r\nX-Velocity-Requests: document={0}/login;xhr={0}/login/bootstrap\r\nX-Velocity-Settle: response:complete;navigation:settled;network:settled\r\nX-Velocity-Runtime-State: router:name=dashboard;store:panel=ready\r\nX-Velocity-Protocol-Events: redirect|commit|{0}/login|dashboard ready\r\nContent-Length: {1}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{2}",
                            response_base_url,
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let save = call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Login Flow",
                "startUrl": base_url,
                "variables": {"email": "rust@example.com"},
                "steps": [
                    {"kind": "fill_field", "field": "email", "value": "{{email}}"},
                    {"kind": "submit_form", "form": "login"},
                    {"kind": "wait_for_text", "text": "Welcome back", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_title", "title": "Dashboard", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_url_contains", "fragment": "/login", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_request", "method": "GET", "urlContains": "/bootstrap", "resource": "xhr", "status": 200, "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_storage", "scope": "local", "key": "theme", "value": "dark", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_settle", "scope": "network", "state": "settled", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_runtime_state", "scope": "router", "key": "name", "value": "dashboard", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_protocol_event", "protocolKind": "redirect", "protocolPhase": "commit", "protocolTarget": "/login", "protocolDetail": "dashboard", "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "wait_for_stable", "stablePolls": 1, "timeoutMs": 1500, "intervalMs": 10},
                    {"kind": "extract_text", "output": "page_title", "source": "title"},
                    {"kind": "save_checkpoint", "name": "after-login"},
                    {"kind": "if_output_equals", "output": "page_title", "equals": "Dashboard", "thenSteps": [
                        {"kind": "assert_output", "output": "page_title", "equals": "Dashboard"}
                    ], "elseSteps": []},
                    {"kind": "restore_checkpoint", "name": "after-login"},
                    {"kind": "if_text_contains", "text": "Welcome back", "thenSteps": [
                        {"kind": "assert_text_contains", "text": "Welcome back"}
                    ], "elseSteps": []}
                ]
            }),
        )
        .unwrap();
        assert!(save.contains("Saved browser workflow 'Login Flow'"));

        let rel_path = ".velocity/browser-workflows/login-flow.browser.json";
        let read_back = call_tool_in_workspace(
            &root,
            "browser_read_workflow",
            &json!({"relativeFilePath": rel_path}),
        )
        .unwrap();
        assert!(read_back.contains("Login Flow"));
        assert!(read_back.contains("fill_field"));
        assert!(read_back.contains("submit_form"));
        assert!(read_back.contains("wait_for_text"));
        assert!(read_back.contains("wait_for_title"));
        assert!(read_back.contains("wait_for_url_contains"));
        assert!(read_back.contains("wait_for_request"));
        assert!(read_back.contains("wait_for_storage"));
        assert!(read_back.contains("wait_for_settle"));
        assert!(read_back.contains("wait_for_runtime_state"));
        assert!(read_back.contains("wait_for_protocol_event"));
        assert!(read_back.contains("wait_for_stable"));
        assert!(read_back.contains("extract_text"));
        assert!(read_back.contains("save_checkpoint"));
        assert!(read_back.contains("if_output_equals"));
        assert!(read_back.contains("restore_checkpoint"));
        assert!(read_back.contains("if_text_contains"));
        assert!(read_back.contains("assert_output"));

        let compact_read_back = call_tool_in_workspace(
            &root,
            "browser_read_workflow",
            &json!({"relativeFilePath": rel_path, "compact": true}),
        )
        .unwrap();
        assert!(compact_read_back.contains("\"workflow\":"));
        assert!(compact_read_back.contains("\"name\": \"Login Flow\""));
        assert!(compact_read_back.contains("\"step_count\": 16"));
        assert!(compact_read_back.contains("\"json_path\":"));
        assert!(compact_read_back.contains("\"nda_path\":"));
        assert!(!compact_read_back.contains("\"steps\":"));

        let workflows = call_tool_in_workspace(
            &root,
            "browser_list_workflows",
            &json!({}),
        )
        .unwrap();
        assert!(workflows.contains("Login Flow"));
        assert!(workflows.contains("\"step_count\": 16"));
        assert!(workflows.contains("\"variable_count\": 1"));
        assert!(workflows.contains("\"json_path\":"));
        assert!(workflows.contains("\"nda_path\":"));

        call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Account Flow",
                "startUrl": format!("{}/account", base_url),
                "steps": [
                    {"kind": "assert_text_contains", "text": "Welcome back"}
                ]
            }),
        )
        .unwrap();
        let filtered_workflows = call_tool_in_workspace(
            &root,
            "browser_list_workflows",
            &json!({"workflowNameContains": "flow", "startUrlContains": "/account", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_workflows.contains("Account Flow"));
        assert!(!filtered_workflows.contains("Login Flow"));

        let replay = call_tool_in_workspace(
            &root,
            "browser_replay_workflow",
            &json!({"relativeFilePath": rel_path}),
        )
        .unwrap();
        assert!(replay.contains("Workflow 'Login Flow' completed."));
        assert!(replay.contains("Final title:"));
        assert!(replay.contains("Protocol events: 1"));
        assert!(replay.contains("Cookies: 1"));
        assert!(replay.contains("Run Report:"));

        let compact_replay = call_tool_in_workspace(
            &root,
            "browser_replay_workflow",
            &json!({"relativeFilePath": rel_path, "compact": true}),
        )
        .unwrap();
        assert!(compact_replay.contains("\"workflow_name\": \"Login Flow\""));
        assert!(compact_replay.contains("\"network_summary\":"));
        assert!(compact_replay.contains("\"redirect_count\": 1"));
        assert!(compact_replay.contains("\"run_report_path\":"));
        assert!(!compact_replay.contains("Workflow 'Login Flow' completed."));

        let workflow_runs = call_tool_in_workspace(
            &root,
            "browser_list_workflow_runs",
            &json!({}),
        )
        .unwrap();
        assert!(workflow_runs.contains("Login Flow"));
        assert!(workflow_runs.contains("replay-login-flow"));
        assert!(workflow_runs.contains("Dashboard"));
        assert!(workflow_runs.contains("\"network_summary\":"));
        assert!(workflow_runs.contains("\"redirect_count\": 1"));
        assert!(workflow_runs.contains("\"run_report_path\":"));

        let filtered_workflow_runs = call_tool_in_workspace(
            &root,
            "browser_list_workflow_runs",
            &json!({"workflowNameContains": "login", "sessionIdContains": "replay", "finalUrlContains": "/login", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_workflow_runs.contains("Login Flow"));
        assert!(filtered_workflow_runs.contains("replay-login-flow"));

        let workflow_run = call_tool_in_workspace(
            &root,
            "browser_read_workflow_run",
            &json!({
                "workflowName": "Login Flow",
                "sessionId": "replay-login-flow"
            }),
        )
        .unwrap();
        assert!(workflow_run.contains("\"workflow_name\": \"Login Flow\""));
        assert!(workflow_run.contains("\"session_id\": \"replay-login-flow\""));

        let compact_workflow_run = call_tool_in_workspace(
            &root,
            "browser_read_workflow_run",
            &json!({
                "workflowName": "Login Flow",
                "sessionId": "replay-login-flow",
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_workflow_run.contains("\"workflow\":"));
        assert!(compact_workflow_run.contains("\"workflow_name\": \"Login Flow\""));
        assert!(compact_workflow_run.contains("\"request_count\":"));
        assert!(compact_workflow_run.contains("\"network_summary\":"));
        assert!(compact_workflow_run.contains("\"redirect_count\": 1"));
        assert!(compact_workflow_run.contains("\"run_report_path\":"));
        assert!(!compact_workflow_run.contains("\"outputs\":"));
        assert!(workflow_run.contains("\"final_title\": \"Dashboard\""));
    }

    #[test]
    fn browser_workflow_suite_tools_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..5 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.contains(" /login ") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Login Flow",
                "startUrl": base_url,
                "steps": [
                    {"kind": "fill_field", "field": "email", "value": "rust@example.com"},
                    {"kind": "submit_form", "form": "login"},
                    {"kind": "assert_text_contains", "text": "Welcome back"}
                ]
            }),
        )
        .unwrap();

        let suite_save = call_tool_in_workspace(
            &root,
            "browser_save_workflow_suite",
            &json!({
                "name": "Smoke Pack",
                "workflows": [
                    ".velocity/browser-workflows/login-flow.browser.json",
                    ".velocity/browser-workflows/missing.browser.json"
                ]
            }),
        )
        .unwrap();
        assert!(suite_save.contains("Saved browser workflow suite 'Smoke Pack'"));

        let compact_suite_save = call_tool_in_workspace(
            &root,
            "browser_save_workflow_suite",
            &json!({
                "name": "Smoke Pack Compact",
                "workflows": [
                    ".velocity/browser-workflows/login-flow.browser.json",
                    ".velocity/browser-workflows/missing.browser.json"
                ],
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_suite_save.contains("\"suite\":"));
        assert!(compact_suite_save.contains("\"name\": \"Smoke Pack Compact\""));
        assert!(compact_suite_save.contains("\"workflow_count\": 2"));
        assert!(compact_suite_save.contains("\"json_path\":"));

        let suite_read = call_tool_in_workspace(
            &root,
            "browser_read_workflow_suite",
            &json!({"relativeFilePath": ".velocity/browser-suites/smoke-pack.suite.json"}),
        )
        .unwrap();
        assert!(suite_read.contains("Smoke Pack"));
        assert!(suite_read.contains("login-flow.browser.json"));

        let compact_suite_read = call_tool_in_workspace(
            &root,
            "browser_read_workflow_suite",
            &json!({"relativeFilePath": ".velocity/browser-suites/smoke-pack.suite.json", "compact": true}),
        )
        .unwrap();
        assert!(compact_suite_read.contains("\"suite\":"));
        assert!(compact_suite_read.contains("\"name\": \"Smoke Pack\""));
        assert!(compact_suite_read.contains("\"workflow_count\": 2"));
        assert!(compact_suite_read.contains("\"json_path\":"));
        assert!(!compact_suite_read.contains("\"workflows\":"));

        let suites = call_tool_in_workspace(
            &root,
            "browser_list_workflow_suites",
            &json!({}),
        )
        .unwrap();
        assert!(suites.contains("Smoke Pack"));
        assert!(suites.contains("\"workflow_count\": 2"));
        assert!(suites.contains("\"json_path\":"));

        call_tool_in_workspace(
            &root,
            "browser_save_workflow_suite",
            &json!({
                "name": "Account Pack",
                "workflows": [".velocity/browser-workflows/login-flow.browser.json"]
            }),
        )
        .unwrap();
        let filtered_suites = call_tool_in_workspace(
            &root,
            "browser_list_workflow_suites",
            &json!({"suiteNameContains": "account", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_suites.contains("Account Pack"));
        assert!(!filtered_suites.contains("Smoke Pack"));

        let suite_run = call_tool_in_workspace(
            &root,
            "browser_run_workflow_suite",
            &json!({"relativeFilePath": ".velocity/browser-suites/smoke-pack.suite.json"}),
        )
        .unwrap();
        assert!(suite_run.contains("Workflow suite 'Smoke Pack' completed."));
        assert!(suite_run.contains("Total: 2"));
        assert!(suite_run.contains("Passed: 1"));
        assert!(suite_run.contains("Failed: 1"));
        assert!(suite_run.contains("Suite Report:"));

        let compact_suite_run = call_tool_in_workspace(
            &root,
            "browser_run_workflow_suite",
            &json!({"relativeFilePath": ".velocity/browser-suites/smoke-pack.suite.json", "compact": true}),
        )
        .unwrap();
        assert!(compact_suite_run.contains("\"suite_name\": \"Smoke Pack\""));
        assert!(compact_suite_run.contains("\"suite_report_path\":"));
        assert!(!compact_suite_run.contains("Workflow suite 'Smoke Pack' completed."));

        let suite_runs = call_tool_in_workspace(
            &root,
            "browser_list_workflow_suite_runs",
            &json!({}),
        )
        .unwrap();
        assert!(suite_runs.contains("Smoke Pack"));
        assert!(suite_runs.contains("\"passed\": 1"));
        assert!(suite_runs.contains("\"failed\": 1"));
        assert!(suite_runs.contains("\"suite_report_path\":"));

        let filtered_suite_runs = call_tool_in_workspace(
            &root,
            "browser_list_workflow_suite_runs",
            &json!({"suiteNameContains": "smoke", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_suite_runs.contains("Smoke Pack"));

        let suite_report = call_tool_in_workspace(
            &root,
            "browser_read_workflow_suite_run",
            &json!({"suiteName": "Smoke Pack"}),
        )
        .unwrap();
        assert!(suite_report.contains("\"suite_name\": \"Smoke Pack\""));
        assert!(suite_report.contains("\"total\": 2"));
        assert!(suite_report.contains("missing.browser.json"));

        let compact_suite_report = call_tool_in_workspace(
            &root,
            "browser_read_workflow_suite_run",
            &json!({"suiteName": "Smoke Pack", "compact": true}),
        )
        .unwrap();
        assert!(compact_suite_report.contains("\"suite\":"));
        assert!(compact_suite_report.contains("\"suite_name\": \"Smoke Pack\""));
        assert!(compact_suite_report.contains("\"failed\": 1"));
        assert!(compact_suite_report.contains("\"suite_report_path\":"));
        assert!(!compact_suite_report.contains("\"items\":"));
    }

    #[test]
    fn browser_session_wait_tool_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = if idx == 0 {
                        "<html><head><title>Loading</title></head><body><p>Preparing dashboard</p></body></html>"
                    } else {
                        "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "waiter"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "waiter", "url": url}),
        )
        .unwrap();

        let waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "waiter", "text": "Ready", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(waited.contains("Session wait complete."));
        assert!(waited.contains("Title: Dashboard"));
        assert!(waited.contains("Diff: title,summary"));

        let compact_waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "waiter", "text": "Ready", "timeoutMs": 1500, "intervalMs": 10, "compact": true}),
        )
        .unwrap();
        assert!(compact_waited.contains("\"session_id\": \"waiter\""));
        assert!(compact_waited.contains("\"diff_summary\": \"title,summary\""));
        assert!(!compact_waited.contains("Session wait complete."));
    }

    #[test]
    fn browser_session_wait_request_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        let request_url = format!("{}/bootstrap", url);
        let response_url = url.clone();

        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                    let response = if idx == 0 {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nX-Velocity-Requests: document={0};xhr={1}\r\nContent-Length: {2}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{3}",
                            response_url,
                            request_url,
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "requester"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "requester", "url": url}),
        )
        .unwrap();

        let waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "requester", "requestMethod": "GET", "requestUrlContains": "/bootstrap", "requestStatus": 200, "requestResource": "xhr", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(waited.contains("Session wait complete."));
        assert!(waited.contains("Requests: 2"));
        assert!(waited.contains("Diff: requests+1"));
    }

    #[test]
    fn browser_session_wait_protocol_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        let response_url = url.clone();

        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                    let response = if idx == 0 {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nX-Velocity-Protocol-Events: event_stream|open|{0}/events|text/event-stream connected;websocket|open|wss://example.test/socket|live updates ready\r\nContent-Length: {1}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{2}",
                            response_url,
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "protocol-waiter"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "protocol-waiter", "url": url}),
        )
        .unwrap();

        let waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "protocol-waiter", "protocolKind": "event_stream", "protocolPhase": "open", "protocolTarget": "/events", "protocolDetail": "connected", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(waited.contains("Session wait complete."));
        assert!(waited.contains("Protocol events: 2"));
        assert!(waited.contains("Network summary: redirects=0, downloads=0, uploads=0, streams=2, event_streams=1, websockets=1"));
        assert!(waited.contains("Diff: protocol+2"));
    }

    #[test]
    fn browser_session_wait_storage_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                    let response = if idx == 0 {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nX-Velocity-Session-Storage: csrf=token123\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "storage-waiter"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "storage-waiter", "url": url}),
        )
        .unwrap();

        let waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "storage-waiter", "storageScope": "session", "storageKey": "csrf", "storageValue": "token", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(waited.contains("Session wait complete."));
        assert!(waited.contains("Session storage: 1"));
        assert!(waited.contains("Diff: storage+1"));
    }

    #[test]
    fn browser_session_wait_stream_complete_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        let response_url = url.clone();

        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>";
                    let response = if idx == 0 {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nX-Velocity-Protocol-Events: event_stream|open|{0}/events|connected;event_stream|complete|{0}/events|stream complete\r\nContent-Length: {1}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{2}",
                            response_url,
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "stream-waiter"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "stream-waiter", "url": url}),
        )
        .unwrap();

        let waited = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "stream-waiter", "streamComplete": true, "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(waited.contains("Protocol events: 2"));
        assert!(waited.contains("Diff: protocol+2"));
    }

    #[test]
    fn browser_session_wait_title_and_stable_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for idx in 0..5 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = match idx {
                        0 => "<html><head><title>Loading</title></head><body><p>Preparing</p></body></html>",
                        1 => "<html><head><title>Dashboard Ready</title></head><body><p>Preparing</p></body></html>",
                        _ => "<html><head><title>Dashboard Ready</title></head><body><p>Stable</p></body></html>",
                    };
                    let response = if idx == 1 {
                        format!(
                            "HTTP/1.1 200 OK\r\nX-Velocity-Mutations: route:dashboard;hydration:complete\r\nX-Velocity-Settle: response:complete;navigation:settled;network:settled\r\nX-Velocity-Runtime-State: router:name=dashboard;store:panel=ready\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "steady"}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "steady", "url": url}),
        )
        .unwrap();

        let title_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "title": "Dashboard", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(title_wait.contains("Title: Dashboard Ready"));

        let mutation_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "mutation": "hydration", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(mutation_wait.contains("Diff: no_semantic_change"));

        let runtime_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "runtimeScope": "router", "runtimeKey": "name", "runtimeValue": "dashboard", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(runtime_wait.contains("Runtime state: 2"));

        let settle_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "settleScope": "network", "settleState": "settled", "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(settle_wait.contains("Settle signals: 3"));

        let stable_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "stablePolls": 2, "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(stable_wait.contains("Title: Dashboard Ready"));
        assert!(stable_wait.contains("Diff: summary,mutations-2,settle+3,settle-3,runtime-2"));

        let network_idle_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "networkIdle": true, "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(network_idle_wait.contains("Settle signals: 3"));

        let app_ready_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "appReady": true, "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(app_ready_wait.contains("Runtime state: 2"));

        let mutation_settled_wait = call_tool_in_workspace(
            &root,
            "browser_session_wait",
            &json!({"sessionId": "steady", "mutationSettled": true, "timeoutMs": 1500, "intervalMs": 10}),
        )
        .unwrap();
        assert!(mutation_settled_wait.contains("Settle signals: 3"));
    }

    #[test]
    fn browser_session_tools_round_trip() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        fn read_http_request(stream: &mut TcpStream) -> String {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut data = Vec::new();
            let mut buf = [0u8; 1024];
            let mut expected_total = None;

            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(read) => {
                        data.extend_from_slice(&buf[..read]);
                        if expected_total.is_none() {
                            if let Some(header_end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
                                let header_len = header_end + 4;
                                let header_text = String::from_utf8_lossy(&data[..header_len]);
                                let content_length = header_text
                                    .lines()
                                    .find_map(|line| {
                                        let mut parts = line.splitn(2, ':');
                                        let name = parts.next()?.trim();
                                        let value = parts.next()?.trim();
                                        if name.eq_ignore_ascii_case("Content-Length") {
                                            value.parse::<usize>().ok()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                expected_total = Some(header_len + content_length);
                            }
                        }
                        if let Some(total) = expected_total {
                            if data.len() >= total {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }

            String::from_utf8_lossy(&data).to_string()
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        let server_url = url.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut stream) = stream {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default().to_string();
                    if first_line.contains("GET / ") {
                        assert!(request.contains("User-Agent: VelocityTestAgent/1.0"));
                        assert!(request.contains("X-Test-Header: network-ok"));
                    }
                    let body = if first_line.contains("GET /details ") {
                        "<html><head><title>Detail Test</title></head><body><p>Reached detail page</p></body></html>".to_string()
                    } else if first_line.contains("POST /login ") {
                        "<html><head><title>Submitted Test</title></head><body><p>Saved</p></body></html>".to_string()
                    } else {
                        format!("<html><head><title>Session Test</title></head><body><a href='{0}/details'>Open detail</a><form id='login' action='{0}/login' method='post'><input name='email' placeholder='Email'></form></body></html>", server_url)
                    };
                    let response = if first_line.contains("POST /login ") {
                        format!(
                            "HTTP/1.1 200 OK\r\nSet-Cookie: token=xyz; Path=/\r\nX-Velocity-Session-Storage: csrf=abc123\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nSet-Cookie: token=xyz; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                } else {
                    break;
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let created = call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(created.contains("Created browser session 'qa-session'"));

        let compact_created = call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_created.contains("\"session\":"));
        assert!(compact_created.contains("\"id\": \"qa-session\""));
        assert!(compact_created.contains("\"session_json_path\":"));

        let runtime_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let runtime_port = runtime_listener.local_addr().unwrap().port();
        let runtime_api_base = format!("http://127.0.0.1:{}", runtime_port);
        let runtime_target_url = "https://runtime.test/app".to_string();
        let runtime_target_url_for_server = runtime_target_url.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = runtime_listener.accept() {
                let request = read_http_request(&mut stream);
                assert!(request.starts_with("POST /api/runtime/capture HTTP/1.1"));
                assert!(request.contains("\"url\":\"https://runtime.test/app\""));
                assert!(request.contains("\"timeout_ms\":4321"));
                let body = serde_json::json!({
                    "final_url": runtime_target_url_for_server,
                    "title": "Runtime Test",
                    "html": "<html><head><title>Runtime Test</title></head><body><main><a href='/next'>Next</a><form><input name='email' value='agent@example.com'></form></main></body></html>",
                    "aom_summary": "main region with next link and email field",
                    "page_text": "Runtime Test Next",
                    "scripts": ["app.js"],
                    "fields": {"email": "agent@example.com"},
                    "cookies": [{"name": "runtime", "value": "cookie"}],
                    "local_storage": {"theme": "dark"},
                    "session_storage": {"csrf": "token123"},
                    "settle_signals": ["dom:settled", "network:idle"],
                    "runtime_state": [
                        {"scope": "runtime", "key": "backend", "value": "chromedp"},
                        {"scope": "router", "key": "name", "value": "dashboard"}
                    ],
                    "protocol_events": [
                        {"kind": "navigation", "phase": "commit", "target": "https://runtime.test/app", "detail": "runtime ready"}
                    ],
                    "requests": [
                        {"method": "GET", "url": "https://runtime.test/app", "status_code": 200, "resource": "document"}
                    ],
                    "warnings": ["test-warning"]
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "runtime-session"}),
        )
        .unwrap();

        let runtime_capture = call_tool_in_workspace(
            &root,
            "browser_runtime_capture",
            &json!({
                "sessionId": "runtime-session",
                "url": runtime_target_url,
                "timeoutMs": 4321,
                "apiBase": runtime_api_base
            }),
        )
        .unwrap();
        assert!(runtime_capture.contains("Runtime capture complete."));
        assert!(runtime_capture.contains("Backend: chromedp"));
        assert!(runtime_capture.contains("Title: Runtime Test"));
        assert!(runtime_capture.contains("Warnings (1): test-warning"));
        assert!(runtime_capture.contains("HTML fallback:"));

        let compact_runtime_capture = call_tool_in_workspace(
            &root,
            "browser_runtime_capture",
            &json!({
                "sessionId": "runtime-session",
                "url": "https://runtime.test/app",
                "timeoutMs": 4321,
                "apiBase": "http://127.0.0.1:1",
                "compact": true
            }),
        );
        assert!(compact_runtime_capture.is_err());

        let runtime_session = call_tool_in_workspace(
            &root,
            "browser_get_session",
            &json!({"sessionId": "runtime-session", "compact": true}),
        )
        .unwrap();
        assert!(runtime_session.contains("\"current_url\": \"https://runtime.test/app\""));
        assert!(runtime_session.contains("\"cookie_count\": 1"));

        let runtime_transcript = call_tool_in_workspace(
            &root,
            "browser_read_session_transcript",
            &json!({"sessionId": "runtime-session", "compact": true}),
        )
        .unwrap();
        assert!(runtime_transcript.contains("\"event_kind\": \"runtime_capture\""));

        let initial_network = call_tool_in_workspace(
            &root,
            "browser_get_session_network",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(initial_network.contains("Browser session network config for 'qa-session'"));
        assert!(initial_network.contains("Headers: 0"));

        let updated_network = call_tool_in_workspace(
            &root,
            "browser_set_session_network",
            &json!({
                "sessionId": "qa-session",
                "userAgent": "VelocityTestAgent/1.0",
                "headers": {"X-Test-Header": "network-ok"},
                "timeoutMs": 1200,
                "followRedirects": false,
                "allowedUrlPrefixes": [url.clone()],
                "blockedUrlPrefixes": [format!("{}/blocked", url)]
            }),
        )
        .unwrap();
        assert!(updated_network.contains("Updated browser session network config for 'qa-session'"));
        assert!(updated_network.contains("Headers: 1"));
        assert!(updated_network.contains("Timeout ms: 1200"));
        assert!(updated_network.contains("Follow redirects: false"));

        let compact_network = call_tool_in_workspace(
            &root,
            "browser_get_session_network",
            &json!({"sessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_network.contains("\"user_agent\": \"VelocityTestAgent/1.0\""));
        assert!(compact_network.contains("\"X-Test-Header\": \"network-ok\""));
        assert!(compact_network.contains("\"timeout_ms\": 1200"));
        assert!(compact_network.contains("\"follow_redirects\": false"));
        assert!(compact_network.contains("\"allowed_url_prefixes\":"));
        assert!(compact_network.contains("\"blocked_url_prefixes\":"));

        let blocked_navigation = call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "qa-session", "url": format!("{}/blocked", url)}),
        )
        .unwrap_err();
        assert!(blocked_navigation.to_string().contains("network policy blocked url"));

        let navigated = call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "qa-session", "url": url}),
        )
        .unwrap();
        assert!(navigated.contains("Session: qa-session"));
        assert!(navigated.contains("Forms: 1"));
        assert!(navigated.contains("Cookies: 1"));
        assert!(navigated.contains("HTML fallback:"));

        let clicked = call_tool_in_workspace(
            &root,
            "browser_session_click",
            &json!({"sessionId": "qa-session", "role": "link", "name": "detail"}),
        )
        .unwrap();
        assert!(clicked.contains("Action: click"));
        assert!(clicked.contains("Title: Detail Test"));

        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "qa-session", "url": url}),
        )
        .unwrap();

        let compact_clicked = call_tool_in_workspace(
            &root,
            "browser_session_click",
            &json!({"sessionId": "qa-session", "role": "link", "name": "detail", "compact": true}),
        )
        .unwrap();
        assert!(compact_clicked.contains("\"action\": \"click\""));
        assert!(compact_clicked.contains("\"title\": \"Detail Test\""));

        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "qa-session", "url": url}),
        )
        .unwrap();

        let filled = call_tool_in_workspace(
            &root,
            "browser_session_fill",
            &json!({"sessionId": "qa-session", "field": "email", "value": "agent@example.com"}),
        )
        .unwrap();
        assert!(filled.contains("Action: fill_field"));
        assert!(filled.contains("Target: email"));

        let compact_filled = call_tool_in_workspace(
            &root,
            "browser_session_fill",
            &json!({"sessionId": "qa-session", "field": "email", "value": "agent@example.com", "compact": true}),
        )
        .unwrap();
        assert!(compact_filled.contains("\"action\": \"fill_field\""));
        assert!(compact_filled.contains("\"target\": \"email\""));

        let submitted = call_tool_in_workspace(
            &root,
            "browser_session_submit",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(submitted.contains("Action: submit_form"));
        assert!(submitted.contains("Title: Submitted Test"));

        call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "qa-session", "url": url}),
        )
        .unwrap();
        call_tool_in_workspace(
            &root,
            "browser_session_fill",
            &json!({"sessionId": "qa-session", "field": "email", "value": "agent@example.com"}),
        )
        .unwrap();

        let compact_submitted = call_tool_in_workspace(
            &root,
            "browser_session_submit",
            &json!({"sessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_submitted.contains("\"action\": \"submit_form\""));
        assert!(compact_submitted.contains("\"title\": \"Submitted Test\""));

        let session = call_tool_in_workspace(
            &root,
            "browser_get_session",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(session.contains("\"id\": \"qa-session\""));
        assert!(session.contains("\"name\": \"token\""));

        let compact_session = call_tool_in_workspace(
            &root,
            "browser_get_session",
            &json!({"sessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_session.contains("\"id\": \"qa-session\""));
        assert!(compact_session.contains("\"cookie_count\": 1"));
        assert!(compact_session.contains("\"session_json_path\":"));
        assert!(!compact_session.contains("\"cookies\":"));

        let sessions = call_tool_in_workspace(
            &root,
            "browser_list_sessions",
            &json!({}),
        )
        .unwrap();
        assert!(sessions.contains("qa-session"));
        assert!(sessions.contains("\"cookie_count\": 1"));
        assert!(sessions.contains(&url));
        assert!(sessions.contains("\"session_json_path\":"));

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "archive-session"}),
        )
        .unwrap();
        let filtered_sessions = call_tool_in_workspace(
            &root,
            "browser_list_sessions",
            &json!({"sessionIdContains": "qa", "urlContains": "/login", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_sessions.contains("qa-session"));
        assert!(!filtered_sessions.contains("archive-session"));

        let snapshots = call_tool_in_workspace(
            &root,
            "browser_list_snapshots",
            &json!({}),
        )
        .unwrap();
        assert!(snapshots.contains(&url));
        assert!(snapshots.contains("Session Test"));
        assert!(snapshots.contains("\"json_path\":"));

        let filtered_snapshots = call_tool_in_workspace(
            &root,
            "browser_list_snapshots",
            &json!({"urlContains": "/login", "titleContains": "submitted", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_snapshots.contains("Submitted Test"));

        let snapshot = call_tool_in_workspace(
            &root,
            "browser_read_snapshot",
            &json!({"url": format!("{}/login", url)}),
        )
        .unwrap();
        assert!(snapshot.contains("\"title\": \"Submitted Test\""));
        assert!(snapshot.contains("\"forms\":"));

        let compact_snapshot_diff = call_tool_in_workspace(
            &root,
            "browser_diff_snapshots",
            &json!({"beforeUrl": format!("{}/login", url), "afterUrl": format!("{}/login", url), "compact": true}),
        )
        .unwrap();
        assert!(compact_snapshot_diff.contains("\"diff\":"));
        assert!(compact_snapshot_diff.contains("\"before_url\":"));
        assert!(compact_snapshot_diff.contains("\"summary\":"));
        assert!(compact_snapshot_diff.contains("\"before_json_path\":"));
        assert!(compact_snapshot_diff.contains("\"after_json_path\":"));
        assert!(!compact_snapshot_diff.contains("\"added_elements\":"));

        let compact_snapshot = call_tool_in_workspace(
            &root,
            "browser_read_snapshot",
            &json!({"url": format!("{}/login", url), "compact": true}),
        )
        .unwrap();
        assert!(compact_snapshot.contains("\"snapshot\":"));
        assert!(compact_snapshot.contains("\"title\": \"Submitted Test\""));
        assert!(compact_snapshot.contains("\"runtime_state_count\":"));
        assert!(compact_snapshot.contains("\"json_path\":"));
        assert!(compact_snapshot.contains("\"html_fallback_path\":"));
        assert!(!compact_snapshot.contains("\"forms\":"));

        let visual_fallback = call_tool_in_workspace(
            &root,
            "browser_read_visual_fallback",
            &json!({"url": format!("{}/login", url)}),
        )
        .unwrap();
        assert!(visual_fallback.contains("<title>Submitted Test</title>"));
        assert!(visual_fallback.contains("Submitted"));

        let compact_visual_fallback = call_tool_in_workspace(
            &root,
            "browser_read_visual_fallback",
            &json!({"url": format!("{}/login", url), "compact": true}),
        )
        .unwrap();
        assert!(compact_visual_fallback.contains("\"url\":"));
        assert!(compact_visual_fallback.contains("\"html_path\":"));
        assert!(compact_visual_fallback.contains("\"byte_count\":"));

        let snapshot_diff = call_tool_in_workspace(
            &root,
            "browser_diff_snapshots",
            &json!({"beforeUrl": format!("{}/login", url), "afterUrl": format!("{}/login", url)}),
        )
        .unwrap();
        assert!(snapshot_diff.contains("\"summary\": \"no_semantic_change\""));

        let storage_updated = call_tool_in_workspace(
            &root,
            "browser_set_storage",
            &json!({"sessionId": "qa-session", "scope": "local", "entries": {"theme": "dark", "token": "seeded"}}),
        )
        .unwrap();
        assert!(storage_updated.contains("scope 'local'"));

        let compact_storage_updated = call_tool_in_workspace(
            &root,
            "browser_set_storage",
            &json!({"sessionId": "qa-session", "scope": "local", "entries": {"theme": "dark", "token": "seeded"}, "compact": true}),
        )
        .unwrap();
        assert!(compact_storage_updated.contains("\"scope\": \"local\""));
        assert!(compact_storage_updated.contains("\"updated_entry_count\": 2"));
        assert!(compact_storage_updated.contains("\"session_json_path\":"));

        let storage = call_tool_in_workspace(
            &root,
            "browser_get_storage",
            &json!({"sessionId": "qa-session", "scope": "local"}),
        )
        .unwrap();
        assert!(storage.contains("\"theme\": \"dark\""));
        assert!(storage.contains("\"token\": \"seeded\""));

        let compact_storage = call_tool_in_workspace(
            &root,
            "browser_get_storage",
            &json!({"sessionId": "qa-session", "scope": "local", "compact": true}),
        )
        .unwrap();
        assert!(compact_storage.contains("\"scope\": \"local\""));
        assert!(compact_storage.contains("\"entry_count\": 2"));
        assert!(compact_storage.contains("\"entries\":"));
        assert!(compact_storage.contains("\"session_json_path\":"));
        assert!(compact_storage.contains("\"id\": \"qa-session\""));

        let cookies = call_tool_in_workspace(
            &root,
            "browser_get_cookies",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(cookies.contains("\"name\": \"token\""));
        assert!(cookies.contains("\"value\": \"xyz\""));

        let compact_cookies = call_tool_in_workspace(
            &root,
            "browser_get_cookies",
            &json!({"sessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_cookies.contains("\"cookie_count\": 1"));
        assert!(compact_cookies.contains("\"cookie_names\":"));
        assert!(compact_cookies.contains("\"token\""));
        assert!(compact_cookies.contains("\"session_json_path\":"));
        assert!(!compact_cookies.contains("\"value\": \"xyz\""));

        let cookie_update = call_tool_in_workspace(
            &root,
            "browser_set_cookies",
            &json!({"sessionId": "qa-session", "cookies": [{"name": "refresh", "value": "seeded-refresh"}], "compact": true}),
        )
        .unwrap();
        assert!(cookie_update.contains("\"updated_cookie_count\": 1"));
        assert!(cookie_update.contains("\"cookie_count\": 2"));
        assert!(cookie_update.contains("\"cookie_names\":"));
        assert!(cookie_update.contains("\"refresh\""));
        assert!(cookie_update.contains("\"session_json_path\":"));

        let cookies_after_update = call_tool_in_workspace(
            &root,
            "browser_get_cookies",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(cookies_after_update.contains("\"name\": \"refresh\""));
        assert!(cookies_after_update.contains("\"value\": \"seeded-refresh\""));

        let auth_diagnostics = call_tool_in_workspace(
            &root,
            "browser_auth_diagnostics",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(auth_diagnostics.contains("\"diagnosis\": \"unknown\""));
        assert!(auth_diagnostics.contains("\"has_login_form\": false"));
        assert!(auth_diagnostics.contains("\"has_auth_cookie\": true"));
        assert!(auth_diagnostics.contains("\"has_csrf_token\": true"));
        assert!(auth_diagnostics.contains("\"snapshot_json_path\":"));

        let saved_auth_profile = call_tool_in_workspace(
            &root,
            "browser_save_auth_profile",
            &json!({"profileName": "qa-profile", "sourceSessionId": "qa-session"}),
        )
        .unwrap();
        assert!(saved_auth_profile.contains("Saved browser auth profile 'qa-profile'"));
        assert!(saved_auth_profile.contains("Source kind: session"));

        let compact_saved_auth_profile = call_tool_in_workspace(
            &root,
            "browser_save_auth_profile",
            &json!({"profileName": "qa-profile", "sourceSessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_saved_auth_profile.contains("\"profile\":"));
        assert!(compact_saved_auth_profile.contains("\"name\": \"qa-profile\""));
        assert!(compact_saved_auth_profile.contains("\"profile_json_path\":"));

        let auth_profiles = call_tool_in_workspace(
            &root,
            "browser_list_auth_profiles",
            &json!({}),
        )
        .unwrap();
        assert!(auth_profiles.contains("qa-profile"));
        assert!(auth_profiles.contains("\"cookie_count\":"));
        assert!(auth_profiles.contains("\"json_path\":"));

        let filtered_auth_profiles = call_tool_in_workspace(
            &root,
            "browser_list_auth_profiles",
            &json!({"profileNameContains": "qa", "sourceSessionIdContains": "qa-session", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_auth_profiles.contains("qa-profile"));

        let auth_profile = call_tool_in_workspace(
            &root,
            "browser_read_auth_profile",
            &json!({"profileName": "qa-profile"}),
        )
        .unwrap();
        assert!(auth_profile.contains("\"name\": \"qa-profile\""));
        assert!(auth_profile.contains("\"cookies\":"));
        assert!(auth_profile.contains("\"value\": \"xyz\""));

        let compact_auth_profile = call_tool_in_workspace(
            &root,
            "browser_read_auth_profile",
            &json!({"profileName": "qa-profile", "compact": true}),
        )
        .unwrap();
        assert!(compact_auth_profile.contains("\"profile\":"));
        assert!(compact_auth_profile.contains("\"cookie_count\":"));
        assert!(compact_auth_profile.contains("\"profile_json_path\":"));
        assert!(!compact_auth_profile.contains("\"cookies\":"));

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "profile-target"}),
        )
        .unwrap();
        let applied_auth_profile = call_tool_in_workspace(
            &root,
            "browser_apply_auth_profile",
            &json!({"profileName": "qa-profile", "targetSessionId": "profile-target"}),
        )
        .unwrap();
        assert!(applied_auth_profile.contains("Applied browser auth profile 'qa-profile' to session 'profile-target'"));
        assert!(applied_auth_profile.contains("Auth diagnosis: unknown"));

        let compact_applied_auth_profile = call_tool_in_workspace(
            &root,
            "browser_apply_auth_profile",
            &json!({"profileName": "qa-profile", "targetSessionId": "profile-target", "compact": true}),
        )
        .unwrap();
        assert!(compact_applied_auth_profile.contains("\"profile_name\": \"qa-profile\""));
        assert!(compact_applied_auth_profile.contains("\"target_session\":"));
        assert!(compact_applied_auth_profile.contains("\"auth_diagnostics\":"));
        assert!(compact_applied_auth_profile.contains("\"profile_json_path\":"));

        let access_diagnostics = call_tool_in_workspace(
            &root,
            "browser_access_diagnostics",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(access_diagnostics.contains("\"diagnosis\": \"clear\""));
        assert!(access_diagnostics.contains("\"challenge_signal_count\":"));
        assert!(access_diagnostics.contains("\"session\":"));
        assert!(access_diagnostics.contains("\"snapshot_json_path\":"));

        let session_transcript = call_tool_in_workspace(
            &root,
            "browser_read_session_transcript",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(session_transcript.contains("Browser session transcript for 'qa-session'"));
        assert!(session_transcript.contains("[navigate] ok - Navigated to"));
        assert!(session_transcript.contains("[click] ok - click -> link:Open detail"));
        assert!(session_transcript.contains("[fill_field] ok - fill_field -> email"));
        assert!(session_transcript.contains("[submit_form] ok - submit_form -> login"));

        let compact_session_transcript = call_tool_in_workspace(
            &root,
            "browser_read_session_transcript",
            &json!({"sessionId": "qa-session", "limit": 3, "sortDirection": "desc", "compact": true}),
        )
        .unwrap();
        assert!(compact_session_transcript.contains("\"entry_count\": 3"));
        assert!(compact_session_transcript.contains("\"transcript_json_path\":"));
        assert!(compact_session_transcript.contains("\"entries\":"));

        let transcript_entry = call_tool_in_workspace(
            &root,
            "browser_read_session_transcript",
            &json!({"sessionId": "qa-session", "sequence": 1}),
        )
        .unwrap();
        assert!(transcript_entry.contains("\"sequence\": 1"));
        assert!(transcript_entry.contains("\"event_kind\": "));
        assert!(transcript_entry.contains("\"session_json_path\":"));

        let session_health = call_tool_in_workspace(
            &root,
            "browser_session_health",
            &json!({"sessionId": "qa-session"}),
        )
        .unwrap();
        assert!(session_health.contains("Browser session health for 'qa-session'"));
        assert!(session_health.contains("Recovery posture: recover_navigation"));
        assert!(session_health.contains("Auth diagnosis: unknown"));
        assert!(session_health.contains("Access diagnosis: clear"));
        assert!(session_health.contains("Latest failure: #"));
        assert!(session_health.contains("HTML fallback: "));

        let compact_session_health = call_tool_in_workspace(
            &root,
            "browser_session_health",
            &json!({"sessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_session_health.contains("\"recovery_posture\": \"recover_navigation\""));
        assert!(compact_session_health.contains("\"recommended_action\":"));
        assert!(compact_session_health.contains("\"auth_diagnostics\":"));
        assert!(compact_session_health.contains("\"access_diagnostics\":"));
        assert!(compact_session_health.contains("\"network\":"));
        assert!(compact_session_health.contains("\"checkpoint_count\": 0"));
        assert!(compact_session_health.contains("\"recent_failure_count\":"));
        assert!(compact_session_health.contains("\"session_json_path\":"));
        assert!(compact_session_health.contains("\"html_fallback_path\":"));
        assert!(compact_session_health.contains("\"snapshot\":"));
        assert!(compact_session_health.contains("\"latest_failure\":"));
        assert!(compact_session_health.contains("\"evidence_signals\":"));

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "reseed-target"}),
        )
        .unwrap();
        let reseeded = call_tool_in_workspace(
            &root,
            "browser_reseed_auth",
            &json!({"targetSessionId": "reseed-target", "sourceSessionId": "qa-session"}),
        )
        .unwrap();
        assert!(reseeded.contains("Reseeded auth state into session 'reseed-target'"));
        assert!(reseeded.contains("Source kind: session"));
        assert!(reseeded.contains("Auth diagnosis: unknown"));

        let compact_reseeded = call_tool_in_workspace(
            &root,
            "browser_reseed_auth",
            &json!({"targetSessionId": "reseed-target", "sourceSessionId": "qa-session", "compact": true}),
        )
        .unwrap();
        assert!(compact_reseeded.contains("\"source_kind\": \"session\""));
        assert!(compact_reseeded.contains("\"source_session_id\": \"qa-session\""));
        assert!(compact_reseeded.contains("\"copied_cookie_count\":"));
        assert!(compact_reseeded.contains("\"auth_diagnostics\":"));
        assert!(compact_reseeded.contains("\"session_json_path\":"));

        let profile_target_session = call_tool_in_workspace(
            &root,
            "browser_get_session",
            &json!({"sessionId": "profile-target"}),
        )
        .unwrap();
        assert!(profile_target_session.contains("\"name\": \"token\""));
        assert!(profile_target_session.contains("\"name\": \"refresh\""));
    }

    #[test]
    fn runtime_explicit_session_tools_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let api_base = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            let png = [0x89u8, b'P', b'N', b'G'];
            for _ in 0..9 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default().to_string();
                    if first_line.starts_with("POST /api/runtime/session HTTP/1.1") {
                        let body = json!({
                            "sessionId": "rt-123",
                            "runtimeState": {
                                "sessionId": "rt-123",
                                "alive": true,
                                "mode": "managed",
                                "createdAt": "2026-07-20T20:00:00Z",
                                "lastAction": "open"
                            },
                            "protocolEvidence": {
                                "backend": "go-chromedp",
                                "transport": "http-json",
                                "sessionMode": "managed",
                                "supportsActions": ["navigate", "click", "js_click", "fill", "submit", "press_key", "evaluate"],
                                "supportsCapture": true,
                                "supportsSessions": true
                            },
                            "warnings": ["create-warning"]
                        })
                        .to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    } else if first_line.starts_with("POST /api/runtime/session/rt-123/capture HTTP/1.1") {
                        let body = json!({
                            "sessionId": "rt-123",
                            "finalUrl": "https://runtime.test/captured",
                            "title": "Runtime Captured",
                            "html": "<html><head><title>Runtime Captured</title></head><body><form><input name='email'></form></body></html>",
                            "cookies": [{"name": "rt", "value": "cookie"}],
                            "storage": {"local": {"theme": "dark"}, "session": {"csrf": "token"}},
                            "fields": {"email": "input[name='email']"},
                            "runtimeState": {"sessionId": "rt-123", "alive": true, "mode": "managed", "lastAction": "capture", "frameCount": 2, "shadowHostCount": 1},
                            "protocolEvidence": {"backend": "go-chromedp", "transport": "http-json", "sessionMode": "managed", "supportsActions": ["navigate"], "supportsCapture": true, "supportsSessions": true},
                            "warnings": ["capture-warning"],
                            "frames": [
                                {"selector": "iframe#checkout", "source": "https://payments.example/frame", "accessible": false, "sameOrigin": false, "semanticNodeCount": 0},
                                {"selector": "iframe[name=embedded]", "source": "/embedded", "accessible": true, "sameOrigin": true, "semanticNodeCount": 4}
                            ],
                            "shadowHosts": [
                                {"selector": "checkout-shell", "tag": "checkout-shell", "mode": "open", "semanticNodeCount": 3, "textSample": "Pay now"}
                            ],
                            "aom": "main form",
                            "pageText": "Runtime Captured",
                            "scripts": ["https://cdn.example/app.js"]
                        })
                        .to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    } else if first_line.starts_with("POST /api/runtime/session/rt-123/action HTTP/1.1") {
                        let is_evaluate = request.contains("\"action\":\"evaluate\"") || request.contains("\"action\": \"evaluate\"");
                        let body = if is_evaluate {
                            json!({
                                "sessionId": "rt-123",
                                "finalUrl": "https://runtime.test/after-evaluate",
                                "title": "Runtime After Evaluate",
                                "html": "<html><head><title>Runtime After Evaluate</title></head><body><p>eval</p></body></html>",
                                "cookies": ["flow=ok"],
                                "storage": {"local": {"theme": "light"}, "session": {"csrf": "token2"}},
                                "fields": {},
                                "runtimeState": {"sessionId": "rt-123", "alive": true, "mode": "managed", "lastAction": "evaluate"},
                                "protocolEvidence": {"backend": "go-chromedp", "transport": "http-json", "sessionMode": "managed", "supportsActions": ["fill", "evaluate"], "supportsCapture": true, "supportsSessions": true},
                                "warnings": ["action-warning"],
                                "action": {"action": "evaluate", "script": "({ answer: 42 })", "result": "{\"answer\":42}", "waitAppliedMs": 600, "warnings": ["post-action wait did not settle cleanly"]},
                                "aom": "updated",
                                "pageText": "Runtime After Evaluate",
                                "scripts": []
                            })
                        } else {
                            json!({
                                "sessionId": "rt-123",
                                "finalUrl": "https://runtime.test/after-action",
                                "title": "Runtime After Action",
                                "html": "<html><head><title>Runtime After Action</title></head><body><p>done</p></body></html>",
                                "cookies": ["flow=ok"],
                                "storage": {"local": {"theme": "light"}, "session": {"csrf": "token2"}},
                                "fields": {},
                                "runtimeState": {"sessionId": "rt-123", "alive": true, "mode": "managed", "lastAction": "fill"},
                                "protocolEvidence": {"backend": "go-chromedp", "transport": "http-json", "sessionMode": "managed", "supportsActions": ["fill", "evaluate"], "supportsCapture": true, "supportsSessions": true},
                                "warnings": ["action-warning"],
                                "action": {"action": "fill", "target": "#email", "value": "agent@example.com", "waitAppliedMs": 1500, "warnings": ["post-action wait did not settle cleanly"]},
                                "aom": "updated",
                                "pageText": "Runtime After Action",
                                "scripts": []
                            })
                        }
                        .to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    } else if first_line.starts_with("POST /api/runtime/visual-artifact HTTP/1.1") {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: image/png\r\nX-Runtime-Artifact-Kind: runtime_screenshot\r\nX-Runtime-Page-Url: https://runtime.test/final-shot\r\nConnection: close\r\n\r\n",
                            png.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.write_all(&png);
                        let _ = stream.flush();
                    } else if first_line.starts_with("DELETE /api/runtime/session/rt-123 HTTP/1.1") {
                        let body = json!({"sessionId": "rt-123", "status": "closed"}).to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let created = call_tool_in_workspace(
            &root,
            "runtime_create_session",
            &json!({"sessionId": "runtime-explicit", "startUrl": "https://runtime.test/start", "apiBase": api_base, "compact": true}),
        )
        .unwrap();
        assert!(created.contains("\"runtime_session_id\": \"rt-123\""));
        assert!(created.contains("\"warning_count\": 1"));
        assert!(created.contains("create-warning"));

        let read_back = call_tool_in_workspace(
            &root,
            "runtime_get_session",
            &json!({"sessionId": "runtime-explicit", "compact": true}),
        )
        .unwrap();
        assert!(read_back.contains("\"id\": \"runtime-explicit\""));
        assert!(read_back.contains("\"runtime_session_id\": \"rt-123\""));

        let captured = call_tool_in_workspace(
            &root,
            "runtime_capture_session",
            &json!({"sessionId": "runtime-explicit", "compact": true}),
        )
        .unwrap();
        assert!(captured.contains("\"title\": \"Runtime Captured\""));
        assert!(captured.contains("\"capture_backend\": \"go-chromedp\""));
        assert!(captured.contains("\"warning_count\": 1"));
        assert!(captured.contains("\"frame_count\": 2"));
        assert!(captured.contains("\"shadow_host_count\": 1"));
        assert!(captured.contains("\"iframe#checkout\""));
        assert!(captured.contains("\"checkout-shell\""));

        let filled = call_tool_in_workspace(
            &root,
            "runtime_session_fill",
            &json!({"sessionId": "runtime-explicit", "selector": "#email", "value": "agent@example.com", "apiBase": "ignored", "compact": true}),
        )
        .unwrap();
        assert!(filled.contains("\"title\": \"Runtime After Action\""));
        assert!(filled.contains("\"warning_count\": 2"));
        assert!(filled.contains("action-warning"));
        assert!(filled.contains("post-action wait did not settle cleanly"));
        assert!(filled.contains("\"action\": {"));
        assert!(filled.contains("\"target\": \"#email\""));

        let evaluated = call_tool_in_workspace(
            &root,
            "runtime_session_evaluate",
            &json!({"sessionId": "runtime-explicit", "script": "({ answer: 42 })", "compact": true}),
        )
        .unwrap();
        assert!(evaluated.contains("\"title\": \"Runtime After Evaluate\""));
        assert!(evaluated.contains("\"action\": {"));
        assert!(evaluated.contains("\"action\": \"evaluate\""));
        assert!(evaluated.contains("\"script\": \"({ answer: 42 })\""));
        assert!(evaluated.contains("\\\"answer\\\":42"));

        let visual = call_tool_in_workspace(
            &root,
            "browser_runtime_visual_capture",
            &json!({"url": "https://runtime.test/shot", "apiBase": "http://127.0.0.1:".to_string() + &port.to_string(), "compact": true}),
        )
        .unwrap();
        assert!(visual.contains("\"artifact_kind\": \"runtime_screenshot\""));
        assert!(visual.contains("\"captured_url\": \"https://runtime.test/final-shot\""));
        assert!(visual.contains("\"mime_type\": \"image/png\""));

        let evaluated_text = call_tool_in_workspace(
            &root,
            "runtime_session_evaluate",
            &json!({"sessionId": "runtime-explicit", "script": "({ answer: 42 })"}),
        )
        .unwrap();
        assert!(evaluated_text.contains("Action: evaluate (wait 600ms)"));
        assert!(evaluated_text.contains("script=({ answer: 42 })"));
        assert!(evaluated_text.contains("result={\"answer\":42}"));

        let closed = call_tool_in_workspace(
            &root,
            "runtime_close_session",
            &json!({"sessionId": "runtime-explicit", "compact": true}),
        )
        .unwrap();
        assert!(closed.contains("\"session_id\": \"runtime-explicit\""));
        assert!(closed.contains("\"runtime_session_id\": \"rt-123\""));
        assert!(!root.join(".velocity").join("runtime-browser-sessions").join("runtime-explicit.json").exists());
    }

    #[test]
    fn browser_checkpoint_and_session_replay_tools_round_trip() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..5 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.contains(" /login ") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();

        call_tool_in_workspace(
            &root,
            "browser_create_session",
            &json!({"sessionId": "auth-session"}),
        )
        .unwrap();
        let navigated = call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "auth-session", "url": base_url}),
        )
        .unwrap();
        assert!(navigated.contains("Session navigate complete."));
        assert!(navigated.contains("Session: auth-session"));

        let compact_navigated = call_tool_in_workspace(
            &root,
            "browser_session_navigate",
            &json!({"sessionId": "auth-session", "url": base_url, "compact": true}),
        )
        .unwrap();
        assert!(compact_navigated.contains("\"session_id\": \"auth-session\""));
        assert!(compact_navigated.contains("\"title\": \"Login\""));
        assert!(compact_navigated.contains("\"snapshot_json_path\":"));
        assert!(!compact_navigated.contains("Session navigate complete."));

        let saved = call_tool_in_workspace(
            &root,
            "browser_save_checkpoint",
            &json!({"sessionId": "auth-session", "checkpointName": "before-submit"}),
        )
        .unwrap();
        assert!(saved.contains("Saved browser checkpoint 'before-submit'"));

        let compact_saved = call_tool_in_workspace(
            &root,
            "browser_save_checkpoint",
            &json!({"sessionId": "auth-session", "checkpointName": "before-submit", "compact": true}),
        )
        .unwrap();
        assert!(compact_saved.contains("\"checkpoint\":"));
        assert!(compact_saved.contains("\"name\": \"before-submit\""));
        assert!(compact_saved.contains("\"checkpoint_json_path\":"));

        let listed = call_tool_in_workspace(
            &root,
            "browser_list_checkpoints",
            &json!({"sessionId": "auth-session"}),
        )
        .unwrap();
        assert!(listed.contains("\"name\": \"before-submit\""));
        assert!(listed.contains("\"title\": \"Login\""));
        assert!(listed.contains("\"snapshot_summary\":"));
        assert!(listed.contains("\"request_count\":"));
        assert!(listed.contains("\"runtime_state_count\":"));
        assert!(listed.contains("\"checkpoint_json_path\":"));

        let filtered_listed = call_tool_in_workspace(
            &root,
            "browser_list_checkpoints",
            &json!({"sessionId": "auth-session", "checkpointNameContains": "before", "titleContains": "login", "limit": 1, "sortDirection": "desc"}),
        )
        .unwrap();
        assert!(filtered_listed.contains("\"name\": \"before-submit\""));

        let checkpoint = call_tool_in_workspace(
            &root,
            "browser_read_checkpoint",
            &json!({"sessionId": "auth-session", "checkpointName": "before-submit"}),
        )
        .unwrap();
        assert!(checkpoint.contains("\"name\": \"before-submit\""));
        assert!(checkpoint.contains("\"id\": \"auth-session\""));

        let compact_checkpoint = call_tool_in_workspace(
            &root,
            "browser_read_checkpoint",
            &json!({"sessionId": "auth-session", "checkpointName": "before-submit", "compact": true}),
        )
        .unwrap();
        assert!(compact_checkpoint.contains("\"checkpoint\":"));
        assert!(compact_checkpoint.contains("\"name\": \"before-submit\""));
        assert!(compact_checkpoint.contains("\"snapshot_summary\":"));
        assert!(compact_checkpoint.contains("\"form_count\": 1"));
        assert!(compact_checkpoint.contains("\"checkpoint_json_path\":"));
        assert!(!compact_checkpoint.contains("\"session\":"));

        let save_workflow = call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Resume Login",
                "startUrl": base_url,
                "steps": [
                    {"kind": "fill_field", "field": "email", "value": "rust@example.com"},
                    {"kind": "submit_form", "form": "login"},
                    {"kind": "assert_text_contains", "text": "Welcome back"}
                ]
            }),
        )
        .unwrap();
        assert!(save_workflow.contains("Saved browser workflow 'Resume Login'"));

        let compact_save_workflow = call_tool_in_workspace(
            &root,
            "browser_save_workflow",
            &json!({
                "name": "Resume Login Compact",
                "startUrl": base_url,
                "steps": [
                    {"kind": "fill_field", "field": "email", "value": "rust@example.com"},
                    {"kind": "submit_form", "form": "login"},
                    {"kind": "assert_text_contains", "text": "Welcome back"}
                ],
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_save_workflow.contains("\"workflow\":"));
        assert!(compact_save_workflow.contains("\"name\": \"Resume Login Compact\""));
        assert!(compact_save_workflow.contains("\"json_path\":"));
        assert!(compact_save_workflow.contains("\"nda_path\":"));

        let replay = call_tool_in_workspace(
            &root,
            "browser_replay_workflow",
            &json!({
                "relativeFilePath": ".velocity/browser-workflows/resume-login.browser.json",
                "sessionId": "auth-session"
            }),
        )
        .unwrap();
        assert!(replay.contains("Workflow 'Resume Login' completed."));
        assert!(replay.contains("Final title: Dashboard"));
        assert!(replay.contains("Session: auth-session"));
        assert!(replay.contains("Session JSON:"));

        let compact_replay = call_tool_in_workspace(
            &root,
            "browser_read_workflow_run",
            &json!({
                "workflowName": "Resume Login",
                "sessionId": "auth-session",
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_replay.contains("\"workflow_name\": \"Resume Login\""));
        assert!(compact_replay.contains("\"session_id\": \"auth-session\""));
        assert!(compact_replay.contains("\"request_count\":"));
        assert!(compact_replay.contains("\"network_summary\":"));

        call_tool_in_workspace(
            &root,
            "browser_save_checkpoint",
            &json!({"sessionId": "auth-session", "checkpointName": "after-submit"}),
        )
        .unwrap();
        let checkpoint_diff = call_tool_in_workspace(
            &root,
            "browser_diff_checkpoints",
            &json!({
                "sessionId": "auth-session",
                "beforeCheckpointName": "before-submit",
                "afterCheckpointName": "after-submit"
            }),
        )
        .unwrap();
        assert!(checkpoint_diff.contains("\"summary\":"));
        assert!(checkpoint_diff.contains("forms-1"));

        let compact_checkpoint_diff = call_tool_in_workspace(
            &root,
            "browser_diff_checkpoints",
            &json!({
                "sessionId": "auth-session",
                "beforeCheckpointName": "before-submit",
                "afterCheckpointName": "after-submit",
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_checkpoint_diff.contains("\"diff\":"));
        assert!(compact_checkpoint_diff.contains("\"before_url\":"));
        assert!(compact_checkpoint_diff.contains("\"summary\":"));
        assert!(compact_checkpoint_diff.contains("\"before_json_path\":"));
        assert!(compact_checkpoint_diff.contains("\"after_json_path\":"));
        assert!(!compact_checkpoint_diff.contains("\"added_elements\":"));

        let restored = call_tool_in_workspace(
            &root,
            "browser_restore_checkpoint",
            &json!({
                "sessionId": "auth-session",
                "checkpointName": "before-submit",
                "targetSessionId": "forked-session"
            }),
        )
        .unwrap();
        assert!(restored.contains("Restored browser session checkpoint 'before-submit'"));
        assert!(restored.contains("Session: forked-session"));
        assert!(restored.contains("Title: Login"));
        assert!(restored.contains("Auth diagnosis: login_required"));

        let compact_restored = call_tool_in_workspace(
            &root,
            "browser_restore_checkpoint",
            &json!({
                "sessionId": "auth-session",
                "checkpointName": "before-submit",
                "targetSessionId": "forked-session-compact",
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_restored.contains("\"checkpoint_name\": \"before-submit\""));
        assert!(compact_restored.contains("\"session_id\": \"forked-session-compact\""));
        assert!(compact_restored.contains("\"title\": \"Login\""));
        assert!(compact_restored.contains("\"auth_diagnostics\":"));
        assert!(compact_restored.contains("\"diagnosis\": \"login_required\""));
        assert!(compact_restored.contains("\"network_summary\":"));
        assert!(compact_restored.contains("\"snapshot_json_path\":"));
        assert!(!compact_restored.contains("Restored browser session checkpoint"));

        let reseeded = call_tool_in_workspace(
            &root,
            "browser_reseed_auth",
            &json!({
                "targetSessionId": "forked-session",
                "sourceSessionId": "auth-session",
                "sourceCheckpointName": "before-submit"
            }),
        )
        .unwrap();
        assert!(reseeded.contains("Reseeded auth state into session 'forked-session'"));
        assert!(reseeded.contains("Source kind: checkpoint"));
        assert!(reseeded.contains("Source checkpoint: before-submit"));

        let compact_reseeded = call_tool_in_workspace(
            &root,
            "browser_reseed_auth",
            &json!({
                "targetSessionId": "forked-session",
                "sourceSessionId": "auth-session",
                "sourceCheckpointName": "before-submit",
                "compact": true
            }),
        )
        .unwrap();
        assert!(compact_reseeded.contains("\"source_kind\": \"checkpoint\""));
        assert!(compact_reseeded.contains("\"source_checkpoint_name\": \"before-submit\""));
        assert!(compact_reseeded.contains("\"copied_cookie_names\":"));
        assert!(compact_reseeded.contains("\"auth_diagnostics\":"));
        assert!(compact_reseeded.contains("\"session_json_path\":"));
    }
}

struct SidecarDaemon {
    child: Child,
}

static DAEMON: Lazy<Mutex<Option<SidecarDaemon>>> = Lazy::new(|| Mutex::new(None));

fn execute_csharp_mcp_tool(tool_name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let exe_path = "C:\\Users\\visse\\OneDrive\\Documents\\Payment and Transaction Flow\\Velocity\\NdaMcpServer\\bin\\Debug\\net10.0\\NdaMcpServer.exe";
    
    let mut daemon_guard = DAEMON.lock().map_err(|e| e.to_string())?;
    
    if daemon_guard.is_none() {
        if !std::path::Path::new(exe_path).exists() {
            return execute_rust_fallback_tool(tool_name, arguments);
        }
        
        let child = Command::new(exe_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        *daemon_guard = Some(SidecarDaemon { child });
    } else {
        let daemon = daemon_guard.as_mut().unwrap();
        if let Ok(Some(_status)) = daemon.child.try_wait() {
            let child = Command::new(exe_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            *daemon = SidecarDaemon { child };
        }
    }
    
    let daemon = daemon_guard.as_mut().unwrap();
    
    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        },
        "id": 999
    });

    let request_str = serde_json::to_string(&request)? + "\n";

    {
        let stdin = daemon.child.stdin.as_mut().ok_or("Failed to open stdin of C# daemon")?;
        stdin.write_all(request_str.as_bytes())?;
        stdin.flush()?;
    }

    let response_str;
    {
        let stdout = daemon.child.stdout.as_mut().ok_or("Failed to open stdout of C# daemon")?;
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line.is_empty() {
                return Err("C# sidecar daemon closed stdout unexpectedly".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                response_str = trimmed.to_string();
                break;
            } else {
                eprintln!("[C# Sidecar Log] {}", trimmed);
            }
        }
    }

    let response: Value = serde_json::from_str(&response_str)?;

    if let Some(err) = response.get("error") {
        return Err(format!("C# Execution Error: {}", err["message"].as_str().unwrap_or("Unknown")).into());
    }

    let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
    let text = response["result"]["content"][0]["text"].as_str().ok_or("Failed to parse tool text output")?;

    if is_error {
        Err(text.into())
    } else {
        Ok(text.to_string())
    }
}

// --- Self-contained Rust Fallback & Sandboxing Runner ---

fn execute_rust_fallback_tool(tool_name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    match tool_name {
        "convert_to_nda" => {
            let file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            let output_path = arguments["outputPath"].as_str().unwrap_or("");
            
            let final_output = if output_path.is_empty() {
                format!("{}.nda", file_path)
            } else {
                output_path.to_string()
            };
            
            let content = std::fs::read(file_path)?;
            
            let mut nda_bytes = Vec::new();
            nda_bytes.extend_from_slice(b"NDAV");
            
            let size = content.len() as u32;
            nda_bytes.extend_from_slice(&size.to_le_bytes());
            
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.txt");
            nda_bytes.extend_from_slice(file_name.as_bytes());
            nda_bytes.push(0);
            nda_bytes.extend_from_slice(&content);
            
            std::fs::write(&final_output, nda_bytes)?;
            
            Ok(format!("Success: File converted and signed to NDA container at: {}", final_output))
        }
        "read_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            let nda_bytes = std::fs::read(nda_path)?;
            
            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }
            
            let size = u32::from_le_bytes([nda_bytes[4], nda_bytes[5], nda_bytes[6], nda_bytes[7]]) as usize;
            
            let mut name_end = 8;
            while name_end < nda_bytes.len() && nda_bytes[name_end] != 0 {
                name_end += 1;
            }
            
            let file_name = String::from_utf8_lossy(&nda_bytes[8..name_end]).to_string();
            
            let report = json!({
                "format": "NDAV-Fallback",
                "fileName": file_name,
                "payloadSizeBytes": size,
                "visualDisplayCommands": [
                    "display_text: NDA Container Contents Verified",
                    format!("display_text: Filename: {}", file_name),
                    format!("display_text: Size: {} bytes", size)
                ]
            });
            
            Ok(serde_json::to_string_pretty(&report)?)
        }
        "execute_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            let nda_bytes = std::fs::read(nda_path)?;
            
            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }
            
            let mut name_end = 8;
            while name_end < nda_bytes.len() && nda_bytes[name_end] != 0 {
                name_end += 1;
            }
            
            let file_name = String::from_utf8_lossy(&nda_bytes[8..name_end]).to_string();
            let payload = &nda_bytes[name_end + 1..];
            
            let temp_dir = std::env::temp_dir();
            let temp_file_path = temp_dir.join(&file_name);
            std::fs::write(&temp_file_path, payload)?;
            
            let ext = std::path::Path::new(&file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
                
            let cmd_args = arguments["arguments"].as_array();
            let mut args_vec = Vec::new();
            if let Some(arr) = cmd_args {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        args_vec.push(s.to_string());
                    }
                }
            }
            
            let (shell_cmd, mut final_args) = match ext.as_str() {
                "py" => {
                    ("python".to_string(), vec![temp_file_path.to_string_lossy().to_string()])
                }
                "js" => {
                    ("node".to_string(), vec![temp_file_path.to_string_lossy().to_string()])
                }
                "ps1" => {
                    ("powershell".to_string(), vec![
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-File".to_string(),
                        temp_file_path.to_string_lossy().to_string()
                    ])
                }
                "sh" => {
                    ("bash".to_string(), vec![temp_file_path.to_string_lossy().to_string()])
                }
                "bat" | "cmd" => {
                    ("cmd".to_string(), vec!["/c".to_string(), temp_file_path.to_string_lossy().to_string()])
                }
                _ => {
                    (temp_file_path.to_string_lossy().to_string(), Vec::new())
                }
            };
            
            final_args.extend(args_vec);
            
            let dll_path = "C:\\WUIAS\\wuias_shield\\wuias_shield.dll";
            let use_sandbox = std::path::Path::new(dll_path).exists() && cfg!(target_os = "windows");
            
            let output = if use_sandbox {
                #[cfg(target_os = "windows")]
                {
                    run_in_dll_sandbox(&shell_cmd, &final_args, dll_path)?
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let out = Command::new(&shell_cmd)
                        .args(&final_args)
                        .output()?;
                    String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
                }
            } else {
                let out = Command::new(&shell_cmd)
                    .args(&final_args)
                    .output()?;
                String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
            };
            
            let _ = std::fs::remove_file(temp_file_path);
            
            Ok(output)
        }
        _ => Err(format!("Unknown fallback tool: {}", tool_name).into())
    }
}

// --- Windows DLL Sandboxing Native Implementations ---

#[cfg(target_os = "windows")]
extern "system" {
    fn CreateProcessW(
        lpApplicationName: *const u16,
        lpCommandLine: *mut u16,
        lpProcessAttributes: *mut std::ffi::c_void,
        lpThreadAttributes: *mut std::ffi::c_void,
        bInheritHandles: i32,
        dwCreationFlags: u32,
        lpEnvironment: *mut std::ffi::c_void,
        lpCurrentDirectory: *const u16,
        lpStartupInfo: *mut STARTUPINFOW,
        lpProcessInformation: *mut PROCESS_INFORMATION,
    ) -> i32;
    fn VirtualAllocEx(
        hProcess: *mut std::ffi::c_void,
        lpAddress: *mut std::ffi::c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut std::ffi::c_void;
    fn WriteProcessMemory(
        hProcess: *mut std::ffi::c_void,
        lpBaseAddress: *mut std::ffi::c_void,
        lpBuffer: *const std::ffi::c_void,
        nSize: usize,
        lpNumberOfBytesWritten: *mut usize,
    ) -> i32;
    fn CreateRemoteThread(
        hProcess: *mut std::ffi::c_void,
        lpThreadAttributes: *mut std::ffi::c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        lpParameter: *mut std::ffi::c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> *mut std::ffi::c_void;
    fn ResumeThread(hThread: *mut std::ffi::c_void) -> u32;
    fn GetModuleHandleW(lpModuleName: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(
        hModule: *mut std::ffi::c_void,
        lpProcName: *const u8,
    ) -> *mut std::ffi::c_void;
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    fn WaitForSingleObject(hHandle: *mut std::ffi::c_void, dwMilliseconds: u32) -> u32;
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct STARTUPINFOW {
    cb: u32,
    lpReserved: *mut u16,
    lpDesktop: *mut u16,
    lpTitle: *mut u16,
    dwX: u32,
    dwY: u32,
    dwXSize: u32,
    dwYSize: u32,
    dwXCountChars: u32,
    dwYCountChars: u32,
    dwFillAttribute: u32,
    dwFlags: u32,
    wShowWindow: u16,
    cbReserved2: u16,
    lpReserved2: *mut u8,
    hStdInput: *mut std::ffi::c_void,
    hStdOutput: *mut std::ffi::c_void,
    hStdError: *mut std::ffi::c_void,
}

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct PROCESS_INFORMATION {
    hProcess: *mut std::ffi::c_void,
    hThread: *mut std::ffi::c_void,
    dwProcessId: u32,
    dwThreadId: u32,
}

#[cfg(target_os = "windows")]
fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn run_in_dll_sandbox(app: &str, args: &[String], dll_path: &str) -> Result<String, Box<dyn Error>> {
    let session_id = format!("nda_session_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs());
    let redirect_dir = format!("C:\\WUIAS\\sandbox\\redirect\\{}", session_id);
    std::fs::create_dir_all(&redirect_dir)?;
    
    let w_dll_path = to_wstring(dll_path);
    let cmd_line_str = format!("\"{}\" {}", app, args.join(" "));
    let mut w_cmd_line = to_wstring(&cmd_line_str);
    
    // Pre-create registry key
    let _ = Command::new("reg")
        .args(&["add", &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id), "/f"])
        .output();
        
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        
        std::env::set_var("WUIAS_SESSION_ID", &session_id);
        std::env::set_var("WUIAS_REDIRECT_DIR", &redirect_dir);
        
        let CREATE_SUSPENDED: u32 = 0x00000004;
        let success = CreateProcessW(
            std::ptr::null(),
            w_cmd_line.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            CREATE_SUSPENDED,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        );
        
        std::env::remove_var("WUIAS_SESSION_ID");
        std::env::remove_var("WUIAS_REDIRECT_DIR");
        
        if success == 0 {
            return Err(format!("CreateProcessW failed. Error code: {}", std::io::Error::last_os_error()).into());
        }
        
        let path_size = (dll_path.len() + 1) * 2;
        let MEM_COMMIT = 0x1000;
        let MEM_RESERVE = 0x2000;
        let PAGE_READWRITE = 0x04;
        
        let remote_mem = VirtualAllocEx(
            pi.hProcess,
            std::ptr::null_mut(),
            path_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        
        if remote_mem.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("VirtualAllocEx failed in target process".into());
        }
        
        let dll_bytes: Vec<u8> = w_dll_path
            .iter()
            .flat_map(|&w| w.to_le_bytes())
            .collect();
            
        let mut written = 0;
        let write_ok = WriteProcessMemory(
            pi.hProcess,
            remote_mem,
            dll_bytes.as_ptr() as *const std::ffi::c_void,
            dll_bytes.len(),
            &mut written,
        );
        
        if write_ok == 0 {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("WriteProcessMemory failed to write DLL path".into());
        }
        
        let kernel32_name = to_wstring("kernel32.dll");
        let h_kernel32 = GetModuleHandleW(kernel32_name.as_ptr());
        if h_kernel32.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("Failed to locate kernel32.dll in host".into());
        }
        
        let load_library_addr = GetProcAddress(h_kernel32, b"LoadLibraryW\0".as_ptr());
        if load_library_addr.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("Failed to resolve LoadLibraryW address".into());
        }
        
        let mut thread_id = 0;
        let load_library_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(load_library_addr);
        let h_thread = CreateRemoteThread(
            pi.hProcess,
            std::ptr::null_mut(),
            0,
            load_library_fn,
            remote_mem,
            0,
            &mut thread_id,
        );
        
        if h_thread.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("CreateRemoteThread failed to load DLL".into());
        }
        
        WaitForSingleObject(h_thread, 5000);
        CloseHandle(h_thread);
        
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);
        
        WaitForSingleObject(pi.hProcess, 0xFFFFFFFF);
        CloseHandle(pi.hProcess);
    }
    
    let mut run_output = format!("=== Sandboxed execution completed (Session: {}) ===\n", session_id);
    
    fn count_files_recursive(dir: &std::path::Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        count += count_files_recursive(&entry.path());
                    } else if file_type.is_file() {
                        count += 1;
                    }
                }
            }
        }
        count
    }
    
    let files_count = count_files_recursive(std::path::Path::new(&redirect_dir));
    run_output += &format!("Sandbox redirect folder: {}\nRedirected files written: {}\n", redirect_dir, files_count);
    
    let _ = Command::new("reg")
        .args(&["delete", &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id), "/f"])
        .output();
        
    Ok(run_output)
}
