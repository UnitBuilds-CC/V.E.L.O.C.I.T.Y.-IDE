use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskKind {
    MacroTask,
    MicroTask,
    Timer,
}

#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub id: u64,
    pub kind: TaskKind,
    pub script: String,
    pub delay_ms: u64,
}

pub struct JsEventLoopScheduler {
    pub task_queue: VecDeque<ScheduledTask>,
    pub microtask_queue: VecDeque<ScheduledTask>,
    pub seq: u64,
}

impl JsEventLoopScheduler {
    pub fn new() -> Self {
        Self {
            task_queue: VecDeque::new(),
            microtask_queue: VecDeque::new(),
            seq: 1,
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
        });
        id
    }

    pub fn queue_microtask(&mut self, script: &str) -> u64 {
        let id = self.seq;
        self.seq += 1;
        self.microtask_queue.push_back(ScheduledTask {
            id,
            kind: TaskKind::MicroTask,
            script: script.to_string(),
            delay_ms: 0,
        });
        id
    }

    pub fn pop_next_task(&mut self) -> Option<ScheduledTask> {
        if let Some(micro) = self.microtask_queue.pop_front() {
            return Some(micro);
        }
        self.task_queue.pop_front()
    }
}
