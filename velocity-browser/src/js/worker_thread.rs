use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct WorkerMessage {
    pub channel_id: String,
    pub payload_json: String,
}

pub struct WorkerThread {
    pub worker_id: String,
    pub script_url: String,
    pub inbox: VecDeque<WorkerMessage>,
}

pub struct WebWorkerPool {
    pub workers: Vec<WorkerThread>,
}

impl Default for WebWorkerPool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebWorkerPool {
    pub fn new() -> Self {
        Self { workers: Vec::new() }
    }

    pub fn spawn_worker(&mut self, script_url: &str) -> String {
        let id = format!("worker_{}", self.workers.len() + 1);
        self.workers.push(WorkerThread {
            worker_id: id.clone(),
            script_url: script_url.to_string(),
            inbox: VecDeque::new(),
        });
        id
    }

    pub fn post_message(&mut self, worker_id: &str, payload_json: &str) -> bool {
        if let Some(worker) = self.workers.iter_mut().find(|w| w.worker_id == worker_id) {
            worker.inbox.push_back(WorkerMessage {
                channel_id: "main".to_string(),
                payload_json: payload_json.to_string(),
            });
            true
        } else {
            false
        }
    }
}
