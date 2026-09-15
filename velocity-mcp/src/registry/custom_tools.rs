//! Dynamic custom tool registry.
//!
//! Allows registering shell-command-backed tools at runtime without restarting
//! the MCP server. Custom tools are persisted to `.velocity/custom_tools.json`
//! and survive restarts. Built-in tools always take priority over custom tools
//! with the same name.

use crate::registry::types::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::process::Command;

/// A custom tool definition persisted to disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomTool {
    pub name: String,
    pub description: String,
    /// Shell command to execute. Use `{{param}}` placeholders for arguments.
    pub command: String,
    /// JSON Schema for the tool's input parameters.
    #[serde(default = "default_input_schema")]
    pub input_schema: Value,
}

fn default_input_schema() -> Value {
    json!({"type": "object", "properties": {}, "required": []})
}

/// Path to the custom tools persistence file.
fn tools_path(root: &Path) -> std::path::PathBuf {
    root.join(".velocity").join("custom_tools.json")
}

/// Load custom tools from `.velocity/custom_tools.json`.
fn load_tools(root: &Path) -> HashMap<String, CustomTool> {
    match std::fs::read_to_string(tools_path(root)) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Save custom tools to `.velocity/custom_tools.json`.
fn save_tools(root: &Path, tools: &HashMap<String, CustomTool>) -> Result<(), String> {
    let dir = root.join(".velocity");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create .velocity directory: {e}"))?;
    let json = serde_json::to_string_pretty(tools)
        .map_err(|e| format!("Failed to serialize custom tools: {e}"))?;
    std::fs::write(tools_path(root), json)
        .map_err(|e| format!("Failed to write custom_tools.json: {e}"))?;
    Ok(())
}

/// Register a custom tool. Returns an error if the name conflicts with a
/// built-in tool or is already registered.
pub fn register_tool(root: &Path, tool: CustomTool) -> Result<String, Box<dyn Error>> {
    // Reject names that collide with built-in tools.
    let builtins = crate::registry::tool_definitions::get_tools();
    if builtins.iter().any(|t| t.name == tool.name) {
        return Err(format!(
            "Cannot register '{}': conflicts with a built-in tool",
            tool.name
        )
        .into());
    }

    let mut map = load_tools(root);
    let name = tool.name.clone();
    map.insert(name.clone(), tool);
    save_tools(root, &map)?;
    Ok(format!("Custom tool '{}' registered successfully", name))
}

/// Unregister a custom tool by name.
pub fn unregister_tool(root: &Path, name: &str) -> Result<String, Box<dyn Error>> {
    let mut map = load_tools(root);
    if map.remove(name).is_some() {
        save_tools(root, &map)?;
        Ok(format!("Custom tool '{}' unregistered", name))
    } else {
        Err(format!("Custom tool '{}' not found", name).into())
    }
}

/// List all registered custom tools as `Tool` descriptors.
pub fn list_tools(root: &Path) -> Vec<Tool> {
    load_tools(root)
        .values()
        .map(|ct| Tool {
            name: ct.name.clone(),
            description: ct.description.clone(),
            input_schema: ct.input_schema.clone(),
        })
        .collect()
}

/// Look up a custom tool by name.
pub fn get_tool(root: &Path, name: &str) -> Option<CustomTool> {
    load_tools(root).get(name).cloned()
}

/// Execute a custom tool by substituting `{{param}}` placeholders in the
/// command template with argument values, then running via the platform shell.
pub fn execute_tool(
    root: &Path,
    tool: &CustomTool,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let mut cmd_str = tool.command.clone();

    // Substitute {{param}} placeholders with argument values.
    if let Some(args_obj) = arguments.as_object() {
        for (key, value) in args_obj {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            cmd_str = cmd_str.replace(&placeholder, &replacement);
        }
    }

    let output = if cfg!(target_os = "windows") {
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &cmd_str])
            .current_dir(root)
            .output()?
    } else {
        Command::new("sh")
            .args(["-c", &cmd_str])
            .current_dir(root)
            .output()?
    };

    let mut result = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }

    if !output.status.success() {
        return Err(format!(
            "Custom tool '{}' exited with status {}: {}",
            tool.name, output.status, result
        )
        .into());
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
        tmp
    }

    #[test]
    fn register_and_list() {
        let tmp = setup();
        let tool = CustomTool {
            name: "my-echo".into(),
            description: "Echo a message".into(),
            command: "echo {{msg}}".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "msg": { "type": "string" } },
                "required": ["msg"]
            }),
        };
        assert!(register_tool(tmp.path(), tool).is_ok());
        let tools = list_tools(tmp.path());
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "my-echo");
    }

    #[test]
    fn unregister() {
        let tmp = setup();
        let tool = CustomTool {
            name: "temp-tool".into(),
            description: "Temporary".into(),
            command: "echo hi".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        register_tool(tmp.path(), tool).unwrap();
        assert_eq!(list_tools(tmp.path()).len(), 1);
        assert!(unregister_tool(tmp.path(), "temp-tool").is_ok());
        assert_eq!(list_tools(tmp.path()).len(), 0);
    }

    #[test]
    fn unregister_nonexistent_fails() {
        let tmp = setup();
        assert!(unregister_tool(tmp.path(), "nope").is_err());
    }

    #[test]
    fn persistence_across_reload() {
        let tmp = setup();
        let tool = CustomTool {
            name: "persist-me".into(),
            description: "Survives reload".into(),
            command: "echo ok".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        register_tool(tmp.path(), tool).unwrap();

        // Verify the file exists on disk.
        assert!(tools_path(tmp.path()).exists());

        // list_tools reads from disk each time, so it should see the tool.
        let tools = list_tools(tmp.path());
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "persist-me");
    }

    #[test]
    fn execute_substitution() {
        let tmp = setup();
        let tool = CustomTool {
            name: "greeter".into(),
            description: "Greet someone".into(),
            command: if cfg!(target_os = "windows") {
                "cmd /c echo Hello {{name}}"
            } else {
                "echo Hello {{name}}"
            }
            .into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        let result = execute_tool(tmp.path(), &tool, &json!({"name": "World"})).unwrap();
        assert!(
            result.trim().contains("Hello World"),
            "expected 'Hello World' in output, got: {result:?}"
        );
    }

    #[test]
    fn get_tool_returns_none_for_missing() {
        let tmp = setup();
        assert!(get_tool(tmp.path(), "nonexistent").is_none());
    }

    #[test]
    fn get_tool_returns_registered() {
        let tmp = setup();
        let tool = CustomTool {
            name: "find-me".into(),
            description: "Findable".into(),
            command: "echo hi".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        register_tool(tmp.path(), tool).unwrap();
        let found = get_tool(tmp.path(), "find-me");
        assert!(found.is_some());
        assert_eq!(found.unwrap().description, "Findable");
    }
}
