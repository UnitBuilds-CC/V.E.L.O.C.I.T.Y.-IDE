use super::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};


pub fn empty_browser_session_state(session_id: &str) -> BrowserSessionState {
    BrowserSessionState {
        id: session_id.to_string(),
        current_url: None,
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    }
}

pub fn default_browser_user_agent() -> &'static str {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36"
}

pub fn normalize_network_config(config: &mut BrowserSessionNetworkConfig) {
    config.headers.retain(|key: &String, _| !key.trim().is_empty());
    config.allowed_url_prefixes = config
        .allowed_url_prefixes
        .iter()
        .map(|value: &String| value.trim().to_string())
        .filter(|value: &String| !value.is_empty())
        .collect();
    config.blocked_url_prefixes = config
        .blocked_url_prefixes
        .iter()
        .map(|value: &String| value.trim().to_string())
        .filter(|value: &String| !value.is_empty())
        .collect();
}

pub fn network_policy_allows_url(
    config: &BrowserSessionNetworkConfig,
    url: &str,
) -> Result<(), String> {
    if config
        .blocked_url_prefixes
        .iter()
        .any(|prefix| url.starts_with(prefix))
    {
        return Err(format!("network policy blocked url '{url}'"));
    }
    if !config.allowed_url_prefixes.is_empty()
        && !config
            .allowed_url_prefixes
            .iter()
            .any(|prefix| url.starts_with(prefix))
    {
        return Err(format!("network policy disallowed url '{url}'"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct BrowserReplayState {
    pub session: BrowserSessionState,
    pub snapshot: BrowserPageSnapshot,
    pub filled_fields: HashMap<String, String>,
    pub variables: HashMap<String, String>,
    pub outputs: HashMap<String, String>,
}

pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5_000;
pub const DEFAULT_WAIT_INTERVAL_MS: u64 = 250;
pub const DEFAULT_STABLE_POLLS: u32 = 2;

pub fn replay_lookup<'a>(state: &'a BrowserReplayState, key: &str) -> Option<&'a str> {
    state
        .outputs
        .get(key)
        .map(|value: &String| value.as_str())
        .or_else(|| state.variables.get(key).map(|value: &String| value.as_str()))
}

pub fn resolve_template(input: &str, state: &BrowserReplayState) -> String {
    if !input.contains("{{") {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut remaining = input;
    loop {
        let Some(start) = remaining.find("{{") else {
            out.push_str(remaining);
            break;
        };
        out.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            out.push_str(&remaining[start..]);
            break;
        };
        let key = after_start[..end].trim();
        if let Some(value) = replay_lookup(state, key) {
            out.push_str(value);
        } else {
            out.push_str(&remaining[start..start + end + 4]);
        }
        remaining = &after_start[end + 2..];
    }
    out
}

pub fn content_hash_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>()
}

pub fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let search = format!("{}=", attr_name);
    let lower = tag.to_ascii_lowercase();
    let idx = lower.find(&search)?;
    let after_eq = &tag[idx + search.len()..];
    if after_eq.is_empty() {
        return None;
    }
    let quote_char = after_eq.chars().next()?;
    if quote_char == '"' || quote_char == '\'' {
        let val_part = &after_eq[1..];
        let end_idx = val_part.find(quote_char)?;
        Some(val_part[..end_idx].to_string())
    } else {
        let end_idx = after_eq.find(|c: char| c.is_whitespace() || c == '/' || c == '>');
        Some(match end_idx {
            Some(end) => after_eq[..end].to_string(),
            None => after_eq.to_string(),
        })
    }
}

pub fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }

    let base_trimmed = base.trim_end_matches('/');
    if relative.starts_with('/') {
        if let Some(domain_end) = base_trimmed.find("://") {
            let domain_part = &base_trimmed[domain_end + 3..];
            if let Some(slash_idx) = domain_part.find('/') {
                let domain = &base_trimmed[..domain_end + 3 + slash_idx];
                return format!("{}{}", domain, relative);
            }
        }
        return format!("{}{}", base_trimmed, relative);
    }

    if let Some(last_slash) = base_trimmed.rfind('/') {
        if last_slash > 8 {
            return format!("{}/{}", &base_trimmed[..last_slash], relative);
        }
    }
    format!("{}/{}", base_trimmed, relative)
}

pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

pub fn strip_html_tags(fragment: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut last_was_space = true;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => {
                let normalized = if ch.is_whitespace() { ' ' } else { ch };
                if normalized == ' ' {
                    if !last_was_space {
                        text.push(' ');
                        last_was_space = true;
                    }
                } else {
                    text.push(normalized);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

pub fn extract_element_body_text(html: &str, start_index: usize, closing_tag: &str) -> String {
    let body_start = start_index.min(html.len());
    let lower_tail = html[body_start..].to_ascii_lowercase();
    if let Some(close_rel) = lower_tail.find(closing_tag) {
        strip_html_tags(&html[body_start..body_start + close_rel])
    } else {
        String::new()
    }
}

pub fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

pub fn sanitize_file_stem(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "workflow".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn session_file_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-sessions")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

pub fn runtime_session_file_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("runtime-browser-sessions")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

pub fn browser_runtime_visual_dir(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-runtime-visuals")
}

pub fn browser_runtime_visual_png_path(workspace_root: &Path, artifact_id: &str) -> PathBuf {
    browser_runtime_visual_dir(workspace_root)
        .join(format!("{}.png", sanitize_file_stem(artifact_id)))
}

pub fn browser_runtime_visual_metadata_path(workspace_root: &Path, artifact_id: &str) -> PathBuf {
    browser_runtime_visual_dir(workspace_root)
        .join(format!("{}.json", sanitize_file_stem(artifact_id)))
}

pub fn runtime_visual_artifact_id(url: &str) -> String {
    content_hash_id(url)
}

pub fn crawl_facts_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-captures")
        .join(format!("{}.nda", content_hash_id(url)))
}

pub fn browser_snapshot_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-snapshots")
        .join(format!("{}.json", content_hash_id(url)))
}

pub fn browser_html_fallback_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-html-fallbacks")
        .join(format!("{}.html", content_hash_id(url)))
}

pub fn browser_session_transcript_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-session-transcripts")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

pub fn browser_workflow_json_path(workspace_root: &Path, workflow_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .join(format!(
            "{}.browser.json",
            sanitize_file_stem(workflow_name)
        ))
}

pub fn browser_workflow_nda_path(workspace_root: &Path, workflow_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .join(format!("{}.browser.nda", sanitize_file_stem(workflow_name)))
}

pub fn browser_workflow_run_path(
    workspace_root: &Path,
    workflow_name: &str,
    session_id: &str,
) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-runs")
        .join(format!(
            "{}--{}.run.json",
            sanitize_file_stem(workflow_name),
            sanitize_file_stem(session_id)
        ))
}
