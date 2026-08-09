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
    GitLab,
    Jira,
    Slack,
    Discord,
    Notion,
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
    pub fn generic(
        id: impl Into<String>,
        name: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
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
    pub fn github(
        id: impl Into<String>,
        name: impl Into<String>,
        auth_secret: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::GitHub,
            base_url: "https://api.github.com".to_string(),
            auth_secret,
            auth: AuthScheme::Bearer,
            headers: vec![
                (
                    "Accept".to_string(),
                    "application/vnd.github+json".to_string(),
                ),
                ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
            ],
        }
    }

    /// Slack Web API preset: `https://slack.com/api`, bearer auth.
    /// `auth_secret` should name a stored bot/user token handle.
    pub fn slack(
        id: impl Into<String>,
        name: impl Into<String>,
        auth_secret: Option<String>,
    ) -> Self {
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

    /// GitLab REST preset: `https://gitlab.com/api/v4`, private-token header auth.
    pub fn gitlab(
        id: impl Into<String>,
        name: impl Into<String>,
        auth_secret: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::GitLab,
            base_url: "https://gitlab.com/api/v4".to_string(),
            auth_secret,
            auth: AuthScheme::Header {
                name: "PRIVATE-TOKEN".to_string(),
            },
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        }
    }

    /// Jira Cloud REST preset: `https://api.atlassian.com/ex/jira`, basic auth
    /// via bearer (cloud API token). `auth_secret` should name a stored token.
    pub fn jira(
        id: impl Into<String>,
        name: impl Into<String>,
        cloud_id: &str,
        auth_secret: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::Jira,
            base_url: format!("https://api.atlassian.com/ex/jira/{cloud_id}"),
            auth_secret,
            auth: AuthScheme::Bearer,
            headers: vec![
                ("Accept".to_string(), "application/json".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
        }
    }

    /// Discord webhook/API preset: `https://discord.com/api/v10`, bearer auth.
    pub fn discord(
        id: impl Into<String>,
        name: impl Into<String>,
        auth_secret: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::Discord,
            base_url: "https://discord.com/api/v10".to_string(),
            auth_secret,
            auth: AuthScheme::Bearer,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        }
    }

    /// Notion API preset: `https://api.notion.com/v1`, bearer auth with
    /// Notion-Version header.
    pub fn notion(
        id: impl Into<String>,
        name: impl Into<String>,
        auth_secret: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::Notion,
            base_url: "https://api.notion.com/v1".to_string(),
            auth_secret,
            auth: AuthScheme::Bearer,
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Notion-Version".to_string(), "2022-06-28".to_string()),
            ],
        }
    }

    /// Webhook connector: POST-only outbound to an arbitrary URL.
    pub fn webhook(id: impl Into<String>, name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ConnectorKind::Webhook,
            base_url: url.into(),
            auth_secret: None,
            auth: AuthScheme::None,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        }
    }

    /// Whether this connector needs a secret to build a valid request.
    pub fn requires_secret(&self) -> bool {
        !matches!(self.auth, AuthScheme::None)
    }
}

impl ConnectorKind {
    /// Human-readable label for the connector kind.
    pub fn label(&self) -> &'static str {
        match self {
            Self::GenericRest => "generic",
            Self::Webhook => "webhook",
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Jira => "jira",
            Self::Slack => "slack",
            Self::Discord => "discord",
            Self::Notion => "notion",
        }
    }
}
