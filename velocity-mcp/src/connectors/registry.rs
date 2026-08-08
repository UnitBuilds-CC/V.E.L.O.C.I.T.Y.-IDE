//! Persisted registry of configured connectors.
//!
//! Stored as a single JSON document at `.velocity/connectors.json`. Secrets are
//! never stored here — only the *handle* (`auth_secret`) into the encrypted
//! secret store is persisted.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::types::ConnectorConfig;

fn store_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".velocity").join("connectors.json")
}

/// The full set of configured connectors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectorRegistry {
    #[serde(default)]
    pub connectors: Vec<ConnectorConfig>,
}

// Management methods consumed by the Governance/Integrations panel;
// `load`/`get` back the connector_call tool.
impl ConnectorRegistry {
    /// Load the registry from `.velocity/connectors.json`, or an empty registry
    /// if the file is missing or unreadable.
    pub fn load(workspace_root: &Path) -> Self {
        let path = store_path(workspace_root);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Persist the registry to `.velocity/connectors.json`.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(store_path(workspace_root), text).map_err(|e| e.to_string())
    }

    /// Insert a connector, replacing any existing one with the same id.
    pub fn add(&mut self, config: ConnectorConfig) {
        if let Some(existing) = self.connectors.iter_mut().find(|c| c.id == config.id) {
            *existing = config;
        } else {
            self.connectors.push(config);
        }
    }

    /// Remove a connector by id; returns whether one was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.connectors.len();
        self.connectors.retain(|c| c.id != id);
        self.connectors.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&ConnectorConfig> {
        self.connectors.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ConnectorConfig> {
        self.connectors.iter_mut().find(|c| c.id == id)
    }

    pub fn len(&self) -> usize {
        self.connectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connectors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_replaces_by_id() {
        let mut reg = ConnectorRegistry::default();
        reg.add(ConnectorConfig::generic("a", "First", "https://one.example"));
        reg.add(ConnectorConfig::generic("a", "Second", "https://two.example"));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("a").unwrap().name, "Second");
    }

    #[test]
    fn remove_reports_hit() {
        let mut reg = ConnectorRegistry::default();
        reg.add(ConnectorConfig::generic("a", "A", "https://a.example"));
        assert!(reg.remove("a"));
        assert!(!reg.remove("a"));
        assert!(reg.is_empty());
    }

    #[test]
    fn round_trip_persists_config() {
        let dir = std::env::temp_dir().join(format!("vel-conn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut reg = ConnectorRegistry::default();
        reg.add(ConnectorConfig::github(
            "gh",
            "GitHub",
            Some("gh_token".to_string()),
        ));
        reg.save(&dir).unwrap();

        let loaded = ConnectorRegistry::load(&dir);
        assert_eq!(loaded.len(), 1);
        let cfg = loaded.get("gh").unwrap();
        assert_eq!(cfg.base_url, "https://api.github.com");
        assert_eq!(cfg.auth_secret.as_deref(), Some("gh_token"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
