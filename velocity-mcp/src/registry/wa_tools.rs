use super::parsers::{parse_wa_nodes, parse_wa_steps};
use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn handle_wa_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    log::debug!("wa_tool: {} called", name);
    let result = match name {
        "wa_create_session" => {
            let report = crate::wa::create_session_report(
                root,
                arguments["sessionId"]
                    .as_str()
                    .ok_or("sessionId is required")?,
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA session creation summary: {err}"))
                })?
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA session summary: {err}"))
                })?
            } else {
                serde_json::to_string_pretty(&report.session)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA session: {err}")))?
            }
        }
        "wa_list_sessions" => {
            let sort_direction =
                crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA snapshot save summary: {err}"))
                })?
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA snapshot summary: {err}"))
                })?
            } else {
                serde_json::to_string_pretty(&report.snapshot).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA snapshot: {err}"))
                })?
            }
        }
        "wa_capture_windows_snapshot" => {
            let max_depth = arguments["maxDepth"].as_u64().unwrap_or(3) as u32;
            let max_children_per_node =
                arguments["maxChildrenPerNode"].as_u64().unwrap_or(64) as usize;
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA Windows capture summary: {err}"))
                })?
            } else {
                crate::wa::render_windows_capture_report(&report)
            }
        }
        "wa_list_snapshots" => {
            let sort_direction =
                crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA script save summary: {err}"))
                })?
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA script summary: {err}"))
                })?
            } else {
                serde_json::to_string_pretty(&report.script)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA script: {err}")))?
            }
        }
        "wa_list_scripts" => {
            let sort_direction =
                crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA selector resolution: {err}"))
                })?
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
                arguments["action"].as_str().ok_or("action is required")?,
                arguments["nodeId"].as_str(),
                arguments["role"].as_str(),
                arguments["name"].as_str(),
                arguments["value"].as_str(),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA action plan: {err}"))
                })?
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
                arguments["action"].as_str().ok_or("action is required")?,
                arguments["nodeId"].as_str(),
                arguments["role"].as_str(),
                arguments["name"].as_str(),
                arguments["value"].as_str(),
            )?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA Windows action report: {err}"))
                })?
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA Windows wait report: {err}"))
                })?
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA script run report: {err}"))
                })?
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
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise WA run summary: {err}"))
                })?
            } else {
                serde_json::to_string_pretty(&report.run)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise WA run: {err}")))?
            }
        }
        "wa_list_runs" => {
            let sort_direction =
                crate::wa::parse_list_sort_direction(arguments["sortDirection"].as_str())
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
                crate::wa::clipboard::ClipboardContent::Text(t) => format!(
                    "\"{}\"",
                    t.replace('"', "\\\"").chars().take(500).collect::<String>()
                ),
                crate::wa::clipboard::ClipboardContent::Files(f) => format!("{:?}", f),
                _ => "null".to_string(),
            };
            format!(
                "{{\"sequence\":{},\"formats\":{:?},\"content\":{}}}",
                state.sequence_number, state.available_formats, content_str
            )
        }
        "wa_clipboard_write" => {
            if let Some(text) = arguments["text"].as_str() {
                let result = crate::wa::clipboard::ClipboardManager::write_text(text);
                format!(
                    "{{\"success\":{},\"detail\":\"{}\"}}",
                    result.success, result.detail
                )
            } else if let Some(html) = arguments["html"].as_str() {
                let result = crate::wa::clipboard::ClipboardManager::write_html(html, None);
                format!(
                    "{{\"success\":{},\"detail\":\"{}\"}}",
                    result.success, result.detail
                )
            } else {
                "{\"error\":\"provide text, html, or files\"}".to_string()
            }
        }
        "wa_clipboard_clear" => {
            let result = crate::wa::clipboard::ClipboardManager::clear();
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success, result.detail
            )
        }
        // ─── Process Management ───────────────────────────────────────────────────
        "wa_process_launch" => {
            let exe = arguments["exePath"].as_str().ok_or("exePath is required")?;
            let config = crate::wa::process_mgmt::LaunchConfig::new(exe);
            let result = crate::wa::process_mgmt::ProcessManager::launch(&config);
            format!(
                "{{\"success\":{},\"pid\":{},\"detail\":\"{}\"}}",
                result.success,
                result
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "null".to_string()),
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
                processes
                    .into_iter()
                    .filter(|p| p.name.to_lowercase().contains(&f.to_lowercase()))
                    .collect()
            } else {
                processes
            };
            serde_json::to_string(
                &filtered
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "pid": p.pid,
                            "name": p.name,
                            "has_window": p.has_window,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_string())
        }
        "wa_process_kill" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let success = crate::wa::process_mgmt::ProcessManager::kill(pid);
            format!("{{\"success\":{},\"pid\":{}}}", success, pid)
        }
        "wa_process_kill_tree" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let killed = crate::wa::process_mgmt::ProcessManager::kill_tree(pid);
            // `killed` counts the target as well as its children, so zero means
            // nothing was terminated - reporting success there was bug #32.
            format!(
                "{{\"success\":{},\"pid\":{},\"killed\":{},\"detail\":{}}}",
                killed > 0,
                pid,
                killed,
                if killed > 0 {
                    "null".to_string()
                } else {
                    "\"no process was terminated; the pid is unknown or already gone\"".to_string()
                }
            )
        }
        "wa_process_running" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let status = crate::wa::process_mgmt::ProcessManager::status(pid);
            serde_json::to_string(&serde_json::json!({
                "pid": status.pid,
                "running": status.running,
                "method": status.method,
                "detail": status.detail,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise process status: {err}")))?
        }
        "wa_process_info" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            match crate::wa::process_mgmt::ProcessManager::get_process(pid) {
                Some(p) => serde_json::to_string(&serde_json::json!({
                    "pid": p.pid,
                    "name": p.name,
                    "exe_path": p.exe_path,
                    "parent_pid": p.parent_pid,
                    "main_window_title": p.main_window_title,
                    "has_window": p.has_window,
                    "cpu_percent": p.cpu_percent,
                    "memory_bytes": p.memory_bytes,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
                None => format!("{{\"pid\":{},\"found\":false}}", pid),
            }
        }
        "wa_process_wait" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let timeout_ms = arguments["timeoutMs"].as_u64().unwrap_or(5000);
            let condition = if let Some(title) = arguments["windowTitleContains"].as_str() {
                crate::wa::process_mgmt::ProcessWaitCondition::WindowAppears {
                    title_contains: title.to_string(),
                }
            } else {
                crate::wa::process_mgmt::ProcessWaitCondition::Exit
            };
            let result = crate::wa::process_mgmt::ProcessManager::wait_for(
                pid,
                &condition,
                std::time::Duration::from_millis(timeout_ms),
            );
            format!(
                "{{\"condition_met\":{},\"elapsed_ms\":{},\"exit_code\":{},\"detail\":\"{}\"}}",
                result.condition_met,
                result.elapsed.as_millis(),
                result
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                result.detail.replace('"', "\\\"")
            )
        }
        // ─── UIA Direct (cached-tree lookup / invoke) ─────────────────────────
        "wa_uia_tree" => {
            let pid = arguments["processId"]
                .as_u64()
                .ok_or("processId is required")? as u32;
            let max_depth = arguments["maxDepth"].as_u64().unwrap_or(4) as u32;
            let max_children = arguments["maxChildren"].as_u64().unwrap_or(64) as u32;
            let mut client = crate::wa::uia_ffi::UiaDirectClient::initialize_for_process(pid)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA init failed: {e}")))?;
            let tree = client
                .build_tree(pid, max_depth, max_children)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA tree build failed: {e}")))?;
            serde_json::to_string(&serde_json::json!({
                "process_id": tree.process_id,
                "element_count": tree.element_count,
                "fresh": tree.is_fresh(),
            }))
            .unwrap_or_else(|_| "{}".to_string())
        }
        "wa_uia_lookup" => {
            let pid = arguments["processId"]
                .as_u64()
                .ok_or("processId is required")? as u32;
            let max_depth = arguments["maxDepth"].as_u64().unwrap_or(4) as u32;
            let max_children = arguments["maxChildren"].as_u64().unwrap_or(64) as u32;
            let mut client = crate::wa::uia_ffi::UiaDirectClient::initialize_for_process(pid)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA init failed: {e}")))?;
            let tree = client
                .build_tree(pid, max_depth, max_children)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA tree build failed: {e}")))?;
            if let Some(aid) = arguments["automationId"].as_str() {
                match tree.find_by_id(aid) {
                    Some(el) => serde_json::to_string(&uia_element_json(el))
                        .unwrap_or_else(|_| "{}".to_string()),
                    None => format!("{{\"found\":false,\"automation_id\":\"{}\"}}", aid),
                }
            } else if let Some(name) = arguments["name"].as_str() {
                let matches = tree.find_by_name(name);
                serde_json::to_string(
                    &matches
                        .iter()
                        .map(|el| uia_element_json(el))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_else(|_| "[]".to_string())
            } else if arguments["x"].is_number() && arguments["y"].is_number() {
                let x = arguments["x"].as_f64().unwrap_or(0.0);
                let y = arguments["y"].as_f64().unwrap_or(0.0);
                match tree.element_at_point(x, y) {
                    Some(el) => serde_json::to_string(&uia_element_json(el))
                        .unwrap_or_else(|_| "{}".to_string()),
                    None => format!("{{\"found\":false,\"x\":{},\"y\":{}}}", x, y),
                }
            } else {
                return Err(Box::<dyn Error>::from(
                    "provide automationId, name, or x+y for wa_uia_lookup",
                ));
            }
        }
        "wa_uia_invoke" => {
            let pid = arguments["processId"]
                .as_u64()
                .ok_or("processId is required")? as u32;
            let pattern_str = arguments["pattern"].as_str().ok_or("pattern is required")?;
            let pattern = crate::wa::uia_ffi::UiaPattern::from_str(pattern_str)
                .ok_or_else(|| format!("unknown UIA pattern '{pattern_str}'"))?;
            let value = arguments["value"].as_str();
            let max_depth = arguments["maxDepth"].as_u64().unwrap_or(4) as u32;
            let max_children = arguments["maxChildren"].as_u64().unwrap_or(64) as u32;
            let mut client = crate::wa::uia_ffi::UiaDirectClient::initialize_for_process(pid)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA init failed: {e}")))?;
            let tree = client
                .build_tree(pid, max_depth, max_children)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA tree build failed: {e}")))?;
            let element = if let Some(aid) = arguments["automationId"].as_str() {
                tree.find_by_id(aid).cloned()
            } else if let Some(name) = arguments["name"].as_str() {
                tree.find_by_name(name).first().cloned().cloned()
            } else if arguments["x"].is_number() && arguments["y"].is_number() {
                let x = arguments["x"].as_f64().unwrap_or(0.0);
                let y = arguments["y"].as_f64().unwrap_or(0.0);
                tree.element_at_point(x, y).cloned()
            } else {
                return Err(Box::<dyn Error>::from(
                    "provide automationId, name, or x+y to target the element",
                ));
            };
            let Some(element) = element else {
                return Err(Box::<dyn Error>::from(
                    "target element not found in UIA tree",
                ));
            };
            match client.invoke_pattern(&element, pattern, value) {
                Ok(()) => format!(
                    "{{\"success\":true,\"pattern\":\"{}\",\"element\":\"{}\"}}",
                    pattern_str,
                    element.name.replace('"', "\\\"")
                ),
                Err(e) => format!(
                    "{{\"success\":false,\"pattern\":\"{}\",\"error\":\"{}\"}}",
                    pattern_str,
                    e.replace('"', "\\\"")
                ),
            }
        }
        // ─── Window Management ────────────────────────────────────────────────────
        "wa_window_list" => {
            // Route through the specialised lookups when a filter is supplied so the
            // advertised `titleContains` / `pid` / `className` arguments are honoured.
            let title_filter = arguments["titleContains"].as_str();
            let windows = if let Some(pid) = arguments["pid"].as_u64() {
                crate::wa::window_mgmt::WindowManager::find_by_pid(pid as u32)
            } else if let Some(class_name) = arguments["className"].as_str() {
                crate::wa::window_mgmt::WindowManager::find_by_class(class_name)
            } else {
                crate::wa::window_mgmt::WindowManager::enumerate_windows()
            };
            let filtered: Vec<serde_json::Value> = windows
                .iter()
                .filter(|w| {
                    title_filter
                        .map(|needle| w.title.to_lowercase().contains(&needle.to_lowercase()))
                        .unwrap_or(true)
                })
                .map(|w| {
                    serde_json::json!({
                        "hwnd": w.hwnd,
                        "title": w.title,
                        "class_name": w.class_name,
                        "pid": w.process_id,
                        "is_foreground": w.is_foreground,
                    })
                })
                .collect();
            let count = filtered.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "count": count,
                "windows": filtered,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise window list: {err}")))?
        }
        "wa_window_foreground" => {
            match crate::wa::window_mgmt::WindowManager::get_foreground_window() {
                Some(w) => serde_json::to_string(&serde_json::json!({
                    "success": true,
                    "found": true,
                    "hwnd": w.hwnd,
                    "title": w.title,
                    "class_name": w.class_name,
                    "pid": w.process_id,
                }))
                .map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise foreground window: {err}"))
                })?,
                None => serde_json::to_string(&serde_json::json!({
                    "success": true,
                    "found": false,
                }))
                .map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise foreground window: {err}"))
                })?,
            }
        }
        "wa_window_action" => {
            let hwnd = arguments["hwnd"].as_u64().ok_or("hwnd is required")?;
            let action = arguments["action"].as_str().ok_or("action is required")?;
            let x = arguments["x"].as_i64().unwrap_or(0) as i32;
            let y = arguments["y"].as_i64().unwrap_or(0) as i32;
            let width = arguments["width"].as_u64().unwrap_or(0) as u32;
            let height = arguments["height"].as_u64().unwrap_or(0) as u32;
            let op = match action {
                "move" => crate::wa::window_mgmt::WindowOperation::Move { x, y },
                "resize" => crate::wa::window_mgmt::WindowOperation::Resize { width, height },
                "move_resize" | "moveresize" => {
                    crate::wa::window_mgmt::WindowOperation::MoveResize {
                        x,
                        y,
                        width,
                        height,
                    }
                }
                "minimize" => crate::wa::window_mgmt::WindowOperation::Minimize,
                "maximize" => crate::wa::window_mgmt::WindowOperation::Maximize,
                "restore" => crate::wa::window_mgmt::WindowOperation::Restore,
                "close" => crate::wa::window_mgmt::WindowOperation::Close,
                "focus" | "activate" | "bring_to_front" => {
                    crate::wa::window_mgmt::WindowOperation::BringToFront
                }
                "send_to_back" => crate::wa::window_mgmt::WindowOperation::SendToBack,
                "topmost" => crate::wa::window_mgmt::WindowOperation::SetTopMost(true),
                "untopmost" => crate::wa::window_mgmt::WindowOperation::SetTopMost(false),
                "opacity" => crate::wa::window_mgmt::WindowOperation::SetOpacity(
                    arguments["opacity"].as_u64().unwrap_or(255) as u8,
                ),
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown window action '{other}'"
                    )))
                }
            };
            let result = crate::wa::window_mgmt::WindowManager::apply_operation(hwnd, &op);
            let new_rect = match result.new_rect {
                Some(r) => format!(
                    ",\"new_rect\":{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
                    r.x, r.y, r.width, r.height
                ),
                None => ",\"new_rect\":null".to_string(),
            };
            format!(
                "{{\"success\":{},\"hwnd\":{},\"operation\":\"{}\",\"detail\":\"{}\"{}}}",
                result.success,
                result.hwnd,
                result.operation.replace('"', "\\\""),
                result.detail.replace('"', "\\\""),
                new_rect
            )
        }
        // ─── Virtual Desktop ──────────────────────────────────────────────────────
        "wa_virtual_desktop_list" => {
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let state = mgr.enumerate();
            let desktops: Vec<serde_json::Value> = state
                .desktops
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "name": d.name,
                        "index": d.index,
                        "is_current": d.is_current,
                    })
                })
                .collect();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "total": state.total_count,
                "current_index": state.current_index,
                // False means Windows did not publish which desktop is active, so
                // `current_index` above is a placeholder, not a measurement.
                "current_desktop_known": state.current_desktop_known,
                "desktops": desktops,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise virtual desktop list: {err}"))
            })?
        }
        "wa_vdesktop_window_info" => {
            let hwnd = arguments["hwnd"].as_u64().ok_or("hwnd is required")?;
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            mgr.enumerate();
            let snapshot = mgr.state().cloned();
            let probe = mgr.probe_window_desktop(hwnd);
            if !probe.window_exists {
                // An unknown window is an error, not a desktop assignment.
                let reason = probe.reason.unwrap_or_default();
                return Err(Box::<dyn Error>::from(format!(
                    "no such window: hwnd {hwnd}{}",
                    if reason.is_empty() {
                        String::new()
                    } else {
                        format!(" ({reason})")
                    }
                )));
            }
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "hwnd": hwnd,
                // A null here means Windows would not answer (its
                // IVirtualDesktopManager coclass is not registered on every
                // build), not that the window is absent from that desktop.
                "desktop_index": probe.desktop_index,
                "desktop_id": probe.desktop_id,
                "is_on_current_desktop": probe.on_current_desktop,
                "current_index": snapshot.as_ref().map(|s| s.current_index),
                "current_desktop_known": snapshot
                    .as_ref()
                    .map(|s| s.current_desktop_known)
                    .unwrap_or(false),
                "lookup_reason": probe.reason,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise virtual desktop window info: {err}"))
            })?
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
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success,
                result.detail.replace('"', "\\\"")
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
            let default_region = crate::wa::ocr::OcrRegion {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            };
            let r = region.as_ref().unwrap_or(&default_region);
            // A `pid` target recognises the whole window; otherwise fall back to the
            // explicit screen region (or the full-screen default).
            let result = match arguments["pid"].as_u64() {
                Some(pid) => crate::wa::ocr::OcrEngine::recognize_window(pid as u32, &config),
                None => crate::wa::ocr::OcrEngine::recognize_region(r, &config),
            };
            let blocks: Vec<serde_json::Value> = result
                .blocks
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "text": b.text,
                        "confidence": b.confidence,
                        "line_index": b.line_index,
                        "word_index": b.word_index,
                        "bounds": {
                            "x": b.bounds.x,
                            "y": b.bounds.y,
                            "width": b.bounds.width,
                            "height": b.bounds.height,
                        },
                    })
                })
                .collect();
            let block_count = blocks.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "language": result.language,
                "full_text": result.full_text,
                "block_count": block_count,
                "blocks": blocks,
                "source": result.source,
                "duration_ms": result.duration.as_millis() as u64,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise OCR result: {err}")))?
        }
        // ─── Notifications ────────────────────────────────────────────────────────
        "wa_notifications_list" => {
            let notifications =
                crate::wa::notifications::NotificationManager::get_visible_notifications();
            let items: Vec<serde_json::Value> = notifications
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "id": n.id,
                        "app_name": n.app_name,
                        "title": n.title,
                        "body": n.body,
                        "timestamp_ms": n.timestamp_ms,
                        "actions": n.actions,
                        "is_visible": n.is_visible,
                        "is_system": n.is_system,
                    })
                })
                .collect();
            let count = items.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "count": count,
                "notifications": items,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise notifications list: {err}")))?
        }
        "wa_notifications_dismiss" => {
            let pattern = arguments["pattern"].as_str();
            let result = crate::wa::notifications::NotificationManager::dismiss_matching(pattern);
            serde_json::to_string(&serde_json::json!({
                "success": result.success,
                "action": result.action,
                "pattern": pattern.unwrap_or("*"),
                "detail": result.detail,
                "notifications_remaining": result.notifications_remaining,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise notifications dismiss: {err}"))
            })?
        }
        // ─── Registry ─────────────────────────────────────────────────────────────
        "wa_registry_read" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let hive =
                crate::wa::registry::RegistryHive::from_str(hive_str).ok_or("invalid hive")?;
            let result = crate::wa::registry::RegistryManager::read(hive, path, name);
            let value_json = match &result.value {
                Some(crate::wa::registry::RegistryValue::String(s))
                | Some(crate::wa::registry::RegistryValue::ExpandString(s)) => {
                    serde_json::Value::String(s.clone())
                }
                Some(crate::wa::registry::RegistryValue::DWord(d)) => serde_json::json!(*d),
                Some(crate::wa::registry::RegistryValue::QWord(q)) => serde_json::json!(*q),
                Some(crate::wa::registry::RegistryValue::Binary(b)) => serde_json::json!(b),
                Some(crate::wa::registry::RegistryValue::MultiString(m)) => serde_json::json!(m),
                None => serde_json::Value::Null,
            };
            let vtype = result
                .value
                .as_ref()
                .map(|v| v.as_ps_type())
                .unwrap_or("none");
            serde_json::to_string(&serde_json::json!({
                "success": result.success,
                "operation": result.operation,
                "hive": hive_str,
                "path": path,
                "name": name,
                "type": vtype,
                "value": value_json,
                "detail": result.detail,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise registry read result: {err}"))
            })?
        }
        "wa_registry_write" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let value = arguments["value"].as_str().ok_or("value is required")?;
            let vtype = arguments["type"].as_str().ok_or("type is required")?;
            let hive =
                crate::wa::registry::RegistryHive::from_str(hive_str).ok_or("invalid hive")?;
            let reg_value = match vtype {
                "String" => crate::wa::registry::RegistryValue::String(value.to_string()),
                "ExpandString" => {
                    crate::wa::registry::RegistryValue::ExpandString(value.to_string())
                }
                "DWord" => crate::wa::registry::RegistryValue::DWord(
                    value
                        .parse::<u32>()
                        .map_err(|e| format!("DWord value '{value}' is not a u32: {e}"))?,
                ),
                "QWord" => crate::wa::registry::RegistryValue::QWord(
                    value
                        .parse::<u64>()
                        .map_err(|e| format!("QWord value '{value}' is not a u64: {e}"))?,
                ),
                "MultiString" => crate::wa::registry::RegistryValue::MultiString(
                    value
                        .lines()
                        .map(|line| line.trim().to_string())
                        .filter(|line| !line.is_empty())
                        .collect(),
                ),
                "Binary" => crate::wa::registry::RegistryValue::Binary(
                    value
                        .split_whitespace()
                        .map(|byte| {
                            u8::from_str_radix(byte.trim_start_matches("0x"), 16)
                                .map_err(|e| format!("Binary byte '{byte}' is not hex: {e}"))
                        })
                        .collect::<Result<Vec<u8>, _>>()?,
                ),
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown registry value type '{other}'"
                    )))
                }
            };
            let entry = crate::wa::registry::RegistryEntry {
                hive,
                path: path.to_string(),
                name: name.to_string(),
                value: reg_value,
            };
            let result = crate::wa::registry::RegistryManager::write(&entry);
            serde_json::to_string(&serde_json::json!({
                "success": result.success,
                "operation": result.operation,
                "hive": hive_str,
                "path": path,
                "name": name,
                "type": vtype,
                "detail": result.detail,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise registry write result: {err}"))
            })?
        }
        "wa_registry_delete" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let hive =
                crate::wa::registry::RegistryHive::from_str(hive_str).ok_or("invalid hive")?;
            let result = crate::wa::registry::RegistryManager::delete(hive, path, name);
            serde_json::to_string(&serde_json::json!({
                "success": result.success,
                "operation": result.operation,
                "hive": hive_str,
                "path": path,
                "name": name,
                "detail": result.detail,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise registry delete result: {err}"))
            })?
        }
        "wa_registry_exists" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let hive =
                crate::wa::registry::RegistryHive::from_str(hive_str).ok_or("invalid hive")?;
            let name = arguments["name"].as_str();
            let exists = crate::wa::registry::RegistryManager::exists(hive, path, name);
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "operation": "exists",
                "hive": hive_str,
                "path": path,
                "name": name,
                "exists": exists,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise registry exists result: {err}"))
            })?
        }
        "wa_registry_enumerate" => {
            let hive_str = arguments["hive"].as_str().ok_or("hive is required")?;
            let path = arguments["path"].as_str().ok_or("path is required")?;
            let hive =
                crate::wa::registry::RegistryHive::from_str(hive_str).ok_or("invalid hive")?;
            let values: Vec<serde_json::Value> =
                crate::wa::registry::RegistryManager::enumerate_values(hive, path)
                    .iter()
                    .map(|entry| {
                        serde_json::json!({
                            "name": entry.name,
                            "type": entry.value.as_ps_type(),
                        })
                    })
                    .collect();
            let subkeys = crate::wa::registry::RegistryManager::enumerate_subkeys(hive, path);
            let value_count = values.len();
            let subkey_count = subkeys.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "operation": "enumerate",
                "hive": hive_str,
                "path": path,
                "value_count": value_count,
                "values": values,
                "subkey_count": subkey_count,
                "subkeys": subkeys,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise registry enumerate result: {err}"))
            })?
        }
        // ─── System Settings ──────────────────────────────────────────────────────
        "wa_system_dark_mode" => {
            match arguments["enabled"].as_bool() {
                // Set mode: toggle dark mode via the real SystemSettingsManager.
                Some(enabled) => {
                    let result = crate::wa::registry::SystemSettingsManager::set(
                        &crate::wa::registry::SystemSetting::DarkMode(enabled),
                    );
                    serde_json::to_string(&serde_json::json!({
                        "success": result.success,
                        "operation": "set",
                        "enabled": enabled,
                        "detail": result.detail,
                    }))
                    .map_err(|err| {
                        Box::<dyn Error>::from(format!("serialise dark mode set: {err}"))
                    })?
                }
                // Query mode: read the current dark mode state (read-only).
                None => {
                    let dark_mode = crate::wa::registry::SystemSettingsManager::is_dark_mode();
                    serde_json::to_string(&serde_json::json!({
                        "success": true,
                        "operation": "query",
                        "dark_mode": dark_mode,
                    }))
                    .map_err(|err| {
                        Box::<dyn Error>::from(format!("serialise dark mode query: {err}"))
                    })?
                }
            }
        }
        // ─── Triggers ─────────────────────────────────────────────────────────────
        "wa_trigger_register" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let kind_str = arguments["kind"].as_str().ok_or("kind is required")?;
            let action_script = arguments["actionScript"]
                .as_str()
                .ok_or("actionScript is required")?;
            let target = arguments["target"].as_str().unwrap_or("");
            let duration_ms = arguments["durationMs"].as_u64().unwrap_or(1000);
            let max_fires = arguments["maxFires"].as_u64().map(|v| v as u32);
            let kind = match kind_str {
                "delay" => crate::wa::triggers::TriggerKind::Delay(
                    std::time::Duration::from_millis(duration_ms),
                ),
                "interval" => crate::wa::triggers::TriggerKind::Interval {
                    period: std::time::Duration::from_millis(duration_ms),
                    max_fires,
                },
                "file_watch" => crate::wa::triggers::TriggerKind::FileWatch {
                    path: std::path::PathBuf::from(target),
                    event: crate::wa::triggers::FileWatchEvent::Any,
                },
                "window_appears" => crate::wa::triggers::TriggerKind::WindowAppears {
                    title_contains: target.to_string(),
                },
                "window_closes" => crate::wa::triggers::TriggerKind::WindowCloses {
                    title_contains: target.to_string(),
                },
                "process_starts" => crate::wa::triggers::TriggerKind::ProcessStarts {
                    name_contains: target.to_string(),
                },
                "process_exits" => crate::wa::triggers::TriggerKind::ProcessExits {
                    pid: target
                        .parse::<u32>()
                        .map_err(|_| "process_exits requires a numeric target pid")?,
                },
                "clipboard_changed" => crate::wa::triggers::TriggerKind::ClipboardChanged,
                "system_idle" => crate::wa::triggers::TriggerKind::SystemIdle {
                    idle_threshold: std::time::Duration::from_millis(duration_ms),
                },
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown trigger kind '{other}'"
                    )))
                }
            };
            let created_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let definition = crate::wa::triggers::TriggerDefinition {
                id: arguments["id"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("trigger-{created_at_ms}")),
                name: name.to_string(),
                kind,
                action: crate::wa::triggers::TriggerAction::RunPowerShell(
                    action_script.to_string(),
                ),
                enabled: arguments["enabled"].as_bool().unwrap_or(true),
                max_fires,
                fire_count: 0,
                created_at_ms,
                last_fired_at_ms: None,
            };
            let mut mgr = lock_trigger_manager()?;
            let registered = mgr.register(definition);
            let (id, registered_name, enabled) = (
                registered.id.clone(),
                registered.name.clone(),
                registered.enabled,
            );
            let active_count = mgr.active_triggers().len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "id": id,
                "name": registered_name,
                "kind": kind_str,
                "enabled": enabled,
                "active_count": active_count,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise trigger registration: {err}"))
            })?
        }
        "wa_trigger_list" => {
            let mgr = lock_trigger_manager()?;
            let active = mgr.active_triggers();
            let triggers: Vec<serde_json::Value> = active
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "name": t.name,
                        "enabled": t.enabled,
                        "fire_count": t.fire_count,
                        "max_fires": t.max_fires,
                        "created_at_ms": t.created_at_ms,
                        "last_fired_at_ms": t.last_fired_at_ms,
                    })
                })
                .collect();
            let count = triggers.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "count": count,
                "triggers": triggers,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise trigger list: {err}")))?
        }
        "wa_trigger_fire" => {
            let trigger_id = arguments["triggerId"]
                .as_str()
                .ok_or("triggerId is required")?;
            let mut mgr = lock_trigger_manager()?;
            match mgr.fire(trigger_id) {
                Some(result) => serde_json::to_string(&serde_json::json!({
                    "success": result.success,
                    "trigger_id": result.trigger_id,
                    "fired_at_ms": result.fired_at_ms,
                    "detail": result.detail,
                    "action_output": result.action_output,
                }))
                .map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise trigger fire result: {err}"))
                })?,
                None => serde_json::to_string(&serde_json::json!({
                    "success": false,
                    "trigger_id": trigger_id,
                    "error": format!("Trigger '{trigger_id}' not found"),
                }))
                .map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise trigger fire error: {err}"))
                })?,
            }
        }
        "wa_trigger_remove" => {
            let trigger_id = arguments["triggerId"]
                .as_str()
                .ok_or("triggerId is required")?;
            let mut mgr = lock_trigger_manager()?;
            let removed = mgr.remove(trigger_id);
            let remaining = mgr.active_triggers().len();
            serde_json::to_string(&serde_json::json!({
                "success": removed,
                "trigger_id": trigger_id,
                "active_count": remaining,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise trigger removal: {err}")))?
        }
        // ─── Recovery ──────────────────────────────────────────────────────────
        "wa_recovery_set_policy" => {
            let max_attempts = arguments["maxRetries"].as_u64().unwrap_or(3) as u32;
            let base_delay = arguments["baseDelayMs"].as_u64().unwrap_or(500);
            let cb_threshold = arguments["circuitBreakerThreshold"].as_u64().unwrap_or(5) as u32;
            // Persist into the process-global recovery state so `wa_recovery_get_status`
            // (and future retry-wrapped operations) observe the configured policy.
            let mut state = lock_recovery_state()?;
            state.0.max_attempts = max_attempts;
            state.0.initial_delay = std::time::Duration::from_millis(base_delay);
            state.1.failure_threshold = cb_threshold;
            format!(
                "{{\"success\":true,\"max_attempts\":{max_attempts},\"base_delay_ms\":{base_delay},\"circuit_breaker_threshold\":{cb_threshold}}}"
            )
        }
        "wa_recovery_get_status" => {
            let mut state = lock_recovery_state()?;
            let allowed = state.1.should_allow();
            let state_str = match state.1.state {
                crate::wa::recovery::CircuitState::Closed => "closed",
                crate::wa::recovery::CircuitState::Open => "open",
                crate::wa::recovery::CircuitState::HalfOpen => "half_open",
            };
            format!(
                "{{\"circuit_allowed\":{allowed},\"state\":\"{state_str}\",\"failure_count\":{},\"failure_threshold\":{},\"max_attempts\":{},\"base_delay_ms\":{}}}",
                state.1.failure_count,
                state.1.failure_threshold,
                state.0.max_attempts,
                state.0.initial_delay.as_millis()
            )
        }
        // ─── Events ────────────────────────────────────────────────────────────
        "wa_event_subscribe" => {
            let event_kind = arguments["eventKind"]
                .as_str()
                .ok_or("eventKind is required")?;
            let timeout_ms = arguments["timeoutMs"].as_u64().unwrap_or(5000);
            let kind =
                crate::wa::events::UiaEventKind::from_api_str(event_kind).ok_or_else(|| {
                    format!(
                        "unknown eventKind '{event_kind}' (expected window_opened, window_closed, element_focus, structure_changed, element_value_changed)"
                    )
                })?;
            // Subscribe to exactly what the polled listener can observe rather
            // than always polling focus changes behind the caller's back (bug #15).
            let label = kind.poller_label().ok_or_else(|| {
                format!(
                    "eventKind '{event_kind}' is not capturable by the polled UIA listener; supported kinds are window_opened, window_closed, element_focus, structure_changed"
                )
            })?;
            let subscription = crate::wa::events::EventSubscription {
                event_kinds: vec![kind],
                process_filter: arguments["processId"].as_u64().map(|v| v as u32),
                duration: std::time::Duration::from_millis(timeout_ms),
                ..Default::default()
            };
            let mut listener = crate::wa::events::EventListener::new();
            let listen = listener.listen(&subscription);
            let captured = listen.events.len();
            let timed_out = listen.timed_out;
            let hit_event_limit = listen.hit_event_limit;
            let warnings = listen.errors;
            // Persist captured events into the process-global buffer so a later
            // `wa_event_poll` can retrieve them; subscribe alone is fire-and-forget.
            let buffered = {
                let mut buffer = lock_event_buffer()?;
                for event in listen.events {
                    buffer.push(event);
                }
                buffer.len()
            };
            // A budget-killed or unparseable listener also yields zero events, so
            // report the failure instead of a clean-looking `subscribed:true`.
            if captured == 0 && !warnings.is_empty() {
                return Err(Box::<dyn Error>::from(format!(
                    "wa_event_subscribe('{event_kind}') captured nothing: {}",
                    warnings.join("; ")
                )));
            }
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "subscribed": true,
                "event_kind": event_kind,
                "captured_kind": label,
                "events_captured": captured,
                "buffered_total": buffered,
                "timed_out": timed_out,
                "hit_event_limit": hit_event_limit,
                "warnings": warnings,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise event subscription: {err}")))?
        }
        "wa_event_poll" => {
            let max_events = arguments["maxEvents"].as_u64().unwrap_or(20) as usize;
            let since = LAST_EVENT_POLL_MS.load(std::sync::atomic::Ordering::Relaxed);
            let buffer = lock_event_buffer()?;
            // Return events captured since the previous poll (newest first), capped at
            // `max_events`, then advance the cursor so each event is reported once.
            let mut since_events = buffer.events_since(since);
            since_events.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms));
            let events: Vec<serde_json::Value> = since_events
                .into_iter()
                .take(max_events)
                .map(uia_event_json)
                .collect();
            let count = events.len();
            let buffered_total = buffer.len();
            LAST_EVENT_POLL_MS.store(wall_clock_ms(), std::sync::atomic::Ordering::Relaxed);
            serde_json::to_string(&serde_json::json!({
                "count": count,
                "buffered_total": buffered_total,
                "events": events,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise polled events: {err}")))?
        }
        "wa_event_unsubscribe" => {
            let mut buffer = lock_event_buffer()?;
            let cleared = buffer.len();
            buffer.clear();
            LAST_EVENT_POLL_MS.store(wall_clock_ms(), std::sync::atomic::Ordering::Relaxed);
            format!(
                "{{\"success\":true,\"detail\":\"Listener stopped\",\"events_cleared\":{cleared}}}"
            )
        }
        // ─── File Dialog ───────────────────────────────────────────────────────
        "wa_file_dialog_detect" => {
            // Probe for a live file dialog and report what was found, including the
            // detected kind (open / save_as / folder_browse) that the
            // `wa_file_dialog_open`/`_save` tools need to target correctly.
            let target = crate::wa::file_dialog::FileDialogTarget {
                process_id: arguments["processId"].as_u64().map(|v| v as u32),
                title_contains: arguments["titleContains"].as_str().map(str::to_string),
                wait_timeout: std::time::Duration::from_millis(
                    arguments["timeoutMs"].as_u64().unwrap_or(5000),
                ),
                ..Default::default()
            };
            let detected = crate::wa::file_dialog::FileDialogManager::detect_dialog(&target);
            serde_json::to_string(&match detected {
                Some(dialog) => serde_json::json!({
                    "success": true,
                    "found": true,
                    "hwnd": dialog.hwnd,
                    "process_id": dialog.process_id,
                    "title": dialog.title,
                    "kind": crate::wa::file_dialog::dialog_kind_token(dialog.kind),
                }),
                None => serde_json::json!({
                    "success": true,
                    "found": false,
                }),
            })
            .map_err(|err| Box::<dyn Error>::from(format!("serialise file dialog detect: {err}")))?
        }
        "wa_file_dialog_open" => {
            let file_path = arguments["filePath"]
                .as_str()
                .ok_or("filePath is required")?;
            // Build a real target (honouring `processId` and the Open kind) and drive
            // the dialog through it so the advertised pid filter is actually applied.
            let target = crate::wa::file_dialog::FileDialogTarget {
                process_id: arguments["processId"].as_u64().map(|v| v as u32),
                kind: crate::wa::file_dialog::FileDialogKind::Open,
                ..Default::default()
            };
            let result = crate::wa::file_dialog::FileDialogManager::wait_and_set_path(
                std::path::Path::new(file_path),
                &target,
            );
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success,
                result.detail.replace('"', "\\\"")
            )
        }
        "wa_file_dialog_save" => {
            let file_path = arguments["filePath"]
                .as_str()
                .ok_or("filePath is required")?;
            let target = crate::wa::file_dialog::FileDialogTarget {
                process_id: arguments["processId"].as_u64().map(|v| v as u32),
                kind: crate::wa::file_dialog::FileDialogKind::SaveAs,
                ..Default::default()
            };
            let result = crate::wa::file_dialog::FileDialogManager::wait_and_set_path(
                std::path::Path::new(file_path),
                &target,
            );
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success,
                result.detail.replace('"', "\\\"")
            )
        }
        // ─── Virtual Desktop Extended ──────────────────────────────────────────
        "wa_vdesktop_create" => {
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let name = arguments["name"].as_str().map(|s| s.to_string());
            let op = crate::wa::virtual_desktop::VDesktopOperation::Create { name };
            let result = mgr.apply(&op);
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success,
                result.detail.replace('"', "\\\"")
            )
        }
        "wa_vdesktop_remove" => {
            let index = arguments["index"].as_u64().ok_or("index is required")? as u32;
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let op = crate::wa::virtual_desktop::VDesktopOperation::Remove(index);
            let result = mgr.apply(&op);
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success,
                result.detail.replace('"', "\\\"")
            )
        }
        "wa_vdesktop_move_window" => {
            let hwnd = arguments["hwnd"].as_u64().ok_or("hwnd is required")?;
            let desktop_index = arguments["targetIndex"]
                .as_u64()
                .ok_or("targetIndex is required")? as u32;
            let mut mgr = crate::wa::virtual_desktop::VirtualDesktopManager::new();
            let op = crate::wa::virtual_desktop::VDesktopOperation::MoveWindow {
                hwnd,
                desktop_index,
            };
            let result = mgr.apply(&op);
            format!(
                "{{\"success\":{},\"detail\":\"{}\"}}",
                result.success,
                result.detail.replace('"', "\\\"")
            )
        }
        "wa_monitor_list" => {
            // Enumerate the physical displays (bounds, work area, DPI, colour depth)
            // so callers can target `wa_window_tile` and screenshot tools at a
            // specific monitor instead of guessing coordinates.
            let mut manager = crate::wa::multi_monitor::MultiMonitorManager::empty();
            let refreshed = manager.refresh();
            let monitors: Vec<serde_json::Value> = manager
                .all()
                .iter()
                .map(|m| {
                    let orientation = match m.orientation {
                        crate::wa::multi_monitor::MonitorOrientation::Landscape => "landscape",
                        crate::wa::multi_monitor::MonitorOrientation::Portrait => "portrait",
                        crate::wa::multi_monitor::MonitorOrientation::LandscapeFlipped => {
                            "landscape_flipped"
                        }
                        crate::wa::multi_monitor::MonitorOrientation::PortraitFlipped => {
                            "portrait_flipped"
                        }
                    };
                    serde_json::json!({
                        "index": m.index,
                        "device_name": m.device_name,
                        "friendly_name": m.friendly_name,
                        "is_primary": m.is_primary,
                        "bounds": {
                            "x": m.bounds.x,
                            "y": m.bounds.y,
                            "width": m.bounds.width,
                            "height": m.bounds.height,
                        },
                        "work_area": {
                            "x": m.work_area.x,
                            "y": m.work_area.y,
                            "width": m.work_area.width,
                            "height": m.work_area.height,
                        },
                        "dpi": m.dpi,
                        "dpi_scale": m.dpi_scale,
                        "physical_width": m.physical_width,
                        "physical_height": m.physical_height,
                        "refresh_rate": m.refresh_rate,
                        "bits_per_pixel": m.bits_per_pixel,
                        "orientation": orientation,
                    })
                })
                .collect();
            serde_json::to_string(&serde_json::json!({
                "success": refreshed,
                "monitor_count": monitors.len(),
                "monitors": monitors,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise monitor list: {err}")))?
        }
        // ─── Window Tiling ─────────────────────────────────────────────────────
        "wa_window_tile" => {
            // Bug #37: this used to enumerate *every* titled window in the session
            // and tile all of it, and an out-of-range `monitor` silently fell back
            // to the primary display. `monitor: 99` therefore rearranged the whole
            // desktop and still answered success. It now names its targets.
            let requested: Vec<u64> = arguments["hwnds"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
                .unwrap_or_default();
            if requested.is_empty() {
                return Err(Box::<dyn Error>::from(
                    "hwnds is required: wa_window_tile will not rearrange every window on the \
                     desktop. Pass the handles returned by wa_window_list.",
                ));
            }
            let monitor_index = arguments["monitor"].as_u64().unwrap_or(0) as u32;
            let mut mm = crate::wa::multi_monitor::MultiMonitorManager::empty();
            if !mm.refresh() || mm.count() == 0 {
                return Err(Box::<dyn Error>::from(
                    "no monitors could be enumerated, so tile bounds cannot be computed",
                ));
            }
            let monitor = match mm.get(monitor_index) {
                Some(m) => Some((m.work_area.width, m.work_area.height)),
                None => None,
            };
            let (mw, mh) = match monitor {
                Some(dims) => dims,
                None => {
                    let valid: Vec<u32> = (0..mm.count() as u32).collect();
                    return Err(Box::<dyn Error>::from(format!(
                        "no monitor at index {monitor_index} (valid indices: {valid:?})"
                    )));
                }
            };
            if mw == 0 || mh == 0 {
                return Err(Box::<dyn Error>::from(format!(
                    "monitor {monitor_index} reports a {mw}x{mh} work area; refusing to tile"
                )));
            }
            // Skip handles that are no longer live windows rather than issuing
            // MoveWindow against garbage.
            let live: Vec<u64> = crate::wa::window_mgmt::WindowManager::enumerate_windows()
                .iter()
                .map(|w| w.hwnd)
                .collect();
            let targets: Vec<u64> = requested
                .iter()
                .copied()
                .filter(|h| live.contains(h))
                .collect();
            let skipped: Vec<u64> = requested
                .iter()
                .copied()
                .filter(|h| !live.contains(h))
                .collect();
            if targets.is_empty() {
                return Err(Box::<dyn Error>::from(format!(
                    "none of the {} requested handle(s) are live windows",
                    requested.len()
                )));
            }
            let columns = arguments["columns"].as_u64().map(|v| v as u32);
            let results =
                crate::wa::window_mgmt::WindowManager::tile_windows(&targets, mw, mh, columns);
            let succeeded = results.iter().filter(|r| r.success).count();
            let failures: Vec<serde_json::Value> = results
                .iter()
                .filter(|r| !r.success)
                .map(|r| serde_json::json!({ "hwnd": r.hwnd, "detail": r.detail }))
                .collect();
            serde_json::to_string(&serde_json::json!({
                "success": succeeded == targets.len(),
                "windows_tiled": succeeded,
                "windows_requested": targets.len(),
                "windows_skipped": skipped,
                "columns": columns,
                "monitor": monitor_index,
                "monitor_work_area": [mw, mh],
                "failures": failures,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise tile result: {err}")))?
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
            format!(
                "{{\"success\":{},\"browser\":\"{}\",\"url\":\"{}\",\"pid\":{}}}",
                result.success,
                browser,
                url,
                result
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "null".to_string())
            )
        }
        "wa_browser_screenshot" => {
            let output_path = arguments["outputPath"]
                .as_str()
                .unwrap_or("browser_screenshot.png");
            let img =
                crate::wa::screenshot::capture(&crate::wa::screenshot::CaptureTarget::FullScreen);
            // Bug #23: this defaulted to `.png` but wrote BMP bytes, and discarded
            // the write error so a failed save still reported `success: true`
            // whenever pixels had been captured.
            let saved = img.save_to(std::path::Path::new(output_path));
            serde_json::to_string(&serde_json::json!({
                "success": saved.is_ok() && img.pixel_count() > 0,
                "path": output_path,
                "format": crate::wa::screenshot::image_format_for_path(std::path::Path::new(
                    output_path,
                )),
                "width": img.width,
                "height": img.height,
                "pixel_count": img.pixel_count(),
                "detail": saved.err().map(|e| e.to_string()),
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise screenshot result: {err}")))?
        }
        // ─── Screenshot ──────────────────────────────────────────────────────
        "wa_screenshot" => {
            let output_path = arguments["outputPath"].as_str().unwrap_or("screenshot.png");
            let target = if let Some(pid) = arguments["pid"].as_u64() {
                crate::wa::screenshot::CaptureTarget::Window(pid as u32)
            } else if let Some(region) = arguments["region"].as_object() {
                crate::wa::screenshot::CaptureTarget::Region(crate::wa::screenshot::CaptureRegion {
                    x: region.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    y: region.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    width: region.get("width").and_then(|v| v.as_u64()).unwrap_or(1920) as u32,
                    height: region
                        .get("height")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1080) as u32,
                })
            } else {
                crate::wa::screenshot::CaptureTarget::FullScreen
            };
            let img = crate::wa::screenshot::capture(&target);
            // Encode to whatever the requested path asks for and report the format
            // that was actually written, so a `.png` request yields PNG bytes
            // (bug #23: `save_bmp` was the only writer and `"format"` was a literal).
            let format =
                crate::wa::screenshot::image_format_for_path(std::path::Path::new(output_path));
            let saved = img.save_to(std::path::Path::new(output_path));
            serde_json::to_string(&serde_json::json!({
                "success": saved.is_ok() && img.pixel_count() > 0,
                "path": output_path,
                "format": format,
                "width": img.width,
                "height": img.height,
                "pixel_count": img.pixel_count(),
                "detail": saved.err().map(|e| e.to_string()),
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise screenshot result: {err}")))?
        }
        // ─── System Tray / UAC ───────────────────────────────────────────────
        "wa_tray_list" => {
            let icons: Vec<serde_json::Value> =
                crate::wa::notifications::NotificationManager::get_tray_icons()
                    .iter()
                    .map(|icon| {
                        serde_json::json!({
                            "tooltip": icon.tooltip,
                            "process_id": icon.process_id,
                            "is_visible": icon.is_visible,
                        })
                    })
                    .collect();
            let count = icons.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "count": count,
                "icons": icons,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise tray icons: {err}")))?
        }
        "wa_tray_click" => {
            let tooltip = arguments["tooltip"].as_str().ok_or("tooltip is required")?;
            let action = match arguments["action"].as_str().unwrap_or("click") {
                "double_click" | "doubleclick" => crate::wa::notifications::TrayAction::DoubleClick,
                "right_click" | "rightclick" => crate::wa::notifications::TrayAction::RightClick,
                "click" => crate::wa::notifications::TrayAction::Click,
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown tray action '{other}'"
                    )))
                }
            };
            let result =
                crate::wa::notifications::NotificationManager::click_tray_icon(tooltip, &action);
            serde_json::to_string(&serde_json::json!({
                "success": result.success,
                "action": result.action,
                "tooltip": tooltip,
                "detail": result.detail,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise tray click: {err}")))?
        }
        "wa_uac_detect" => {
            let visible = crate::wa::notifications::NotificationManager::is_uac_prompt_visible();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "uac_prompt_visible": visible,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise UAC detection: {err}")))?
        }
        // ─── OCR extras ──────────────────────────────────────────────────────
        "wa_ocr_languages" => {
            let languages = crate::wa::ocr::OcrEngine::available_languages();
            let count = languages.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "count": count,
                "languages": languages,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise OCR languages: {err}")))?
        }
        // ─── System DPI ──────────────────────────────────────────────────────
        "wa_system_dpi" => {
            let dpi = crate::wa::registry::SystemSettingsManager::get_dpi();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "operation": "query",
                "dpi": dpi,
                "scale_percent": dpi.map(|d| (d as f64 / 96.0) * 100.0),
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise DPI query: {err}")))?
        }
        // ─── Desktop Platform ────────────────────────────────────────────────
        "wa_platform_info" => {
            let kind = crate::wa::platform::DesktopAutomationAdapter::platform_kind();
            let kind_str = match kind {
                crate::wa::platform::DesktopPlatformKind::Windows => "windows",
                crate::wa::platform::DesktopPlatformKind::Linux => "linux",
                crate::wa::platform::DesktopPlatformKind::MacOs => "macos",
            };
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "platform": kind_str,
                "automation_supported": kind_str == "windows",
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise platform info: {err}")))?
        }
        "wa_platform_tree_snapshot" => {
            let app_name = arguments["appName"].as_str().unwrap_or("");
            match crate::wa::platform::DesktopAutomationAdapter::capture_tree_snapshot(
                root, app_name,
            ) {
                Ok(snapshot_id) => serde_json::to_string(&serde_json::json!({
                    "success": true,
                    "app_name": app_name,
                    "snapshot": snapshot_id,
                }))
                .map_err(|err| Box::<dyn Error>::from(format!("serialise tree snapshot: {err}")))?,
                Err(detail) => serde_json::to_string(&serde_json::json!({
                    "success": false,
                    "app_name": app_name,
                    "detail": detail,
                }))
                .map_err(|err| Box::<dyn Error>::from(format!("serialise tree snapshot: {err}")))?,
            }
        }
        // ─── Advanced Input Injection ────────────────────────────────────────
        "wa_input_sequence" => {
            let label = arguments["label"].as_str().unwrap_or("mcp-input-sequence");
            let steps = arguments["steps"]
                .as_array()
                .ok_or("steps array is required")?;
            let mut sequence = crate::wa::advanced_input::InputSequence::new(label);
            for step in steps {
                let op = step["op"]
                    .as_str()
                    .ok_or("each step requires an 'op' field")?;
                match op {
                    "click" | "right_click" | "middle_click" => {
                        let x = step["x"].as_i64().ok_or("click requires 'x'")? as i32;
                        let y = step["y"].as_i64().ok_or("click requires 'y'")? as i32;
                        match op {
                            "click" => sequence.click_at(x, y),
                            "right_click" => sequence.right_click_at(x, y),
                            _ => sequence.push(crate::wa::advanced_input::InputEvent::MouseClick {
                                button: crate::wa::advanced_input::MouseButton::Middle,
                                x,
                                y,
                            }),
                        };
                    }
                    "move" => {
                        let x = step["x"].as_i64().ok_or("move requires 'x'")? as i32;
                        let y = step["y"].as_i64().ok_or("move requires 'y'")? as i32;
                        sequence.push(crate::wa::advanced_input::InputEvent::MouseMove {
                            x,
                            y,
                            absolute: step["absolute"].as_bool().unwrap_or(true),
                        });
                    }
                    "type" => {
                        let text = step["text"].as_str().ok_or("type requires 'text'")?;
                        sequence.type_text(text);
                    }
                    "key_combo" => {
                        let key = step["key"].as_str().ok_or("key_combo requires 'key'")?;
                        let modifiers = parse_key_modifiers(step["modifiers"].as_array())?;
                        sequence.key_combo(&modifiers, key);
                    }
                    "scroll" => {
                        sequence.scroll(
                            step["vertical"].as_i64().unwrap_or(0) as i32,
                            step["horizontal"].as_i64().unwrap_or(0) as i32,
                        );
                    }
                    "wait" => {
                        sequence.wait_ms(step["ms"].as_u64().unwrap_or(100));
                    }
                    "drag_drop" => {
                        let from = parse_screen_point(&step["from"])?;
                        let to = parse_screen_point(&step["to"])?;
                        sequence.drag_drop(from, to);
                    }
                    other => {
                        return Err(Box::<dyn Error>::from(format!(
                            "unknown input op '{other}'"
                        )))
                    }
                }
            }
            let event_count = sequence.events.len();
            // `viaScript` opts out of native SendInput and drives the sequence through
            // the generated PowerShell script instead, which is the portable path and
            // the only one that works when native injection is blocked.
            let via_script = arguments["viaScript"].as_bool().unwrap_or(false);
            let result = if via_script {
                crate::wa::advanced_input::execute_sequence_script(&sequence)
            } else {
                crate::wa::advanced_input::execute_sequence(&sequence)
            };
            serde_json::to_string(&serde_json::json!({
                "success": result.success,
                "label": label,
                "events_queued": event_count,
                "events_sent": result.events_sent,
                "via_script": via_script,
                "detail": result.detail,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise input sequence result: {err}"))
            })?
        }
        // ─── Interaction Recording ───────────────────────────────────────────
        "wa_record_start" => {
            let session_id = arguments["sessionId"].as_str().unwrap_or("default");
            let duration_seconds = arguments["durationSeconds"].as_u64().unwrap_or(30) as u32;
            let process_id = arguments["processId"].as_u64().map(|v| v as u32);
            let window_title = arguments["windowTitle"].as_str();
            let mut session = lock_recording_session()?;
            session.session_id = session_id.to_string();
            session.start(process_id, window_title);
            let state = recording_state_str(session.state);
            // The hook script is what actually produces events on Windows; hand it back
            // so the caller can run it and feed the JSON lines to `wa_record_ingest`.
            let hook_script =
                crate::wa::recording::build_recording_hook_script(process_id, duration_seconds);
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "session_id": session_id,
                "state": state,
                "filter_process_id": process_id,
                "filter_window_title": window_title,
                "hook_script": hook_script,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise recording start: {err}")))?
        }
        "wa_record_ingest" => {
            let raw_events = arguments["events"]
                .as_array()
                .ok_or("events array is required")?;
            let mut session = lock_recording_session()?;
            let mut ingested = 0usize;
            for raw in raw_events {
                let event = parse_recorded_event(raw)?;
                session.push_event(event);
                ingested += 1;
            }
            let total = session.event_count();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "ingested": ingested,
                "total_events": total,
                "state": recording_state_str(session.state),
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise recording ingest: {err}")))?
        }
        "wa_record_pause" => {
            let mut session = lock_recording_session()?;
            let resume = arguments["resume"].as_bool().unwrap_or(false);
            if resume {
                session.resume();
            } else {
                session.pause();
            }
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "state": recording_state_str(session.state),
                "event_count": session.event_count(),
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise recording pause: {err}")))?
        }
        "wa_record_status" => {
            let session = lock_recording_session()?;
            let event_count = session.event_count();
            let elapsed_ms = session.elapsed().as_millis() as u64;
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "session_id": session.session_id,
                "state": recording_state_str(session.state),
                "event_count": event_count,
                "elapsed_ms": elapsed_ms,
                "filter_process_id": session.filter_process_id,
                "filter_window_title": session.filter_window_title,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise recording status: {err}")))?
        }
        "wa_record_replay_plan" => {
            let session = lock_recording_session()?;
            let config = crate::wa::recording::ReplayConfig {
                speed_multiplier: arguments["speedMultiplier"].as_f64().unwrap_or(1.0),
                min_step_delay: std::time::Duration::from_millis(
                    arguments["minStepDelayMs"].as_u64().unwrap_or(50),
                ),
                verify_postconditions: arguments["verifyPostconditions"].as_bool().unwrap_or(true),
                stop_on_failure: arguments["stopOnFailure"].as_bool().unwrap_or(true),
            };
            let delays = crate::wa::recording::compute_replay_delays(&session.events, &config);
            let total_ms: u64 = delays.iter().map(|d| d.as_millis() as u64).sum();
            let steps: Vec<serde_json::Value> = delays
                .iter()
                .map(|d| serde_json::json!({ "delay_ms": d.as_millis() as u64 }))
                .collect();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "step_count": steps.len(),
                "total_replay_ms": total_ms,
                "speed_multiplier": config.speed_multiplier,
                "steps": steps,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise recording replay plan: {err}"))
            })?
        }
        "wa_record_stop" => {
            let script_name = arguments["scriptName"]
                .as_str()
                .unwrap_or("recorded-script");
            let persist = arguments["persist"].as_bool().unwrap_or(true);
            let mut session = lock_recording_session()?;
            session.stop();
            let event_count = session.event_count();
            let elapsed_ms = session.elapsed().as_millis() as u64;
            let script_path = if persist && event_count > 0 {
                Some(crate::wa::recording::persist_recording(
                    root,
                    &session,
                    script_name,
                )?)
            } else {
                None
            };
            let state = recording_state_str(session.state);
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "state": state,
                "event_count": event_count,
                "elapsed_ms": elapsed_ms,
                "script_name": script_name,
                "script_path": script_path,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise recording stop: {err}")))?
        }
        // ─── Cross-Context Browser/Desktop Bridge ────────────────────────────
        "wa_bridge_run" => {
            let name = arguments["name"].as_str().unwrap_or("bridge-workflow");
            let raw_steps = arguments["steps"]
                .as_array()
                .ok_or("steps array is required")?;
            if raw_steps.is_empty() {
                return Err(Box::<dyn Error>::from("steps must not be empty"));
            }
            let mut steps = Vec::with_capacity(raw_steps.len());
            for raw in raw_steps {
                steps.push(parse_bridge_step(raw)?);
            }
            let workflow = crate::wa::browser_bridge::BridgeWorkflow {
                name: name.to_string(),
                steps,
                timeout: std::time::Duration::from_millis(
                    arguments["timeoutMs"].as_u64().unwrap_or(30_000),
                ),
                fail_fast: arguments["failFast"].as_bool().unwrap_or(true),
            };
            let download_dir = arguments["downloadDir"]
                .as_str()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| root.join("output").join("downloads"));
            let executor = crate::wa::browser_bridge::BridgeExecutor::new(download_dir);
            let result = executor.execute(&workflow);
            let step_results: Vec<serde_json::Value> = result
                .steps
                .iter()
                .map(|step| {
                    serde_json::json!({
                        "step_index": step.step_index,
                        "context": step.context,
                        "success": step.success,
                        "detail": step.detail,
                        "elapsed_ms": step.elapsed.as_millis() as u64,
                        "data": step.data,
                    })
                })
                .collect();
            serde_json::to_string(&serde_json::json!({
                "success": result.succeeded,
                "workflow_name": result.workflow_name,
                "total_elapsed_ms": result.total_elapsed.as_millis() as u64,
                "stopped_at": result.stopped_at,
                "step_count": step_results.len(),
                "steps": step_results,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise bridge result: {err}")))?
        }
        // ─── UIA Root / Cache Control ────────────────────────────────────────
        "wa_uia_root" => {
            let pid = arguments["processId"]
                .as_u64()
                .ok_or("processId is required")? as u32;
            let max_depth = arguments["maxDepth"].as_u64().unwrap_or(4) as u32;
            let max_children = arguments["maxChildren"].as_u64().unwrap_or(64) as u32;
            let refresh = arguments["refresh"].as_bool().unwrap_or(false);
            let mut client = crate::wa::uia_ffi::UiaDirectClient::initialize_for_process(pid)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA init failed: {e}")))?;
            let had_fresh_cache = client.has_fresh_cache(pid);
            if refresh {
                client.invalidate_cache();
            }
            // Rebuild so the cache is populated before the root is read back out.
            client
                .build_tree(pid, max_depth, max_children)
                .map_err(|e| Box::<dyn Error>::from(format!("UIA tree build failed: {e}")))?;
            let scope = arguments["scope"].as_str().unwrap_or("process");
            let root_element = match scope {
                "process" => client.get_process_root(pid),
                "desktop" => client.get_root_element(),
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown UIA root scope '{other}' (expected process or desktop)"
                    )))
                }
            };
            match root_element {
                Ok(element) => serde_json::to_string(&serde_json::json!({
                    "success": true,
                    "process_id": pid,
                    "scope": scope,
                    "had_fresh_cache": had_fresh_cache,
                    "cache_invalidated": refresh,
                    "root": uia_element_json(&element),
                }))
                .map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise UIA root element: {err}"))
                })?,
                Err(detail) => serde_json::to_string(&serde_json::json!({
                    "success": false,
                    "process_id": pid,
                    "scope": scope,
                    "had_fresh_cache": had_fresh_cache,
                    "cache_invalidated": refresh,
                    "detail": detail,
                }))
                .map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise UIA root element: {err}"))
                })?,
            }
        }
        // ─── CSS / XPath Selector Resolution ─────────────────────────────────
        "wa_resolve_css_selector" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let css_selector = arguments["cssSelector"]
                .as_str()
                .ok_or("cssSelector is required")?;
            let matches = crate::wa::selector::resolve_css_selector(
                root,
                session_id,
                arguments["snapshotName"].as_str(),
                css_selector,
            )?;
            serialise_selector_matches("css", css_selector, &matches)?
        }
        "wa_resolve_xpath" => {
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let xpath = arguments["xpath"].as_str().ok_or("xpath is required")?;
            let matches = crate::wa::selector::resolve_xpath(
                root,
                session_id,
                arguments["snapshotName"].as_str(),
                xpath,
            )?;
            serialise_selector_matches("xpath", xpath, &matches)?
        }
        // ─── Recovery Planning ───────────────────────────────────────────────
        "wa_recovery_plan" => {
            let scenario = arguments["scenario"]
                .as_str()
                .unwrap_or("element_not_found");
            let plan = match scenario {
                "element_not_found" => crate::wa::recovery::RecoveryPlan::for_element_not_found(),
                "blocked_by_popup" => crate::wa::recovery::RecoveryPlan::for_blocked_by_popup(),
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown recovery scenario '{other}' (expected element_not_found or blocked_by_popup)"
                    )))
                }
            };
            let actions: Vec<String> = plan.actions.iter().map(recovery_action_str).collect();
            let action_count = actions.len();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "scenario": scenario,
                "trigger_error": plan.trigger_error,
                "action_count": action_count,
                "actions": actions,
                "max_recovery_time_ms": plan.max_recovery_time.as_millis() as u64,
            }))
            .map_err(|err| Box::<dyn Error>::from(format!("serialise recovery plan: {err}")))?
        }
        "wa_recovery_adaptive_wait" => {
            let mut wait = crate::wa::recovery::AdaptiveWait::default();
            if let Some(observations) = arguments["observationsMs"].as_array() {
                for observation in observations {
                    if let Some(ms) = observation.as_u64() {
                        wait.record_observation(std::time::Duration::from_millis(ms));
                    }
                }
            }
            let interval = wait.recommended_poll_interval();
            serde_json::to_string(&serde_json::json!({
                "success": true,
                "observations": wait.observed_ready_times.len(),
                "estimated_ready_ms": wait.estimated_ready_time.as_millis() as u64,
                "recommended_poll_interval_ms": interval.as_millis() as u64,
                "min_wait_ms": wait.min_wait.as_millis() as u64,
                "max_wait_ms": wait.max_wait.as_millis() as u64,
            }))
            .map_err(|err| {
                Box::<dyn Error>::from(format!("serialise adaptive wait recommendation: {err}"))
            })?
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
}

/// Serialise CSS/XPath selector matches into a uniform tool payload.
fn serialise_selector_matches(
    kind: &str,
    expression: &str,
    matches: &[(i32, crate::wa::model::WaNode)],
) -> Result<String, Box<dyn Error>> {
    let nodes: Vec<serde_json::Value> = matches
        .iter()
        .map(|(index, node)| {
            serde_json::json!({
                "index": index,
                "node": node,
            })
        })
        .collect();
    let count = nodes.len();
    serde_json::to_string(&serde_json::json!({
        "success": true,
        "selector_kind": kind,
        "expression": expression,
        "match_count": count,
        "matches": nodes,
    }))
    .map_err(|err| Box::<dyn Error>::from(format!("serialise selector matches: {err}")))
}

/// Render a `RecoveryAction` as a stable, human-readable string.
fn recovery_action_str(action: &crate::wa::recovery::RecoveryAction) -> String {
    match action {
        crate::wa::recovery::RecoveryAction::Wait(duration) => {
            format!("wait:{}ms", duration.as_millis())
        }
        crate::wa::recovery::RecoveryAction::RefreshSnapshot => "refresh_snapshot".to_string(),
        crate::wa::recovery::RecoveryAction::FocusTargetWindow => "focus_target_window".to_string(),
        crate::wa::recovery::RecoveryAction::DismissPopup => "dismiss_popup".to_string(),
        crate::wa::recovery::RecoveryAction::SendEscape => "send_escape".to_string(),
        crate::wa::recovery::RecoveryAction::RestartProcess { exe_path } => {
            format!("restart_process:{exe_path}")
        }
        crate::wa::recovery::RecoveryAction::CustomScript(script) => {
            format!("custom_script:{script}")
        }
    }
}

/// Parse one bridge workflow step into its typed `BridgeStep` form.
///
/// Each step carries a `context` ("browser", "desktop", "wait", "transfer") plus an
/// `action` naming the variant, so the full cross-context surface is reachable.
fn parse_bridge_step(
    raw: &serde_json::Value,
) -> Result<crate::wa::browser_bridge::BridgeStep, Box<dyn Error>> {
    let context = raw["context"]
        .as_str()
        .ok_or("bridge step requires 'context'")?;
    let action = raw["action"]
        .as_str()
        .ok_or("bridge step requires 'action'")?;
    let text = |key: &str| -> Result<String, Box<dyn Error>> {
        Ok(raw[key]
            .as_str()
            .ok_or(format!("bridge step '{action}' requires '{key}'"))?
            .to_string())
    };
    let optional_text =
        |key: &str| -> Option<String> { raw[key].as_str().map(|value| value.to_string()) };
    let timeout = |key: &str| -> std::time::Duration {
        std::time::Duration::from_millis(raw[key].as_u64().unwrap_or(5_000))
    };
    let path = |key: &str| -> Result<std::path::PathBuf, Box<dyn Error>> {
        Ok(std::path::PathBuf::from(text(key)?))
    };

    match context {
        "browser" => {
            let browser_action = match action {
                "navigate" => {
                    crate::wa::browser_bridge::BrowserAction::Navigate { url: text("url")? }
                }
                "click" => crate::wa::browser_bridge::BrowserAction::Click {
                    selector: text("selector")?,
                },
                "type" => crate::wa::browser_bridge::BrowserAction::Type {
                    selector: text("selector")?,
                    text: text("text")?,
                },
                "download" => crate::wa::browser_bridge::BrowserAction::Download {
                    url: text("url")?,
                    expected_filename: optional_text("expectedFilename"),
                },
                "trigger_upload" => crate::wa::browser_bridge::BrowserAction::TriggerUpload {
                    input_selector: text("inputSelector")?,
                },
                "eval_js" => crate::wa::browser_bridge::BrowserAction::EvalJs {
                    script: text("script")?,
                },
                "wait_for_element" => crate::wa::browser_bridge::BrowserAction::WaitForElement {
                    selector: text("selector")?,
                    timeout_ms: raw["timeoutMs"].as_u64().unwrap_or(5_000),
                },
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown browser bridge action '{other}'"
                    )))
                }
            };
            Ok(crate::wa::browser_bridge::BridgeStep::Browser(
                browser_action,
            ))
        }
        "desktop" => {
            let desktop_action = match action {
                "open_file" => crate::wa::browser_bridge::DesktopAction::OpenFile {
                    path: path("path")?,
                },
                "focus_window" => crate::wa::browser_bridge::DesktopAction::FocusWindow {
                    title_contains: text("titleContains")?,
                },
                "type_text" => crate::wa::browser_bridge::DesktopAction::TypeText {
                    text: text("text")?,
                },
                "handle_file_dialog" => {
                    crate::wa::browser_bridge::DesktopAction::HandleFileDialog {
                        path: path("path")?,
                    }
                }
                "click_element" => crate::wa::browser_bridge::DesktopAction::ClickElement {
                    name: text("name")?,
                    role: optional_text("role"),
                },
                "copy_to_clipboard" => crate::wa::browser_bridge::DesktopAction::CopyToClipboard,
                "paste_from_clipboard" => {
                    crate::wa::browser_bridge::DesktopAction::PasteFromClipboard
                }
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown desktop bridge action '{other}'"
                    )))
                }
            };
            Ok(crate::wa::browser_bridge::BridgeStep::Desktop(
                desktop_action,
            ))
        }
        "wait" => {
            let condition = match action {
                "file_appears" => crate::wa::browser_bridge::CrossContextCondition::FileAppears {
                    path: path("path")?,
                    timeout: timeout("timeoutMs"),
                },
                "window_appears" => {
                    crate::wa::browser_bridge::CrossContextCondition::WindowAppears {
                        title_contains: text("titleContains")?,
                        timeout: timeout("timeoutMs"),
                    }
                }
                "browser_navigates" => {
                    crate::wa::browser_bridge::CrossContextCondition::BrowserNavigates {
                        url_contains: text("urlContains")?,
                        timeout: timeout("timeoutMs"),
                    }
                }
                "clipboard_contains" => {
                    crate::wa::browser_bridge::CrossContextCondition::ClipboardContains {
                        text: text("text")?,
                        timeout: timeout("timeoutMs"),
                    }
                }
                "process_starts" => {
                    crate::wa::browser_bridge::CrossContextCondition::ProcessStarts {
                        name: text("name")?,
                        timeout: timeout("timeoutMs"),
                    }
                }
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown cross-context wait condition '{other}'"
                    )))
                }
            };
            Ok(crate::wa::browser_bridge::BridgeStep::CrossContextWait(
                condition,
            ))
        }
        "transfer" => {
            let op = match action {
                "browser_to_desktop" => {
                    crate::wa::browser_bridge::DataTransferOp::BrowserToDesktop {
                        browser_selector: text("browserSelector")?,
                        desktop_target: text("desktopTarget")?,
                    }
                }
                "desktop_to_browser" => {
                    crate::wa::browser_bridge::DataTransferOp::DesktopToBrowser {
                        desktop_source: text("desktopSource")?,
                        browser_selector: text("browserSelector")?,
                    }
                }
                "download_and_open" => crate::wa::browser_bridge::DataTransferOp::DownloadAndOpen {
                    download_url: text("downloadUrl")?,
                    app_exe: optional_text("appExe"),
                },
                "read_desktop_text" => crate::wa::browser_bridge::DataTransferOp::ReadDesktopText {
                    element_name: text("elementName")?,
                },
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown data transfer op '{other}'"
                    )))
                }
            };
            Ok(crate::wa::browser_bridge::BridgeStep::DataTransfer(op))
        }
        other => Err(Box::<dyn Error>::from(format!(
            "unknown bridge step context '{other}' (expected browser, desktop, wait, or transfer)"
        ))),
    }
}

/// Process-global recording session, shared across `wa_record_*` calls.
fn recording_session() -> &'static std::sync::Mutex<crate::wa::recording::RecordingSession> {
    static SESSION: std::sync::OnceLock<std::sync::Mutex<crate::wa::recording::RecordingSession>> =
        std::sync::OnceLock::new();
    SESSION.get_or_init(|| {
        std::sync::Mutex::new(crate::wa::recording::RecordingSession::new("default"))
    })
}

/// Lock the global recording session, converting a poisoned mutex into a tool error.
fn lock_recording_session(
) -> Result<std::sync::MutexGuard<'static, crate::wa::recording::RecordingSession>, Box<dyn Error>>
{
    recording_session()
        .lock()
        .map_err(|e| Box::<dyn Error>::from(format!("recording session lock poisoned: {e}")))
}

/// Render a `RecordingState` as a stable string for tool output.
fn recording_state_str(state: crate::wa::recording::RecordingState) -> &'static str {
    match state {
        crate::wa::recording::RecordingState::Idle => "idle",
        crate::wa::recording::RecordingState::Recording => "recording",
        crate::wa::recording::RecordingState::Paused => "paused",
    }
}

/// Parse one recorded event as emitted by the recording hook script.
fn parse_recorded_event(
    raw: &serde_json::Value,
) -> Result<crate::wa::recording::RecordedEvent, Box<dyn Error>> {
    let kind_str = raw["kind"]
        .as_str()
        .ok_or("recorded event requires 'kind'")?;
    let button = |value: &serde_json::Value| match value.as_str().unwrap_or("left") {
        "right" => crate::wa::recording::MouseButton::Right,
        "middle" => crate::wa::recording::MouseButton::Middle,
        _ => crate::wa::recording::MouseButton::Left,
    };
    let kind = match kind_str {
        "click" => crate::wa::recording::RecordedEventKind::Click {
            x: raw["x"].as_i64().unwrap_or(0) as i32,
            y: raw["y"].as_i64().unwrap_or(0) as i32,
            button: button(&raw["button"]),
        },
        "double_click" => crate::wa::recording::RecordedEventKind::DoubleClick {
            x: raw["x"].as_i64().unwrap_or(0) as i32,
            y: raw["y"].as_i64().unwrap_or(0) as i32,
        },
        "type" => crate::wa::recording::RecordedEventKind::Type {
            text: raw["text"].as_str().unwrap_or("").to_string(),
        },
        "key_combo" => crate::wa::recording::RecordedEventKind::KeyCombo {
            keys: raw["keys"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        },
        "focus" => crate::wa::recording::RecordedEventKind::Focus,
        "scroll" => crate::wa::recording::RecordedEventKind::Scroll {
            delta_x: raw["deltaX"].as_i64().unwrap_or(0) as i32,
            delta_y: raw["deltaY"].as_i64().unwrap_or(0) as i32,
        },
        "drag_drop" => crate::wa::recording::RecordedEventKind::DragDrop {
            from: (
                raw["fromX"].as_i64().unwrap_or(0) as i32,
                raw["fromY"].as_i64().unwrap_or(0) as i32,
            ),
            to: (
                raw["toX"].as_i64().unwrap_or(0) as i32,
                raw["toY"].as_i64().unwrap_or(0) as i32,
            ),
        },
        "window_activate" => crate::wa::recording::RecordedEventKind::WindowActivate,
        other => {
            return Err(Box::<dyn Error>::from(format!(
                "unknown recorded event kind '{other}'"
            )))
        }
    };
    let target = raw["target"]
        .as_object()
        .map(|obj| crate::wa::recording::RecordedTarget {
            node_id: obj
                .get("nodeId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            role: obj
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            name: obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            automation_id: obj
                .get("automationId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });
    Ok(crate::wa::recording::RecordedEvent {
        offset: std::time::Duration::from_millis(raw["offsetMs"].as_u64().unwrap_or(0)),
        kind,
        target,
        window_title: raw["windowTitle"].as_str().unwrap_or("").to_string(),
        process_id: raw["processId"].as_u64().map(|v| v as u32),
    })
}

/// Parse a JSON array of modifier names into `KeyModifier` values.
fn parse_key_modifiers(
    raw: Option<&Vec<serde_json::Value>>,
) -> Result<Vec<crate::wa::advanced_input::KeyModifier>, Box<dyn Error>> {
    let Some(items) = raw else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            let name = item
                .as_str()
                .ok_or("each modifier must be a string")?
                .to_lowercase();
            Ok(match name.as_str() {
                "ctrl" | "control" => crate::wa::advanced_input::KeyModifier::Ctrl,
                "shift" => crate::wa::advanced_input::KeyModifier::Shift,
                "alt" => crate::wa::advanced_input::KeyModifier::Alt,
                "win" | "windows" | "super" => crate::wa::advanced_input::KeyModifier::Win,
                "ctrl_shift" => crate::wa::advanced_input::KeyModifier::CtrlShift,
                "ctrl_alt" => crate::wa::advanced_input::KeyModifier::CtrlAlt,
                "alt_shift" => crate::wa::advanced_input::KeyModifier::AltShift,
                "ctrl_shift_alt" => crate::wa::advanced_input::KeyModifier::CtrlShiftAlt,
                other => {
                    return Err(Box::<dyn Error>::from(format!(
                        "unknown key modifier '{other}'"
                    )))
                }
            })
        })
        .collect()
}

/// Parse a `{x, y}` JSON object into an absolute screen point.
fn parse_screen_point(
    value: &serde_json::Value,
) -> Result<crate::wa::advanced_input::ScreenPoint, Box<dyn Error>> {
    let obj = value
        .as_object()
        .ok_or("screen point must be an object with 'x' and 'y'")?;
    Ok(crate::wa::advanced_input::ScreenPoint {
        x: obj
            .get("x")
            .and_then(|v| v.as_i64())
            .ok_or("screen point requires numeric 'x'")? as i32,
        y: obj
            .get("y")
            .and_then(|v| v.as_i64())
            .ok_or("screen point requires numeric 'y'")? as i32,
    })
}

/// Process-global trigger registry.
///
/// `TriggerManager` holds registrations in memory, so each MCP call must share
/// one instance; otherwise `wa_trigger_fire` / `wa_trigger_remove` would always
/// run against an empty manager and could never observe a registered trigger.
fn trigger_registry() -> &'static std::sync::Mutex<crate::wa::triggers::TriggerManager> {
    static REGISTRY: std::sync::OnceLock<std::sync::Mutex<crate::wa::triggers::TriggerManager>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(crate::wa::triggers::TriggerManager::new()))
}

/// Lock the global trigger registry, converting a poisoned mutex into a tool error.
fn lock_trigger_manager(
) -> Result<std::sync::MutexGuard<'static, crate::wa::triggers::TriggerManager>, Box<dyn Error>> {
    trigger_registry()
        .lock()
        .map_err(|e| Box::<dyn Error>::from(format!("trigger registry lock poisoned: {e}")))
}

/// Serialise a cached UIA element into a compact JSON description for tool output.
pub fn uia_element_json(el: &crate::wa::uia_ffi::CachedUiaElement) -> serde_json::Value {
    serde_json::json!({
        "automation_id": el.automation_id,
        "name": el.name,
        "control_type": el.control_type,
        "class_name": el.class_name,
        "enabled": el.is_enabled,
        "offscreen": el.is_offscreen,
        "depth": el.depth,
        "rect": {
            "x": el.bounding_rect.x,
            "y": el.bounding_rect.y,
            "width": el.bounding_rect.width,
            "height": el.bounding_rect.height,
        },
        "patterns": el.supported_patterns.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
    })
}

/// Process-global UIA event buffer shared by `wa_event_subscribe`, `wa_event_poll`,
/// and `wa_event_unsubscribe` so captured events persist across tool calls.
fn global_event_buffer() -> &'static std::sync::Mutex<crate::wa::events::EventBuffer> {
    static BUFFER: std::sync::OnceLock<std::sync::Mutex<crate::wa::events::EventBuffer>> =
        std::sync::OnceLock::new();
    BUFFER.get_or_init(|| {
        std::sync::Mutex::new(crate::wa::events::EventBuffer::new(
            500,
            std::time::Duration::from_millis(100),
        ))
    })
}

/// Lock the global event buffer, converting a poisoned mutex into a tool error.
fn lock_event_buffer(
) -> Result<std::sync::MutexGuard<'static, crate::wa::events::EventBuffer>, Box<dyn Error>> {
    global_event_buffer()
        .lock()
        .map_err(|e| Box::<dyn Error>::from(format!("event buffer lock poisoned: {e}")))
}

/// Cursor (unix ms) marking the last `wa_event_poll` so each event is reported once.
static LAST_EVENT_POLL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Process-global recovery policy + circuit breaker shared by `wa_recovery_set_policy`
/// and `wa_recovery_get_status` so configuration persists across calls.
fn recovery_state() -> &'static std::sync::Mutex<(
    crate::wa::recovery::RetryPolicy,
    crate::wa::recovery::CircuitBreaker,
)> {
    static STATE: std::sync::OnceLock<
        std::sync::Mutex<(
            crate::wa::recovery::RetryPolicy,
            crate::wa::recovery::CircuitBreaker,
        )>,
    > = std::sync::OnceLock::new();
    STATE.get_or_init(|| {
        std::sync::Mutex::new((
            crate::wa::recovery::RetryPolicy::default(),
            crate::wa::recovery::CircuitBreaker::default(),
        ))
    })
}

/// Lock the global recovery state, converting a poisoned mutex into a tool error.
fn lock_recovery_state() -> Result<
    std::sync::MutexGuard<
        'static,
        (
            crate::wa::recovery::RetryPolicy,
            crate::wa::recovery::CircuitBreaker,
        ),
    >,
    Box<dyn Error>,
> {
    recovery_state()
        .lock()
        .map_err(|e| Box::<dyn Error>::from(format!("recovery state lock poisoned: {e}")))
}

/// Current unix wall-clock time in milliseconds (0 on clock error).
fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Serialise a captured UIA event into JSON for `wa_event_poll` output.
fn uia_event_json(event: &crate::wa::events::UiaEvent) -> serde_json::Value {
    serde_json::json!({
        "kind": format!("{:?}", event.kind),
        "timestamp_ms": event.timestamp_ms,
        "source_automation_id": event.source_automation_id,
        "source_name": event.source_name,
        "source_control_type": event.source_control_type,
        "process_id": event.process_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // `wa_window_action` must drive the real native WindowManager rather than echoing a
    // stub. A null HWND cannot be moved, so the operation must report failure cleanly
    // (on and off Windows) while still echoing the hwnd for audit.
    #[test]
    fn window_action_null_hwnd_reports_failure() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "hwnd": 0, "action": "move", "x": 10, "y": 10 });
        let out = handle_wa_tool(temp.path(), "wa_window_action", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(out.contains("\"success\":false"), "got: {out}");
        assert!(out.contains("\"hwnd\":0"), "got: {out}");
    }

    #[test]
    fn window_action_unknown_action_errors() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "hwnd": 1, "action": "explode" });
        let out = handle_wa_tool(temp.path(), "wa_window_action", &args);
        assert!(out.is_err(), "unknown action must be rejected");
    }

    #[test]
    fn window_action_requires_hwnd() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "action": "minimize" });
        assert!(handle_wa_tool(temp.path(), "wa_window_action", &args).is_err());
    }

    // `wa_event_subscribe` used to map every unrecognised/unsupported eventKind
    // onto a focus poll and answer `subscribed:true` (bug #15). It must reject
    // kinds the polled listener cannot observe instead of pretending.
    #[test]
    fn event_subscribe_rejects_uncapturable_kind() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "eventKind": "element_value_changed", "timeoutMs": 100 });
        let err = handle_wa_tool(temp.path(), "wa_event_subscribe", &args)
            .expect_err("uncapturable kind must be rejected");
        assert!(err.to_string().contains("not capturable"), "got: {err}");
    }

    #[test]
    fn event_subscribe_rejects_unknown_kind() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "eventKind": "vibes", "timeoutMs": 100 });
        let err = handle_wa_tool(temp.path(), "wa_event_subscribe", &args)
            .expect_err("unknown kind must be rejected");
        assert!(err.to_string().contains("unknown eventKind"), "got: {err}");
    }

    #[test]
    fn event_subscribe_requires_event_kind() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "timeoutMs": 100 });
        assert!(handle_wa_tool(temp.path(), "wa_event_subscribe", &args).is_err());
    }

    /// Real UIA run: a short focus subscription must report the kind it actually
    /// polled, and any listener failure must surface rather than be swallowed.
    #[test]
    #[cfg(target_os = "windows")]
    fn event_subscribe_reports_what_it_polled() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "eventKind": "element_focus", "timeoutMs": 400 });
        let out = handle_wa_tool(temp.path(), "wa_event_subscribe", &args)
            .expect("listener should report honestly, not error silently")
            .expect("tool should produce output");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["captured_kind"], "focus_changed", "got: {out}");
        assert!(parsed["warnings"].is_array(), "got: {out}");
        assert!(parsed["events_captured"].is_number(), "got: {out}");
    }

    // Bug #37: `wa_window_tile` used to enumerate every titled window on the
    // desktop and tile all of them, and an out-of-range `monitor` silently fell
    // back to the primary display - so a bogus probe call rearranged real
    // windows and still reported success. It must now name its targets and
    // refuse anything it cannot aim. The grid maths itself ("is `columns`
    // honoured?") is asserted side-effect-free in
    // `window_mgmt::tests::tile_rects_*`, because a positive tile case here
    // would move the developer's own windows during `cargo test`.
    #[test]
    fn window_tile_refuses_without_named_handles() {
        let temp = tempfile::tempdir().unwrap();
        for args in [
            serde_json::json!({}),
            serde_json::json!({ "hwnds": [] }),
            serde_json::json!({ "hwnds": "459150" }),
        ] {
            let err = handle_wa_tool(temp.path(), "wa_window_tile", &args)
                .expect_err(&format!("must refuse when no handles are named: {args}"));
            let msg = err.to_string();
            assert!(
                msg.contains("hwnds is required"),
                "unhelpful refusal: {msg}"
            );
            assert!(
                msg.contains("wa_window_list"),
                "refusal should say where to get handles: {msg}"
            );
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn window_tile_rejects_an_out_of_range_monitor_and_lists_valid_ones() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "hwnds": [1], "monitor": 99 });
        let err = handle_wa_tool(temp.path(), "wa_window_tile", &args)
            .expect_err("monitor 99 must not silently retarget the primary display");
        let msg = err.to_string();
        assert!(msg.contains("no monitor at index 99"), "got: {msg}");
        assert!(
            msg.contains("valid indices"),
            "should name the monitors that do exist: {msg}"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn window_tile_refuses_when_no_requested_handle_is_a_live_window() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "hwnds": [1, 2, 3], "monitor": 0 });
        let err = handle_wa_tool(temp.path(), "wa_window_tile", &args)
            .expect_err("dead handles must not be reported as a successful tile");
        assert!(
            err.to_string().contains("none of the 3 requested handle"),
            "got: {err}"
        );
    }

    // The result shape changed with bug #37: it now reports what it was asked
    // for, what it skipped, and the monitor it actually used.
    #[test]
    #[cfg(target_os = "windows")]
    fn window_tile_reports_skips_and_monitor_instead_of_a_bare_success_flag() {
        let temp = tempfile::tempdir().unwrap();
        // Monitor 0 always exists on any machine that gets this far, and handle
        // 1 never does, so this exercises the shape without moving anything.
        let args = serde_json::json!({ "hwnds": [1], "monitor": 0, "columns": 4 });
        match handle_wa_tool(temp.path(), "wa_window_tile", &args) {
            Ok(Some(out)) => {
                let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
                for field in [
                    "success",
                    "windows_tiled",
                    "windows_requested",
                    "windows_skipped",
                    "monitor",
                ] {
                    assert!(!parsed[field].is_null(), "missing {field} in {out}");
                }
            }
            Ok(None) => panic!("tool produced no output"),
            Err(err) => {
                let msg = err.to_string();
                assert!(msg.contains("live windows"), "unexpected error: {msg}");
            }
        }
    }

    // `wa_registry_read` must perform a real read (returning a structured result with a
    // `success` flag) rather than the old `script_ready` stub. A bogus key is absent on
    // any machine, so this is deterministic and side-effect free.
    #[test]
    fn registry_read_executes_for_real() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({
            "hive": "HKCU",
            "path": "SOFTWARE\\NonexistentVelocityTestKey",
            "name": "nope"
        });
        let out = handle_wa_tool(temp.path(), "wa_registry_read", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(!out.contains("script_ready"), "stub still present: {out}");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["operation"], "read");
        assert!(parsed["success"].is_boolean());
    }

    #[test]
    fn registry_read_invalid_hive_errors() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "hive": "BOGUS", "path": "x", "name": "y" });
        assert!(handle_wa_tool(temp.path(), "wa_registry_read", &args).is_err());
    }

    // `wa_notifications_list` must perform a real detection pass (returning a structured
    // list) rather than the old `script_ready` stub. Read-only, so safe on any machine.
    #[test]
    fn notifications_list_executes_for_real() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({});
        let out = handle_wa_tool(temp.path(), "wa_notifications_list", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(!out.contains("script_ready"), "stub still present: {out}");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["success"], serde_json::json!(true));
        assert!(parsed["count"].is_number());
        assert!(parsed["notifications"].is_array());
    }

    // `wa_system_dark_mode` without an `enabled` argument must perform a real read of the
    // current dark mode state (returning a structured result) rather than the old
    // `script_ready` stub. Read-only, so safe on any machine.
    #[test]
    fn system_dark_mode_query_executes_for_real() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({});
        let out = handle_wa_tool(temp.path(), "wa_system_dark_mode", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(!out.contains("script_ready"), "stub still present: {out}");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["success"], serde_json::json!(true));
        assert_eq!(parsed["operation"], "query");
        // dark_mode is either a boolean (Windows read succeeded) or null (unavailable).
        assert!(parsed["dark_mode"].is_boolean() || parsed["dark_mode"].is_null());
    }

    // `wa_ocr_screen` must perform a real OCR pass (returning a structured result) rather
    // than the old `script_ready` stub. A tiny region keeps it fast; the recognized text is
    // environment dependent, so only the structured shape is asserted.
    #[test]
    fn ocr_screen_executes_for_real() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({ "x": 0, "y": 0, "width": 8, "height": 8 });
        let out = handle_wa_tool(temp.path(), "wa_ocr_screen", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(!out.contains("script_ready"), "stub still present: {out}");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["success"], serde_json::json!(true));
        assert!(parsed["block_count"].is_number());
        assert!(parsed["blocks"].is_array());
        assert!(parsed["full_text"].is_string());
    }

    // `wa_notifications_dismiss` must perform a real dismissal pass (returning a structured
    // result) rather than the old `script_ready` stub. With no matching notifications the
    // dismissed count is zero, but the PowerShell pass still runs for real.
    #[test]
    fn notifications_dismiss_executes_for_real() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({});
        let out = handle_wa_tool(temp.path(), "wa_notifications_dismiss", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(!out.contains("script_ready"), "stub still present: {out}");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["action"], "dismiss");
        assert_eq!(parsed["pattern"], "*");
        assert!(parsed["success"].is_boolean());
        assert!(parsed["notifications_remaining"].is_number());
    }
}
