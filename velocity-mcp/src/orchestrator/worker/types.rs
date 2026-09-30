#![allow(dead_code)]

use crossbeam_channel::Sender as CrossbeamSender;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::agent::{AiProvider, HeadlessSubAgentEventKind, HeadlessSubAgentProgress};
use crate::automation::instruction_registry::AgentTaskKind;
use crate::automation::task_router::RoutedModelRoute;
use crate::safety::SafeMutex;

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
    #[allow(dead_code)]
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
    /// True for task kinds (Analysis, Planning) whose expected output is
    /// textual rather than file modifications. The validator skips the
    /// "no scoped file changes" check when this is set.
    pub is_read_only: bool,
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
    #[allow(dead_code)]
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
            is_read_only: false,
        }
    }

    /// A failure result for a worker whose thread vanished before reporting.
    /// Carries no task borrow, so handles holding only a `TaskId` can synthesize
    /// it.
    pub fn detached(task_id: TaskId) -> Self {
        let message = "worker thread exited without reporting a result".to_string();
        Self {
            success: false,
            task_id,
            status_updates: vec![message.clone()],
            message,
            ..Self::empty(task_id)
        }
    }

    fn empty(task_id: TaskId) -> Self {
        Self {
            success: true,
            task_id,
            outputs: Vec::new(),
            duration: Duration::ZERO,
            message: String::new(),
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
            is_read_only: false,
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
    /// Kept so a detached worker channel can still be reported against the
    /// task it was launched for (the task itself lives in the panel graph).
    pub task_id: TaskId,
}

impl WorkerHandle for LiveWorkerHandle {
    fn poll(&mut self) -> Option<WorkerResult> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            // The worker thread died without sending a result (a panic in
            // `run_assignment` drops the sender). Without this arm the poll
            // returns `None` forever and the task sits in Running unstopped.
            Err(mpsc::TryRecvError::Disconnected) => Some(WorkerResult::detached(self.task_id)),
        }
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
        let progress = self.progress.lock_safe();
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
    /// Optional list of files to pre-index for speculative pre-computation.
    pub scoped_files: Option<Vec<PathBuf>>,
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
    pub is_read_only: bool,
}

#[derive(Debug, Clone)]
pub struct ScopedPaths {
    pub explicit_files: Vec<PathBuf>,
    pub scope_roots: Vec<PathBuf>,
}
