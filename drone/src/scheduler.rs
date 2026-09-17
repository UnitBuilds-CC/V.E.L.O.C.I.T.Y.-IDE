//! Drone task scheduler — priority-based scheduling with concurrency control.
//!
//! Provides autonomous task scheduling for drone operations such as scanning,
//! monitoring, reporting, syncing, and health checks. Tasks are dispatched by
//! priority (higher value = higher priority) up to a configurable concurrency
//! limit.

use std::time::Instant;

// ── Task Kind ──

/// The kind of operation a scheduled drone task performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DroneTaskKind {
    /// Scan the workspace or remote target for changes.
    Scan,
    /// Monitor a resource for events.
    Monitor,
    /// Generate and submit a report.
    Report,
    /// Synchronise state with a peer or central server.
    Sync,
    /// Run a health-check probe.
    HealthCheck,
}

impl DroneTaskKind {
    /// Human-readable label.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Monitor => "monitor",
            Self::Report => "report",
            Self::Sync => "sync",
            Self::HealthCheck => "health_check",
        }
    }
}

// ── Task Status ──

/// Lifecycle status of a scheduled drone task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DroneTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl DroneTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the task is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

// ── DroneTask ──

/// A single unit of work managed by the [`DroneScheduler`].
#[derive(Debug, Clone)]
pub struct DroneTask {
    pub id: u64,
    pub kind: DroneTaskKind,
    pub priority: u32,
    pub payload: String,
    pub status: DroneTaskStatus,
    pub created_at: Instant,
    pub completed_at: Option<Instant>,
}

impl DroneTask {
    fn new(id: u64, kind: DroneTaskKind, payload: String, priority: u32) -> Self {
        Self {
            id,
            kind,
            priority,
            payload,
            status: DroneTaskStatus::Pending,
            created_at: Instant::now(),
            completed_at: None,
        }
    }
}

// ── Scheduler Stats ──

/// Aggregate statistics for the scheduler.
#[derive(Debug, Clone)]
pub struct DroneSchedulerStats {
    pub total_submitted: u64,
    pub completed: u64,
    pub failed: u64,
    pub pending: usize,
    pub running: usize,
    pub success_rate: f64,
}

// ── DroneScheduler ──

/// Priority-based task scheduler with bounded concurrency.
///
/// Tasks are submitted with a priority value; [`tick`](DroneScheduler::tick)
/// promotes the highest-priority pending tasks to *Running* up to
/// `max_concurrent`. Completed / failed tasks are tallied for stats.
pub struct DroneScheduler {
    tasks: Vec<DroneTask>,
    max_concurrent: usize,
    running_count: usize,
    completed_count: u64,
    failed_count: u64,
    next_id: u64,
}

impl DroneScheduler {
    /// Create a new scheduler that allows at most `max_concurrent` tasks to run
    /// simultaneously.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            tasks: Vec::new(),
            max_concurrent: max_concurrent.max(1),
            running_count: 0,
            completed_count: 0,
            failed_count: 0,
            next_id: 1,
        }
    }

    /// Submit a new task and return its unique ID.
    pub fn submit_task(&mut self, kind: DroneTaskKind, payload: String, priority: u32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(DroneTask::new(id, kind, payload, priority));
        id
    }

    /// Promote pending tasks to *Running* up to the concurrency limit.
    ///
    /// Returns the IDs of the tasks that were started, sorted by descending
    /// priority (highest priority first). Ties are broken by submission order
    /// (earlier first).
    pub fn tick(&mut self) -> Vec<u64> {
        let slots = self.max_concurrent.saturating_sub(self.running_count);
        if slots == 0 {
            return Vec::new();
        }

        // Collect indices of pending tasks.
        let mut pending_indices: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.status == DroneTaskStatus::Pending)
            .map(|(i, _)| i)
            .collect();

        // Sort by descending priority; ties keep original order (stable sort).
        pending_indices.sort_by(|&a, &b| self.tasks[b].priority.cmp(&self.tasks[a].priority));

        let to_start = pending_indices.into_iter().take(slots);
        let mut started_ids = Vec::new();

        for idx in to_start {
            self.tasks[idx].status = DroneTaskStatus::Running;
            self.running_count += 1;
            started_ids.push(self.tasks[idx].id);
        }

        // Return sorted by descending priority (already in that order).
        started_ids
    }

    /// Mark a running task as completed or failed.
    ///
    /// If `success` is `true` the task moves to *Completed*; otherwise *Failed*.
    /// If the task is not currently in the *Running* state this is a no-op.
    pub fn complete_task(&mut self, id: u64, success: bool) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            if task.status != DroneTaskStatus::Running {
                return;
            }
            if success {
                task.status = DroneTaskStatus::Completed;
                self.completed_count += 1;
            } else {
                task.status = DroneTaskStatus::Failed;
                self.failed_count += 1;
            }
            task.completed_at = Some(Instant::now());
            self.running_count = self.running_count.saturating_sub(1);
        }
    }

    /// Cancel a pending or running task. Returns `true` if the task was found
    /// and cancelled.
    pub fn cancel_task(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            if task.status.is_terminal() {
                return false;
            }
            let was_running = task.status == DroneTaskStatus::Running;
            task.status = DroneTaskStatus::Cancelled;
            task.completed_at = Some(Instant::now());
            if was_running {
                self.running_count = self.running_count.saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    /// Number of tasks currently in the *Pending* state.
    pub fn pending_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == DroneTaskStatus::Pending)
            .count()
    }

    /// Number of tasks currently in the *Running* state.
    pub fn running_count(&self) -> usize {
        self.running_count
    }

    /// Compute aggregate statistics.
    pub fn scheduler_stats(&self) -> DroneSchedulerStats {
        let pending = self.pending_count();
        let running = self.running_count;
        let total_finished = self.completed_count + self.failed_count;
        let success_rate = if total_finished == 0 {
            1.0
        } else {
            self.completed_count as f64 / total_finished as f64
        };

        DroneSchedulerStats {
            total_submitted: self.next_id - 1,
            completed: self.completed_count,
            failed: self.failed_count,
            pending,
            running,
            success_rate,
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(k: DroneTaskKind) -> DroneTaskKind {
        k
    }

    #[test]
    fn submit_returns_incrementing_ids() {
        let mut s = DroneScheduler::new(4);
        let id1 = s.submit_task(kind(DroneTaskKind::Scan), "a".into(), 1);
        let id2 = s.submit_task(kind(DroneTaskKind::Sync), "b".into(), 2);
        let id3 = s.submit_task(kind(DroneTaskKind::Report), "c".into(), 3);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[test]
    fn initial_pending_count() {
        let mut s = DroneScheduler::new(4);
        s.submit_task(kind(DroneTaskKind::Scan), "x".into(), 1);
        s.submit_task(kind(DroneTaskKind::Scan), "y".into(), 1);
        assert_eq!(s.pending_count(), 2);
        assert_eq!(s.running_count(), 0);
    }

    #[test]
    fn tick_starts_pending_tasks() {
        let mut s = DroneScheduler::new(4);
        s.submit_task(kind(DroneTaskKind::Scan), "a".into(), 5);
        s.submit_task(kind(DroneTaskKind::Sync), "b".into(), 10);
        let started = s.tick();
        assert_eq!(started.len(), 2);
        // Highest priority first.
        assert_eq!(started[0], 2); // priority 10
        assert_eq!(started[1], 1); // priority 5
        assert_eq!(s.running_count(), 2);
        assert_eq!(s.pending_count(), 0);
    }

    #[test]
    fn tick_respects_max_concurrent() {
        let mut s = DroneScheduler::new(2);
        for i in 0..5 {
            s.submit_task(kind(DroneTaskKind::Monitor), format!("m{i}"), i);
        }
        let started = s.tick();
        assert_eq!(started.len(), 2);
        assert_eq!(s.running_count(), 2);
        assert_eq!(s.pending_count(), 3);
    }

    #[test]
    fn tick_returns_empty_when_no_slots() {
        let mut s = DroneScheduler::new(1);
        s.submit_task(kind(DroneTaskKind::Scan), "a".into(), 1);
        s.tick(); // fills the single slot
        let started2 = s.tick();
        assert!(started2.is_empty());
    }

    #[test]
    fn tick_priority_ordering() {
        let mut s = DroneScheduler::new(10);
        s.submit_task(kind(DroneTaskKind::Report), "low".into(), 1);
        s.submit_task(kind(DroneTaskKind::Report), "high".into(), 100);
        s.submit_task(kind(DroneTaskKind::Report), "mid".into(), 50);
        let started = s.tick();
        assert_eq!(started, vec![2, 3, 1]); // priorities 100, 50, 1
    }

    #[test]
    fn complete_task_success() {
        let mut s = DroneScheduler::new(4);
        let id = s.submit_task(kind(DroneTaskKind::HealthCheck), "hc".into(), 1);
        s.tick();
        s.complete_task(id, true);
        assert_eq!(s.running_count(), 0);
        let stats = s.scheduler_stats();
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.failed, 0);
    }

    #[test]
    fn complete_task_failure() {
        let mut s = DroneScheduler::new(4);
        let id = s.submit_task(kind(DroneTaskKind::HealthCheck), "hc".into(), 1);
        s.tick();
        s.complete_task(id, false);
        let stats = s.scheduler_stats();
        assert_eq!(stats.completed, 0);
        assert_eq!(stats.failed, 1);
    }

    #[test]
    fn cancel_pending_task() {
        let mut s = DroneScheduler::new(4);
        let id = s.submit_task(kind(DroneTaskKind::Scan), "s".into(), 1);
        assert!(s.cancel_task(id));
        assert_eq!(s.pending_count(), 0);
        assert!(!s.tick().contains(&id)); // should not be started
    }

    #[test]
    fn cancel_running_task() {
        let mut s = DroneScheduler::new(4);
        let id = s.submit_task(kind(DroneTaskKind::Scan), "s".into(), 1);
        s.tick();
        assert_eq!(s.running_count(), 1);
        assert!(s.cancel_task(id));
        assert_eq!(s.running_count(), 0);
    }

    #[test]
    fn cancel_already_terminal_returns_false() {
        let mut s = DroneScheduler::new(4);
        let id = s.submit_task(kind(DroneTaskKind::Scan), "s".into(), 1);
        s.tick();
        s.complete_task(id, true);
        assert!(!s.cancel_task(id)); // already completed
    }

    #[test]
    fn cancel_unknown_id_returns_false() {
        let mut s = DroneScheduler::new(4);
        assert!(!s.cancel_task(999));
    }

    #[test]
    fn scheduler_stats_initial() {
        let s = DroneScheduler::new(4);
        let stats = s.scheduler_stats();
        assert_eq!(stats.total_submitted, 0);
        assert_eq!(stats.completed, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.running, 0);
        assert!((stats.success_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scheduler_stats_mixed() {
        let mut s = DroneScheduler::new(4);
        let id1 = s.submit_task(kind(DroneTaskKind::Scan), "a".into(), 1);
        let id2 = s.submit_task(kind(DroneTaskKind::Sync), "b".into(), 2);
        let _id3 = s.submit_task(kind(DroneTaskKind::Report), "c".into(), 3);
        s.tick(); // all 3 running
        s.complete_task(id1, true);
        s.complete_task(id2, false);
        let stats = s.scheduler_stats();
        assert_eq!(stats.total_submitted, 3);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.running, 1);
        assert_eq!(stats.pending, 0);
        assert!((stats.success_rate - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn tick_after_completion_fills_slots() {
        let mut s = DroneScheduler::new(1);
        let id1 = s.submit_task(kind(DroneTaskKind::Scan), "a".into(), 10);
        let id2 = s.submit_task(kind(DroneTaskKind::Scan), "b".into(), 5);
        let started1 = s.tick();
        assert_eq!(started1, vec![id1]);
        s.complete_task(id1, true);
        let started2 = s.tick();
        assert_eq!(started2, vec![id2]);
    }

    #[test]
    fn max_concurrent_clamped_to_one() {
        // Passing 0 should be clamped to 1 so at least one task can run.
        let mut s = DroneScheduler::new(0);
        s.submit_task(kind(DroneTaskKind::Scan), "x".into(), 1);
        let started = s.tick();
        assert_eq!(started.len(), 1);
    }

    #[test]
    fn task_kind_as_str() {
        assert_eq!(DroneTaskKind::Scan.as_str(), "scan");
        assert_eq!(DroneTaskKind::Monitor.as_str(), "monitor");
        assert_eq!(DroneTaskKind::Report.as_str(), "report");
        assert_eq!(DroneTaskKind::Sync.as_str(), "sync");
        assert_eq!(DroneTaskKind::HealthCheck.as_str(), "health_check");
    }

    #[test]
    fn task_status_is_terminal() {
        assert!(!DroneTaskStatus::Pending.is_terminal());
        assert!(!DroneTaskStatus::Running.is_terminal());
        assert!(DroneTaskStatus::Completed.is_terminal());
        assert!(DroneTaskStatus::Failed.is_terminal());
        assert!(DroneTaskStatus::Cancelled.is_terminal());
    }

    #[test]
    fn complete_task_on_pending_is_noop() {
        let mut s = DroneScheduler::new(4);
        let id = s.submit_task(kind(DroneTaskKind::Scan), "a".into(), 1);
        // Task is still pending, not running — should be a no-op in release.
        s.complete_task(id, true);
        // Stats should not reflect a completion because the task was not running.
        let stats = s.scheduler_stats();
        assert_eq!(stats.completed, 0);
        assert_eq!(stats.pending, 1);
    }

    #[test]
    fn many_tasks_priority_ordering() {
        let mut s = DroneScheduler::new(100);
        for i in 0..20 {
            s.submit_task(kind(DroneTaskKind::Sync), format!("s{i}"), i * 5);
        }
        let started = s.tick();
        assert_eq!(started.len(), 20);
        // IDs should be in descending priority order: id 20 (pri 95), id 19 (pri 90)...
        // The highest priority is the last submitted (i=19, priority=95).
        assert_eq!(started[0], 20); // priority 95
        assert_eq!(started[19], 1); // priority 0
    }
}
