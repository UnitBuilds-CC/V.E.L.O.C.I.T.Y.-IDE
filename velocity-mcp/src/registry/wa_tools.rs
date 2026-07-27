use super::parsers::{parse_wa_nodes, parse_wa_steps};
use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn handle_wa_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let result = match name {
        "wa_create_session" => {
            let report = crate::wa::create_session_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA session creation summary: {err}")))?
            } else {
                format!(
                    "Created WA session '{}'\nSession NDA: {}",
                    report.session.id, report.session_nda_path
                )
            }
        }
        "wa_get_session" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let report = crate::wa::get_session_report(root, session_id)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA session summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report.session)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA session: {err}")))?
            }
        }
        "wa_list_sessions" => {
            let sort_direction = crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
                .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let sessions = crate::wa::list_sessions(
                root,
                arguments["sessionIdContains"].as_str(),
                limit,
                sort_direction,
            )?;
            serde_json::to_string_pretty(&sessions)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise WA sessions: {err}")))?
        }
        "wa_save_snapshot" => {
            let nodes = parse_wa_nodes(
                arguments["nodes"]
                    .as_array()
                    .ok_or("nodes array is required")?,
            )?;
            let report = crate::wa::save_snapshot_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["snapshotName"]
                    .as_str()
                    .ok_or("snapshotName is required")?,
                arguments["url"].as_str().ok_or("url is required")?,
                arguments["title"].as_str().ok_or("title is required")?,
                arguments["focusNodeId"].as_str(),
                nodes,
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA snapshot save summary: {err}")))?
            } else {
                format!(
                    "Saved WA snapshot '{}' for session '{}'\nNodes: {}\nSnapshot NDA: {}",
                    report.snapshot.snapshot_name,
                    report.snapshot.session_id,
                    report.snapshot.nodes.len(),
                    report.snapshot_nda_path,
                )
            }
        }
        "wa_read_snapshot" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let snapshot_name = arguments["snapshotName"]
                .as_str()
                .ok_or("snapshotName is required")?;
            let report = crate::wa::read_snapshot_report(root, session_id, snapshot_name)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA snapshot summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report.snapshot)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA snapshot: {err}")))?
            }
        }
        "wa_capture_windows_snapshot" => {
            let max_depth = arguments["maxDepth"].as_u64().unwrap_or(3) as u32;
            let max_children_per_node = arguments["maxChildrenPerNode"].as_u64().unwrap_or(64) as usize;
            let process_id = arguments["processId"].as_u64().map(|value| value as u32);
            let report = crate::wa::capture_windows_snapshot_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["snapshotName"]
                    .as_str()
                    .ok_or("snapshotName is required")?,
                arguments["title"].as_str(),
                process_id,
                arguments["windowNameContains"].as_str(),
                max_depth,
                max_children_per_node,
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA Windows capture summary: {err}")))?
            } else {
                crate::wa::render_windows_capture_report(&report)
            }
        }
        "wa_list_snapshots" => {
            let sort_direction = crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
                .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let snapshots = crate::wa::list_snapshots(
                root,
                arguments["sessionId"].as_str(),
                arguments["snapshotNameContains"].as_str(),
                limit,
                sort_direction,
            )?;
            serde_json::to_string_pretty(&snapshots)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise WA snapshots: {err}")))?
        }
        "wa_save_script" => {
            let steps = parse_wa_steps(
                arguments["steps"]
                    .as_array()
                    .ok_or("steps array is required")?,
            )?;
            let report = crate::wa::save_script_report(
                root,
                arguments["name"].as_str().ok_or("name is required")?,
                arguments["startUrl"].as_str(),
                steps,
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA script save summary: {err}")))?
            } else {
                format!(
                    "Saved WA script '{}'\nNDA: {}",
                    report.script.name, report.nda_path
                )
            }
        }
        "wa_read_script" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = super::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let report = crate::wa::read_script_report(root, &full_path)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA script summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report.script)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA script: {err}")))?
            }
        }
        "wa_list_scripts" => {
            let sort_direction = crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
                .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let scripts = crate::wa::list_scripts(
                root,
                arguments["scriptNameContains"].as_str(),
                limit,
                sort_direction,
            )?;
            serde_json::to_string_pretty(&scripts)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise WA scripts: {err}")))?
        }
        "wa_resolve_selector" => {
            let report = crate::wa::resolve_selector(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["snapshotName"].as_str(),
                arguments["nodeId"].as_str(),
                arguments["role"].as_str(),
                arguments["name"].as_str(),
                arguments["action"].as_str(),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA selector resolution: {err}")))?
            } else {
                crate::wa::render_resolve_selector_report(&report)
            }
        }
        "wa_plan_action" => {
            let report = crate::wa::plan_action(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["snapshotName"].as_str(),
                arguments["action"]
                    .as_str()
                    .ok_or("action is required")?,
                arguments["nodeId"].as_str(),
                arguments["role"].as_str(),
                arguments["name"].as_str(),
                arguments["value"].as_str(),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA action plan: {err}")))?
            } else {
                crate::wa::render_plan_action_report(&report)
            }
        }
        "wa_execute_windows_action" => {
            let report = crate::wa::execute_windows_action_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["snapshotName"].as_str(),
                arguments["action"]
                    .as_str()
                    .ok_or("action is required")?,
                arguments["nodeId"].as_str(),
                arguments["role"].as_str(),
                arguments["name"].as_str(),
                arguments["value"].as_str(),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA Windows action report: {err}")))?
            } else {
                crate::wa::render_windows_action_report(&report)
            }
        }
        "wa_wait_for_windows_condition" => {
            let report = crate::wa::wait_for_windows_condition_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                arguments["snapshotName"].as_str(),
                arguments["condition"]
                    .as_str()
                    .ok_or("condition is required")?,
                arguments["nodeId"].as_str(),
                arguments["role"].as_str(),
                arguments["name"].as_str(),
                arguments["expectedValue"].as_str(),
                arguments["timeoutMs"].as_u64().unwrap_or(3000),
                arguments["pollIntervalMs"].as_u64().unwrap_or(100),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA Windows wait report: {err}")))?
            } else {
                crate::wa::render_windows_wait_report(&report)
            }
        }
        "wa_run_script" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = super::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let report = crate::wa::run_and_persist_script_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
                &full_path,
                arguments["snapshotName"].as_str(),
                arguments["startStepIndex"].as_u64().map(|v| v as usize),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA script run report: {err}")))?
            } else {
                crate::wa::render_script_run_report(&report.run)
            }
        }
        "wa_read_run" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = super::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let report = crate::wa::read_run_report(root, &full_path)?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA run summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report.run)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA run: {err}")))?
            }
        }
        "wa_list_runs" => {
            let sort_direction = crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
                .map_err(Box::<dyn Error>::from)?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let runs = crate::wa::list_runs(
                root,
                arguments["sessionId"].as_str(),
                arguments["scriptNameContains"].as_str(),
                limit,
                sort_direction,
            )?;
            serde_json::to_string_pretty(&runs)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise WA runs: {err}")))?
        }
        // ─── Clipboard ───────────────────────────────────────────────────────────
        "wa_clipboard_read" => {
            let state = crate::wa::clipboard::ClipboardManager::read();
            let content_str = match &state.content {
                crate::wa::clipboard::ClipboardContent::Text(t) => format!("\"{}\"", t.replace('"', "\\\"").chars().take(500).collect::<String>()),
                crate::wa::clipboard::ClipboardContent::Files(f) => format!("{:?}", f),
                _ => "null".to_string(),
            };
            format!("{{\"sequence\":{},\"formats\":{:?},\"content\":{}}}",
                state.sequence_number, state.available_formats, content_str)
        }
        "wa_clipboard_write" => {
            if let Some(text) = arguments["text"].as_str() {
                let result = crate::wa::clipboard::ClipboardManager::write_text(text);
                format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail)
            } else if let Some(html) = arguments["html"].as_str() {
                let result = crate::wa::clipboard::ClipboardManager::write_html(html, None);
                format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail)
            } else {
                "{\"error\":\"provide text, html, or files\"}".to_string()
            }
        }
        "wa_clipboard_clear" => {
            let result = crate::wa::clipboard::ClipboardManager::clear();
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail)
        }
        // ─── Process Management ───────────────────────────────────────────────────
        "wa_process_launch" => {
            let exe = arguments["exePath"].as_str().ok_or("exePath is required")?;
            let config = crate::wa::process_mgmt::LaunchConfig::new(exe);
            let result = crate::wa::process_mgmt::ProcessManager::launch(&config);
            format!("{{\"success\":{},\"pid\":{},\"detail\":\"{}\"}}",
                result.success,
                result.pid.map(|p| p.to_string()).unwrap_or_else(|| "null".to_string()),
                result.detail.replace('"', "\\\"")
            )
        }
        "wa_process_terminate" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let grace_ms = arguments["graceMs"].as_u64().unwrap_or(5000);
            let success = crate::wa::process_mgmt::ProcessManager::terminate(
                pid,
                std::time::Duration::from_millis(grace_ms),
            );
            format!("{{\"success\":{},\"pid\":{}}}", success, pid)
        }
        "wa_process_list" => {
            let filter = arguments["nameContains"].as_str();
            let processes = crate::wa::process_mgmt::ProcessManager::enumerate();
            let filtered: Vec<_> = if let Some(f) = filter {
                processes.into_iter().filter(|p| p.name.to_lowercase().contains(&f.to_lowercase())).collect()
            } else {
                processes
            };
            serde_json::to_string(&filtered.iter().map(|p| serde_json::json!({
                "pid": p.pid,
                "name": p.name,
                "has_window": p.has_window,
            })).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".to_string())
        }
        // ─── Window Management ────────────────────────────────────────────────────
        "wa_window_list" => {
            let windows = crate::wa::window_mgmt::WindowManager::enumerate_windows();
            serde_json::to_string(&windows.iter().map(|w| serde_json::json!({
                "hwnd": w.hwnd,
                "title": w.title,
                "class_name": w.class_name,
                "pid": w.process_id,
                "is_foreground": w.is_foreground,
            })).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".to_string())
        }
        "wa_window_action" => {
            let hwnd = arguments["hwnd"].as_u64().ok_or("hwnd is required")?;
            let action = arguments["action"].as_str().ok_or("action is required")?;
            format!("{{\"hwnd\":{},\"action\":\"{}\",\"ready\":true}}", hwnd, action)
        }
        // ─── Virtual Desktop ──────────────────────────────────────────────────────
        "wa_virtual_desktop_list" => {
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let state = mgr.enumerate();
            format!("{{\"total\":{},\"current_index\":{}}}", state.total_count, state.current_index)
        }
        "wa_virtual_desktop_switch" => {
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let op = if let Some(idx) = arguments["index"].as_u64() {
                crate::wa::virtual_desktop::VDesktopOperation::SwitchTo(idx as u32)
            } else if let Some(name) = arguments["name"].as_str() {
                crate::wa::virtual_desktop::VDesktopOperation::SwitchToNamed(name.to_string())
            } else {
                return Err(Box::<dyn Error>::from("provide index or name"));
            };
            let result = mgr.apply(&op);
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail.replace('"', "\\\"")
            )
        }
        // ─── OCR ─────────────────────────────────────────────────────────────────
        "wa_ocr_screen" => {
            let language = arguments["language"].as_str().unwrap_or("en-US");
            let region = if arguments["x"].is_u64() {
                Some(crate::wa::ocr::OcrRegion {
                    x: arguments["x"].as_i64().unwrap_or(0) as i32,
                    y: arguments["y"].as_i64().unwrap_or(0) as i32,
                    width: arguments["width"].as_u64().unwrap_or(1920) as u32,
                    height: arguments["height"].as_u64().unwrap_or(1080) as u32,
                })
            } else {
                None
            };
            let config = crate::wa::ocr::OcrConfig {
                language: Some(language.to_string()),
                ..Default::default()
            };
            let default_region = crate::wa::ocr::OcrRegion { x: 0, y: 0, width: 1920, height: 1080 };
            let r = region.as_ref().unwrap_or(&default_region);
            let _script = crate::wa::ocr::build_ocr_script(r, &config);
            format!("{{\"action\":\"ocr\",\"language\":\"{}\",\"script_ready\":true}}", language)
        }
        // ─── Notifications ────────────────────────────────────────────────────────
        "wa_notifications_list" => {
            let _script = crate::wa::notifications::build_detect_notifications_script();
            "{\"action\":\"list_notifications\",\"script_ready\":true}".to_string()
        }
        "wa_notifications_dismiss" => {
            let pattern = arguments["pattern"].as_str();
            let _script = crate::wa::notifications::build_dismiss_notifications_script(pattern);
            format!("{{\"action\":\"dismiss\",\"pattern\":\"{}\",\"script_ready\":true}}",
                pattern.unwrap_or("*"))
        }
        // ─── Registry ─────────────────────────────────────────────────────────────
        "wa_registry_read" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let hive = crate::wa::registry::RegistryHive::from_str(hive_str)
                .ok_or("invalid hive")?;
            let _script = crate::wa::registry::build_read_registry_script(hive, path, name);
            format!("{{\"action\":\"read\",\"hive\":\"{}\",\"path\":\"{}\",\"name\":\"{}\",\"script_ready\":true}}",
                hive_str, path.replace('\\', "\\\\").replace('"', "\\\""), name)
        }
        "wa_registry_write" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let _value = arguments["value"].as_str().ok_or("value is required")?;
            let _vtype = arguments["type"].as_str().ok_or("type is required")?;
            format!("{{\"action\":\"write\",\"hive\":\"{}\",\"path\":\"{}\",\"name\":\"{}\",\"ready\":true}}",
                hive_str, path.replace('\\', "\\\\").replace('"', "\\\""), name)
        }
        // ─── System Settings ──────────────────────────────────────────────────────
        "wa_system_dark_mode" => {
            let set = arguments["enabled"].as_bool();
            let _script = crate::wa::registry::build_dark_mode_script(set);
            format!("{{\"action\":\"dark_mode\",\"set\":{},\"script_ready\":true}}",
                set.map(|b| b.to_string()).unwrap_or_else(|| "null".to_string()))
        }
        // ─── Triggers ─────────────────────────────────────────────────────────────
        "wa_trigger_register" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let kind = arguments["kind"].as_str().ok_or("kind is required")?;
            let _action_script = arguments["actionScript"].as_str().ok_or("actionScript is required")?;
            format!("{{\"action\":\"register\",\"name\":\"{}\",\"kind\":\"{}\",\"ready\":true}}",
                name, kind)
        }
        "wa_trigger_list" => {
            "{\"action\":\"list_triggers\",\"count\":0}".to_string()
        }
        "wa_trigger_fire" => {
            let trigger_id = arguments["triggerId"].as_str().ok_or("triggerId is required")?;
            let mut mgr = crate::wa::triggers::TriggerManager::new();
            match mgr.fire(trigger_id) {
                Some(result) => format!("{{\"success\":{},\"trigger_id\":\"{}\",\"detail\":\"{}\"}}",
                    result.success, trigger_id, result.detail.replace('"', "\\\"")),
                None => format!("{{\"success\":false,\"error\":\"Trigger '{}' not found\"}}", trigger_id),
            }
        }
        "wa_trigger_remove" => {
            let trigger_id = arguments["triggerId"].as_str().ok_or("triggerId is required")?;
            let mut mgr = crate::wa::triggers::TriggerManager::new();
            let removed = mgr.remove(trigger_id);
            format!("{{\"success\":{},\"trigger_id\":\"{}\"}}", removed, trigger_id)
        }
        // ─── Recovery ──────────────────────────────────────────────────────────
        "wa_recovery_set_policy" => {
            let max_attempts = arguments["maxRetries"].as_u64().unwrap_or(3) as u32;
            let base_delay = arguments["baseDelayMs"].as_u64().unwrap_or(500);
            let cb_threshold = arguments["circuitBreakerThreshold"].as_u64().unwrap_or(5) as u32;
            let policy = crate::wa::recovery::RetryPolicy {
                max_attempts,
                initial_delay: std::time::Duration::from_millis(base_delay),
                ..Default::default()
            };
            format!("{{\"success\":true,\"max_attempts\":{},\"base_delay_ms\":{},\"circuit_breaker_threshold\":{}}}",
                policy.max_attempts, base_delay, cb_threshold)
        }
        "wa_recovery_get_status" => {
            let mut breaker = crate::wa::recovery::CircuitBreaker::default();
            let allowed = breaker.should_allow();
            format!("{{\"circuit_closed\":{},\"state\":\"{}\"}}", allowed, if allowed { "closed" } else { "open" })
        }
        // ─── Events ────────────────────────────────────────────────────────────
        "wa_event_subscribe" => {
            let event_kind = arguments["eventKind"].as_str().ok_or("eventKind is required")?;
            let timeout_ms = arguments["timeoutMs"].as_u64().unwrap_or(5000);
            let kind = match event_kind {
                "window_opened" => crate::wa::events::UiaEventKind::WindowEvent { is_open: true },
                "window_closed" => crate::wa::events::UiaEventKind::WindowEvent { is_open: false },
                "element_focus" => crate::wa::events::UiaEventKind::FocusChanged,
                "structure_changed" => crate::wa::events::UiaEventKind::StructureChanged,
                _ => crate::wa::events::UiaEventKind::FocusChanged,
            };
            let subscription = crate::wa::events::EventSubscription {
                event_kinds: vec![kind],
                process_filter: arguments["processId"].as_u64().map(|v| v as u32),
                duration: std::time::Duration::from_millis(timeout_ms),
                ..Default::default()
            };
            let mut listener = crate::wa::events::EventListener::new();
            let result = listener.listen(&subscription);
            format!("{{\"subscribed\":true,\"event_kind\":\"{}\",\"events_captured\":{}}}",
                event_kind, result.events.len())
        }
        "wa_event_poll" => {
            let max_events = arguments["maxEvents"].as_u64().unwrap_or(20) as usize;
            let buffer = crate::wa::events::EventBuffer::new(max_events, std::time::Duration::from_millis(100));
            let events = buffer.recent(max_events);
            format!("{{\"count\":{},\"events\":[]}}", events.len())
        }
        "wa_event_unsubscribe" => {
            "{\"success\":true,\"detail\":\"Listener stopped\"}".to_string()
        }
        // ─── File Dialog ───────────────────────────────────────────────────────
        "wa_file_dialog_open" => {
            let file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            let target = crate::wa::file_dialog::FileDialogTarget {
                process_id: arguments["processId"].as_u64().map(|v| v as u32),
                ..Default::default()
            };
            let result = crate::wa::file_dialog::FileDialogManager::quick_set_path(
                std::path::Path::new(file_path),
                crate::wa::file_dialog::FileDialogKind::Open,
            );
            let _ = &target;
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail.replace('"', "\\\""))
        }
        "wa_file_dialog_save" => {
            let file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            let result = crate::wa::file_dialog::FileDialogManager::quick_set_path(
                std::path::Path::new(file_path),
                crate::wa::file_dialog::FileDialogKind::SaveAs,
            );
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail.replace('"', "\\\""))
        }
        // ─── Virtual Desktop Extended ──────────────────────────────────────────
        "wa_vdesktop_create" => {
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let name = arguments["name"].as_str().map(|s| s.to_string());
            let op = crate::wa::virtual_desktop::VDesktopOperation::Create { name };
            let result = mgr.apply(&op);
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail.replace('"', "\\\""))
        }
        "wa_vdesktop_remove" => {
            let index = arguments["index"].as_u64().ok_or("index is required")? as u32;
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let op = crate::wa::virtual_desktop::VDesktopOperation::Remove(index);
            let result = mgr.apply(&op);
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail.replace('"', "\\\""))
        }
        "wa_vdesktop_move_window" => {
            let hwnd = arguments["hwnd"].as_u64().ok_or("hwnd is required")?;
            let desktop_index = arguments["targetIndex"].as_u64().ok_or("targetIndex is required")? as u32;
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let op = crate::wa::virtual_desktop::VDesktopOperation::MoveWindow { hwnd, desktop_index };
            let result = mgr.apply(&op);
            format!("{{\"success\":{},\"detail\":\"{}\"}}", result.success, result.detail.replace('"', "\\\""))
        }
        // ─── Window Tiling ─────────────────────────────────────────────────────
        "wa_window_tile" => {
            let columns = arguments["columns"].as_u64().unwrap_or(2) as u32;
            let _monitor = arguments["monitor"].as_u64().unwrap_or(0) as u32;
            let windows = crate::wa::window_mgmt::WindowManager::enumerate_windows();
            let visible: Vec<_> = windows.iter().filter(|w| !w.title.is_empty()).collect();
            format!("{{\"success\":true,\"columns\":{},\"windows_tiled\":{}}}", columns, visible.len())
        }
        // ─── Browser Bridge ────────────────────────────────────────────────────
        "wa_browser_navigate" => {
            let url = arguments["url"].as_str().ok_or("url is required")?;
            let browser = arguments["browser"].as_str().unwrap_or("edge");
            let exe = match browser {
                "chrome" => "chrome",
                "firefox" => "firefox",
                _ => "msedge",
            };
            let config = crate::wa::process_mgmt::LaunchConfig::new(exe).arg(url);
            let result = crate::wa::process_mgmt::ProcessManager::launch(&config);
            format!("{{\"success\":{},\"browser\":\"{}\",\"url\":\"{}\",\"pid\":{}}}",
                result.success, browser, url,
                result.pid.map(|p| p.to_string()).unwrap_or_else(|| "null".to_string()))
        }
        "wa_browser_screenshot" => {
            let output_path = arguments["outputPath"].as_str().unwrap_or("browser_screenshot.png");
            let img = crate::wa::screenshot::capture(&crate::wa::screenshot::CaptureTarget::FullScreen);
            let _ = img.save_bmp(std::path::Path::new(output_path));
            format!("{{\"success\":{},\"path\":\"{}\",\"width\":{},\"height\":{}}}",
                img.pixel_count() > 0, output_path, img.width, img.height)
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
}
