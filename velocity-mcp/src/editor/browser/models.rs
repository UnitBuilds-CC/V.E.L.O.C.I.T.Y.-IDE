use super::engine::summarize_network_activity;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use velocity_ide::site_map::verifier::NdaNode;
use velocity_ide::site_map::{SiteMap, VcTriple};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserListSortDirection {
    Asc,
    Desc,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBrowserCookie {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_unix: Option<i64>,
    #[serde(default)]
    pub session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scheme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_port: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeActionApiResult {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub wait_applied_ms: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeBrowserSessionState {
    pub id: String,
    pub runtime_session_id: String,
    pub api_base: String,
    pub current_url: Option<String>,
    pub last_title: Option<String>,
    #[serde(default)]
    pub cookies: Vec<RuntimeBrowserCookie>,
    #[serde(default)]
    pub local_storage: HashMap<String, String>,
    #[serde(default)]
    pub session_storage: HashMap<String, String>,
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
pub struct BrowserRequestRecord {
    pub method: String,
    pub url: String,
    pub status_code: u16,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserRuntimeState {
    pub scope: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserProtocolEvent {
    pub kind: String,
    pub phase: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFrameInventoryEntry {
    pub selector: String,
    pub name: String,
    pub title: String,
    pub source: String,
    pub same_origin: bool,
    pub accessible: bool,
    pub semantic_node_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserShadowHostInventoryEntry {
    pub selector: String,
    pub tag: String,
    pub role: String,
    pub mode: String,
    pub semantic_node_count: usize,
    pub text_sample: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCanvasInventoryEntry {
    pub selector: String,
    pub width: usize,
    pub height: usize,
    #[serde(default)]
    pub context_kinds: Vec<String>,
    pub text_op_count: usize,
    pub image_op_count: usize,
    pub webgl_draw_count: usize,
    pub readback_count: usize,
    pub likely_animated: bool,
    pub runtime_evidence: bool,
    pub text_sample: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BrowserNetworkSummary {
    pub redirect_count: usize,
    pub download_count: usize,
    pub upload_count: usize,
    pub stream_count: usize,
    pub event_stream_count: usize,
    pub websocket_count: usize,
    pub event_count: usize,
    pub other_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_redirect_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_download_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_upload_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stream_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_stream_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_websocket_target: Option<String>,
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
    #[serde(default)]
    pub mutations: Vec<String>,
    #[serde(default)]
    pub requests: Vec<BrowserRequestRecord>,
    #[serde(default)]
    pub settle_signals: Vec<String>,
    #[serde(default)]
    pub runtime_state: Vec<BrowserRuntimeState>,
    #[serde(default)]
    pub protocol_events: Vec<BrowserProtocolEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceEntry {
    pub timestamp: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(default)]
    pub level: Option<String>,
    pub message: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceSummaryReport {
    #[serde(rename = "totalEntries")]
    pub total_entries: usize,
    #[serde(rename = "consoleCount")]
    pub console_count: usize,
    #[serde(rename = "networkCount")]
    pub network_count: usize,
    #[serde(rename = "mutationCount")]
    pub mutation_count: usize,
    #[serde(rename = "screenshotCount")]
    pub screenshot_count: usize,
    #[serde(rename = "warningCount")]
    pub warning_count: usize,
    #[serde(rename = "recentEntries")]
    pub recent_entries: Vec<TraceEntry>,
    #[serde(rename = "latestScreenshot", default)]
    pub latest_screenshot: Option<String>,
    #[serde(rename = "healthImpact", default)]
    pub health_impact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshotSummary {
    pub url: String,
    pub title: String,
    pub element_count: usize,
    pub form_count: usize,
    pub cookie_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BrowserSessionNetworkConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default)]
    pub allowed_url_prefixes: Vec<String>,
    #[serde(default)]
    pub blocked_url_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionState {
    pub id: String,
    pub current_url: Option<String>,
    pub cookies: Vec<BrowserCookie>,
    #[serde(default)]
    pub runtime_cookies: Vec<RuntimeBrowserCookie>,
    #[serde(default)]
    pub local_storage: HashMap<String, String>,
    #[serde(default)]
    pub session_storage: HashMap<String, String>,
    #[serde(default)]
    pub network: BrowserSessionNetworkConfig,
    #[serde(skip, default)]
    pub last_html: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionSummary {
    pub id: String,
    pub current_url: Option<String>,
    pub cookie_count: usize,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub network_header_count: usize,
    #[serde(default)]
    pub has_network_policy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_json_path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionCheckpoint {
    pub name: String,
    pub session: BrowserSessionState,
    pub snapshot: Option<BrowserPageSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionCheckpointSummary {
    pub name: String,
    pub session_id: String,
    pub has_snapshot: bool,
    pub current_url: Option<String>,
    pub title: Option<String>,
    pub snapshot_summary: Option<String>,
    pub element_count: usize,
    pub form_count: usize,
    pub mutation_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub cookie_count: usize,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_json_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWebNavigateReport {
    pub snapshot: BrowserSnapshotSummary,
    pub snapshot_summary: String,
    pub snapshot_json_path: String,
    pub nda_facts_path: String,
    pub sitemap_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionCreateReport {
    pub session: BrowserSessionSummary,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionReadReport {
    pub session: BrowserSessionSummary,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionNetworkReadReport {
    pub session: BrowserSessionSummary,
    pub network: BrowserSessionNetworkConfig,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionNetworkUpdateReport {
    pub session: BrowserSessionSummary,
    pub network: BrowserSessionNetworkConfig,
    pub updated_header_count: usize,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserStorageReadReport {
    pub session: BrowserSessionSummary,
    pub scope: String,
    pub entry_count: usize,
    pub entries: HashMap<String, String>,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserStorageUpdateReport {
    pub session: BrowserSessionSummary,
    pub scope: String,
    pub updated_entry_count: usize,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCookieReadReport {
    pub session: BrowserSessionSummary,
    pub cookie_count: usize,
    pub cookie_names: Vec<String>,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCookieUpdateReport {
    pub session: BrowserSessionSummary,
    pub updated_cookie_count: usize,
    pub cookie_count: usize,
    pub cookie_names: Vec<String>,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthDiagnosticsReport {
    pub session: BrowserSessionSummary,
    pub diagnosis: String,
    pub recommended_action: String,
    pub snapshot_available: bool,
    pub has_login_form: bool,
    pub has_auth_cookie: bool,
    pub has_csrf_token: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub router_name: Option<String>,
    pub auth_signal_count: usize,
    pub auth_signals: Vec<String>,
    pub session_json_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_json_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthReseedReport {
    pub target_session: BrowserSessionSummary,
    pub source_kind: String,
    pub source_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_checkpoint_name: Option<String>,
    pub copied_cookie_count: usize,
    pub copied_cookie_names: Vec<String>,
    pub copied_local_storage_count: usize,
    pub copied_local_storage_keys: Vec<String>,
    pub copied_session_storage_count: usize,
    pub copied_session_storage_keys: Vec<String>,
    pub session_json_path: String,
    pub auth_diagnostics: BrowserAuthDiagnosticsReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeAuthReseedReport {
    pub target_runtime_session: RuntimeBrowserSessionState,
    pub source_kind: String,
    pub source_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_checkpoint_name: Option<String>,
    pub copied_cookie_count: usize,
    pub copied_cookie_names: Vec<String>,
    pub copied_local_storage_count: usize,
    pub copied_local_storage_keys: Vec<String>,
    pub copied_session_storage_count: usize,
    pub copied_session_storage_keys: Vec<String>,
    pub session_json_path: String,
    pub auth_diagnostics: BrowserAuthDiagnosticsReport,
    pub warning_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthProfile {
    pub name: String,
    pub source_kind: String,
    pub source_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_checkpoint_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_url: Option<String>,
    pub cookies: Vec<BrowserCookie>,
    pub local_storage: HashMap<String, String>,
    pub session_storage: HashMap<String, String>,
    pub auth_diagnostics: BrowserAuthDiagnosticsReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthProfileSummary {
    pub name: String,
    pub source_kind: String,
    pub source_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_checkpoint_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_url: Option<String>,
    pub cookie_count: usize,
    pub cookie_names: Vec<String>,
    pub local_storage_count: usize,
    pub local_storage_keys: Vec<String>,
    pub session_storage_count: usize,
    pub session_storage_keys: Vec<String>,
    pub diagnosis: String,
    pub recommended_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthProfileSaveReport {
    pub profile: BrowserAuthProfileSummary,
    pub profile_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthProfileReadReport {
    pub profile: BrowserAuthProfileSummary,
    pub profile_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAuthProfileApplyReport {
    pub profile_name: String,
    pub target_session: BrowserSessionSummary,
    pub copied_cookie_count: usize,
    pub copied_cookie_names: Vec<String>,
    pub copied_local_storage_count: usize,
    pub copied_local_storage_keys: Vec<String>,
    pub copied_session_storage_count: usize,
    pub copied_session_storage_keys: Vec<String>,
    pub session_json_path: String,
    pub profile_json_path: String,
    pub auth_diagnostics: BrowserAuthDiagnosticsReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserAccessDiagnosticsReport {
    pub session: BrowserSessionSummary,
    pub diagnosis: String,
    pub recommended_action: String,
    pub snapshot_available: bool,
    pub challenge_signal_count: usize,
    pub challenge_signals: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub router_name: Option<String>,
    pub session_json_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_json_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCompatibilityReport {
    pub level: String,
    pub cause: String,
    pub summary: String,
    pub recommended_action: String,
    pub signal_count: usize,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionHealthReport {
    pub session: BrowserSessionSummary,
    pub network: BrowserSessionNetworkConfig,
    pub auth_diagnostics: BrowserAuthDiagnosticsReport,
    pub access_diagnostics: BrowserAccessDiagnosticsReport,
    pub compatibility: BrowserCompatibilityReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<BrowserSnapshotSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
    pub checkpoint_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_checkpoint: Option<BrowserSessionCheckpointSummary>,
    pub recent_failure_count: usize,
    pub recent_failures: Vec<BrowserSessionTranscriptEntrySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_failure: Option<BrowserSessionTranscriptEntrySummary>,
    pub recovery_posture: String,
    pub recommended_action: String,
    pub evidence_signal_count: usize,
    pub evidence_signals: Vec<String>,
    pub session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionTranscriptEntry {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub event_kind: String,
    pub outcome: String,
    pub summary: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_summary: Option<String>,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub session_json_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_json_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_json_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nda_facts_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionTranscriptEntrySummary {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub event_kind: String,
    pub outcome: String,
    pub summary: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionTranscriptReadReport {
    pub session: BrowserSessionSummary,
    pub entry_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_sequence: Option<u64>,
    pub transcript_json_path: String,
    pub entries: Vec<BrowserSessionTranscriptEntrySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCheckpointSaveReport {
    pub checkpoint: BrowserSessionCheckpointSummary,
    pub checkpoint_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionNavigationReport {
    pub session_id: String,
    pub requested_url: String,
    pub url: String,
    pub title: String,
    pub form_count: usize,
    pub cookie_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub snapshot_json_path: String,
    pub session_json_path: String,
    pub nda_facts_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserRuntimeCaptureReport {
    pub session_id: String,
    pub url: String,
    pub title: String,
    pub form_count: usize,
    pub cookie_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    pub frame_count: usize,
    pub shadow_host_count: usize,
    pub canvas_count: usize,
    pub webgl_canvas_count: usize,
    #[serde(default)]
    pub frames: Vec<BrowserFrameInventoryEntry>,
    #[serde(default)]
    pub shadow_hosts: Vec<BrowserShadowHostInventoryEntry>,
    #[serde(default)]
    pub canvases: Vec<BrowserCanvasInventoryEntry>,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub snapshot_json_path: String,
    pub session_json_path: String,
    pub nda_facts_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
    pub capture_backend: String,
    pub aom_summary_chars: usize,
    pub warning_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RuntimeActionApiResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeBrowserSessionReadReport {
    pub session: RuntimeBrowserSessionState,
    pub session_json_path: String,
    pub warning_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeBrowserSessionCloseReport {
    pub session_id: String,
    pub runtime_session_id: String,
    pub removed_session_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTargetActionability {
    pub kind: String,
    pub role: String,
    pub name: String,
    pub score: u8,
    pub actionable: bool,
    pub reason: String,
    #[serde(default)]
    pub supported_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provenance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionActionReport {
    pub action: String,
    pub target: String,
    pub session_id: String,
    pub url: String,
    pub title: String,
    pub diff_summary: String,
    pub form_count: usize,
    pub cookie_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub snapshot_json_path: String,
    pub session_json_path: String,
    pub nda_facts_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_actionability: Option<BrowserTargetActionability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSessionWaitReport {
    pub session_id: String,
    pub requested_url: String,
    pub url: String,
    pub title: String,
    pub diff_summary: String,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub snapshot_json_path: String,
    pub session_json_path: String,
    pub nda_facts_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_target_actionability: Option<BrowserTargetActionability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCheckpointRestoreReport {
    pub checkpoint_name: String,
    pub session_id: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub session_json_path: String,
    pub snapshot_json_path: Option<String>,
    pub nda_facts_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
    pub auth_diagnostics: BrowserAuthDiagnosticsReport,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserWorkflowStep {
    Navigate {
        url: String,
    },
    Click {
        role: String,
        name: String,
    },
    FillField {
        field: String,
        value: String,
    },
    SubmitForm {
        form: Option<String>,
    },
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
    WaitForMutation {
        label: String,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForRequest {
        method: Option<String>,
        url_contains: Option<String>,
        status: Option<u16>,
        resource: Option<String>,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForStorage {
        scope: String,
        key: String,
        value: Option<String>,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForSettle {
        label: Option<String>,
        scope: Option<String>,
        state: Option<String>,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForRuntimeState {
        scope: String,
        key: String,
        value: Option<String>,
        timeout_ms: Option<u64>,
        interval_ms: Option<u64>,
    },
    WaitForProtocolEvent {
        event_kind: Option<String>,
        phase: Option<String>,
        target: Option<String>,
        detail: Option<String>,
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
    SaveCheckpoint {
        name: String,
    },
    RestoreCheckpoint {
        name: String,
    },
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
    AssertElement {
        role: String,
        name: String,
    },
    AssertTextContains {
        text: String,
    },
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
    pub added_mutations: Vec<String>,
    pub removed_mutations: Vec<String>,
    pub added_requests: Vec<String>,
    pub removed_requests: Vec<String>,
    pub added_settle_signals: Vec<String>,
    pub removed_settle_signals: Vec<String>,
    pub added_runtime_state: Vec<String>,
    pub removed_runtime_state: Vec<String>,
    pub added_protocol_events: Vec<String>,
    pub removed_protocol_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshotDiffReport {
    pub before_url: String,
    pub after_url: String,
    pub summary: String,
    pub diff: BrowserSnapshotDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshotDiffSummary {
    pub before_url: String,
    pub after_url: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshotDiffReadReport {
    pub diff: BrowserSnapshotDiffSummary,
    pub before_json_path: String,
    pub after_json_path: String,
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
pub struct BrowserWorkflowSummary {
    pub name: String,
    pub start_url: String,
    pub variable_count: usize,
    pub step_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nda_path: Option<String>,
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
    pub mutation_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    pub outputs: HashMap<String, String>,
    pub log: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowRunSummary {
    pub workflow_name: String,
    pub session_id: String,
    pub final_url: String,
    pub final_title: String,
    pub step_count: usize,
    pub cookie_count: usize,
    pub local_storage_count: usize,
    pub session_storage_count: usize,
    pub request_count: usize,
    pub settle_signal_count: usize,
    pub runtime_state_count: usize,
    pub protocol_event_count: usize,
    #[serde(default)]
    pub network_summary: BrowserNetworkSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuite {
    pub name: String,
    pub workflows: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteSummary {
    pub name: String,
    pub workflow_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteRunSummary {
    pub suite_name: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite_report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSnapshotReadReport {
    pub snapshot: BrowserSnapshotSummary,
    pub json_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserCheckpointReadReport {
    pub checkpoint: BrowserSessionCheckpointSummary,
    pub checkpoint_json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSaveReport {
    pub workflow: BrowserWorkflowSummary,
    pub json_path: String,
    pub nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowReadReport {
    pub workflow: BrowserWorkflowSummary,
    pub json_path: String,
    pub nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteSaveReport {
    pub suite: BrowserWorkflowSuiteSummary,
    pub json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteReadReport {
    pub suite: BrowserWorkflowSuiteSummary,
    pub json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowReplayReport {
    pub workflow: BrowserWorkflowRunSummary,
    pub snapshot_json_path: String,
    pub session_json_path: String,
    pub nda_facts_path: String,
    pub run_report_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_fallback_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowRunReadReport {
    pub workflow: BrowserWorkflowRunSummary,
    pub run_report_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserVisualFallbackReadReport {
    pub url: String,
    pub html_path: String,
    pub byte_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteExecutionReport {
    pub suite: BrowserWorkflowSuiteRunSummary,
    pub suite_report_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserWorkflowSuiteRunReadReport {
    pub suite: BrowserWorkflowSuiteRunSummary,
    pub suite_report_path: String,
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
