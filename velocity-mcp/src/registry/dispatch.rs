use super::browser_tools::handle_browser_tool;
use super::custom_tools;
use super::system_tools::handle_system_tool;
use super::team_tools::handle_team_tool;
use super::wa_tools::handle_wa_tool;
use crate::agent::drone_bridge::handle_drone_tool;
use crate::errors::ToolError;
use crate::security::audit::{self, ToolAuditOutcome};
use serde_json::Value;
use std::error::Error;
use std::path::Path;
use std::time::Instant;

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

    let start = Instant::now();

    // Try built-in tool handlers first (in priority order).
    let builtin = try_builtin_tools(&root, name, arguments);

    match &builtin {
        Ok(Some(output)) => {
            // Built-in tool succeeded.
            audit::record_tool_call("stdio", name, start, ToolAuditOutcome::Success);
            Ok(output.clone())
        }
        Ok(None) => {
            // No built-in handler — try custom tools.
            let custom_result = try_custom_tool(&root, name, arguments);
            let outcome = match &custom_result {
                Ok(_) => ToolAuditOutcome::Success,
                Err(e) => ToolAuditOutcome::Error(e.to_string()),
            };
            audit::record_tool_call("stdio", name, start, outcome);
            custom_result
        }
        Err(e) => {
            audit::record_tool_call("stdio", name, start, ToolAuditOutcome::Error(e.to_string()));
            Err(e.to_string().into())
        }
    }
}

/// Try all built-in tool handlers. Returns `Ok(None)` if no handler recognizes
/// the tool name.
fn try_builtin_tools(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    if let Some(result) = handle_system_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_team_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_browser_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_wa_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = handle_drone_tool(root, name, arguments)? {
        return Ok(Some(result));
    }
    Ok(None)
}

/// Try to execute a dynamically registered custom tool.
fn try_custom_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    if let Some(tool) = custom_tools::get_tool(root, name) {
        custom_tools::execute_tool(root, &tool, arguments)
    } else {
        log::warn!("tool dispatch: unknown tool '{}'", name);
        // Return the structured variant rather than an opaque string: callers
        // (the JSON-RPC layer, the parity tests, plugin bridges) branch on the
        // failure mode instead of matching message text.
        Err(Box::new(ToolError::ToolNotFound(name.to_string())))
    }
}
