pub mod browser_tools;
pub mod dispatch;
pub mod parsers;
pub mod system_tools;
pub mod team_tools;
pub mod tool_definitions;
pub mod types;
pub mod wa_tools;

pub use dispatch::call_tool_in_workspace;
pub use tool_definitions::get_tools;

use serde_json::Value;
use std::error::Error;

pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let root = std::env::current_dir()?;
    call_tool_in_workspace(&root, name, arguments)
}

#[cfg(test)]
mod tests;
