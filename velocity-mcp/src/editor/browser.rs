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
pub struct BrowserPageSnapshot {
    pub url: String,
    pub title: String,
    pub summary: String,
    pub elements: Vec<AomElement>,
    pub forms: Vec<BrowserForm>,
    pub cookies: Vec<BrowserCookie>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionState {
    pub id: String,
    pub current_url: Option<String>,
    pub cookies: Vec<BrowserCookie>,
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
    AssertElement { role: String, name: String },
    AssertTextContains { text: String },
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflow {
    pub name: String,
    pub start_url: String,
    pub steps: Vec<BrowserWorkflowStep>,
}

struct BrowserHttpResponse {
    html: String,
    cookies: Vec<BrowserCookie>,
}

struct BrowserReplayState {
    session: BrowserSessionState,
    snapshot: BrowserPageSnapshot,
    filled_fields: HashMap<String, String>,
}

const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_WAIT_INTERVAL_MS: u64 = 250;

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

    let html = response
        .into_string()
        .map_err(|e| format!("Failed to read HTTP body: {:?}", e))?;
    Ok(BrowserHttpResponse {
        html,
        cookies: response_cookies,
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

fn parse_html_to_snapshot(url: &str, html: &str, cookies: &[BrowserCookie]) -> BrowserPageSnapshot {
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
                elements.push(AomElement {
                    role: role.to_string(),
                    name,
                    value: value_attr,
                    target_url: None,
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
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let facts_path = crawl_facts_path(url, sitemap_path);
    if let Some(parent) = facts_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser capture dir: {err}"))?;
    }

    let mut facts = vec![
        "browser-capture version 4".to_string(),
        "field_count 4".to_string(),
        "field\tkind\tpage-crawl".to_string(),
        format!("field\telement_count\t{}", elements.len()),
        format!("field\tform_count\t{}", forms.len()),
        format!("field\tcookie_count\t{}", cookies.len()),
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
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
    let session_path = save_session_state(workspace_root, &session)?;

    Ok(format!(
        "Session navigate complete.\nSession: {}\nURL: {}\nTitle: {}\nForms: {}\nCookies: {}\nSnapshot JSON: {}\nSession JSON: {}\nNDA Facts: {}",
        session.id,
        snapshot.url,
        snapshot.title,
        snapshot.forms.len(),
        snapshot.cookies.len(),
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
    session.current_url = Some(url.to_string());
    Ok(parse_html_to_snapshot(url, &response.html, &session.cookies))
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

fn find_element<'a>(snapshot: &'a BrowserPageSnapshot, role: &str, name: &str) -> Option<&'a AomElement> {
    snapshot
        .elements
        .iter()
        .find(|element| element.role.eq_ignore_ascii_case(role) && element.name.eq_ignore_ascii_case(name))
}

fn find_form<'a>(snapshot: &'a BrowserPageSnapshot, form_id: Option<&str>) -> Option<&'a BrowserForm> {
    match form_id {
        Some(id) => snapshot.forms.iter().find(|form| form.id.eq_ignore_ascii_case(id)),
        None => snapshot.forms.first(),
    }
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

    let mut added_elements = after_elements.difference(&before_elements).cloned().collect::<Vec<_>>();
    let mut removed_elements = before_elements.difference(&after_elements).cloned().collect::<Vec<_>>();
    let mut added_forms = after_forms.difference(&before_forms).cloned().collect::<Vec<_>>();
    let mut removed_forms = before_forms.difference(&after_forms).cloned().collect::<Vec<_>>();
    let mut added_cookies = after_cookies.difference(&before_cookies).cloned().collect::<Vec<_>>();
    let mut removed_cookies = before_cookies.difference(&after_cookies).cloned().collect::<Vec<_>>();

    added_elements.sort();
    removed_elements.sort();
    added_forms.sort();
    removed_forms.sort();
    added_cookies.sort();
    removed_cookies.sort();

    BrowserSnapshotDiff {
        title_changed: before.title != after.title,
        summary_changed: before.summary != after.summary,
        added_elements,
        removed_elements,
        added_forms,
        removed_forms,
        added_cookies,
        removed_cookies,
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

    if parts.is_empty() {
        "no_semantic_change".to_string()
    } else {
        parts.join(",")
    }
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

pub fn wait_for_session(
    workspace_root: &Path,
    session_id: &str,
    text: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
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
        }));
    let diff = if let Some(wait_text) = text {
        wait_for_condition(&mut session, &mut snapshot, timeout_ms, interval_ms, |candidate| {
            snapshot_contains_text(candidate, wait_text)
        })?
    } else if let (Some(wait_role), Some(wait_name)) = (role, name) {
        wait_for_condition(&mut session, &mut snapshot, timeout_ms, interval_ms, |candidate| {
            find_element(candidate, wait_role, wait_name).is_some()
        })?
    } else {
        return Err("browser_session_wait requires either text or both role and name".to_string());
    };

    persist_snapshot_to_sitemap(&snapshot, sitemap_path)?;
    let facts_path = write_crawl_facts(
        &snapshot.url,
        &snapshot.title,
        &snapshot.summary,
        &snapshot.elements,
        &snapshot.forms,
        &snapshot.cookies,
        sitemap_path,
    )?;
    let snapshot_path = write_snapshot_json(&snapshot, sitemap_path)?;
    let session_path = save_session_state(workspace_root, &session)?;

    Ok(format!(
        "Session wait complete.\nSession: {}\nURL: {}\nTitle: {}\nDiff: {}\nSnapshot JSON: {}\nSession JSON: {}\nNDA Facts: {}",
        session.id,
        snapshot.url,
        snapshot.title,
        render_snapshot_diff(&diff),
        snapshot_path.display(),
        session_path.display(),
        facts_path.display(),
    ))
}

fn apply_fill_field(state: &mut BrowserReplayState, field_name: &str, value: &str) -> Result<(), String> {
    let mut matched = false;
    for form in &mut state.snapshot.forms {
        for field in &mut form.fields {
            if field.name.eq_ignore_ascii_case(field_name) || field.label.eq_ignore_ascii_case(field_name) {
                field.value = value.to_string();
                state.filled_fields.insert(field.name.clone(), value.to_string());
                matched = true;
            }
        }
    }
    for element in &mut state.snapshot.elements {
        if element.role.eq_ignore_ascii_case("textbox") && element.name.eq_ignore_ascii_case(field_name) {
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
    state.session.current_url = Some(target_url.clone());
    state.snapshot = parse_html_to_snapshot(&target_url, &response.html, &state.session.cookies);
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

pub fn render_workflow_dsl(workflow: &BrowserWorkflow) -> String {
    let mut lines = vec![
        "browser-workflow version 2".to_string(),
        format!("name\t{}", encode_nda_text(&workflow.name)),
        format!("start_url\t{}", encode_nda_text(&workflow.start_url)),
    ];

    for (idx, step) in workflow.steps.iter().enumerate() {
        match step {
            BrowserWorkflowStep::Navigate { url } => {
                lines.push(format!("step\t{}\tnavigate\t{}", idx, encode_nda_text(url)));
            }
            BrowserWorkflowStep::Click { role, name } => {
                lines.push(format!(
                    "step\t{}\tclick\trole={}\tname={}",
                    idx,
                    encode_nda_text(role),
                    encode_nda_text(name)
                ));
            }
            BrowserWorkflowStep::FillField { field, value } => {
                lines.push(format!(
                    "step\t{}\tfill_field\tfield={}\tvalue={}",
                    idx,
                    encode_nda_text(field),
                    encode_nda_text(value)
                ));
            }
            BrowserWorkflowStep::SubmitForm { form } => {
                lines.push(format!(
                    "step\t{}\tsubmit_form\tform={}",
                    idx,
                    encode_nda_text(form.as_deref().unwrap_or("default"))
                ));
            }
            BrowserWorkflowStep::WaitForText {
                text,
                timeout_ms,
                interval_ms,
            } => {
                lines.push(format!(
                    "step\t{}\twait_for_text\ttext={}\ttimeout_ms={}\tinterval_ms={}",
                    idx,
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
                    "step\t{}\twait_for_element\trole={}\tname={}\ttimeout_ms={}\tinterval_ms={}",
                    idx,
                    encode_nda_text(role),
                    encode_nda_text(name),
                    timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS),
                    interval_ms.unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
                ));
            }
            BrowserWorkflowStep::AssertElement { role, name } => {
                lines.push(format!(
                    "step\t{}\tassert_element\trole={}\tname={}",
                    idx,
                    encode_nda_text(role),
                    encode_nda_text(name)
                ));
            }
            BrowserWorkflowStep::AssertTextContains { text } => {
                lines.push(format!("step\t{}\tassert_text\t{}", idx, encode_nda_text(text)));
            }
        }
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

pub fn replay_workflow(workflow: &BrowserWorkflow) -> Result<String, String> {
    let mut session = BrowserSessionState {
        id: format!("replay-{}", sanitize_file_stem(&workflow.name)),
        current_url: Some(workflow.start_url.clone()),
        cookies: Vec::new(),
    };
    let snapshot = crawl_page_snapshot_with_session(&mut session, &workflow.start_url)?;
    let mut state = BrowserReplayState {
        session,
        snapshot,
        filled_fields: HashMap::new(),
    };
    let mut log = vec![format!("start {} -> {}", workflow.start_url, state.snapshot.title)];

    for step in &workflow.steps {
        match step {
            BrowserWorkflowStep::Navigate { url } => {
                state.snapshot = crawl_page_snapshot_with_session(&mut state.session, url)?;
                log.push(format!("navigate {} -> {}", url, state.snapshot.title));
            }
            BrowserWorkflowStep::Click { role, name } => {
                let target = find_element(&state.snapshot, role, name)
                    .ok_or_else(|| format!("workflow click target not found: role='{}' name='{}'", role, name))?;
                let target_url = target.target_url.clone().ok_or_else(|| {
                    format!(
                        "workflow click target '{}' is not a navigable link in the current static browser engine",
                        name
                    )
                })?;
                state.snapshot = crawl_page_snapshot_with_session(&mut state.session, &target_url)?;
                log.push(format!("click {}:{} -> {}", role, name, state.snapshot.title));
            }
            BrowserWorkflowStep::FillField { field, value } => {
                apply_fill_field(&mut state, field, value)?;
                log.push(format!("fill_field {} ok", field));
            }
            BrowserWorkflowStep::SubmitForm { form } => {
                submit_current_form(&mut state, form.as_deref())?;
                log.push(format!(
                    "submit_form {} -> {}",
                    form.as_deref().unwrap_or("default"),
                    state.snapshot.title
                ));
            }
            BrowserWorkflowStep::WaitForText {
                text,
                timeout_ms,
                interval_ms,
            } => {
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| snapshot_contains_text(candidate, text),
                )?;
                log.push(format!("wait_for_text '{}' -> {}", text, render_snapshot_diff(&diff)));
            }
            BrowserWorkflowStep::WaitForElement {
                role,
                name,
                timeout_ms,
                interval_ms,
            } => {
                let diff = wait_for_condition(
                    &mut state.session,
                    &mut state.snapshot,
                    *timeout_ms,
                    *interval_ms,
                    |candidate| find_element(candidate, role, name).is_some(),
                )?;
                log.push(format!(
                    "wait_for_element {}:{} -> {}",
                    role,
                    name,
                    render_snapshot_diff(&diff)
                ));
            }
            BrowserWorkflowStep::AssertElement { role, name } => {
                find_element(&state.snapshot, role, name).ok_or_else(|| {
                    format!("workflow assertion failed: missing element role='{}' name='{}'", role, name)
                })?;
                log.push(format!("assert_element {}:{} ok", role, name));
            }
            BrowserWorkflowStep::AssertTextContains { text } => {
                if !snapshot_contains_text(&state.snapshot, text) {
                    return Err(format!("workflow assertion failed: text '{}' not present", text));
                }
                log.push(format!("assert_text '{}' ok", text));
            }
        }
    }

    Ok(format!(
        "Workflow '{}' completed.\nFinal URL: {}\nFinal title: {}\nSteps executed: {}\nCookies: {}\n{}",
        workflow.name,
        state.snapshot.url,
        state.snapshot.title,
        workflow.steps.len(),
        state.session.cookies.len(),
        log.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        crawl_facts_path, create_session, diff_snapshots, load_session_state, load_workflow,
        load_snapshot_json, navigate_session, parse_html_to_snapshot, render_workflow_dsl,
        replay_workflow, save_workflow, wait_for_session, write_crawl_facts, AomElement,
        BrowserCookie, BrowserWorkflow, BrowserWorkflowStep,
    };
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
                },
                AomElement {
                    role: "button".to_string(),
                    name: "Search".to_string(),
                    value: String::new(),
                    target_url: None,
                },
            ],
            &[],
            &[BrowserCookie { name: "sid".to_string(), value: "123".to_string() }],
            &sitemap_path,
        )
        .unwrap();

        assert_eq!(facts_path, crawl_facts_path("https://example.com/docs", &sitemap_path));
        let facts = fs::read_to_string(facts_path).unwrap();
        assert!(facts.starts_with("browser-capture version 4\n"));
        assert!(facts.contains("field_count 4\n"));
        assert!(facts.contains("field\tcookie_count\t1\n"));
        assert!(facts.contains("element_field\t0\trole\tlink"));
        assert!(facts.contains("cookie_field\t0\tname\tsid"));
    }

    #[test]
    fn parses_html_into_snapshot_with_forms() {
        let snapshot = parse_html_to_snapshot(
            "https://example.com",
            "<html><head><title>Docs</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'><input name='password' type='password'><input type='submit' value='Sign in'></form><a href='/api'>API</a></body></html>",
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

        let workflow = BrowserWorkflow {
            name: "Login Flow".to_string(),
            start_url: base_url,
            steps: vec![
                BrowserWorkflowStep::FillField { field: "email".to_string(), value: "rust@example.com".to_string() },
                BrowserWorkflowStep::SubmitForm { form: Some("login".to_string()) },
                BrowserWorkflowStep::AssertTextContains { text: "Welcome back".to_string() },
            ],
        };

        let result = replay_workflow(&workflow).unwrap();
        assert!(result.contains("Workflow 'Login Flow' completed."));
        assert!(result.contains("Final title: Dashboard"));
        assert!(result.contains("Cookies: 1"));
    }

    #[test]
    fn computes_browser_snapshot_diffs() {
        let before = parse_html_to_snapshot(
            "https://example.com",
            "<html><head><title>Login</title></head><body><form id='login' action='/login' method='post'><input name='email' placeholder='Email'></form></body></html>",
            &[],
        );
        let after = parse_html_to_snapshot(
            "https://example.com/dashboard",
            "<html><head><title>Dashboard</title></head><body><a href='/reports'>Reports</a></body></html>",
            &[BrowserCookie { name: "session".to_string(), value: "abc123".to_string() }],
        );

        let diff = diff_snapshots(&before, &after);
        assert!(diff.title_changed);
        assert!(diff.added_elements.iter().any(|entry| entry.contains("link:Reports")));
        assert!(diff.removed_forms.iter().any(|entry| entry.contains("login:POST")));
        assert!(diff.added_cookies.iter().any(|entry| entry == "session=abc123"));
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
}
