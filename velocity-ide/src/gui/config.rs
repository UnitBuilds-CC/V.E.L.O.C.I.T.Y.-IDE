//! API provider configuration and persistent storage.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Supported API providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Anthropic,
    Cloudflare,
    Custom(String),
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::OpenAI => write!(f, "OpenAI"),
            Provider::Anthropic => write!(f, "Anthropic"),
            Provider::Cloudflare => write!(f, "Cloudflare Workers AI"),
            Provider::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Configuration for a single API provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: Provider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub account_id: Option<String>, // For Cloudflare
}

/// Application configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub providers: Vec<ProviderConfig>,
    pub active_provider: Option<usize>,
    pub theme_dark: bool,
}

impl AppConfig {
    /// Get the config file path.
    pub fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("V.E.L.O.C.I.T.Y").join("config.json")
    }

    /// Load config from disk.
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save config to disk.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
    }

    /// Check if config is complete (has at least one provider).
    pub fn is_configured(&self) -> bool {
        !self.providers.is_empty() && self.active_provider.is_some()
    }

    /// Get the active provider config.
    pub fn active_provider_config(&self) -> Option<&ProviderConfig> {
        self.active_provider.and_then(|i| self.providers.get(i))
    }
}
