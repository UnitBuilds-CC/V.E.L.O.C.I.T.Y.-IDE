use crate::js::vm::JsValue;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskKind {
    MacroTask,
    MicroTask,
    Timer,
    Interval,
}

#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub id: u64,
    pub kind: TaskKind,
    pub script: String,
    pub delay_ms: u64,
    /// Optional closure to invoke instead of script string.
    pub closure: Option<JsValue>,
}

pub struct JsEventLoopScheduler {
    pub task_queue: VecDeque<ScheduledTask>,
    pub microtask_queue: VecDeque<ScheduledTask>,
    pub interval_registry: Vec<ScheduledTask>,
    pub cancelled_ids: Vec<u64>,
    pub seq: u64,
    pub tick_limit: usize,
}

impl Default for JsEventLoopScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl JsEventLoopScheduler {
    pub fn new() -> Self {
        Self {
            task_queue: VecDeque::new(),
            microtask_queue: VecDeque::new(),
            interval_registry: Vec::new(),
            cancelled_ids: Vec::new(),
            seq: 1,
            tick_limit: 100,
        }
    }

    pub fn schedule_timer(&mut self, script: &str, delay_ms: u64) -> u64 {
        let id = self.seq;
        self.seq += 1;
        self.task_queue.push_back(ScheduledTask {
            id,
            kind: TaskKind::Timer,
            script: script.to_string(),
            delay_ms,
            closure: None,
        });
        id
    }

    /// Schedule a timer with a closure callback instead of a script string.
    pub fn schedule_timer_closure(&mut self, func: JsValue, delay_ms: u64) -> u64 {
        let id = self.seq;
        self.seq += 1;
        self.task_queue.push_back(ScheduledTask {
            id,
            kind: TaskKind::Timer,
            script: String::new(),
            delay_ms,
            closure: Some(func),
        });
        id
    }

    pub fn schedule_interval(&mut self, script: &str, delay_ms: u64) -> u64 {
        let id = self.seq;
        self.seq += 1;
        let task = ScheduledTask {
            id,
            kind: TaskKind::Interval,
            script: script.to_string(),
            delay_ms,
            closure: None,
        };
        self.interval_registry.push(task.clone());
        self.task_queue.push_back(task);
        id
    }

    /// Schedule an interval with a closure callback.
    pub fn schedule_interval_closure(&mut self, func: JsValue, delay_ms: u64) -> u64 {
        let id = self.seq;
        self.seq += 1;
        let task = ScheduledTask {
            id,
            kind: TaskKind::Interval,
            script: String::new(),
            delay_ms,
            closure: Some(func),
        };
        self.interval_registry.push(task.clone());
        self.task_queue.push_back(task);
        id
    }

    pub fn cancel_timer(&mut self, id: u64) {
        self.cancelled_ids.push(id);
        self.task_queue.retain(|t| t.id != id);
        self.interval_registry.retain(|t| t.id != id);
    }

    pub fn queue_microtask(&mut self, script: &str) -> u64 {
        let id = self.seq;
        self.seq += 1;
        self.microtask_queue.push_back(ScheduledTask {
            id,
            kind: TaskKind::MicroTask,
            script: script.to_string(),
            delay_ms: 0,
            closure: None,
        });
        id
    }

    /// Queue a microtask with a closure callback.
    pub fn queue_microtask_closure(&mut self, func: JsValue) -> u64 {
        let id = self.seq;
        self.seq += 1;
        self.microtask_queue.push_back(ScheduledTask {
            id,
            kind: TaskKind::MicroTask,
            script: String::new(),
            delay_ms: 0,
            closure: Some(func),
        });
        id
    }

    pub fn pop_next_task(&mut self) -> Option<ScheduledTask> {
        // Drain microtasks first
        if let Some(micro) = self.microtask_queue.pop_front() {
            return Some(micro);
        }
        // Then macro tasks / timers
        while let Some(task) = self.task_queue.pop_front() {
            if self.cancelled_ids.contains(&task.id) {
                continue;
            }
            // If interval, re-queue for next firing
            if task.kind == TaskKind::Interval
                && self.interval_registry.iter().any(|t| t.id == task.id)
            {
                self.task_queue.push_back(task.clone());
            }
            return Some(task);
        }
        None
    }

    /// Drain all pending tasks up to tick_limit, returning the number of tasks executed.
    /// Caller must provide a closure to execute each task script.
    pub fn has_pending_tasks(&self) -> bool {
        !self.task_queue.is_empty() || !self.microtask_queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_timer_returns_id() {
        let mut sched = JsEventLoopScheduler::new();
        let id1 = sched.schedule_timer("alert(1)", 100);
        let id2 = sched.schedule_timer("alert(2)", 200);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn cancel_timer_removes_task() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.schedule_timer("alert(1)", 100);
        sched.cancel_timer(id);
        assert!(sched.pop_next_task().is_none());
    }

    #[test]
    fn interval_requeues() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.schedule_interval("tick()", 50);
        let t1 = sched.pop_next_task().unwrap();
        assert_eq!(t1.id, id);
        // Should be requeued
        assert!(sched.has_pending_tasks());
    }

    #[test]
    fn cancel_interval_stops_requeue() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.schedule_interval("tick()", 50);
        sched.cancel_timer(id);
        assert!(!sched.has_pending_tasks());
    }

    #[test]
    fn microtasks_drain_before_macrotasks() {
        let mut sched = JsEventLoopScheduler::new();
        sched.schedule_timer("macro", 0);
        sched.queue_microtask("micro");
        let first = sched.pop_next_task().unwrap();
        assert_eq!(first.script, "micro");
    }

    #[test]
    fn schedule_timer_closure_returns_id() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.schedule_timer_closure(JsValue::NativeFunction("cb".into()), 100);
        assert!(id > 0);
        let task = sched.pop_next_task().unwrap();
        assert!(task.closure.is_some());
        assert_eq!(task.delay_ms, 100);
    }

    #[test]
    fn schedule_interval_closure() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.schedule_interval_closure(JsValue::NativeFunction("tick".into()), 50);
        let task = sched.pop_next_task().unwrap();
        assert_eq!(task.id, id);
        assert!(task.closure.is_some());
        assert_eq!(task.kind, TaskKind::Interval);
    }

    #[test]
    fn queue_microtask_closure_returns_id() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.queue_microtask_closure(JsValue::NativeFunction("micro_cb".into()));
        assert!(id > 0);
        let task = sched.pop_next_task().unwrap();
        assert_eq!(task.kind, TaskKind::MicroTask);
        assert!(task.closure.is_some());
    }

    #[test]
    fn pop_next_task_returns_none_when_empty() {
        let mut sched = JsEventLoopScheduler::new();
        assert!(sched.pop_next_task().is_none());
    }

    #[test]
    fn has_pending_tasks_false_initially() {
        let sched = JsEventLoopScheduler::new();
        assert!(!sched.has_pending_tasks());
    }

    #[test]
    fn multiple_microtasks_fifo_order() {
        let mut sched = JsEventLoopScheduler::new();
        sched.queue_microtask("first");
        sched.queue_microtask("second");
        sched.queue_microtask("third");
        let t1 = sched.pop_next_task().unwrap();
        let t2 = sched.pop_next_task().unwrap();
        let t3 = sched.pop_next_task().unwrap();
        assert_eq!(t1.script, "first");
        assert_eq!(t2.script, "second");
        assert_eq!(t3.script, "third");
    }

    #[test]
    fn timer_ids_are_sequential() {
        let mut sched = JsEventLoopScheduler::new();
        let id1 = sched.schedule_timer("a", 10);
        let id2 = sched.schedule_timer("b", 20);
        let id3 = sched.schedule_timer("c", 30);
        assert_eq!(id2, id1 + 1);
        assert_eq!(id3, id2 + 1);
    }

    #[test]
    fn cancel_timer_removes_from_interval_registry() {
        let mut sched = JsEventLoopScheduler::new();
        let id = sched.schedule_interval("tick()", 50);
        assert_eq!(sched.interval_registry.len(), 1);
        sched.cancel_timer(id);
        assert!(sched.interval_registry.is_empty());
    }

    #[test]
    fn default_scheduler_is_empty() {
        let sched = JsEventLoopScheduler::default();
        assert!(!sched.has_pending_tasks());
        assert_eq!(sched.seq, 1);
        assert_eq!(sched.tick_limit, 100);
    }
}
