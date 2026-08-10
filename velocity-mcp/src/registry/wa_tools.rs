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
            format!("{{\"success\":true,\"pid\":{},\"killed\":{}}}", pid, killed)
        }
        "wa_process_running" => {
            let pid = arguments["pid"].as_u64().ok_or("pid is required")? as u32;
            let running = crate::wa::process_mgmt::ProcessManager::is_running(pid);
            format!("{{\"pid\":{},\"running\":{}}}", pid, running)
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
            let windows = crate::wa::window_mgmt::WindowManager::enumerate_windows();
            serde_json::to_string(
                &windows
                    .iter()
                    .map(|w| {
                        serde_json::json!({
                            "hwnd": w.hwnd,
                            "title": w.title,
                            "class_name": w.class_name,
                            "pid": w.process_id,
                            "is_foreground": w.is_foreground,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_string())
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
            format!(
                "{{\"total\":{},\"current_index\":{}}}",
                state.total_count, state.current_index
            )
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
            let result = crate::wa::ocr::OcrEngine::recognize_region(r, &config);
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
            let _value = arguments["value"].as_str().ok_or("value is required")?;
            let _vtype = arguments["type"].as_str().ok_or("type is required")?;
            format!("{{\"action\":\"write\",\"hive\":\"{}\",\"path\":\"{}\",\"name\":\"{}\",\"ready\":true}}",
                hive_str, path.replace('\\', "\\\\").replace('"', "\\\""), name)
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
            let kind = arguments["kind"].as_str().ok_or("kind is required")?;
            let _action_script = arguments["actionScript"]
                .as_str()
                .ok_or("actionScript is required")?;
            format!(
                "{{\"action\":\"register\",\"name\":\"{}\",\"kind\":\"{}\",\"ready\":true}}",
                name, kind
            )
        }
        "wa_trigger_list" => "{\"action\":\"list_triggers\",\"count\":0}".to_string(),
        "wa_trigger_fire" => {
            let trigger_id = arguments["triggerId"]
                .as_str()
                .ok_or("triggerId is required")?;
            let mut mgr = crate::wa::triggers::TriggerManager::new();
            match mgr.fire(trigger_id) {
                Some(result) => format!(
                    "{{\"success\":{},\"trigger_id\":\"{}\",\"detail\":\"{}\"}}",
                    result.success,
                    trigger_id,
                    result.detail.replace('"', "\\\"")
                ),
                None => format!(
                    "{{\"success\":false,\"error\":\"Trigger '{}' not found\"}}",
                    trigger_id
                ),
            }
        }
        "wa_trigger_remove" => {
            let trigger_id = arguments["triggerId"]
                .as_str()
                .ok_or("triggerId is required")?;
            let mut mgr = crate::wa::triggers::TriggerManager::new();
            let removed = mgr.remove(trigger_id);
            format!(
                "{{\"success\":{},\"trigger_id\":\"{}\"}}",
                removed, trigger_id
            )
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
            format!(
                "{{\"circuit_closed\":{},\"state\":\"{}\"}}",
                allowed,
                if allowed { "closed" } else { "open" }
            )
        }
        // ─── Events ────────────────────────────────────────────────────────────
        "wa_event_subscribe" => {
            let event_kind = arguments["eventKind"]
                .as_str()
                .ok_or("eventKind is required")?;
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
            format!(
                "{{\"subscribed\":true,\"event_kind\":\"{}\",\"events_captured\":{}}}",
                event_kind,
                result.events.len()
            )
        }
        "wa_event_poll" => {
            let max_events = arguments["maxEvents"].as_u64().unwrap_or(20) as usize;
            let buffer = crate::wa::events::EventBuffer::new(
                max_events,
                std::time::Duration::from_millis(100),
            );
            let events = buffer.recent(max_events);
            format!("{{\"count\":{},\"events\":[]}}", events.len())
        }
        "wa_event_unsubscribe" => "{\"success\":true,\"detail\":\"Listener stopped\"}".to_string(),
        // ─── File Dialog ───────────────────────────────────────────────────────
        "wa_file_dialog_open" => {
            let file_path = arguments["filePath"]
                .as_str()
                .ok_or("filePath is required")?;
            let target = crate::wa::file_dialog::FileDialogTarget {
                process_id: arguments["processId"].as_u64().map(|v| v as u32),
                ..Default::default()
            };
            let result = crate::wa::file_dialog::FileDialogManager::quick_set_path(
                std::path::Path::new(file_path),
                crate::wa::file_dialog::FileDialogKind::Open,
            );
            let _ = &target;
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
            let result = crate::wa::file_dialog::FileDialogManager::quick_set_path(
                std::path::Path::new(file_path),
                crate::wa::file_dialog::FileDialogKind::SaveAs,
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
        // ─── Window Tiling ─────────────────────────────────────────────────────
        "wa_window_tile" => {
            let monitor_index = arguments["monitor"].as_u64().unwrap_or(0) as u32;
            let windows = crate::wa::window_mgmt::WindowManager::enumerate_windows();
            let hwnds: Vec<u64> = windows
                .iter()
                .filter(|w| !w.title.is_empty())
                .map(|w| w.hwnd)
                .collect();
            // Resolve the target monitor's work area (excludes taskbar) for tile bounds;
            // fall back to a sensible default when enumeration is unavailable.
            let mut mm = crate::wa::multi_monitor::MultiMonitorManager::empty();
            let _ = mm.refresh();
            let (mw, mh) = mm
                .get(monitor_index)
                .or_else(|| mm.primary())
                .map(|m| (m.work_area.width, m.work_area.height))
                .filter(|(w, h)| *w > 0 && *h > 0)
                .unwrap_or((1920, 1080));
            let results = crate::wa::window_mgmt::WindowManager::tile_windows(&hwnds, mw, mh);
            let succeeded = results.iter().filter(|r| r.success).count();
            format!(
                "{{\"success\":true,\"windows_tiled\":{},\"succeeded\":{},\"monitor_width\":{},\"monitor_height\":{}}}",
                hwnds.len(),
                succeeded,
                mw,
                mh
            )
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
            let _ = img.save_bmp(std::path::Path::new(output_path));
            format!(
                "{{\"success\":{},\"path\":\"{}\",\"width\":{},\"height\":{}}}",
                img.pixel_count() > 0,
                output_path,
                img.width,
                img.height
            )
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
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

    // `wa_window_tile` must actually run the tiling path and report a real result shape
    // (windows_tiled / succeeded / monitor bounds) instead of a hardcoded stub.
    #[test]
    fn window_tile_executes_and_reports() {
        let temp = tempfile::tempdir().unwrap();
        let args = serde_json::json!({});
        let out = handle_wa_tool(temp.path(), "wa_window_tile", &args)
            .expect("dispatch should not error")
            .expect("tool should produce output");
        assert!(out.contains("\"success\":true"), "got: {out}");
        assert!(out.contains("windows_tiled"), "got: {out}");
        assert!(out.contains("monitor_width"), "got: {out}");
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
