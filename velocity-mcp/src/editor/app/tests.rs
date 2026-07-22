use super::types::*;
use super::helpers::*;
use super::wa::*;
use crate::editor::orchestrator_panel::{
    OrchestratorDashboardSnapshot, OrchestratorTaskSnapshot,
};
use crate::orchestrator::worker::{
    WorkerThreadEvent, WorkerThreadEventKind, WorkerThreadSnapshot,
};
use crate::automation::AgentTaskKind;
use crate::editor::mission_control::MissionControlState;
use crate::editor::orchestrator_panel::OrchestratorPanel;
use crate::editor::task_timeline::TaskTimelineState as TTState;
use crate::editor::chat_panel::ChatPanelState;
use crate::editor::agent_ui_state::AgentUiState;
use crate::editor::smart_sidebar::SmartSidebarState;
use std::path::PathBuf;
use super::render::desktop_automation_runtime_validation_brief;

fn task_snapshot(task_id: u64, events: Vec<WorkerThreadEvent>) -> OrchestratorTaskSnapshot {
    OrchestratorTaskSnapshot {
        id: task_id,
        title: "Worker".to_string(),
        description: "Worker task".to_string(),
        status_label: "Running".to_string(),
        provider_label: String::new(),
        model_label: String::new(),
        scope: Vec::new(),
        rationale: String::new(),
        outputs: Vec::new(),
        message: String::new(),
        run_summary_path: None,
        run_facts_path: None,
        wa_run_path: None,
        wa_run_id: None,
        live_thread: Some(WorkerThreadSnapshot {
            events,
            status_updates: Vec::new(),
            transcript: String::new(),
            changed_files: Vec::new(),
            operator_notes: Vec::new(),
        }),
    }
}

fn dashboard_snapshot(
    task_id: u64,
    events: Vec<WorkerThreadEvent>,
) -> OrchestratorDashboardSnapshot {
    OrchestratorDashboardSnapshot {
        tasks: vec![task_snapshot(task_id, events)],
        ..OrchestratorDashboardSnapshot::default()
    }
}

#[test]
fn infers_desktop_automation_task_kind_from_windows_automation_goals() {
    assert_eq!(
        infer_task_kind_from_goal("Add windows automation desktop test coverage for the IDE"),
        AgentTaskKind::DesktopAutomation
    );
    assert_eq!(
        infer_task_kind_from_goal("WA runtime validation for desktop automation"),
        AgentTaskKind::DesktopAutomation
    );
}

#[test]
fn desktop_automation_preset_seeds_brief_and_policy_kind() {
    let mut mission_control = MissionControlState::new();
    let mut orchestrator = OrchestratorPanel::new();

    mission_control.brief = desktop_automation_runtime_validation_brief().to_string();
    mission_control.set_selected_task(None);
    orchestrator.set_selected_policy_kind(AgentTaskKind::DesktopAutomation);

    assert!(mission_control.brief.contains("desktop automation runtime"));
    assert_eq!(
        orchestrator.selected_policy_kind(),
        AgentTaskKind::DesktopAutomation
    );
}

#[test]
fn desktop_automation_task_detection_prefers_lane_and_wa_language() {
    let mut task = task_snapshot(9, Vec::new());
    task.title = "Validate WA runtime".to_string();
    task.description = "Desktop automation evidence pass".to_string();

    assert!(task_matches_desktop_automation_lane(
        &task,
        Some(AgentTaskKind::DesktopAutomation.as_str())
    ));
    assert!(task_matches_desktop_automation_lane(&task, None));
    assert!(!task_matches_desktop_automation_lane(&task_snapshot(11, Vec::new()), None));
}

#[test]
fn desktop_automation_evidence_state_prefers_live_then_artifacts() {
    let live_task = task_snapshot(13, vec![WorkerThreadEvent {
        kind: WorkerThreadEventKind::Status,
        message: "Capturing WA snapshot".to_string(),
    }]);
    assert_eq!(
        desktop_automation_evidence_state(&live_task),
        DesktopAutomationEvidenceState::LiveEvidence
    );

    let mut artifact_task = task_snapshot(14, Vec::new());
    artifact_task.status_label = "Done".to_string();
    artifact_task.live_thread = None;
    artifact_task.wa_run_path = Some(".velocity\\wa-runs\\desktop-run.wa-run.nda".to_string());
    artifact_task.wa_run_id = Some("desktop-run".to_string());
    artifact_task.run_summary_path = Some("runs\\desktop\\summary.txt".to_string());
    assert_eq!(
        desktop_automation_evidence_state(&artifact_task),
        DesktopAutomationEvidenceState::ArtifactBacked
    );

    let mut waiting_task = task_snapshot(15, Vec::new());
    waiting_task.status_label = "Done".to_string();
    waiting_task.live_thread = None;
    assert_eq!(
        desktop_automation_evidence_state(&waiting_task),
        DesktopAutomationEvidenceState::AwaitingEvidence
    );
}

#[test]
fn desktop_automation_evidence_lines_include_live_and_artifact_details() {
    let mut task = task_snapshot(
        16,
        vec![
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::Status,
                message: "Capturing WA snapshot".to_string(),
            },
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::ToolFinished,
                message: "WA action verified".to_string(),
            },
        ],
    );
    task.outputs = vec!["snapshot:capture".to_string(), "script:verified".to_string()];
    task.wa_run_path = Some(".velocity\\wa-runs\\desktop-run.wa-run.nda".to_string());
    task.wa_run_id = Some("desktop-run".to_string());
    task.run_summary_path = Some("runs\\desktop\\summary.txt".to_string());
    task.run_facts_path = Some("runs\\desktop\\facts.nda".to_string());
    if let Some(thread) = task.live_thread.as_mut() {
        thread.changed_files.push(".velocity\\wa-snapshots\\capture.wa.nda".to_string());
        thread.transcript = "WA transcript evidence".to_string();
        thread.operator_notes.push("Retry with focused selector".to_string());
    }

    let lines = desktop_automation_evidence_lines(&task);
    assert!(lines.iter().any(|line| line.contains("Evidence state: Live WA evidence")));
    assert!(lines.iter().any(|line| line.contains("WA run artifact:")));
    assert!(lines.iter().any(|line| line.contains("WA run id: desktop-run")));
    assert!(lines.iter().any(|line| line.contains("Run summary artifact:")));
    assert!(lines.iter().any(|line| line.contains("NDA facts artifact:")));
    assert!(lines.iter().any(|line| line.contains("Reported outputs:")));
    assert!(lines.iter().any(|line| line.contains("Live worker evidence updates: 2")));
    assert!(lines.iter().any(|line| line.contains("Observed file activity:")));
    assert!(lines.iter().any(|line| line.contains("Live transcript captured")));
    assert!(lines.iter().any(|line| line.contains("Operator notes recorded: 1")));
}

#[test]
fn desktop_automation_mission_summary_counts_evidence_states() {
    let live_task = task_snapshot(21, vec![WorkerThreadEvent {
        kind: WorkerThreadEventKind::Status,
        message: "WA capture running".to_string(),
    }]);

    let mut artifact_task = task_snapshot(22, Vec::new());
    artifact_task.title = "Desktop automation artifact review".to_string();
    artifact_task.status_label = "Done".to_string();
    artifact_task.live_thread = None;
    artifact_task.wa_run_path = Some(".velocity\\wa-runs\\desktop-run.wa-run.nda".to_string());
    artifact_task.wa_run_id = Some("desktop-run".to_string());
    artifact_task.run_facts_path = Some("runs\\desktop\\facts.nda".to_string());

    let mut waiting_task = task_snapshot(23, Vec::new());
    waiting_task.title = "Desktop automation follow-up".to_string();
    waiting_task.status_label = "Done".to_string();
    waiting_task.live_thread = None;

    let summary = desktop_automation_mission_summary(
        &[live_task, artifact_task, waiting_task],
        Some(AgentTaskKind::DesktopAutomation.as_str()),
    )
    .expect("desktop automation summary");

    assert_eq!(summary.task_count, 3);
    assert_eq!(summary.live_count, 1);
    assert_eq!(summary.artifact_count, 1);
    assert_eq!(summary.awaiting_count, 1);
    assert!(summary.state_labels.iter().any(|label| label.contains("Live WA evidence: 1")));
    assert!(summary.state_labels.iter().any(|label| label.contains("WA artifacts captured: 1")));
    assert!(summary.state_labels.iter().any(|label| label.contains("Awaiting WA evidence: 1")));
}

#[test]
fn desktop_automation_selected_task_status_summarizes_artifacts_and_live_signals() {
    let mut task = task_snapshot(
        24,
        vec![
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::Status,
                message: "Capturing WA snapshot".to_string(),
            },
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::ToolFinished,
                message: "WA verification complete".to_string(),
            },
        ],
    );
    task.outputs = vec!["snapshot:capture".to_string(), "script:verified".to_string()];
    task.wa_run_path = Some(".velocity\\wa-runs\\desktop-run.wa-run.nda".to_string());
    task.wa_run_id = Some("desktop-run".to_string());
    task.run_summary_path = Some("runs\\desktop\\summary.txt".to_string());
    task.run_facts_path = Some("runs\\desktop\\facts.nda".to_string());
    if let Some(thread) = task.live_thread.as_mut() {
        thread.transcript = "WA transcript evidence".to_string();
        thread.operator_notes.push("Retry selector".to_string());
    }

    let status = desktop_automation_selected_task_status(&task);
    assert_eq!(status.state_label, "Live WA evidence");
    assert_eq!(status.artifact_count, 3);
    assert_eq!(status.output_count, 2);
    assert_eq!(status.evidence_update_count, 2);
    assert!(status.has_transcript);
    assert!(status.has_operator_notes);
}

#[test]
fn desktop_automation_selected_task_cues_surface_artifacts_and_next_step() {
    let mut task = task_snapshot(25, Vec::new());
    task.status_label = "Done".to_string();
    task.live_thread = None;
    task.outputs = vec!["snapshot:capture".to_string()];
    task.wa_run_path = Some(".velocity\\wa-runs\\desktop-run.wa-run.nda".to_string());
    task.wa_run_id = Some("desktop-run".to_string());
    task.run_summary_path = Some("runs\\desktop\\summary.txt".to_string());
    task.run_facts_path = Some("runs\\desktop\\facts.nda".to_string());

    let cues = desktop_automation_selected_task_cues(&task);
    assert!(cues.artifact_lines.iter().any(|line| line.contains("WA run ready:")));
    assert!(cues.artifact_lines.iter().any(|line| line.contains("WA run id: desktop-run")));
    assert!(cues.artifact_lines.iter().any(|line| line.contains("Run summary ready:")));
    assert!(cues.artifact_lines.iter().any(|line| line.contains("NDA facts ready:")));
    assert!(cues.artifact_lines.iter().any(|line| line.contains("Reported outputs ready:")));
    assert!(cues.next_action.contains("Review the captured WA artifacts"));
}

#[test]
fn mirror_worker_events_into_timeline_appends_only_new_events() {
    let mut timeline = TTState::default();
    let mut mission_control = MissionControlState::new();
    let first_snapshot = dashboard_snapshot(
        7,
        vec![
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::Status,
                message: "Planning step ready".to_string(),
            },
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::OperatorNote,
                message: "Keep auth flow".to_string(),
            },
        ],
    );

    let mut app = super::VelocityApp {
        agent_tx: crossbeam_channel::unbounded().0,
        agent_rx: crossbeam_channel::unbounded().1,
        workspace_root: PathBuf::from("."),
        tabs: Vec::new(),
        active_tab: None,
        buffers: std::collections::HashMap::new(),
        dock_state: None,
        chat: ChatPanelState::default(),
        command_output: String::new(),
        account_usage: Vec::new(),
        usage_date: String::new(),
        command_palette: CommandPalette { open: false, query: String::new(), selected: 0 },
        status_message: String::new(),
        appearance: crate::editor::theme::AppearanceSettings::default(),
        provider_settings: crate::usage::WorkspaceProviderSettings::default(),
        left_sidebar_visible: true,
        left_sidebar_width: 240.0,
        right_sidebar_visible: true,
        right_sidebar_width: 280.0,
        tab_counter: 0,
        agent_ui_state: AgentUiState::default(),
        task_timeline: timeline,
        smart_sidebar: SmartSidebarState::default(),
        projects: Vec::new(),
        show_add_project_ui: false,
        new_project_path_input: String::new(),
        agent_active: false,
        pending_approvals: Vec::new(),
        auto_approve: false,
        available_models: Vec::new(),
        selected_model: String::new(),
        thinking_enabled: false,
        thinking_supported: false,
        tools_supported: false,
        models_loading: false,
        provider: crate::agent::AiProvider::CloudflareWorkersAi,
        pending_open_path: None,
        pending_save_as_path: None,
        show_full_diff: false,
        build_errors_count: 0,
        gpu_name: String::new(),
        search_query: String::new(),
        search_hits: Vec::new(),
        pending_cursor_line: None,
        file_tree: None,
        last_tree_update: std::time::Instant::now(),
        toasts: crate::editor::toast::ToastQueue::default(),
        orchestrator: OrchestratorPanel::new(),
        mission_control,
        next_intervention_id: 1,
        mediator: std::sync::Arc::new(crate::automation::mediator::MediatorArena::new()),
        graph_view: crate::editor::graph_view::MerkleGraphView::new(),
        terminal_rx: None,
        terminal_input: String::new(),
        current_agent_task_id: 0,
        chat_input: String::new(),
        chat_history: String::new(),
    };

    app.mirror_worker_events_into_timeline(&first_snapshot);
    assert_eq!(app.task_timeline.event_count(), 2);
    assert_eq!(app.mission_control.mirrored_worker_event_count(7), 2);

    app.mirror_worker_events_into_timeline(&first_snapshot);
    assert_eq!(app.task_timeline.event_count(), 2);

    let second_snapshot = dashboard_snapshot(
        7,
        vec![
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::Status,
                message: "Planning step ready".to_string(),
            },
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::OperatorNote,
                message: "Keep auth flow".to_string(),
            },
            WorkerThreadEvent {
                kind: WorkerThreadEventKind::Transcript,
                message: "Generated component outline".to_string(),
            },
        ],
    );

    app.mirror_worker_events_into_timeline(&second_snapshot);
    assert_eq!(app.task_timeline.event_count(), 3);
    assert_eq!(app.mission_control.mirrored_worker_event_count(7), 3);

    let newest = app.task_timeline.visible_events().next().unwrap().1;
    assert_eq!(
        app.task_timeline.get_text(newest.name_offset, newest.name_len),
        "Worker output"
    );
    assert_eq!(
        app.task_timeline.get_text(newest.description_offset, newest.description_len),
        "Generated component outline"
    );
}

#[test]
fn clearing_worker_event_tracking_allows_replay_after_replan() {
    let snapshot = dashboard_snapshot(
        3,
        vec![WorkerThreadEvent {
            kind: WorkerThreadEventKind::Status,
            message: "Queued follow-up".to_string(),
        }],
    );

    let mut app = super::VelocityApp {
        agent_tx: crossbeam_channel::unbounded().0,
        agent_rx: crossbeam_channel::unbounded().1,
        workspace_root: PathBuf::from("."),
        tabs: Vec::new(),
        active_tab: None,
        buffers: std::collections::HashMap::new(),
        dock_state: None,
        chat: ChatPanelState::default(),
        command_output: String::new(),
        account_usage: Vec::new(),
        usage_date: String::new(),
        command_palette: CommandPalette { open: false, query: String::new(), selected: 0 },
        status_message: String::new(),
        appearance: crate::editor::theme::AppearanceSettings::default(),
        provider_settings: crate::usage::WorkspaceProviderSettings::default(),
        left_sidebar_visible: true,
        left_sidebar_width: 240.0,
        right_sidebar_visible: true,
        right_sidebar_width: 280.0,
        tab_counter: 0,
        agent_ui_state: AgentUiState::default(),
        task_timeline: TTState::default(),
        smart_sidebar: SmartSidebarState::default(),
        projects: Vec::new(),
        show_add_project_ui: false,
        new_project_path_input: String::new(),
        agent_active: false,
        pending_approvals: Vec::new(),
        auto_approve: false,
        available_models: Vec::new(),
        selected_model: String::new(),
        thinking_enabled: false,
        thinking_supported: false,
        tools_supported: false,
        models_loading: false,
        provider: crate::agent::AiProvider::CloudflareWorkersAi,
        pending_open_path: None,
        pending_save_as_path: None,
        show_full_diff: false,
        build_errors_count: 0,
        gpu_name: String::new(),
        search_query: String::new(),
        search_hits: Vec::new(),
        pending_cursor_line: None,
        file_tree: None,
        last_tree_update: std::time::Instant::now(),
        toasts: crate::editor::toast::ToastQueue::default(),
        orchestrator: OrchestratorPanel::new(),
        mission_control: MissionControlState::new(),
        next_intervention_id: 1,
        mediator: std::sync::Arc::new(crate::automation::mediator::MediatorArena::new()),
        graph_view: crate::editor::graph_view::MerkleGraphView::new(),
        terminal_rx: None,
        terminal_input: String::new(),
        current_agent_task_id: 0,
        chat_input: String::new(),
        chat_history: String::new(),
    };

    app.mirror_worker_events_into_timeline(&snapshot);
    assert_eq!(app.task_timeline.event_count(), 1);

    app.mission_control.clear_worker_event_tracking();
    app.mirror_worker_events_into_timeline(&snapshot);
    assert_eq!(app.task_timeline.event_count(), 2);
    assert_eq!(app.mission_control.mirrored_worker_event_count(3), 1);
}
