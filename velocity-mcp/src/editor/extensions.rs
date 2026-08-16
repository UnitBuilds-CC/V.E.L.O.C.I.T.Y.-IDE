//! Extension/Plugin system — registry, loading, and sandboxed execution.
//!
//! Extensions are WASM modules or Lua scripts in the .velocity/extensions/ directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Extension manifest (extension.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub entry_point: String,
    pub contributes: ExtensionContributions,
    pub activation_events: Vec<String>,
}

/// What an extension contributes to the IDE.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionContributions {
    #[serde(default)]
    pub commands: Vec<ExtCommand>,
    #[serde(default)]
    pub keybindings: Vec<ExtKeybinding>,
    #[serde(default)]
    pub themes: Vec<ExtTheme>,
    #[serde(default)]
    pub languages: Vec<ExtLanguage>,
    #[serde(default)]
    pub snippets: Vec<ExtSnippetFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtCommand {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtKeybinding {
    pub command: String,
    pub key: String,
    pub when: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtTheme {
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtLanguage {
    pub id: String,
    pub extensions: Vec<String>,
    pub configuration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtSnippetFile {
    pub language: String,
    pub path: String,
}

/// Extension state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionState {
    Installed,
    Active,
    Disabled,
    Error,
}

/// A loaded extension.
#[derive(Debug, Clone)]
pub struct Extension {
    pub manifest: ExtensionManifest,
    pub state: ExtensionState,
    pub path: PathBuf,
    pub error: Option<String>,
}

/// Runtime contributions loaded from an activated extension.
#[derive(Debug, Clone, Default)]
pub struct LoadedContributions {
    /// Snippet JSON files loaded from the extension's snippet paths.
    pub snippets: Vec<(String, Vec<crate::editor::snippets::Snippet>)>,
    /// Theme JSON files loaded from the extension's theme paths.
    pub themes: Vec<(String, String)>,
    /// Language configuration JSON files loaded from the extension.
    pub language_configs: Vec<(String, String)>,
}

/// Extension registry managing all installed extensions.
#[derive(Debug, Default)]
pub struct ExtensionRegistry {
    pub extensions: Vec<Extension>,
    pub commands: HashMap<String, String>,
    /// Loaded contributions keyed by extension ID (only populated after activate).
    pub loaded: HashMap<String, LoadedContributions>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan the extensions directory and load manifests.
    pub fn scan(&mut self, workspace_root: &Path) {
        let ext_dir = workspace_root.join(".velocity").join("extensions");
        if !ext_dir.exists() {
            return;
        }

        let Ok(entries) = std::fs::read_dir(&ext_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.load_extension(&path);
            }
        }
    }

    fn load_extension(&mut self, ext_path: &Path) {
        let manifest_path = ext_path.join("extension.json");
        let Ok(content) = std::fs::read_to_string(&manifest_path) else {
            return;
        };
        let Ok(manifest) = serde_json::from_str::<ExtensionManifest>(&content) else {
            return;
        };

        // Register contributed commands
        for cmd in &manifest.contributes.commands {
            self.commands.insert(cmd.id.clone(), manifest.id.clone());
        }

        self.extensions.push(Extension {
            manifest,
            state: ExtensionState::Installed,
            path: ext_path.to_path_buf(),
            error: None,
        });
    }

    /// Activate an extension by ID.
    ///
    /// Validates the entry point, loads snippet files, theme files, and language
    /// configurations declared in the extension manifest. Returns an error if the
    /// extension is not found or its entry point is missing.
    pub fn activate(&mut self, id: &str) -> Result<(), String> {
        let ext = self
            .extensions
            .iter_mut()
            .find(|e| e.manifest.id == id)
            .ok_or_else(|| format!("Extension '{}' not found", id))?;

        // Validate entry point exists relative to extension path
        let entry = ext.path.join(&ext.manifest.entry_point);
        if !entry.exists() {
            ext.state = ExtensionState::Error;
            ext.error = Some(format!("Entry point not found: {}", entry.display()));
            return Err(ext.error.clone().unwrap());
        }

        // Load contributions
        let ext_path = ext.path.clone();
        let contributions = Self::load_contributions(&ext_path, &ext.manifest.contributes);

        ext.state = ExtensionState::Active;
        ext.error = None;
        self.loaded.insert(id.to_string(), contributions);
        Ok(())
    }

    /// Load contribution files from an extension's directory.
    fn load_contributions(
        ext_path: &Path,
        contributes: &ExtensionContributions,
    ) -> LoadedContributions {
        let mut loaded = LoadedContributions::default();

        // Load snippet files
        for snippet_file in &contributes.snippets {
            let snippet_path = ext_path.join(&snippet_file.path);
            if snippet_path.exists() {
                let collection =
                    crate::editor::snippets::SnippetCollection::load_from_file(&snippet_path);
                loaded
                    .snippets
                    .push((snippet_file.language.clone(), collection.snippets));
            }
        }

        // Load theme files
        for theme in &contributes.themes {
            let theme_path = ext_path.join(&theme.path);
            if theme_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&theme_path) {
                    loaded.themes.push((theme.label.clone(), content));
                }
            }
        }

        // Load language configuration files
        for lang in &contributes.languages {
            if let Some(config_path) = &lang.configuration {
                let full_path = ext_path.join(config_path);
                if full_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&full_path) {
                        loaded.language_configs.push((lang.id.clone(), content));
                    }
                }
            }
        }

        loaded
    }

    /// Disable an extension and unload its contributions.
    pub fn disable(&mut self, id: &str) {
        if let Some(ext) = self.extensions.iter_mut().find(|e| e.manifest.id == id) {
            ext.state = ExtensionState::Disabled;
        }
        self.loaded.remove(id);
    }

    /// Get active extensions count.
    pub fn active_count(&self) -> usize {
        self.extensions
            .iter()
            .filter(|e| e.state == ExtensionState::Active)
            .count()
    }

    /// List all registered commands from extensions.
    pub fn extension_commands(&self) -> Vec<(&str, &str)> {
        self.extensions
            .iter()
            .filter(|e| e.state == ExtensionState::Active)
            .flat_map(|e| {
                e.manifest
                    .contributes
                    .commands
                    .iter()
                    .map(|c| (c.id.as_str(), c.title.as_str()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_empty() {
        let reg = ExtensionRegistry::new();
        assert_eq!(reg.extensions.len(), 0);
        assert_eq!(reg.active_count(), 0);
    }

    #[test]
    fn activate_extension() {
        let mut reg = ExtensionRegistry::new();
        // Create a temp directory with a fake entry point
        let tmp = std::env::temp_dir().join("velocity_ext_test");
        let _ = std::fs::create_dir_all(&tmp);
        let entry = tmp.join("main.wasm");
        let _ = std::fs::write(&entry, b"fake");

        reg.extensions.push(Extension {
            manifest: ExtensionManifest {
                id: "test.ext".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                description: None,
                entry_point: "main.wasm".to_string(),
                contributes: Default::default(),
                activation_events: vec![],
            },
            state: ExtensionState::Installed,
            path: tmp.clone(),
            error: None,
        });
        assert!(reg.activate("test.ext").is_ok());
        assert_eq!(reg.active_count(), 1);
        assert!(reg.loaded.contains_key("test.ext"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn activate_missing_entry_point() {
        let mut reg = ExtensionRegistry::new();
        reg.extensions.push(Extension {
            manifest: ExtensionManifest {
                id: "bad.ext".to_string(),
                name: "Bad".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                description: None,
                entry_point: "nonexistent.wasm".to_string(),
                contributes: Default::default(),
                activation_events: vec![],
            },
            state: ExtensionState::Installed,
            path: PathBuf::from("/tmp/nonexistent_ext"),
            error: None,
        });
        assert!(reg.activate("bad.ext").is_err());
        assert_eq!(reg.extensions[0].state, ExtensionState::Error);
    }
}
