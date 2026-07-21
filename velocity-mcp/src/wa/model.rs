use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaListSortDirection {
    Asc,
    Desc,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaNode {
    pub id: String,
    pub role: String,
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaSession {
    pub id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub latest_snapshot_name: Option<String>,
    pub latest_snapshot_nda_path: Option<String>,
    pub snapshot_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaSnapshot {
    pub session_id: String,
    pub snapshot_name: String,
    pub created_at_ms: u64,
    pub url: String,
    pub title: String,
    pub focus_node_id: Option<String>,
    pub nodes: Vec<WaNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaScriptStep {
    pub action: String,
    pub node_id: Option<String>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub value: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaScript {
    pub name: String,
    pub created_at_ms: u64,
    pub start_url: Option<String>,
    pub steps: Vec<WaScriptStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaSessionCreateReport {
    pub session: WaSession,
    pub session_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaSessionReadReport {
    pub session: WaSession,
    pub session_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaSessionListEntry {
    pub id: String,
    pub snapshot_count: u32,
    pub latest_snapshot_name: Option<String>,
    pub updated_at_ms: u64,
    pub session_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaSnapshotSaveReport {
    pub snapshot: WaSnapshot,
    pub snapshot_nda_path: String,
    pub session_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaSnapshotReadReport {
    pub snapshot: WaSnapshot,
    pub snapshot_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaSnapshotListEntry {
    pub session_id: String,
    pub snapshot_name: String,
    pub url: String,
    pub title: String,
    pub node_count: usize,
    pub created_at_ms: u64,
    pub snapshot_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaScriptSaveReport {
    pub script: WaScript,
    pub relative_file_path: String,
    pub nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaScriptReadReport {
    pub script: WaScript,
    pub relative_file_path: String,
    pub nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaResolveSelectorReport {
    pub session_id: String,
    pub snapshot_name: String,
    pub action: Option<String>,
    pub selector: WaScriptStep,
    pub matched: WaNode,
    pub candidate_count: usize,
    pub snapshot_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaPlanActionReport {
    pub session_id: String,
    pub snapshot_name: String,
    pub action: String,
    pub input_value: Option<String>,
    pub selector: WaScriptStep,
    pub matched: WaNode,
    pub preconditions: Vec<String>,
    pub planned_step: WaScriptStep,
    pub snapshot_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaWindowsCaptureReport {
    pub source: String,
    pub target_process_id: Option<u32>,
    pub target_window_title: String,
    pub snapshot: WaSnapshot,
    pub snapshot_nda_path: String,
    pub session_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaWindowsActionReport {
    pub source: String,
    pub session_id: String,
    pub snapshot_name: String,
    pub action: String,
    pub requested_value: Option<String>,
    pub selector: WaScriptStep,
    pub matched: WaNode,
    pub preconditions: Vec<String>,
    pub target_process_id: Option<u32>,
    pub target_window_title: String,
    pub executed_node_id: String,
    pub execution_status: String,
    pub execution_detail: String,
    pub snapshot_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaWindowsWaitReport {
    pub source: String,
    pub session_id: String,
    pub snapshot_name: String,
    pub condition: String,
    pub expected_value: Option<String>,
    pub selector: WaScriptStep,
    pub matched: WaNode,
    pub target_process_id: Option<u32>,
    pub target_window_title: String,
    pub observed_value: Option<String>,
    pub satisfied: bool,
    pub elapsed_ms: u64,
    pub timeout_ms: u64,
    pub poll_interval_ms: u64,
    pub detail: String,
    pub snapshot_nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaScriptRunStepReport {
    pub index: usize,
    pub action: String,
    pub required: bool,
    pub node_id: Option<String>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub value: Option<String>,
    pub status: String,
    pub detail: String,
    pub verification_status: Option<String>,
    pub verification_detail: Option<String>,
    pub action_report: Option<WaWindowsActionReport>,
    pub wait_report: Option<WaWindowsWaitReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaScriptRunReport {
    pub run_id: String,
    pub created_at_ms: u64,
    pub source: String,
    pub session_id: String,
    pub snapshot_name: String,
    pub script_name: String,
    pub script_relative_file_path: String,
    pub script_nda_path: String,
    pub start_step_index: usize,
    pub step_count: usize,
    pub completed_step_count: usize,
    pub verified_step_count: usize,
    pub succeeded: bool,
    pub stopped_at_step_index: Option<usize>,
    pub steps: Vec<WaScriptRunStepReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaRunArtifactReport {
    pub run: WaScriptRunReport,
    pub relative_file_path: String,
    pub nda_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaRunListEntry {
    pub run_id: String,
    pub session_id: String,
    pub snapshot_name: String,
    pub script_name: String,
    pub start_step_index: usize,
    pub step_count: usize,
    pub completed_step_count: usize,
    pub verified_step_count: usize,
    pub succeeded: bool,
    pub stopped_at_step_index: Option<usize>,
    pub created_at_ms: u64,
    pub nda_path: String,
}
