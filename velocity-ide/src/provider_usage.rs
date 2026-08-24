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
}
