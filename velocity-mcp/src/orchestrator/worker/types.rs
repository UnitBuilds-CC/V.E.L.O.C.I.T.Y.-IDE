#![allow(dead_code)]

use crossbeam_channel::Sender as CrossbeamSender;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::agent::{AiProvider, HeadlessSubAgentEventKind, HeadlessSubAgentProgress};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::task_router::RoutedModelRoute;

use super::super::blueprint::Task;
use super::super::TaskId;

/// Structured result produced by a worker after attempting a task.
#[derive(Debug, Clone)]
pub struct WorkerAttempt {
    pub provider_label: String,
    pub model_label: String,
    pub model_id: String,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub success: bool,
    pub task_id: TaskId,
    pub outputs: Vec<String>,
    pub duration: Duration,
    pub message: String,
    pub provider_label: String,
    pub model_label: String,
    pub transcript: String,
    pub status_updates: Vec<String>,
    pub attempts: Vec<WorkerAttempt>,
    pub created_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub out_of_scope_created_files: Vec<String>,
    pub run_summary_path: Option<PathBuf>,
    pub run_facts_path: Option<PathBuf>,
    pub wa_run_path: Option<String>,
    pub wa_run_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerThreadEvent {
    pub kind: WorkerThreadEventKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerThreadEventKind {
    Status,
    Transcript,
    FileChange,
    OperatorNote,
    ToolApproval,
    ToolStarted,
    ToolFinished,
}

#[derive(Debug, Clone, Default)]
pub struct WorkerThreadSnapshot {
    pub events: Vec<WorkerThreadEvent>,
    pub status_updates: Vec<String>,
    pub transcript: String,
    pub changed_files: Vec<String>,
    pub operator_notes: Vec<String>,
}

impl WorkerResult {
    pub fn new(task: &Task) -> Self {
        Self {
            success: true,
            task_id: task.id,
            outputs: Vec::new(),
            duration: Duration::ZERO,
            message: "ok".to_string(),
            provider_label: String::new(),
            model_label: String::new(),
            transcript: String::new(),
            status_updates: Vec::new(),
            attempts: Vec::new(),
            created_files: Vec::new(),
            deleted_files: Vec::new(),
            out_of_scope_created_files: Vec::new(),
            run_summary_path: None,
            run_facts_path: None,
            wa_run_path: None,
            wa_run_id: None,
        }
    }
}

/// Abstract handle for launching and polling a worker task.
pub trait WorkerHandle {
    fn poll(&mut self) -> Option<WorkerResult>;
    fn cancel(&mut self) -> bool;
    fn send_note(&mut self, note: String) -> bool;
    fn snapshot(&self) -> WorkerThreadSnapshot;
}

pub struct LiveWorkerHandle {
    pub rx: mpsc::Receiver<WorkerResult>,
    pub control_tx: CrossbeamSender<crate::agent::UiToAgentMessage>,
    pub cancel_sent: bool,
    pub progress: Arc<std::sync::Mutex<HeadlessSubAgentProgress>>,
}

impl WorkerHandle for LiveWorkerHandle {
    fn poll(&mut self) -> Option<WorkerResult> {
        self.rx.try_recv().ok()
    }

    fn cancel(&mut self) -> bool {
        if self.cancel_sent {
            return false;
        }
        if self
            .control_tx
            .send(crate::agent::UiToAgentMessage::CancelTask)
            .is_ok()
        {
            self.cancel_sent = true;
            true
        } else {
            false
        }
    }

    fn send_note(&mut self, note: String) -> bool {
        self.control_tx
            .send(crate::agent::UiToAgentMessage::UserPrompt(note))
            .is_ok()
    }

    fn snapshot(&self) -> WorkerThreadSnapshot {
        let progress = self.progress.lock().unwrap();
        WorkerThreadSnapshot {
            events: progress
                .events
                .iter()
                .map(|event| WorkerThreadEvent {
                    kind: match event.kind {
                        HeadlessSubAgentEventKind::Status => WorkerThreadEventKind::Status,
                        HeadlessSubAgentEventKind::Transcript => WorkerThreadEventKind::Transcript,
                        HeadlessSubAgentEventKind::FileChange => WorkerThreadEventKind::FileChange,
                        HeadlessSubAgentEventKind::OperatorNote => {
                            WorkerThreadEventKind::OperatorNote
                        }
                        HeadlessSubAgentEventKind::ToolApproval => {
                            WorkerThreadEventKind::ToolApproval
                        }
                        HeadlessSubAgentEventKind::ToolStarted => {
                            WorkerThreadEventKind::ToolStarted
                        }
                        HeadlessSubAgentEventKind::ToolFinished => {
                            WorkerThreadEventKind::ToolFinished
                        }
                    },
                    message: event.message.clone(),
                })
                .collect(),
            status_updates: progress.status_updates.clone(),
            transcript: progress.transcript.clone(),
            changed_files: progress.changed_files.clone(),
            operator_notes: progress.operator_notes.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkerAssignment {
    pub task: Task,
    pub task_kind: AgentTaskKind,
    pub workspace_root: PathBuf,
    pub instructions: String,
    pub planned_site_map_root: u64,
    pub provider: AiProvider,
    pub provider_label: String,
    pub model_id: String,
    pub model_label: String,
    pub thinking: bool,
    pub fallback_chain: Vec<RoutedModelRoute>,
}

#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    pub success: bool,
    pub task_kind: AgentTaskKind,
    pub provider_label: String,
    pub model_label: String,
    pub changed_files: Vec<String>,
    pub created_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub out_of_scope_created_files: Vec<String>,
    pub transcript: String,
    pub status_updates: Vec<String>,
    pub attempts: Vec<WorkerAttempt>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ScopedPaths {
    pub explicit_files: Vec<PathBuf>,
    pub scope_roots: Vec<PathBuf>,
}
