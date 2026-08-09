#![allow(dead_code)]

use super::*;
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};

pub fn crawl_page_snapshot(url: &str) -> Result<BrowserPageSnapshot, String> {
    let mut session = BrowserSessionState {
        id: "ephemeral".to_string(),
        current_url: Some(url.to_string()),
        cookies: Vec::new(),
        runtime_cookies: Vec::new(),
        local_storage: HashMap::new(),
        session_storage: HashMap::new(),
        network: BrowserSessionNetworkConfig::default(),
        last_html: None,
    };
    crawl_page_snapshot_with_session(&mut session, url)
}

pub fn crawl_page_snapshot_with_session(
    session: &mut BrowserSessionState,
    url: &str,
) -> Result<BrowserPageSnapshot, String> {
    let response = fetch_with_session(url, "GET", None, &session.cookies, &session.network)?;
    for cookie in response.cookies.iter().cloned() {
        merge_cookie(&mut session.cookies, cookie);
    }
    apply_storage_updates(&mut session.local_storage, &response.local_storage_updates);
    apply_storage_updates(
        &mut session.session_storage,
        &response.session_storage_updates,
    );
    session.current_url = Some(url.to_string());
    let storage = storage_buckets(session);
    session.current_url = Some(response.final_url.clone());
    session.last_html = Some(response.html.clone());
    Ok(parse_html_to_snapshot_with_runtime_state(
        &response.final_url,
        &response.html,
        &session.cookies,
        &storage,
        &response.mutations,
        &response.requests,
        &response.settle_signals,
        &response.runtime_state,
        &response.protocol_events,
    ))
}

pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
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

pub fn role_actionability(role: &str) -> u8 {
    match role.to_ascii_lowercase().as_str() {
        "link" => 80,
        "button" => 70,
        "textbox" => 40,
        _ => 10,
    }
}

fn element_actionability_score(element: &AomElement) -> i32 {
    let mut score =
        i32::from(role_actionability(&element.role)).max(i32::from(element.actionability));
    if element.target_url.is_some() {
        score += 30;
    }
    if !element.value.is_empty() {
        score += 5;
    }
    score.clamp(0, 255)
}

pub fn describe_element_actionability(element: &AomElement) -> BrowserTargetActionability {
    let score = element_actionability_score(element) as u8;
    let actionable = match element.role.to_ascii_lowercase().as_str() {
        "link" => element.target_url.is_some(),
        "button" => element.target_url.is_some(),
        "textbox" => score >= 40,
        _ => score >= 40,
    };
    let reason = match element.role.to_ascii_lowercase().as_str() {
        "link" if element.target_url.is_none() => {
            "matched link lacks a navigable target in the current static browser engine".to_string()
        }
        "button" if element.target_url.is_none() => {
            "matched button has no navigable target; use browser_session_submit for forms or a richer runtime for JS buttons".to_string()
        }
        "textbox" if score < 40 => "matched textbox is present but weakly actionable".to_string(),
        _ if actionable => "semantic target is actionable in the current browser model".to_string(),
        _ => "semantic target is present but not actionable in the current browser model".to_string(),
    };
    BrowserTargetActionability {
        kind: "element".to_string(),
        role: element.role.clone(),
        name: element.name.clone(),
        score,
        actionable,
        reason,
        supported_actions: element.supported_actions.clone(),
        provenance: element.provenance.clone(),
        target_url: element.target_url.clone(),
    }
}

pub fn describe_form_field_actionability(field: &BrowserFormField) -> BrowserTargetActionability {
    let hidden = field.input_type.eq_ignore_ascii_case("hidden");
    BrowserTargetActionability {
        kind: "form_field".to_string(),
        role: field.input_type.clone(),
        name: if field.label.is_empty() {
            field.name.clone()
        } else {
            field.label.clone()
        },
        score: if hidden { 0 } else { 80 },
        actionable: !hidden,
        reason: if hidden {
            "matched form field is hidden and not actionable for browser_session_fill".to_string()
        } else {
            "form field is actionable in the current browser model".to_string()
        },
        supported_actions: vec!["fill".to_string()],
        provenance: "native".to_string(),
        target_url: None,
    }
}

pub fn describe_form_actionability(form: &BrowserForm) -> BrowserTargetActionability {
    let actionable = !form.action.trim().is_empty();
    BrowserTargetActionability {
        kind: "form".to_string(),
        role: form.method.clone(),
        name: if form.id.is_empty() {
            "default".to_string()
        } else {
            form.id.clone()
        },
        score: if actionable { 75 } else { 35 },
        actionable,
        reason: if actionable {
            "form submit target is actionable in the current browser model".to_string()
        } else {
            "matched form has no explicit action URL, which this static browser engine cannot safely infer".to_string()
        },
        supported_actions: vec!["submit".to_string()],
        provenance: "native".to_string(),
        target_url: if actionable {
            Some(form.action.clone())
        } else {
            None
        },
    }
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
    Some(
        best + if field.input_type.eq_ignore_ascii_case("hidden") {
            0
        } else {
            25
        },
    )
}

pub fn find_element<'a>(
    snapshot: &'a BrowserPageSnapshot,
    role: &str,
    name: &str,
) -> Option<&'a AomElement> {
    snapshot
        .elements
        .iter()
        .filter_map(|element| {
            element_match_score(element, role, name).map(|score| (score, element))
        })
        .max_by(
            |&(left_score, left_element): &(i32, &AomElement),
             &(right_score, right_element): &(i32, &AomElement)| {
                left_score
                    .cmp(&right_score)
                    .then_with(|| right_element.name.len().cmp(&left_element.name.len()))
            },
        )
        .map(|(_, element)| element)
}

pub fn find_form<'a>(
    snapshot: &'a BrowserPageSnapshot,
    form_id: Option<&str>,
) -> Option<&'a BrowserForm> {
    match form_id {
        Some(id) => snapshot
            .forms
            .iter()
            .find(|form| form.id.eq_ignore_ascii_case(id)),
        None => snapshot.forms.first(),
    }
}

pub fn find_form_field<'a>(
    snapshot: &'a BrowserPageSnapshot,
    field_name: &str,
) -> Option<&'a BrowserFormField> {
    snapshot
        .forms
        .iter()
        .flat_map(|form| form.fields.iter())
        .filter_map(|field| form_field_match_score(field, field_name).map(|score| (score, field)))
        .max_by(
            |&(left_score, left_field): &(i32, &BrowserFormField),
             &(right_score, right_field): &(i32, &BrowserFormField)| {
                left_score
                    .cmp(&right_score)
                    .then_with(|| right_field.label.len().cmp(&left_field.label.len()))
            },
        )
        .map(|(_, field)| field)
}

pub fn find_textbox_element<'a>(
    snapshot: &'a BrowserPageSnapshot,
    field_name: &str,
) -> Option<&'a AomElement> {
    snapshot
        .elements
        .iter()
        .filter(|element| element.role.eq_ignore_ascii_case("textbox"))
        .filter_map(|element| {
            string_match_score(&element.name, field_name).map(|score| (score, element))
        })
        .max_by(
            |&(left_score, left_element): &(i32, &AomElement),
             &(right_score, right_element): &(i32, &AomElement)| {
                left_score
                    .cmp(&right_score)
                    .then_with(|| right_element.name.len().cmp(&left_element.name.len()))
            },
        )
        .map(|(_, element)| element)
}

pub fn extract_snapshot_value(
    snapshot: &BrowserPageSnapshot,
    source: &str,
    role: Option<&str>,
    name: Option<&str>,
    field: Option<&str>,
) -> Result<String, String> {
    match source.to_ascii_lowercase().as_str() {
        "title" => Ok(snapshot.title.clone()),
        "summary" => Ok(snapshot.summary.clone()),
        "url" => Ok(snapshot.url.clone()),
        "element" => {
            let req_role = role.ok_or_else(|| "extract element requires role".to_string())?;
            let req_name = name.ok_or_else(|| "extract element requires name".to_string())?;
            let element = find_element(snapshot, req_role, req_name).ok_or_else(|| {
                format!(
                    "extract element not found: role='{}' name='{}'",
                    req_role, req_name
                )
            })?;
            Ok(element.value.clone())
        }
        "field" => {
            let req_field = field
                .or(name)
                .ok_or_else(|| "extract field requires field name".to_string())?;
            let form_field = find_form_field(snapshot, req_field)
                .ok_or_else(|| format!("extract field not found: '{}'", req_field))?;
            Ok(form_field.value.clone())
        }
        other => Err(format!("unsupported extract source '{}'", other)),
    }
}

pub fn snapshot_contains_text(snapshot: &BrowserPageSnapshot, needle: &str) -> bool {
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

fn snapshot_mutation_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot.mutations.iter().cloned().collect::<HashSet<_>>()
}

fn request_signature(request: &BrowserRequestRecord) -> String {
    format!(
        "{}:{}:{}:{}",
        request.method, request.url, request.status_code, request.resource
    )
}

fn snapshot_request_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .requests
        .iter()
        .map(request_signature)
        .collect::<HashSet<_>>()
}

fn snapshot_settle_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .settle_signals
        .iter()
        .cloned()
        .collect::<HashSet<_>>()
}

fn runtime_state_signature(entry: &BrowserRuntimeState) -> String {
    format!("{}:{}={}", entry.scope, entry.key, entry.value)
}

fn snapshot_runtime_state_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .runtime_state
        .iter()
        .map(runtime_state_signature)
        .collect::<HashSet<_>>()
}

fn snapshot_protocol_event_signatures(snapshot: &BrowserPageSnapshot) -> HashSet<String> {
    snapshot
        .protocol_events
        .iter()
        .map(protocol_event_signature)
        .collect::<HashSet<_>>()
}

pub fn diff_snapshots(
    before: &BrowserPageSnapshot,
    after: &BrowserPageSnapshot,
) -> BrowserSnapshotDiff {
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
    let before_forms = before
        .forms
        .iter()
        .map(form_signature)
        .collect::<HashSet<_>>();
    let after_forms = after
        .forms
        .iter()
        .map(form_signature)
        .collect::<HashSet<_>>();
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
    let before_mutations = snapshot_mutation_signatures(before);
    let after_mutations = snapshot_mutation_signatures(after);
    let before_requests = snapshot_request_signatures(before);
    let after_requests = snapshot_request_signatures(after);
    let before_settle_signals = snapshot_settle_signatures(before);
    let after_settle_signals = snapshot_settle_signatures(after);
    let before_runtime_state = snapshot_runtime_state_signatures(before);
    let after_runtime_state = snapshot_runtime_state_signatures(after);
    let before_protocol_events = snapshot_protocol_event_signatures(before);
    let after_protocol_events = snapshot_protocol_event_signatures(after);

    let mut added_elements = after_elements
        .difference(&before_elements)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_elements = before_elements
        .difference(&after_elements)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_forms = after_forms
        .difference(&before_forms)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_forms = before_forms
        .difference(&after_forms)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_cookies = after_cookies
        .difference(&before_cookies)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_cookies = before_cookies
        .difference(&after_cookies)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_storage = after_storage
        .difference(&before_storage)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_storage = before_storage
        .difference(&after_storage)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_mutations = after_mutations
        .difference(&before_mutations)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_mutations = before_mutations
        .difference(&after_mutations)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_requests = after_requests
        .difference(&before_requests)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_requests = before_requests
        .difference(&after_requests)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_settle_signals = after_settle_signals
        .difference(&before_settle_signals)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_settle_signals = before_settle_signals
        .difference(&after_settle_signals)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_runtime_state = after_runtime_state
        .difference(&before_runtime_state)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_runtime_state = before_runtime_state
        .difference(&after_runtime_state)
        .cloned()
        .collect::<Vec<_>>();
    let mut added_protocol_events = after_protocol_events
        .difference(&before_protocol_events)
        .cloned()
        .collect::<Vec<_>>();
    let mut removed_protocol_events = before_protocol_events
        .difference(&after_protocol_events)
        .cloned()
        .collect::<Vec<_>>();

    added_elements.sort();
    removed_elements.sort();
    added_forms.sort();
    removed_forms.sort();
    added_cookies.sort();
    removed_cookies.sort();
    added_storage.sort();
    removed_storage.sort();
    added_mutations.sort();
    removed_mutations.sort();
    added_requests.sort();
    removed_requests.sort();
    added_settle_signals.sort();
    removed_settle_signals.sort();
    added_runtime_state.sort();
    removed_runtime_state.sort();
    added_protocol_events.sort();
    removed_protocol_events.sort();

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
        added_mutations,
        removed_mutations,
        added_requests,
        removed_requests,
        added_settle_signals,
        removed_settle_signals,
        added_runtime_state,
        removed_runtime_state,
        added_protocol_events,
        removed_protocol_events,
    }
}

pub fn render_snapshot_diff(diff: &BrowserSnapshotDiff) -> String {
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
    if !diff.added_mutations.is_empty() {
        parts.push(format!("mutations+{}", diff.added_mutations.len()));
    }
    if !diff.removed_mutations.is_empty() {
        parts.push(format!("mutations-{}", diff.removed_mutations.len()));
    }
    if !diff.added_requests.is_empty() {
        parts.push(format!("requests+{}", diff.added_requests.len()));
    }
    if !diff.removed_requests.is_empty() {
        parts.push(format!("requests-{}", diff.removed_requests.len()));
    }
    if !diff.added_settle_signals.is_empty() {
        parts.push(format!("settle+{}", diff.added_settle_signals.len()));
    }
    if !diff.removed_settle_signals.is_empty() {
        parts.push(format!("settle-{}", diff.removed_settle_signals.len()));
    }
    if !diff.added_runtime_state.is_empty() {
        parts.push(format!("runtime+{}", diff.added_runtime_state.len()));
    }
    if !diff.removed_runtime_state.is_empty() {
        parts.push(format!("runtime-{}", diff.removed_runtime_state.len()));
    }
    if !diff.added_protocol_events.is_empty() {
        parts.push(format!("protocol+{}", diff.added_protocol_events.len()));
    }
    if !diff.removed_protocol_events.is_empty() {
        parts.push(format!("protocol-{}", diff.removed_protocol_events.len()));
    }

    if parts.is_empty() {
        "no_semantic_change".to_string()
    } else {
        parts.join(",")
    }
}

pub fn is_semantically_stable(diff: &BrowserSnapshotDiff) -> bool {
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
        && diff.added_mutations.is_empty()
        && diff.removed_mutations.is_empty()
        && diff.added_requests.is_empty()
        && diff.removed_requests.is_empty()
        && diff.added_settle_signals.is_empty()
        && diff.removed_settle_signals.is_empty()
        && diff.added_runtime_state.is_empty()
        && diff.removed_runtime_state.is_empty()
        && diff.added_protocol_events.is_empty()
        && diff.removed_protocol_events.is_empty()
}

pub fn wait_for_condition<F>(
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
            return Err(format!(
                "wait condition not satisfied within {}ms",
                timeout.as_millis()
            ));
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

pub fn wait_for_stable_snapshot(
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
            return Err(format!(
                "wait for stable snapshot not satisfied within {}ms",
                timeout.as_millis()
            ));
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
