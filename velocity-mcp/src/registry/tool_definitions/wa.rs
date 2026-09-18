use crate::registry::types::Tool;
use serde_json::json;

/// Bug #40: appended to every tool that drives the interactive session instead
/// of a target the caller names. A caller should learn about the opt-in from
/// `tools/list`, before it disrupts whoever is sitting at the keyboard - not
/// from the side effect of the call that first tried.
const SESSION_GATE_NOTE: &str = "Opt-in only: it changes the session of whoever is at the keyboard (switches or creates virtual desktops, or injects keystrokes into the focused window) rather than acting on a target you name, so it is refused unless the server runs with VELOCITY_WA_ALLOW_SESSION_CHANGE=1.";

/// Bug #40: the milder family - tools that mutate state the operator shares
/// with every window (the clipboard) or place a new window in front of them
/// (starting a program), without ever moving them to another desktop.
const SESSION_EFFECT_NOTE: &str = "Opt-in only: it acts on state you share with every other window, or puts a new window in front of you, rather than on a target you name, so it is refused unless the server runs with VELOCITY_WA_ALLOW_SESSION_EFFECT=1.";

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
            description: "Capture a live Windows accessibility snapshot via UIAutomation and persist it as a WA snapshot. Read-only: it walks the target window's tree and synthesises no input.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier. The session must already exist - create it with wa_create_session." },
                    "snapshotName": { "type": "string", "description": "Logical name for the captured snapshot." },
                    "title": { "type": "string", "description": "Optional title override for the captured snapshot." },
                    "processId": { "type": "integer", "minimum": 1, "description": "Optional target process id. When omitted the foreground window is captured; when supplied but no top-level window belongs to it, the capture is refused and the live windows are listed rather than a substitute being captured." },
                    "windowNameContains": { "type": "string", "description": "Optional case-insensitive window title substring filter. Refused on no match, like processId." },
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
        // ─── Clipboard Tools ─────────────────────────────────────────────────────
        Tool {
            name: "wa_clipboard_read".to_string(),
            description: "Read the current Windows clipboard content (text, HTML, files, or image detection).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["text", "html", "files", "auto"], "description": "Clipboard format to read. Defaults to auto." }
                }
            }),
        },
        Tool {
            name: "wa_clipboard_write".to_string(),
            description: format!("Write content to the Windows clipboard. {SESSION_EFFECT_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text content to write to clipboard." },
                    "html": { "type": "string", "description": "HTML content to write to clipboard." },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "File paths to place on clipboard." }
                }
            }),
        },
        Tool {
            name: "wa_clipboard_clear".to_string(),
            description: format!("Clear the Windows clipboard. {SESSION_EFFECT_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── Process Management Tools ────────────────────────────────────────────
        Tool {
            name: "wa_process_launch".to_string(),
            description: format!("Launch a Windows process with optional arguments, working directory, and elevation. {SESSION_EFFECT_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "exePath": { "type": "string", "description": "Path to executable." },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "Command line arguments." },
                    "workingDir": { "type": "string", "description": "Working directory." },
                    "hidden": { "type": "boolean", "description": "Start hidden (no window)." },
                    "elevated": { "type": "boolean", "description": "Run as administrator." },
                    "waitForWindow": { "type": "boolean", "description": "Wait for main window to appear." }
                },
                "required": ["exePath"]
            }),
        },
        Tool {
            name: "wa_process_terminate".to_string(),
            description: "Gracefully terminate a Windows process by PID.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID to terminate." },
                    "graceMs": { "type": "integer", "description": "Grace period in ms before force kill. Defaults to 5000." }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "wa_process_list".to_string(),
            description: "List running Windows processes, optionally filtered by name.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nameContains": { "type": "string", "description": "Optional case-insensitive process name filter." },
                    "limit": { "type": "integer", "description": "Max results to return." }
                }
            }),
        },
        // ─── Window Management Tools ─────────────────────────────────────────────
        Tool {
            name: "wa_window_list".to_string(),
            description: "List visible desktop windows with titles, positions, and states.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "titleContains": { "type": "string", "description": "Optional case-insensitive title substring filter." },
                    "pid": { "type": "integer", "minimum": 1, "description": "Optional process id: list only windows owned by this pid." },
                    "className": { "type": "string", "description": "Optional window class name: list only windows of this class." }
                }
            }),
        },
        Tool {
            name: "wa_window_action".to_string(),
            description: format!("Perform a window operation (move, resize, minimize, maximize, close, focus, topmost). 'focus' asks the shell for the foreground, which can move you onto the desktop owning that window: {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": { "type": "integer", "description": "Window handle." },
                    "action": { "type": "string", "enum": ["move", "resize", "move_resize", "minimize", "maximize", "restore", "close", "focus", "send_to_back", "topmost", "untopmost", "opacity"], "description": "Operation to perform." },
                    "x": { "type": "integer", "description": "X position (for move / move_resize)." },
                    "y": { "type": "integer", "description": "Y position (for move / move_resize)." },
                    "width": { "type": "integer", "description": "Width (for resize / move_resize)." },
                    "height": { "type": "integer", "description": "Height (for resize / move_resize)." },
                    "opacity": { "type": "integer", "description": "Opacity 0-255 (for opacity action; 255 = opaque)." }
                },
                "required": ["hwnd", "action"]
            }),
        },
        // ─── Virtual Desktop Tools ───────────────────────────────────────────────
        Tool {
            name: "wa_virtual_desktop_list".to_string(),
            description: "List all Windows virtual desktops.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_virtual_desktop_switch".to_string(),
            description: format!("Switch to a virtual desktop by index or name. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "description": "Desktop index (0-based)." },
                    "name": { "type": "string", "description": "Desktop name (Windows 11)." }
                }
            }),
        },
        // ─── OCR Tools ───────────────────────────────────────────────────────────
        Tool {
            name: "wa_ocr_screen".to_string(),
            description: "Perform OCR text recognition on a screen region, a full screen, or a specific window.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1, "description": "Optional process id: recognise that window's contents instead of a screen region." },
                    "x": { "type": "integer", "description": "Region X offset (pixels)." },
                    "y": { "type": "integer", "description": "Region Y offset (pixels)." },
                    "width": { "type": "integer", "description": "Region width." },
                    "height": { "type": "integer", "description": "Region height." },
                    "language": { "type": "string", "description": "OCR language tag (e.g. en-US). Defaults to system language." }
                }
            }),
        },
        // ─── Notification Tools ──────────────────────────────────────────────────
        Tool {
            name: "wa_notifications_list".to_string(),
            description: "List currently visible Windows toast notifications.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_notifications_dismiss".to_string(),
            description: format!("Dismiss visible Windows notifications, optionally filtered by pattern. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Optional wildcard pattern to match notification title/body." }
                }
            }),
        },
        // ─── Registry Tools ──────────────────────────────────────────────────────
        Tool {
            name: "wa_registry_read".to_string(),
            description: "Read a Windows registry value.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hive": { "type": "string", "enum": ["HKCU", "HKLM", "HKCR", "HKU", "HKCC"], "description": "Registry hive." },
                    "path": { "type": "string", "description": "Registry key path." },
                    "name": { "type": "string", "description": "Value name." }
                },
                "required": ["hive", "path", "name"]
            }),
        },
        Tool {
            name: "wa_registry_write".to_string(),
            description: "Write a Windows registry value.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hive": { "type": "string", "enum": ["HKCU", "HKLM", "HKCR", "HKU", "HKCC"], "description": "Registry hive." },
                    "path": { "type": "string", "description": "Registry key path." },
                    "name": { "type": "string", "description": "Value name." },
                    "value": { "type": "string", "description": "Value to write." },
                    "type": { "type": "string", "enum": ["String", "DWord", "QWord", "ExpandString", "Binary", "MultiString"], "description": "Registry value type." }
                },
                "required": ["hive", "path", "name", "value", "type"]
            }),
        },
        // ─── System Settings Tools ───────────────────────────────────────────────
        Tool {
            name: "wa_system_dark_mode".to_string(),
            description: "Get or toggle Windows dark mode.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean", "description": "Set dark mode. Omit to just query current state." }
                }
            }),
        },
        // ─── Trigger Tools ───────────────────────────────────────────────────────
        Tool {
            name: "wa_trigger_register".to_string(),
            description: "Register a new automation trigger (file watch, window appears, idle detect, etc.).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Optional stable trigger id. Auto-generated from the registration timestamp when omitted." },
                    "name": { "type": "string", "description": "Trigger name." },
                    "kind": { "type": "string", "enum": ["file_watch", "window_appears", "window_closes", "process_starts", "process_exits", "clipboard_changed", "system_idle", "delay", "interval"], "description": "Trigger type." },
                    "target": { "type": "string", "description": "Target path/title/name/pid depending on kind." },
                    "actionScript": { "type": "string", "description": "PowerShell script to execute when triggered." },
                    "enabled": { "type": "boolean", "description": "Whether the trigger starts enabled. Defaults to true." },
                    "durationMs": { "type": "integer", "minimum": 0, "description": "Delay/interval/idle-threshold in ms for the delay, interval, and system_idle kinds. Defaults to 1000." },
                    "maxFires": { "type": "integer", "minimum": 1, "description": "Optional maximum number of times the trigger may fire." }
                },
                "required": ["name", "kind", "actionScript"]
            }),
        },
        Tool {
            name: "wa_trigger_list".to_string(),
            description: "List all registered automation triggers.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_trigger_fire".to_string(),
            description: "Manually fire a registered trigger by ID, executing its action script immediately.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "triggerId": { "type": "string", "description": "The ID of the trigger to fire." }
                },
                "required": ["triggerId"]
            }),
        },
        Tool {
            name: "wa_trigger_remove".to_string(),
            description: "Remove a registered automation trigger by ID.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "triggerId": { "type": "string", "description": "The ID of the trigger to remove." }
                },
                "required": ["triggerId"]
            }),
        },
        // ─── Recovery Tools ─────────────────────────────────────────────────────
        Tool {
            name: "wa_recovery_set_policy".to_string(),
            description: "Configure the retry/recovery policy for WA operations (max retries, backoff, circuit breaker).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "maxRetries": { "type": "integer", "description": "Maximum retry attempts. Default 3." },
                    "baseDelayMs": { "type": "integer", "description": "Base delay between retries in ms. Default 500." },
                    "circuitBreakerThreshold": { "type": "integer", "description": "Failures before circuit opens. Default 5." }
                }
            }),
        },
        Tool {
            name: "wa_recovery_get_status".to_string(),
            description: "Get the current recovery/circuit-breaker status for WA operations.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── Event Subscription Tools ───────────────────────────────────────────
        Tool {
            name: "wa_event_subscribe".to_string(),
            description: "Block and capture Windows UI Automation events by polling: window_opened, window_closed, element_focus and structure_changed are supported; element_value_changed is not capturable by the polled listener and is rejected.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "eventKind": { "type": "string", "enum": ["window_opened", "window_closed", "element_focus", "structure_changed", "element_value_changed"], "description": "Type of UIA event to capture. The four polled kinds are capturable; element_value_changed returns an explicit error." },
                    "processId": { "type": "integer", "description": "Optional PID filter." },
                    "timeoutMs": { "type": "integer", "description": "Listen duration in ms. Default 5000, capped by the script budget." }
                },
                "required": ["eventKind"]
            }),
        },
        Tool {
            name: "wa_event_poll".to_string(),
            description: "Poll buffered UIA events that have been captured since last poll.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "maxEvents": { "type": "integer", "description": "Maximum events to return. Default 20." }
                }
            }),
        },
        Tool {
            name: "wa_event_unsubscribe".to_string(),
            description: "Unsubscribe from UIA events and stop the listener.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── File Dialog Tools ──────────────────────────────────────────────────
        Tool {
            name: "wa_file_dialog_detect".to_string(),
            description: "Detect a live file dialog and report its handle, owner process, title and kind (open / save_as / folder_browse).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "processId": { "type": "integer", "description": "Optional PID of the process owning the dialog." },
                    "titleContains": { "type": "string", "description": "Optional case-insensitive title filter. Defaults to Open|Save|Browse|Select." },
                    "timeoutMs": { "type": "integer", "minimum": 0, "description": "How long to wait for a dialog to appear, in milliseconds. Defaults to 5000." }
                }
            }),
        },
        Tool {
            name: "wa_file_dialog_open".to_string(),
            description: "Interact with an open file dialog: set the file path and confirm.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Full path to set in the file dialog." },
                    "processId": { "type": "integer", "description": "Optional PID of the process owning the dialog." }
                },
                "required": ["filePath"]
            }),
        },
        Tool {
            name: "wa_file_dialog_save".to_string(),
            description: "Interact with a save file dialog: set the file path and confirm.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Full path to set in the save dialog." },
                    "processId": { "type": "integer", "description": "Optional PID of the process owning the dialog." }
                },
                "required": ["filePath"]
            }),
        },
        // ─── Virtual Desktop Extended Tools ─────────────────────────────────────
        Tool {
            name: "wa_vdesktop_create".to_string(),
            description: format!("Create a new Windows virtual desktop. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Optional name for the new desktop (Windows 11)." }
                }
            }),
        },
        Tool {
            name: "wa_vdesktop_remove".to_string(),
            description: format!("Remove a Windows virtual desktop by index. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "description": "Index of the desktop to remove." }
                },
                "required": ["index"]
            }),
        },
        Tool {
            name: "wa_vdesktop_move_window".to_string(),
            description: format!("Move a window to a different virtual desktop. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": { "type": "integer", "description": "Window handle to move." },
                    "targetIndex": { "type": "integer", "description": "Target desktop index." }
                },
                "required": ["hwnd", "targetIndex"]
            }),
        },
        // ─── Window Tiling Tools ──────────────────────────────────────────────
        Tool {
            name: "wa_window_tile".to_string(),
            description: "Tile an explicitly named set of windows in a grid on one monitor. `hwnds` is required: this tool will not enumerate and rearrange every window on the desktop. Handles must come from wa_window_list.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnds": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 1 },
                        "minItems": 1,
                        "description": "Window handles to tile, placed left-to-right then top-to-bottom in the order given. Handles that are no longer live windows are skipped and reported in `windows_skipped`."
                    },
                    "columns": { "type": "integer", "minimum": 1, "description": "Number of columns in the tile grid. Defaults to a square-ish layout (ceil(sqrt(n))) and is clamped to the number of handles supplied." },
                    "monitor": { "type": "integer", "minimum": 0, "description": "Monitor index to tile on. Default 0. An index with no monitor attached is an error; it does not fall back to the primary display. See wa_monitor_list for valid indices." }
                },
                "required": ["hwnds"]
            }),
        },
        Tool {
            name: "wa_monitor_list".to_string(),
            description: "Enumerate all connected monitors with bounds, work area, DPI scaling, physical resolution, refresh rate and colour depth.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── Browser Bridge Tools ───────────────────────────────────────────────
        Tool {
            name: "wa_browser_navigate".to_string(),
            description: format!("Navigate the browser bridge to a URL (launches browser if needed). {SESSION_EFFECT_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to navigate to." },
                    "browser": { "type": "string", "enum": ["chrome", "edge", "firefox"], "description": "Browser to use. Default edge." }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "wa_browser_screenshot".to_string(),
            description: "Capture a screenshot of the browser window via the bridge.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "outputPath": { "type": "string", "description": "Path to save the screenshot; the encoded format follows the file extension (.png, .bmp, .jpg/.jpeg, .gif, .webp). Default 'browser_screenshot.png'." }
                }
            }),
        },
        // ─── Process Management (extended) ───────────────────────────────────
        Tool {
            name: "wa_process_kill".to_string(),
            description: "Force-kill a process immediately by PID (no graceful shutdown).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1, "description": "Process id to kill." }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "wa_process_kill_tree".to_string(),
            description: "Kill a process and all of its descendant processes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1, "description": "Root process id of the tree to kill." }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "wa_process_running".to_string(),
            description: "Check whether a process with the given PID is currently running.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1, "description": "Process id to check." }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "wa_process_info".to_string(),
            description: "Get detailed information about a single process by PID.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1, "description": "Process id to inspect." }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "wa_process_wait".to_string(),
            description: "Wait for a process condition (exit by default, or a window title to appear) up to a timeout.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1, "description": "Process id to wait on." },
                    "timeoutMs": { "type": "integer", "minimum": 0, "description": "Maximum wait time in milliseconds. Default 5000." },
                    "windowTitleContains": { "type": "string", "description": "Optional: wait for a window whose title contains this substring instead of process exit." }
                },
                "required": ["pid"]
            }),
        },
        // ─── UIA Direct (cached-tree lookup / invoke) ─────────────────────────
        Tool {
            name: "wa_uia_tree".to_string(),
            description: "Build the cached UIAutomation tree for a process via direct COM (fast path) and report its size.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "processId": { "type": "integer", "minimum": 1, "description": "Target process id." },
                    "maxDepth": { "type": "integer", "minimum": 0, "description": "Maximum traversal depth. Default 4." },
                    "maxChildren": { "type": "integer", "minimum": 1, "description": "Maximum children inspected per node. Default 64." }
                },
                "required": ["processId"]
            }),
        },
        Tool {
            name: "wa_uia_lookup".to_string(),
            description: "Look up a UIAutomation element in a process's cached tree by automationId, name, or screen point (x+y).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "processId": { "type": "integer", "minimum": 1, "description": "Target process id." },
                    "automationId": { "type": "string", "description": "AutomationId to look up (exact match)." },
                    "name": { "type": "string", "description": "Name to look up (may return multiple)." },
                    "x": { "type": "number", "description": "Screen x coordinate for point lookup." },
                    "y": { "type": "number", "description": "Screen y coordinate for point lookup." },
                    "maxDepth": { "type": "integer", "minimum": 0, "description": "Maximum traversal depth. Default 4." },
                    "maxChildren": { "type": "integer", "minimum": 1, "description": "Maximum children inspected per node. Default 64." }
                },
                "required": ["processId"]
            }),
        },
        Tool {
            name: "wa_uia_invoke".to_string(),
            description: "Invoke a UIAutomation pattern (e.g. Invoke, Value, Toggle) on an element targeted by automationId, name, or x+y.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "processId": { "type": "integer", "minimum": 1, "description": "Target process id." },
                    "pattern": { "type": "string", "description": "Pattern name: Invoke, Value, RangeValue, Selection, SelectionItem, Toggle, ExpandCollapse, Scroll, Transform, Window, etc." },
                    "value": { "type": "string", "description": "Optional value for value-bearing patterns (e.g. Value)." },
                    "automationId": { "type": "string", "description": "Target element AutomationId." },
                    "name": { "type": "string", "description": "Target element name (first match)." },
                    "x": { "type": "number", "description": "Screen x coordinate for point targeting." },
                    "y": { "type": "number", "description": "Screen y coordinate for point targeting." },
                    "maxDepth": { "type": "integer", "minimum": 0, "description": "Maximum traversal depth. Default 4." },
                    "maxChildren": { "type": "integer", "minimum": 1, "description": "Maximum children inspected per node. Default 64." }
                },
                "required": ["processId", "pattern"]
            }),
        },
        Tool {
            name: "wa_uia_root".to_string(),
            description: "Read the root UIAutomation element for a process or the desktop, with explicit control over the cached tree.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "processId": { "type": "integer", "minimum": 1, "description": "Target process id." },
                    "scope": { "type": "string", "enum": ["process", "desktop"], "description": "Root scope: the process root or the desktop root. Defaults to process." },
                    "refresh": { "type": "boolean", "description": "When true, invalidate the cached tree before rebuilding. Defaults to false." },
                    "maxDepth": { "type": "integer", "minimum": 0, "description": "Maximum traversal depth. Default 4." },
                    "maxChildren": { "type": "integer", "minimum": 1, "description": "Maximum children inspected per node. Default 64." }
                },
                "required": ["processId"]
            }),
        },
        // ─── Selector Resolution (CSS / XPath) ────────────────────────────────
        Tool {
            name: "wa_resolve_css_selector".to_string(),
            description: "Resolve a CSS-style selector against a saved WA semantic snapshot and return matching nodes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "cssSelector": { "type": "string", "description": "CSS-style selector expression to resolve." }
                },
                "required": ["sessionId", "cssSelector"]
            }),
        },
        Tool {
            name: "wa_resolve_xpath".to_string(),
            description: "Resolve an XPath expression against a saved WA semantic snapshot and return matching nodes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "WA session identifier." },
                    "snapshotName": { "type": "string", "description": "Optional snapshot name. Defaults to the latest snapshot for the session." },
                    "xpath": { "type": "string", "description": "XPath expression to resolve." }
                },
                "required": ["sessionId", "xpath"]
            }),
        },
        // ─── Recovery Planning ────────────────────────────────────────────────
        Tool {
            name: "wa_recovery_plan".to_string(),
            description: "Return the ordered recovery action plan for a known automation failure scenario.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "scenario": { "type": "string", "enum": ["element_not_found", "blocked_by_popup"], "description": "Failure scenario to plan recovery for. Defaults to element_not_found." }
                }
            }),
        },
        Tool {
            name: "wa_recovery_adaptive_wait".to_string(),
            description: "Feed observed element ready-times to the adaptive wait estimator and get the recommended poll interval.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "observationsMs": { "type": "array", "items": { "type": "integer", "minimum": 0 }, "description": "Previously observed ready-times in milliseconds." }
                }
            }),
        },
        // ─── Screenshot ───────────────────────────────────────────────────────
        Tool {
            name: "wa_screenshot".to_string(),
            description: "Capture the full screen, a single window by PID, or a screen region, and persist it in the format implied by the output file extension.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "outputPath": { "type": "string", "description": "Destination file path. The encoder follows the extension: .bmp, .jpg/.jpeg, .gif, .webp, otherwise .png (also the default for an unrecognised or missing extension). Defaults to screenshot.png. The response `format` names the encoder that actually ran." },
                    "pid": { "type": "integer", "minimum": 1, "description": "Optional process id to capture a specific window instead of the full screen." },
                    "region": {
                        "type": "object",
                        "description": "Optional screen region. Takes precedence over full-screen capture when pid is omitted.",
                        "properties": {
                            "x": { "type": "integer" },
                            "y": { "type": "integer" },
                            "width": { "type": "integer", "minimum": 1 },
                            "height": { "type": "integer", "minimum": 1 }
                        }
                    }
                }
            }),
        },
        // ─── Window Lookup ────────────────────────────────────────────────────
        Tool {
            name: "wa_window_foreground".to_string(),
            description: "Return the current foreground (focused) desktop window, if any.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_vdesktop_window_info".to_string(),
            description: "Report which virtual desktop a window belongs to and whether it is on the current desktop.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": { "type": "integer", "description": "Window handle to inspect." }
                },
                "required": ["hwnd"]
            }),
        },
        // ─── Registry (extended) ──────────────────────────────────────────────
        Tool {
            name: "wa_registry_delete".to_string(),
            description: "Delete a Windows registry value.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hive": { "type": "string", "enum": ["HKCU", "HKLM", "HKCR", "HKU", "HKCC"], "description": "Registry hive." },
                    "path": { "type": "string", "description": "Registry key path." },
                    "name": { "type": "string", "description": "Value name to delete." }
                },
                "required": ["hive", "path", "name"]
            }),
        },
        Tool {
            name: "wa_registry_exists".to_string(),
            description: "Check whether a registry key (and optionally a specific value under it) exists.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hive": { "type": "string", "enum": ["HKCU", "HKLM", "HKCR", "HKU", "HKCC"], "description": "Registry hive." },
                    "path": { "type": "string", "description": "Registry key path." },
                    "name": { "type": "string", "description": "Optional value name. When omitted, tests for the key itself." }
                },
                "required": ["hive", "path"]
            }),
        },
        Tool {
            name: "wa_registry_enumerate".to_string(),
            description: "Enumerate the values and subkeys under a registry key.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hive": { "type": "string", "enum": ["HKCU", "HKLM", "HKCR", "HKU", "HKCC"], "description": "Registry hive." },
                    "path": { "type": "string", "description": "Registry key path to enumerate." }
                },
                "required": ["hive", "path"]
            }),
        },
        // ─── System Tray / UAC / DPI ──────────────────────────────────────────
        Tool {
            name: "wa_tray_list".to_string(),
            description: "List system tray (notification area) icons with their tooltips and owning processes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_tray_click".to_string(),
            description: format!("Click a system tray icon identified by its tooltip text. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "tooltip": { "type": "string", "description": "Tooltip text of the tray icon to click." },
                    "action": { "type": "string", "enum": ["click", "double_click", "right_click"], "description": "Mouse action to perform. Defaults to click." }
                },
                "required": ["tooltip"]
            }),
        },
        Tool {
            name: "wa_uac_detect".to_string(),
            description: "Detect whether a UAC (User Account Control) consent prompt is currently visible.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_system_dpi".to_string(),
            description: "Query the current system DPI and the equivalent scale percentage.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── OCR (extended) ───────────────────────────────────────────────────
        Tool {
            name: "wa_ocr_languages".to_string(),
            description: "List the OCR language tags available on this system.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── Desktop Platform ─────────────────────────────────────────────────
        Tool {
            name: "wa_platform_info".to_string(),
            description: "Report the host desktop platform and whether native desktop automation is supported.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_platform_tree_snapshot".to_string(),
            description: "Capture a platform-native accessibility tree snapshot for an application and persist it to the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "appName": { "type": "string", "description": "Application name or window title substring to capture." }
                }
            }),
        },
        // ─── Input Injection ──────────────────────────────────────────────────
        Tool {
            name: "wa_input_sequence".to_string(),
            description: format!("Build and atomically inject a sequence of low-level mouse and keyboard events via native SendInput. {SESSION_GATE_NOTE}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "Label for the sequence, used in logging. Defaults to mcp-input-sequence." },
                    "viaScript": { "type": "boolean", "description": "Inject through the generated PowerShell script instead of native SendInput. Defaults to false." },
                    "steps": {
                        "type": "array",
                        "description": "Ordered input operations to inject.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "op": { "type": "string", "enum": ["click", "right_click", "middle_click", "move", "type", "key_combo", "scroll", "wait", "drag_drop"], "description": "Operation kind." },
                                "x": { "type": "integer", "description": "Screen x coordinate (click / right_click / middle_click / move)." },
                                "y": { "type": "integer", "description": "Screen y coordinate (click / right_click / middle_click / move)." },
                                "absolute": { "type": "boolean", "description": "For move: use absolute screen coordinates. Defaults to true." },
                                "text": { "type": "string", "description": "For type: the text to enter." },
                                "key": { "type": "string", "description": "For key_combo: the primary key, e.g. A, F5, Enter, Tab." },
                                "modifiers": { "type": "array", "items": { "type": "string", "enum": ["ctrl", "shift", "alt", "win", "ctrl_shift", "ctrl_alt", "alt_shift", "ctrl_shift_alt"] }, "description": "For key_combo: modifier keys held during the press." },
                                "vertical": { "type": "integer", "description": "For scroll: vertical clicks, positive = up." },
                                "horizontal": { "type": "integer", "description": "For scroll: horizontal clicks, positive = right." },
                                "ms": { "type": "integer", "minimum": 0, "description": "For wait: pause duration in milliseconds. Defaults to 100." },
                                "from": { "type": "object", "description": "For drag_drop: origin point {x, y}." },
                                "to": { "type": "object", "description": "For drag_drop: destination point {x, y}." }
                            },
                            "required": ["op"]
                        }
                    }
                },
                "required": ["steps"]
            }),
        },
        // ─── Interaction Recording ────────────────────────────────────────────
        Tool {
            name: "wa_record_start".to_string(),
            description: "Start a shared interaction recording session and return the PowerShell hook script that produces events.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string", "description": "Recording session identifier. Defaults to default." },
                    "processId": { "type": "integer", "minimum": 1, "description": "Optional process id to restrict recording to." },
                    "windowTitle": { "type": "string", "description": "Optional window title substring to restrict recording to." },
                    "durationSeconds": { "type": "integer", "minimum": 1, "description": "Hook capture duration in seconds. Defaults to 30." }
                }
            }),
        },
        Tool {
            name: "wa_record_ingest".to_string(),
            description: "Ingest recorded interaction events (as emitted by the recording hook) into the active recording session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "events": {
                        "type": "array",
                        "description": "Recorded events to append.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["click", "double_click", "type", "key_combo", "focus", "scroll", "drag_drop", "window_activate"], "description": "Event kind." },
                                "offsetMs": { "type": "integer", "minimum": 0, "description": "Offset from recording start in milliseconds." },
                                "x": { "type": "integer" },
                                "y": { "type": "integer" },
                                "button": { "type": "string", "enum": ["left", "right", "middle"] },
                                "text": { "type": "string" },
                                "keys": { "type": "array", "items": { "type": "string" } },
                                "deltaX": { "type": "integer" },
                                "deltaY": { "type": "integer" },
                                "fromX": { "type": "integer" },
                                "fromY": { "type": "integer" },
                                "toX": { "type": "integer" },
                                "toY": { "type": "integer" },
                                "windowTitle": { "type": "string" },
                                "processId": { "type": "integer", "minimum": 1 },
                                "target": { "type": "object", "description": "Optional UIA target: nodeId, role, name, automationId." }
                            },
                            "required": ["kind"]
                        }
                    }
                },
                "required": ["events"]
            }),
        },
        Tool {
            name: "wa_record_pause".to_string(),
            description: "Pause or resume the active recording session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "resume": { "type": "boolean", "description": "When true resume recording, otherwise pause it. Defaults to false." }
                }
            }),
        },
        Tool {
            name: "wa_record_status".to_string(),
            description: "Report the state, event count, and elapsed time of the active recording session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "wa_record_replay_plan".to_string(),
            description: "Compute the inter-step replay delays for the recorded events under a given replay configuration.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "speedMultiplier": { "type": "number", "exclusiveMinimum": 0, "description": "Replay speed: 1.0 real-time, 2.0 double speed. Defaults to 1.0." },
                    "minStepDelayMs": { "type": "integer", "minimum": 0, "description": "Minimum delay between steps. Defaults to 50." },
                    "verifyPostconditions": { "type": "boolean", "description": "Verify focus/value postconditions per step. Defaults to true." },
                    "stopOnFailure": { "type": "boolean", "description": "Stop replay on first failure. Defaults to true." }
                }
            }),
        },
        Tool {
            name: "wa_record_stop".to_string(),
            description: "Stop the active recording session and optionally persist it as a WA semantic script artifact.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "scriptName": { "type": "string", "description": "Script name to persist under. Defaults to recorded-script." },
                    "persist": { "type": "boolean", "description": "When false, stop without writing an artifact. Defaults to true." }
                }
            }),
        },
        // ─── Cross-Context Browser/Desktop Bridge ─────────────────────────────
        Tool {
            name: "wa_bridge_run".to_string(),
            description: "Execute an ordered cross-context workflow that mixes browser actions, desktop actions, cross-context waits, and data transfers.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Workflow name. Defaults to bridge-workflow." },
                    "timeoutMs": { "type": "integer", "minimum": 1, "description": "Global workflow timeout in milliseconds. Defaults to 30000." },
                    "failFast": { "type": "boolean", "description": "Abort on the first failing step. Defaults to true." },
                    "downloadDir": { "type": "string", "description": "Directory for browser downloads. Defaults to <workspace>/output/downloads." },
                    "steps": {
                        "type": "array",
                        "minItems": 1,
                        "description": "Ordered workflow steps. Each step declares a context and an action.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "context": { "type": "string", "enum": ["browser", "desktop", "wait", "transfer"], "description": "Execution context for the step." },
                                "action": { "type": "string", "description": "Action name. browser: navigate, click, type, download, trigger_upload, eval_js, wait_for_element. desktop: open_file, focus_window, type_text, handle_file_dialog, click_element, copy_to_clipboard, paste_from_clipboard. wait: file_appears, window_appears, browser_navigates, clipboard_contains, process_starts. transfer: browser_to_desktop, desktop_to_browser, download_and_open, read_desktop_text." },
                                "url": { "type": "string" },
                                "selector": { "type": "string" },
                                "text": { "type": "string" },
                                "script": { "type": "string" },
                                "expectedFilename": { "type": "string" },
                                "inputSelector": { "type": "string" },
                                "timeoutMs": { "type": "integer", "minimum": 0 },
                                "path": { "type": "string" },
                                "titleContains": { "type": "string" },
                                "name": { "type": "string" },
                                "role": { "type": "string" },
                                "urlContains": { "type": "string" },
                                "browserSelector": { "type": "string" },
                                "desktopTarget": { "type": "string" },
                                "desktopSource": { "type": "string" },
                                "downloadUrl": { "type": "string" },
                                "appExe": { "type": "string" },
                                "elementName": { "type": "string" }
                            },
                            "required": ["context", "action"]
                        }
                    }
                },
                "required": ["steps"]
            }),
        },
    ]
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{get_wa_tools, SESSION_EFFECT_NOTE, SESSION_GATE_NOTE};

    fn schema_of(name: &str) -> serde_json::Value {
        get_wa_tools()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is no longer advertised"))
            .input_schema
    }

    fn description_of(name: &str) -> String {
        get_wa_tools()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is no longer advertised"))
            .description
    }

    // Bug #37: the handler now refuses to act unless it is given explicit
    // handles, so the advertised schema has to demand them too - otherwise a
    // spec-following caller is told it may tile "visible windows" and then gets
    // a validation error back.
    #[test]
    fn window_tile_schema_demands_the_handles_the_handler_requires() {
        let schema = schema_of("wa_window_tile");
        let required = schema["required"].as_array().expect("required array");
        assert!(
            required.iter().any(|v| v == "hwnds"),
            "hwnds must be required, got: {required:?}"
        );
        let hwnds = &schema["properties"]["hwnds"];
        assert_eq!(hwnds["type"], "array", "got: {hwnds}");
        assert_eq!(hwnds["items"]["type"], "integer", "got: {hwnds}");

        let desc = description_of("wa_window_tile");
        assert!(
            !desc.contains("2-column, 3-column"),
            "stale layout claim still advertised: {desc}"
        );
    }

    // Bug #23: the capture backend encodes to whatever the extension asks for,
    // so the docs must not promise a bitmap specifically, and must name every
    // format the shared extension->encoder mapping actually supports.
    #[test]
    fn screenshot_schema_names_every_format_the_encoder_can_write() {
        let desc = description_of("wa_screenshot");
        assert!(
            !desc.contains("as a Windows bitmap"),
            "stale BMP-only claim still advertised: {desc}"
        );
        let path_desc = schema_of("wa_screenshot")["properties"]["outputPath"]["description"]
            .as_str()
            .expect("outputPath description")
            .to_ascii_lowercase();
        for ext in ["bmp", "png", "jpeg", "gif", "webp"] {
            let probed = std::path::Path::new("x").join(format!("shot.{ext}"));
            let encoder = crate::wa::screenshot::image_format_for_path(&probed);
            assert_eq!(encoder, ext, "unexpected mapping for .{ext}");
            assert!(
                path_desc.contains(ext),
                "encoder supports {ext} but the parameter doc never mentions it: {path_desc}"
            );
        }
        assert!(
            path_desc.contains("default") && path_desc.contains("png"),
            "the fallback format must be documented: {path_desc}"
        );
    }

    /// Bug #40: an opt-in that is only discoverable by triggering the side
    /// effect is no opt-in at all. Every tool that enforces the session gate has
    /// to advertise it in `tools/list`, and the read-only tools must not imply
    /// they are gated when they are not.
    #[test]
    fn session_affecting_tools_advertise_the_opt_in_they_enforce() {
        for name in [
            "wa_virtual_desktop_switch",
            "wa_vdesktop_create",
            "wa_vdesktop_remove",
            "wa_vdesktop_move_window",
            "wa_input_sequence",
            "wa_tray_click",
            "wa_notifications_dismiss",
            "wa_window_action",
        ] {
            let desc = description_of(name);
            assert!(
                desc.contains(crate::wa::session_guard::ALLOW_ENV),
                "{name} enforces the session gate but never advertises it: {desc}"
            );
        }
        for readable in [
            "wa_virtual_desktop_list",
            "wa_vdesktop_window_info",
            "wa_tray_list",
            "wa_notifications_list",
            "wa_screenshot",
            "wa_clipboard_read",
            "wa_monitor_list",
            "wa_window_list",
        ] {
            let desc = description_of(readable);
            assert!(
                !desc.contains(crate::wa::session_guard::ALLOW_ENV),
                "{readable} measures the session without changing it, so it must not read as gated"
            );
            assert!(
                !desc.contains(crate::wa::session_guard::EFFECT_ENV),
                "{readable} measures the session without changing it, so it must not read as gated"
            );
        }
    }

    /// Bug #40: the milder family has its own switch, and advertising the wrong
    /// one would have the operator grant desktop-moving to unlock a clipboard
    /// write - or refuse the clipboard entirely while believing they had
    /// consented. Read-only neighbours must stay out of both lists.
    #[test]
    fn session_effect_tools_advertise_their_own_opt_in() {
        for name in [
            "wa_clipboard_write",
            "wa_clipboard_clear",
            "wa_process_launch",
            "wa_browser_navigate",
        ] {
            let desc = description_of(name);
            assert!(
                desc.contains(crate::wa::session_guard::EFFECT_ENV),
                "{name} enforces the session-effect gate but never advertises it: {desc}"
            );
            assert!(
                !desc.contains(crate::wa::session_guard::ALLOW_ENV),
                "{name} must not ask for the broader desktop-moving consent: {desc}"
            );
        }
    }

    /// The two switch names differ by one word, so a typo would silently make
    /// one family ungated while its description still looked right.
    #[test]
    fn the_two_opt_in_switches_are_distinct() {
        assert_ne!(
            crate::wa::session_guard::ALLOW_ENV,
            crate::wa::session_guard::EFFECT_ENV
        );
        assert!(SESSION_GATE_NOTE.contains(crate::wa::session_guard::ALLOW_ENV));
        assert!(!SESSION_GATE_NOTE.contains(crate::wa::session_guard::EFFECT_ENV));
        assert!(SESSION_EFFECT_NOTE.contains(crate::wa::session_guard::EFFECT_ENV));
        assert!(
            !SESSION_EFFECT_NOTE.contains(crate::wa::session_guard::ALLOW_ENV),
            "{SESSION_EFFECT_NOTE}"
        );
    }
}
