pub mod browser_tools;
pub mod company_tools;
pub mod custom_tools;
pub mod dispatch;
pub mod event_store;
pub mod generation_tools;
pub mod hooks;
pub mod lazy;
pub mod parsers;
pub mod rules_tools;
pub mod skills;
pub mod system_tools;
pub mod team_tools;
pub mod tool_definitions;
pub mod tool_groups;
pub mod types;
pub mod wa_tools;

pub use dispatch::call_tool_in_workspace;
pub use lazy::{LazyToolRegistry, LruToolCache, ToolMetrics, ToolTimer, DEFAULT_CACHE_CAPACITY};
pub use tool_definitions::get_tools;
pub use tool_groups::{
    request_tool_groups_prose, request_tool_groups_schema, ToolGroup, ToolSurface,
    REQUEST_TOOL_GROUPS_TOOL,
};

use serde_json::Value;
use std::error::Error;

pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let root = std::env::current_dir()?;
    call_tool_in_workspace(&root, name, arguments)
}

#[cfg(test)]
mod tests;
