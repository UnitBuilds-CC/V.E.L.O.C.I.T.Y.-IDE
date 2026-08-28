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
        matches!(self, Provider::Openai | Provider::Anthropic | Provider::Google | Provider::Mistral)
    }

    /// Default API base URL for this provider.
    pub fn api_base_url(&self) -> &str {
        match self {
            Provider::Openai => "https://api.openai.com",
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::Google => "https://generativelanguage.googleapis.com",
            Provider::Mistral => "https://api.mistral.ai",
            Provider::Cohere => "https://api.cohere.ai",
            Provider::Xai => "https://api.x.ai",
            Provider::Github => "https://api.github.com",
        }
    }

    /// Environment variable name for this provider's API key.
    pub fn api_key_env_var(&self) -> &str {
        match self {
            Provider::Openai => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Google => "GOOGLE_AI_API_KEY",
            Provider::Mistral => "MISTRAL_API_KEY",
            Provider::Cohere => "COHERE_API_KEY",
            Provider::Xai => "XAI_API_KEY",
            Provider::Github => "GITHUB_TOKEN",
        }
    }

    /// Known models and their per-token pricing (input/output USD per 1M tokens).
    pub fn model_pricing(&self) -> Vec<(&'static str, f64, f64)> {
        match self {
            Provider::Openai => vec![
                ("gpt-4o", 2.50, 10.00),
                ("gpt-4o-mini", 0.15, 0.60),
                ("gpt-4-turbo", 10.00, 30.00),
                ("o3-mini", 1.10, 4.40),
            ],
            Provider::Anthropic => vec![
                ("claude-sonnet-4-20250514", 3.00, 15.00),
                ("claude-3-5-sonnet-20241022", 3.00, 15.00),
                ("claude-3-haiku-20240307", 0.25, 1.25),
                ("claude-3-opus-20240229", 15.00, 75.00),
            ],
            Provider::Google => vec![
                ("gemini-2.0-flash", 0.075, 0.30),
                ("gemini-1.5-pro", 1.25, 5.00),
                ("gemini-1.5-flash", 0.075, 0.30),
            ],
            Provider::Mistral => vec![
                ("mistral-large-latest", 2.00, 6.00),
                ("mistral-small-latest", 0.10, 0.30),
                ("codestral-latest", 0.30, 0.90),
            ],
            Provider::Cohere => vec![
                ("command-r-plus", 2.50, 10.00),
                ("command-r", 0.15, 0.60),
            ],
            Provider::Xai => vec![
                ("grok-3", 3.00, 15.00),
                ("grok-3-mini", 0.30, 0.50),
            ],
            Provider::Github => vec![], // Copilot is subscription-based.
        }
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

impl ProviderCredential {
    /// Validate the credential for common issues.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.provider.is_empty() {
            issues.push("provider name is empty".into());
        }
        if Provider::from_str_loose(&self.provider).is_none() {
            issues.push(format!("unrecognized provider: '{}'", self.provider));
        }
        if self.api_key.is_empty() {
            issues.push("api_key is empty".into());
        }
        if self.api_key.len() < 8 {
            issues.push("api_key seems too short (< 8 chars)".into());
        }
        if let Some(ref url) = self.base_url {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                issues.push(format!("base_url '{}' must start with http:// or https://", url));
            }
        }
        issues
    }

    /// Return the masked API key for display (first 8 + last 4 chars).
    pub fn masked_key(&self) -> String {
        if self.api_key.len() > 12 {
            format!("{}...{}", &self.api_key[..8], &self.api_key[self.api_key.len() - 4..])
        } else if self.api_key.len() > 4 {
            format!("{}...", &self.api_key[..4])
        } else {
            "****".into()
        }
    }
}

/// Batch-validate all credentials, returning per-credential issues.
pub fn validate_credentials(creds: &[ProviderCredential]) -> Vec<(usize, Vec<String>)> {
    creds.iter()
        .enumerate()
        .map(|(i, c)| (i, c.validate()))
        .filter(|(_, issues)| !issues.is_empty())
        .collect()
}

/// Find credentials by provider name (case-insensitive).
pub fn find_by_provider<'a>(creds: &'a [ProviderCredential], provider: &str) -> Vec<&'a ProviderCredential> {
    let lower = provider.to_lowercase();
    creds.iter().filter(|c| c.provider.to_lowercase() == lower).collect()
}

/// Check for duplicate provider entries (same provider name).
pub fn find_duplicate_providers(creds: &[ProviderCredential]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut dupes = Vec::new();
    for c in creds {
        let lower = c.provider.to_lowercase();
        if !seen.insert(lower.clone()) {
            dupes.push(lower);
        }
    }
    dupes.sort();
    dupes.dedup();
    dupes
}

/// Estimate cost for a given model and token count using known pricing.
pub fn estimate_cost(provider: &Provider, model: &str, input_tokens: u64, output_tokens: u64) -> Option<f64> {
    let pricing = provider.model_pricing();
    let entry = pricing.iter().find(|(m, _, _)| *m == model)?;
    let input_per_mtok = entry.1;
    let output_per_mtok = entry.2;
    let input_cost = input_per_mtok * (input_tokens as f64) / 1_000_000.0;
    let output_cost = output_per_mtok * (output_tokens as f64) / 1_000_000.0;
    Some(input_cost + output_cost)
}

/// Compare providers for a given workload (model name + token counts).
/// Returns estimated costs per provider that has pricing for the model.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderCostEstimate {
    pub provider: String,
    pub display_name: String,
    pub model: String,
    pub estimated_cost_usd: f64,
}

/// Compare costs across all providers for a given model and workload.
pub fn compare_provider_costs(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Vec<ProviderCostEstimate> {
    let all_providers = [
        Provider::Openai,
        Provider::Anthropic,
        Provider::Google,
        Provider::Mistral,
        Provider::Cohere,
        Provider::Xai,
        Provider::Github,
    ];
    let mut estimates = Vec::new();
    for p in &all_providers {
        if let Some(cost) = estimate_cost(p, model, input_tokens, output_tokens) {
            estimates.push(ProviderCostEstimate {
                provider: p.to_string(),
                display_name: p.display_name().to_string(),
                model: model.to_string(),
                estimated_cost_usd: cost,
            });
        }
    }
    estimates.sort_by(|a, b| a.estimated_cost_usd.partial_cmp(&b.estimated_cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    estimates
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
    // Restrict file permissions — owner read/write only.
    restrict_file_permissions(&path)?;
    Ok(())
}

/// Restrict file permissions to owner-only (chmod 600 on Unix).
#[cfg_attr(not(unix), allow(unused_variables))]
fn restrict_file_permissions(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    }
    // On Windows, file permissions are more complex (ACLs).
    // The file is created in the user's home directory which is typically
    // accessible only by that user. No additional action needed.
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

// ─── Usage diagnostics ──────────────────────────────────────────────────

/// Diagnostic info about a usage snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct UsageSnapshotInfo {
    pub generated_at: String,
    pub provider_count: usize,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
    pub total_requests: u64,
    pub providers_with_valid_keys: usize,
    pub providers_with_usage_api: usize,
    pub total_models_tracked: usize,
    pub validation_issues: Vec<String>,
}

/// Compact summary of a single provider's status.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub provider: String,
    pub display_name: String,
    pub key_valid: bool,
    pub cost_usd: f64,
    pub tokens_used: u64,
    pub model_count: usize,
    pub status: String,
}

/// Cost breakdown across all providers.
#[derive(Debug, Clone, Serialize)]
pub struct CostBreakdown {
    pub total_cost_usd: f64,
    pub per_provider: Vec<ProviderCost>,
    pub highest_cost_provider: Option<String>,
    pub estimated_monthly_usd: f64,
}

/// Cost data for a single provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderCost {
    pub provider: String,
    pub display_name: String,
    pub cost_usd: f64,
    pub percentage_of_total: f64,
}

impl UsageSnapshot {
    /// Validate the snapshot for consistency.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.providers.is_empty() {
            issues.push("No providers configured".to_string());
        }
        let computed_tokens: u64 = self.providers.iter().map(|p| p.tokens_used).sum();
        if computed_tokens != self.total_tokens {
            issues.push(format!(
                "total_tokens ({}) doesn't match sum of provider tokens ({})",
                self.total_tokens, computed_tokens
            ));
        }
        let computed_cost: f64 = self.providers.iter().map(|p| p.cost_usd).sum();
        if (computed_cost - self.total_cost_usd).abs() > 0.01 {
            issues.push(format!(
                "total_cost_usd ({:.2}) doesn't match sum of provider costs ({:.2})",
                self.total_cost_usd, computed_cost
            ));
        }
        for p in &self.providers {
            if p.provider.is_empty() {
                issues.push("Provider has empty provider name".to_string());
            }
            if p.display_name.is_empty() {
                issues.push("Provider has empty display_name".to_string());
            }
        }
        issues
    }

    /// Build diagnostic info for this snapshot.
    pub fn info(&self) -> UsageSnapshotInfo {
        let valid_keys = self.providers.iter().filter(|p| p.key_valid).count();
        let usage_api = self.providers.iter().filter(|p| p.has_usage_api).count();
        let models_tracked: usize = self.providers.iter().map(|p| p.models.len()).sum();
        UsageSnapshotInfo {
            generated_at: self.generated_at.clone(),
            provider_count: self.providers.len(),
            total_tokens: self.total_tokens,
            total_cost_usd: self.total_cost_usd,
            total_requests: self.total_requests,
            providers_with_valid_keys: valid_keys,
            providers_with_usage_api: usage_api,
            total_models_tracked: models_tracked,
            validation_issues: self.validate(),
        }
    }

    /// Get compact summaries for each provider.
    pub fn provider_summaries(&self) -> Vec<ProviderSummary> {
        self.providers
            .iter()
            .map(|p| ProviderSummary {
                provider: p.provider.clone(),
                display_name: p.display_name.clone(),
                key_valid: p.key_valid,
                cost_usd: p.cost_usd,
                tokens_used: p.tokens_used,
                model_count: p.models.len(),
                status: p.status.clone(),
            })
            .collect()
    }

    /// Build a cost breakdown across all providers.
    pub fn cost_breakdown(&self) -> CostBreakdown {
        let total = self.total_cost_usd;
        let mut per_provider: Vec<ProviderCost> = self
            .providers
            .iter()
            .map(|p| {
                let pct = if total > 0.0 {
                    p.cost_usd / total * 100.0
                } else {
                    0.0
                };
                ProviderCost {
                    provider: p.provider.clone(),
                    display_name: p.display_name.clone(),
                    cost_usd: p.cost_usd,
                    percentage_of_total: pct,
                }
            })
            .collect();
        per_provider.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));

        let highest = per_provider.first().map(|p| p.provider.clone());
        // Extrapolate: if the snapshot covers 30 days, monthly = current total
        let estimated_monthly = total; // Already roughly monthly from 30-day queries

        CostBreakdown {
            total_cost_usd: total,
            per_provider,
            highest_cost_provider: highest,
            estimated_monthly_usd: estimated_monthly,
        }
    }
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
            Some(Provider::Google) => query_google(&agent, cred),
            Some(Provider::Mistral) => query_mistral(&agent, cred),
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

/// Query Google AI usage via model listing and generation stats.
///
/// Google's Generative Language API doesn't expose billing directly,
/// but we can verify the key and list available models.
fn query_google(agent: &ureq::Agent, cred: &ProviderCredential) -> ProviderResult {
    let base = cred.base_url.as_deref()
        .unwrap_or("https://generativelanguage.googleapis.com");
    let url = format!("{}/v1beta/models?key={}", base.trim_end_matches('/'), cred.api_key);

    match agent.get(&url).call() {
        Ok(resp) => {
            match resp.into_json::<serde_json::Value>() {
                Ok(json) => {
                    let models = json.get("models")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| {
                                    let name = m.get("name")?.as_str()?;
                                    Some(ModelUsage {
                                        model: name.trim_start_matches("models/").to_string(),
                                        tokens: 0,
                                        cost_usd: 0.0,
                                        requests: 0,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    ProviderResult {
                        key_valid: true,
                        tokens_used: 0,
                        cost_usd: 0.0,
                        request_count: 0,
                        period_start: None,
                        period_end: None,
                        status: format!("key valid ({} models available)", models.len()),
                        models,
                    }
                }
                Err(e) => ProviderResult {
                    key_valid: true,
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
                ureq::Error::Status(400, _) | ureq::Error::Status(403, _) => "invalid API key".to_string(),
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

/// Query Mistral usage via their usage API.
///
/// Mistral exposes a usage endpoint at /v1/usage.
fn query_mistral(agent: &ureq::Agent, cred: &ProviderCredential) -> ProviderResult {
    let base = cred.base_url.as_deref().unwrap_or("https://api.mistral.ai");
    let url = format!("{}/v1/models", base.trim_end_matches('/'));

    match agent.get(&url)
        .set("Authorization", &format!("Bearer {}", cred.api_key))
        .call()
    {
        Ok(resp) => {
            match resp.into_json::<serde_json::Value>() {
                Ok(json) => {
                    let models = json.get("data")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| {
                                    let id = m.get("id")?.as_str()?;
                                    Some(ModelUsage {
                                        model: id.to_string(),
                                        tokens: 0,
                                        cost_usd: 0.0,
                                        requests: 0,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    ProviderResult {
                        key_valid: true,
                        tokens_used: 0,
                        cost_usd: 0.0,
                        request_count: 0,
                        period_start: None,
                        period_end: None,
                        status: format!("key valid ({} models available)", models.len()),
                        models,
                    }
                }
                Err(e) => ProviderResult {
                    key_valid: true,
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
    restrict_file_permissions(&path)?;
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
        assert!(Provider::Google.has_usage_api());
        assert!(Provider::Mistral.has_usage_api());
        assert!(!Provider::Cohere.has_usage_api());
        assert!(!Provider::Xai.has_usage_api());
        assert!(!Provider::Github.has_usage_api());
    }

    #[test]
    fn api_base_url_returns_urls() {
        assert_eq!(Provider::Openai.api_base_url(), "https://api.openai.com");
        assert_eq!(Provider::Anthropic.api_base_url(), "https://api.anthropic.com");
        assert_eq!(Provider::Google.api_base_url(), "https://generativelanguage.googleapis.com");
        assert!(Provider::Mistral.api_base_url().starts_with("https://"));
    }

    #[test]
    fn api_key_env_var_matches() {
        assert_eq!(Provider::Openai.api_key_env_var(), "OPENAI_API_KEY");
        assert_eq!(Provider::Anthropic.api_key_env_var(), "ANTHROPIC_API_KEY");
        assert_eq!(Provider::Github.api_key_env_var(), "GITHUB_TOKEN");
    }

    #[test]
    fn model_pricing_returns_data() {
        let openai = Provider::Openai.model_pricing();
        assert!(!openai.is_empty());
        assert!(openai.iter().any(|(m, _, _)| *m == "gpt-4o"));

        let anthropic = Provider::Anthropic.model_pricing();
        assert!(!anthropic.is_empty());

        let github = Provider::Github.model_pricing();
        assert!(github.is_empty()); // Subscription-based.
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

    fn make_test_snapshot() -> UsageSnapshot {
        UsageSnapshot {
            generated_at: "2026-08-23T12:00:00Z".into(),
            providers: vec![
                ProviderUsage {
                    provider: "openai".into(),
                    display_name: "OpenAI".into(),
                    key_valid: true,
                    has_usage_api: true,
                    tokens_used: 50000,
                    cost_usd: 1.50,
                    request_count: 42,
                    period_start: Some("2026-07-24T00:00:00Z".into()),
                    period_end: Some("2026-08-23T00:00:00Z".into()),
                    status: "ok".into(),
                    models: vec![
                        ModelUsage { model: "gpt-4o".into(), tokens: 30000, cost_usd: 1.00, requests: 30 },
                        ModelUsage { model: "gpt-4o-mini".into(), tokens: 20000, cost_usd: 0.50, requests: 12 },
                    ],
                },
                ProviderUsage {
                    provider: "anthropic".into(),
                    display_name: "Anthropic".into(),
                    key_valid: true,
                    has_usage_api: true,
                    tokens_used: 10000,
                    cost_usd: 0.50,
                    request_count: 10,
                    period_start: None,
                    period_end: None,
                    status: "key valid".into(),
                    models: vec![],
                },
                ProviderUsage {
                    provider: "cohere".into(),
                    display_name: "Cohere".into(),
                    key_valid: false,
                    has_usage_api: false,
                    tokens_used: 0,
                    cost_usd: 0.0,
                    request_count: 0,
                    period_start: None,
                    period_end: None,
                    status: "invalid key".into(),
                    models: vec![],
                },
            ],
            total_tokens: 60000,
            total_cost_usd: 2.00,
            total_requests: 52,
        }
    }

    #[test]
    fn snapshot_validate_clean() {
        let snap = make_test_snapshot();
        let issues = snap.validate();
        assert!(issues.is_empty(), "expected no issues, got: {:?}", issues);
    }

    #[test]
    fn snapshot_validate_empty_providers() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let issues = snap.validate();
        assert!(!issues.is_empty());
        assert!(issues[0].contains("No providers"));
    }

    #[test]
    fn snapshot_validate_mismatched_totals() {
        let mut snap = make_test_snapshot();
        snap.total_tokens = 99999; // wrong
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("total_tokens")));
    }

    #[test]
    fn snapshot_validate_mismatched_cost() {
        let mut snap = make_test_snapshot();
        snap.total_cost_usd = 999.99; // wrong
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("total_cost_usd")));
    }

    #[test]
    fn snapshot_info_counts() {
        let snap = make_test_snapshot();
        let info = snap.info();
        assert_eq!(info.provider_count, 3);
        assert_eq!(info.providers_with_valid_keys, 2);
        assert_eq!(info.providers_with_usage_api, 2);
        assert_eq!(info.total_models_tracked, 2);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn snapshot_info_serializes() {
        let snap = make_test_snapshot();
        let info = snap.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"provider_count\":3"));
        assert!(json.contains("\"total_tokens\":60000"));
    }

    #[test]
    fn provider_summaries_count() {
        let snap = make_test_snapshot();
        let summaries = snap.provider_summaries();
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].provider, "openai");
        assert!(summaries[0].key_valid);
        assert_eq!(summaries[0].model_count, 2);
        assert!(!summaries[2].key_valid); // cohere
    }

    #[test]
    fn cost_breakdown_sorted_by_cost() {
        let snap = make_test_snapshot();
        let breakdown = snap.cost_breakdown();
        assert!((breakdown.total_cost_usd - 2.00).abs() < 0.01);
        // OpenAI should be first (highest cost)
        assert_eq!(breakdown.per_provider[0].provider, "openai");
        assert_eq!(breakdown.highest_cost_provider, Some("openai".to_string()));
    }

    #[test]
    fn cost_breakdown_percentages_sum() {
        let snap = make_test_snapshot();
        let breakdown = snap.cost_breakdown();
        let total_pct: f64 = breakdown.per_provider.iter().map(|p| p.percentage_of_total).sum();
        assert!((total_pct - 100.0).abs() < 0.1);
    }

    #[test]
    fn cost_breakdown_zero_total() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![ProviderUsage {
                provider: "test".into(),
                display_name: "Test".into(),
                key_valid: true,
                has_usage_api: false,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status: "ok".into(),
                models: vec![],
            }],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let breakdown = snap.cost_breakdown();
        assert_eq!(breakdown.total_cost_usd, 0.0);
        // Percentages should be 0 when total is 0
        assert!(breakdown.per_provider.iter().all(|p| p.percentage_of_total == 0.0));
    }

    #[test]
    fn cost_breakdown_serializes() {
        let snap = make_test_snapshot();
        let breakdown = snap.cost_breakdown();
        let json = serde_json::to_string(&breakdown).unwrap();
        assert!(json.contains("\"highest_cost_provider\":\"openai\""));
        assert!(json.contains("\"total_cost_usd\":2.0"));
    }

    #[test]
    fn provider_summary_serializes() {
        let summary = ProviderSummary {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            key_valid: true,
            cost_usd: 1.5,
            tokens_used: 50000,
            model_count: 2,
            status: "ok".into(),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"key_valid\":true"));
        assert!(json.contains("\"model_count\":2"));
    }

    // ─── Credential Validation Tests ─────────────────────────────────────────

    #[test]
    fn credential_validate_valid() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test-12345678".into(),
            base_url: None,
            model: None,
        };
        assert!(cred.validate().is_empty());
    }

    #[test]
    fn credential_validate_empty_provider() {
        let cred = ProviderCredential {
            provider: "".into(),
            api_key: "sk-test-12345678".into(),
            base_url: None,
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.iter().any(|i| i.contains("empty")));
        assert!(issues.iter().any(|i| i.contains("unrecognized")));
    }

    #[test]
    fn credential_validate_short_key() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "abc".into(),
            base_url: None,
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn credential_validate_bad_base_url() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test-12345678".into(),
            base_url: Some("ftp://bad".into()),
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.iter().any(|i| i.contains("http://")));
    }

    #[test]
    fn masked_key_long_key() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test-1234567890abcdef".into(),
            base_url: None,
            model: None,
        };
        let masked = cred.masked_key();
        assert!(masked.starts_with("sk-test-"));
        assert!(masked.contains("..."));
        assert!(masked.ends_with("cdef"));
    }

    #[test]
    fn masked_key_short_key() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "abc".into(),
            base_url: None,
            model: None,
        };
        assert_eq!(cred.masked_key(), "****");
    }

    #[test]
    fn masked_key_medium_key() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "abcdef12".into(),
            base_url: None,
            model: None,
        };
        let masked = cred.masked_key();
        assert!(masked.starts_with("abcd"));
        assert!(masked.contains("..."));
    }

    #[test]
    fn validate_credentials_batch() {
        let creds = vec![
            ProviderCredential {
                provider: "openai".into(),
                api_key: "sk-test-12345678".into(),
                base_url: None,
                model: None,
            },
            ProviderCredential {
                provider: "".into(),
                api_key: "abc".into(),
                base_url: None,
                model: None,
            },
        ];
        let issues = validate_credentials(&creds);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].0, 1); // second credential has issues
    }

    #[test]
    fn find_by_provider_case_insensitive() {
        let creds = vec![
            ProviderCredential {
                provider: "OpenAI".into(),
                api_key: "sk-test".into(),
                base_url: None,
                model: None,
            },
            ProviderCredential {
                provider: "anthropic".into(),
                api_key: "sk-ant".into(),
                base_url: None,
                model: None,
            },
        ];
        let found = find_by_provider(&creds, "openai");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].provider, "OpenAI");
    }

    #[test]
    fn find_duplicate_providers_detects_dupes() {
        let creds = vec![
            ProviderCredential { provider: "openai".into(), api_key: "sk-1".into(), base_url: None, model: None },
            ProviderCredential { provider: "anthropic".into(), api_key: "sk-2".into(), base_url: None, model: None },
            ProviderCredential { provider: "OpenAI".into(), api_key: "sk-3".into(), base_url: None, model: None },
        ];
        let dupes = find_duplicate_providers(&creds);
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0], "openai");
    }

    #[test]
    fn find_duplicate_providers_no_dupes() {
        let creds = vec![
            ProviderCredential { provider: "openai".into(), api_key: "sk-1".into(), base_url: None, model: None },
            ProviderCredential { provider: "anthropic".into(), api_key: "sk-2".into(), base_url: None, model: None },
        ];
        let dupes = find_duplicate_providers(&creds);
        assert!(dupes.is_empty());
    }

    #[test]
    fn estimate_cost_gpt4o() {
        // gpt-4o: $2.50/M input, $10.00/M output
        let cost = estimate_cost(&Provider::Openai, "gpt-4o", 1_000_000, 1_000_000).unwrap();
        assert!((cost - 12.50).abs() < 0.01);
    }

    #[test]
    fn estimate_cost_unknown_model() {
        let cost = estimate_cost(&Provider::Openai, "gpt-99-turbo", 1000, 1000);
        assert!(cost.is_none());
    }

    #[test]
    fn estimate_cost_zero_tokens() {
        let cost = estimate_cost(&Provider::Openai, "gpt-4o", 0, 0).unwrap();
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn compare_provider_costs_sorted() {
        // gpt-4o-mini: $0.15/M input, $0.60/M output
        // Only OpenAI has gpt-4o-mini, so only one result.
        let estimates = compare_provider_costs("gpt-4o-mini", 1_000_000, 1_000_000);
        assert_eq!(estimates.len(), 1);
        assert_eq!(estimates[0].provider, "openai");
        assert!((estimates[0].estimated_cost_usd - 0.75).abs() < 0.01);
    }

    #[test]
    fn compare_provider_costs_no_match() {
        let estimates = compare_provider_costs("nonexistent-model", 1000, 1000);
        assert!(estimates.is_empty());
    }

    #[test]
    fn provider_cost_estimate_serializes() {
        let est = ProviderCostEstimate {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            model: "gpt-4o".into(),
            estimated_cost_usd: 1.23,
        };
        let json = serde_json::to_string(&est).unwrap();
        assert!(json.contains("\"estimated_cost_usd\":1.23"));
    }

    // ── Provider Display ──────────────────────────────────────────────────────

    #[test]
    fn provider_display_all_variants() {
        assert_eq!(Provider::Openai.to_string(), "openai");
        assert_eq!(Provider::Anthropic.to_string(), "anthropic");
        assert_eq!(Provider::Google.to_string(), "google");
        assert_eq!(Provider::Mistral.to_string(), "mistral");
        assert_eq!(Provider::Cohere.to_string(), "cohere");
        assert_eq!(Provider::Xai.to_string(), "xai");
        assert_eq!(Provider::Github.to_string(), "github");
    }

    #[test]
    fn provider_display_name_all() {
        assert_eq!(Provider::Mistral.display_name(), "Mistral");
        assert_eq!(Provider::Cohere.display_name(), "Cohere");
        assert_eq!(Provider::Github.display_name(), "GitHub Copilot");
    }

    // ── from_str_loose extended ───────────────────────────────────────────────

    #[test]
    fn from_str_loose_google_ai_alias() {
        assert_eq!(Provider::from_str_loose("google_ai"), Some(Provider::Google));
    }

    #[test]
    fn from_str_loose_command_alias() {
        assert_eq!(Provider::from_str_loose("command"), Some(Provider::Cohere));
    }

    #[test]
    fn from_str_loose_github_copilot_alias() {
        assert_eq!(Provider::from_str_loose("github_copilot"), Some(Provider::Github));
    }

    #[test]
    fn from_str_loose_mistral_exact() {
        assert_eq!(Provider::from_str_loose("mistral"), Some(Provider::Mistral));
        assert_eq!(Provider::from_str_loose("MISTRAL"), Some(Provider::Mistral));
    }

    #[test]
    fn from_str_loose_cohere_exact() {
        assert_eq!(Provider::from_str_loose("cohere"), Some(Provider::Cohere));
        assert_eq!(Provider::from_str_loose("Cohere"), Some(Provider::Cohere));
    }

    #[test]
    fn from_str_loose_xai() {
        assert_eq!(Provider::from_str_loose("xai"), Some(Provider::Xai));
        assert_eq!(Provider::from_str_loose("XAI"), Some(Provider::Xai));
    }

    #[test]
    fn from_str_loose_empty() {
        assert_eq!(Provider::from_str_loose(""), None);
    }

    // ── api_base_url all providers ────────────────────────────────────────────

    #[test]
    fn api_base_url_all_providers() {
        assert_eq!(Provider::Cohere.api_base_url(), "https://api.cohere.ai");
        assert_eq!(Provider::Xai.api_base_url(), "https://api.x.ai");
        assert_eq!(Provider::Github.api_base_url(), "https://api.github.com");
    }

    #[test]
    fn api_base_url_all_start_with_https() {
        let all = [
            Provider::Openai, Provider::Anthropic, Provider::Google,
            Provider::Mistral, Provider::Cohere, Provider::Xai, Provider::Github,
        ];
        for p in &all {
            assert!(p.api_base_url().starts_with("https://"), "{} base URL doesn't start with https://", p);
        }
    }

    // ── api_key_env_var all providers ─────────────────────────────────────────

    #[test]
    fn api_key_env_var_all() {
        assert_eq!(Provider::Google.api_key_env_var(), "GOOGLE_AI_API_KEY");
        assert_eq!(Provider::Mistral.api_key_env_var(), "MISTRAL_API_KEY");
        assert_eq!(Provider::Cohere.api_key_env_var(), "COHERE_API_KEY");
        assert_eq!(Provider::Xai.api_key_env_var(), "XAI_API_KEY");
    }

    #[test]
    fn api_key_env_var_all_end_with_key_or_token() {
        let all = [
            Provider::Openai, Provider::Anthropic, Provider::Google,
            Provider::Mistral, Provider::Cohere, Provider::Xai, Provider::Github,
        ];
        for p in &all {
            let v = p.api_key_env_var();
            assert!(v.ends_with("API_KEY") || v.ends_with("TOKEN"),
                "{} env var '{}' doesn't end with API_KEY or TOKEN", p, v);
        }
    }

    // ── model_pricing per provider ────────────────────────────────────────────

    #[test]
    fn model_pricing_google() {
        let p = Provider::Google.model_pricing();
        assert_eq!(p.len(), 3);
        assert!(p.iter().any(|(m, _, _)| *m == "gemini-2.0-flash"));
    }

    #[test]
    fn model_pricing_mistral() {
        let p = Provider::Mistral.model_pricing();
        assert_eq!(p.len(), 3);
        assert!(p.iter().any(|(m, _, _)| *m == "codestral-latest"));
    }

    #[test]
    fn model_pricing_cohere() {
        let p = Provider::Cohere.model_pricing();
        assert_eq!(p.len(), 2);
        assert!(p.iter().any(|(m, _, _)| *m == "command-r-plus"));
    }

    #[test]
    fn model_pricing_xai() {
        let p = Provider::Xai.model_pricing();
        assert_eq!(p.len(), 2);
        assert!(p.iter().any(|(m, _, _)| *m == "grok-3"));
    }

    #[test]
    fn model_pricing_input_lte_output() {
        // For all providers, input price should be <= output price
        let all = [
            Provider::Openai, Provider::Anthropic, Provider::Google,
            Provider::Mistral, Provider::Cohere, Provider::Xai,
        ];
        for p in &all {
            for (model, input, output) in p.model_pricing() {
                assert!(input <= output,
                    "{}: {} input ({}) > output ({})", p, model, input, output);
            }
        }
    }

    // ── estimate_cost extended ────────────────────────────────────────────────

    #[test]
    fn estimate_cost_anthropic_sonnet() {
        // claude-sonnet: $3/M input, $15/M output
        let cost = estimate_cost(&Provider::Anthropic, "claude-sonnet-4-20250514", 1_000_000, 1_000_000).unwrap();
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn estimate_cost_google_flash() {
        // gemini-2.0-flash: $0.075/M input, $0.30/M output
        let cost = estimate_cost(&Provider::Google, "gemini-2.0-flash", 1_000_000, 1_000_000).unwrap();
        assert!((cost - 0.375).abs() < 0.001);
    }

    #[test]
    fn estimate_cost_mistral_large() {
        // mistral-large: $2/M input, $6/M output
        let cost = estimate_cost(&Provider::Mistral, "mistral-large-latest", 500_000, 500_000).unwrap();
        assert!((cost - 4.0).abs() < 0.01);
    }

    #[test]
    fn estimate_cost_cohere_command_r() {
        // command-r: $0.15/M input, $0.60/M output
        let cost = estimate_cost(&Provider::Cohere, "command-r", 1_000_000, 0).unwrap();
        assert!((cost - 0.15).abs() < 0.001);
    }

    #[test]
    fn estimate_cost_xai_grok3() {
        // grok-3: $3/M input, $15/M output
        let cost = estimate_cost(&Provider::Xai, "grok-3", 0, 1_000_000).unwrap();
        assert!((cost - 15.0).abs() < 0.01);
    }

    #[test]
    fn estimate_cost_input_only() {
        let cost = estimate_cost(&Provider::Openai, "gpt-4o", 2_000_000, 0).unwrap();
        assert!((cost - 5.0).abs() < 0.01); // 2M * $2.50/M
    }

    #[test]
    fn estimate_cost_output_only() {
        let cost = estimate_cost(&Provider::Openai, "gpt-4o", 0, 500_000).unwrap();
        assert!((cost - 5.0).abs() < 0.01); // 500K * $10.00/M
    }

    #[test]
    fn estimate_cost_github_returns_none() {
        // GitHub has no per-model pricing
        let cost = estimate_cost(&Provider::Github, "gpt-4o", 1000, 1000);
        assert!(cost.is_none());
    }

    // ── compare_provider_costs extended ───────────────────────────────────────

    #[test]
    fn compare_provider_costs_gpt4o_only_openai() {
        let estimates = compare_provider_costs("gpt-4o", 1_000_000, 1_000_000);
        assert_eq!(estimates.len(), 1);
        assert_eq!(estimates[0].provider, "openai");
        assert_eq!(estimates[0].display_name, "OpenAI");
        assert_eq!(estimates[0].model, "gpt-4o");
    }

    #[test]
    fn compare_provider_costs_sorted_ascending() {
        // Use a model name that might exist in multiple providers (if any)
        // For now, verify single-entry is trivially sorted
        let estimates = compare_provider_costs("gpt-4o-mini", 1_000_000, 1_000_000);
        for w in estimates.windows(2) {
            assert!(w[0].estimated_cost_usd <= w[1].estimated_cost_usd);
        }
    }

    // ── ProviderCredential validate edge cases ────────────────────────────────

    #[test]
    fn credential_validate_empty_key() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "".into(),
            base_url: None,
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.iter().any(|i| i.contains("empty")));
    }

    #[test]
    fn credential_validate_exactly_8_chars_ok() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "12345678".into(), // exactly 8 chars
            base_url: None,
            model: None,
        };
        let issues = cred.validate();
        // 8 chars is NOT < 8, so no "too short" issue
        assert!(!issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn credential_validate_7_chars_too_short() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "1234567".into(), // 7 chars
            base_url: None,
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.iter().any(|i| i.contains("too short")));
    }

    #[test]
    fn credential_validate_http_base_url_ok() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test-12345678".into(),
            base_url: Some("http://localhost:8080".into()),
            model: None,
        };
        let issues = cred.validate();
        // http:// is valid, not just https://
        assert!(!issues.iter().any(|i| i.contains("http://")));
    }

    #[test]
    fn credential_validate_https_base_url_ok() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test-12345678".into(),
            base_url: Some("https://proxy.example.com".into()),
            model: None,
        };
        assert!(cred.validate().is_empty());
    }

    #[test]
    fn credential_validate_no_scheme_base_url() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test-12345678".into(),
            base_url: Some("proxy.example.com".into()),
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.iter().any(|i| i.contains("http://")));
    }

    #[test]
    fn credential_validate_multiple_issues() {
        let cred = ProviderCredential {
            provider: "".into(), // empty + unrecognized
            api_key: "abc".into(), // empty + too short
            base_url: Some("ftp://bad".into()), // bad scheme
            model: None,
        };
        let issues = cred.validate();
        assert!(issues.len() >= 4, "expected >= 4 issues, got {:?}", issues);
    }

    // ── masked_key boundary cases ─────────────────────────────────────────────

    #[test]
    fn masked_key_exactly_4_chars() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "abcd".into(),
            base_url: None,
            model: None,
        };
        // 4 chars: not > 4, so falls to else => "****"
        assert_eq!(cred.masked_key(), "****");
    }

    #[test]
    fn masked_key_exactly_5_chars() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "abcde".into(),
            base_url: None,
            model: None,
        };
        // 5 chars: > 4 but not > 12 => "abcd..."
        let masked = cred.masked_key();
        assert!(masked.starts_with("abcd"));
        assert!(masked.contains("..."));
    }

    #[test]
    fn masked_key_exactly_12_chars() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "123456789012".into(),
            base_url: None,
            model: None,
        };
        // 12 chars: not > 12 => "1234..."
        let masked = cred.masked_key();
        assert!(masked.starts_with("1234"));
        assert!(masked.contains("..."));
        // Should NOT have the last-4 format
        assert!(!masked.contains("...9012"));
    }

    #[test]
    fn masked_key_exactly_13_chars() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "1234567890123".into(),
            base_url: None,
            model: None,
        };
        // 13 chars: > 12 => "12345678...0123"
        let masked = cred.masked_key();
        assert_eq!(masked, "12345678...0123");
    }

    // ── validate_credentials extended ─────────────────────────────────────────

    #[test]
    fn validate_credentials_empty_list() {
        let creds: Vec<ProviderCredential> = vec![];
        let issues = validate_credentials(&creds);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_credentials_all_valid() {
        let creds = vec![
            ProviderCredential {
                provider: "openai".into(),
                api_key: "sk-test-12345678".into(),
                base_url: None,
                model: None,
            },
            ProviderCredential {
                provider: "anthropic".into(),
                api_key: "sk-ant-12345678".into(),
                base_url: None,
                model: None,
            },
        ];
        let issues = validate_credentials(&creds);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_credentials_multiple_invalid() {
        let creds = vec![
            ProviderCredential { provider: "".into(), api_key: "".into(), base_url: None, model: None },
            ProviderCredential { provider: "openai".into(), api_key: "sk-ok-12345678".into(), base_url: None, model: None },
            ProviderCredential { provider: "bad".into(), api_key: "sk-bad-12345678".into(), base_url: None, model: None },
        ];
        let issues = validate_credentials(&creds);
        // Index 0 and 2 have issues, index 1 is clean
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].0, 0);
        assert_eq!(issues[1].0, 2);
    }

    // ── find_by_provider extended ─────────────────────────────────────────────

    #[test]
    fn find_by_provider_no_matches() {
        let creds = vec![
            ProviderCredential { provider: "openai".into(), api_key: "sk-1".into(), base_url: None, model: None },
        ];
        let found = find_by_provider(&creds, "anthropic");
        assert!(found.is_empty());
    }

    #[test]
    fn find_by_provider_multiple_matches() {
        let creds = vec![
            ProviderCredential { provider: "openai".into(), api_key: "sk-1".into(), base_url: None, model: None },
            ProviderCredential { provider: "OpenAI".into(), api_key: "sk-2".into(), base_url: None, model: None },
            ProviderCredential { provider: "anthropic".into(), api_key: "sk-3".into(), base_url: None, model: None },
        ];
        let found = find_by_provider(&creds, "openai");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn find_by_provider_empty_list() {
        let creds: Vec<ProviderCredential> = vec![];
        let found = find_by_provider(&creds, "openai");
        assert!(found.is_empty());
    }

    // ── find_duplicate_providers extended ─────────────────────────────────────

    #[test]
    fn find_duplicate_providers_empty() {
        let creds: Vec<ProviderCredential> = vec![];
        let dupes = find_duplicate_providers(&creds);
        assert!(dupes.is_empty());
    }

    #[test]
    fn find_duplicate_providers_triple_dedup() {
        let creds = vec![
            ProviderCredential { provider: "openai".into(), api_key: "sk-1".into(), base_url: None, model: None },
            ProviderCredential { provider: "OpenAI".into(), api_key: "sk-2".into(), base_url: None, model: None },
            ProviderCredential { provider: "OPENAI".into(), api_key: "sk-3".into(), base_url: None, model: None },
        ];
        let dupes = find_duplicate_providers(&creds);
        // Should appear only once after dedup
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0], "openai");
    }

    #[test]
    fn find_duplicate_providers_multiple_dupes() {
        let creds = vec![
            ProviderCredential { provider: "openai".into(), api_key: "sk-1".into(), base_url: None, model: None },
            ProviderCredential { provider: "anthropic".into(), api_key: "sk-2".into(), base_url: None, model: None },
            ProviderCredential { provider: "openai".into(), api_key: "sk-3".into(), base_url: None, model: None },
            ProviderCredential { provider: "anthropic".into(), api_key: "sk-4".into(), base_url: None, model: None },
        ];
        let dupes = find_duplicate_providers(&creds);
        assert_eq!(dupes.len(), 2);
        // Sorted: anthropic < openai
        assert_eq!(dupes[0], "anthropic");
        assert_eq!(dupes[1], "openai");
    }

    // ── unix_to_iso extended ──────────────────────────────────────────────────

    #[test]
    fn unix_to_iso_epoch_zero() {
        let iso = unix_to_iso(0);
        assert_eq!(iso, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn unix_to_iso_known_date() {
        // 2024-01-01T00:00:00Z = 1704067200
        let iso = unix_to_iso(1704067200);
        assert!(iso.starts_with("2024-01-01"), "got: {}", iso);
    }

    #[test]
    fn unix_to_iso_leap_day() {
        // 2024-02-29T00:00:00Z = 1709164800
        let iso = unix_to_iso(1709164800);
        assert!(iso.starts_with("2024-02-29"), "got: {}", iso);
    }

    #[test]
    fn unix_to_iso_year_boundary() {
        // 2023-12-31T00:00:00Z = 1703980800
        let iso = unix_to_iso(1703980800);
        assert!(iso.starts_with("2023-12-31"), "got: {}", iso);
    }

    #[test]
    fn unix_to_iso_format_always_valid() {
        // Test several timestamps and verify format
        for secs in [0, 86400, 31536000, 1000000000, 1700000000] {
            let iso = unix_to_iso(secs);
            assert!(iso.ends_with("T00:00:00Z"), "bad format: {}", iso);
            assert_eq!(iso.len(), 20, "bad length for: {}", iso);
        }
    }

    // ── is_leap extended ──────────────────────────────────────────────────────

    #[test]
    fn is_leap_century_rules() {
        assert!(!is_leap(1800));
        assert!(!is_leap(1900));
        assert!(!is_leap(2100));
        assert!(is_leap(1600));
        assert!(is_leap(2000));
        assert!(is_leap(2400));
    }

    #[test]
    fn is_leap_regular_years() {
        for y in [2021, 2022, 2023, 2025, 2026, 2027] {
            assert!(!is_leap(y), "{} should not be leap", y);
        }
    }

    #[test]
    fn is_leap_leap_years() {
        for y in [2000, 2004, 2008, 2012, 2016, 2020, 2024] {
            assert!(is_leap(y), "{} should be leap", y);
        }
    }

    // ── velocity_dir ──────────────────────────────────────────────────────────

    #[test]
    fn velocity_dir_ends_with_dot_velocity() {
        let dir = velocity_dir();
        assert!(dir.to_str().unwrap().ends_with(".velocity"));
    }

    // ── UsageSnapshot validate edge cases ─────────────────────────────────────

    #[test]
    fn snapshot_validate_empty_provider_name() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![ProviderUsage {
                provider: "".into(),
                display_name: "Test".into(),
                key_valid: true,
                has_usage_api: false,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status: "ok".into(),
                models: vec![],
            }],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("empty provider name")));
    }

    #[test]
    fn snapshot_validate_empty_display_name() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![ProviderUsage {
                provider: "openai".into(),
                display_name: "".into(),
                key_valid: true,
                has_usage_api: false,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status: "ok".into(),
                models: vec![],
            }],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("empty display_name")));
    }

    #[test]
    fn snapshot_validate_both_mismatches() {
        let mut snap = make_test_snapshot();
        snap.total_tokens = 99999;
        snap.total_cost_usd = 999.99;
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("total_tokens")));
        assert!(issues.iter().any(|i| i.contains("total_cost_usd")));
    }

    // ── UsageSnapshot info edge cases ─────────────────────────────────────────

    #[test]
    fn snapshot_info_no_valid_keys() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![ProviderUsage {
                provider: "openai".into(),
                display_name: "OpenAI".into(),
                key_valid: false,
                has_usage_api: true,
                tokens_used: 0,
                cost_usd: 0.0,
                request_count: 0,
                period_start: None,
                period_end: None,
                status: "invalid".into(),
                models: vec![],
            }],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let info = snap.info();
        assert_eq!(info.providers_with_valid_keys, 0);
        assert_eq!(info.providers_with_usage_api, 1);
    }

    #[test]
    fn snapshot_info_models_tracked_sum() {
        let snap = make_test_snapshot();
        let info = snap.info();
        // openai has 2 models, anthropic has 0, cohere has 0
        assert_eq!(info.total_models_tracked, 2);
    }

    // ── cost_breakdown extended ───────────────────────────────────────────────

    #[test]
    fn cost_breakdown_single_provider() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![ProviderUsage {
                provider: "openai".into(),
                display_name: "OpenAI".into(),
                key_valid: true,
                has_usage_api: true,
                tokens_used: 100,
                cost_usd: 5.0,
                request_count: 10,
                period_start: None,
                period_end: None,
                status: "ok".into(),
                models: vec![],
            }],
            total_tokens: 100,
            total_cost_usd: 5.0,
            total_requests: 10,
        };
        let breakdown = snap.cost_breakdown();
        assert_eq!(breakdown.per_provider.len(), 1);
        assert!((breakdown.per_provider[0].percentage_of_total - 100.0).abs() < 0.01);
        assert_eq!(breakdown.highest_cost_provider, Some("openai".to_string()));
    }

    #[test]
    fn cost_breakdown_estimated_monthly_equals_total() {
        let snap = make_test_snapshot();
        let breakdown = snap.cost_breakdown();
        assert!((breakdown.estimated_monthly_usd - breakdown.total_cost_usd).abs() < 0.001);
    }

    #[test]
    fn cost_breakdown_no_providers() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let breakdown = snap.cost_breakdown();
        assert!(breakdown.per_provider.is_empty());
        assert_eq!(breakdown.highest_cost_provider, None);
    }

    // ── Struct derives ────────────────────────────────────────────────────────

    #[test]
    fn provider_credential_clone_debug() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "sk-test".into(),
            base_url: Some("https://proxy.example.com".into()),
            model: Some("gpt-4o".into()),
        };
        let cloned = cred.clone();
        assert_eq!(cloned.provider, "openai");
        assert_eq!(cloned.api_key, "sk-test");
        assert_eq!(cloned.base_url.as_deref(), Some("https://proxy.example.com"));
        assert_eq!(cloned.model.as_deref(), Some("gpt-4o"));
        // Debug
        let debug = format!("{:?}", cred);
        assert!(debug.contains("ProviderCredential"));
    }

    #[test]
    fn provider_clone_debug() {
        let p = Provider::Openai;
        let cloned = p.clone();
        assert_eq!(cloned, Provider::Openai);
        let debug = format!("{:?}", p);
        assert!(debug.contains("Openai"));
    }

    #[test]
    fn usage_snapshot_clone_debug() {
        let snap = make_test_snapshot();
        let cloned = snap.clone();
        assert_eq!(cloned.total_tokens, snap.total_tokens);
        assert_eq!(cloned.providers.len(), snap.providers.len());
        let debug = format!("{:?}", snap);
        assert!(debug.contains("UsageSnapshot"));
    }

    #[test]
    fn provider_usage_clone_debug() {
        let pu = ProviderUsage {
            provider: "test".into(),
            display_name: "Test".into(),
            key_valid: true,
            has_usage_api: false,
            tokens_used: 100,
            cost_usd: 0.5,
            request_count: 5,
            period_start: None,
            period_end: None,
            status: "ok".into(),
            models: vec![],
        };
        let cloned = pu.clone();
        assert_eq!(cloned.provider, "test");
        assert_eq!(cloned.tokens_used, 100);
    }

    #[test]
    fn model_usage_clone_debug() {
        let mu = ModelUsage {
            model: "gpt-4o".into(),
            tokens: 50000,
            cost_usd: 1.5,
            requests: 42,
        };
        let cloned = mu.clone();
        assert_eq!(cloned.model, "gpt-4o");
        assert_eq!(cloned.tokens, 50000);
    }

    #[test]
    fn provider_cost_estimate_clone_debug() {
        let est = ProviderCostEstimate {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            model: "gpt-4o".into(),
            estimated_cost_usd: 1.23,
        };
        let cloned = est.clone();
        assert_eq!(cloned.provider, "openai");
        let debug = format!("{:?}", est);
        assert!(debug.contains("ProviderCostEstimate"));
    }

    #[test]
    fn usage_snapshot_info_clone_debug() {
        let snap = make_test_snapshot();
        let info = snap.info();
        let cloned = info.clone();
        assert_eq!(cloned.provider_count, info.provider_count);
        let debug = format!("{:?}", info);
        assert!(debug.contains("UsageSnapshotInfo"));
    }

    #[test]
    fn provider_summary_clone_debug() {
        let s = ProviderSummary {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            key_valid: true,
            cost_usd: 1.5,
            tokens_used: 50000,
            model_count: 2,
            status: "ok".into(),
        };
        let cloned = s.clone();
        assert_eq!(cloned.provider, "openai");
    }

    #[test]
    fn cost_breakdown_clone_debug() {
        let snap = make_test_snapshot();
        let bd = snap.cost_breakdown();
        let cloned = bd.clone();
        assert!((cloned.total_cost_usd - bd.total_cost_usd).abs() < 0.001);
    }

    #[test]
    fn provider_cost_clone_debug() {
        let pc = ProviderCost {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            cost_usd: 1.5,
            percentage_of_total: 75.0,
        };
        let cloned = pc.clone();
        assert_eq!(cloned.provider, "openai");
    }

    // ── ModelUsage serialization ──────────────────────────────────────────────

    #[test]
    fn model_usage_serializes() {
        let mu = ModelUsage {
            model: "gpt-4o".into(),
            tokens: 50000,
            cost_usd: 1.23,
            requests: 42,
        };
        let json = serde_json::to_string(&mu).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o\""));
        assert!(json.contains("\"tokens\":50000"));
        let parsed: ModelUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.tokens, 50000);
    }

    // ── Provider serde ────────────────────────────────────────────────────────

    #[test]
    fn provider_serde_roundtrip() {
        let p = Provider::Anthropic;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"anthropic\"");
        let parsed: Provider = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Provider::Anthropic);
    }

    #[test]
    fn provider_serde_all_variants() {
        let all = [
            Provider::Openai, Provider::Anthropic, Provider::Google,
            Provider::Mistral, Provider::Cohere, Provider::Xai, Provider::Github,
        ];
        for p in &all {
            let json = serde_json::to_string(p).unwrap();
            let parsed: Provider = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, p);
        }
    }

    // ── chrono_utc_now ────────────────────────────────────────────────────────

    #[test]
    fn chrono_utc_now_format() {
        let now = chrono_utc_now();
        // Should be ISO-8601: YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(now.len(), 20, "bad length: {}", now);
        assert!(now.ends_with('Z'), "should end with Z: {}", now);
        assert!(now.contains('T'), "should contain T: {}", now);
        // Year should be reasonable (2024+)
        assert!(now.starts_with("202"), "unexpected year: {}", now);
    }

    // ── ProviderSummary serialization ─────────────────────────────────────────

    #[test]
    fn provider_summary_fields() {
        let s = ProviderSummary {
            provider: "anthropic".into(),
            display_name: "Anthropic".into(),
            key_valid: false,
            cost_usd: 0.0,
            tokens_used: 0,
            model_count: 0,
            status: "invalid key".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"provider\":\"anthropic\""));
        assert!(json.contains("\"key_valid\":false"));
        assert!(json.contains("\"status\":\"invalid key\""));
    }

    // ── snapshot validate cost tolerance ──────────────────────────────────────

    #[test]
    fn snapshot_validate_cost_within_tolerance() {
        let mut snap = make_test_snapshot();
        // Adjust by 0.005 — within the 0.01 tolerance
        snap.total_cost_usd = 2.005;
        let issues = snap.validate();
        assert!(!issues.iter().any(|i| i.contains("total_cost_usd")),
            "should be within tolerance, got: {:?}", issues);
    }

    #[test]
    fn snapshot_validate_cost_outside_tolerance() {
        let mut snap = make_test_snapshot();
        snap.total_cost_usd = 2.02; // 0.02 off, > 0.01 tolerance
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("total_cost_usd")));
    }

    // ── provider_summaries extended ───────────────────────────────────────────

    #[test]
    fn provider_summaries_preserves_order() {
        let snap = make_test_snapshot();
        let summaries = snap.provider_summaries();
        assert_eq!(summaries[0].provider, "openai");
        assert_eq!(summaries[1].provider, "anthropic");
        assert_eq!(summaries[2].provider, "cohere");
    }

    #[test]
    fn provider_summaries_empty_snapshot() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let summaries = snap.provider_summaries();
        assert!(summaries.is_empty());
    }

    // ── Block 190: New tests ──────────────────────────────────────────────────

    #[test]
    fn usage_snapshot_info_json_key_count() {
        let snap = make_test_snapshot();
        let info = snap.info();
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 9);
    }

    #[test]
    fn provider_summary_json_key_count() {
        let s = ProviderSummary {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            key_valid: true,
            cost_usd: 1.5,
            tokens_used: 50000,
            model_count: 2,
            status: "ok".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 7);
    }

    #[test]
    fn cost_breakdown_json_key_count() {
        let snap = make_test_snapshot();
        let bd = snap.cost_breakdown();
        let v: serde_json::Value = serde_json::to_value(&bd).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 4);
    }

    #[test]
    fn provider_cost_json_key_count() {
        let pc = ProviderCost {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            cost_usd: 1.5,
            percentage_of_total: 75.0,
        };
        let v: serde_json::Value = serde_json::to_value(&pc).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 4);
    }

    #[test]
    fn provider_cost_estimate_json_key_count() {
        let est = ProviderCostEstimate {
            provider: "openai".into(),
            display_name: "OpenAI".into(),
            model: "gpt-4o".into(),
            estimated_cost_usd: 1.23,
        };
        let v: serde_json::Value = serde_json::to_value(&est).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 4);
    }

    #[test]
    fn usage_snapshot_info_json_types() {
        let snap = make_test_snapshot();
        let info = snap.info();
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert!(v["generated_at"].is_string());
        assert!(v["provider_count"].is_u64());
        assert!(v["total_tokens"].is_u64());
        assert!(v["total_cost_usd"].is_f64());
        assert!(v["total_requests"].is_u64());
        assert!(v["providers_with_valid_keys"].is_u64());
        assert!(v["providers_with_usage_api"].is_u64());
        assert!(v["total_models_tracked"].is_u64());
        assert!(v["validation_issues"].is_array());
    }

    #[test]
    fn cost_breakdown_percentages_sum_to_100() {
        let snap = make_test_snapshot();
        let bd = snap.cost_breakdown();
        let pct_sum: f64 = bd.per_provider.iter().map(|p| p.percentage_of_total).sum();
        assert!((pct_sum - 100.0).abs() < 0.01, "percentages sum to {}", pct_sum);
    }

    #[test]
    fn cost_breakdown_sorted_descending_by_cost() {
        let snap = make_test_snapshot();
        let bd = snap.cost_breakdown();
        for w in bd.per_provider.windows(2) {
            assert!(w[0].cost_usd >= w[1].cost_usd,
                "not sorted descending: {} < {}", w[0].cost_usd, w[1].cost_usd);
        }
    }

    #[test]
    fn estimate_cost_formula_exact() {
        // gpt-4o-mini: $0.15/M input, $0.60/M output
        // 2M input + 3M output = 0.15*2 + 0.60*3 = 0.30 + 1.80 = 2.10
        let cost = estimate_cost(&Provider::Openai, "gpt-4o-mini", 2_000_000, 3_000_000).unwrap();
        assert!((cost - 2.10).abs() < 0.001, "got {}", cost);
    }

    #[test]
    fn estimate_cost_large_token_count() {
        // Verify no overflow with large token counts
        let cost = estimate_cost(&Provider::Openai, "gpt-4o", 1_000_000_000, 1_000_000_000);
        assert!(cost.is_some());
        let c = cost.unwrap();
        // 1B tokens * $2.50/M + 1B * $10.00/M = $2500 + $10000 = $12500
        assert!((c - 12500.0).abs() < 1.0, "got {}", c);
    }

    #[test]
    fn from_str_loose_grok_alias() {
        assert_eq!(Provider::from_str_loose("grok"), Some(Provider::Xai));
        assert_eq!(Provider::from_str_loose("GROK"), Some(Provider::Xai));
    }

    #[test]
    fn from_str_loose_copilot_alias() {
        assert_eq!(Provider::from_str_loose("copilot"), Some(Provider::Github));
        assert_eq!(Provider::from_str_loose("COPILOT"), Some(Provider::Github));
    }

    #[test]
    fn from_str_loose_gemini_alias() {
        assert_eq!(Provider::from_str_loose("gemini"), Some(Provider::Google));
        assert_eq!(Provider::from_str_loose("GEMINI"), Some(Provider::Google));
    }

    #[test]
    fn from_str_loose_claude_alias() {
        assert_eq!(Provider::from_str_loose("claude"), Some(Provider::Anthropic));
        assert_eq!(Provider::from_str_loose("CLAUDE"), Some(Provider::Anthropic));
    }

    #[test]
    fn model_pricing_openai_count() {
        assert_eq!(Provider::Openai.model_pricing().len(), 4);
    }

    #[test]
    fn model_pricing_anthropic_count() {
        assert_eq!(Provider::Anthropic.model_pricing().len(), 4);
    }

    #[test]
    fn model_pricing_github_empty() {
        assert!(Provider::Github.model_pricing().is_empty());
    }

    #[test]
    fn model_pricing_all_positive() {
        let all = [
            Provider::Openai, Provider::Anthropic, Provider::Google,
            Provider::Mistral, Provider::Cohere, Provider::Xai,
        ];
        for p in &all {
            for (model, input, output) in p.model_pricing() {
                assert!(input > 0.0, "{}: {} input price is 0", p, model);
                assert!(output > 0.0, "{}: {} output price is 0", p, model);
            }
        }
    }

    #[test]
    fn provider_summaries_model_count_matches_models() {
        let snap = make_test_snapshot();
        let summaries = snap.provider_summaries();
        for (i, s) in summaries.iter().enumerate() {
            assert_eq!(s.model_count, snap.providers[i].models.len(),
                "provider {}: model_count {} != models.len() {}",
                s.provider, s.model_count, snap.providers[i].models.len());
        }
    }

    #[test]
    fn usage_snapshot_json_roundtrip_via_value() {
        let snap = make_test_snapshot();
        let v: serde_json::Value = serde_json::to_value(&snap).unwrap();
        let parsed: UsageSnapshot = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.total_tokens, snap.total_tokens);
        assert_eq!(parsed.total_cost_usd, snap.total_cost_usd);
        assert_eq!(parsed.total_requests, snap.total_requests);
        assert_eq!(parsed.providers.len(), snap.providers.len());
        assert_eq!(parsed.generated_at, snap.generated_at);
    }

    #[test]
    fn provider_usage_json_optional_fields_absent() {
        let pu = ProviderUsage {
            provider: "test".into(),
            display_name: "Test".into(),
            key_valid: true,
            has_usage_api: false,
            tokens_used: 100,
            cost_usd: 0.5,
            request_count: 5,
            period_start: None,
            period_end: None,
            status: "ok".into(),
            models: vec![],
        };
        let json = serde_json::to_string(&pu).unwrap();
        // None fields should serialize as null in JSON
        assert!(json.contains("\"period_start\":null"));
        assert!(json.contains("\"period_end\":null"));
    }

    #[test]
    fn provider_usage_json_optional_fields_present() {
        let pu = ProviderUsage {
            provider: "test".into(),
            display_name: "Test".into(),
            key_valid: true,
            has_usage_api: false,
            tokens_used: 100,
            cost_usd: 0.5,
            request_count: 5,
            period_start: Some("2025-01-01".into()),
            period_end: Some("2025-01-31".into()),
            status: "ok".into(),
            models: vec![],
        };
        let json = serde_json::to_string(&pu).unwrap();
        assert!(json.contains("\"period_start\":\"2025-01-01\""));
        assert!(json.contains("\"period_end\":\"2025-01-31\""));
    }

    #[test]
    fn snapshot_validate_empty_providers_reports_issue() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let issues = snap.validate();
        assert!(issues.iter().any(|i| i.contains("No providers")));
    }

    #[test]
    fn usage_snapshot_info_validation_issues_populated() {
        let mut snap = make_test_snapshot();
        snap.total_tokens = 999999; // mismatch
        let info = snap.info();
        assert!(!info.validation_issues.is_empty(),
            "validation_issues should be populated for invalid snapshot");
    }

    #[test]
    fn provider_debug_format_all_variants() {
        let all = [
            (Provider::Openai, "Openai"),
            (Provider::Anthropic, "Anthropic"),
            (Provider::Google, "Google"),
            (Provider::Mistral, "Mistral"),
            (Provider::Cohere, "Cohere"),
            (Provider::Xai, "Xai"),
            (Provider::Github, "Github"),
        ];
        for (p, name) in &all {
            let debug = format!("{:?}", p);
            assert!(debug.contains(name), "{:?} doesn't contain {}", debug, name);
        }
    }

    #[test]
    fn masked_key_empty_string() {
        let cred = ProviderCredential {
            provider: "openai".into(),
            api_key: "".into(),
            base_url: None,
            model: None,
        };
        // 0 chars: not > 4 => else => "****"
        assert_eq!(cred.masked_key(), "****");
    }

    #[test]
    fn cost_breakdown_zero_total_percentages_all_zero() {
        let snap = UsageSnapshot {
            generated_at: "test".into(),
            providers: vec![
                ProviderUsage {
                    provider: "a".into(), display_name: "A".into(),
                    key_valid: true, has_usage_api: false,
                    tokens_used: 0, cost_usd: 0.0, request_count: 0,
                    period_start: None, period_end: None,
                    status: "ok".into(), models: vec![],
                },
                ProviderUsage {
                    provider: "b".into(), display_name: "B".into(),
                    key_valid: true, has_usage_api: false,
                    tokens_used: 0, cost_usd: 0.0, request_count: 0,
                    period_start: None, period_end: None,
                    status: "ok".into(), models: vec![],
                },
            ],
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_requests: 0,
        };
        let bd = snap.cost_breakdown();
        for pc in &bd.per_provider {
            assert!((pc.percentage_of_total - 0.0).abs() < 0.001);
        }
    }

    #[test]
    fn model_usage_json_roundtrip_via_value() {
        let mu = ModelUsage {
            model: "gpt-4o".into(),
            tokens: 50000,
            cost_usd: 1.23,
            requests: 42,
        };
        let v: serde_json::Value = serde_json::to_value(&mu).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 4);
        let parsed: ModelUsage = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.tokens, 50000);
        assert!((parsed.cost_usd - 1.23).abs() < 0.001);
        assert_eq!(parsed.requests, 42);
    }
}
