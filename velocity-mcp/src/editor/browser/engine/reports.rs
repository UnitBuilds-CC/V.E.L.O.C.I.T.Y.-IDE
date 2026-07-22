use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn summarize_network_activity(events: &[BrowserProtocolEvent]) -> BrowserNetworkSummary {
    let mut summary = BrowserNetworkSummary {
        event_count: events.len(),
        ..BrowserNetworkSummary::default()
    };
    for event in events {
        if event.kind.eq_ignore_ascii_case("redirect") {
            summary.redirect_count += 1;
            summary.last_redirect_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("download") {
            summary.download_count += 1;
            summary.last_download_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("upload") {
            summary.upload_count += 1;
            summary.last_upload_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("event_stream")
            || (event.kind.eq_ignore_ascii_case("stream")
                && (event.phase.eq_ignore_ascii_case("sse")
                    || event.detail.to_ascii_lowercase().contains("event-stream")
                    || event.target.to_ascii_lowercase().contains("/events")))
        {
            summary.event_stream_count += 1;
            summary.stream_count += 1;
            summary.last_event_stream_target = Some(event.target.clone());
            summary.last_stream_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("websocket")
            || (event.kind.eq_ignore_ascii_case("stream")
                && (event.phase.eq_ignore_ascii_case("websocket")
                    || event.phase.eq_ignore_ascii_case("ws")
                    || event.target.to_ascii_lowercase().starts_with("ws://")
                    || event.target.to_ascii_lowercase().starts_with("wss://")))
        {
            summary.websocket_count += 1;
            summary.stream_count += 1;
            summary.last_websocket_target = Some(event.target.clone());
            summary.last_stream_target = Some(event.target.clone());
        } else if event.kind.eq_ignore_ascii_case("stream") {
            summary.stream_count += 1;
            summary.last_stream_target = Some(event.target.clone());
        } else {
            summary.other_count += 1;
        }
    }
    summary
}

pub fn render_network_summary(summary: &BrowserNetworkSummary) -> Option<String> {
    if summary.event_count == 0 {
        return None;
    }
    let mut parts = vec![
        format!("redirects={}", summary.redirect_count),
        format!("downloads={}", summary.download_count),
        format!("uploads={}", summary.upload_count),
        format!("streams={}", summary.stream_count),
    ];
    if summary.event_stream_count > 0 {
        parts.push(format!("event_streams={}", summary.event_stream_count));
    }
    if summary.websocket_count > 0 {
        parts.push(format!("websockets={}", summary.websocket_count));
    }
    if summary.other_count > 0 {
        parts.push(format!("other={}", summary.other_count));
    }
    if let Some(target) = summary.last_redirect_target.as_deref() {
        parts.push(format!("last_redirect={}", target));
    }
    if let Some(target) = summary.last_download_target.as_deref() {
        parts.push(format!("last_download={}", target));
    }
    if let Some(target) = summary.last_upload_target.as_deref() {
        parts.push(format!("last_upload={}", target));
    }
    if let Some(target) = summary.last_stream_target.as_deref() {
        parts.push(format!("last_stream={}", target));
    }
    if let Some(target) = summary.last_event_stream_target.as_deref() {
        parts.push(format!("last_event_stream={}", target));
    }
    if let Some(target) = summary.last_websocket_target.as_deref() {
        parts.push(format!("last_websocket={}", target));
    }
    Some(parts.join(", "))
}

pub fn storage_buckets(session: &BrowserSessionState) -> Vec<BrowserStorageBucket> {
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

pub fn storage_signature(bucket: &BrowserStorageBucket) -> Vec<String> {
    let mut entries = bucket
        .entries
        .iter()
        .map(|(key, value)| format!("{}:{}={}", bucket.scope, key, value))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

pub fn apply_storage_updates(target: &mut HashMap<String, String>, updates: &HashMap<String, String>) {
    for (key, value) in updates {
        target.insert(String::from(key), String::from(value));
    }
}

pub fn fetch_with_session(
    url: &str,
    method: &str,
    body: Option<&str>,
    cookies: &[BrowserCookie],
    network: &BrowserSessionNetworkConfig,
) -> Result<BrowserHttpResponse, String> {
    network_policy_allows_url(network, url)?;

    let mut agent_builder = ureq::AgentBuilder::new();
    if let Some(timeout_ms) = network.timeout_ms {
        agent_builder = agent_builder.timeout(Duration::from_millis(timeout_ms));
    }
    if let Some(follow_redirects) = network.follow_redirects {
        agent_builder = if follow_redirects {
            agent_builder.redirects(10)
        } else {
            agent_builder.redirects(0)
        };
    }
    let agent = agent_builder.build();
    let mut request = agent.request(method, url).set(
        "User-Agent",
        network
            .user_agent
            .as_deref()
            .unwrap_or(default_browser_user_agent()),
    );
    for (key, value) in &network.headers {
        request = request.set(key, value);
    }
    if let Some(header) = cookie_header(cookies) {
        let header_str: &str = &header;
        request = request.set("Cookie", header_str);
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
    let status_code = response.status();
    let local_storage_updates = response
        .header("X-Velocity-Local-Storage")
        .map(parse_storage_header)
        .unwrap_or_default();
    let session_storage_updates = response
        .header("X-Velocity-Session-Storage")
        .map(parse_storage_header)
        .unwrap_or_default();
    let mutations = response
        .header("X-Velocity-Mutations")
        .map(parse_list_header)
        .unwrap_or_default();
    let requests = request_records_from_headers(
        method,
        url,
        status_code,
        response.header("X-Velocity-Requests"),
    );
    let settle_signals =
        settle_signals_from_headers(method, status_code, response.header("X-Velocity-Settle"));
    let runtime_state = runtime_state_from_headers(response.header("X-Velocity-Runtime-State"));
    let mut protocol_events =
        protocol_events_from_headers(response.header("X-Velocity-Protocol-Events"));
    let final_url = response.get_url().to_string();
    if final_url != url {
        protocol_events.push(BrowserProtocolEvent {
            kind: "navigation".to_string(),
            phase: "redirected".to_string(),
            target: final_url.clone(),
            detail: url.to_string(),
        });
        protocol_events.sort_by(|left, right| {
            protocol_event_signature(left).cmp(&protocol_event_signature(right))
        });
        protocol_events.dedup();
    }

    let html = response
        .into_string()
        .map_err(|e| format!("Failed to read HTTP body: {:?}", e))?;
    Ok(BrowserHttpResponse {
        html,
        final_url,
        cookies: response_cookies,
        local_storage_updates,
        session_storage_updates,
        mutations,
        requests,
        settle_signals,
        runtime_state,
        protocol_events,
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
        let Some(tag_end_rel) = tag_end_rel else {
            break;
        };
        let tag_end = form_start + tag_end_rel;
        let form_tag = &html[form_start + 1..tag_end];
        let body_start = tag_end + 1;
        let close_rel = lower_html[body_start..].find("</form>");
        let Some(close_rel) = close_rel else {
            break;
        };
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
                let input_type =
                    extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
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
    mutations: &[String],
    requests: &[BrowserRequestRecord],
    settle_signals: &[String],
) -> BrowserPageSnapshot {
    parse_html_to_snapshot_with_runtime_state(
        url,
        html,
        cookies,
        storage,
        mutations,
        requests,
        settle_signals,
        &[],
        &[],
    )
}

pub fn parse_html_to_snapshot_with_runtime_state(
    url: &str,
    html: &str,
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
    mutations: &[String],
    requests: &[BrowserRequestRecord],
    settle_signals: &[String],
    runtime_state: &[BrowserRuntimeState],
    protocol_events: &[BrowserProtocolEvent],
) -> BrowserPageSnapshot {
    let forms = parse_forms(url, html);
    let mut elements = Vec::new();
    let mut title = "Untitled Page".to_string();
    let mut page_text = String::new();

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let tag_start = i;
            let mut tag_content = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '>' {
                tag_content.push(chars[i]);
                i += 1;
            }
            let body_start = (i + 1).min(chars.len());
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
                let clean_text = extract_element_body_text(html, body_start, "</a>");
                if let Some(href_value) = href {
                    let absolute_href = resolve_relative_url(url, &href_value);
                    elements.push(AomElement {
                        role: "link".to_string(),
                        name: if clean_text.is_empty() {
                            absolute_href.clone()
                        } else {
                            clean_text
                        },
                        value: absolute_href.clone(),
                        target_url: Some(absolute_href),
                        supported_actions: vec!["open".to_string(), "click".to_string()],
                        provenance: "native-static".to_string(),
                        actionability: role_actionability("link"),
                    });
                }
            } else if lower.starts_with("button") {
                let label = extract_element_body_text(html, body_start, "</button>");
                let fallback = extract_attr(trimmed, "aria-label")
                    .or_else(|| extract_attr(trimmed, "name"))
                    .or_else(|| extract_attr(trimmed, "value"))
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
                let input_type =
                    extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
                let placeholder = extract_attr(trimmed, "placeholder").unwrap_or_default();
                let aria_label = extract_attr(trimmed, "aria-label").unwrap_or_default();
                let name_attr = extract_attr(trimmed, "name").unwrap_or_default();
                let value_attr = extract_attr(trimmed, "value").unwrap_or_default();
                let name = if !placeholder.is_empty() {
                    placeholder
                } else if !aria_label.is_empty() {
                    aria_label
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
            let _ = tag_start;
        } else {
            if chars[i] != '\r' && chars[i] != '\n' && chars[i] != '\t' {
                page_text.push(chars[i]);
            }
            i += 1;
        }
    }

    for form in &forms {
        for field in &form.fields {
            if elements.iter().any(|element| {
                element.role.eq_ignore_ascii_case("textbox")
                    && (element.name.eq_ignore_ascii_case(&field.label)
                        || element.name.eq_ignore_ascii_case(&field.name))
            }) {
                continue;
            }
            elements.push(AomElement {
                role: "textbox".to_string(),
                name: if field.label.is_empty() {
                    field.name.clone()
                } else {
                    field.label.clone()
                },
                value: field.value.clone(),
                target_url: None,
                supported_actions: vec!["focus".to_string(), "type".to_string()],
                provenance: "native-static-repaired".to_string(),
                actionability: if field.input_type.eq_ignore_ascii_case("hidden") {
                    0
                } else {
                    role_actionability("textbox")
                },
            });
        }
        if let Some(label) = form
            .submit_label
            .as_ref()
            .filter(|label: &&String| !label.trim().is_empty())
        {
            if !elements.iter().any(|element| {
                element.role.eq_ignore_ascii_case("button")
                    && element.name.eq_ignore_ascii_case(label)
            }) {
                elements.push(AomElement {
                    role: "button".to_string(),
                    name: label.trim().to_string(),
                    value: form.id.clone(),
                    target_url: None,
                    supported_actions: vec!["click".to_string(), "submit".to_string()],
                    provenance: "native-static-repaired".to_string(),
                    actionability: role_actionability("button"),
                });
            }
        }
    }

    BrowserPageSnapshot {
        url: url.to_string(),
        title,
        summary: truncate_string(page_text.trim(), 1000),
        elements,
        forms,
        cookies: cookies.to_vec(),
        storage: storage.to_vec(),
        mutations: mutations.to_vec(),
        requests: requests.to_vec(),
        settle_signals: settle_signals.to_vec(),
        runtime_state: runtime_state.to_vec(),
        protocol_events: protocol_events.to_vec(),
    }
}

pub fn write_snapshot_json(
    snapshot: &BrowserPageSnapshot,
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let snapshot_path = browser_snapshot_path(&snapshot.url, sitemap_path);
    if let Some(parent) = snapshot_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser snapshot dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(snapshot)
        .map_err(|err| format!("serialise browser snapshot: {err}"))?;
    fs::write(&snapshot_path, json).map_err(|err| format!("write browser snapshot: {err}"))?;
    Ok(snapshot_path)
}

pub fn write_html_fallback(
    url: &str,
    html: &str,
    sitemap_path: &Path,
) -> Result<Option<PathBuf>, String> {
    if html.trim().is_empty() {
        return Ok(None);
    }
    let path = browser_html_fallback_path(url, sitemap_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create browser html fallback dir: {err}"))?;
    }
    fs::write(&path, html.as_bytes())
        .map_err(|err| format!("write browser html fallback: {err}"))?;
    Ok(Some(path))
}

pub fn load_html_fallback(url: &str, sitemap_path: &Path) -> Result<String, String> {
    let path = browser_html_fallback_path(url, sitemap_path);
    fs::read_to_string(&path).map_err(|err| format!("read browser html fallback: {err}"))
}

pub fn load_snapshot_json(url: &str, sitemap_path: &Path) -> Result<BrowserPageSnapshot, String> {
    let snapshot_path = browser_snapshot_path(url, sitemap_path);
    let raw = fs::read(&snapshot_path).map_err(|err| format!("read browser snapshot: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse browser snapshot: {err}"))
}

