//! Velocity Router HTTP client.
//!
//! Provides a typed interface to the Velocity Model Router API for usage
//! tracking, assignment submission, and cost monitoring.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Configuration for connecting to the Velocity Router.
#[derive(Debug, Clone)]
pub struct VelocityConfig {
    pub base_url: String,
    pub api_key: String,
}

impl VelocityConfig {
    /// Load from environment variables or config file.
    /// Checks VELOCITY_BASE_URL and VELOCITY_API_KEY env vars first,
    /// then falls back to ~/.velocity/config.toml.
    pub fn load() -> Result<Self> {
        // Try environment variables first.
        if let (Ok(url), Ok(key)) = (
            std::env::var("VELOCITY_BASE_URL"),
            std::env::var("VELOCITY_API_KEY"),
        ) {
            return Ok(Self {
                base_url: url,
                api_key: key,
            });
        }

        // Try ~/.velocity/config.toml
        if let Some(home) = dirs_next() {
            let config_path = home.join(".velocity").join("config.toml");
            if config_path.exists() {
                let content = std::fs::read_to_string(&config_path)
                    .context("failed to read ~/.velocity/config.toml")?;
                let mut base_url = None;
                let mut api_key = None;
                for line in content.lines() {
                    let line = line.trim();
                    if let Some(val) = line.strip_prefix("base_url") {
                        let val = val.trim().trim_start_matches('=').trim().trim_matches('"');
                        base_url = Some(val.to_string());
                    }
                    if let Some(val) = line.strip_prefix("api_key") {
                        let val = val.trim().trim_start_matches('=').trim().trim_matches('"');
                        api_key = Some(val.to_string());
                    }
                }
                if let (Some(url), Some(key)) = (base_url, api_key) {
                    return Ok(Self {
                        base_url: url,
                        api_key: key,
                    });
                }
            }
        }

        anyhow::bail!(
            "Velocity Router not configured.\n\
             Set VELOCITY_BASE_URL and VELOCITY_API_KEY environment variables,\n\
             or create ~/.velocity/config.toml with:\n\
             \n\
             base_url = \"http://localhost:8787\"\n\
             api_key = \"vr_standard_your_key_here\""
        )
    }

    /// Save configuration to ~/.velocity/config.toml.
    pub fn save(&self) -> Result<()> {
        if let Some(home) = dirs_next() {
            let config_dir = home.join(".velocity");
            std::fs::create_dir_all(&config_dir)
                .context("failed to create ~/.velocity directory")?;
            let config_path = config_dir.join("config.toml");
            let content = format!(
                "base_url = \"{}\"\napi_key = \"{}\"\n",
                self.base_url, self.api_key
            );
            std::fs::write(&config_path, content)
                .context("failed to write ~/.velocity/config.toml")?;
            Ok(())
        } else {
            anyhow::bail!("cannot determine home directory")
        }
    }
}

/// Simple home directory lookup without adding a dependency.
fn dirs_next() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

// ─── API Response Types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UsageResponse {
    pub tier: String,
    pub tokens_used: u64,
    pub tokens_limit: u64,
    pub cost_usd: f64,
    pub cost_limit_usd: f64,
    pub assignments_count: u64,
    pub period: UsagePeriod,
}

#[derive(Debug, Deserialize)]
pub struct UsagePeriod {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize)]
pub struct UsageDetailed {
    pub label: String,
    pub tier: String,
    pub total_assignments: u64,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
    pub by_model: Vec<ModelUsage>,
    pub by_domain: Vec<DomainUsage>,
}

#[derive(Debug, Deserialize)]
pub struct ModelUsage {
    pub model_id: String,
    pub assignments: u64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct DomainUsage {
    pub domain: String,
    pub assignments: u64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct RateLimitResponse {
    pub key_label: String,
    pub tier: String,
    pub rate_limit: RateLimitInfo,
    pub tokens: QuotaInfo,
    pub cost: CostInfo,
    pub billing_period: BillingPeriodInfo,
}

#[derive(Debug, Deserialize)]
pub struct RateLimitInfo {
    pub max_requests_per_minute: u32,
    pub resets_in_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct QuotaInfo {
    pub used: u64,
    pub limit: u64,
    pub quota_pct: f64,
    pub projected_monthly: u64,
}

#[derive(Debug, Deserialize)]
pub struct CostInfo {
    pub used_usd: f64,
    pub limit_usd: f64,
    pub quota_pct: f64,
    pub projected_monthly_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct BillingPeriodInfo {
    pub start: String,
    pub resets_in_days: i64,
}

#[derive(Debug, Serialize)]
pub struct AssignmentRequest {
    pub task: String,
    pub tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub file_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct AssignmentResponse {
    pub id: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub assembled_output: Option<String>,
    pub cost: Option<AssignmentCost>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssignmentCost {
    pub total_tokens: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub models_available: usize,
}

// ─── Client ─────────────────────────────────────────────────────────────

/// Synchronous HTTP client for the Velocity Router API.
pub struct VelocityClient {
    config: VelocityConfig,
    agent: ureq::Agent,
}

impl VelocityClient {
    pub fn new(config: VelocityConfig) -> Self {
        Self {
            config,
            agent: ureq::Agent::new(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let config = VelocityConfig::load()?;
        Ok(Self::new(config))
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.config.api_key)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.base_url.trim_end_matches('/'), path)
    }

    /// GET /health — check if the router is reachable.
    pub fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .agent
            .get(&self.url("/health"))
            .call()
            .context("failed to reach velocity router")?;
        resp.into_json()
            .context("failed to parse health response")
    }

    /// GET /v1/usage — current usage summary.
    pub fn get_usage(&self) -> Result<UsageResponse> {
        let resp = self
            .agent
            .get(&self.url("/v1/usage"))
            .set("Authorization", &self.auth_header())
            .call()
            .context("failed to fetch usage")?;
        resp.into_json()
            .context("failed to parse usage response")
    }

    /// GET /v1/usage/detailed — per-model and per-domain breakdown.
    pub fn get_usage_detailed(&self) -> Result<UsageDetailed> {
        let resp = self
            .agent
            .get(&self.url("/v1/usage/detailed"))
            .set("Authorization", &self.auth_header())
            .call()
            .context("failed to fetch detailed usage")?;
        resp.into_json()
            .context("failed to parse detailed usage response")
    }

    /// GET /v1/usage/rate-limit — rate limit and quota status.
    pub fn get_rate_limit(&self) -> Result<RateLimitResponse> {
        let resp = self
            .agent
            .get(&self.url("/v1/usage/rate-limit"))
            .set("Authorization", &self.auth_header())
            .call()
            .context("failed to fetch rate limit status")?;
        resp.into_json()
            .context("failed to parse rate limit response")
    }

    /// POST /v1/assignments — submit a task for orchestration.
    pub fn submit_assignment(&self, req: &AssignmentRequest) -> Result<AssignmentResponse> {
        let resp = self
            .agent
            .post(&self.url("/v1/assignments"))
            .set("Authorization", &self.auth_header())
            .set("Content-Type", "application/json")
            .send_json(serde_json::to_value(req)?)
            .context("failed to submit assignment")?;
        resp.into_json()
            .context("failed to parse assignment response")
    }

    /// GET /v1/assignments/:id — poll assignment status.
    pub fn get_assignment(&self, id: &str) -> Result<AssignmentResponse> {
        let resp = self
            .agent
            .get(&self.url(&format!("/v1/assignments/{}", id)))
            .set("Authorization", &self.auth_header())
            .call()
            .context("failed to fetch assignment")?;
        resp.into_json()
            .context("failed to parse assignment response")
    }
}

// ─── Formatting Helpers ─────────────────────────────────────────────────

/// Format a number with thousands separators.
pub fn fmt_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format a USD amount.
pub fn fmt_currency(n: f64) -> String {
    if n == 0.0 {
        "$0.00".to_string()
    } else if n < 0.01 {
        format!("${:.4}", n)
    } else {
        format!("${:.2}", n)
    }
}

/// Format a percentage.
pub fn fmt_percent(n: f64) -> String {
    format!("{:.1}%", n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_number_works() {
        assert_eq!(fmt_number(0), "0");
        assert_eq!(fmt_number(999), "999");
        assert_eq!(fmt_number(1000), "1,000");
        assert_eq!(fmt_number(1234567), "1,234,567");
    }

    #[test]
    fn fmt_currency_works() {
        assert_eq!(fmt_currency(0.0), "$0.00");
        assert_eq!(fmt_currency(0.0034), "$0.0034");
        assert_eq!(fmt_currency(1.5), "$1.50");
        assert_eq!(fmt_currency(123.456), "$123.46");
    }

    #[test]
    fn fmt_percent_works() {
        assert_eq!(fmt_percent(0.0), "0.0%");
        assert_eq!(fmt_percent(42.567), "42.6%");
        assert_eq!(fmt_percent(100.0), "100.0%");
    }

    #[test]
    fn config_load_from_env() {
        // Use unique prefixed env vars to avoid races with other tests.
        std::env::set_var("VELOCITY_BASE_URL", "http://test:8787");
        std::env::set_var("VELOCITY_API_KEY", "vr_test_key");
        let config = VelocityConfig::load().unwrap();
        assert_eq!(config.base_url, "http://test:8787");
        assert_eq!(config.api_key, "vr_test_key");
        // Clean up.
        std::env::remove_var("VELOCITY_BASE_URL");
        std::env::remove_var("VELOCITY_API_KEY");
        // After removing, load() should fail.
        assert!(VelocityConfig::load().is_err());
    }
}
