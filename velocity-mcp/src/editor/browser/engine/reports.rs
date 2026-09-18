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

pub fn apply_storage_updates(
    target: &mut HashMap<String, String>,
    updates: &HashMap<String, String>,
) {
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

    // `data:` and `about:` name their own document and have no transport: ureq
    // reads them as a host-less URL and fails with a Debug dump about
    // `EmptyHost`. Bug #56 - resolve them the way the native session does,
    // through the shared helper, so the two engines cannot drift apart.
    if let Some(synthetic) = velocity_browser::session::resolve_synthetic_url(url) {
        let html = synthetic?;
        return Ok(BrowserHttpResponse {
            html,
            final_url: url.to_string(),
            // A synthetic page was never fetched, so there is genuinely
            // nothing to report: no cookie arrived, no storage moved, no
            // request settled. Empty lists are the honest answer, and
            // inventing a settle signal here would be exactly the kind of
            // evidence a caller cannot act on.
            cookies: Vec::new(),
            local_storage_updates: HashMap::new(),
            session_storage_updates: HashMap::new(),
            mutations: Vec::new(),
            requests: Vec::new(),
            settle_signals: Vec::new(),
            runtime_state: Vec::new(),
            protocol_events: Vec::new(),
        });
    }

    // Anything still left that is not http(s) only reaches ureq in order to
    // fail there. Catching it here keeps the transport's internal struct out
    // of the message the agent has to reason about.
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!(
            "this session only fetches http:// and https:// URLs, not '{}'; for an \
             offline page use data:text/html,... or about:blank",
            clip_url_for_message(url)
        ));
    }

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
    // Bound and wrapping labels live between tags, which `scan_tags` drops on
    // the floor. Resolve them over the raw markup once per page.
    let label_texts = collect_label_texts(html);
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
                let label = field_label(trimmed, &name, &label_texts);
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
                let label = field_label(trimmed, &name, &label_texts);
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

/// The accessible name of one control, in the order the native session uses
/// (bug #45): explicit ARIA wins over a bound `<label>`, which wins over a
/// placeholder, which wins over the bare `name`.
///
/// Bug #57: this parser consulted only `placeholder` and `aria-label`, so a
/// control labelled the ordinary way - `<label for="x">` or a label wrapped
/// around the input - reported its `name` as its label. `find_form_field`
/// already scores matches against `field.label`, and `browser_session_fill`
/// advertises "field label, name, or nearby semantic text", so the advertised
/// strategy silently did nothing.
fn field_label(tag: &str, name: &str, label_texts: &HashMap<String, String>) -> String {
    let bound = extract_attr(tag, "id")
        .and_then(|id| label_texts.get(&id).cloned())
        .or_else(|| label_texts.get(name).cloned());
    extract_attr(tag, "aria-label")
        .or(bound)
        .or_else(|| extract_attr(tag, "placeholder"))
        .unwrap_or_else(|| name.to_string())
}

/// Map each control's `id` and `name` to the visible text of the `<label>`
/// that labels it, whether bound with `for=` or wrapped around the control.
fn collect_label_texts(html: &str) -> HashMap<String, String> {
    let mut labels: HashMap<String, String> = HashMap::new();
    let lower = html.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("<label") {
        let tag_start = from + rel;
        let Some(close_rel) = lower[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + close_rel;
        // An unclosed `<label>` is common enough in real markup, and the old
        // code abandoned the whole scan the moment it met one: a single stray
        // label cost every field on the page its accessible name, so
        // `browser_session_fill` by label silently stopped working (bug #58).
        // A `<label>` opener implicitly closes one already open, so a later
        // real `</label>` must not be read as closing this one - and the extent
        // stops at the nested control rather than at the end of the scope a
        // browser would use, which would swallow every word after it and read
        // `<label>Search<input><button>Go</button>` as "Search Go".
        let closer = lower[tag_end..].find("</label>").map(|rel| tag_end + rel);
        let next_label = lower[tag_end + 1..]
            .find("<label")
            .map(|rel| tag_end + 1 + rel);
        let explicitly_closed = matches!(closer, Some(end) if next_label.is_none_or(|nl| end < nl));
        let (text_end, next_from) = match (explicitly_closed, closer) {
            (true, Some(end)) => (end, end + "</label>".len()),
            _ => (implicit_label_extent(&lower, tag_end), tag_end + 1),
        };
        from = next_from;
        let text = visible_text(&html[tag_end + 1..text_end]);
        if text.is_empty() {
            continue;
        }
        if let Some(for_id) = extract_attr(&html[tag_start + 1..tag_end], "for") {
            labels.entry(for_id).or_insert_with(|| text.clone());
        }
        for tag in scan_tags(&html[tag_end + 1..text_end]) {
            let trimmed = tag.trim();
            let tag_lower = trimmed.to_ascii_lowercase();
            if !(tag_lower.starts_with("input")
                || tag_lower.starts_with("textarea")
                || tag_lower.starts_with("select"))
            {
                continue;
            }
            // Index under both handles so a lookup finds it whichever the
            // caller knew: `<label>Nested<input name="q"></label>` is reachable
            // as "q" as well as by its id.
            for key in ["name", "id"] {
                if let Some(value) = extract_attr(trimmed, key) {
                    labels.entry(value).or_insert_with(|| text.clone());
                }
            }
            // A `<label>` has exactly one labeled control - its first labelable
            // descendant - so stop here. Binding the rest would let one opened
            // label claim every field that follows it, which an unterminated
            // extent in particular could otherwise do across the whole page.
            break;
        }
    }
    labels
}

/// Where the contents of an implicitly closed `<label>` stop: whichever comes
/// first, the next `<label>` opener or the end of the first control nested
/// inside it.
fn implicit_label_extent(lower: &str, tag_end: usize) -> usize {
    let rest = tag_end + 1;
    let next_label = lower[rest..].find("<label").map(|rel| rest + rel);
    let first_control = ["<input", "<textarea", "<select"]
        .iter()
        .filter_map(|needle| lower[rest..].find(needle).map(|rel| rest + rel))
        .min()
        .and_then(|start| lower[start..].find('>').map(|rel| start + rel + 1));
    next_label
        .into_iter()
        .chain(first_control)
        .min()
        .unwrap_or(lower.len())
}

/// Tag markup is noise; only the text between the angle brackets is a label.
fn visible_text(fragment: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            c => out.push(if c.is_whitespace() { ' ' } else { c }),
        }
    }
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.trim().to_string()
}

/// Convenience overload of [`parse_html_to_snapshot_with_runtime_state`] for callers
/// that have no runtime state or protocol events to fold into the snapshot.
pub fn parse_html_to_snapshot(
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
    fs::read_to_string(&path)
        .map_err(|err| describe_missing_artifact("html fallback", url, &path, &err))
}

pub fn load_snapshot_json(url: &str, sitemap_path: &Path) -> Result<BrowserPageSnapshot, String> {
    let snapshot_path = browser_snapshot_path(url, sitemap_path);
    let raw = fs::read(&snapshot_path)
        .map_err(|err| describe_missing_artifact("snapshot", url, &snapshot_path, &err))?;
    serde_json::from_slice(&raw).map_err(|err| {
        format!(
            "parse browser snapshot for '{}': {err}",
            clip_url_for_message(url)
        )
    })
}

/// Name the URL and the file that was looked for when a page artifact cannot
/// be read.
///
/// Bug #55: snapshots are stored under a hash of the URL, so the raw
/// `The system cannot find the file specified. (os error 2)` named neither the
/// page the caller asked about nor where the engine looked. Same class as the
/// bug #34 fix for `wa_read_snapshot`, which is why this reads better now:
/// a failed lookup has to tell the caller what to capture first.
fn describe_missing_artifact(kind: &str, url: &str, path: &Path, err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        format!(
            "no browser {} artifact is stored for '{}'; nothing has been captured from that \
             page yet (looked for {}) - navigate it with browser_session_navigate or web_navigate \
             first",
            kind,
            clip_url_for_message(url),
            path.display()
        )
    } else {
        format!(
            "read browser {} for '{}': {err}",
            kind,
            clip_url_for_message(url)
        )
    }
}

/// Shorten a URL enough to quote in an error without echoing a whole inline
/// document back at the caller. Char-based: slicing by byte offset panics on
/// any multi-byte character that straddles the cut.
fn clip_url_for_message(url: &str) -> String {
    const MAX: usize = 80;
    let mut clipped: String = url.chars().take(MAX).collect();
    if url.chars().count() > MAX {
        clipped.push_str("...");
    }
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BrowserHttpResponse` has no `Debug`, so `expect_err` cannot be used on
    /// it. Pulling the message out by hand keeps the assertion readable.
    fn failure(result: Result<BrowserHttpResponse, String>) -> String {
        match result {
            Ok(_) => panic!("expected the call to fail, but it succeeded"),
            Err(err) => err,
        }
    }

    #[test]
    fn a_data_url_renders_its_own_document_without_the_network() {
        // Bug #56: ureq has no transport for `data:` and answered with a
        // `Transport { kind: InvalidUrl, ... EmptyHost }` Debug dump, so a
        // persisted-session form flow could never be driven offline.
        let url = "data:text/html,%3Cform%3E%3Cinput%20name%3Dq%3E%3C%2Fform%3E";
        let page = fetch_with_session(
            url,
            "GET",
            None,
            &[],
            &BrowserSessionNetworkConfig::default(),
        )
        .expect("a data: URL names its own document and needs no fetch");
        assert!(
            page.html.contains("<form>"),
            "decoded body: {:?}",
            page.html
        );
        assert!(
            page.html.contains("name=q"),
            "decoded body: {:?}",
            page.html
        );
        assert_eq!(page.final_url, url);
    }

    #[test]
    fn a_synthetic_page_claims_no_observed_activity() {
        // Nothing was fetched, so nothing may be reported as observed. These
        // lists describing traffic have to stay empty rather than carry a
        // placeholder signal the caller would read as real evidence.
        let page = fetch_with_session(
            "about:blank",
            "GET",
            None,
            &[],
            &BrowserSessionNetworkConfig::default(),
        )
        .expect("about:blank renders empty");
        assert!(page.html.is_empty());
        assert!(page.cookies.is_empty());
        assert!(page.requests.is_empty());
        assert!(page.settle_signals.is_empty());
        assert!(page.protocol_events.is_empty());
        assert!(page.local_storage_updates.is_empty());
    }

    #[test]
    fn an_unfetchable_scheme_is_named_rather_than_dumped() {
        let err = failure(fetch_with_session(
            "ftp://example.com/pub",
            "GET",
            None,
            &[],
            &BrowserSessionNetworkConfig::default(),
        ));
        assert!(err.contains("ftp://example.com/pub"), "{err}");
        assert!(
            !err.contains("EmptyHost") && !err.contains("Transport"),
            "the transport's internal struct leaked into the message: {err}"
        );
    }

    #[test]
    fn an_about_target_with_no_document_says_so() {
        let err = failure(fetch_with_session(
            "about:config",
            "GET",
            None,
            &[],
            &BrowserSessionNetworkConfig::default(),
        ));
        assert!(err.contains("about:config"), "{err}");
    }

    #[test]
    fn real_http_urls_are_left_for_the_transport() {
        // The short-circuit must not swallow genuine fetches by accident.
        assert!(velocity_browser::session::resolve_synthetic_url("https://example.com").is_none());
        assert!(velocity_browser::session::resolve_synthetic_url("http://example.com").is_none());
    }

    #[test]
    fn a_missing_snapshot_names_the_url_and_the_hint() {
        // Bug #55: this used to be `read browser snapshot: The system cannot
        // find the file specified. (os error 2)` - it named neither the page
        // nor where the engine looked, because snapshots are stored by hash.
        let dir = tempfile::tempdir().expect("temp dir");
        let sitemap = dir.path().join("site_map");
        let err = load_snapshot_json("https://example.com/never-captured", &sitemap)
            .expect_err("nothing has been captured into an empty workspace");
        assert!(
            err.contains("https://example.com/never-captured"),
            "the URL is missing from: {err}"
        );
        assert!(err.contains("navigate"), "no recovery hint in: {err}");
        assert!(
            !err.contains("os error 2"),
            "raw OS error still leaking: {err}"
        );
    }

    #[test]
    fn a_missing_html_fallback_names_the_url() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = load_html_fallback("https://example.net/never-captured", dir.path())
            .expect_err("nothing has been captured");
        assert!(err.contains("https://example.net/never-captured"), "{err}");
        assert!(!err.contains("os error 2"), "{err}");
    }

    #[test]
    fn clipping_a_multibyte_url_does_not_panic() {
        // Byte-offset slicing is the bug class this clip exists to avoid.
        let wide = format!("https://example.com/{}", "🦀".repeat(100));
        let clipped = clip_url_for_message(&wide);
        assert!(clipped.ends_with("..."), "{clipped}");
        assert!(
            clipped.chars().count() <= 83,
            "{} chars",
            clipped.chars().count()
        );
    }

    /// `(name, label)` of the single field in a one-field page.
    fn only_field(html: &str) -> (String, String) {
        let forms = parse_forms("https://example.com/", html);
        assert_eq!(forms.len(), 1, "expected one form in {html}");
        assert_eq!(forms[0].fields.len(), 1, "expected one field in {html}");
        (
            forms[0].fields[0].name.clone(),
            forms[0].fields[0].label.clone(),
        )
    }

    #[test]
    fn a_bound_label_becomes_the_field_label() {
        // Bug #57: `for` was never followed, so the field reported its own name
        // as its label and label-based matching could never find it.
        let (name, label) =
            only_field("<form><label for=\"x\">Search</label><input id=\"x\" name=\"q\"></form>");
        assert_eq!(name, "q");
        assert_eq!(label, "Search");
    }

    #[test]
    fn a_wrapping_label_becomes_the_field_label() {
        let (_, label) = only_field("<form><label>Nested<input name=\"q\"></label></form>");
        assert_eq!(label, "Nested");
    }

    #[test]
    fn label_text_ignores_markup_inside_the_label() {
        // Wrapping, so the association is real: the point here is that <b> is
        // stripped rather than leaking its tag name or its text into the label.
        let (_, label) =
            only_field("<form><label>First <b>name</b><input name=\"q\"></label></form>");
        assert_eq!(label, "First name");
    }

    #[test]
    fn a_sibling_label_does_not_claim_an_unbound_input() {
        // `<label>Text</label><input>` with no `for` and no wrapping labels
        // nothing in HTML: a bare <label> associates with its first labelable
        // *descendant*. Falling back to the name is the honest answer, not a
        // guess at an association a browser would not honour.
        let (_, label) = only_field("<form><label>First name</label><input name=\"q\"></form>");
        assert_eq!(label, "q");
    }

    #[test]
    fn an_unterminated_label_still_names_its_control() {
        // The page that surfaced bug #58: the label is never closed, which used
        // to abandon the whole scan and leave the field answering only to its
        // name. Attributes are unquoted here too, as they were in the probe.
        let (_, label) =
            only_field("<form method=get><label>Search<input name=q><button>Go</button></form>");
        assert_eq!(label, "Search");
    }

    #[test]
    fn a_later_bound_label_survives_an_earlier_unterminated_one() {
        let forms = parse_forms(
            "https://example.com/",
            "<form><label>Alpha<input name=\"a\"><label for=\"c\">Gamma</label>\
             <input id=\"c\" name=\"c\"></form>",
        );
        assert_eq!(forms[0].fields.len(), 2);
        assert_eq!(forms[0].fields[0].label, "Alpha");
        assert_eq!(forms[0].fields[1].label, "Gamma");
    }

    #[test]
    fn a_closed_label_still_reads_text_that_sits_after_its_control() {
        // The implicit-close extent is capped at the nested control, but a real
        // `</label>` must not be: `<label><input>Subscribe</label>` labels the
        // control with the text that trails it.
        let (_, label) = only_field("<form><label><input name=\"q\">Subscribe</label></form>");
        assert_eq!(label, "Subscribe");
    }

    #[test]
    fn a_wrapping_label_names_only_its_first_control() {
        let forms = parse_forms(
            "https://example.com/",
            "<form><label>Outer<input name=\"a\"><input name=\"b\"></label></form>",
        );
        assert_eq!(forms[0].fields.len(), 2);
        assert_eq!(forms[0].fields[0].label, "Outer");
        assert_eq!(forms[0].fields[1].label, "b");
    }

    #[test]
    fn aria_label_outranks_a_bound_label_which_outranks_a_placeholder() {
        // The order bug #45 established for the native engine. Placeholder used
        // to be consulted before aria-label here, which is backwards.
        let (_, aria_wins) = only_field(
            "<form><label for=\"x\">Bound</label><input id=\"x\" name=\"q\" aria-label=\"Aria\" placeholder=\"Ph\"></form>",
        );
        assert_eq!(aria_wins, "Aria");
        let (_, bound_wins) = only_field(
            "<form><label for=\"x\">Bound</label><input id=\"x\" name=\"q\" placeholder=\"Ph\"></form>",
        );
        assert_eq!(bound_wins, "Bound");
        let (_, placeholder_last) =
            only_field("<form><input name=\"q\" placeholder=\"Ph\"></form>");
        assert_eq!(placeholder_last, "Ph");
    }

    #[test]
    fn an_unlabelled_field_still_falls_back_to_its_name() {
        let (_, label) = only_field("<form><input name=\"q\"></form>");
        assert_eq!(label, "q");
    }
}
