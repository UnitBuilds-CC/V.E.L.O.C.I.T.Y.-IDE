//! Connector configuration types.
//!
//! A [`ConnectorConfig`] describes how to reach an external HTTP service: a base
//! URL, an optional authentication scheme resolved from the encrypted secret
//! store (by *handle*, never a plaintext value), and static headers. Concrete
//! service presets ([`ConnectorKind::GitHub`], [`ConnectorKind::Slack`]) are
//! thin defaults over [`ConnectorKind::GenericRest`].

use serde::{Deserialize, Serialize};

/// The category of connector, mostly a UI/preset hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorKind {
    GenericRest,
    Webhook,
    GitHub,
    Slack,
}

/// How a resolved secret value is injected into an outbound request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthScheme {
    /// No authentication.
    #[default]
    None,
    /// `Authorization: Bearer <secret>`.
    Bearer,
    /// A custom header carrying the raw secret, e.g. `x-api-key: <secret>`.
    Header { name: String },
    /// A query parameter carrying the raw secret, e.g. `?token=<secret>`.
    Query { param: String },
}

/// A configured connection to an external HTTP service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectorConfig {
    pub id: String,
    pub name: String,
    pub kind: ConnectorKind,
    /// Base URL that request paths are joined onto (no trailing slash required).
    pub base_url: String,
    /// Handle into the secret store whose value satisfies `auth`. `None` means
    /// no credential is attached.
    pub auth_secret: Option<String>,
    /// How the resolved secret is injected.
    #[serde(default)]
    pub auth: AuthScheme,
    /// Static headers always sent with requests through this connector.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

// Preset constructors (generic/github/slack) are used by the Governance/Integrations panel;
// `requires_secret` backs build_request.
impl ConnectorConfig {
    /// A minimal generic REST connector with no auth.
    pub fn generic(id: impl Into<String>, name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::GenericRest,
            base_url: base_url.into(),
            auth_secret: None,
            auth: AuthScheme::None,
            headers: Vec::new(),
        }
    }

    /// GitHub REST preset: `https://api.github.com`, bearer auth, JSON accept
    /// header. `auth_secret` should name a stored token handle.
    pub fn github(id: impl Into<String>, name: impl Into<String>, auth_secret: Option<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::GitHub,
            base_url: "https://api.github.com".to_string(),
            auth_secret,
            auth: AuthScheme::Bearer,
            headers: vec![
                ("Accept".to_string(), "application/vnd.github+json".to_string()),
                ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
            ],
        }
    }

    /// Slack Web API preset: `https://slack.com/api`, bearer auth.
    /// `auth_secret` should name a stored bot/user token handle.
    pub fn slack(id: impl Into<String>, name: impl Into<String>, auth_secret: Option<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::Slack,
            base_url: "https://slack.com/api".to_string(),
            auth_secret,
            auth: AuthScheme::Bearer,
            headers: Vec::new(),
        }
    }

    /// Whether this connector needs a secret to build a valid request.
    pub fn requires_secret(&self) -> bool {
        !matches!(self.auth, AuthScheme::None)
    }
}
