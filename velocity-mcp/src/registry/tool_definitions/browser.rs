use crate::registry::types::Tool;
use serde_json::json;

pub fn get_browser_tools() -> Vec<Tool> {
    vec![
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
            name: "runtime_reseed_auth".to_string(),
            description: "Copy auth cookies plus CSRF-relevant storage from a source browser session or checkpoint into a target explicit runtime session, then report the resulting runtime auth diagnosis.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "targetSessionId": { "type": "string", "description": "Persisted runtime session identifier to update with recovered auth state." },
                    "sourceSessionId": { "type": "string", "description": "Source browser session identifier to copy auth state from." },
                    "sourceCheckpointName": { "type": "string", "description": "Optional checkpoint name on the source session; when provided, copy from that checkpoint instead of the live source session." },
                    "waitTimeoutMs": { "type": "integer", "minimum": 1, "description": "Optional post-apply wait timeout in milliseconds for the Go runtime state-apply endpoint." },
                    "compact": { "type": "boolean", "description": "When true, return a structured runtime auth reseed report instead of verbose multiline text." }
                },
                "required": ["targetSessionId", "sourceSessionId"]
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
            name: "browser_get_trace_summary".to_string(),
            description: "Read compact runtime trace summary for browser operations including console messages, network activity, DOM mutations, screenshots, and health warnings.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "compact": { "type": "boolean", "description": "When true, return structured JSON trace summary; otherwise render operator summary text." }
                }
            }),
        },
        Tool {
            name: "browser_get_trace_logs".to_string(),
            description: "Read rich runtime trace entries for browser operations.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "compact": { "type": "boolean", "description": "When true, return structured JSON trace log array; otherwise render formatted trace entry lines." }
                }
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
        Tool {
            name: "browser_native_navigate".to_string(),
            description: "Navigate the native pure-Rust browser engine to a URL over HTTPS, load it into the live DOM, and return the readable AOM view plus the NDA delta the navigation produced. Session state (DOM, cookies, storage) persists across native tool calls sharing the same sessionId.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Stable session id identifying the live native browser session." },
                    "url": { "type": "string", "description": "Absolute URL to navigate to (http/https)." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report (status, delta, view) instead of readable text." }
                },
                "required": ["sessionId", "url"]
            }),
        },
        Tool {
            name: "browser_native_read".to_string(),
            description: "Read the current live Agentic Object Model of a native browser session: URL, title, and every actionable element with its node id, role, accessible name, value, and actionability score. Use the node ids or role+name here to target native click/type/select/submit actions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session to read." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON view report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_click".to_string(),
            description: "Click an element in the live native browser DOM, targeted by node id or by role + accessible name, and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "nodeId": { "type": "string", "description": "Target node id (accepts 5 or node_5). If omitted, role/name resolution is used." },
                    "role": { "type": "string", "description": "Optional AOM role filter (button, link, textbox, ...) used with name." },
                    "name": { "type": "string", "description": "Accessible name to resolve when nodeId is not given (case-insensitive, exact then substring)." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_type".to_string(),
            description: "Type text into an input-like element in the live native browser DOM, targeted by node id or role + accessible name, and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "nodeId": { "type": "string", "description": "Target node id (accepts 5 or node_5). If omitted, role/name resolution is used." },
                    "role": { "type": "string", "description": "Optional AOM role filter used with name." },
                    "name": { "type": "string", "description": "Accessible name to resolve when nodeId is not given." },
                    "text": { "type": "string", "description": "Text to enter into the element's value." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "text"]
            }),
        },
        Tool {
            name: "browser_native_select".to_string(),
            description: "Set the selected value of a combobox/select element in the live native browser DOM, targeted by node id or role + accessible name, and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "nodeId": { "type": "string", "description": "Target node id (accepts 5 or node_5). If omitted, role/name resolution is used." },
                    "role": { "type": "string", "description": "Optional AOM role filter used with name." },
                    "name": { "type": "string", "description": "Accessible name to resolve when nodeId is not given." },
                    "value": { "type": "string", "description": "Value to select." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "value"]
            }),
        },
        Tool {
            name: "browser_native_submit".to_string(),
            description: "Submit a form (or form control) in the live native browser DOM, targeted by node id or role + accessible name, and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "nodeId": { "type": "string", "description": "Target node id (accepts 5 or node_5). If omitted, role/name resolution is used." },
                    "role": { "type": "string", "description": "Optional AOM role filter used with name." },
                    "name": { "type": "string", "description": "Accessible name to resolve when nodeId is not given." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_scroll".to_string(),
            description: "Scroll the native browser viewport by a pixel delta and return the resulting NDA delta (the session scroll fact plus any nodes whose in-viewport visibility flipped) and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "deltaX": { "type": "integer", "description": "Horizontal scroll delta in pixels (positive = right)." },
                    "deltaY": { "type": "integer", "description": "Vertical scroll delta in pixels (positive = down). Defaults to 0." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_scroll_into_view".to_string(),
            description: "Scroll an element into the native browser viewport by its accessible name (button/link text, label, aria-label) and return the NDA delta showing which nodes entered or left the viewport, plus the refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "label": { "type": "string", "description": "Accessible name of the element to bring into view." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "label"]
            }),
        },
        Tool {
            name: "browser_native_remember".to_string(),
            description: "Index the current page of the native browser session into vector memory (title + visible text + optional note, TF-IDF embedded) so it can be recalled later by meaning, keyword, or tag without re-crawling. Returns the memory id and what was indexed.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Categorical tags for later tag-mode recall (e.g. [\"login\", \"checkout\"])." },
                    "outcome": { "type": "number", "description": "Outcome score 0.0-1.0 recording how well the interaction on this page went. Defaults to 0." },
                    "note": { "type": "string", "description": "Optional free-text note indexed alongside the page text." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_recall".to_string(),
            description: "Recall pages previously stored with browser_native_remember. Modes: semantic (TF-IDF cosine similarity, scored), keyword (substring over text/url), tag (exact tag match), similar (query is a memory id; finds pages most similar to that memory, scored). Set minOutcome to only recall pages whose interaction outcome scored at least that high — i.e. recall what worked. Each hit lists memory id, url, similarity, tags, outcome, and a text snippet.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "query": { "type": "string", "description": "Search text, the tag name in tag mode, or the memory id in similar mode." },
                    "mode": { "type": "string", "enum": ["semantic", "keyword", "tag", "similar"], "description": "Recall strategy. Defaults to semantic." },
                    "limit": { "type": "integer", "description": "Maximum hits to return. Defaults to 5." },
                    "minOutcome": { "type": "number", "description": "Only return memories with outcome score >= this value (0.0..=1.0). Defaults to 0 (no filter)." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId", "query"]
            }),
        },
        Tool {
            name: "browser_native_page_text".to_string(),
            description: "Read the visible text of the current page in the native browser session: title + body text in reading order, whitespace collapsed, script/style content skipped. The token-cheapest way to read a whole page. Set maxChars to bound the output on huge pages.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "maxChars": { "type": "integer", "description": "Truncate the text to this many characters (0 or omitted = no limit)." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_screencast".to_string(),
            description: "Structural screencast of the native browser session: frames record the page's shape (viewport size, AOM element count, content hash) instead of pixels. Actions: capture (record a frame of the current state), list (show the frame timeline), save (persist the timeline as JSON under .velocity/browser_artifacts/screencasts/). Diff frame hashes to see when the page actually changed.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "action": { "type": "string", "enum": ["capture", "list", "save"], "description": "Screencast operation. Defaults to capture." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_find".to_string(),
            description: "Query the live Agentic Object Model of the native browser session by role and/or a case-insensitive text match over accessible names and values. Returns only the matching elements with their node ids — far cheaper than reading the whole element view on big pages. At least one of role or text is required.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "role": { "type": "string", "description": "AOM role to filter by (button, link, textbox, combobox, checkbox, ...)." },
                    "text": { "type": "string", "description": "Case-insensitive substring matched against each element's accessible name and value." },
                    "limit": { "type": "integer", "description": "Maximum hits to return. Defaults to 20." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_validate".to_string(),
            description: "Run HTML5 constraint validation over every form control on the current page of the native browser session (required, email/url/number type checks, pattern, minlength/maxlength, min/max range). Reports which controls would block a submit and why — check before submitting instead of burning a failed round trip.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_links".to_string(),
            description: "List the current page's navigation map in the native browser session: every link's text and href target in document order, with the node id to click. Optional case-insensitive filter over link text and href. The token-cheap answer to \"where can I go from here\".".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "filter": { "type": "string", "description": "Case-insensitive substring matched against link text and href." },
                    "limit": { "type": "integer", "description": "Maximum links to return. Defaults to 50." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_history".to_string(),
            description: "List the native browser session's navigation history stack: every visited url with its page title in stack order, marking the entry the session currently points at. The token-cheap answer to \"where have I been\" — pairs with browser_native_back/forward.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_checkpoint".to_string(),
            description: "Named page-state checkpoints in the native browser session. action=save snapshots the current state under a name; action=diff reports everything that changed since that snapshot as one NDA delta (spanning any number of actions); action=list and action=drop manage saved checkpoints.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "action": { "type": "string", "description": "One of save, diff, list, drop. Defaults to save." },
                    "name": { "type": "string", "description": "Checkpoint name. Required for save, diff, and drop." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_reflect".to_string(),
            description: "Self-reflection over recent native browser actions: every action is scored from the NDA delta it actually produced, and this tool reports detected failure patterns (repeated failures, navigation loops, blocked clicks) with suggested strategy adjustments, plus the recent outcome scores.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "recent": { "type": "integer", "description": "How many recent action outcomes to include as context. Defaults to 5." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_predict".to_string(),
            description: "Suggest the next best action on the current page using learned per-domain confidence: outcome scores from past native actions teach which (element role, action) combinations work on this domain, and the highest-confidence actionable element is proposed. Also reports the learned patterns for the domain.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_brief".to_string(),
            description: "One-call pre-action context bundle: page identity, suggested next action, learned per-domain patterns, semantically similar remembered pages, failure lessons and recent action outcomes. Replaces separate predict + recall + reflect calls, saving tokens.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "memories": { "type": "integer", "description": "Max similar remembered pages to include (default 3)." },
                    "recent": { "type": "integer", "description": "Max recent action outcomes to include (default 5)." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_learn".to_string(),
            description: "Persist or restore the session's experience stores as NDA artifacts under .velocity/browser_artifacts/, so what one session learned improves later ones. what=confidence (default) is the learned per-domain action confidence; what=memory is the vector page memory (remembered pages); what=outcomes is the scored action-outcome history that feeds browser_native_reflect; what=all bundles all three stores into a single artifact. action=save exports the store; action=load imports a previously saved artifact into the current session; action=list enumerates every saved artifact (file, kind, size) so an agent can discover inheritable experience. Saving with file=default_all.nda publishes the bundle as the workspace default: every new session auto-inherits it on first use.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "action": { "type": "string", "description": "save (default) to persist the store, load to restore it, list to enumerate saved artifacts." },
                    "what": { "type": "string", "description": "Which store: confidence (default), memory, outcomes or all." },
                    "file": { "type": "string", "description": "Artifact file name (default {sessionId}_{what}.nda). Pass another session's file to inherit its experience." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_back".to_string(),
            description: "Navigate the native browser session back to the previous page in its history stack and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_forward".to_string(),
            description: "Navigate the native browser session forward in its history stack and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_eval".to_string(),
            description: "Evaluate a JavaScript expression in the live native browser session and return the result alongside the refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "expression": { "type": "string", "description": "JavaScript expression to evaluate." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON report instead of readable text." }
                },
                "required": ["sessionId", "expression"]
            }),
        },
        // -- Phase 5: Enhanced agent tools --
        Tool {
            name: "browser_native_wait_for".to_string(),
            description: "Poll the live AOM until an element matching the given role and/or accessible name appears (with timeout). Returns the matched node id and refreshed view, or a not-found message.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "role": { "type": "string", "description": "Optional AOM role filter (button, link, textbox, ...)." },
                    "name": { "type": "string", "description": "Accessible name to wait for (case-insensitive)." },
                    "timeout": { "type": "integer", "description": "Maximum wait time in milliseconds (default 5000)." },
                    "compact": { "type": "boolean", "description": "When true, return JSON instead of readable text." }
                },
                "required": ["sessionId", "name"]
            }),
        },
        Tool {
            name: "browser_native_extract".to_string(),
            description: "Extract content from a DOM element: its text content, innerHTML, outerHTML, or a specific attribute value.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "nodeId": { "type": "string", "description": "Target node id." },
                    "role": { "type": "string", "description": "Optional AOM role filter for name resolution." },
                    "name": { "type": "string", "description": "Accessible name for target resolution." },
                    "what": { "type": "string", "description": "What to extract: 'text', 'html', 'outerHTML', or 'attr:NAME' (default: text)." },
                    "compact": { "type": "boolean", "description": "When true, return JSON." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_cookies".to_string(),
            description: "Get, set, or delete cookies for the current session/origin.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id." },
                    "operation": { "type": "string", "description": "One of: 'get', 'set', 'delete' (default: get)." },
                    "name": { "type": "string", "description": "Cookie name." },
                    "value": { "type": "string", "description": "Cookie value (for set)." },
                    "domain": { "type": "string", "description": "Cookie domain (for set)." }
                },
                "required": ["sessionId", "name"]
            }),
        },
        Tool {
            name: "browser_native_storage".to_string(),
            description: "Get, set, or clear localStorage/sessionStorage for the current session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id." },
                    "storageType": { "type": "string", "description": "'local' or 'session' (default: local)." },
                    "operation": { "type": "string", "description": "One of: 'get', 'set', 'clear' (default: get)." },
                    "key": { "type": "string", "description": "Storage key." },
                    "value": { "type": "string", "description": "Storage value (for set)." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_network".to_string(),
            description: "List recent network requests (fetch/XHR) made during the session with URL, method, status, and resource type.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id." },
                    "compact": { "type": "boolean", "description": "When true, return JSON array." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_screenshot".to_string(),
            description: "Serialize the current DOM as a structured text snapshot showing URL, title, DOM node count, and all actionable AOM elements. A semantic representation, not pixels.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_hover".to_string(),
            description: "Hover an element (fire mouseenter/mouseover events) targeted by node id or role + accessible name.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id." },
                    "nodeId": { "type": "string", "description": "Target node id." },
                    "role": { "type": "string", "description": "Optional AOM role filter." },
                    "name": { "type": "string", "description": "Accessible name for resolution." },
                    "compact": { "type": "boolean", "description": "When true, return JSON." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_press_key".to_string(),
            description: "Press a keyboard key (fire keydown/keypress/keyup events) in the session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id." },
                    "key": { "type": "string", "description": "Key to press (e.g. 'Enter', 'Escape', 'Tab', 'a')." },
                    "compact": { "type": "boolean", "description": "When true, return JSON." }
                },
                "required": ["sessionId", "key"]
            }),
        },
        // -- Label-based semantic actions: target elements by what the agent
        //    reads on screen, no node ids needed --
        Tool {
            name: "browser_native_click_text".to_string(),
            description: "Click the clickable element (button, link, checkbox, radio) whose visible text or accessible name best matches the query. Exact matches beat substring matches; ties break on actionability. Returns the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "text": { "type": "string", "description": "Visible text or accessible name of the element to click (case-insensitive)." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "text"]
            }),
        },
        Tool {
            name: "browser_native_fill_label".to_string(),
            description: "Fill the text input or textarea whose label, placeholder, or accessible name best matches the query. Returns the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "label": { "type": "string", "description": "Label/placeholder/accessible name of the control to fill (case-insensitive)." },
                    "text": { "type": "string", "description": "Text to enter into the control's value." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "label", "text"]
            }),
        },
        Tool {
            name: "browser_native_check_label".to_string(),
            description: "Check or uncheck the checkbox/radio whose label best matches the query and return the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "label": { "type": "string", "description": "Label/accessible name of the checkbox or radio (case-insensitive)." },
                    "checked": { "type": "boolean", "description": "Desired state: true to check (default), false to uncheck." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "label"]
            }),
        },
        Tool {
            name: "browser_native_select_label".to_string(),
            description: "Pick an option in the select/combobox whose label best matches the query. The option is matched by its visible text or value (exact beats substring). Returns the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "label": { "type": "string", "description": "Label/accessible name of the select control (case-insensitive)." },
                    "option": { "type": "string", "description": "Visible text or value of the option to select." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "label", "option"]
            }),
        },
        Tool {
            name: "browser_native_focus_label".to_string(),
            description: "Move session keyboard focus to the focusable element whose accessible name best matches the query. Focus is a readable fact (AOM focused) so the delta shows exactly where focus moved. Follow with browser_native_press to type or submit.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "label": { "type": "string", "description": "Accessible name of the element to focus (case-insensitive)." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "label"]
            }),
        },
        Tool {
            name: "browser_native_press".to_string(),
            description: "Press a key against the session's focused element: 'Enter' submits the enclosing form, 'Tab' advances focus to the next control (wrapping), and a single character types into the focused control. Requires focus set via browser_native_focus_label. Returns the resulting NDA delta and refreshed AOM view.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "key": { "type": "string", "description": "Key to press: 'Enter', 'Tab', or a single character." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId", "key"]
            }),
        },
        Tool {
            name: "browser_native_read_form".to_string(),
            description: "Read every form control on the page as compact text: one line per control with its accessible name, role, and current value or checked state. The cheapest way to verify form state before submitting.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_observe".to_string(),
            description: "Dump the full readable fact base of the session (URL, title, AOM roles/names/values, focus, layout, cookies, storage) as 'subject predicate = object' lines. The complete observation an agent can diff or reason over.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_settle".to_string(),
            description: "Flush pending timers and microtasks in the session's JS runtime and return the NDA delta of everything that changed while settling. Call after actions that trigger async work (setTimeout, fetch callbacks).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON action report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_export_nda".to_string(),
            description: "Persist the live session's state as an NDA artifact under .velocity/browser_artifacts/. format=binary (default) writes the 18-byte hashed triple stream ({sessionId}_native.nda); format=readable writes and returns the lossless fact text ({sessionId}_facts.txt); format=trace writes console/mutation/performance/network traces ({sessionId}_trace.nda). Returns the artifact path and fact count.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "format": { "type": "string", "enum": ["binary", "readable", "trace"], "description": "Artifact format: binary NDA triples (default), readable fact text, or trace stream." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON export report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_tab_open".to_string(),
            description: "Open a new blank background tab in the live native browser session. Returns the refreshed tab list. Use browser_native_tab_switch to bring it to the foreground before acting on it.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "tabId": { "type": "string", "description": "Unique id for the new tab; must not collide with an existing tab." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON tab report instead of readable text." }
                },
                "required": ["sessionId", "tabId"]
            }),
        },
        Tool {
            name: "browser_native_tab_list".to_string(),
            description: "List every tab in the live native browser session with its id, title, URL, and which one is the active (foreground) tab that all other browser_native_* tools act on.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON tab report instead of readable text." }
                },
                "required": ["sessionId"]
            }),
        },
        Tool {
            name: "browser_native_tab_switch".to_string(),
            description: "Bring the named tab to the foreground; the previous foreground tab is parked in the background with its full state (DOM, cookies, focus, traces). Returns the tab list plus the view of the newly active tab.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "tabId": { "type": "string", "description": "Id of the tab to bring to the foreground." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON tab report instead of readable text." }
                },
                "required": ["sessionId", "tabId"]
            }),
        },
        Tool {
            name: "browser_native_tab_close".to_string(),
            description: "Close a background tab and drop its state. The active tab cannot be closed - switch to another tab first. Returns the refreshed tab list.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Session id of the live native browser session." },
                    "tabId": { "type": "string", "description": "Id of the background tab to close." },
                    "compact": { "type": "boolean", "description": "When true, return a JSON tab report instead of readable text." }
                },
                "required": ["sessionId", "tabId"]
            }),
        },
    ]
}
