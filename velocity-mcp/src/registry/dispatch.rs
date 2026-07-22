use serde_json::Value;
use std::error::Error;
use std::path::Path;
use super::browser_tools::handle_browser_tool;
use super::system_tools::handle_system_tool;
use super::wa_tools::handle_wa_tool;

pub fn call_tool_in_workspace(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let root = root.canonicalize()?;

    if let Some(result) = handle_system_tool(&root, name, arguments)? {
        return Ok(result);
    }

    if let Some(result) = handle_browser_tool(&root, name, arguments)? {
        return Ok(result);
    }

    if let Some(result) = handle_wa_tool(&root, name, arguments)? {
        return Ok(result);
    }

    Err(format!("Unknown tool: {}", name).into())
}
