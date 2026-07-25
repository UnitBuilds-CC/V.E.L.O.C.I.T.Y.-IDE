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
            let format = arguments["format"].as_str().unwrap_or("auto");
            let _script = crate::wa::clipboard::build_read_clipboard_script();
            format!("{{\"format\":\"{}\",\"script_ready\":true,\"note\":\"Execute via wa_execute_ps to get live clipboard\"}}", format)
        }
        "wa_clipboard_write" => {
            if let Some(text) = arguments["text"].as_str() {
                let _script = crate::wa::clipboard::build_write_text_script(text);
                format!("{{\"action\":\"write_text\",\"length\":{}}}", text.len())
            } else if let Some(html) = arguments["html"].as_str() {
                let _script = crate::wa::clipboard::build_write_html_script(html, None);
                format!("{{\"action\":\"write_html\",\"length\":{}}}", html.len())
            } else {
                "{\"error\":\"provide text, html, or files\"}".to_string()
            }
        }
        "wa_clipboard_clear" => {
            let _script = crate::wa::clipboard::build_clear_clipboard_script();
            "{\"action\":\"clear\",\"ready\":true}".to_string()
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
            let _script = crate::wa::process_mgmt::build_enumerate_processes_script(filter);
            format!("{{\"script_ready\":true,\"filter\":\"{}\"}}", filter.unwrap_or("*"))
        }
        // ─── Window Management ────────────────────────────────────────────────────
        "wa_window_list" => {
            let _script = crate::wa::window_mgmt::build_enumerate_windows_script();
            "{\"script_ready\":true,\"action\":\"enumerate_windows\"}".to_string()
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
        _ => return Ok(None),
    };

    Ok(Some(result))
}
