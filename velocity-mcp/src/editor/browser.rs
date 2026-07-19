use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use velocity_ide::site_map::verifier::NdaNode;
use velocity_ide::site_map::{SiteMap, VcTriple};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AomElement {
    pub role: String,
    pub name: String,
    pub value: String,
    pub target_url: Option<String>,
    #[serde(default)]
    pub supported_actions: Vec<String>,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub actionability: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFormField {
    pub name: String,
    pub label: String,
    pub input_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserForm {
    pub id: String,
    pub action: String,
    pub method: String,
    pub fields: Vec<BrowserFormField>,
    pub submit_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserStorageBucket {
    pub scope: String,
    pub entries: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserPageSnapshot {
    pub url: String,
    pub title: String,
    pub summary: String,
    pub elements: Vec<AomElement>,
    pub forms: Vec<BrowserForm>,
    pub cookies: Vec<BrowserCookie>,
    #[serde(default)]
    pub storage: Vec<BrowserStorageBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionState {
    pub id: String,
    pub current_url: Option<String>,
    pub cookies: Vec<BrowserCookie>,
    #[serde(default)]
    pub local_storage: HashMap<String, String>,
    #[serde(default)]
    pub session_storage: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionCheckpoint {
    pub name: String,
    pub session: BrowserSessionState,
    pub snapshot: Option<BrowserPageSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserWorkflowStep {
    Navigate { url: String },
    Click { role: String, name: String },
    FillField { field: String, value: String },
    SubmitForm { form: Option<String> },
    WaitForText {
        text: String,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForElement {
        role: String,
        name: String,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForTitle {
        title: String,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForUrlContains {
        fragment: String,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForStable {
        stable_polls: Option<u32>,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    ExtractText {
        output: String,
        source: String,
        role: Option<String>,
        name: Option<String>,
        field: Option<String>,
    },
    SaveCheckpoint { name: String },
    RestoreCheckpoint { name: String },
    IfTextContains {
        text: String,
        then_steps: Vec<BrowserWorkflowStep>,
        else_steps: Vec<BrowserWorkflowStep>,
    },
    IfOutputEquals {
        output: String,
        equals: String,
        then_steps: Vec<BrowserWorkflowStep>,
        else_steps: Vec<BrowserWorkflowStep>,
    },
    AssertElement { role: String, name: String },
    AssertTextContains { text: String },
    AssertOutput {
        output: String,
        equals: Option<String>,
        contains: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshotDiff {
    pub title_changed: bool,
    pub summary_changed: bool,
    pub added_elements: Vec<String>,
    pub removed_elements: Vec<String>,
    pub added_forms: Vec<String>,
    pub removed_forms: Vec<String>,
    pub added_cookies: Vec<String>,
    pub removed_cookies: Vec<String>,
    pub added_storage: Vec<String>,
    pub removed_storage: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflow {
    pub name: String,
    pub start_url: String,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    pub steps: Vec<BrowserWorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowRunReport {
    pub workflow_name: String,
    pub session_id: String,
    pub final_url: String,
    pub final_title: String,
    pub step_count: usize,
    pub cookie_count: usize,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub outputs: HashMap<String, String>,
    pub log: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuite {
    pub name: String,
    pub workflows: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteRunItem {
    pub workflow_path: String,
    pub workflow_name: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteRunReport {
    pub suite_name: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub items: Vec<BrowserWorkflowSuiteRunItem>,
}

struct BrowserHttpResponse {
    html: String,
    cookies: Vec<BrowserCookie>,
    local_storage_updates: HashMap<String, String>,
    session_storage_updates: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct BrowserReplayState {
    session: BrowserSessionState,
    snapshot: BrowserPageSnapshot,
    filled_fields: HashMap<String, String>,
    variables: HashMap<String, String>,
    outputs: HashMap<String, String>,
}

const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_WAIT_INTERVAL_MS: u64 = 250;
const DEFAULT_STABLE_POLLS: u32 = 2;

fn replay_lookup<'a>(state: &'a BrowserReplayState, key: &str) -> Option<&'a str> {
    state
        .outputs
        .get(key)
        .map(|value| value.as_str())
        .or_else(|| state.variables.get(key).map(|value| value.as_str()))
}

fn resolve_template(input: &str, state: &BrowserReplayState) -> String {
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

fn content_hash_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>()
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
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

fn resolve_relative_url(base: &str, relative: &str) -> String {
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

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn sanitize_file_stem(value: &str) -> String {
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

fn session_file_path(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-sessions")
        .join(format!("{}.json", sanitize_file_stem(session_id)))
}

fn crawl_facts_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-captures")
        .join(format!("{}.nda", content_hash_id(url)))
}

fn browser_snapshot_path(url: &str, sitemap_path: &Path) -> PathBuf {
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-snapshots")
        .join(format!("{}.json", content_hash_id(url)))
}

fn browser_workflow_json_path(workspace_root: &Path, workflow_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .join(format!("{}.browser.json", sanitize_file_stem(workflow_name)))
}

fn browser_workflow_nda_path(workspace_root: &Path, workflow_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .join(format!("{}.browser.nda", sanitize_file_stem(workflow_name)))
}

fn browser_workflow_run_path(workspace_root: &Path, workflow_name: &str, session_id: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-runs")
        .join(format!(
            "{}--{}.run.json",
            sanitize_file_stem(workflow_name),
            sanitize_file_stem(session_id)
        ))
}

fn browser_workflow_suite_json_path(workspace_root: &Path, suite_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-suites")
        .join(format!("{}.suite.json", sanitize_file_stem(suite_name)))
}

fn browser_workflow_suite_run_path(workspace_root: &Path, suite_name: &str) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-suite-runs")
        .join(format!("{}.suite-run.json", sanitize_file_stem(suite_name)))
}

fn browser_session_checkpoint_path(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("browser-session-checkpoints")
        .join(sanitize_file_stem(session_id))
        .join(format!("{}.checkpoint.json", sanitize_file_stem(checkpoint_name)))
}

fn parse_cookie_header(value: &str) -> Option<BrowserCookie> {
    let cookie_part = value.split(';').next()?.trim();
    let mut parts = cookie_part.splitn(2, '=');
    let name = parts.next()?.trim();
    let cookie_value = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    Some(BrowserCookie {
        name: name.to_string(),
        value: cookie_value.to_string(),
    })
}

fn merge_cookie(cookies: &mut Vec<BrowserCookie>, cookie: BrowserCookie) {
    if let Some(existing) = cookies.iter_mut().find(|entry| entry.name == cookie.name) {
        *existing = cookie;
    } else {
        cookies.push(cookie);
    }
}

fn cookie_header(cookies: &[BrowserCookie]) -> Option<String> {
    if cookies.is_empty() {
        None
    } else {
        Some(
            cookies
                .iter()
                .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

fn parse_storage_header(raw: &str) -> HashMap<String, String> {
    let mut updates = HashMap::new();
    for pair in raw.split(';') {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            updates.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    updates
}

fn storage_buckets(session: &BrowserSessionState) -> Vec<BrowserStorageBucket> {
    let mut buckets = Vec::new();
    if !session.local_storage.is_empty() {
        buckets.push(BrowserStorageBucket {
            scope: "local".to_string(),
            entries: session.local_storage.clone(),
        });
    }
    if !session.session_storage.is_empty() {
        buckets.push(BrowserStorageBucket {
            scope: "session".to_string(),
            entries: session.session_storage.clone(),
        });
    }
    buckets
}

fn storage_signature(bucket: &BrowserStorageBucket) -> Vec<String> {
    let mut entries = bucket
        .entries
        .iter()
        .map(|(key, value)| format!("{}:{}={}", bucket.scope, key, value))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn apply_storage_updates(target: &mut HashMap<String, String>, updates: &HashMap<String, String>) {
    for (key, value) in updates {
        target.insert(key.clone(), value.clone());
    }
}

fn fetch_with_session(
    url: &str,
    method: &str,
    body: Option<&str>,
    cookies: &[BrowserCookie],
) -> Result<BrowserHttpResponse, String> {
    let agent = ureq::Agent::new();
    let mut request = agent
        .request(method, url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36",
        );
    if let Some(header) = cookie_header(cookies) {
        request = request.set("Cookie", &header);
    }

    let response = if method.eq_ignore_ascii_case("POST") {
        request
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(body.unwrap_or_default())
            .map_err(|e| format!("HTTP request failed: {:?}", e))?
    } else {
        request
            .call()
            .map_err(|e| format!("HTTP request failed: {:?}", e))?
    };

    let mut response_cookies = Vec::new();
    for header in response.all("Set-Cookie") {
        if let Some(cookie) = parse_cookie_header(header) {
            merge_cookie(&mut response_cookies, cookie);
        }
    }
    let local_storage_updates = response
        .header("X-Velocity-Local-Storage")
        .map(parse_storage_header)
        .unwrap_or_default();
    let session_storage_updates = response
        .header("X-Velocity-Session-Storage")
        .map(parse_storage_header)
        .unwrap_or_default();

    let html = response
        .into_string()
        .map_err(|e| format!("Failed to read HTTP body: {:?}", e))?;
    Ok(BrowserHttpResponse {
        html,
        cookies: response_cookies,
        local_storage_updates,
        session_storage_updates,
    })
}

fn scan_tags(fragment: &str) -> Vec<String> {
    let chars: Vec<char> = fragment.chars().collect();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            i += 1;
            let mut tag = String::new();
            while i < chars.len() && chars[i] != '>' {
                tag.push(chars[i]);
                i += 1;
            }
            tags.push(tag);
        }
        i += 1;
    }
    tags
}

fn parse_forms(url: &str, html: &str) -> Vec<BrowserForm> {
    let lower_html = html.to_ascii_lowercase();
    let mut forms = Vec::new();
    let mut search_from = 0;

    while let Some(form_start_rel) = lower_html[search_from..].find("<form") {
        let form_start = search_from + form_start_rel;
        let tag_end_rel = lower_html[form_start..].find('>');
        let Some(tag_end_rel) = tag_end_rel else { break; };
        let tag_end = form_start + tag_end_rel;
        let form_tag = &html[form_start + 1..tag_end];
        let body_start = tag_end + 1;
        let close_rel = lower_html[body_start..].find("</form>");
        let Some(close_rel) = close_rel else { break; };
        let body_end = body_start + close_rel;
        let form_body = &html[body_start..body_end];

        let form_id = extract_attr(form_tag, "id")
            .or_else(|| extract_attr(form_tag, "name"))
            .unwrap_or_else(|| format!("form-{}", forms.len()));
        let action = extract_attr(form_tag, "action")
            .map(|value| resolve_relative_url(url, &value))
            .unwrap_or_else(|| url.to_string());
        let method = extract_attr(form_tag, "method")
            .unwrap_or_else(|| "GET".to_string())
            .to_ascii_uppercase();

        let mut fields = Vec::new();
        let mut submit_label = None;
        for raw_tag in scan_tags(form_body) {
            let trimmed = raw_tag.trim();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("input") {
                let input_type = extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
                let name = extract_attr(trimmed, "name")
                    .or_else(|| extract_attr(trimmed, "id"))
                    .unwrap_or_else(|| format!("field-{}", fields.len()));
                let label = extract_attr(trimmed, "placeholder")
                    .or_else(|| extract_attr(trimmed, "aria-label"))
                    .unwrap_or_else(|| name.clone());
                let value = extract_attr(trimmed, "value").unwrap_or_default();

                if matches!(input_type.as_str(), "submit" | "button") {
                    if submit_label.is_none() {
                        submit_label = Some(if !value.is_empty() { value } else { label });
                    }
                } else {
                    fields.push(BrowserFormField {
                        name,
                        label,
                        input_type,
                        value,
                    });
                }
            } else if lower.starts_with("textarea") {
                let name = extract_attr(trimmed, "name")
                    .or_else(|| extract_attr(trimmed, "id"))
                    .unwrap_or_else(|| format!("field-{}", fields.len()));
                let label = extract_attr(trimmed, "placeholder")
                    .or_else(|| extract_attr(trimmed, "aria-label"))
                    .unwrap_or_else(|| name.clone());
                fields.push(BrowserFormField {
                    name,
                    label,
                    input_type: "textarea".to_string(),
                    value: String::new(),
                });
            } else if lower.starts_with("button") && submit_label.is_none() {
                submit_label = extract_attr(trimmed, "aria-label")
                    .or_else(|| extract_attr(trimmed, "name"))
                    .or_else(|| extract_attr(trimmed, "value"));
            }
        }

        forms.push(BrowserForm {
            id: form_id,
            action,
            method,
            fields,
            submit_label,
        });
        search_from = body_end + "</form>".len();
    }

    forms
}

fn parse_html_to_snapshot(
    url: &str,
    html: &str,
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
) -> BrowserPageSnapshot {
    let mut elements = Vec::new();
    let mut title = "Untitled Page".to_string();
    let mut page_text = String::new();

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut tag_content = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '>' {
                tag_content.push(chars[i]);
                i += 1;
            }
            let trimmed = tag_content.trim();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("title") {
                i += 1;
                let mut t = String::new();
                while i < chars.len() && chars[i] != '<' {
                    t.push(chars[i]);
                    i += 1;
                }
                title = t.trim().to_string();
            } else if lower.starts_with("a ") || lower.starts_with("a>") {
                let href = extract_attr(trimmed, "href");
                let mut text_content = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '<' {
                    text_content.push(chars[j]);
                    j += 1;
                }
                let clean_text = text_content.trim().to_string();
                if let Some(href_value) = href {
                    let absolute_href = resolve_relative_url(url, &href_value);
                    elements.push(AomElement {
                        role: "link".to_string(),
                        name: if clean_text.is_empty() { absolute_href.clone() } else { clean_text },
                        value: absolute_href.clone(),
                        target_url: Some(absolute_href),
                        supported_actions: vec!["open".to_string(), "click".to_string()],
                        provenance: "native-static".to_string(),
                        actionability: role_actionability("link"),
                    });
                }
            } else if lower.starts_with("button") {
                let mut text_content = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '<' {
                    text_content.push(chars[j]);
                    j += 1;
                }
                let label = text_content.trim().to_string();
                let fallback = extract_attr(trimmed, "aria-label")
                    .or_else(|| extract_attr(trimmed, "name"))
                    .unwrap_or_default();
                let final_name = if label.is_empty() { fallback } else { label };
                if !final_name.is_empty() {
                    elements.push(AomElement {
                        role: "button".to_string(),
                        name: final_name,
                        value: String::new(),
                        target_url: None,
                        supported_actions: vec!["click".to_string()],
                        provenance: "native-static".to_string(),
                        actionability: role_actionability("button"),
                    });
                }
            } else if lower.starts_with("input") {
                let input_type = extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
                let placeholder = extract_attr(trimmed, "placeholder").unwrap_or_default();
                let name_attr = extract_attr(trimmed, "name").unwrap_or_default();
                let value_attr = extract_attr(trimmed, "value").unwrap_or_default();
                let name = if !placeholder.is_empty() {
                    placeholder
                } else if !name_attr.is_empty() {
                    name_attr
                } else {
                    "Input Field".to_string()
                };
                let role = match input_type.as_str() {
                    "button" | "submit" => "button",
                    _ => "textbox",
                };
                let supported_actions = if role == "button" {
                    vec!["click".to_string()]
                } else {
                    vec!["focus".to_string(), "type".to_string()]
                };
                elements.push(AomElement {
                    role: role.to_string(),
                    name,
                    value: value_attr,
                    target_url: None,
                    supported_actions,
                    provenance: "native-static".to_string(),
                    actionability: role_actionability(role),
                });
            }
        } else {
            if chars[i] != '\r' && chars[i] != '\n' && chars[i] != '\t' {
                page_text.push(chars[i]);
            }
            i += 1;
        }
    }

    BrowserPageSnapshot {
        url: url.to_string(),
        title,
        summary: truncate_string(page_text.trim(), 1000),
        elements,
        forms: parse_forms(url, html),
        cookies: cookies.to_vec(),
        storage: storage.to_vec(),
    }
}

fn write_snapshot_json(snapshot: &BrowserPageSnapshot, sitemap_path: &Path) -> Result<PathBuf, String> {
    let snapshot_path = browser_snapshot_path(&snapshot.url, sitemap_path);
    if let Some(parent) = snapshot_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser snapshot dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(snapshot).map_err(|err| format!("serialise browser snapshot: {err}"))?;
    fs::write(&snapshot_path, json).map_err(|err| format!("write browser snapshot: {err}"))?;
    Ok(snapshot_path)
}

fn load_snapshot_json(url: &str, sitemap_path: &Path) -> Result<BrowserPageSnapshot, String> {
    let snapshot_path = browser_snapshot_path(url, sitemap_path);
    let raw = fs::read(&snapshot_path).map_err(|err| format!("read browser snapshot: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser snapshot: {err}"))
}

fn write_crawl_facts(
    url: &str,
    title: &str,
    summary: &str,
    elements: &[AomElement],
    forms: &[BrowserForm],
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let facts_path = crawl_facts_path(url, sitemap_path);
    if let Some(parent) = facts_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser capture dir: {err}"))?;
    }

    let storage_entry_count = storage.iter().map(|bucket| bucket.entries.len()).sum::<usize>();
    let mut facts = vec![
        "browser-capture version 5".to_string(),
        "field_count 5".to_string(),
        "field\tkind\tpage-crawl".to_string(),
        format!("field\telement_count\t{}", elements.len()),
        format!("field\tform_count\t{}", forms.len()),
        format!("field\tcookie_count\t{}", cookies.len()),
        format!("field\tstorage_entry_count\t{}", storage_entry_count),
        "page_field_count 3".to_string(),
        format!("page_field\turl\t{}", encode_nda_text(url)),
        format!("page_field\ttitle\t{}", encode_nda_text(title)),
        format!("page_field\tsummary\t{}", encode_nda_text(summary)),
    ];

    for (idx, element) in elements.iter().enumerate() {
        facts.push(format!("element\t{}", idx));
        facts.push(format!("element_field\t{}\trole\t{}", idx, encode_nda_text(&element.role)));
        facts.push(format!("element_field\t{}\tname\t{}", idx, encode_nda_text(&element.name)));
        facts.push(format!("element_field\t{}\tvalue\t{}", idx, encode_nda_text(&element.value)));
        facts.push(format!(
            "element_field\t{}\ttarget_url\t{}",
            idx,
            encode_nda_text(element.target_url.as_deref().unwrap_or("-")),
        ));
    }

    for (form_idx, form) in forms.iter().enumerate() {
        facts.push(format!("form\t{}", form_idx));
        facts.push(format!("form_field\t{}\tid\t{}", form_idx, encode_nda_text(&form.id)));
        facts.push(format!("form_field\t{}\taction\t{}", form_idx, encode_nda_text(&form.action)));
        facts.push(format!("form_field\t{}\tmethod\t{}", form_idx, encode_nda_text(&form.method)));
        if let Some(submit_label) = &form.submit_label {
            facts.push(format!("form_field\t{}\tsubmit_label\t{}", form_idx, encode_nda_text(submit_label)));
        }
        for (field_idx, field) in form.fields.iter().enumerate() {
            facts.push(format!("form_input\t{}\t{}", form_idx, field_idx));
            facts.push(format!(
                "form_input_field\t{}\t{}\tname\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.name)
            ));
            facts.push(format!(
                "form_input_field\t{}\t{}\tlabel\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.label)
            ));
            facts.push(format!(
                "form_input_field\t{}\t{}\ttype\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.input_type)
            ));
        }
    }

    for (idx, cookie) in cookies.iter().enumerate() {
        facts.push(format!("cookie\t{}", idx));
        facts.push(format!("cookie_field\t{}\tname\t{}", idx, encode_nda_text(&cookie.name)));
        facts.push(format!("cookie_field\t{}\tvalue\t{}", idx, encode_nda_text(&cookie.value)));
    }

    for (bucket_idx, bucket) in storage.iter().enumerate() {
        facts.push(format!("storage\t{}", bucket_idx));
        facts.push(format!("storage_field\t{}\tscope\t{}", bucket_idx, encode_nda_text(&bucket.scope)));
        for (entry_idx, (key, value)) in bucket.entries.iter().enumerate() {
            facts.push(format!("storage_entry\t{}\t{}", bucket_idx, entry_idx));
            facts.push(format!(
                "storage_entry_field\t{}\t{}\tkey\t{}",
                bucket_idx,
                entry_idx,
                encode_nda_text(key)
            ));
            facts.push(format!(
                "storage_entry_field\t{}\t{}\tvalue\t{}",
                bucket_idx,
                entry_idx,
                encode_nda_text(value)
            ));
        }
    }

    fs::write(&facts_path, facts.join("\n") + "\n")
        .map_err(|err| format!("write browser capture facts: {err}"))?;
    Ok(facts_path)
}

fn persist_snapshot_to_sitemap(snapshot: &BrowserPageSnapshot, sitemap_path: &Path) -> Result<(), String> {
    let mut sm = SiteMap::open(sitemap_path, 0).map_err(|e| format!("Failed to open SiteMap: {:?}", e))?;
    let page_hash = sm.register_string(&snapshot.url).map_err(|e| e.to_string())?;
    let title_hash = sm.register_string(&snapshot.title).map_err(|e| e.to_string())?;
    let summary_hash = sm.register_string(&snapshot.summary).map_err(|e| e.to_string())?;

    let mut live_triples = vec![
        VcTriple { subject_hash: page_hash, predicate_id: 10, object_hash: page_hash },
        VcTriple { subject_hash: page_hash, predicate_id: 11, object_hash: title_hash },
        VcTriple { subject_hash: page_hash, predicate_id: 12, object_hash: summary_hash },
    ];

    for triple in &live_triples {
        sm.put_node(&NdaNode::Triple {
            subject_hash: triple.subject_hash,
            predicate_id: triple.predicate_id,
            object_hash: triple.object_hash,
        })
        .map_err(|e| e.to_string())?;
    }

    let mut aom_node_hashes = Vec::new();
    for el in &snapshot.elements {
        let el_role_hash = sm.register_string(&el.role).map_err(|e| e.to_string())?;
        let el_name_hash = sm.register_string(&el.name).map_err(|e| e.to_string())?;
        let el_val_hash = sm.register_string(&el.value).map_err(|e| e.to_string())?;

        let mut hasher = Sha256::new();
        hasher.update(page_hash.to_le_bytes());
        hasher.update(el.role.as_bytes());
        hasher.update(el.name.as_bytes());
        let digest = hasher.finalize();
        let el_hash = u64::from_le_bytes(digest[0..8].try_into().unwrap());

        for triple in [
            VcTriple { subject_hash: el_hash, predicate_id: 16, object_hash: el_role_hash },
            VcTriple { subject_hash: el_hash, predicate_id: 17, object_hash: el_name_hash },
            VcTriple { subject_hash: el_hash, predicate_id: 18, object_hash: el_val_hash },
        ] {
            sm.put_node(&NdaNode::Triple {
                subject_hash: triple.subject_hash,
                predicate_id: triple.predicate_id,
                object_hash: triple.object_hash,
            })
            .map_err(|e| e.to_string())?;
            live_triples.push(triple);
        }

        if let Some(target) = &el.target_url {
            let target_hash = sm.register_string(target).map_err(|e| e.to_string())?;
            let triple = VcTriple { subject_hash: page_hash, predicate_id: 1, object_hash: target_hash };
            sm.put_node(&NdaNode::Triple {
                subject_hash: triple.subject_hash,
                predicate_id: triple.predicate_id,
                object_hash: triple.object_hash,
            })
            .map_err(|e| e.to_string())?;
            live_triples.push(triple);
        }

        aom_node_hashes.push(el_hash);
    }

    if !aom_node_hashes.is_empty() {
        let aom_root_node = NdaNode::Scope {
            children: aom_node_hashes.iter().copied().map(|target| NdaNode::Call { target }).collect(),
        };
        let root_hash = sm.put_node(&aom_root_node).map_err(|e| e.to_string())?;
        let triple = VcTriple { subject_hash: page_hash, predicate_id: 6, object_hash: root_hash };
        sm.put_node(&NdaNode::Triple {
            subject_hash: triple.subject_hash,
            predicate_id: triple.predicate_id,
            object_hash: triple.object_hash,
        })
        .map_err(|e| e.to_string())?;
        live_triples.push(triple);
    }

    sm.put_file_snapshot(&format!("browser:{}", snapshot.url), &live_triples)
        .map_err(|e| e.to_string())?;
    sm.flush().map_err(|e| e.to_string())
}

pub fn create_session(workspace_root: &Path, session_id: &str) -> Result<PathBuf, String> {
    let session = BrowserSessionState {
        id: session_id.to_string(),
        current_url: None,
        cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
    };
    save_session_state(workspace_root, &session)
}

pub fn save_session_state(workspace_root: &Path, session: &BrowserSessionState) -> Result<PathBuf, String> {
    let path = session_file_path(workspace_root, &session.id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create session dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(session).map_err(|err| format!("serialise browser session: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser session: {err}"))?;
    Ok(path)
}

pub fn load_session_state(workspace_root: &Path, session_id: &str) -> Result<BrowserSessionState, String> {
    let path = session_file_path(workspace_root, session_id);
    let raw = fs::read(&path).map_err(|err| format!("read browser session: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser session: {err}"))
}

pub fn session_state_to_json(session: &BrowserSessionState) -> Result<String, String> {
    serde_json::to_string_pretty(session).map_err(|err| format!("serialise browser session state: {err}"))
}

pub fn set_session_storage_entries(
    workspace_root: &Path,
    session_id: &str,
    scope: &str,
    entries: &HashMap<String, String>,
) -> Result<PathBuf, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    match scope {
        "local" => apply_storage_updates(&mut session.local_storage, entries),
        "session" => apply_storage_updates(&mut session.session_storage, entries),
        _ => return Err(format!("unsupported browser storage scope: '{}'", scope)),
    }
    save_session_state(workspace_root, &session)
}

pub fn get_session_storage_entries(
    workspace_root: &Path,
    session_id: &str,
    scope: &str,
) -> Result<String, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let entries = match scope {
        "local" => &session.local_storage,
        "session" => &session.session_storage,
        _ => return Err(format!("unsupported browser storage scope: '{}'", scope)),
    };
    serde_json::to_string_pretty(entries).map_err(|err| format!("serialise browser storage state: {err}"))
}

fn write_session_checkpoint(
    workspace_root: &Path,
    checkpoint: &BrowserSessionCheckpoint,
) -> Result<PathBuf, String> {
    let path = browser_session_checkpoint_path(workspace_root, &checkpoint.session.id, &checkpoint.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create checkpoint dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(checkpoint)
        .map_err(|err| format!("serialise browser checkpoint: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser checkpoint: {err}"))?;
    Ok(path)
}

fn persist_checkpoint_from_replay_state(
    workspace_root: &Path,
    state: &BrowserReplayState,
    checkpoint_name: &str,
) -> Result<PathBuf, String> {
    save_session_state(workspace_root, &state.session)?;
    let checkpoint = BrowserSessionCheckpoint {
        name: checkpoint_name.to_string(),
        session: state.session.clone(),
        snapshot: Some(state.snapshot.clone()),
    };
    write_session_checkpoint(workspace_root, &checkpoint)
}

pub fn save_session_checkpoint(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let session = load_session_state(workspace_root, session_id)?;
    let snapshot = match session.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let checkpoint = BrowserSessionCheckpoint {
        name: checkpoint_name.to_string(),
        session,
        snapshot,
    };
    write_session_checkpoint(workspace_root, &checkpoint)
}

pub fn restore_session_checkpoint(
    workspace_root: &Path,
    session_id: &str,
    checkpoint_name: &str,
    target_session_id: Option<&str>,
    sitemap_path: &Path,
) -> Result<String, String> {
    let path = browser_session_checkpoint_path(workspace_root, session_id, checkpoint_name);
    let raw = fs::read(&path).map_err(|err| format!("read browser checkpoint: {err}"))?;
    let mut checkpoint: BrowserSessionCheckpoint =
        serde_json::from_slice(&raw).map_err(|err| format!("parse browser checkpoint: {err}"))?;
    if let Some(target) = target_session_id {
        checkpoint.session.id = target.to_string();
    }
    let session_path = save_session_state(workspace_root, &checkpoint.session)?;

    let mut details = vec![
        format!("Restored browser session checkpoint '{}'", checkpoint.name),
        format!("Session: {}", checkpoint.session.id),
        format!("Session JSON: {}", session_path.display()),
    ];

    if let Some(snapshot) = checkpoint.snapshot {
        persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
        let facts_path = write_crawl_facts(
            &snapshot.url,
            &snapshot.title,
            &snapshot.summary,
            &snapshot.elements,
            &snapshot.forms,
            &snapshot.cookies,
            &snapshot.storage,
            sitemap_path,
        )?;
        let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
        details.push(format!("URL: {}", snapshot.url));
        details.push(format!("Title: {}", snapshot.title));
        details.push(format!("Snapshot JSON: {}", snapshot_path.display()));
        details.push(format!("NDA Facts: {}", facts_path.display()));
    }

    Ok(details.join("\n"))
}

pub fn navigate_session(
    workspace_root: &Path,
    session_id: &str,
    url: &str,
    sitemap_path: &Path,
) -> Result<String, String> {
    let mut session = load_session_state(workspace_root, session_id).unwrap_or(BrowserSessionState {
        id: session_id.to_string(),
        current_url: None,
        cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
    });
    let snapshot = crawl_page_snapshot_with_session(&mut session, url)?;
    persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &snapshot.url,
        &snapshot.title,
        &snapshot.summary,
        &snapshot.elements,
        &snapshot.forms,
        &snapshot.cookies,
        &snapshot.storage,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
    let session_path = save_session_state(workspace_root, &session)?;

    Ok(format!(
        "Session navigate complete.\nSession: {}\nURL: {}\nTitle: {}\nForms: {}\nCookies: {}\nLocal storage: {}\nSession storage: {}\nSnapshot JSON: {}\nSession JSON: {}\nNDA Facts: {}",
        session.id,
        snapshot.url,
        snapshot.title,
        snapshot.forms.len(),
        snapshot.cookies.len(),
        session.local_storage.len(),
        session.session_storage.len(),
        snapshot_path.display(),
        session_path.display(),
        facts_path.display(),
    ))
}

pub fn crawl_page_snapshot(url: &str) -> Result<BrowserPageSnapshot, String> {
    let mut session = BrowserSessionState {
        id: "ephemeral".to_string(),
        current_url: Some(url.to_string()),
        cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
    };
    crawl_page_snapshot_with_session(&mut session, url)
}

pub fn crawl_page_snapshot_with_session(
    session: &mut BrowserSessionState,
    url: &str,
) -> Result<BrowserPageSnapshot, String> {
    let response = fetch_with_session(url, "GET", None, &session.cookies)?;
    for cookie in response.cookies.iter().cloned() {
        merge_cookie(&mut session.cookies, cookie);
    }
    apply_storage_updates(&mut session.local_storage, &response.local_storage_updates);
    apply_storage_updates(&mut session.session_storage, &response.session_storage_updates);
    session.current_url = Some(url.to_string());
    let storage = storage_buckets(session);
    Ok(parse_html_to_snapshot(url, &response.html, &session.cookies, &storage))
}

fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn normalize_match_text(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn string_match_score(haystack: &str, needle: &str) -> Option<i32> {
    let haystack = normalize_match_text(haystack);
    let needle = normalize_match_text(needle);
    if needle.is_empty() {
        return Some(1);
    }
    if haystack.is_empty() {
        return None;
    }
    if haystack == needle {
        return Some(1_000);
    }
    if haystack.starts_with(&needle) {
        return Some(700);
    }
    if haystack.contains(&needle) {
        return Some(500);
    }

    let needle_terms = needle.split(' ').collect::<Vec<_>>();
    let mut matched_terms = 0;
    for term in &needle_terms {
        if haystack.contains(term) {
            matched_terms += 1;
        }
    }
    if matched_terms == 0 {
        return None;
    }
    Some(200 + matched_terms * 40 - (haystack.len() as i32 - needle.len() as i32).abs().min(120))
}

fn role_actionability(role: &str) -> u8 {
    match role.to_ascii_lowercase().as_str() {
        "link" => 80,
        "button" => 70,
        "textbox" => 40,
        _ => 10,
    }
}

fn element_actionability_score(element: &AomElement) -> i32 {
    let mut score = i32::from(role_actionability(&element.role));
    if element.target_url.is_some() {
        score += 30;
    }
    if !element.value.is_empty() {
        score += 5;
    }
    score
}

fn element_match_score(element: &AomElement, role: &str, name: &str) -> Option<i32> {
    if !element.role.eq_ignore_ascii_case(role) {
        return None;
    }

    let mut score = element_actionability_score(element);
    if let Some(name_score) = string_match_score(&element.name, name) {
        score += name_score;
    } else {
        let value_score = string_match_score(&element.value, name);
        let target_score = element
            .target_url
            .as_deref()
            .and_then(|target| string_match_score(target, name));
        score += value_score.max(target_score).unwrap_or_default();
        if value_score.is_none() && target_score.is_none() {
            return None;
        }
    }

    if let Some(target_url) = element.target_url.as_deref() {
        if let Some(target_score) = string_match_score(target_url, name) {
            score += target_score / 4;
        }
    }
    Some(score)
}

fn form_field_match_score(field: &BrowserFormField, field_name: &str) -> Option<i32> {
    let label_score = string_match_score(&field.label, field_name);
    let name_score = string_match_score(&field.name, field_name);
    let value_score = string_match_score(&field.value, field_name);
    let best = label_score.max(name_score).max(value_score)?;
    Some(best + if field.input_type.eq_ignore_ascii_case("hidden") { 0 } else { 25 })
}

fn find_element<'a>(snapshot: &'a BrowserPageSnapshot, role: &str, name: &str) -> Option<&'a AomElement> {
    snapshot
        .elements
        .iter()
        .filter_map(|element| element_match_score(element, role, name).map(|score| (score, element)))
        .max_by(|(left_score, left_element), (right_score, right_element)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_element.name.len().cmp(&left_element.name.len()))
        })
        .map(|(_, element)| element)
}

fn find_form<'a>(snapshot: &'a BrowserPageSnapshot, form_id: Option<&str>) -> Option<&'a BrowserForm> {
    match form_id {
        Some(id) => snapshot.forms.iter().find(|form| form.id.eq_ignore_ascii_case(id)),
        None => snapshot.forms.first(),
    }
}

fn find_form_field<'a>(snapshot: &'a BrowserPageSnapshot, field_name: &str) -> Option<&'a BrowserFormField> {
    snapshot
        .forms
        .iter()
        .flat_map(|form| form.fields.iter())
        .filter_map(|field| form_field_match_score(field, field_name).map(|score| (score, field)))
        .max_by(|(left_score, left_field), (right_score, right_field)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_field.label.len().cmp(&left_field.label.len()))
        })
        .map(|(_, field)| field)
}

fn snapshot_contains_text(snapshot: &BrowserPageSnapshot, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    snapshot.title.to_ascii_lowercase().contains(&needle)
        || snapshot.summary.to_ascii_lowercase().contains(&needle)
        || snapshot.forms.iter().any(|form| {
            form.id.to_ascii_lowercase().contains(&needle)
                || form
                    .submit_label
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&needle)
                || form.fields.iter().any(|field| {
                    field.label.to_ascii_lowercase().contains(&needle)
                        || field.name.to_ascii_lowercase().contains(&needle)
                        || field.value.to_ascii_lowercase().contains(&needle)
                })
        })
        || snapshot.elements.iter().any(|element| {
            element.name.to_ascii_lowercase().contains(&needle)
                || element.value.to_ascii_lowercase().contains(&needle)
                || element
                    .target_url
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
}

fn element_signature(element: &AomElement) -> String {
    format!(
        "{}:{}:{}:{}",
        element.role,
        element.name,
        element.value,
        element.target_url.as_deref().unwrap_or_default()
    )
}

fn form_signature(form: &BrowserForm) -> String {
    format!("{}:{}:{}", form.id, form.method, form.action)
}

fn cookie_signature(cookie: &BrowserCookie) -> String {
    format!("{}={}", cookie.name, cookie.value)
}

fn snapshot_storage_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .storage
        .iter()
        .flat_map(storage_signature)
        .collect::<HashSet<_>>()
}

pub fn diff_snapshots(before: &BrowserPageSnapshot, after: &BrowserPageSnapshot) -> BrowserSnapshotDiff {
    let before_elements = before
        .elements
        .iter()
        .map(element_signature)
        .collect::<HashSet<_>>();
    let after_elements = after
        .elements
        .iter()
        .map(element_signature)
        .collect::<HashSet<_>>();
    let before_forms = before.forms.iter().map(form_signature).collect::<HashSet<_>>();
    let after_forms = after.forms.iter().map(form_signature).collect::<HashSet<_>>();
    let before_cookies = before
        .cookies
        .iter()
        .map(cookie_signature)
        .collect::<HashSet<_>>();
    let after_cookies = after
        .cookies
        .iter()
        .map(cookie_signature)
        .collect::<HashSet<_>>();
    let before_storage = snapshot_storage_signatures(before);
    let after_storage = snapshot_storage_signatures(after);

    let mut added_elements = after_elements.difference(&before_elements).cloned().collect::<Vec<_>>();
    let mut removed_elements = before_elements.difference(&after_elements).cloned().collect::<Vec<_>>();
    let mut added_forms = after_forms.difference(&before_forms).cloned().collect::<Vec<_>>();
    let mut removed_forms = before_forms.difference(&after_forms).cloned().collect::<Vec<_>>();
    let mut added_cookies = after_cookies.difference(&before_cookies).cloned().collect::<Vec<_>>();
    let mut removed_cookies = before_cookies.difference(&after_cookies).cloned().collect::<Vec<_>>();
    let mut added_storage = after_storage.difference(&before_storage).cloned().collect::<Vec<_>>();
    let mut removed_storage = before_storage.difference(&after_storage).cloned().collect::<Vec<_>>();

    added_elements.sort();
    removed_elements.sort();
    added_forms.sort();
    removed_forms.sort();
    added_cookies.sort();
    removed_cookies.sort();
    added_storage.sort();
    removed_storage.sort();

    BrowserSnapshotDiff {
        title_changed: before.title != after.title,
        summary_changed: before.summary != after.summary,
        added_elements,
        removed_elements,
        added_forms,
        removed_forms,
        added_cookies,
        removed_cookies,
        added_storage,
        removed_storage,
    }
}

fn render_snapshot_diff(diff: &BrowserSnapshotDiff) -> String {
    let mut parts = Vec::new();
    if diff.title_changed {
        parts.push("title".to_string());
    }
    if diff.summary_changed {
        parts.push("summary".to_string());
    }
    if !diff.added_elements.is_empty() {
        parts.push(format!("elements+{}", diff.added_elements.len()));
    }
    if !diff.removed_elements.is_empty() {
        parts.push(format!("elements-{}", diff.removed_elements.len()));
    }
    if !diff.added_forms.is_empty() {
        parts.push(format!("forms+{}", diff.added_forms.len()));
    }
    if !diff.removed_forms.is_empty() {
        parts.push(format!("forms-{}", diff.removed_forms.len()));
    }
    if !diff.added_cookies.is_empty() {
        parts.push(format!("cookies+{}", diff.added_cookies.len()));
    }
    if !diff.removed_cookies.is_empty() {
        parts.push(format!("cookies-{}", diff.removed_cookies.len()));
    }
    if !diff.added_storage.is_empty() {
        parts.push(format!("storage+{}", diff.added_storage.len()));
    }
    if !diff.removed_storage.is_empty() {
        parts.push(format!("storage-{}", diff.removed_storage.len()));
    }

    if parts.is_empty() {
        "no_semantic_change".to_string()
    } else {
        parts.join(",")
    }
}

fn is_semantically_stable(diff: &BrowserSnapshotDiff) -> bool {
    !diff.title_changed
        && !diff.summary_changed
        && diff.added_elements.is_empty()
        && diff.removed_elements.is_empty()
        && diff.added_forms.is_empty()
        && diff.removed_forms.is_empty()
        && diff.added_cookies.is_empty()
        && diff.removed_cookies.is_empty()
        && diff.added_storage.is_empty()
        && diff.removed_storage.is_empty()
}

fn wait_for_condition<F>(
    session: &mut BrowserSessionState,
    current_snapshot: &mut BrowserPageSnapshot,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
    mut predicate: F,
) -> Result<BrowserSnapshotDiff, String>
where
    F: FnMut(&BrowserPageSnapshot) -> bool,
{
    if predicate(current_snapshot) {
        return Ok(diff_snapshots(current_snapshot, current_snapshot));
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS));
    let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS));
    let started = Instant::now();
    let original = current_snapshot.clone();

    loop {
        if started.elapsed() >= timeout {
            return Err(format!("wait condition not satisfied within {}ms", timeout.as_millis()));
        }
        thread::sleep(interval);
        let url = session
            .current_url
            .clone()
            .unwrap_or_else(|| current_snapshot.url.clone());
        let refreshed = crawl_page_snapshot_with_session(session, &url)?;
        if predicate(&refreshed) {
            let diff = diff_snapshots(&original, &refreshed);
            *current_snapshot = refreshed;
            return Ok(diff);
        }
    }
}

fn wait_for_stable_snapshot(
    session: &mut BrowserSessionState,
    current_snapshot: &mut BrowserPageSnapshot,
    stable_polls: Option<u32>,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
) -> Result<BrowserSnapshotDiff, String> {
    let required_stable = stable_polls.unwrap_or(DEFAULT_STABLE_POLLS).max(1);
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS));
    let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS));
    let started = Instant::now();
    let original = current_snapshot.clone();
    let mut previous = current_snapshot.clone();
    let mut consecutive_stable = 0u32;

    loop {
        if started.elapsed() >= timeout {
            return Err(format!("wait for stable snapshot not satisfied within {}ms", timeout.as_millis()));
        }
        thread::sleep(interval);
        let url = session
            .current_url
            .clone()
            .unwrap_or_else(|| current_snapshot.url.clone());
        let refreshed = crawl_page_snapshot_with_session(session, &url)?;
        let poll_diff = diff_snapshots(&previous, &refreshed);
        if is_semantically_stable(&poll_diff) {
            consecutive_stable += 1;
        } else {
            consecutive_stable = 0;
        }
        previous = refreshed.clone();
        if consecutive_stable >= required_stable {
            let final_diff = diff_snapshots(&original, &refreshed);
            *current_snapshot = refreshed;
            return Ok(final_diff);
        }
    }
}

pub fn wait_for_session(
    workspace_root: &Path,
    session_id: &str,
    text: Option<&str>,
    title: Option<&str>,
    url_contains: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    stable_polls: Option<u32>,
    timeout_ms: Option<u64>,
    interval_ms: Option<u64>,
    sitemap_path: &Path,
) -> Result<String, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    let current_url = session
        .current_url
        .clone()
        .ok_or_else(|| format!("browser session '{}' has no current URL", session_id))?;
    let mut snapshot = load_snapshot_json(&current_url, sitemap_path)
        .unwrap_or_else(|_| crawl_page_snapshot_with_session(&mut session, &current_url).unwrap_or(BrowserPageSnapshot {
            url: current_url.clone(),
            title: "Untitled Page".to_string(),
            summary: String::new(),
            elements: Vec::new(),
            forms: Vec::new(),
            cookies: session.cookies.clone(),
            storage: storage_buckets(&session),
        }));
    let diff = if let Some(wait_text) = text {
        wait_for_condition(&mut session, &mut snapshot, timeout_ms, interval_ms, |candidate| {
            snapshot_contains_text(candidate, wait_text)
        })?
    } else if let Some(wait_title) = title {
        wait_for_condition(&mut session, &mut snapshot, timeout_ms, interval_ms, |candidate| {
            candidate.title.to_ascii_lowercase().contains(&wait_title.to_ascii_lowercase())
        })?
    } else if let Some(wait_fragment) = url_contains {
        wait_for_condition(&mut session, &mut snapshot, timeout_ms, interval_ms, |candidate| {
            candidate.url.contains(wait_fragment)
        })?
    } else if let (Some(wait_role), Some(wait_name)) = (role, name) {
        wait_for_condition(&mut session, &mut snapshot, timeout_ms, interval_ms, |candidate| {
            find_element(candidate, wait_role, wait_name).is_some()
        })?
    } else if stable_polls.is_some() {
        wait_for_stable_snapshot(&mut session, &mut snapshot, stable_polls, timeout_ms, interval_ms)?
    } else {
        return Err("browser_session_wait requires text, title, urlContains, stablePolls, or both role and name".to_string());
    };

    persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &snapshot.url,
        &snapshot.title,
        &snapshot.summary,
        &snapshot.elements,
        &snapshot.forms,
        &snapshot.cookies,
        &snapshot.storage,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
    let session_path = save_session_state(workspace_root, &session)?;

    Ok(format!(
        "Session wait complete.\nSession: {}\nURL: {}\nTitle: {}\nDiff: {}\nLocal storage: {}\nSession storage: {}\nSnapshot JSON: {}\nSession JSON: {}\nNDA Facts: {}",
        session.id,
        snapshot.url,
        snapshot.title,
        render_snapshot_diff(&diff),
        session.local_storage.len(),
        session.session_storage.len(),
        snapshot_path.display(),
        session_path.display(),
        facts_path.display(),
    ))
}

fn extract_snapshot_value(
    snapshot: &BrowserPageSnapshot,
    source: &str,
    role: Option<&str>,
    name: Option<&str>,
    field: Option<&str>,
) -> Result<String, String> {
    if source.eq_ignore_ascii_case("title") {
        return Ok(snapshot.title.clone());
    }
    if source.eq_ignore_ascii_case("summary") {
        return Ok(snapshot.summary.clone());
    }
    if source.eq_ignore_ascii_case("field_value") {
        let field_name = field.ok_or_else(|| "extract_text field is required for source=field_value".to_string())?;
        let matched = find_form_field(snapshot, field_name)
            .ok_or_else(|| format!("workflow extract field not found: '{}'", field_name))?;
        return Ok(matched.value.clone());
    }

    let role = role.ok_or_else(|| format!("extract_text role is required for source='{}'", source))?;
    let name = name.ok_or_else(|| format!("extract_text name is required for source='{}'", source))?;
    let matched = find_element(snapshot, role, name)
        .ok_or_else(|| format!("workflow extract target not found: role='{}' name='{}'", role, name))?;
    if source.eq_ignore_ascii_case("element_name") {
        Ok(matched.name.clone())
    } else if source.eq_ignore_ascii_case("element_value") {
        Ok(matched.value.clone())
    } else if source.eq_ignore_ascii_case("element_url") {
        Ok(matched.target_url.clone().unwrap_or_default())
    } else {
        Err(format!("unsupported workflow extract source: '{}'", source))
    }
}

fn apply_fill_field(state: &mut BrowserReplayState, field_name: &str, value: &str) -> Result<(), String> {
    let best_field_name = find_form_field(&state.snapshot, field_name).map(|field| field.name.clone());
    let best_element_name = state
        .snapshot
        .elements
        .iter()
        .filter(|element| element.role.eq_ignore_ascii_case("textbox"))
        .filter_map(|element| string_match_score(&element.name, field_name).map(|score| (score, element.name.clone())))
        .max_by(|(left_score, left_name), (right_score, right_name)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_name.len().cmp(&left_name.len()))
        })
        .map(|(_, name)| name);

    let mut matched = false;
    for form in &mut state.snapshot.forms {
        for field in &mut form.fields {
            if best_field_name.as_deref() == Some(field.name.as_str()) {
                field.value = value.to_string();
                state.filled_fields.insert(field.name.clone(), value.to_string());
                matched = true;
            }
        }
    }
    for element in &mut state.snapshot.elements {
        if element.role.eq_ignore_ascii_case("textbox")
            && best_element_name.as_deref() == Some(element.name.as_str())
        {
            element.value = value.to_string();
            matched = true;
        }
    }
    if matched {
        Ok(())
    } else {
        Err(format!("workflow fill target not found: '{}'", field_name))
    }
}

fn submit_current_form(state: &mut BrowserReplayState, form_id: Option<&str>) -> Result<(), String> {
    let form = find_form(&state.snapshot, form_id)
        .cloned()
        .ok_or_else(|| match form_id {
            Some(id) => format!("workflow submit target form not found: '{}'", id),
            None => "workflow submit target form not found".to_string(),
        })?;

    let mut encoded_pairs = Vec::new();
    for field in &form.fields {
        let value = state
            .filled_fields
            .get(&field.name)
            .cloned()
            .unwrap_or_else(|| field.value.clone());
        encoded_pairs.push(format!("{}={}", url_encode(&field.name), url_encode(&value)));
    }
    let payload = encoded_pairs.join("&");
    let target_url = if form.method.eq_ignore_ascii_case("GET") && !payload.is_empty() {
        if form.action.contains('?') {
            format!("{}&{}", form.action, payload)
        } else {
            format!("{}?{}", form.action, payload)
        }
    } else {
        form.action.clone()
    };

    let response = if form.method.eq_ignore_ascii_case("POST") {
        fetch_with_session(&target_url, "POST", Some(&payload), &state.session.cookies)?
    } else {
        fetch_with_session(&target_url, "GET", None, &state.session.cookies)?
    };

    for cookie in response.cookies.iter().cloned() {
        merge_cookie(&mut state.session.cookies, cookie);
    }
    apply_storage_updates(&mut state.session.local_storage, &response.local_storage_updates);
    apply_storage_updates(&mut state.session.session_storage, &response.session_storage_updates);
    state.session.current_url = Some(target_url.clone());
    let storage = storage_buckets(&state.session);
    state.snapshot = parse_html_to_snapshot(&target_url, &response.html, &state.session.cookies, &storage);
    state.filled_fields.clear();
    Ok(())
}

pub fn crawl_and_sync_sitemap(url: &str, sitemap_path: &Path) -> Result<String, String> {
    let snapshot = crawl_page_snapshot(url)?;
    persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &snapshot.url,
        &snapshot.title,
        &snapshot.summary,
        &snapshot.elements,
        &snapshot.forms,
        &snapshot.cookies,
        &snapshot.storage,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;

    Ok(format!(
        "Crawler finished.\nURL: {}\nTitle: {}\nInteractive Elements: {}\nForms: {}\nCookies: {}\nRegistered in Merkle SiteMap at {:?}\nSnapshot JSON: {:?}\nNDA Facts: {:?}",
        snapshot.url,
        snapshot.title,
        snapshot.elements.len(),
        snapshot.forms.len(),
        snapshot.cookies.len(),
        sitemap_path,
        snapshot_path,
        facts_path,
    ))
}

fn render_workflow_step_lines(lines: &mut Vec<String>, step: &BrowserWorkflowStep, prefix: &str) {
    match step {
        BrowserWorkflowStep::Navigate { url } => {
            lines.push(format!("{}\tnavigate\t{}", prefix, encode_nda_text(url)));
        }
        BrowserWorkflowStep::Click { role, name } => {
            lines.push(format!(
                "{}\tclick\trole={}\tname={}"
                ,prefix,
                encode_nda_text(role),
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::FillField { field, value } => {
            lines.push(format!(
                "{}\tfill_field\tfield={}\tvalue={}"
                ,prefix,
                encode_nda_text(field),
                encode_nda_text(value)
            ));
        }
        BrowserWorkflowStep::SubmitForm { form } => {
            lines.push(format!(
                "{}\tsubmit_form\tform={}"
                ,prefix,
                encode_nda_text(form.as_deref().unwrap_or("default"))
            ));
        }
        BrowserWorkflowStep::WaitForText {
            text,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_text\ttext={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(text),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForElement {
            role,
            name,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_element\trole={}\tname={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(role),
                encode_nda_text(name),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForTitle {
            title,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_title\ttitle={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(title),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForUrlContains {
            fragment,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_url_contains\tfragment={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                encode_nda_text(fragment),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::WaitForStable {
            stable_polls,
            timeout_ms,
            interval_ms,
        } => {
            lines.push(format!(
                "{}\twait_for_stable\tstable_polls={}\ttimeout_ms={}\tinterval_ms={}"
                ,prefix,
                stable_polls.unwrap_or(DEFAULT_STABLE_POLLS),
                timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            ));
        }
        BrowserWorkflowStep::ExtractText {
            output,
            source,
            role,
            name,
            field,
        } => {
            lines.push(format!(
                "{}\textract_text\toutput={}\tsource={}\trole={}\tname={}\tfield={}"
                ,prefix,
                encode_nda_text(output),
                encode_nda_text(source),
                encode_nda_text(role.as_deref().unwrap_or_default()),
                encode_nda_text(name.as_deref().unwrap_or_default()),
                encode_nda_text(field.as_deref().unwrap_or_default())
            ));
        }
        BrowserWorkflowStep::SaveCheckpoint { name } => {
            lines.push(format!("{}\tsave_checkpoint\tname={}", prefix, encode_nda_text(name)));
        }
        BrowserWorkflowStep::RestoreCheckpoint { name } => {
            lines.push(format!("{}\trestore_checkpoint\tname={}", prefix, encode_nda_text(name)));
        }
        BrowserWorkflowStep::IfTextContains {
            text,
            then_steps,
            else_steps,
        } => {
            lines.push(format!("{}\tif_text_contains\ttext={}", prefix, encode_nda_text(text)));
            for (idx, nested) in then_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:then:{}", prefix, idx));
            }
            for (idx, nested) in else_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:else:{}", prefix, idx));
            }
        }
        BrowserWorkflowStep::IfOutputEquals {
            output,
            equals,
            then_steps,
            else_steps,
        } => {
            lines.push(format!(
                "{}\tif_output_equals\toutput={}\tequals={}"
                ,prefix,
                encode_nda_text(output),
                encode_nda_text(equals)
            ));
            for (idx, nested) in then_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:then:{}", prefix, idx));
            }
            for (idx, nested) in else_steps.iter().enumerate() {
                render_workflow_step_lines(lines, nested, &format!("{}:else:{}", prefix, idx));
            }
        }
        BrowserWorkflowStep::AssertElement { role, name } => {
            lines.push(format!(
                "{}\tassert_element\trole={}\tname={}"
                ,prefix,
                encode_nda_text(role),
                encode_nda_text(name)
            ));
        }
        BrowserWorkflowStep::AssertTextContains { text } => {
            lines.push(format!("{}\tassert_text\t{}", prefix, encode_nda_text(text)));
        }
        BrowserWorkflowStep::AssertOutput {
            output,
            equals,
            contains,
        } => {
            lines.push(format!(
                "{}\tassert_output\toutput={}\tequals={}\tcontains={}"
                ,prefix,
                encode_nda_text(output),
                encode_nda_text(equals.as_deref().unwrap_or_default()),
                encode_nda_text(contains.as_deref().unwrap_or_default())
            ));
        }
    }
}

pub fn render_workflow_dsl(workflow: &BrowserWorkflow) -> String {
    let mut lines = vec![
        "browser-workflow version 2".to_string(),
        format!("name\t{}", encode_nda_text(&workflow.name)),
        format!("start_url\t{}", encode_nda_text(&workflow.start_url)),
    ];

    for (idx, step) in workflow.steps.iter().enumerate() {
        let prefix = format!("step\t{}", idx);
        render_workflow_step_lines(&mut lines, step, &prefix);
    }

    lines.join("\n") + "\n"
}

pub fn save_workflow(workspace_root: &Path, workflow: &BrowserWorkflow) -> Result<(PathBuf, PathBuf), String> {
    let json_path = browser_workflow_json_path(workspace_root, &workflow.name);
    let nda_path = browser_workflow_nda_path(workspace_root, &workflow.name);
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create workflow dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(workflow).map_err(|err| format!("serialise workflow: {err}"))?;
    fs::write(&json_path, json).map_err(|err| format!("write workflow json: {err}"))?;
    fs::write(&nda_path, render_workflow_dsl(workflow)).map_err(|err| format!("write workflow nda: {err}"))?;
    Ok((json_path, nda_path))
}

pub fn load_workflow(path: &Path) -> Result<BrowserWorkflow, String> {
    let raw = fs::read(path).map_err(|err| format!("read workflow: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse workflow: {err}"))
}

pub fn save_workflow_suite(workspace_root: &Path, suite: &BrowserWorkflowSuite) -> Result<PathBuf, String> {
    let path = browser_workflow_suite_json_path(workspace_root, &suite.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create workflow suite dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(suite).map_err(|err| format!("serialise workflow suite: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write workflow suite: {err}"))?;
    Ok(path)
}

pub fn load_workflow_suite(path: &Path) -> Result<BrowserWorkflowSuite, String> {
    let raw = fs::read(path).map_err(|err| format!("read workflow suite: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse workflow suite: {err}"))
}

fn execute_workflow_steps(
    steps: &[BrowserWorkflowStep],
    state: &mut BrowserReplayState,
    log: &mut Vec<String>,
    checkpoints: &mut HashMap<String, BrowserReplayState>,
    workspace_root: Option<&Path>,
) -> Result<(), String> {
    for step in steps {
        match step {
            BrowserWorkflowStep::Navigate { url } => {
                let resolved_url = resolve_template(url, state);
                state.snapshot = crawl_page_snapshot_with_session(&mut state.session, &resolved_url)?;
                log.push(format!("navigate {} -> {}", resolved_url, state.snapshot.title));
            }
            BrowserWorkflowStep::Click { role, name } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                let target = find_element(&state.snapshot, &resolved_role, &resolved_name)
                    .ok_or_else(|| format!("workflow click target not found: role='{}' name='{}'", resolved_role, resolved_name))?;
                let target_url = target.target_url.clone().ok_or_else(|| {
                    format!(
                        "workflow click target '{}' is not a navigable link in the current static browser engine",
                        resolved_name
                    )
                })?;
                state.snapshot = crawl_page_snapshot_with_session(&mut state.session, &target_url)?;
                log.push(format!("click {}:{} -> {}", resolved_role, resolved_name, state.snapshot.title));
            }
            BrowserWorkflowStep::FillField { field, value } => {
                let resolved_field = resolve_template(field, state);
                let resolved_value = resolve_template(value, state);
                apply_fill_field(state, &resolved_field, &resolved_value)?;
                log.push(format!("fill_field {} ok", resolved_field));
            }
            BrowserWorkflowStep::SubmitForm { form } => {
                let resolved_form = form.as_ref().map(|value| resolve_template(value, state));
                submit_current_form(state, resolved_form.as_deref())?;
                log.push(format!(
                    "submit_form {} -> {}",
                    resolved_form.as_deref().unwrap_or("default"),
                    state.snapshot.title
                ));
            }
            BrowserWorkflowStep::WaitForText {
                text,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_text = resolve_template(text, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| snapshot_contains_text(candidate, &resolved_text),
                )?;
                log.push(format!("wait_for_text '{}' -> {}", resolved_text, render_snapshot_diff(&diff)));
            }
            BrowserWorkflowStep::WaitForElement {
                role,
                name,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| find_element(candidate, &resolved_role, &resolved_name).is_some(),
                )?;
                log.push(format!(
                    "wait_for_element {}:{} -> {}",
                    resolved_role,
                    resolved_name,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForTitle {
                title,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_title = resolve_template(title, state);
                let lowered = resolved_title.to_ascii_lowercase();
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| candidate.title.to_ascii_lowercase().contains(&lowered),
                )?;
                log.push(format!("wait_for_title '{}' -> {}", resolved_title, render_snapshot_diff(&diff)));
            }
            BrowserWorkflowStep::WaitForUrlContains {
                fragment,
                timeout_ms,
                interval_ms,
            } => {
                let resolved_fragment = resolve_template(fragment, state);
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| candidate.url.contains(&resolved_fragment),
                )?;
                log.push(format!(
                    "wait_for_url_contains '{}' -> {}",
                    resolved_fragment,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::WaitForStable {
                stable_polls,
                timeout_ms,
                interval_ms,
            } => {
                let diff = wait_for_stable_snapshot(
                    &mut state.session,
                    &mut state.snapshot,
                    *stable_polls,
                    *timeout_ms,
                    *interval_ms,
                )?;
                log.push(format!(
                    "wait_for_stable polls={} -> {}",
                    stable_polls.unwrap_or(DEFAULT_STABLE_POLLS),
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::ExtractText {
                output,
                source,
                role,
                name,
                field,
            } => {
                let resolved_source = resolve_template(source, state);
                let resolved_role = role.as_ref().map(|value| resolve_template(value, state));
                let resolved_name = name.as_ref().map(|value| resolve_template(value, state));
                let resolved_field = field.as_ref().map(|value| resolve_template(value, state));
                let extracted = extract_snapshot_value(
                    &state.snapshot,
                    &resolved_source,
                    resolved_role.as_deref(),
                    resolved_name.as_deref(),
                    resolved_field.as_deref(),
                )?;
                state.outputs.insert(output.clone(), extracted.clone());
                log.push(format!("extract_text {}='{}'", output, truncate_string(&extracted, 80)));
            }
            BrowserWorkflowStep::SaveCheckpoint { name } => {
                let resolved_name = resolve_template(name, state);
                checkpoints.insert(resolved_name.clone(), state.clone());
                if let Some(root) = workspace_root {
                    let path = persist_checkpoint_from_replay_state(root, state, &resolved_name)?;
                    log.push(format!("save_checkpoint {} -> {}", resolved_name, path.display()));
                } else {
                    log.push(format!("save_checkpoint {} ok", resolved_name));
                }
            }
            BrowserWorkflowStep::RestoreCheckpoint { name } => {
                let resolved_name = resolve_template(name, state);
                let restored = checkpoints
                    .get(&resolved_name)
                    .cloned()
                    .ok_or_else(|| format!("workflow restore checkpoint not found: '{}'", resolved_name))?;
                *state = restored;
                log.push(format!("restore_checkpoint {} -> {}", resolved_name, state.snapshot.title));
            }
            BrowserWorkflowStep::IfTextContains {
                text,
                then_steps,
                else_steps,
            } => {
                let resolved_text = resolve_template(text, state);
                let matched = snapshot_contains_text(&state.snapshot, &resolved_text);
                log.push(format!("if_text_contains '{}' -> {}", resolved_text, if matched { "then" } else { "else" }));
                let branch = if matched { then_steps } else { else_steps };
                execute_workflow_steps(branch, state, log, checkpoints, workspace_root)?;
            }
            BrowserWorkflowStep::IfOutputEquals {
                output,
                equals,
                then_steps,
                else_steps,
            } => {
                let actual = state.outputs.get(output).cloned().unwrap_or_default();
                let resolved_expected = resolve_template(equals, state);
                let matched = actual == resolved_expected;
                log.push(format!("if_output_equals {}='{}' -> {}", output, truncate_string(&actual, 80), if matched { "then" } else { "else" }));
                let branch = if matched { then_steps } else { else_steps };
                execute_workflow_steps(branch, state, log, checkpoints, workspace_root)?;
            }
            BrowserWorkflowStep::AssertElement { role, name } => {
                let resolved_role = resolve_template(role, state);
                let resolved_name = resolve_template(name, state);
                find_element(&state.snapshot, &resolved_role, &resolved_name).ok_or_else(|| {
                    format!("workflow assertion failed: missing element role='{}' name='{}'", resolved_role, resolved_name)
                })?;
                log.push(format!("assert_element {}:{} ok", resolved_role, resolved_name));
            }
            BrowserWorkflowStep::AssertTextContains { text } => {
                let resolved_text = resolve_template(text, state);
                if !snapshot_contains_text(&state.snapshot, &resolved_text) {
                    return Err(format!("workflow assertion failed: text '{}' not present", resolved_text));
                }
                log.push(format!("assert_text '{}' ok", resolved_text));
            }
            BrowserWorkflowStep::AssertOutput {
                output,
                equals,
                contains,
            } => {
                let actual = state
                    .outputs
                    .get(output)
                    .cloned()
                    .ok_or_else(|| format!("workflow assertion failed: output '{}' not present", output))?;
                if let Some(expected) = equals {
                    let resolved_expected = resolve_template(expected, state);
                    if actual != resolved_expected {
                        return Err(format!("workflow assertion failed: output '{}' expected '{}' but was '{}'", output, resolved_expected, actual));
                    }
                }
                if let Some(expected_fragment) = contains {
                    let resolved_fragment = resolve_template(expected_fragment, state);
                    if !actual.contains(&resolved_fragment) {
                        return Err(format!("workflow assertion failed: output '{}' does not contain '{}'", output, resolved_fragment));
                    }
                }
                log.push(format!("assert_output {} ok", output));
            }
        }
    }
    Ok(())
}

fn replay_workflow_with_state(
    workflow: &BrowserWorkflow,
    mut state: BrowserReplayState,
    workspace_root: Option<&Path>,
) -> Result<(String, BrowserReplayState, BrowserWorkflowRunReport), String> {
    let mut log = vec![format!("start {} -> {}", state.snapshot.url, state.snapshot.title)];
    let mut checkpoints = HashMap::new();
    execute_workflow_steps(&workflow.steps, &mut state, &mut log, &mut checkpoints, workspace_root)?;

    let report = BrowserWorkflowRunReport {
        workflow_name: workflow.name.clone(),
        session_id: state.session.id.clone(),
        final_url: state.snapshot.url.clone(),
        final_title: state.snapshot.title.clone(),
        step_count: workflow.steps.len(),
        cookie_count: state.session.cookies.len(),
        local_storage_count: state.session.local_storage.len(),
        session_storage_count: state.session.session_storage.len(),
        outputs: state.outputs.clone(),
        log: log.clone(),
    };
    let result = format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSession: {}\nSteps executed: {}\nCookies: {}\nLocal storage: {}\nSession storage: {}\nOutputs: {}\n{}",
        workflow.name,
        state.snapshot.url,
        state.snapshot.title,
        state.session.id,
        workflow.steps.len(),
        state.session.cookies.len(),
        state.session.local_storage.len(),
        state.session.session_storage.len(),
        state.outputs.len(),
        log.join("\n")
    );
    Ok((result, state, report))
}

fn persist_replay_state(
    workspace_root: &Path,
    state: &BrowserReplayState,
    sitemap_path: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    persist_snapshot_to_sitemap(&state.snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &state.snapshot.url,
        &state.snapshot.title,
        &state.snapshot.summary,
        &state.snapshot.elements,
        &state.snapshot.forms,
        &state.snapshot.cookies,
        &state.snapshot.storage,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&state.snapshot, sitemap_path)?;
    let session_path = save_session_state(workspace_root, &state.session)?;
    Ok((snapshot_path, session_path, facts_path))
}

fn persist_run_report(
    workspace_root: &Path,
    report: &BrowserWorkflowRunReport,
) -> Result<PathBuf, String> {
    let path = browser_workflow_run_path(workspace_root, &report.workflow_name, &report.session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser run dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(report).map_err(|err| format!("serialise browser run report: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser run report: {err}"))?;
    Ok(path)
}

fn persist_suite_run_report(
    workspace_root: &Path,
    report: &BrowserWorkflowSuiteRunReport,
) -> Result<PathBuf, String> {
    let path = browser_workflow_suite_run_path(workspace_root, &report.suite_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser suite run dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(report).map_err(|err| format!("serialise browser suite run report: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write browser suite run report: {err}"))?;
    Ok(path)
}

pub fn replay_workflow(workflow: &BrowserWorkflow) -> Result<String, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, _, _) = replay_workflow_with_state(workflow, state, None)?;
    Ok(result)
}

pub fn replay_workflow_with_artifacts(
    workspace_root: &Path,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<String, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, final_state, report) = replay_workflow_with_state(workflow, state, Some(workspace_root))?;
    let (snapshot_path, session_path, facts_path) = persist_replay_state(workspace_root, &final_state, sitemap_path)?;
    let report_path = persist_run_report(workspace_root, &report)?;
    Ok(format!(
        "{}\nSnapshot JSON: {}\nSession JSON: {}\nNDA Facts: {}\nRun Report: {}",
        result,
        snapshot_path.display(),
        session_path.display(),
        facts_path.display(),
        report_path.display()
    ))
}

pub fn run_workflow_suite(
    workspace_root: &Path,
    suite: &BrowserWorkflowSuite,
    sitemap_path: &Path,
) -> Result<String, String> {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut items = Vec::with_capacity(suite.workflows.len());

    for workflow_path in &suite.workflows {
        let full_path = workspace_root.join(workflow_path);
        match load_workflow(&full_path) {
            Ok(workflow) => match replay_workflow_with_artifacts(workspace_root, &workflow, sitemap_path) {
                Ok(summary) => {
                    passed += 1;
                    items.push(BrowserWorkflowSuiteRunItem {
                        workflow_path: workflow_path.clone(),
                        workflow_name: workflow.name,
                        status: "passed".to_string(),
                        summary: truncate_string(&summary, 400),
                    });
                }
                Err(err) => {
                    failed += 1;
                    items.push(BrowserWorkflowSuiteRunItem {
                        workflow_path: workflow_path.clone(),
                        workflow_name: workflow.name,
                        status: "failed".to_string(),
                        summary: err,
                    });
                }
            },
            Err(err) => {
                failed += 1;
                items.push(BrowserWorkflowSuiteRunItem {
                    workflow_path: workflow_path.clone(),
                    workflow_name: workflow_path.clone(),
                    status: "failed".to_string(),
                    summary: err,
                });
            }
        }
    }

    let report = BrowserWorkflowSuiteRunReport {
        suite_name: suite.name.clone(),
        total: suite.workflows.len(),
        passed,
        failed,
        items,
    };
    let report_path = persist_suite_run_report(workspace_root, &report)?;
    Ok(format!(
        "Workflow suite '{}' completed.\nTotal: {}\nPassed: {}\nFailed: {}\nSuite Report: {}",
        suite.name,
        report.total,
        report.passed,
        report.failed,
        report_path.display()
    ))
}

pub fn replay_workflow_in_session(
    workspace_root: &Path,
    session_id: &str,
    workflow: &BrowserWorkflow,
    sitemap_path: &Path,
) -> Result<String, String> {
    let mut session = load_session_state(workspace_root, session_id)?;
    let snapshot = match session.current_url.clone() {
        Some(url) => load_snapshot_json(&url, sitemap_path)
            .or_else(|_| crawl_page_snapshot_with_session(&mut session, &url)),
        None => crawl_page_snapshot_with_session(&mut session, &workflow.start_url),
    }?;
    session.id = session_id.to_string();
    let state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
        variables: workflow.variables.clone(),
        outputs: HashMap::new(),
    };
    let (result, final_state, report) = replay_workflow_with_state(workflow, state, Some(workspace_root))?;
    let (snapshot_path, session_path, facts_path) =
        persist_replay_state(workspace_root, &final_state, sitemap_path)?;
    let report_path = persist_run_report(workspace_root, &report)?;
    Ok(format!(
        "{}\nSnapshot JSON: {}\nSession JSON: {}\nNDA Facts: {}\nRun Report: {}",
        result,
        snapshot_path.display(),
        session_path.display(),
        facts_path.display(),
        report_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        crawl_facts_path, create_session, diff_snapshots, load_session_state, load_workflow,
        load_snapshot_json, load_workflow_suite, navigate_session, parse_html_to_snapshot,
        render_workflow_dsl, replay_workflow, replay_workflow_in_session, restore_session_checkpoint,
        run_workflow_suite, save_session_checkpoint, save_workflow, save_workflow_suite,
        wait_for_session, write_crawl_facts, AomElement, BrowserCookie, BrowserWorkflow,
        BrowserWorkflowStep, BrowserWorkflowSuite,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    fn read_http_request(stream: &mut TcpStream) -> String {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        let mut expected_total = None;

        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    data.extend_from_slice(&buf[..read]);
                    if expected_total.is_none() {
                        if let Some(header_end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
                            let headers_end = header_end + 4;
                            let headers = String::from_utf8_lossy(&data[..headers_end]);
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    let lower = line.to_ascii_lowercase();
                                    lower
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            expected_total = Some(headers_end + content_length);
                        }
                    }
                    if let Some(total) = expected_total {
                        if data.len() >= total {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        String::from_utf8_lossy(&data).to_string()
    }

    #[test]
    fn writes_browser_capture_facts() {
        let temp = tempfile::tempdir().unwrap();
        let sitemap_path = temp.path().join("site_map");
        let facts_path = write_crawl_facts(
            "https://example.com/docs",
            "Docs",
            "Documentation landing page",
            &[
                AomElement {
                    role: "link".to_string(),
                    name: "API".to_string(),
                    value: "https://example.com/api".to_string(),
                    target_url: Some("https://example.com/api".to_string()),
                    supported_actions: vec!["open".to_string(), "click".to_string()],
                    provenance: "native-static".to_string(),
                    actionability: super::role_actionability("link"),
                },
                AomElement {
                    role: "button".to_string(),
                    name: "Search".to_string(),
                    value: String::new(),
                    target_url: None,
                    supported_actions: vec!["click".to_string()],
                    provenance: "native-static".to_string(),
                    actionability: super::role_actionability("button"),
                },
            ],
            &[],
            &[BrowserCookie { name: "sid".to_string(), value: "123".to_string() }],
            &[super::BrowserStorageBucket {
                scope: "local".to_string(),
                entries: HashMap::from([("theme".to_string(), "dark".to_string())]),
            }],
            &sitemap_path,
        )
        .unwrap();

        assert_eq!(facts_path, crawl_facts_path("https://example.com/docs", &sitemap_path));
        let facts = fs::read_to_string(facts_path).unwrap();
        assert!(facts.starts_with("browser-capture version 5\n"));
        assert!(facts.contains("field_count 5\n"));
        assert!(facts.contains("field\tcookie_count\t1\n"));
        assert!(facts.contains("field\tstorage_entry_count\t1\n"));
        assert!(facts.contains("element_field\t0\trole\tlink"));
        assert!(facts.contains("cookie_field\t0\tname\tsid"));
        assert!(facts.contains("storage_field\t0\tscope\tlocal"));
    }

    #[test]
    fn parses_html_into_snapshot_with_forms() {
        let snapshot = parse_html_to_snapshot(
            "https://example.com",
            "<html><head><title>Docs</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input name='password' type='password'><input type='submit' value='Sign in'></form><a href='/api'>API</a></body></html>",
            &[],
            &[],
        );
        assert_eq!(snapshot.title, "Docs");
        assert_eq!(snapshot.forms.len(), 1);
        assert_eq!(snapshot.forms[0].id, "login");
        assert_eq!(snapshot.forms[0].fields.len(), 2);
        assert_eq!(snapshot.forms[0].submit_label.as_deref(), Some("Sign in"));
    }

    #[test]
    fn saves_and_loads_browser_workflow_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let workflow = BrowserWorkflow {
            name: "Checkout Smoke".to_string(),
            start_url: "https://example.com".to_string(),
            variables: HashMap::new(),
            steps: vec![
                BrowserWorkflowStep::FillField { field: "email".to_string(), value: "a@example.com".to_string() },
                BrowserWorkflowStep::WaitForText {
                    text: "Confirm".to_string(),
                    timeout_ms: Some(1500),
                    interval_ms: Some(50),
                },
                BrowserWorkflowStep::SubmitForm { form: None },
            ],
        };

        let (json_path, nda_path) = save_workflow(temp.path(), &workflow).unwrap();
        assert!(json_path.exists());
        assert!(nda_path.exists());
        let loaded = load_workflow(&json_path).unwrap();
        assert_eq!(loaded, workflow);
        let dsl = render_workflow_dsl(&workflow);
        assert!(dsl.contains("browser-workflow version 2"));
        assert!(dsl.contains("fill_field"));
        assert!(dsl.contains("wait_for_text"));
        assert!(dsl.contains("submit_form"));
    }

    #[test]
    fn replays_branching_and_checkpoints_deterministically() {
        let workflow = BrowserWorkflow {
            name: "Branching Flow".to_string(),
            start_url: "https://example.com".to_string(),
            variables: HashMap::new(),
            steps: vec![
                BrowserWorkflowStep::ExtractText {
                    output: "page_title".to_string(),
                    source: "title".to_string(),
                    role: None,
                    name: None,
                    field: None,
                },
                BrowserWorkflowStep::SaveCheckpoint { name: "initial".to_string() },
                BrowserWorkflowStep::IfOutputEquals {
                    output: "page_title".to_string(),
                    equals: "Checkout".to_string(),
                    then_steps: vec![BrowserWorkflowStep::FillField {
                        field: "email".to_string(),
                        value: "branch@example.com".to_string(),
                    }],
                    else_steps: vec![BrowserWorkflowStep::AssertTextContains { text: "Never".to_string() }],
                },
                BrowserWorkflowStep::RestoreCheckpoint { name: "initial".to_string() },
                BrowserWorkflowStep::IfTextContains {
                    text: "Checkout".to_string(),
                    then_steps: vec![BrowserWorkflowStep::AssertTextContains { text: "Checkout".to_string() }],
                    else_steps: vec![BrowserWorkflowStep::AssertTextContains { text: "Never".to_string() }],
                },
            ],
        };
        let snapshot = parse_html_to_snapshot(
            "https://example.com",
            "<html><head><title>Checkout</title></head><body><form id='login'><input name='email' placeholder='Email'></form><p>Checkout ready</p></body></html>",
            &[],
            &[],
        );
        let state = super::BrowserReplayState {
            session: super::BrowserSessionState {
                id: "branch-session".to_string(),
                current_url: Some("https://example.com".to_string()),
                cookies: Vec::new(),
                local_storage: HashMap::new(),
                session_storage: HashMap::new(),
            },
            snapshot,
            filled_fields: HashMap::new(),
            variables: HashMap::new(),
            outputs: HashMap::new(),
        };

        let (summary, final_state, report) = super::replay_workflow_with_state(&workflow, state, None).unwrap();
        assert!(summary.contains("Workflow 'Branching Flow' completed."));
        assert!(summary.contains("Outputs: 1"));
        assert_eq!(final_state.outputs.get("page_title").map(String::as_str), Some("Checkout"));
        assert!(report.log.iter().any(|entry| entry.contains("if_output_equals page_title='Checkout' -> then")));
        assert!(report.log.iter().any(|entry| entry.contains("restore_checkpoint initial -> Checkout")));
        assert!(report.log.iter().any(|entry| entry.contains("if_text_contains 'Checkout' -> then")));
    }

    #[test]
    fn saves_and_runs_browser_workflow_suite() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.starts_with("POST /login") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let workflow = BrowserWorkflow {
            name: "Login Flow".to_string(),
            start_url: base_url,
            variables: HashMap::new(),
            steps: vec![
                BrowserWorkflowStep::FillField { field: "email".to_string(), value: "rust@example.com".to_string() },
                BrowserWorkflowStep::SubmitForm { form: Some("login".to_string()) },
                BrowserWorkflowStep::AssertTextContains { text: "Welcome back".to_string() },
            ],
        };
        let (workflow_path, _) = save_workflow(root, &workflow).unwrap();
        let suite = BrowserWorkflowSuite {
            name: "Smoke Pack".to_string(),
            workflows: vec![
                workflow_path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"),
                ".velocity/browser-workflows/missing.browser.json".to_string(),
            ],
        };
        let suite_path = save_workflow_suite(root, &suite).unwrap();
        let loaded = load_workflow_suite(&suite_path).unwrap();
        assert_eq!(loaded, suite);

        let sitemap_path = root.join("site_map");
        let summary = run_workflow_suite(root, &suite, &sitemap_path).unwrap();
        assert!(summary.contains("Workflow suite 'Smoke Pack' completed."));
        assert!(summary.contains("Total: 2"));
        assert!(summary.contains("Passed: 1"));
        assert!(summary.contains("Failed: 1"));
        assert!(summary.contains("Suite Report:"));
    }

    #[test]
    fn ranks_semantic_element_and_field_matches() {
        let snapshot = parse_html_to_snapshot(
            "https://example.com/app",
            "<html><head><title>Portal</title></head><body><a href='/settings/billing'>Billing Settings</a><a href='/settings'>Settings</a><form id='profile'><input name='user_email' placeholder='Work Email'></form></body></html>",
            &[],
            &[],
        );

        let matched_link = super::find_element(&snapshot, "link", "billing").unwrap();
        assert_eq!(matched_link.name, "Billing Settings");
        assert_eq!(matched_link.target_url.as_deref(), Some("https://example.com/settings/billing"));
        assert!(matched_link.supported_actions.iter().any(|action| action == "open"));
        assert_eq!(matched_link.provenance, "native-static");
        assert!(matched_link.actionability >= 80);

        let matched_field = super::find_form_field(&snapshot, "email").unwrap();
        assert_eq!(matched_field.name, "user_email");
    }

    #[test]
    fn extracts_semantic_values_from_snapshot_sources() {
        let snapshot = parse_html_to_snapshot(
            "https://example.com",
            "<html><head><title>Docs</title></head><body><form id='login' action='/login' method='post'><input name='email' value='saved@example.com' placeholder='Email'></form><a href='/api'>API</a></body></html>",
            &[],
            &[],
        );
        let title = super::extract_snapshot_value(&snapshot, "title", None, None, None).unwrap();
        let link_name = super::extract_snapshot_value(&snapshot, "element_name", Some("link"), Some("API"), None).unwrap();
        let field_value = super::extract_snapshot_value(&snapshot, "field_value", None, None, Some("email")).unwrap();
        assert_eq!(title, "Docs");
        assert_eq!(link_name, "API");
        assert_eq!(field_value, "saved@example.com");
    }

    #[test]
    fn replays_browser_workflow_over_local_pages_with_form_submit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.starts_with("POST /login") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let mut variables = HashMap::new();
        variables.insert("email".to_string(), "rust@example.com".to_string());
        let workflow = BrowserWorkflow {
            name: "Login Flow".to_string(),
            start_url: base_url,
            variables,
            steps: vec![
                BrowserWorkflowStep::FillField { field: "email".to_string(), value: "{{email}}".to_string() },
                BrowserWorkflowStep::SubmitForm { form: Some("login".to_string()) },
                BrowserWorkflowStep::ExtractText {
                    output: "page_title".to_string(),
                    source: "title".to_string(),
                    role: None,
                    name: None,
                    field: None,
                },
                BrowserWorkflowStep::AssertOutput {
                    output: "page_title".to_string(),
                    equals: Some("Dashboard".to_string()),
                    contains: None,
                },
                BrowserWorkflowStep::AssertTextContains { text: "Welcome back".to_string() },
            ],
        };

        let result = replay_workflow(&workflow).unwrap();
        assert!(result.contains("Workflow 'Login Flow' completed."));
        assert!(result.contains("Final title: Dashboard"));
        assert!(result.contains("Cookies: 1"));
        assert!(result.contains("Outputs: 1"));
        assert!(result.contains("extract_text page_title='Dashboard'"));
    }

    #[test]
    fn computes_browser_snapshot_diffs() {
        let before = parse_html_to_snapshot(
            "https://example.com",
            "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'></form></body></html>",
            &[],
            &[],
        );
        let after = parse_html_to_snapshot(
            "https://example.com/dashboard",
            "<html><head><title>Dashboard</title></head><body><a href='/reports'>Reports</a></body></html>",
            &[BrowserCookie { name: "session".to_string(), value: "abc123".to_string() }],
            &[super::BrowserStorageBucket {
                scope: "local".to_string(),
                entries: HashMap::from([("token".to_string(), "abc123".to_string())]),
            }],
        );

        let diff = diff_snapshots(&before, &after);
        assert!(diff.title_changed);
        assert!(diff.added_elements.iter().any(|entry| entry.contains("link:Reports")));
        assert!(diff.removed_forms.iter().any(|entry| entry.contains("login:POST")));
        assert!(diff.added_cookies.iter().any(|entry| entry == "session=abc123"));
        assert!(diff.added_storage.iter().any(|entry| entry == "local:token=abc123"));
    }

    #[test]
    fn waits_for_session_text_with_polling() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        std::thread::spawn(move || {
            for idx in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = if idx == 0 {
                        "<html><head><title>Loading</title></head><body><p>Preparing dashboard</p></body></html>"
                    } else {
                        "<html><head><title>Dashboard</title></head><body><p>Ready</p></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sitemap_path = root.join("site_map");
        create_session(root, "wait-session").unwrap();
        navigate_session(root, "wait-session", &url, &sitemap_path).unwrap();
        let baseline = load_snapshot_json(&url, &sitemap_path).unwrap();
        assert_eq!(baseline.title, "Loading");

        let result = wait_for_session(
            root,
            "wait-session",
            Some("Ready"),
            None,
            None,
            None,
            None,
            None,
            Some(1500),
            Some(10),
            &sitemap_path,
        )
        .unwrap();
        assert!(result.contains("Session wait complete."));
        assert!(result.contains("Title: Dashboard"));
        assert!(result.contains("Diff: title,summary"));
    }

    #[test]
    fn waits_for_session_title_and_stability() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        std::thread::spawn(move || {
            for idx in 0..5 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = read_http_request(&mut stream);
                    let body = match idx {
                        0 => "<html><head><title>Loading</title></head><body><p>Preparing</p></body></html>",
                        1 => "<html><head><title>Dashboard Ready</title></head><body><p>Preparing</p></body></html>",
                        _ => "<html><head><title>Dashboard Ready</title></head><body><p>Stable</p></body></html>",
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sitemap_path = root.join("site_map");
        create_session(root, "title-session").unwrap();
        navigate_session(root, "title-session", &url, &sitemap_path).unwrap();

        let title_result = wait_for_session(
            root,
            "title-session",
            None,
            Some("Dashboard"),
            None,
            None,
            None,
            None,
            Some(1500),
            Some(10),
            &sitemap_path,
        )
        .unwrap();
        assert!(title_result.contains("Title: Dashboard Ready"));

        let stable_result = wait_for_session(
            root,
            "title-session",
            None,
            None,
            None,
            None,
            None,
            Some(2),
            Some(1500),
            Some(10),
            &sitemap_path,
        )
        .unwrap();
        assert!(stable_result.contains("Title: Dashboard Ready"));
        assert!(stable_result.contains("Diff: summary"));
    }

    #[test]
    fn replays_new_wait_workflow_steps() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let start_url = format!("http://127.0.0.1:{}/start", port);

        std::thread::spawn(move || {
            for _ in 0..4 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _request = read_http_request(&mut stream);
                    let body = "<html><head><title>Dashboard</title></head><body><p>Stable</p></body></html>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let workflow = BrowserWorkflow {
            name: "Static Wait Flow".to_string(),
            start_url: start_url.clone(),
            variables: HashMap::new(),
            steps: vec![
                BrowserWorkflowStep::WaitForTitle {
                    title: "Dashboard".to_string(),
                    timeout_ms: Some(50),
                    interval_ms: Some(5),
                },
                BrowserWorkflowStep::WaitForUrlContains {
                    fragment: "/start".to_string(),
                    timeout_ms: Some(50),
                    interval_ms: Some(5),
                },
                BrowserWorkflowStep::WaitForStable {
                    stable_polls: Some(1),
                    timeout_ms: Some(50),
                    interval_ms: Some(5),
                },
            ],
        };
        let snapshot = parse_html_to_snapshot(
            &start_url,
            "<html><head><title>Dashboard</title></head><body><p>Stable</p></body></html>",
            &[],
            &[],
        );
        let state = super::BrowserReplayState {
            session: super::BrowserSessionState {
                id: "static-wait-session".to_string(),
                current_url: Some(start_url),
                cookies: Vec::new(),
                local_storage: HashMap::new(),
                session_storage: HashMap::new(),
            },
            snapshot,
            filled_fields: HashMap::new(),
            variables: HashMap::new(),
            outputs: HashMap::new(),
        };

        let (summary, _, report) = super::replay_workflow_with_state(&workflow, state, None).unwrap();
        assert!(summary.contains("Workflow 'Static Wait Flow' completed."));
        assert!(report.log.iter().any(|entry| entry.contains("wait_for_title 'Dashboard'")));
        assert!(report.log.iter().any(|entry| entry.contains("wait_for_url_contains '/start'")));
        assert!(report.log.iter().any(|entry| entry.contains("wait_for_stable polls=1 -> no_semantic_change")));
    }

    #[test]
    fn persists_session_navigation_and_cookies() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = "<html><head><title>Session Test</title></head><body><a href='/next'>Next</a></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nSet-Cookie: token=xyz; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sitemap_path = root.join("site_map");
        create_session(root, "qa-session").unwrap();
        let result = navigate_session(root, "qa-session", &url, &sitemap_path).unwrap();
        assert!(result.contains("Session: qa-session"));
        let session = load_session_state(root, "qa-session").unwrap();
        assert_eq!(session.cookies.len(), 1);
        assert_eq!(session.cookies[0].name, "token");
    }

    #[test]
    fn saves_restores_and_replays_browser_session_checkpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", port);

        std::thread::spawn(move || {
            for _ in 0..3 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.starts_with("POST /login") {
                        "<html><head><title>Dashboard</title></head><body><p>Welcome back</p></body></html>"
                    } else {
                        "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input type='submit' value='Sign in'></form></body></html>"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc123; Path=/\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sitemap_path = root.join("site_map");
        create_session(root, "auth-session").unwrap();
        navigate_session(root, "auth-session", &base_url, &sitemap_path).unwrap();
        let checkpoint_path = save_session_checkpoint(root, "auth-session", "before-submit", &sitemap_path).unwrap();
        assert!(checkpoint_path.exists());

        let workflow = BrowserWorkflow {
            name: "Resume Login".to_string(),
            start_url: base_url.clone(),
            variables: HashMap::new(),
            steps: vec![
                BrowserWorkflowStep::FillField { field: "email".to_string(), value: "rust@example.com".to_string() },
                BrowserWorkflowStep::SubmitForm { form: Some("login".to_string()) },
                BrowserWorkflowStep::AssertTextContains { text: "Welcome back".to_string() },
            ],
        };

        let replay = replay_workflow_in_session(root, "auth-session", &workflow, &sitemap_path).unwrap();
        assert!(replay.contains("Workflow 'Resume Login' completed."));
        assert!(replay.contains("Final title: Dashboard"));
        assert!(replay.contains("Session: auth-session"));
        let session = load_session_state(root, "auth-session").unwrap();
        let expected_login_url = format!("{}/login", base_url);
        assert_eq!(session.current_url.as_deref(), Some(expected_login_url.as_str()));
        assert_eq!(session.cookies.len(), 1);

        let restored = restore_session_checkpoint(
            root,
            "auth-session",
            "before-submit",
            Some("forked-session"),
            &sitemap_path,
        )
        .unwrap();
        assert!(restored.contains("Restored browser session checkpoint 'before-submit'"));
        assert!(restored.contains("Session: forked-session"));
        assert!(restored.contains("Title: Login"));
        let restored_session = load_session_state(root, "forked-session").unwrap();
        assert_eq!(restored_session.current_url.as_deref(), Some(base_url.as_str()));
    }
}
