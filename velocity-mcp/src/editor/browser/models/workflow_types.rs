use super::session_types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
