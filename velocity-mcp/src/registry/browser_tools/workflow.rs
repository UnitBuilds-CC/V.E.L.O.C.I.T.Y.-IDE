use crate::registry::parsers::parse_browser_steps;
use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn handle_workflow_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    let result = match name {
        "browser_save_workflow" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let start_url = arguments["startUrl"]
                .as_str()
                .ok_or("startUrl is required")?;
            let steps = arguments["steps"]
                .as_array()
                .ok_or("steps must be an array")?;
            let mut variables = std::collections::HashMap::new();
            if let Some(map) = arguments["variables"].as_object() {
                for (key, value) in map {
                    let text = value
                        .as_str()
                        .ok_or("workflow variables must be string values")?;
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
            let report = crate::editor::browser::save_workflow_report(root, &workflow)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise browser workflow save summary: {err}")))?
            } else {
                crate::editor::browser::render_workflow_save_report(&report)
            }
        }
        "browser_read_workflow" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = crate::registry::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let workflow = crate::editor::browser::load_workflow(&full_path)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_workflow_report(&full_path)
                    .map_err(|e| Box::<dyn Error>::from(e))?;
                serde_json::to_string_pretty(&report)?
            } else {
                serde_json::to_string_pretty(&workflow)?
            }
        }
        "browser_list_workflows" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let workflows = crate::editor::browser::list_workflows(
                root,
                arguments["workflowNameContains"].as_str(),
                arguments["startUrlContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            serde_json::to_string_pretty(&workflows)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise workflows: {err}")))?
        }
        "browser_replay_workflow" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = crate::registry::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let workflow = crate::editor::browser::load_workflow(&full_path)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            let sitemap_path = root.join(".velocity").join("site_map");
            let compact = arguments["compact"].as_bool().unwrap_or(false);
            if let Some(session_id) = arguments["sessionId"].as_str() {
                if compact {
                    let report = crate::editor::browser::replay_workflow_in_session_report(
                        root,
                        session_id,
                        &workflow,
                        &sitemap_path,
                    )
                    .map_err(|e| Box::<dyn Error>::from(e))?;
                    serde_json::to_string_pretty(&report).map_err(|err| {
                        Box::<dyn Error>::from(format!("serialise browser workflow replay summary: {err}"))
                    })?
                } else {
                    crate::editor::browser::replay_workflow_in_session(
                        root,
                        session_id,
                        &workflow,
                        &sitemap_path,
                    )
                    .map_err(|e| Box::<dyn Error>::from(e))?
                }
            } else if compact {
                let report = crate::editor::browser::replay_workflow_with_artifacts_report(
                    root,
                    &workflow,
                    &sitemap_path,
                )
                .map_err(|e| Box::<dyn Error>::from(e))?;
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser workflow replay summary: {err}"))
                })?
            } else {
                crate::editor::browser::replay_workflow_with_artifacts(
                    root,
                    &workflow,
                    &sitemap_path,
                )
                .map_err(|e| Box::<dyn Error>::from(e))?
            }
        }
        "browser_list_workflow_runs" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let runs = crate::editor::browser::list_workflow_runs(
                root,
                arguments["workflowNameContains"].as_str(),
                arguments["sessionIdContains"].as_str(),
                arguments["finalUrlContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            serde_json::to_string_pretty(&runs)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow runs: {err}")))?
        }
        "browser_read_workflow_run" => {
            let workflow_name = arguments["workflowName"]
                .as_str()
                .ok_or("workflowName is required")?;
            let session_id = arguments["sessionId"]
                .as_str()
                .ok_or("sessionId is required")?;
            let report =
                crate::editor::browser::read_workflow_run(root, workflow_name, session_id)
                    .map_err(|e| Box::<dyn Error>::from(e))?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact = crate::editor::browser::read_workflow_run_report(
                    root,
                    workflow_name,
                    session_id,
                )
                .map_err(|e| Box::<dyn Error>::from(e))?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow run summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow run: {err}")))?
            }
        }
        "browser_save_workflow_suite" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let workflows = arguments["workflows"]
                .as_array()
                .ok_or("workflows must be an array")?;
            let suite = crate::editor::browser::BrowserWorkflowSuite {
                name: name.to_string(),
                workflows: workflows
                    .iter()
                    .map(|entry| {
                        entry
                            .as_str()
                            .ok_or("workflow suite entries must be strings")
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|value| value.to_string())
                    .collect(),
            };
            let report = crate::editor::browser::save_workflow_suite_report(root, &suite)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser workflow suite save summary: {err}"))
                })?
            } else {
                crate::editor::browser::render_workflow_suite_save_report(
                    &report,
                )
            }
        }
        "browser_read_workflow_suite" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = crate::registry::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let suite = crate::editor::browser::load_workflow_suite(&full_path)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report = crate::editor::browser::read_workflow_suite_report(&full_path)
                    .map_err(|e| Box::<dyn Error>::from(e))?;
                serde_json::to_string_pretty(&report)?
            } else {
                serde_json::to_string_pretty(&suite)?
            }
        }
        "browser_list_workflow_suites" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let suites = crate::editor::browser::list_workflow_suites(
                root,
                arguments["suiteNameContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            serde_json::to_string_pretty(&suites)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow suites: {err}")))?
        }
        "browser_run_workflow_suite" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = crate::registry::system_tools::resolve_workspace_path(root, rel_path, false)?;
            let suite = crate::editor::browser::load_workflow_suite(&full_path)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            let sitemap_path = root.join(".velocity").join("site_map");
            if arguments["compact"].as_bool().unwrap_or(false) {
                let report =
                    crate::editor::browser::run_workflow_suite_report(root, &suite, &sitemap_path)
                        .map_err(|e| Box::<dyn Error>::from(e))?;
                serde_json::to_string_pretty(&report).map_err(|err| {
                    Box::<dyn Error>::from(format!("serialise browser workflow suite execution summary: {err}"))
                })?
            } else {
                crate::editor::browser::run_workflow_suite(root, &suite, &sitemap_path)
                    .map_err(|e| Box::<dyn Error>::from(e))?
            }
        }
        "browser_list_workflow_suite_runs" => {
            let sort_direction = crate::editor::browser::parse_list_sort_direction(
                arguments["sortDirection"].as_str(),
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            let limit = arguments["limit"].as_u64().map(|value| value as usize);
            let runs = crate::editor::browser::list_workflow_suite_runs(
                root,
                arguments["suiteNameContains"].as_str(),
                limit,
                sort_direction,
            )
            .map_err(|e| Box::<dyn Error>::from(e))?;
            serde_json::to_string_pretty(&runs)
                .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow suite runs: {err}")))?
        }
        "browser_read_workflow_suite_run" => {
            let suite_name = arguments["suiteName"]
                .as_str()
                .ok_or("suiteName is required")?;
            let report = crate::editor::browser::read_workflow_suite_run(root, suite_name)
                .map_err(|e| Box::<dyn Error>::from(e))?;
            if arguments["compact"].as_bool().unwrap_or(false) {
                let compact =
                    crate::editor::browser::read_workflow_suite_run_report(root, suite_name)
                        .map_err(|e| Box::<dyn Error>::from(e))?;
                serde_json::to_string_pretty(&compact)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow suite run summary: {err}")))?
            } else {
                serde_json::to_string_pretty(&report)
                    .map_err(|err| Box::<dyn Error>::from(format!("serialise workflow suite run: {err}")))?
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(result))
}
