use crate::session::BrowserSession;

pub struct SwarmSessionOrchestrator {
    pub swarm_sessions: Vec<BrowserSession>,
}

impl Default for SwarmSessionOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl SwarmSessionOrchestrator {
    pub fn new() -> Self {
        Self {
            swarm_sessions: Vec::new(),
        }
    }

    pub fn spawn_swarm_tab(&mut self, session_id: &str) -> &mut BrowserSession {
        let session = BrowserSession::new(session_id.to_string());
        self.swarm_sessions.push(session);
        let idx = self.swarm_sessions.len() - 1;
        &mut self.swarm_sessions[idx]
    }

    pub fn active_swarm_count(&self) -> usize {
        self.swarm_sessions.len()
    }
}
