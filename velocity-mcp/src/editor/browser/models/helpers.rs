use super::super::engine::summarize_network_activity;
use super::*;
use std::collections::HashMap;

pub fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

pub fn parse_list_sort_direction(value: Option<&str>) -> Result<BrowserListSortDirection, String> {
    match value {
        None => Ok(BrowserListSortDirection::Asc),
        Some(direction) if direction.eq_ignore_ascii_case("asc") => {
            Ok(BrowserListSortDirection::Asc)
        }
        Some(direction) if direction.eq_ignore_ascii_case("desc") => {
            Ok(BrowserListSortDirection::Desc)
        }
        Some(direction) => Err(format!(
            "invalid sort direction '{direction}', expected 'asc' or 'desc'"
        )),
    }
}

pub fn finalize_list<T, F>(
    items: &mut Vec<T>,
    sort_direction: BrowserListSortDirection,
    limit: Option<usize>,
    mut compare: F,
) where
    F: FnMut(&T, &T) -> std::cmp::Ordering,
{
    items.sort_by(|left, right| {
        let ordering = compare(left, right);
        match sort_direction {
            BrowserListSortDirection::Asc => ordering,
            BrowserListSortDirection::Desc => ordering.reverse(),
        }
    });
    if let Some(limit) = limit {
        items.truncate(limit);
    }
}

pub fn summarize_session(session: BrowserSessionState) -> BrowserSessionSummary {
    BrowserSessionSummary {
        id: session.id,
        current_url: session.current_url,
        cookie_count: session.cookies.len(),
        local_storage_count: session.local_storage.len(),
        session_storage_count: session.session_storage.len(),
        network_header_count: session.network.headers.len(),
        has_network_policy: session.network.user_agent.is_some()
            || session.network.timeout_ms.is_some()
            || session.network.follow_redirects.is_some()
            || !session.network.headers.is_empty()
            || !session.network.allowed_url_prefixes.is_empty()
            || !session.network.blocked_url_prefixes.is_empty(),
        session_json_path: None,
    }
}

pub fn summarize_session_transcript_entry(
    entry: BrowserSessionTranscriptEntry,
) -> BrowserSessionTranscriptEntrySummary {
    BrowserSessionTranscriptEntrySummary {
        sequence: entry.sequence,
        timestamp_ms: entry.timestamp_ms,
        event_kind: entry.event_kind,
        outcome: entry.outcome,
        summary: entry.summary,
        session_id: entry.session_id,
        url: entry.url,
        title: entry.title,
        target: entry.target,
    }
}

pub fn summarize_auth_profile(profile: BrowserAuthProfile) -> BrowserAuthProfileSummary {
    BrowserAuthProfileSummary {
        name: profile.name,
        source_kind: profile.source_kind,
        source_session_id: profile.source_session_id,
        source_checkpoint_name: profile.source_checkpoint_name,
        current_url: profile.current_url,
        cookie_count: profile.cookies.len(),
        cookie_names: summarize_cookie_names(&profile.cookies),
        local_storage_count: profile.local_storage.len(),
        local_storage_keys: summarize_sorted_keys(&profile.local_storage),
        session_storage_count: profile.session_storage.len(),
        session_storage_keys: summarize_sorted_keys(&profile.session_storage),
        diagnosis: profile.auth_diagnostics.diagnosis,
        recommended_action: profile.auth_diagnostics.recommended_action,
        json_path: None,
    }
}

pub fn summarize_session_checkpoint(
    checkpoint: BrowserSessionCheckpoint,
) -> BrowserSessionCheckpointSummary {
    let network_summary = checkpoint
        .snapshot
        .as_ref()
        .map(|snapshot| summarize_network_activity(&snapshot.protocol_events))
        .unwrap_or_default();
    BrowserSessionCheckpointSummary {
        name: checkpoint.name,
        session_id: checkpoint.session.id,
        has_snapshot: checkpoint.snapshot.is_some(),
        current_url: checkpoint.session.current_url,
        title: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.title.clone()),
        snapshot_summary: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.summary.clone()),
        element_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.elements.len())
            .unwrap_or(0),
        form_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.forms.len())
            .unwrap_or(0),
        mutation_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.mutations.len())
            .unwrap_or(0),
        request_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.requests.len())
            .unwrap_or(0),
        settle_signal_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.settle_signals.len())
            .unwrap_or(0),
        runtime_state_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.runtime_state.len())
            .unwrap_or(0),
        protocol_event_count: checkpoint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.protocol_events.len())
            .unwrap_or(0),
        network_summary,
        cookie_count: checkpoint.session.cookies.len(),
        local_storage_count: checkpoint.session.local_storage.len(),
        session_storage_count: checkpoint.session.session_storage.len(),
        checkpoint_json_path: None,
    }
}

pub fn summarize_cookie_names(cookies: &[BrowserCookie]) -> Vec<String> {
    let mut names: Vec<String> = cookies.iter().map(|c| c.name.clone()).collect();
    names.sort();
    names
}

pub fn summarize_sorted_keys(map: &HashMap<String, String>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}
