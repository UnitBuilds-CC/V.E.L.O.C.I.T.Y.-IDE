pub mod native;
pub mod navigation;
pub mod session;
pub mod workflow;

use serde_json::Value;
use std::error::Error;
use std::path::Path;

pub fn handle_browser_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    if let Some(res) = navigation::handle_navigation_tool(root, name, arguments)? {
        return Ok(Some(res));
    }
    if let Some(res) = native::handle_native_tool(root, name, arguments)? {
        return Ok(Some(res));
    }
    if let Some(res) = session::handle_session_tool(root, name, arguments)? {
        return Ok(Some(res));
    }
    if let Some(res) = workflow::handle_workflow_tool(root, name, arguments)? {
        return Ok(Some(res));
    }
    Ok(None)
}
