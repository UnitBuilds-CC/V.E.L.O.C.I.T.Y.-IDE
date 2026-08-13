use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DEFAULT_LIMITS: (&str, u32) = ("free", 50);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStats {
    pub label: String,
    pub tier: String,
    #[serde(default)]
    pub requests: u32,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub exhausted: bool,
    #[serde(default)]
    pub exhausted_at: Option<String>,
    #[serde(default = "default_limit_free")]
    pub daily_limit: u32,
}

fn default_limit_free() -> u32 {
    50
}

fn default_free_tier() -> String {
    "free".to_string()
}

fn default_paid_tier() -> String {
    "paid".to_string()
}

fn default_cloudflare_label() -> String {
    "default".to_string()
}

fn default_openrouter_label() -> String {
    "OR-Default".to_string()
}

fn default_azure_label() -> String {
    "Azure-Default".to_string()
}

fn default_azure_deployment() -> String {
    "gpt-4o".to_string()
}

fn default_azure_api_version() -> String {
    "2024-06-01".to_string()
}

fn default_ollama_host() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_model() -> String {
    "llama3.2".to_string()
}

fn default_ollama_label() -> String {
    "Local-Ollama".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct UsageFile {
    date: String,
    accounts: HashMap<String, AccountStats>,
}

#[derive(Debug, Clone)]
pub struct CloudflareAccount {
    pub n: u32,
    pub id: String,
    pub token: String,
    pub tier: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct OpenRouterAccount {
    pub n: u32,
    pub token: String,
    pub tier: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct AzureOpenAiAccount {
    #[allow(dead_code)]
    pub n: u32,
    pub api_key: String,
    pub endpoint: String,
    pub deployment: String,
    pub api_version: String,
    #[allow(dead_code)]
    pub tier: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct LocalOllamaAccount {
    pub host: String,
    pub default_model: String,
    #[allow(dead_code)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceCloudflareSettings {
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub api_token: String,
    #[serde(default = "default_free_tier")]
    pub tier: String,
    #[serde(default = "default_cloudflare_label")]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceOpenRouterSettings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_free_tier")]
    pub tier: String,
    #[serde(default = "default_openrouter_label")]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceAzureOpenAiSettings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_azure_deployment")]
    pub deployment: String,
    #[serde(default = "default_azure_api_version")]
    pub api_version: String,
    #[serde(default = "default_paid_tier")]
    pub tier: String,
    #[serde(default = "default_azure_label")]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceOllamaSettings {
    #[serde(default = "default_ollama_host")]
    pub host: String,
    #[serde(default = "default_ollama_model")]
    pub default_model: String,
    #[serde(default = "default_ollama_label")]
    pub label: String,
}

/// Simple API-key provider settings — used by Deepseek, Groq, Mistral, OpenAI,
/// Google, Together AI, Fireworks AI, Perplexity, Cerebras, Alibaba Qwen.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceApiKeySettings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub label: String,
}

impl WorkspaceApiKeySettings {
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceProviderSettings {
    #[serde(default)]
    pub cloudflare: WorkspaceCloudflareSettings,
    #[serde(default)]
    pub openrouter: WorkspaceOpenRouterSettings,
    #[serde(default)]
    pub azure_openai: WorkspaceAzureOpenAiSettings,
    #[serde(default)]
    pub ollama: WorkspaceOllamaSettings,
    #[serde(default)]
    pub deepseek: WorkspaceApiKeySettings,
    #[serde(default)]
    pub groq: WorkspaceApiKeySettings,
    #[serde(default)]
    pub mistral: WorkspaceApiKeySettings,
    #[serde(default)]
    pub openai: WorkspaceApiKeySettings,
    #[serde(default)]
    pub google: WorkspaceApiKeySettings,
    #[serde(default)]
    pub together: WorkspaceApiKeySettings,
    #[serde(default)]
    pub fireworks: WorkspaceApiKeySettings,
    #[serde(default)]
    pub perplexity: WorkspaceApiKeySettings,
    #[serde(default)]
    pub cerebras: WorkspaceApiKeySettings,
    #[serde(default)]
    pub alibaba: WorkspaceApiKeySettings,
    #[serde(default)]
    pub bedrock: WorkspaceApiKeySettings,
    #[serde(default)]
    pub anthropic: WorkspaceApiKeySettings,
}

impl WorkspaceCloudflareSettings {
    pub fn is_configured(&self) -> bool {
        !self.account_id.trim().is_empty() && !self.api_token.trim().is_empty()
    }
}

impl WorkspaceOpenRouterSettings {
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}

impl WorkspaceAzureOpenAiSettings {
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty() && !self.endpoint.trim().is_empty()
    }
}

impl WorkspaceOllamaSettings {
    pub fn is_configured(&self) -> bool {
        !self.host.trim().is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct AccountUsageView {
    pub n: u32,
    pub label: String,
    pub tier: String,
    pub requests: u32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub daily_limit: u32,
    pub remaining: u32,
    pub exhausted: bool,
}

pub struct UsageTracker {
    path: PathBuf,
    nda_path: PathBuf,
    legacy_path: PathBuf,
    data: UsageFile,
}

impl UsageTracker {
    pub fn new(workspace_root: &Path) -> Self {
        let memory = workspace_root.join("memory");
        let mut tracker = Self {
            path: memory.join(".account_usage.json"),
            nda_path: memory.join(".account_usage.nda"),
            legacy_path: memory.join(".account_state.json"),
            data: UsageFile {
                date: today_utc(),
                accounts: HashMap::new(),
            },
        };
        tracker.load();
        tracker
    }

    fn load(&mut self) {
        let today = today_utc();
        if self.nda_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&self.nda_path) {
                if let Some(parsed) = parse_usage_nda(&raw) {
                    self.data = normalize_usage_file_date(parsed, &today);
                    self.migrate_legacy();
                    return;
                }
            }
        }
        if self.path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&self.path) {
                if let Ok(parsed) = serde_json::from_str::<UsageFile>(&raw) {
                    self.data = normalize_usage_file_date(parsed, &today);
                    self.migrate_legacy();
                    return;
                }
            }
        }
        self.data.date = today;
        self.migrate_legacy();
    }

    fn migrate_legacy(&mut self) {
        if !self.legacy_path.exists() {
            return;
        }
        let Ok(raw) = std::fs::read_to_string(&self.legacy_path) else {
            return;
        };
        let Ok(legacy): Result<HashMap<String, serde_json::Value>, _> = serde_json::from_str(&raw)
        else {
            return;
        };
        let today = today_utc();
        for (key, entry) in legacy {
            if entry.get("date").and_then(|v| v.as_str()) != Some(&today) {
                continue;
            }
            let stats = self
                .data
                .accounts
                .entry(key)
                .or_insert_with(|| AccountStats {
                    label: String::new(),
                    tier: "free".into(),
                    requests: 0,
                    tokens_in: 0,
                    tokens_out: 0,
                    exhausted: false,
                    exhausted_at: None,
                    daily_limit: 50,
                });
            stats.exhausted = true;
            if stats.exhausted_at.is_none() {
                stats.exhausted_at = entry
                    .get("exhausted_at")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
        }
    }

    fn save(&self) {
        if let Some(parent) = self.nda_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.nda_path, serialize_usage_nda(&self.data));
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    fn ensure_account(&mut self, n: u32, label: &str, tier: &str) -> &mut AccountStats {
        let key = n.to_string();
        let daily_limit = std::env::var(format!("CF_ACCOUNT_{n}_DAILY_LIMIT"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                if tier == "paid" {
                    500
                } else {
                    DEFAULT_LIMITS.1
                }
            });

        self.data
            .accounts
            .entry(key)
            .or_insert_with(|| AccountStats {
                label: label.to_string(),
                tier: tier.to_string(),
                requests: 0,
                tokens_in: 0,
                tokens_out: 0,
                exhausted: false,
                exhausted_at: None,
                daily_limit,
            });
        let stats = self.data.accounts.get_mut(&n.to_string()).unwrap();
        if stats.label.is_empty() {
            stats.label = label.to_string();
        }
        if stats.tier.is_empty() {
            stats.tier = tier.to_string();
        }
        stats
    }

    pub fn is_exhausted(&self, n: u32) -> bool {
        self.data
            .accounts
            .get(&n.to_string())
            .map(|s| s.exhausted)
            .unwrap_or(false)
    }

    pub fn mark_exhausted(&mut self, n: u32, label: &str, tier: &str) {
        let exhausted_at = format!("{}Z", chrono_now_iso());
        {
            let stats = self.ensure_account(n, label, tier);
            stats.exhausted = true;
            stats.exhausted_at = Some(exhausted_at.clone());
        }
        self.save();

        // Sync legacy file
        let mut legacy: HashMap<String, serde_json::Value> = HashMap::new();
        if self.legacy_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&self.legacy_path) {
                if let Ok(parsed) = serde_json::from_str(&raw) {
                    legacy = parsed;
                }
            }
        }
        legacy.insert(
            n.to_string(),
            serde_json::json!({
                "date": today_utc(),
                "exhausted_at": exhausted_at,
            }),
        );
        if let Some(parent) = self.legacy_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&legacy) {
            let _ = std::fs::write(&self.legacy_path, json);
        }
    }

    pub fn record_request(
        &mut self,
        n: u32,
        label: &str,
        tier: &str,
        tokens_in: u64,
        tokens_out: u64,
    ) {
        {
            let stats = self.ensure_account(n, label, tier);
            stats.requests += 1;
            stats.tokens_in += tokens_in;
            stats.tokens_out += tokens_out;
        }
        self.save();
    }

    pub fn ensure_or_account(&mut self, n: u32, label: &str, tier: &str) -> &mut AccountStats {
        let key = format!("or_{n}");
        let daily_limit = std::env::var(format!("OPENROUTER_ACCOUNT_{n}_DAILY_LIMIT"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        self.data
            .accounts
            .entry(key.clone())
            .or_insert_with(|| AccountStats {
                label: label.to_string(),
                tier: tier.to_string(),
                requests: 0,
                tokens_in: 0,
                tokens_out: 0,
                exhausted: false,
                exhausted_at: None,
                daily_limit,
            });
        let stats = self.data.accounts.get_mut(&key).unwrap();
        if stats.label.is_empty() {
            stats.label = label.to_string();
        }
        if stats.tier.is_empty() {
            stats.tier = tier.to_string();
        }
        stats
    }

    pub fn is_or_exhausted(&self, n: u32) -> bool {
        self.data
            .accounts
            .get(&format!("or_{n}"))
            .map(|s| s.exhausted)
            .unwrap_or(false)
    }

    pub fn mark_or_exhausted(&mut self, n: u32, label: &str, tier: &str) {
        let exhausted_at = format!("{}Z", chrono_now_iso());
        {
            let stats = self.ensure_or_account(n, label, tier);
            stats.exhausted = true;
            stats.exhausted_at = Some(exhausted_at.clone());
        }
        self.save();
    }

    pub fn record_or_request(
        &mut self,
        n: u32,
        label: &str,
        tier: &str,
        tokens_in: u64,
        tokens_out: u64,
    ) {
        {
            let stats = self.ensure_or_account(n, label, tier);
            stats.requests += 1;
            stats.tokens_in += tokens_in;
            stats.tokens_out += tokens_out;
        }
        self.save();
    }

    pub fn build_views(
        &mut self,
        accounts: &[CloudflareAccount],
        or_accounts: &[OpenRouterAccount],
    ) -> Vec<AccountUsageView> {
        let mut views = Vec::new();
        for acct in accounts {
            let stats = self.ensure_account(acct.n, &acct.label, &acct.tier);
            let remaining = if stats.exhausted {
                0
            } else {
                stats.daily_limit.saturating_sub(stats.requests)
            };
            views.push(AccountUsageView {
                n: acct.n,
                label: stats.label.clone(),
                tier: stats.tier.clone(),
                requests: stats.requests,
                tokens_in: stats.tokens_in,
                tokens_out: stats.tokens_out,
                daily_limit: stats.daily_limit,
                remaining,
                exhausted: stats.exhausted,
            });
        }
        for acct in or_accounts {
            let stats = self.ensure_or_account(acct.n, &acct.label, &acct.tier);
            let remaining = if stats.exhausted {
                0
            } else {
                stats.daily_limit.saturating_sub(stats.requests)
            };
            views.push(AccountUsageView {
                n: acct.n,
                label: stats.label.clone(),
                tier: stats.tier.clone(),
                requests: stats.requests,
                tokens_in: stats.tokens_in,
                tokens_out: stats.tokens_out,
                daily_limit: stats.daily_limit,
                remaining,
                exhausted: stats.exhausted,
            });
        }
        self.save();
        views
    }

    pub fn current_date(&self) -> String {
        self.data.date.clone()
    }

    pub fn pick_account<'a>(
        &self,
        accounts: &'a [CloudflareAccount],
    ) -> Option<&'a CloudflareAccount> {
        let available: Vec<&CloudflareAccount> = accounts
            .iter()
            .filter(|a| !self.is_exhausted(a.n))
            .collect();
        if available.is_empty() {
            return None;
        }
        let idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as usize)
            .unwrap_or(0)
            % available.len();
        Some(available[idx])
    }

    pub fn pick_or_account<'a>(
        &self,
        accounts: &'a [OpenRouterAccount],
    ) -> Option<&'a OpenRouterAccount> {
        let available: Vec<&OpenRouterAccount> = accounts
            .iter()
            .filter(|a| !self.is_or_exhausted(a.n))
            .collect();
        if available.is_empty() {
            return None;
        }
        let idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as usize)
            .unwrap_or(0)
            % available.len();
        Some(available[idx])
    }
}

pub fn provider_settings_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("provider-settings.json")
}

pub fn load_workspace_provider_settings(workspace_root: &Path) -> WorkspaceProviderSettings {
    let path = provider_settings_path(workspace_root);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<WorkspaceProviderSettings>(&raw).ok())
        .unwrap_or_default()
}

pub fn save_workspace_provider_settings(
    workspace_root: &Path,
    settings: &WorkspaceProviderSettings,
) -> Result<(), String> {
    let path = provider_settings_path(workspace_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create provider settings folder: {err}"))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Failed to serialize provider settings: {err}"))?;
    std::fs::write(path, json).map_err(|err| format!("Failed to save provider settings: {err}"))
}

pub fn load_accounts(workspace_root: &Path) -> Vec<CloudflareAccount> {
    let workspace_settings = load_workspace_provider_settings(workspace_root);
    if workspace_settings.cloudflare.is_configured() {
        return vec![CloudflareAccount {
            n: 1,
            id: workspace_settings.cloudflare.account_id,
            token: workspace_settings.cloudflare.api_token,
            tier: workspace_settings.cloudflare.tier.to_lowercase(),
            label: workspace_settings.cloudflare.label,
        }];
    }
    load_accounts_from_env()
}

pub fn load_openrouter_accounts(workspace_root: &Path) -> Vec<OpenRouterAccount> {
    let workspace_settings = load_workspace_provider_settings(workspace_root);
    if workspace_settings.openrouter.is_configured() {
        return vec![OpenRouterAccount {
            n: 1,
            token: workspace_settings.openrouter.api_key,
            tier: workspace_settings.openrouter.tier.to_lowercase(),
            label: workspace_settings.openrouter.label,
        }];
    }
    load_openrouter_accounts_from_env()
}

pub fn load_azure_accounts(workspace_root: &Path) -> Vec<AzureOpenAiAccount> {
    let workspace_settings = load_workspace_provider_settings(workspace_root);
    if workspace_settings.azure_openai.is_configured() {
        return vec![AzureOpenAiAccount {
            n: 1,
            api_key: workspace_settings.azure_openai.api_key,
            endpoint: workspace_settings.azure_openai.endpoint,
            deployment: workspace_settings.azure_openai.deployment,
            api_version: workspace_settings.azure_openai.api_version,
            tier: workspace_settings.azure_openai.tier.to_lowercase(),
            label: workspace_settings.azure_openai.label,
        }];
    }
    load_azure_accounts_from_env()
}

pub fn load_local_ollama_accounts(workspace_root: &Path) -> Vec<LocalOllamaAccount> {
    let workspace_settings = load_workspace_provider_settings(workspace_root);
    if workspace_settings.ollama.is_configured() {
        return vec![LocalOllamaAccount {
            host: workspace_settings.ollama.host,
            default_model: workspace_settings.ollama.default_model,
            label: workspace_settings.ollama.label,
        }];
    }
    load_local_ollama_accounts_from_env()
}

pub fn load_accounts_from_env() -> Vec<CloudflareAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30u32 {
        let id_key = format!("CF_ACCOUNT_{i}_ID");
        let token_key = format!("CF_ACCOUNT_{i}_TOKEN");
        if let (Ok(id), Ok(token)) = (std::env::var(&id_key), std::env::var(&token_key)) {
            let tier =
                std::env::var(format!("CF_ACCOUNT_{i}_TIER")).unwrap_or_else(|_| "free".into());
            let label = std::env::var(format!("CF_ACCOUNT_{i}_LABEL"))
                .unwrap_or_else(|_| format!("account-{i}"));
            accounts.push(CloudflareAccount {
                n: i,
                id,
                token,
                tier: tier.to_lowercase(),
                label,
            });
        }
    }
    if accounts.is_empty() {
        if let (Ok(id), Ok(token)) = (
            std::env::var("CF_ACCOUNT_ID"),
            std::env::var("CF_API_TOKEN"),
        ) {
            accounts.push(CloudflareAccount {
                n: 1,
                id,
                token,
                tier: "free".into(),
                label: "default".into(),
            });
        }
    }
    accounts.sort_by(|a, b| {
        let tier_ord = |t: &str| if t == "free" { 0 } else { 1 };
        (tier_ord(&a.tier), a.n).cmp(&(tier_ord(&b.tier), b.n))
    });
    accounts
}

pub fn load_openrouter_accounts_from_env() -> Vec<OpenRouterAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30u32 {
        let key_var = format!("OPENROUTER_ACCOUNT_{i}_KEY");
        if let Ok(key) = std::env::var(&key_var) {
            let label = std::env::var(format!("OPENROUTER_ACCOUNT_{i}_LABEL"))
                .unwrap_or_else(|_| format!("OR-Account-{i}"));
            let tier = std::env::var(format!("OPENROUTER_ACCOUNT_{i}_TIER"))
                .unwrap_or_else(|_| "free".to_string());
            accounts.push(OpenRouterAccount {
                n: i,
                token: key,
                tier: tier.to_lowercase(),
                label,
            });
        }
    }
    if accounts.is_empty() {
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            accounts.push(OpenRouterAccount {
                n: 1,
                token: key,
                tier: "free".to_string(),
                label: "OR-Default".to_string(),
            });
        }
    }
    accounts.sort_by(|a, b| {
        let tier_ord = |t: &str| if t == "free" { 0 } else { 1 };
        (tier_ord(&a.tier), a.n).cmp(&(tier_ord(&b.tier), b.n))
    });
    accounts
}

pub fn load_azure_accounts_from_env() -> Vec<AzureOpenAiAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30u32 {
        let key_var = format!("AZURE_OPENAI_ACCOUNT_{i}_KEY");
        let endpoint_var = format!("AZURE_OPENAI_ACCOUNT_{i}_ENDPOINT");
        let deployment_var = format!("AZURE_OPENAI_ACCOUNT_{i}_DEPLOYMENT");
        if let (Ok(key), Ok(endpoint)) = (std::env::var(&key_var), std::env::var(&endpoint_var)) {
            let deployment =
                std::env::var(&deployment_var).unwrap_or_else(|_| "gpt-4o".to_string());
            let api_version = std::env::var(format!("AZURE_OPENAI_ACCOUNT_{i}_API_VERSION"))
                .unwrap_or_else(|_| "2024-06-01".to_string());
            let label = std::env::var(format!("AZURE_OPENAI_ACCOUNT_{i}_LABEL"))
                .unwrap_or_else(|_| format!("Azure-Account-{i}"));
            let tier = std::env::var(format!("AZURE_OPENAI_ACCOUNT_{i}_TIER"))
                .unwrap_or_else(|_| "paid".to_string());
            accounts.push(AzureOpenAiAccount {
                n: i,
                api_key: key,
                endpoint,
                deployment,
                api_version,
                tier: tier.to_lowercase(),
                label,
            });
        }
    }
    if accounts.is_empty() {
        if let (Ok(key), Ok(endpoint)) = (
            std::env::var("AZURE_OPENAI_API_KEY"),
            std::env::var("AZURE_OPENAI_ENDPOINT"),
        ) {
            let deployment =
                std::env::var("AZURE_OPENAI_DEPLOYMENT").unwrap_or_else(|_| "gpt-4o".to_string());
            let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
                .unwrap_or_else(|_| "2024-06-01".to_string());
            accounts.push(AzureOpenAiAccount {
                n: 1,
                api_key: key,
                endpoint,
                deployment,
                api_version,
                tier: "paid".to_string(),
                label: "Azure-Default".to_string(),
            });
        }
    }
    accounts
}

pub fn load_local_ollama_accounts_from_env() -> Vec<LocalOllamaAccount> {
    dotenvy::dotenv().ok();
    let host =
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let default_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string());
    vec![LocalOllamaAccount {
        host,
        default_model,
        label: "Local-Ollama".to_string(),
    }]
}

fn serialize_usage_nda(data: &UsageFile) -> String {
    let mut lines = vec![
        "account-usage version 2".to_string(),
        format!("date {}", data.date),
        format!("account_count {}", data.accounts.len()),
    ];
    let mut keys: Vec<&String> = data.accounts.keys().collect();
    keys.sort();
    for key in keys {
        if let Some(stats) = data.accounts.get(key) {
            lines.push(format!("account\t{}", encode_nda_text(key)));
            lines.push(format!(
                "field\t{}\tlabel\t{}",
                encode_nda_text(key),
                encode_nda_text(&stats.label)
            ));
            lines.push(format!(
                "field\t{}\ttier\t{}",
                encode_nda_text(key),
                encode_nda_text(&stats.tier)
            ));
            lines.push(format!(
                "field\t{}\trequests\t{}",
                encode_nda_text(key),
                stats.requests
            ));
            lines.push(format!(
                "field\t{}\ttokens_in\t{}",
                encode_nda_text(key),
                stats.tokens_in
            ));
            lines.push(format!(
                "field\t{}\ttokens_out\t{}",
                encode_nda_text(key),
                stats.tokens_out
            ));
            lines.push(format!(
                "field\t{}\texhausted\t{}",
                encode_nda_text(key),
                stats.exhausted
            ));
            lines.push(format!(
                "field\t{}\texhausted_at\t{}",
                encode_nda_text(key),
                encode_optional_nda_text(stats.exhausted_at.as_deref())
            ));
            lines.push(format!(
                "field\t{}\tdaily_limit\t{}",
                encode_nda_text(key),
                stats.daily_limit
            ));
        }
    }
    lines.join("\n") + "\n"
}

fn parse_usage_nda(raw: &str) -> Option<UsageFile> {
    let mut date = None;
    let mut accounts = HashMap::new();
    let mut version = 1;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "account-usage version 1" {
            version = 1;
            continue;
        }
        if line == "account-usage version 2" {
            version = 2;
            continue;
        }
        if let Some(value) = line.strip_prefix("date ") {
            date = Some(value.to_string());
            continue;
        }
        if version == 2 {
            if line.starts_with("account\t") {
                continue;
            }
            if line.starts_with("account_count ") {
                continue;
            }
            if let Some(rest) = line.strip_prefix("field\t") {
                let parts: Vec<&str> = rest.split('\t').collect();
                if parts.len() != 3 {
                    return None;
                }
                let key = decode_nda_text(parts[0]);
                let field = parts[1];
                let value = parts[2];
                let stats = accounts.entry(key).or_insert_with(|| AccountStats {
                    label: String::new(),
                    tier: String::new(),
                    requests: 0,
                    tokens_in: 0,
                    tokens_out: 0,
                    exhausted: false,
                    exhausted_at: None,
                    daily_limit: default_limit_free(),
                });
                match field {
                    "label" => stats.label = decode_nda_text(value),
                    "tier" => stats.tier = decode_nda_text(value),
                    "requests" => stats.requests = value.parse().ok()?,
                    "tokens_in" => stats.tokens_in = value.parse().ok()?,
                    "tokens_out" => stats.tokens_out = value.parse().ok()?,
                    "exhausted" => stats.exhausted = value.parse().ok()?,
                    "exhausted_at" => stats.exhausted_at = decode_optional_nda_text(value),
                    "daily_limit" => stats.daily_limit = value.parse().ok()?,
                    _ => {}
                }
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("account\t") {
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 9 {
                return None;
            }
            accounts.insert(
                parts[0].to_string(),
                AccountStats {
                    label: decode_nda_text(parts[1]),
                    tier: decode_nda_text(parts[2]),
                    requests: parts[3].parse().ok()?,
                    tokens_in: parts[4].parse().ok()?,
                    tokens_out: parts[5].parse().ok()?,
                    exhausted: parts[6].parse().ok()?,
                    exhausted_at: decode_optional_nda_text(parts[7]),
                    daily_limit: parts[8].parse().ok()?,
                },
            );
        }
    }
    Some(UsageFile {
        date: date?,
        accounts,
    })
}

fn normalize_usage_file_date(parsed: UsageFile, today: &str) -> UsageFile {
    if parsed.date == today {
        parsed
    } else {
        UsageFile {
            date: today.to_string(),
            accounts: HashMap::new(),
        }
    }
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn decode_nda_text(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn encode_optional_nda_text(value: Option<&str>) -> String {
    value
        .map(encode_nda_text)
        .unwrap_or_else(|| "-".to_string())
}

fn decode_optional_nda_text(value: &str) -> Option<String> {
    if value == "-" {
        None
    } else {
        Some(decode_nda_text(value))
    }
}

fn today_utc() -> String {
    // Simple UTC date without chrono dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Days since epoch → approximate UTC date
    let days = secs / 86400;
    // 1970-01-01 + days (good enough; re-syncs daily)
    epoch_days_to_date(days)
}

fn epoch_days_to_date(days: u64) -> String {
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

// Algorithm from Howard Hinnant
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mp < 10 { y } else { y + 1 };
    (y, m, d)
}

fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let tod = secs % 86400;
    let h = tod / 3600;
    let min = (tod % 3600) / 60;
    let s = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nda_usage_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tracker = UsageTracker::new(tmp.path());
        tracker.record_request(2, "primary account", "paid", 11, 22);
        tracker.mark_or_exhausted(3, "OR account", "free");

        let nda =
            std::fs::read_to_string(tmp.path().join("memory").join(".account_usage.nda")).unwrap();
        let json =
            std::fs::read_to_string(tmp.path().join("memory").join(".account_usage.json")).unwrap();

        assert!(nda.starts_with("account-usage version 2\n"));
        assert!(nda.contains("account_count 2"));
        assert!(nda.contains("account\t2"));
        assert!(nda.contains("field\t2\tlabel\tprimary account"));
        assert!(nda.contains("field\t2\ttier\tpaid"));
        assert!(nda.contains("field\t2\trequests\t1"));
        assert!(nda.contains("field\tor_3\texhausted\ttrue"));
        assert!(json.contains("\"label\": \"primary account\""));
    }

    #[test]
    fn reads_nda_usage_state_before_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("memory")).unwrap();
        let today = today_utc();
        std::fs::write(
            tmp.path().join("memory").join(".account_usage.nda"),
            format!(
                "account-usage version 2\ndate {}\naccount_count 1\naccount\t2\nfield\t2\tlabel\tnda label\nfield\t2\ttier\tfree\nfield\t2\trequests\t4\nfield\t2\ttokens_in\t9\nfield\t2\ttokens_out\t12\nfield\t2\texhausted\ttrue\nfield\t2\texhausted_at\t2026-07-19T12:00:00Z\nfield\t2\tdaily_limit\t50\n",
                today
            ),
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("memory").join(".account_usage.json"),
            format!(
                "{{\"date\":\"{}\",\"accounts\":{{\"2\":{{\"label\":\"json label\",\"tier\":\"free\",\"requests\":1,\"tokens_in\":1,\"tokens_out\":1,\"exhausted\":false,\"exhausted_at\":null,\"daily_limit\":50}}}}}}",
                today
            ),
        )
        .unwrap();

        let mut tracker = UsageTracker::new(tmp.path());
        let views = tracker.build_views(
            &[CloudflareAccount {
                n: 2,
                id: "id".to_string(),
                token: "token".to_string(),
                tier: "free".to_string(),
                label: "fallback".to_string(),
            }],
            &[],
        );

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].label, "nda label");
        assert_eq!(views[0].requests, 4);
        assert!(views[0].exhausted);
    }

    #[test]
    fn reads_legacy_v1_usage_state_nda() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("memory")).unwrap();
        let today = today_utc();
        std::fs::write(
            tmp.path().join("memory").join(".account_usage.nda"),
            format!(
                "account-usage version 1\ndate {}\naccount\t2\tlegacy label\tfree\t4\t9\t12\ttrue\t2026-07-19T12:00:00Z\t50\n",
                today
            ),
        )
        .unwrap();

        let mut tracker = UsageTracker::new(tmp.path());
        let views = tracker.build_views(
            &[CloudflareAccount {
                n: 2,
                id: "id".to_string(),
                token: "token".to_string(),
                tier: "free".to_string(),
                label: "fallback".to_string(),
            }],
            &[],
        );

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].label, "legacy label");
        assert_eq!(views[0].requests, 4);
        assert!(views[0].exhausted);
    }
}
