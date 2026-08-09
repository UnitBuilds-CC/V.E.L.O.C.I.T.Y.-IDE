//! Proactive background agent system.
//!
//! Background agents monitor the workspace and external systems, providing
//! proactive alerts, suggestions, and autonomous maintenance. They run
//! independently of the main agent loop and can be triggered by events
//! or scheduled intervals.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A background agent that monitors and acts on events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundAgent {
    /// Unique agent identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What this agent monitors.
    pub monitor: MonitorType,
    /// How often to check (in seconds).
    pub interval_secs: u64,
    /// Whether this agent is currently active.
    pub enabled: bool,
    /// Last time this agent ran.
    pub last_run: Option<u64>,
    /// Actions produced by this agent.
    pub actions: Vec<AgentAction>,
    /// Maximum actions to retain (FIFO eviction).
    pub max_actions: usize,
}

/// What a background agent monitors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MonitorType {
    /// Watch for file changes in a directory.
    FileChanges { path: String, patterns: Vec<String> },
    /// Monitor build/test results.
    BuildHealth { command: String },
    /// Check for dependency updates.
    DependencyUpdates { check_command: String },
    /// Monitor log files for errors.
    LogErrors {
        log_path: String,
        error_patterns: Vec<String>,
    },
    /// Git status monitoring (uncommitted changes, behind remote).
    GitStatus { repo_path: String },
    /// Custom periodic check.
    Custom { prompt: String },
}

impl MonitorType {
    pub fn label(&self) -> &str {
        match self {
            Self::FileChanges { .. } => "file_changes",
            Self::BuildHealth { .. } => "build_health",
            Self::DependencyUpdates { .. } => "dependency_updates",
            Self::LogErrors { .. } => "log_errors",
            Self::GitStatus { .. } => "git_status",
            Self::Custom { .. } => "custom",
        }
    }
}

/// An action produced by a background agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    /// Unique action ID.
    pub id: String,
    /// When this action was created.
    pub timestamp: u64,
    /// Severity level.
    pub severity: ActionSeverity,
    /// Short title.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Suggested next step (if any).
    pub suggestion: Option<String>,
    /// Whether the user has acknowledged this action.
    pub acknowledged: bool,
}

/// Severity of an agent action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionSeverity {
    /// Informational — no action needed.
    Info,
    /// Suggestion — user might want to act.
    Suggestion,
    /// Warning — should look at this soon.
    Warning,
    /// Critical — immediate attention needed.
    Critical,
}

impl ActionSeverity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Suggestion => "suggestion",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

/// Registry managing all background agents.
#[derive(Debug, Clone, Default)]
pub struct BackgroundAgentRegistry {
    pub agents: HashMap<String, BackgroundAgent>,
    /// Global action feed (merged from all agents).
    pub action_feed: Vec<AgentAction>,
    /// Maximum actions in the global feed.
    pub max_feed_size: usize,
}

impl BackgroundAgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            action_feed: Vec::new(),
            max_feed_size: 100,
        }
    }

    /// Register a new background agent.
    pub fn register(&mut self, agent: BackgroundAgent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    /// Remove a background agent.
    pub fn unregister(&mut self, id: &str) -> bool {
        self.agents.remove(id).is_some()
    }

    /// Enable/disable an agent.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(agent) = self.agents.get_mut(id) {
            agent.enabled = enabled;
        }
    }

    /// Get all enabled agents that are due for a check.
    pub fn due_agents(&self, now: u64) -> Vec<&BackgroundAgent> {
        self.agents
            .values()
            .filter(|a| {
                a.enabled
                    && match a.last_run {
                        Some(last) => now - last >= a.interval_secs,
                        None => true,
                    }
            })
            .collect()
    }

    /// Record an action from an agent.
    pub fn record_action(&mut self, agent_id: &str, action: AgentAction) {
        // Add to agent's local list.
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.actions.push(action.clone());
            agent.last_run = Some(now_secs());
            // Evict old actions.
            while agent.actions.len() > agent.max_actions {
                agent.actions.remove(0);
            }
        }
        // Add to global feed.
        self.action_feed.push(action);
        while self.action_feed.len() > self.max_feed_size {
            self.action_feed.remove(0);
        }
    }

    /// Get unacknowledged actions.
    pub fn pending_actions(&self) -> Vec<&AgentAction> {
        self.action_feed
            .iter()
            .filter(|a| !a.acknowledged)
            .collect()
    }

    /// Acknowledge an action by ID.
    pub fn acknowledge(&mut self, action_id: &str) {
        for action in &mut self.action_feed {
            if action.id == action_id {
                action.acknowledged = true;
            }
        }
        for agent in self.agents.values_mut() {
            for action in &mut agent.actions {
                if action.id == action_id {
                    action.acknowledged = true;
                }
            }
        }
    }

    /// Count of pending (unacknowledged) actions.
    pub fn pending_count(&self) -> usize {
        self.action_feed.iter().filter(|a| !a.acknowledged).count()
    }

    /// Count of critical pending actions.
    pub fn critical_count(&self) -> usize {
        self.action_feed
            .iter()
            .filter(|a| !a.acknowledged && a.severity == ActionSeverity::Critical)
            .count()
    }

    /// Create default agents for a workspace.
    pub fn create_defaults(workspace_root: &Path) -> Self {
        let mut registry = Self::new();

        // Git status monitor.
        registry.register(BackgroundAgent {
            id: "git-monitor".to_string(),
            name: "Git Status Monitor".to_string(),
            monitor: MonitorType::GitStatus {
                repo_path: workspace_root.to_string_lossy().to_string(),
            },
            interval_secs: 60,
            enabled: true,
            last_run: None,
            actions: Vec::new(),
            max_actions: 20,
        });

        // Build health monitor.
        registry.register(BackgroundAgent {
            id: "build-monitor".to_string(),
            name: "Build Health Monitor".to_string(),
            monitor: MonitorType::BuildHealth {
                command: "cargo check --all-targets 2>&1".to_string(),
            },
            interval_secs: 300,
            enabled: false, // Disabled by default — user opts in
            last_run: None,
            actions: Vec::new(),
            max_actions: 10,
        });

        // Dependency update checker.
        registry.register(BackgroundAgent {
            id: "dep-checker".to_string(),
            name: "Dependency Update Checker".to_string(),
            monitor: MonitorType::DependencyUpdates {
                check_command: "cargo outdated 2>&1".to_string(),
            },
            interval_secs: 86400, // Once per day
            enabled: false,
            last_run: None,
            actions: Vec::new(),
            max_actions: 10,
        });

        registry
    }

    /// Save registry state to disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create dir: {e}"))?;

        // Save agent configs (without actions, which are transient).
        let configs: Vec<BackgroundAgentConfig> = self
            .agents
            .values()
            .map(|a| BackgroundAgentConfig {
                id: a.id.clone(),
                name: a.name.clone(),
                monitor: a.monitor.clone(),
                interval_secs: a.interval_secs,
                enabled: a.enabled,
            })
            .collect();

        let json =
            serde_json::to_vec_pretty(&configs).map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("background_agents.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load registry state from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root
            .join(".velocity")
            .join("background_agents.json");
        let mut registry = Self::new();
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(configs) = serde_json::from_slice::<Vec<BackgroundAgentConfig>>(&bytes) {
                for config in configs {
                    registry.register(BackgroundAgent {
                        id: config.id,
                        name: config.name,
                        monitor: config.monitor,
                        interval_secs: config.interval_secs,
                        enabled: config.enabled,
                        last_run: None,
                        actions: Vec::new(),
                        max_actions: 20,
                    });
                }
            }
        }
        registry
    }
}

/// Serializable config for persisting agent definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackgroundAgentConfig {
    id: String,
    name: String,
    monitor: MonitorType,
    interval_secs: u64,
    enabled: bool,
}

/// Generate a unique action ID.
fn gen_action_id() -> String {
    let ts = now_secs();
    format!("act_{ts}_{}", ts % 10000)
}

/// Create an AgentAction.
pub fn create_action(
    severity: ActionSeverity,
    title: &str,
    description: &str,
    suggestion: Option<&str>,
) -> AgentAction {
    AgentAction {
        id: gen_action_id(),
        timestamp: now_secs(),
        severity,
        title: title.to_string(),
        description: description.to_string(),
        suggestion: suggestion.map(|s| s.to_string()),
        acknowledged: false,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_agent(id: &str) -> BackgroundAgent {
        BackgroundAgent {
            id: id.to_string(),
            name: format!("{id} Agent"),
            monitor: MonitorType::GitStatus {
                repo_path: ".".to_string(),
            },
            interval_secs: 60,
            enabled: true,
            last_run: None,
            actions: Vec::new(),
            max_actions: 10,
        }
    }

    #[test]
    fn register_and_unregister() {
        let mut registry = BackgroundAgentRegistry::new();
        registry.register(test_agent("a1"));
        registry.register(test_agent("a2"));
        assert_eq!(registry.agents.len(), 2);
        assert!(registry.unregister("a1"));
        assert_eq!(registry.agents.len(), 1);
    }

    #[test]
    fn due_agents_check() {
        let mut registry = BackgroundAgentRegistry::new();
        let mut agent = test_agent("timer");
        agent.interval_secs = 60;
        agent.last_run = Some(now_secs() - 120); // 2 minutes ago
        registry.register(agent);

        let due = registry.due_agents(now_secs());
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn record_and_pending_actions() {
        let mut registry = BackgroundAgentRegistry::new();
        registry.register(test_agent("monitor"));

        let action = create_action(
            ActionSeverity::Warning,
            "Uncommitted changes",
            "You have 5 uncommitted files",
            Some("Run git status to review"),
        );
        registry.record_action("monitor", action);

        assert_eq!(registry.pending_count(), 1);
        assert_eq!(registry.agents["monitor"].actions.len(), 1);
    }

    #[test]
    fn acknowledge_action() {
        let mut registry = BackgroundAgentRegistry::new();
        registry.register(test_agent("mon"));

        let action = create_action(ActionSeverity::Info, "Test", "Desc", None);
        let action_id = action.id.clone();
        registry.record_action("mon", action);

        assert_eq!(registry.pending_count(), 1);
        registry.acknowledge(&action_id);
        assert_eq!(registry.pending_count(), 0);
    }

    #[test]
    fn critical_count() {
        let mut registry = BackgroundAgentRegistry::new();
        registry.register(test_agent("crit"));

        registry.record_action(
            "crit",
            create_action(ActionSeverity::Info, "Info", "", None),
        );
        registry.record_action(
            "crit",
            create_action(ActionSeverity::Critical, "Critical 1", "", None),
        );
        registry.record_action(
            "crit",
            create_action(ActionSeverity::Critical, "Critical 2", "", None),
        );

        assert_eq!(registry.critical_count(), 2);
    }

    #[test]
    fn create_defaults() {
        let registry = BackgroundAgentRegistry::create_defaults(Path::new("/tmp/test"));
        assert!(registry.agents.len() >= 3);
        assert!(registry.agents.contains_key("git-monitor"));
        assert!(registry.agents.contains_key("build-monitor"));
        assert!(registry.agents.contains_key("dep-checker"));
    }

    #[test]
    fn enable_disable() {
        let mut registry = BackgroundAgentRegistry::new();
        registry.register(test_agent("toggle"));
        assert!(registry.agents["toggle"].enabled);
        registry.set_enabled("toggle", false);
        assert!(!registry.agents["toggle"].enabled);
    }

    #[test]
    fn action_feed_max_size() {
        let mut registry = BackgroundAgentRegistry::new();
        registry.max_feed_size = 5;
        registry.register(test_agent("feeder"));

        for i in 0..10 {
            registry.record_action(
                "feeder",
                create_action(ActionSeverity::Info, &format!("Action {i}"), "desc", None),
            );
        }
        assert_eq!(registry.action_feed.len(), 5);
    }
}
