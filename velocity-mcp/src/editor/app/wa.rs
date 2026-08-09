#![allow(dead_code)]
use super::types::*;
use crate::automation::AgentTaskKind;
use crate::editor::orchestrator_panel::OrchestratorTaskSnapshot;
use std::collections::BTreeMap;

pub fn desktop_automation_evidence_state(
    task: &OrchestratorTaskSnapshot,
) -> DesktopAutomationEvidenceState {
    if task.live_thread.is_some() || task.status_label == "Running" {
        DesktopAutomationEvidenceState::LiveEvidence
    } else if task.wa_run_path.is_some()
        || task.wa_run_id.is_some()
        || task.run_summary_path.is_some()
        || task.run_facts_path.is_some()
    {
        DesktopAutomationEvidenceState::ArtifactBacked
    } else {
        DesktopAutomationEvidenceState::AwaitingEvidence
    }
}

pub fn task_matches_desktop_automation_lane(
    task: &OrchestratorTaskSnapshot,
    mission_task_kind: Option<&str>,
) -> bool {
    if mission_task_kind == Some(AgentTaskKind::DesktopAutomation.as_str()) {
        return true;
    }
    let mut haystack = String::new();
    haystack.push_str(&task.title);
    haystack.push(' ');
    haystack.push_str(&task.description);
    haystack.push(' ');
    haystack.push_str(&task.rationale);
    haystack.push(' ');
    haystack.push_str(&task.message);
    for output in &task.outputs {
        haystack.push(' ');
        haystack.push_str(output);
    }
    let lower = haystack.to_lowercase();
    lower.contains("desktop automation")
        || lower.contains("windows automation")
        || lower.contains("desktop test")
        || lower.contains("uia")
        || lower.contains("wa ")
        || lower.starts_with("wa")
}

pub fn desktop_automation_evidence_lines(task: &OrchestratorTaskSnapshot) -> Vec<String> {
    let state = desktop_automation_evidence_state(task);
    let mut lines = vec![
        format!("Evidence state: {}", state.label()),
        state.detail().to_string(),
    ];
    if let Some(path) = &task.wa_run_path {
        lines.push(format!("WA run artifact: {path}"));
    }
    if let Some(run_id) = &task.wa_run_id {
        lines.push(format!("WA run id: {run_id}"));
    }
    if let Some(path) = &task.run_summary_path {
        lines.push(format!("Run summary artifact: {path}"));
    }
    if let Some(path) = &task.run_facts_path {
        lines.push(format!("NDA facts artifact: {path}"));
    }
    if !task.outputs.is_empty() {
        lines.push(format!("Reported outputs: {}", task.outputs.join(", ")));
    }
    if let Some(thread) = &task.live_thread {
        let evidence_event_count = thread
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    crate::orchestrator::worker::WorkerThreadEventKind::Status
                        | crate::orchestrator::worker::WorkerThreadEventKind::ToolStarted
                        | crate::orchestrator::worker::WorkerThreadEventKind::ToolFinished
                )
            })
            .count();
        if evidence_event_count > 0 {
            lines.push(format!(
                "Live worker evidence updates: {evidence_event_count}"
            ));
        }
        if !thread.changed_files.is_empty() {
            lines.push(format!(
                "Observed file activity: {}",
                thread.changed_files.join(", ")
            ));
        }
        if !thread.transcript.trim().is_empty() {
            lines.push("Live transcript captured for operator review.".to_string());
        }
        if !thread.operator_notes.is_empty() {
            lines.push(format!(
                "Operator notes recorded: {}",
                thread.operator_notes.len()
            ));
        }
    }
    lines
}

pub fn desktop_automation_selected_task_status(
    task: &OrchestratorTaskSnapshot,
) -> DesktopAutomationSelectedTaskStatus {
    let state = desktop_automation_evidence_state(task);
    let artifact_count = usize::from(task.wa_run_path.is_some())
        + usize::from(task.run_summary_path.is_some())
        + usize::from(task.run_facts_path.is_some());
    let evidence_update_count = task
        .live_thread
        .as_ref()
        .map(|thread| {
            thread
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        crate::orchestrator::worker::WorkerThreadEventKind::Status
                            | crate::orchestrator::worker::WorkerThreadEventKind::ToolStarted
                            | crate::orchestrator::worker::WorkerThreadEventKind::ToolFinished
                    )
                })
                .count()
        })
        .unwrap_or(0);
    let (has_transcript, has_operator_notes) = task
        .live_thread
        .as_ref()
        .map(|thread| {
            (
                !thread.transcript.trim().is_empty(),
                !thread.operator_notes.is_empty(),
            )
        })
        .unwrap_or((false, false));

    DesktopAutomationSelectedTaskStatus {
        state_label: state.label(),
        state_detail: state.detail(),
        artifact_count,
        output_count: task.outputs.len(),
        evidence_update_count,
        has_transcript,
        has_operator_notes,
    }
}

pub fn desktop_automation_selected_task_cues(
    task: &OrchestratorTaskSnapshot,
) -> DesktopAutomationSelectedTaskCues {
    let mut artifact_lines = Vec::new();
    if let Some(path) = &task.wa_run_path {
        artifact_lines.push(format!("WA run ready: {path}"));
    }
    if let Some(run_id) = &task.wa_run_id {
        artifact_lines.push(format!("WA run id: {run_id}"));
    }
    if let Some(path) = &task.run_summary_path {
        artifact_lines.push(format!("Run summary ready: {path}"));
    }
    if let Some(path) = &task.run_facts_path {
        artifact_lines.push(format!("NDA facts ready: {path}"));
    }
    if !task.outputs.is_empty() {
        artifact_lines.push(format!(
            "Reported outputs ready: {}",
            task.outputs.join(", ")
        ));
    }
    if let Some(thread) = &task.live_thread {
        if !thread.changed_files.is_empty() {
            artifact_lines.push(format!(
                "Observed file activity: {}",
                thread.changed_files.join(", ")
            ));
        }
    }

    let next_action = match desktop_automation_evidence_state(task) {
        DesktopAutomationEvidenceState::LiveEvidence => {
            "Monitor live WA evidence and intervene only if capture, action, or verification stalls."
        }
        DesktopAutomationEvidenceState::ArtifactBacked => {
            "Review the captured WA artifacts, then retry or follow up only if the evidence shows the desktop task is incomplete."
        }
        DesktopAutomationEvidenceState::AwaitingEvidence => {
            "Capture WA evidence or rerun the task before treating the desktop automation step as complete."
        }
    };

    DesktopAutomationSelectedTaskCues {
        artifact_lines,
        next_action,
    }
}

pub fn desktop_automation_mission_summary(
    tasks: &[OrchestratorTaskSnapshot],
    mission_task_kind: Option<&str>,
) -> Option<DesktopAutomationMissionSummary> {
    let desktop_tasks: Vec<&OrchestratorTaskSnapshot> = tasks
        .iter()
        .filter(|task| task_matches_desktop_automation_lane(task, mission_task_kind))
        .collect();
    if desktop_tasks.is_empty() {
        return None;
    }

    let mut live_count = 0usize;
    let mut artifact_count = 0usize;
    let mut awaiting_count = 0usize;
    let mut labels: BTreeMap<&'static str, usize> = BTreeMap::new();
    for task in desktop_tasks.iter().copied() {
        let state = desktop_automation_evidence_state(task);
        match state {
            DesktopAutomationEvidenceState::LiveEvidence => live_count += 1,
            DesktopAutomationEvidenceState::ArtifactBacked => artifact_count += 1,
            DesktopAutomationEvidenceState::AwaitingEvidence => awaiting_count += 1,
        }
        *labels.entry(state.label()).or_insert(0) += 1;
    }

    Some(DesktopAutomationMissionSummary {
        task_count: desktop_tasks.len(),
        live_count,
        artifact_count,
        awaiting_count,
        state_labels: labels
            .into_iter()
            .map(|(label, count)| format!("{label}: {count}"))
            .collect(),
    })
}
