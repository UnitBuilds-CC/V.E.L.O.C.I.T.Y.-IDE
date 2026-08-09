//! Plugin SDK: types and traits for building extensible tools.
//!
//! Plugins extend V.E.L.O.C.I.T.Y. with custom tools, commands, and integrations.
//! Each plugin declares its capabilities via a [`PluginManifest`] and implements
//! the [`PluginHandler`] trait to handle tool invocations.
//!
//! The SDK is designed to be simple enough for quick scripts while supporting
//! complex multi-tool plugins with configuration and state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Manifest describing a plugin's identity and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique plugin identifier (e.g., "github-integration").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Short description of what the plugin does.
    pub description: String,
    /// Semantic version string.
    pub version: String,
    /// Author name.
    pub author: String,
    /// Tools provided by this plugin.
    pub tools: Vec<PluginTool>,
    /// Configuration schema (JSON Schema subset).
    pub config_schema: Option<serde_json::Value>,
    /// Required permissions (e.g., "network", "filesystem", "process").
    pub permissions: Vec<PluginPermission>,
}

/// A tool provided by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    /// Tool name (unique within the plugin).
    pub name: String,
    /// Description for the agent to understand when to use this tool.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: serde_json::Value,
    /// Whether this tool requires user approval before execution.
    pub requires_approval: bool,
}

/// Permissions a plugin can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginPermission {
    /// Access to the local filesystem.
    Filesystem,
    /// Network access (HTTP requests).
    Network,
    /// Spawn subprocess.
    Process,
    /// Access to environment variables.
    Environment,
    /// Access to the clipboard.
    Clipboard,
    /// Read/write access to the workspace.
    Workspace,
}

impl PluginPermission {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::Network => "network",
            Self::Process => "process",
            Self::Environment => "environment",
            Self::Clipboard => "clipboard",
            Self::Workspace => "workspace",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "filesystem" => Some(Self::Filesystem),
            "network" => Some(Self::Network),
            "process" => Some(Self::Process),
            "environment" => Some(Self::Environment),
            "clipboard" => Some(Self::Clipboard),
            "workspace" => Some(Self::Workspace),
            _ => None,
        }
    }
}

/// Result of executing a plugin tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResult {
    /// Whether the tool executed successfully.
    pub success: bool,
    /// Output data (JSON).
    pub output: serde_json::Value,
    /// Error message if unsuccessful.
    pub error: Option<String>,
    /// Logs generated during execution.
    pub logs: Vec<String>,
}

impl PluginResult {
    /// Create a successful result.
    pub fn ok(output: serde_json::Value) -> Self {
        Self {
            success: true,
            output,
            error: None,
            logs: Vec::new(),
        }
    }

    /// Create a failed result.
    pub fn err(msg: &str) -> Self {
        Self {
            success: false,
            output: serde_json::json!(null),
            error: Some(msg.to_string()),
            logs: Vec::new(),
        }
    }

    /// Add a log entry.
    pub fn with_log(mut self, msg: &str) -> Self {
        self.logs.push(msg.to_string());
        self
    }
}

/// Trait that plugin handlers must implement.
///
/// This is the core interface for plugin execution. Each plugin provides
/// a handler that processes tool invocations and returns results.
pub trait PluginHandler: Send + Sync {
    /// Initialize the plugin with its configuration.
    fn initialize(&mut self, config: &serde_json::Value) -> Result<(), String>;

    /// Execute a tool by name with the given input.
    fn execute(&self, tool_name: &str, input: &serde_json::Value) -> PluginResult;

    /// Get the plugin's manifest.
    fn manifest(&self) -> &PluginManifest;

    /// Called when the plugin is being unloaded.
    fn shutdown(&mut self) {}
}

/// A simple built-in plugin that wraps a closure.
///
/// Useful for quick tool registration without creating a full plugin module.
pub struct ClosurePlugin {
    pub manifest: PluginManifest,
    handler: Box<dyn Fn(&serde_json::Value) -> PluginResult + Send + Sync>,
}

impl ClosurePlugin {
    /// Create a new closure-based plugin with a single tool.
    pub fn new(
        id: &str,
        name: &str,
        description: &str,
        tool_name: &str,
        tool_description: &str,
        input_schema: serde_json::Value,
        handler: impl Fn(&serde_json::Value) -> PluginResult + Send + Sync + 'static,
    ) -> Self {
        Self {
            manifest: PluginManifest {
                id: id.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                version: "1.0.0".to_string(),
                author: "built-in".to_string(),
                tools: vec![PluginTool {
                    name: tool_name.to_string(),
                    description: tool_description.to_string(),
                    input_schema,
                    requires_approval: false,
                }],
                config_schema: None,
                permissions: vec![],
            },
            handler: Box::new(handler),
        }
    }
}

impl PluginHandler for ClosurePlugin {
    fn initialize(&mut self, _config: &serde_json::Value) -> Result<(), String> {
        Ok(())
    }

    fn execute(&self, tool_name: &str, input: &serde_json::Value) -> PluginResult {
        if self.manifest.tools.iter().any(|t| t.name == tool_name) {
            (self.handler)(input)
        } else {
            PluginResult::err(&format!("Unknown tool: {tool_name}"))
        }
    }

    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

/// Validate a plugin manifest for correctness.
pub fn validate_manifest(manifest: &PluginManifest) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if manifest.id.is_empty() {
        errors.push("Plugin ID is empty".to_string());
    }
    if manifest.name.is_empty() {
        errors.push("Plugin name is empty".to_string());
    }
    if manifest.version.is_empty() {
        errors.push("Plugin version is empty".to_string());
    }
    if manifest.tools.is_empty() {
        errors.push("Plugin has no tools".to_string());
    }

    // Check for duplicate tool names.
    let mut seen_tools = std::collections::HashSet::new();
    for tool in &manifest.tools {
        if !seen_tools.insert(&tool.name) {
            errors.push(format!("Duplicate tool name: {}", tool.name));
        }
        if tool.name.is_empty() {
            errors.push("Tool name is empty".to_string());
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            description: "A test plugin".to_string(),
            version: "1.0.0".to_string(),
            author: "Test".to_string(),
            tools: vec![PluginTool {
                name: "echo".to_string(),
                description: "Echoes input".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                requires_approval: false,
            }],
            config_schema: None,
            permissions: vec![],
        }
    }

    #[test]
    fn validate_valid_manifest() {
        let manifest = test_manifest();
        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn validate_empty_id() {
        let mut manifest = test_manifest();
        manifest.id = String::new();
        let errors = validate_manifest(&manifest).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("ID")));
    }

    #[test]
    fn validate_no_tools() {
        let mut manifest = test_manifest();
        manifest.tools.clear();
        let errors = validate_manifest(&manifest).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("no tools")));
    }

    #[test]
    fn closure_plugin_executes() {
        let plugin = ClosurePlugin::new(
            "echo",
            "Echo",
            "Echoes input",
            "echo",
            "Echoes the message",
            serde_json::json!({"type": "object"}),
            |input| {
                let msg = input
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none");
                PluginResult::ok(serde_json::json!({"echo": msg}))
            },
        );

        let result = plugin.execute("echo", &serde_json::json!({"message": "hello"}));
        assert!(result.success);
        assert_eq!(result.output["echo"], "hello");
    }

    #[test]
    fn closure_plugin_unknown_tool() {
        let plugin = ClosurePlugin::new(
            "test",
            "Test",
            "Test",
            "tool1",
            "Tool 1",
            serde_json::json!({}),
            |_| PluginResult::ok(serde_json::json!(null)),
        );

        let result = plugin.execute("nonexistent", &serde_json::json!({}));
        assert!(!result.success);
    }

    #[test]
    fn plugin_result_builder() {
        let result = PluginResult::ok(serde_json::json!({"data": 42}))
            .with_log("Processing...")
            .with_log("Done!");
        assert!(result.success);
        assert_eq!(result.logs.len(), 2);
    }

    #[test]
    fn permission_round_trip() {
        let perms = vec![
            PluginPermission::Filesystem,
            PluginPermission::Network,
            PluginPermission::Process,
        ];
        for perm in perms {
            let label = perm.label();
            let recovered = PluginPermission::from_str(label).unwrap();
            assert_eq!(perm, recovered);
        }
    }
}
