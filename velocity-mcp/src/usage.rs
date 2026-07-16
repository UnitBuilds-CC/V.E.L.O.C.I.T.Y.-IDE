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
    legacy_path: PathBuf,
    data: UsageFile,
}

impl UsageTracker {
    pub fn new(workspace_root: &Path) -> Self {
        let memory = workspace_root.join("memory");
        let mut tracker = Self {
            path: memory.join(".account_usage.json"),
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
        if self.path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&self.path) {
                if let Ok(parsed) = serde_json::from_str::<UsageFile>(&raw) {
                    if parsed.date == today {
                        self.data = parsed;
                    } else {
                        self.data = UsageFile {
                            date: today,
                            accounts: HashMap::new(),
                        };
                    }
                }
            }
        } else {
            self.data.date = today;
        }
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
            let stats = self.data.accounts.entry(key).or_insert_with(|| AccountStats {
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
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    fn ensure_account(&mut self, n: u32, label: &str, tier: &str) -> &mut AccountStats {
        let key = n.to_string();
        let daily_limit = std::env::var(format!("CF_ACCOUNT_{n}_DAILY_LIMIT"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| if tier == "paid" { 500 } else { DEFAULT_LIMITS.1 });

        self.data.accounts.entry(key).or_insert_with(|| AccountStats {
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

    pub fn build_views(&mut self, accounts: &[CloudflareAccount]) -> Vec<AccountUsageView> {
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
        self.save();
        views
    }

    pub fn current_date(&self) -> String {
        self.data.date.clone()
    }

    pub fn pick_account<'a>(&self, accounts: &'a [CloudflareAccount]) -> Option<&'a CloudflareAccount> {
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
}

pub fn load_accounts_from_env() -> Vec<CloudflareAccount> {
    dotenvy::dotenv().ok();
    let mut accounts = Vec::new();
    for i in 1..=30u32 {
        let id_key = format!("CF_ACCOUNT_{i}_ID");
        let token_key = format!("CF_ACCOUNT_{i}_TOKEN");
        if let (Ok(id), Ok(token)) = (std::env::var(&id_key), std::env::var(&token_key)) {
            let tier = std::env::var(format!("CF_ACCOUNT_{i}_TIER")).unwrap_or_else(|_| "free".into());
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
        if let (Ok(id), Ok(token)) = (std::env::var("CF_ACCOUNT_ID"), std::env::var("CF_API_TOKEN")) {
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
