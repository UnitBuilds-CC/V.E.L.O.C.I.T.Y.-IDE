//! Plugin registry: discovery, loading, and lifecycle management.
//!
//! The registry manages all loaded plugins, handles tool dispatch to the
//! correct plugin, and provides discovery of available tools for the agent.

use super::plugin_sdk::{PluginHandler, PluginManifest, PluginPermission, PluginResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Registry managing all loaded plugins.
pub struct PluginRegistry {
    /// Loaded plugins keyed by their ID.
    plugins: HashMap<String, Box<dyn PluginHandler>>,
    /// Plugin load order (for deterministic iteration).
    load_order: Vec<String>,
    /// Workspace root for plugin discovery.
    workspace_root: PathBuf,
    /// User-granted permissions per plugin.
    granted_permissions: HashMap<String, Vec<PluginPermission>>,
}

/// Serializable metadata about a loaded plugin (for UI display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub tool_count: usize,
    pub tool_names: Vec<String>,
    pub permissions: Vec<String>,
    pub enabled: bool,
}

impl PluginRegistry {
    /// Create a new empty registry.
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            plugins: HashMap::new(),
            load_order: Vec::new(),
            workspace_root: workspace_root.to_path_buf(),
            granted_permissions: HashMap::new(),
        }
    }

    /// Register a plugin handler.
    pub fn register(&mut self, handler: Box<dyn PluginHandler>) -> Result<(), String> {
        let manifest = handler.manifest().clone();

        // Validate manifest.
        if let Err(errors) = super::plugin_sdk::validate_manifest(&manifest) {
            return Err(format!("Invalid manifest: {}", errors.join(", ")));
        }

        // Check for duplicate ID.
        if self.plugins.contains_key(&manifest.id) {
            return Err(format!("Plugin '{}' already registered", manifest.id));
        }

        let id = manifest.id.clone();
        self.load_order.push(id.clone());
        self.plugins.insert(id, handler);
        Ok(())
    }

    /// Unregister a plugin by ID.
    pub fn unregister(&mut self, id: &str) -> bool {
        if let Some(mut handler) = self.plugins.remove(id) {
            handler.shutdown();
            self.load_order.retain(|pid| pid != id);
            self.granted_permissions.remove(id);
            true
        } else {
            false
        }
    }

    /// Get info about a loaded plugin.
    pub fn info(&self, id: &str) -> Option<PluginInfo> {
        let handler = self.plugins.get(id)?;
        let manifest = handler.manifest();
        Some(PluginInfo {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            version: manifest.version.clone(),
            author: manifest.author.clone(),
            tool_count: manifest.tools.len(),
            tool_names: manifest.tools.iter().map(|t| t.name.clone()).collect(),
            permissions: manifest.permissions.iter().map(|p| p.label().to_string()).collect(),
            enabled: true,
        })
    }

    /// List all loaded plugins.
    pub fn list(&self) -> Vec<PluginInfo> {
        self.load_order.iter()
            .filter_map(|id| self.info(id))
            .collect()
    }

    /// Get the number of loaded plugins.
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Execute a tool by fully-qualified name (plugin_id::tool_name).
    pub fn execute_tool(&self, qualified_name: &str, input: &serde_json::Value) -> PluginResult {
        let (plugin_id, tool_name) = match qualified_name.split_once("::") {
            Some((pid, tn)) => (pid, tn),
            None => return PluginResult::err(&format!(
                "Invalid tool name '{qualified_name}'. Expected format: plugin_id::tool_name"
            )),
        };

        let handler = match self.plugins.get(plugin_id) {
            Some(h) => h,
            None => return PluginResult::err(&format!("Plugin '{plugin_id}' not found")),
        };

        // Check permissions.
        let manifest = handler.manifest();
        if !manifest.permissions.is_empty() {
            let granted = self.granted_permissions.get(plugin_id);
            for required in &manifest.permissions {
                let is_granted = granted.map(|g| g.contains(required)).unwrap_or(false);
                if !is_granted {
                    return PluginResult::err(&format!(
                        "Plugin '{}' requires '{}' permission (not granted)",
                        plugin_id,
                        required.label()
                    ));
                }
            }
        }

        handler.execute(tool_name, input)
    }

    /// Grant permissions to a plugin.
    pub fn grant_permissions(&mut self, plugin_id: &str, permissions: Vec<PluginPermission>) {
        self.granted_permissions.insert(plugin_id.to_string(), permissions);
    }

    /// Get all available tools across all plugins (for agent tool discovery).
    pub fn all_tools(&self) -> Vec<AvailableTool> {
        let mut tools = Vec::new();
        for id in &self.load_order {
            if let Some(handler) = self.plugins.get(id) {
                let manifest = handler.manifest();
                for tool in &manifest.tools {
                    tools.push(AvailableTool {
                        qualified_name: format!("{}::{}", manifest.id, tool.name),
                        description: tool.description.clone(),
                        input_schema: tool.input_schema.clone(),
                        plugin_name: manifest.name.clone(),
                        requires_approval: tool.requires_approval,
                    });
                }
            }
        }
        tools
    }

    /// Discover plugins from the workspace `.velocity/plugins/` directory.
    /// Returns manifest paths found (actual loading is done by the caller).
    pub fn discover_plugins(&self) -> Vec<PathBuf> {
        let dir = self.workspace_root.join(".velocity").join("plugins");
        let mut manifests = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_path = path.join("plugin.json");
                    if manifest_path.exists() {
                        manifests.push(manifest_path);
                    }
                }
            }
        }
        manifests
    }

    /// Save granted permissions to disk.
    pub fn save_permissions(&self) -> Result<(), String> {
        let dir = self.workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Cannot create .velocity dir: {e}"))?;

        let perms: HashMap<String, Vec<String>> = self.granted_permissions.iter()
            .map(|(id, perms)| {
                (id.clone(), perms.iter().map(|p| p.label().to_string()).collect())
            })
            .collect();

        let json = serde_json::to_vec_pretty(&perms)
            .map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("plugin_permissions.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load granted permissions from disk.
    pub fn load_permissions(&mut self) {
        let path = self.workspace_root.join(".velocity").join("plugin_permissions.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(perms) = serde_json::from_slice::<HashMap<String, Vec<String>>>(&bytes) {
                for (id, labels) in perms {
                    let permissions: Vec<PluginPermission> = labels.iter()
                        .filter_map(|l| PluginPermission::from_str(l))
                        .collect();
                    self.granted_permissions.insert(id, permissions);
                }
            }
        }
    }

    /// Shutdown all plugins.
    pub fn shutdown_all(&mut self) {
        for id in &self.load_order.clone() {
            if let Some(handler) = self.plugins.get_mut(id) {
                handler.shutdown();
            }
        }
        self.plugins.clear();
        self.load_order.clear();
    }
}

/// A tool available from a plugin (for agent discovery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableTool {
    pub qualified_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub plugin_name: String,
    pub requires_approval: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::plugin_sdk::{ClosurePlugin, PluginResult};

    fn test_plugin(id: &str, tool_name: &str) -> Box<dyn PluginHandler> {
        let id_owned = id.to_string();
        Box::new(ClosurePlugin::new(
            id,
            &format!("{id} Plugin"),
            &format!("Test plugin {id}"),
            tool_name,
            &format!("Test tool {tool_name}"),
            serde_json::json!({"type": "object"}),
            move |input| {
                PluginResult::ok(serde_json::json!({"plugin": id_owned, "input": input}))
            },
        ))
    }

    #[test]
    fn register_and_list() {
        let mut registry = PluginRegistry::new(Path::new("."));
        registry.register(test_plugin("alpha", "tool_a")).unwrap();
        registry.register(test_plugin("beta", "tool_b")).unwrap();
        assert_eq!(registry.count(), 2);
        let list = registry.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "alpha");
    }

    #[test]
    fn register_duplicate_fails() {
        let mut registry = PluginRegistry::new(Path::new("."));
        registry.register(test_plugin("dup", "tool")).unwrap();
        assert!(registry.register(test_plugin("dup", "tool2")).is_err());
    }

    #[test]
    fn unregister_plugin() {
        let mut registry = PluginRegistry::new(Path::new("."));
        registry.register(test_plugin("temp", "tool")).unwrap();
        assert_eq!(registry.count(), 1);
        assert!(registry.unregister("temp"));
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn execute_tool_dispatch() {
        let mut registry = PluginRegistry::new(Path::new("."));
        registry.register(test_plugin("myplugin", "greet")).unwrap();
        let result = registry.execute_tool("myplugin::greet", &serde_json::json!({"name": "world"}));
        assert!(result.success);
        assert_eq!(result.output["plugin"], "myplugin");
    }

    #[test]
    fn execute_unknown_plugin() {
        let registry = PluginRegistry::new(Path::new("."));
        let result = registry.execute_tool("nonexistent::tool", &serde_json::json!({}));
        assert!(!result.success);
    }

    #[test]
    fn execute_invalid_format() {
        let registry = PluginRegistry::new(Path::new("."));
        let result = registry.execute_tool("no_separator", &serde_json::json!({}));
        assert!(!result.success);
    }

    #[test]
    fn all_tools_discovery() {
        let mut registry = PluginRegistry::new(Path::new("."));
        registry.register(test_plugin("p1", "t1")).unwrap();
        registry.register(test_plugin("p2", "t2")).unwrap();
        let tools = registry.all_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].qualified_name, "p1::t1");
        assert_eq!(tools[1].qualified_name, "p2::t2");
    }

    #[test]
    fn permission_check() {
        use crate::editor::plugin_sdk::{PluginManifest, PluginTool, PluginHandler};

        // A simple handler that requires filesystem permission.
        struct SecureHandler {
            manifest: PluginManifest,
        }
        impl PluginHandler for SecureHandler {
            fn initialize(&mut self, _config: &serde_json::Value) -> Result<(), String> { Ok(()) }
            fn execute(&self, _tool_name: &str, _input: &serde_json::Value) -> PluginResult {
                PluginResult::ok(serde_json::json!({"data": "secret"}))
            }
            fn manifest(&self) -> &PluginManifest { &self.manifest }
        }

        let mut registry = PluginRegistry::new(Path::new("."));
        let handler = SecureHandler {
            manifest: PluginManifest {
                id: "secure".to_string(),
                name: "Secure Plugin".to_string(),
                description: "Needs perms".to_string(),
                version: "1.0.0".to_string(),
                author: "test".to_string(),
                tools: vec![PluginTool {
                    name: "read".to_string(),
                    description: "Read files".to_string(),
                    input_schema: serde_json::json!({}),
                    requires_approval: true,
                }],
                config_schema: None,
                permissions: vec![PluginPermission::Filesystem],
            },
        };
        registry.register(Box::new(handler)).unwrap();

        // Without permission granted, execution should fail.
        let result = registry.execute_tool("secure::read", &serde_json::json!({}));
        assert!(!result.success);
        assert!(result.error.unwrap().contains("permission"));

        // Grant permission and try again.
        registry.grant_permissions("secure", vec![PluginPermission::Filesystem]);
        let result = registry.execute_tool("secure::read", &serde_json::json!({}));
        assert!(result.success);
    }

    #[test]
    fn plugin_info() {
        let mut registry = PluginRegistry::new(Path::new("."));
        registry.register(test_plugin("info-test", "mytool")).unwrap();
        let info = registry.info("info-test").unwrap();
        assert_eq!(info.name, "info-test Plugin");
        assert_eq!(info.tool_count, 1);
        assert_eq!(info.tool_names, vec!["mytool"]);
    }
}
