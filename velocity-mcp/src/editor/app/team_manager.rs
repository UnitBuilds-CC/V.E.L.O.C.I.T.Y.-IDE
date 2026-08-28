use crate::agent::UiToAgentMessage;
use crossbeam_channel::Sender;

/// Lightweight UI-facing manager that forwards team-related user actions to the
/// agent runtime and retains a tiny rolling log for display in the Team Studio.
pub struct TeamManager {
    pub agent_tx: Sender<UiToAgentMessage>,
    pub logs: Vec<String>,
}

impl TeamManager {
    pub fn new(agent_tx: Sender<UiToAgentMessage>) -> Self {
        Self {
            agent_tx,
            logs: Vec::new(),
        }
    }

    /// Launch a team by slug. The agent runtime listens for a user prompt that
    /// starts with `@team:` or similar; using `UserPrompt` keeps the integration
    /// simple and text-driven (runtime already supports routed planning).
    pub fn launch_team(&self, slug: &str) {
        let prompt = format!("@{} launch", slug);
        let _ = self.agent_tx.send(UiToAgentMessage::UserPrompt(prompt));
    }

    /// Request the runtime to cancel ongoing tasks (best-effort).
    pub fn cancel_running(&self) {
        let _ = self.agent_tx.send(UiToAgentMessage::CancelTask);
    }

    pub fn reload_teams(&self) {
        let _ = self.agent_tx.send(UiToAgentMessage::ReloadTeams);
    }

    /// Append a small UI-visible log entry (keeps it bounded).
    pub fn push_log(&mut self, entry: impl Into<String>) {
        self.logs.push(entry.into());
        if self.logs.len() > 200 {
            self.logs.drain(0..(self.logs.len() - 200));
        }
    }
}
