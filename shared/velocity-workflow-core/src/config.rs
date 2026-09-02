//! Engine configuration.
use serde::{Deserialize, Serialize};

/// Configuration for the workflow engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Number of steps to batch before forcing a commit (fsync).
    /// Set to 1 for immediate persistence per step (safe but slow).
    /// Set to 0 for unlimited batching (fast but risky).
    /// Recommended: 10-100 for most workloads.
    pub sync_steps: usize,

    /// Maximum number of concurrent workflow runs.
    pub max_concurrent_runs: usize,

    /// Maximum number of steps executed in parallel within a single run.
    pub max_step_parallelism: usize,

    /// Default step timeout in milliseconds.
    pub default_step_timeout_ms: u64,

    /// Path to the WAL/journal directory.
    pub journal_dir: std::path::PathBuf,

    /// Whether to fsync the journal on each batch commit.
    pub fsync_on_commit: bool,

    /// Worker pool size for remote step execution (0 = local only).
    pub worker_pool_size: usize,

    /// Heartbeat interval for worker health checks.
    pub worker_heartbeat_ms: u64,

    /// Enable multi-region replication.
    pub replication_enabled: bool,

    /// Replication factor (number of replicas).
    pub replication_factor: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sync_steps: 10,
            max_concurrent_runs: 64,
            max_step_parallelism: 4,
            default_step_timeout_ms: 30_000,
            journal_dir: std::path::PathBuf::from(".velocity/workflow-journal"),
            fsync_on_commit: true,
            worker_pool_size: 0,
            worker_heartbeat_ms: 5_000,
            replication_enabled: false,
            replication_factor: 1,
        }
    }
}

impl EngineConfig {
    /// Create a config optimized for safety (sync every step).
    pub fn safe() -> Self {
        Self { sync_steps: 1, ..Default::default() }
    }

    /// Create a config optimized for throughput (batch 100 steps).
    pub fn throughput() -> Self {
        Self { sync_steps: 100, ..Default::default() }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_concurrent_runs == 0 {
            return Err("max_concurrent_runs must be > 0".into());
        }
        if self.replication_factor < 1 {
            return Err("replication_factor must be >= 1".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = EngineConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.sync_steps, 10);
    }

    #[test]
    fn safe_config_syncs_every_step() {
        let config = EngineConfig::safe();
        assert_eq!(config.sync_steps, 1);
    }

    #[test]
    fn throughput_config_batches() {
        let config = EngineConfig::throughput();
        assert_eq!(config.sync_steps, 100);
    }
}
