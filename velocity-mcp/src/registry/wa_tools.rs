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
        _ => return Ok(None),
    };

    Ok(Some(result))
}
