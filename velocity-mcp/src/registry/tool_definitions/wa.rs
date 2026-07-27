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
            description: "Write content to the Windows clipboard.".to_string(),
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
            description: "Clear the Windows clipboard.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ─── Process Management Tools ────────────────────────────────────────────
        Tool {
            name: "wa_process_launch".to_string(),
            description: "Launch a Windows process with optional arguments, working directory, and elevation.".to_string(),
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
                    "titleContains": { "type": "string", "description": "Optional title substring filter." }
                }
            }),
        },
        Tool {
            name: "wa_window_action".to_string(),
            description: "Perform a window operation (move, resize, minimize, maximize, close, focus, topmost).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": { "type": "integer", "description": "Window handle." },
                    "action": { "type": "string", "enum": ["move", "resize", "minimize", "maximize", "restore", "close", "focus", "topmost"], "description": "Operation to perform." },
                    "x": { "type": "integer", "description": "X position (for move)." },
                    "y": { "type": "integer", "description": "Y position (for move)." },
                    "width": { "type": "integer", "description": "Width (for resize)." },
                    "height": { "type": "integer", "description": "Height (for resize)." }
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
            description: "Switch to a virtual desktop by index or name.".to_string(),
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
            description: "Perform OCR text recognition on a screen region or full screen.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
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
            description: "Dismiss visible Windows notifications, optionally filtered by pattern.".to_string(),
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
                    "name": { "type": "string", "description": "Trigger name." },
                    "kind": { "type": "string", "enum": ["file_watch", "window_appears", "window_closes", "process_starts", "process_exits", "clipboard_changed", "system_idle", "delay", "interval"], "description": "Trigger type." },
                    "target": { "type": "string", "description": "Target path/title/name depending on kind." },
                    "actionScript": { "type": "string", "description": "PowerShell script to execute when triggered." }
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
            description: "Subscribe to Windows UI Automation events (window opened, element changed, structure changed).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "eventKind": { "type": "string", "enum": ["window_opened", "window_closed", "element_focus", "element_value_changed", "structure_changed"], "description": "Type of UIA event." },
                    "processId": { "type": "integer", "description": "Optional PID filter." },
                    "timeoutMs": { "type": "integer", "description": "Listen duration in ms. Default 5000." }
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
            description: "Create a new Windows virtual desktop.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Optional name for the new desktop (Windows 11)." }
                }
            }),
        },
        Tool {
            name: "wa_vdesktop_remove".to_string(),
            description: "Remove a Windows virtual desktop by index.".to_string(),
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
            description: "Move a window to a different virtual desktop.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": { "type": "integer", "description": "Window handle to move." },
                    "targetIndex": { "type": "integer", "description": "Target desktop index." }
                },
                "required": ["hwnd", "targetIndex"]
            }),
        },
        // ─── Window Tiling Tool ─────────────────────────────────────────────────
        Tool {
            name: "wa_window_tile".to_string(),
            description: "Tile visible windows in a grid layout (2-column, 3-column, or custom).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "columns": { "type": "integer", "description": "Number of columns in the tile grid. Default 2." },
                    "monitor": { "type": "integer", "description": "Monitor index to tile on. Default 0 (primary)." }
                }
            }),
        },
        // ─── Browser Bridge Tools ───────────────────────────────────────────────
        Tool {
            name: "wa_browser_navigate".to_string(),
            description: "Navigate the browser bridge to a URL (launches browser if needed).".to_string(),
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
                    "outputPath": { "type": "string", "description": "Path to save the screenshot. Default 'browser_screenshot.png'." }
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
    ]
}
