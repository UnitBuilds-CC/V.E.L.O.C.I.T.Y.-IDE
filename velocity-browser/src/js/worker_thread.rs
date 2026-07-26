//! Web Worker pool with lifecycle management, message passing, and
//! worker-to-main communication.

use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerState {
    Running,
    Suspended,
    Terminated,
}

#[derive(Debug, Clone)]
pub struct WorkerMessage {
    pub channel_id: String,
    pub payload_json: String,
}

pub struct WorkerThread {
    pub worker_id: String,
    pub script_url: String,
    pub inbox: VecDeque<WorkerMessage>,
    pub outbox: VecDeque<WorkerMessage>,
    pub state: WorkerState,
    pub on_message_script: Option<String>,
    pub error_count: u32,
    pub max_errors: u32,
}

/// A message channel for direct worker-to-worker communication.
pub struct MessageChannel {
    pub channel_id: String,
    pub port_a_worker: String,
    pub port_b_worker: String,
}

pub struct WebWorkerPool {
    pub workers: Vec<WorkerThread>,
    channels: Vec<MessageChannel>,
    next_channel_id: usize,
}

impl Default for WebWorkerPool {
    fn default() -> Self { Self::new() }
}

impl WebWorkerPool {
    pub fn new() -> Self {
        Self { workers: Vec::new(), channels: Vec::new(), next_channel_id: 1 }
    }

    /// Spawn a new worker and return its ID.
    pub fn spawn_worker(&mut self, script_url: &str) -> String {
        let id = format!("worker_{}", self.workers.len() + 1);
        self.workers.push(WorkerThread {
            worker_id: id.clone(),
            script_url: script_url.to_string(),
            inbox: VecDeque::new(),
            outbox: VecDeque::new(),
            state: WorkerState::Running,
            on_message_script: None,
            error_count: 0,
            max_errors: 10,
        });
        id
    }

    /// Post a message to a worker's inbox.
    pub fn post_message(&mut self, worker_id: &str, payload_json: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            if worker.state != WorkerState::Running { return false; }
            worker.inbox.push_back(WorkerMessage {
                channel_id: "main".to_string(),
                payload_json: payload_json.to_string(),
            });
            true
        } else {
            false
        }
    }

    /// Process all pending messages in a worker's inbox.
    /// Simulates script execution: each message is "handled" and a response
    /// is placed in the worker's outbox.
    pub fn process_messages(&mut self, worker_id: &str) -> usize {
        let worker = match self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            Some(w) if w.state == WorkerState::Running => w,
            _ => return 0,
        };

        let messages: Vec<WorkerMessage> = worker.inbox.drain(..).collect();
        let count = messages.len();
        let handler = worker.on_message_script.clone();

        for msg in messages {
            // Simulate message handling: produce a response echoing the payload
            let response = if let Some(ref _script) = handler {
                WorkerMessage {
                    channel_id: msg.channel_id.clone(),
                    payload_json: format!("{{\"echo\":{}}}", msg.payload_json),
                }
            } else {
                WorkerMessage {
                    channel_id: msg.channel_id.clone(),
                    payload_json: format!("{{\"processed\":{}}}", msg.payload_json),
                }
            };
            worker.outbox.push_back(response);
        }
        count
    }

    /// Drain messages from a worker's outbox (worker → main).
    pub fn drain_main_messages(&mut self, worker_id: &str) -> Vec<WorkerMessage> {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            worker.outbox.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Register an onmessage handler script for a worker.
    pub fn set_on_message(&mut self, worker_id: &str, script: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            worker.on_message_script = Some(script.to_string());
            true
        } else {
            false
        }
    }

    /// Terminate a worker: mark as terminated and drain all queues.
    pub fn terminate_worker(&mut self, worker_id: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            worker.state = WorkerState::Terminated;
            worker.inbox.clear();
            worker.outbox.clear();
            true
        } else {
            false
        }
    }

    /// Suspend a running worker (pauses message processing).
    pub fn suspend_worker(&mut self, worker_id: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            if worker.state == WorkerState::Running {
                worker.state = WorkerState::Suspended;
                return true;
            }
        }
        false
    }

    /// Resume a suspended worker.
    pub fn resume_worker(&mut self, worker_id: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            if worker.state == WorkerState::Suspended {
                worker.state = WorkerState::Running;
                return true;
            }
        }
        false
    }

    /// Terminate all workers.
    pub fn terminate_all(&mut self) {
        for worker in &mut self.workers {
            worker.state = WorkerState::Terminated;
            worker.inbox.clear();
            worker.outbox.clear();
        }
    }

    /// Total number of workers (including terminated).
    pub fn worker_count(&self) -> usize { self.workers.len() }

    /// Number of currently running workers.
    pub fn active_worker_count(&self) -> usize {
        self.workers.iter().filter(|w| w.state == WorkerState::Running).count()
    }

    /// Get a worker's current state.
    pub fn worker_state(&self, worker_id: &str) -> Option<&WorkerState> {
        self.workers.iter().find(|w| w.worker_id == worker_id).map(|w| &w.state)
    }

    /// Record an error for a worker. If max_errors exceeded, auto-terminate.
    pub fn record_error(&mut self, worker_id: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            worker.error_count += 1;
            if worker.error_count >= worker.max_errors {
                worker.state = WorkerState::Terminated;
            }
            true
        } else {
            false
        }
    }

    /// Create a message channel between two workers.
    pub fn create_channel(&mut self, worker_a: &str, worker_b: &str) -> Option<String> {
        // Verify both workers exist
        if !self.workers.iter().any(|w| w.worker_id == worker_a) { return None; }
        if !self.workers.iter().any(|w| w.worker_id == worker_b) { return None; }
        let cid = format!("channel_{}", self.next_channel_id);
        self.next_channel_id += 1;
        self.channels.push(MessageChannel {
            channel_id: cid.clone(),
            port_a_worker: worker_a.to_string(),
            port_b_worker: worker_b.to_string(),
        });
        Some(cid)
    }

    /// Send a message through a channel from worker A to worker B.
    pub fn send_channel_message(&mut self, channel_id: &str, from_worker: &str, payload: &str) -> bool {
        let channel = match self.channels.iter().find(|c| c.channel_id == channel_id) {
            Some(c) => c.clone(),
            None => return false,
        };
        // Determine target: if from is A, send to B, and vice versa
        let target = if channel.port_a_worker == from_worker {
            &channel.port_b_worker
        } else if channel.port_b_worker == from_worker {
            &channel.port_a_worker
        } else {
            return false;
        };
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == *target) {
            if worker.state == WorkerState::Running {
                worker.inbox.push_back(WorkerMessage {
                    channel_id: channel_id.to_string(),
                    payload_json: payload.to_string(),
                });
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_message_roundtrip() {
        let mut pool = WebWorkerPool::new();
        let id = pool.spawn_worker("worker.js");
        assert_eq!(pool.active_worker_count(), 1);

        pool.post_message(&id, "\"hello\"");
        let processed = pool.process_messages(&id);
        assert_eq!(processed, 1);

        let msgs = pool.drain_main_messages(&id);
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].payload_json.contains("hello"));
    }

    #[test]
    fn terminate_and_suspend() {
        let mut pool = WebWorkerPool::new();
        let id = pool.spawn_worker("w.js");

        assert!(pool.suspend_worker(&id));
        assert_eq!(*pool.worker_state(&id).unwrap(), WorkerState::Suspended);
        assert!(!pool.post_message(&id, "test")); // can't post to suspended

        assert!(pool.resume_worker(&id));
        assert!(pool.post_message(&id, "test"));

        assert!(pool.terminate_worker(&id));
        assert_eq!(*pool.worker_state(&id).unwrap(), WorkerState::Terminated);
        assert_eq!(pool.active_worker_count(), 0);
    }

    #[test]
    fn error_auto_terminate() {
        let mut pool = WebWorkerPool::new();
        let id = pool.spawn_worker("w.js");
        // Set low threshold
        pool.workers[0].max_errors = 3;
        for _ in 0..3 { pool.record_error(&id); }
        assert_eq!(*pool.worker_state(&id).unwrap(), WorkerState::Terminated);
    }

    #[test]
    fn channel_communication() {
        let mut pool = WebWorkerPool::new();
        let a = pool.spawn_worker("a.js");
        let b = pool.spawn_worker("b.js");
        let ch = pool.create_channel(&a, &b).unwrap();

        assert!(pool.send_channel_message(&ch, &a, "\"from_a\""));
        // Worker B should have the message in inbox
        let msgs = pool.process_messages(&b);
        assert_eq!(msgs, 1);
        let out = pool.drain_main_messages(&b);
        assert!(out[0].payload_json.contains("from_a"));
    }
}
