use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserListSortDirection {
    Asc,
    Desc,
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
