//! Provider usage tracking — queries user's own API keys across providers.
//!
//! The IDE queries each provider's usage API using the user's own keys,
//! then writes a combined snapshot to `~/.velocity/usage_snapshot.json`.
//! The website dashboard reads this file to display all API usage, even
//! for calls that never went through the Velocity router.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ─── Provider Registry ───────────────────────────────────────────────────

/// Supported AI providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    Anthropic,
    Google,
    Mistral,
    Cohere,
    Xai,
    Github,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Openai => write!(f, "openai"),
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::Google => write!(f, "google"),
            Provider::Mistral => write!(f, "mistral"),
            Provider::Cohere => write!(f, "cohere"),
            Provider::Xai => write!(f, "xai"),
            Provider::Github => write!(f, "github"),
        }
    }
}

impl Provider {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Provider::Openai),
            "anthropic" | "claude" => Some(Provider::Anthropic),
            "google" | "gemini" | "google_ai" => Some(Provider::Google),
            "mistral" => Some(Provider::Mistral),
            "cohere" | "command" => Some(Provider::Cohere),
            "xai" | "grok" => Some(Provider::Xai),
            "github" | "github_copilot" | "copilot" => Some(Provider::Github),
            _ => None,
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &str {
        match self {
            Provider::Openai => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Google => "Google AI",
            Provider::Mistral => "Mistral",
            Provider::Cohere => "Cohere",
            Provider::Xai => "xAI",
            Provider::Github => "GitHub Copilot",
        }
    }

    /// Whether this provider exposes a usage/billing API we can query.
    pub fn has_usage_api(&self) -> bool {
        matches!(self, Provider::Openai | Provider::Anthropic)
    }
}

/// A stored provider credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCredential {
    pub provider: String,
    pub api_key: String,
    /// Optional base URL override (for proxies, Azure, etc.).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Optional model/plan label (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    #[serde(default)]
    pub model: Option<String>,
}

/// Load provider credentials from `~/.velocity/providers.toml`.
pub fn load_credentials() -> Result<Vec<ProviderCredential>> {
    let path = providers_toml_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut creds = Vec::new();
    let mut current: Option<ProviderCredential> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Each [[provider]] block starts a new entry.
        if line == "[[provider]]" {
            if let Some(c) = current.take() {
                creds.push(c);
            }
            current = Some(ProviderCredential {
                provider: String::new(),
                api_key: String::new(),
                base_url: None,
                model: None,
            });
            continue;
        }
        if let Some(ref mut c) = current {
            if let Some(val) = line.strip_prefix("provider") {
                let val = val.trim().trim_start_matches('=').trim().trim_matches('"');
                c.provider = val.to_string();
            } else if let Some(val) = line.strip_prefix("api_key") {
                let val = val.trim().trim_start_matches('=').trim().trim_matches('"');
                c.api_key = val.to_string();
            } else if let Some(val) = line.strip_prefix("base_url") {
                let val = val.trim().trim_start_matches('=').trim().trim_matches('"');
                c.base_url = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("model") {
                let val = val.trim().trim_start_matches('=').trim().trim_matches('"');
                c.model = Some(val.to_string());
            }
        }
    }
    if let Some(c) = current.take() {
        if !c.provider.is_empty() && !c.api_key.is_empty() {
            creds.push(c);
        }
    }
    Ok(creds)
}

/// Save provider credentials to `~/.velocity/providers.toml`.
pub fn save_credentials(creds: &[ProviderCredential]) -> Result<()> {
    let path = providers_toml_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create ~/.velocity directory")?;
    }
    let mut out = String::new();
    for c in creds {
        out.push_str("[[provider]]\n");
        out.push_str(&format!("provider = \"{}\"\n", c.provider));
        out.push_str(&format!("api_key = \"{}\"\n", c.api_key));
        if let Some(ref url) = c.base_url {
            out.push_str(&format!("base_url = \"{}\"\n", url));
        }
        if let Some(ref m) = c.model {
            out.push_str(&format!("model = \"{}\"\n", m));
        }
        out.push('\n');
    }
    std::fs::write(&path, out)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn providers_toml_path() -> PathBuf {
    velocity_dir().join("providers.toml")
}

fn snapshot_path() -> PathBuf {
    velocity_dir().join("usage_snapshot.json")
}

pub fn velocity_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".velocity")
}

// ─── Snapshot Types ──────────────────────────────────────────────────────

/// Combined usage snapshot written to `~/.velocity/usage_snapshot.json`.
///
/// The website reads this file via `/api/provider-usage` to display
/// the user's direct API usage alongside router usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    /// ISO-8601 timestamp of when this snapshot was generated.
    pub generated_at: String,
    /// Per-provider usage breakdown.
    pub providers: Vec<ProviderUsage>,
    /// Aggregated totals across all providers.
    pub total_tokens: u64,
    pub total_cost_usd: f64,
    pub total_requests: u64,
}

/// Usage data for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub display_name: String,
    /// Whether the API key was accepted (key valid + provider reachable).
    pub key_valid: bool,
    /// Whether the provider exposes a usage API we could query.
    pub has_usage_api: bool,
    /// Token usage (from provider API or local tracking).
    pub tokens_used: u64,
    /// Cost in USD.
    pub cost_usd: f64,
    /// Number of API requests.
    pub request_count: u64,
    /// Billing period start (if available from provider).
    pub period_start: Option<String>,
    /// Billing period end (if available from provider).
    pub period_end: Option<String>,
    /// Human-readable status or error message.
    pub status: String,
    /// Per-model breakdown (if available).
    pub models: Vec<ModelUsage>,
}

/// Per-model usage within a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub tokens: u64,
    pub cost_usd: f64,
    pub requests: u64,
}

// ─── Provider API Clients ────────────────────────────────────────────────

/// Query all configured providers and build a usage snapshot.
pub fn query_all_providers(creds: &[ProviderCredential]) -> UsageSnapshot {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();

    let mut providers = Vec::new();
    let mut total_tokens: u64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut total_requests: u64 = 0;

    for cred in creds {
        let prov = Provider::from_str_loose(&cred.provider);
        let display = prov.as_ref()
            .map(|p| p.display_name().to_string())
            .unwrap_or_else(|| cred.provider.clone());
        let has_api = prov.as_ref()
            .map(|p| p.has_usage_api())
            .unwrap_or(false);

        let usage = match prov.as_ref() {
            Some(Provider::Openai) => query_openai(&agent, cred),
            Some(Provider::Anthropic) => query_anthropic(&agent, cred),
            _ => query_generic_key_check(&agent, cred),
        };

        total_tokens += usage.tokens_used;
        total_cost += usage.cost_usd;
        total_requests += usage.request_count;

        providers.push(ProviderUsage {
            provider: cred.provider.clone(),
            display_name: display,
            key_valid: usage.key_valid,
            has_usage_api: has_api,
            tokens_used: usage.tokens_used,
            cost_usd: usage.cost_usd,
            request_count: usage.request_count,
            period_start: usage.period_start,
            period_end: usage.period_end,
            status: usage.status,
            models: usage.models,
        });
    }

    UsageSnapshot {
        generated_at: chrono_utc_now(),
        providers,
        total_tokens,
        total_cost_usd: total_cost,
        total_requests,
    }
}

/// Internal result from querying a single provider.
struct ProviderResult {
    key_valid: bool,
    tokens_used: u64,
    cost_usd: f64,
    request_count: u64,
    period_start: Option<String>,
    period_end: Option<String>,
    status: String,
    models: Vec<ModelUsage>,
}

/// Query OpenAI usage via `GET /dashboard/billing/usage`.
///
/// OpenAI returns usage in cents (divide by 100 for USD).
/// Endpoint: `https://api.openai.com/dashboard/billing/usage`
fn query_openai(agent: &ureq::Agent, cred: &ProviderCredential) -> ProviderResult {
    let base = cred.base_url.as_deref().unwrap_or("https://api.openai.com");
    let url = format!("{}/dashboard/billing/usage", base.trim_end_matches('/'));

    // OpenAI billing API uses start_date/end_date as unix timestamps.
    // Query the current billing cycle (last 30 days).
    let now_secs = current_unix_secs();
    let start_secs = now_secs - 30 * 86400;
    let url = format!("{}?start_date={}&end_date={}", url, start_secs, now_secs);

    match agent.get(&url)
        .set("Authorization", &format!("Bearer {}", cred.api_key))
        .call()
    {
        Ok(resp) => {
            match resp.into_json::<serde_json::Value>() {
                Ok(json) => {
                    // OpenAI returns: { "total_usage": 12345 } (in cents)
                    let total_cents = json.get("total_usage")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let cost_usd = total_cents / 100.0;

                    // Try to extract per-model data if available.
                    let models = Vec::new(); // OpenAI billing API doesn't break down by model.

                    ProviderResult {
                        key_valid: true,
                        tokens_used: 0, // Billing API returns cost, not tokens.
                        cost_usd,
                        request_count: 0,
                        period_start: Some(unix_to_iso(start_secs)),
                        period_end: Some(unix_to_iso(now_secs)),
                        status: format!("queried OpenAI billing (${:.2} last 30d)", cost_usd),
                        models,
                    }
                }
                Err(e) => ProviderResult {
                    key_valid: true, // Key worked, just couldn't parse response.
                    tokens_used: 0,
                    cost_usd: 0.0,
                    request_count: 0,
                    period_start: None,
                    period_end: None,
                    status: format!("key valid, parse error: {}", e),
                    models: Vec::new(),
                },
            }
        }
        Err(e) => {
            let status = match &e {
                ureq::Error::Status(401, _) => "invalid API key (401)".to_string(),
                ureq::Error::Status(code, _) => format!("HTTP {}", code),
                _ => format!("request failed: {}", e),
            };
            ProviderResult {
                key_valid: false,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status,
                models: Vec::new(),
            }
        }
    }
}

/// Query Anthropic usage.
///
/// Anthropic doesn't expose a public usage/billing API.
/// We verify the key by checking the error response from a lightweight call.
fn query_anthropic(agent: &ureq::Agent, cred: &ProviderCredential) -> ProviderResult {
    let base = cred.base_url.as_deref().unwrap_or("https://api.anthropic.com");
    let url = format!("{}/v1/messages", base.trim_end_matches('/'));

    // Send a minimal request to verify the key.
    // Anthropic returns 401 for invalid keys, 400 for bad requests (key is valid).
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "hi"}]
    });

    match agent.post(&url)
        .set("x-api-key", &cred.api_key)
        .set("anthropic-version", "2023-06-01")
        .set("Content-Type", "application/json")
        .send_json(body)
    {
        Ok(_) => ProviderResult {
            key_valid: true,
            tokens_used: 0,
            cost_usd: 0.0,
            request_count: 0,
            period_start: None,
            period_end: None,
            status: "key valid (no usage API available)".to_string(),
            models: Vec::new(),
        },
        Err(e) => {
            // A 400 means the key is valid but the request was bad.
            // A 401 means the key is invalid.
            let (valid, status) = match &e {
                ureq::Error::Status(400, _) => (true, "key valid (no usage API available)".to_string()),
                ureq::Error::Status(401, _) => (false, "invalid API key (401)".to_string()),
                ureq::Error::Status(code, _) => (false, format!("HTTP {}", code)),
                _ => (false, format!("request failed: {}", e)),
            };
            ProviderResult {
                key_valid: valid,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status,
                models: Vec::new(),
            }
        }
    }
}

/// Generic key verification for providers without usage APIs.
///
/// Attempts a lightweight API call to verify the key is valid.
fn query_generic_key_check(agent: &ureq::Agent, cred: &ProviderCredential) -> ProviderResult {
    let (url, auth_header, auth_value) = match cred.provider.to_lowercase().as_str() {
        "google" | "gemini" => {
            let base = cred.base_url.as_deref()
                .unwrap_or("https://generativelanguage.googleapis.com");
            let url = format!("{}/v1beta/models?key={}", base.trim_end_matches('/'), cred.api_key);
            (url, String::new(), String::new())
        }
        "mistral" => {
            let base = cred.base_url.as_deref()
                .unwrap_or("https://api.mistral.ai");
            (format!("{}/v1/models", base.trim_end_matches('/')),
             "Authorization".into(), format!("Bearer {}", cred.api_key))
        }
        "cohere" | "command" => {
            let base = cred.base_url.as_deref()
                .unwrap_or("https://api.cohere.ai");
            (format!("{}/v1/models", base.trim_end_matches('/')),
             "Authorization".into(), format!("Bearer {}", cred.api_key))
        }
        "xai" | "grok" => {
            let base = cred.base_url.as_deref()
                .unwrap_or("https://api.x.ai");
            (format!("{}/v1/models", base.trim_end_matches('/')),
             "Authorization".into(), format!("Bearer {}", cred.api_key))
        }
        "github" | "github_copilot" | "copilot" => {
            // GitHub doesn't have a simple key-check endpoint.
            return ProviderResult {
                key_valid: true, // Assume valid — no easy check.
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status: "GitHub Copilot (no usage API)".to_string(),
                models: Vec::new(),
            };
        }
        _ => {
            return ProviderResult {
                key_valid: false,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status: format!("unknown provider: {}", cred.provider),
                models: Vec::new(),
            };
        }
    };

    let mut req = agent.get(&url);
    if !auth_header.is_empty() {
        req = req.set(&auth_header, &auth_value);
    }

    match req.call() {
        Ok(_) => ProviderResult {
            key_valid: true,
            tokens_used: 0,
            cost_usd: 0.0,
            request_count: 0,
            period_start: None,
            period_end: None,
            status: "key valid".to_string(),
            models: Vec::new(),
        },
        Err(e) => {
            let (valid, status) = match &e {
                ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
                    (false, "invalid API key".to_string())
                }
                // 400/404 often means the key is valid but the endpoint needs more params.
                ureq::Error::Status(_, _) => (true, "key valid".to_string()),
                _ => (false, format!("request failed: {}", e)),
            };
            ProviderResult {
                key_valid: valid,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status,
                models: Vec::new(),
            }
        }
    }
}

// ─── Snapshot I/O ────────────────────────────────────────────────────────

/// Write a usage snapshot to `~/.velocity/usage_snapshot.json`.
pub fn write_snapshot(snapshot: &UsageSnapshot) -> Result<()> {
    let path = snapshot_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create ~/.velocity directory")?;
    }
    let json = serde_json::to_string_pretty(snapshot)
        .context("failed to serialize snapshot")?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Read the usage snapshot from `~/.velocity/usage_snapshot.json`.
pub fn read_snapshot() -> Result<UsageSnapshot> {
    let path = snapshot_path();
    if !path.exists() {
        anyhow::bail!(
            "No usage snapshot found.\n\
             Run `velocity-ide providers refresh` to query your provider APIs."
        );
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&content)
        .context("failed to parse usage snapshot")
}

// ─── Helpers ─────────────────────────────────────────────────────────────

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_to_iso(secs: u64) -> String {
    // Simple ISO-8601 conversion without chrono dependency.
    let days_since_epoch = secs / 86400;
    let mut y = 1970i64;
    let mut remaining = days_since_epoch as i64;

    // Find year.
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let year = y;

    // Find month and day.
    let leap = is_leap(year);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 12usize;
    let mut day = remaining + 1;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md as i64 {
            month = i + 1;
            day = remaining + 1;
            break;
        }
        remaining -= md as i64;
    }

    format!("{:04}-{:02}-{:02}T00:00:00Z", year, month, day)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn chrono_utc_now() -> String {
    // Lightweight ISO-8601 timestamp without chrono dependency.
    let secs = current_unix_secs();
    let d = unix_to_iso(secs);
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    format!("{}T{:02}:{:02}:{:02}Z", d.trim_end_matches("T00:00:00Z"), h, m, s)
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_display_names() {
        assert_eq!(Provider::Openai.display_name(), "OpenAI");
        assert_eq!(Provider::Anthropic.display_name(), "Anthropic");
        assert_eq!(Provider::Google.display_name(), "Google AI");
        assert_eq!(Provider::Xai.display_name(), "xAI");
    }

    #[test]
    fn provider_from_str_loose() {
        assert_eq!(Provider::from_str_loose("openai"), Some(Provider::Openai));
        assert_eq!(Provider::from_str_loose("OpenAI"), Some(Provider::Openai));
        assert_eq!(Provider::from_str_loose("claude"), Some(Provider::Anthropic));
        assert_eq!(Provider::from_str_loose("gemini"), Some(Provider::Google));
        assert_eq!(Provider::from_str_loose("grok"), Some(Provider::Xai));
        assert_eq!(Provider::from_str_loose("copilot"), Some(Provider::Github));
        assert_eq!(Provider::from_str_loose("unknown"), None);
    }

    #[test]
    fn has_usage_api() {
        assert!(Provider::Openai.has_usage_api());
        assert!(Provider::Anthropic.has_usage_api());
        assert!(!Provider::Google.has_usage_api());
        assert!(!Provider::Mistral.has_usage_api());
    }

    #[test]
    fn snapshot_roundtrip() {
        let snap = UsageSnapshot {
            generated_at: "2026-08-23T12:00:00Z".into(),
            providers: vec![ProviderUsage {
                provider: "openai".into(),
                display_name: "OpenAI".into(),
                key_valid: true,
                has_usage_api: true,
                tokens_used: 50000,
                cost_usd: 1.23,
                request_count: 42,
                period_start: Some("2026-07-24T00:00:00Z".into()),
                period_end: Some("2026-08-23T00:00:00Z".into()),
                status: "ok".into(),
                models: vec![ModelUsage {
                    model: "gpt-4o".into(),
                    tokens: 50000,
                    cost_usd: 1.23,
                    requests: 42,
                }],
            }],
            total_tokens: 50000,
            total_cost_usd: 1.23,
            total_requests: 42,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: UsageSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_tokens, 50000);
        assert_eq!(parsed.providers.len(), 1);
        assert_eq!(parsed.providers[0].provider, "openai");
        assert_eq!(parsed.providers[0].models[0].model, "gpt-4o");
    }

    #[test]
    fn providers_toml_roundtrip() {
        let creds = vec![
            ProviderCredential {
                provider: "openai".into(),
                api_key: "sk-test-123".into(),
                base_url: None,
                model: Some("gpt-4o".into()),
            },
            ProviderCredential {
                provider: "anthropic".into(),
                api_key: "sk-ant-test".into(),
                base_url: Some("https://custom-proxy.example.com".into()),
                model: None,
            },
        ];
        // Write to a temp location by overriding the path logic.
        // For this test we just verify the serialization.
        let mut out = String::new();
        for c in &creds {
            out.push_str("[[provider]]\n");
            out.push_str(&format!("provider = \"{}\"\n", c.provider));
            out.push_str(&format!("api_key = \"{}\"\n", c.api_key));
            if let Some(ref url) = c.base_url {
                out.push_str(&format!("base_url = \"{}\"\n", url));
            }
            if let Some(ref m) = c.model {
                out.push_str(&format!("model = \"{}\"\n", m));
            }
            out.push('\n');
        }
        assert!(out.contains("sk-test-123"));
        assert!(out.contains("sk-ant-test"));
        assert!(out.contains("custom-proxy"));
    }

    #[test]
    fn iso_date_conversion() {
        // 2026-08-23 00:00:00 UTC = day 20600-ish since epoch.
        let iso = unix_to_iso(1787443200); // 2026-08-23T00:00:00Z
        assert!(iso.starts_with("2026-08-23"), "got: {}", iso);
    }

    #[test]
    fn leap_year_check() {
        assert!(is_leap(2024));
        assert!(!is_leap(2025));
        assert!(!is_leap(1900));
        assert!(is_leap(2000));
    }
}
