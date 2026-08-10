use super::browser_tools::handle_browser_tool;
use super::system_tools::handle_system_tool;
use super::team_tools::handle_team_tool;
use super::wa_tools::handle_wa_tool;
use crate::errors::ToolError;
use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn call_tool_in_workspace(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let root = root.canonicalize().map_err(|e| {
        log::error!(
            "tool dispatch: failed to canonicalize workspace root: {}",
            e
        );
        e
    })?;

    // Governance gate: deny or park-for-approval per the workspace policy. With
    // no policy configured this allows everything (no behavior change).
    crate::editor::governance::gate_tool_call(&root, name, arguments).map_err(|e| {
        log::warn!("tool dispatch: governance denied '{}': {}", name, e);
        e
    })?;

    if let Some(result) = handle_system_tool(&root, name, arguments)? {
        return Ok(result);
    }

    if let Some(result) = handle_team_tool(&root, name, arguments)? {
        return Ok(result);
    }

    if let Some(result) = handle_browser_tool(&root, name, arguments)? {
        return Ok(result);
    }

    if let Some(result) = handle_wa_tool(&root, name, arguments)? {
        return Ok(result);
    }

    log::warn!("tool dispatch: unknown tool '{}'", name);
    Err(format!("Unknown tool: {}", name).into())
}
