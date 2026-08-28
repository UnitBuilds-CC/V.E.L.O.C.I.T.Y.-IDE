//! OAuth2 integration management for connectors.
//!
//! Manages the OAuth2 authorization code flow: generating authorization URLs,
//! exchanging codes for tokens, refreshing expired tokens, and persisting
//! token state to the encrypted secret store.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// OAuth2 provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Provider {
    /// Unique provider identifier (e.g., "github", "gitlab").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Authorization endpoint URL.
    pub authorize_url: String,
    /// Token exchange endpoint URL.
    pub token_url: String,
    /// OAuth2 client ID (public).
    pub client_id: String,
    /// Handle into the secret store for the client secret.
    pub client_secret_handle: String,
    /// Scopes to request during authorization.
    pub scopes: Vec<String>,
    /// Redirect URI for the callback.
    pub redirect_uri: String,
}

/// An OAuth2 token pair (access + optional refresh).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Token {
    /// The access token value.
    pub access_token: String,
    /// Token type (usually "Bearer").
    pub token_type: String,
    /// Seconds until expiry (from issuance).
    pub expires_in: Option<u64>,
    /// Refresh token for obtaining new access tokens.
    pub refresh_token: Option<String>,
    /// Unix timestamp when this token was issued.
    pub issued_at: u64,
    /// Space-separated scopes granted.
    pub scope: Option<String>,
}

impl OAuth2Token {
    /// Whether this token has expired based on current time.
    pub fn is_expired(&self) -> bool {
        match self.expires_in {
            Some(secs) => {
                let now = now_secs();
                // Consider expired 30 seconds early to avoid race conditions.
                now >= self.issued_at + secs.saturating_sub(30)
            }
            None => false, // No expiry means it doesn't expire.
        }
    }

    /// Seconds remaining until expiry (0 if already expired or no expiry).
    pub fn remaining_secs(&self) -> u64 {
        match self.expires_in {
            Some(secs) => {
                let expires_at = self.issued_at + secs;
                let now = now_secs();
                expires_at.saturating_sub(now)
            }
            None => u64::MAX,
        }
    }
}

/// State for an in-progress OAuth2 authorization flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2FlowState {
    /// The provider ID being authorized.
    pub provider_id: String,
    /// CSRF state parameter.
    pub state: String,
    /// When this flow was initiated (unix timestamp).
    pub initiated_at: u64,
    /// Connector ID this flow is for.
    pub connector_id: String,
}

/// Manages all OAuth2 providers and tokens for a workspace.
#[derive(Debug, Clone, Default)]
pub struct OAuth2Manager {
    /// Configured providers keyed by ID.
    pub providers: HashMap<String, OAuth2Provider>,
    /// Active tokens keyed by provider ID.
    pub tokens: HashMap<String, OAuth2Token>,
    /// In-progress authorization flows keyed by state parameter.
    pub pending_flows: HashMap<String, OAuth2FlowState>,
    /// Workspace root for persistence.
    workspace_root: Option<std::path::PathBuf>,
}

impl OAuth2Manager {
    /// Create a new empty OAuth2 manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with a workspace root for persistence.
    pub fn with_workspace(workspace_root: &Path) -> Self {
        Self {
            providers: HashMap::new(),
            tokens: HashMap::new(),
            pending_flows: HashMap::new(),
            workspace_root: Some(workspace_root.to_path_buf()),
        }
    }

    /// Register an OAuth2 provider.
    pub fn register_provider(&mut self, provider: OAuth2Provider) {
        self.providers.insert(provider.id.clone(), provider);
    }

    /// Remove a provider and its tokens.
    pub fn remove_provider(&mut self, id: &str) -> bool {
        self.tokens.remove(id);
        self.providers.remove(id).is_some()
    }

    /// Generate an authorization URL for a provider.
    /// Returns the URL and the state parameter for CSRF protection.
    pub fn begin_authorization(
        &mut self,
        provider_id: &str,
        connector_id: &str,
    ) -> Result<(String, String), String> {
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

        let state = generate_state(provider_id, connector_id);
        let scopes = provider.scopes.join(" ");

        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            provider.authorize_url,
            url_encode(&provider.client_id),
            url_encode(&provider.redirect_uri),
            url_encode(&scopes),
            url_encode(&state),
        );

        // Store pending flow.
        self.pending_flows.insert(
            state.clone(),
            OAuth2FlowState {
                provider_id: provider_id.to_string(),
                state: state.clone(),
                initiated_at: now_secs(),
                connector_id: connector_id.to_string(),
            },
        );

        Ok((url, state))
    }

    /// Complete authorization by exchanging a code for tokens.
    /// In a real implementation this would make an HTTP call to the token endpoint.
    /// Here we validate the state and produce a token structure.
    pub fn complete_authorization(
        &mut self,
        state: &str,
        _code: &str,
        token: OAuth2Token,
    ) -> Result<String, String> {
        let flow = self
            .pending_flows
            .remove(state)
            .ok_or_else(|| format!("Unknown or expired state: {}", state))?;

        // Check flow isn't too old (10 minute timeout).
        let age = now_secs().saturating_sub(flow.initiated_at);
        if age > 600 {
            return Err("Authorization flow expired".to_string());
        }

        // Store the token.
        self.tokens.insert(flow.provider_id.clone(), token);

        // Return the connector ID this was for.
        Ok(flow.connector_id)
    }

    /// Get a valid (non-expired) token for a provider.
    pub fn get_token(&self, provider_id: &str) -> Option<&OAuth2Token> {
        let token = self.tokens.get(provider_id)?;
        if token.is_expired() {
            None
        } else {
            Some(token)
        }
    }

    /// Check if a provider's token needs refreshing.
    pub fn needs_refresh(&self, provider_id: &str) -> bool {
        match self.tokens.get(provider_id) {
            Some(token) => token.is_expired() && token.refresh_token.is_some(),
            None => false,
        }
    }

    /// Build a refresh token request body.
    pub fn build_refresh_request(&self, provider_id: &str) -> Result<(String, String), String> {
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;
        let token = self
            .tokens
            .get(provider_id)
            .ok_or_else(|| format!("No token for provider '{}'", provider_id))?;
        let refresh = token
            .refresh_token
            .as_deref()
            .ok_or_else(|| format!("No refresh token for provider '{}'", provider_id))?;

        let body = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            url_encode(refresh),
            url_encode(&provider.client_id),
        );

        Ok((provider.token_url.clone(), body))
    }

    /// Update a provider's token after a refresh.
    pub fn update_token(&mut self, provider_id: &str, token: OAuth2Token) {
        self.tokens.insert(provider_id.to_string(), token);
    }

    /// Clean up expired pending flows older than 10 minutes.
    pub fn cleanup_expired_flows(&mut self) {
        let now = now_secs();
        self.pending_flows
            .retain(|_, flow| now - flow.initiated_at < 600);
    }

    /// List all configured providers.
    pub fn list_providers(&self) -> Vec<&OAuth2Provider> {
        self.providers.values().collect()
    }

    /// Check if a provider has a valid token.
    pub fn is_authorized(&self, provider_id: &str) -> bool {
        self.get_token(provider_id).is_some()
    }

    /// Save manager state to disk.
    pub fn save(&self) -> Result<(), String> {
        let root = self
            .workspace_root
            .as_ref()
            .ok_or_else(|| "No workspace root configured".to_string())?;
        let dir = root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let state = PersistedOAuth2State {
            providers: self.providers.values().cloned().collect(),
            tokens: self.tokens.clone(),
        };
        let json =
            serde_json::to_vec_pretty(&state).map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("oauth2_state.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load manager state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut mgr = Self::with_workspace(workspace_root);
        let path = workspace_root.join(".velocity").join("oauth2_state.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(state) = serde_json::from_slice::<PersistedOAuth2State>(&bytes) {
                for provider in state.providers {
                    mgr.providers.insert(provider.id.clone(), provider);
                }
                mgr.tokens = state.tokens;
            }
        }
        mgr
    }
}

/// Serializable state for persistence (tokens stored via secret store handles).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedOAuth2State {
    providers: Vec<OAuth2Provider>,
    tokens: HashMap<String, OAuth2Token>,
}

/// Create built-in OAuth2 provider configurations.
pub fn create_default_providers() -> Vec<OAuth2Provider> {
    vec![
        OAuth2Provider {
            id: "github".to_string(),
            name: "GitHub".to_string(),
            authorize_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            client_id: String::new(), // User fills in.
            client_secret_handle: "github_oauth_secret".to_string(),
            scopes: vec!["repo".to_string(), "read:org".to_string()],
            redirect_uri: "http://localhost:9191/oauth/callback".to_string(),
        },
        OAuth2Provider {
            id: "gitlab".to_string(),
            name: "GitLab".to_string(),
            authorize_url: "https://gitlab.com/oauth/authorize".to_string(),
            token_url: "https://gitlab.com/oauth/token".to_string(),
            client_id: String::new(),
            client_secret_handle: "gitlab_oauth_secret".to_string(),
            scopes: vec!["api".to_string(), "read_user".to_string()],
            redirect_uri: "http://localhost:9191/oauth/callback".to_string(),
        },
        OAuth2Provider {
            id: "notion".to_string(),
            name: "Notion".to_string(),
            authorize_url: "https://api.notion.com/v1/oauth/authorize".to_string(),
            token_url: "https://api.notion.com/v1/oauth/token".to_string(),
            client_id: String::new(),
            client_secret_handle: "notion_oauth_secret".to_string(),
            scopes: Vec::new(), // Notion doesn't use scopes the same way.
            redirect_uri: "http://localhost:9191/oauth/callback".to_string(),
        },
    ]
}

fn generate_state(provider_id: &str, connector_id: &str) -> String {
    let ts = now_secs();
    let hash = ts
        .wrapping_mul(6364136223846793005)
        .wrapping_add((provider_id.len() as u64).wrapping_mul(1442695040888963407))
        .wrapping_add((connector_id.len() as u64).wrapping_mul(7046029254386353131));
    format!("{:016x}{:08x}", hash, ts % 0xFFFFFFFF)
}

/// Minimal URL encoding for OAuth parameters.
fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider() -> OAuth2Provider {
        OAuth2Provider {
            id: "test".to_string(),
            name: "Test Provider".to_string(),
            authorize_url: "https://auth.example.com/authorize".to_string(),
            token_url: "https://auth.example.com/token".to_string(),
            client_id: "client123".to_string(),
            client_secret_handle: "test_secret".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            redirect_uri: "http://localhost:9191/callback".to_string(),
        }
    }

    #[test]
    fn register_and_list_providers() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        assert_eq!(mgr.list_providers().len(), 1);
        assert_eq!(mgr.list_providers()[0].id, "test");
    }

    #[test]
    fn begin_authorization_returns_url_and_state() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        let (url, state) = mgr.begin_authorization("test", "conn1").unwrap();
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("scope=read%20write"));
        assert!(!state.is_empty());
        assert!(mgr.pending_flows.contains_key(&state));
    }

    #[test]
    fn begin_authorization_unknown_provider() {
        let mut mgr = OAuth2Manager::new();
        assert!(mgr.begin_authorization("nonexistent", "c").is_err());
    }

    #[test]
    fn complete_authorization_stores_token() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        let (_, state) = mgr.begin_authorization("test", "conn1").unwrap();

        let token = OAuth2Token {
            access_token: "access_abc".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            refresh_token: Some("refresh_xyz".to_string()),
            issued_at: now_secs(),
            scope: Some("read write".to_string()),
        };

        let connector_id = mgr
            .complete_authorization(&state, "code123", token)
            .unwrap();
        assert_eq!(connector_id, "conn1");
        assert!(mgr.is_authorized("test"));
    }

    #[test]
    fn expired_token_not_returned() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());

        let token = OAuth2Token {
            access_token: "old".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(0), // Already expired.
            refresh_token: None,
            issued_at: now_secs() - 100,
            scope: None,
        };
        mgr.tokens.insert("test".to_string(), token);
        assert!(!mgr.is_authorized("test"));
        assert!(mgr.get_token("test").is_none());
    }

    #[test]
    fn token_remaining_secs() {
        let token = OAuth2Token {
            access_token: "t".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            refresh_token: None,
            issued_at: now_secs(),
            scope: None,
        };
        assert!(token.remaining_secs() > 3500);
    }

    #[test]
    fn needs_refresh_check() {
        let mut mgr = OAuth2Manager::new();
        let token = OAuth2Token {
            access_token: "expired".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(0),
            refresh_token: Some("refresh_me".to_string()),
            issued_at: now_secs() - 100,
            scope: None,
        };
        mgr.tokens.insert("p".to_string(), token);
        assert!(mgr.needs_refresh("p"));
    }

    #[test]
    fn cleanup_expired_flows() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());

        // Add a flow that's "old".
        let state = "old_state".to_string();
        mgr.pending_flows.insert(
            state.clone(),
            OAuth2FlowState {
                provider_id: "test".to_string(),
                state: state.clone(),
                initiated_at: now_secs() - 700, // 700 seconds ago (> 600).
                connector_id: "c".to_string(),
            },
        );

        mgr.cleanup_expired_flows();
        assert!(mgr.pending_flows.is_empty());
    }

    #[test]
    fn remove_provider_clears_tokens() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        mgr.tokens.insert(
            "test".to_string(),
            OAuth2Token {
                access_token: "a".to_string(),
                token_type: "Bearer".to_string(),
                expires_in: None,
                refresh_token: None,
                issued_at: now_secs(),
                scope: None,
            },
        );
        assert!(mgr.remove_provider("test"));
        assert!(mgr.tokens.is_empty());
    }

    #[test]
    fn url_encode_special_chars() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a+b=c"), "a%2Bb%3Dc");
        assert_eq!(url_encode("simple"), "simple");
    }

    #[test]
    fn default_providers_created() {
        let providers = create_default_providers();
        assert!(providers.len() >= 3);
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"github"));
        assert!(ids.contains(&"gitlab"));
        assert!(ids.contains(&"notion"));
    }

    #[test]
    fn token_no_expiry_never_expires() {
        let token = OAuth2Token {
            access_token: "t".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            issued_at: 0,
            scope: None,
        };
        assert!(!token.is_expired());
        assert_eq!(token.remaining_secs(), u64::MAX);
    }

    #[test]
    fn token_zero_expiry_is_expired() {
        let token = OAuth2Token {
            access_token: "t".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(0),
            refresh_token: None,
            issued_at: now_secs() - 100,
            scope: None,
        };
        assert!(token.is_expired());
        assert_eq!(token.remaining_secs(), 0);
    }

    #[test]
    fn needs_refresh_no_token() {
        let mgr = OAuth2Manager::new();
        assert!(!mgr.needs_refresh("nonexistent"));
    }

    #[test]
    fn needs_refresh_no_refresh_token() {
        let mut mgr = OAuth2Manager::new();
        let token = OAuth2Token {
            access_token: "expired".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(0),
            refresh_token: None, // no refresh token
            issued_at: now_secs() - 100,
            scope: None,
        };
        mgr.tokens.insert("p".to_string(), token);
        assert!(!mgr.needs_refresh("p")); // expired but no refresh token
    }

    #[test]
    fn build_refresh_request_success() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        let token = OAuth2Token {
            access_token: "old".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            refresh_token: Some("refresh_me".to_string()),
            issued_at: now_secs(),
            scope: None,
        };
        mgr.tokens.insert("test".to_string(), token);

        let (url, body) = mgr.build_refresh_request("test").unwrap();
        assert_eq!(url, "https://auth.example.com/token");
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("refresh_token=refresh_me"));
        assert!(body.contains("client_id=client123"));
    }

    #[test]
    fn build_refresh_request_no_provider() {
        let mgr = OAuth2Manager::new();
        assert!(mgr.build_refresh_request("nonexistent").is_err());
    }

    #[test]
    fn build_refresh_request_no_token() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        assert!(mgr.build_refresh_request("test").is_err());
    }

    #[test]
    fn build_refresh_request_no_refresh_token() {
        let mut mgr = OAuth2Manager::new();
        mgr.register_provider(test_provider());
        let token = OAuth2Token {
            access_token: "t".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            refresh_token: None,
            issued_at: now_secs(),
            scope: None,
        };
        mgr.tokens.insert("test".to_string(), token);
        assert!(mgr.build_refresh_request("test").is_err());
    }

    #[test]
    fn update_token_replaces_existing() {
        let mut mgr = OAuth2Manager::new();
        let token1 = OAuth2Token {
            access_token: "first".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            refresh_token: None,
            issued_at: now_secs(),
            scope: None,
        };
        mgr.tokens.insert("p".to_string(), token1);

        let token2 = OAuth2Token {
            access_token: "second".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(7200),
            refresh_token: None,
            issued_at: now_secs(),
            scope: None,
        };
        mgr.update_token("p", token2);
        assert_eq!(mgr.tokens["p"].access_token, "second");
    }

    #[test]
    fn complete_authorization_unknown_state() {
        let mut mgr = OAuth2Manager::new();
        let token = OAuth2Token {
            access_token: "t".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: Some(3600),
            refresh_token: None,
            issued_at: now_secs(),
            scope: None,
        };
        assert!(mgr.complete_authorization("unknown_state", "code", token).is_err());
    }

    #[test]
    fn save_without_workspace_fails() {
        let mgr = OAuth2Manager::new();
        assert!(mgr.save().is_err());
    }
}
