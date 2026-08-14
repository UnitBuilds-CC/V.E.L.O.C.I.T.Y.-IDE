//! Live Multi-Agent Orchestration UI: real-time activity feed, worker progress
//! tracking, and coordination dashboard for Mission Control.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum events retained in the activity feed (ring buffer).
const MAX_ACTIVITY_EVENTS: usize = 200;

/// A single event in the live orchestration activity feed.
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub timestamp: Instant,
    pub kind: ActivityEventKind,
    pub task_id: Option<u64>,
    pub message: String,
}

/// Classification of activity events for filtering and color-coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityEventKind {
    WorkerSpawned,
    WorkerProgress,
    WorkerCompleted,
    WorkerFailed,
    WorkerBlocked,
    InterventionQueued,
    PlanRouted,
    CheckpointCreated,
    MemorySaved,
    SystemInfo,
}

impl ActivityEventKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::WorkerSpawned => "Spawned",
            Self::WorkerProgress => "Progress",
            Self::WorkerCompleted => "Done",
            Self::WorkerFailed => "Failed",
            Self::WorkerBlocked => "Blocked",
            Self::InterventionQueued => "Intervention",
            Self::PlanRouted => "Routed",
            Self::CheckpointCreated => "Checkpoint",
            Self::MemorySaved => "Memory",
            Self::SystemInfo => "Info",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::WorkerSpawned => "\u{25b7}",
            Self::WorkerProgress => "\u{22ef}",
            Self::WorkerCompleted => "\u{2714}",
            Self::WorkerFailed => "\u{2716}",
            Self::WorkerBlocked => "\u{25c6}",
            Self::InterventionQueued => "\u{26a0}",
            Self::PlanRouted => "\u{25c7}",
            Self::CheckpointCreated => "\u{22a1}",
            Self::MemorySaved => "\u{25c9}",
            Self::SystemInfo => "\u{2139}",
        }
    }
}

/// Per-worker progress state for the live dashboard.
#[derive(Debug, Clone)]
pub struct WorkerProgress {
    pub task_id: u64,
    pub title: String,
    pub model_label: String,
    pub started_at: Instant,
    pub last_update: Instant,
    pub events_count: usize,
    pub status_text: String,
    pub files_changed: usize,
    pub transcript_len: usize,
}

impl WorkerProgress {
    pub fn new(task_id: u64, title: String, model_label: String) -> Self {
        let now = Instant::now();
        Self {
            task_id,
            title,
            model_label,
            started_at: now,
            last_update: now,
            events_count: 0,
            status_text: "Starting...".to_string(),
            files_changed: 0,
            transcript_len: 0,
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn elapsed_label(&self) -> String {
        let secs = self.elapsed().as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    }

    pub fn progress_fraction(&self) -> f32 {
        // Heuristic: estimate progress from event count (asymptotic approach to 1.0)
        let x = self.events_count as f32;
        1.0 - (-x / 20.0).exp()
    }
}

/// The live orchestration state powering Mission Control's real-time dashboard.
#[derive(Debug)]
pub struct LiveOrchestrationState {
    pub activity_feed: VecDeque<ActivityEvent>,
    pub worker_progress: Vec<WorkerProgress>,
    pub total_tasks_completed: usize,
    pub total_tasks_failed: usize,
    pub total_tokens_used: u64,
    pub session_start: Instant,
    pub filter_kind: Option<ActivityEventKind>,
}

impl Default for LiveOrchestrationState {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveOrchestrationState {
    pub fn new() -> Self {
        Self {
            activity_feed: VecDeque::with_capacity(MAX_ACTIVITY_EVENTS),
            worker_progress: Vec::new(),
            total_tasks_completed: 0,
            total_tasks_failed: 0,
            total_tokens_used: 0,
            session_start: Instant::now(),
            filter_kind: None,
        }
    }

    /// Push an event into the activity feed (ring buffer).
    pub fn push_event(&mut self, kind: ActivityEventKind, task_id: Option<u64>, message: String) {
        if self.activity_feed.len() >= MAX_ACTIVITY_EVENTS {
            self.activity_feed.pop_front();
        }
        self.activity_feed.push_back(ActivityEvent {
            timestamp: Instant::now(),
            kind,
            task_id,
            message,
        });
    }

    /// Register a new worker as active.
    pub fn register_worker(&mut self, task_id: u64, title: String, model_label: String) {
        self.worker_progress
            .push(WorkerProgress::new(task_id, title.clone(), model_label));
        self.push_event(
            ActivityEventKind::WorkerSpawned,
            Some(task_id),
            format!("Worker started: {}", title),
        );
    }

    /// Update a worker's progress from its live thread snapshot.
    pub fn update_worker(
        &mut self,
        task_id: u64,
        events_count: usize,
        status_text: String,
        files_changed: usize,
        transcript_len: usize,
    ) {
        if let Some(wp) = self
            .worker_progress
            .iter_mut()
            .find(|w| w.task_id == task_id)
        {
            let had_new_events = events_count > wp.events_count;
            wp.events_count = events_count;
            wp.status_text = status_text.clone();
            wp.files_changed = files_changed;
            wp.transcript_len = transcript_len;
            wp.last_update = Instant::now();
            if had_new_events {
                self.push_event(
                    ActivityEventKind::WorkerProgress,
                    Some(task_id),
                    status_text,
                );
            }
        }
    }

    /// Mark a worker as completed and remove from active tracking.
    pub fn complete_worker(&mut self, task_id: u64, success: bool) {
        self.worker_progress.retain(|w| w.task_id != task_id);
        if success {
            self.total_tasks_completed += 1;
            self.push_event(
                ActivityEventKind::WorkerCompleted,
                Some(task_id),
                format!("Task #{} completed successfully", task_id),
            );
        } else {
            self.total_tasks_failed += 1;
            self.push_event(
                ActivityEventKind::WorkerFailed,
                Some(task_id),
                format!("Task #{} failed", task_id),
            );
        }
    }

    /// Get filtered activity feed entries.
    pub fn filtered_feed(&self) -> Vec<&ActivityEvent> {
        match self.filter_kind {
            None => self.activity_feed.iter().collect(),
            Some(kind) => self
                .activity_feed
                .iter()
                .filter(|e| e.kind == kind)
                .collect(),
        }
    }

    /// Session uptime label.
    pub fn session_uptime(&self) -> String {
        let secs = self.session_start.elapsed().as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m", secs / 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }

    /// Overall throughput: tasks completed per minute.
    pub fn throughput(&self) -> f64 {
        let mins = self.session_start.elapsed().as_secs_f64() / 60.0;
        if mins < 0.01 {
            0.0
        } else {
            self.total_tasks_completed as f64 / mins
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_feed_ring_buffer() {
        let mut state = LiveOrchestrationState::new();
        for i in 0..250 {
            state.push_event(ActivityEventKind::SystemInfo, None, format!("event {}", i));
        }
        assert_eq!(state.activity_feed.len(), MAX_ACTIVITY_EVENTS);
        assert!(state.activity_feed.front().unwrap().message.contains("50"));
    }

    #[test]
    fn worker_lifecycle() {
        let mut state = LiveOrchestrationState::new();
        state.register_worker(1, "Fix bugs".into(), "gpt-4".into());
        assert_eq!(state.worker_progress.len(), 1);

        state.update_worker(1, 5, "Reading files".into(), 2, 100);
        assert_eq!(state.worker_progress[0].events_count, 5);

        state.complete_worker(1, true);
        assert_eq!(state.worker_progress.len(), 0);
        assert_eq!(state.total_tasks_completed, 1);
    }

    #[test]
    fn progress_fraction_increases() {
        let mut wp = WorkerProgress::new(1, "test".into(), "model".into());
        let f0 = wp.progress_fraction();
        wp.events_count = 10;
        let f1 = wp.progress_fraction();
        wp.events_count = 40;
        let f2 = wp.progress_fraction();
        assert!(f1 > f0);
        assert!(f2 > f1);
        assert!(f2 < 1.0);
    }

    #[test]
    fn filter_feed_by_kind() {
        let mut state = LiveOrchestrationState::new();
        state.push_event(ActivityEventKind::SystemInfo, None, "info".into());
        state.push_event(ActivityEventKind::WorkerCompleted, Some(1), "done".into());
        state.push_event(ActivityEventKind::SystemInfo, None, "info2".into());

        state.filter_kind = Some(ActivityEventKind::WorkerCompleted);
        assert_eq!(state.filtered_feed().len(), 1);

        state.filter_kind = None;
        assert_eq!(state.filtered_feed().len(), 3);
    }
}
