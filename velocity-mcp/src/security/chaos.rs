//! Chaos engineering and resilience testing framework.
//!
//! Provides structured experiments (latency injection, error bursts, resource
//! exhaustion, network partitions, random kills) that can be scheduled against
//! any subsystem target. After execution, results are recorded and a resilience
//! score together with a weakest-link analysis are produced.

use std::collections::HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// The *kind* of chaos being injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChaosKind {
    LatencyInjection,
    ErrorInjection,
    ResourceExhaustion,
    NetworkPartition,
    RandomKill,
}

/// Which subsystem the experiment targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChaosTarget {
    ProviderApi,
    FileSystem,
    IpcChannel,
    AgentLoop,
    BrowserEngine,
    All,
}

impl std::fmt::Display for ChaosTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderApi => f.write_str("ProviderApi"),
            Self::FileSystem => f.write_str("FileSystem"),
            Self::IpcChannel => f.write_str("IpcChannel"),
            Self::AgentLoop => f.write_str("AgentLoop"),
            Self::BrowserEngine => f.write_str("BrowserEngine"),
            Self::All => f.write_str("All"),
        }
    }
}

/// How severe / destructive the experiment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChaosSeverity {
    Low,
    Medium,
    High,
    Critical,
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// A single chaos experiment definition.
#[derive(Debug, Clone)]
pub struct ChaosExperiment {
    pub name: String,
    pub kind: ChaosKind,
    pub target: ChaosTarget,
    pub severity: ChaosSeverity,
    pub duration: Duration,
    pub started_at: Option<Instant>,
}

/// The outcome of running one experiment.
#[derive(Debug, Clone)]
pub struct ChaosResult {
    pub experiment_name: String,
    pub target: ChaosTarget,
    pub system_responded: bool,
    pub recovery_time_ms: Option<u64>,
    pub data_loss: bool,
    pub details: String,
}

/// Aggregate report produced by [`ChaosRunner::experiment_report`].
#[derive(Debug, Clone)]
pub struct ChaosReport {
    pub total_experiments: usize,
    pub passed: usize,
    pub failed: usize,
    pub resilience_score: f64,
    pub weakest_target: Option<String>,
    pub recommendations: Vec<String>,
}

/// Maximum number of chaos results retained before ring-buffer eviction.
const MAX_CHAOS_RESULTS: usize = 256;

/// Orchestrates scheduling, execution tracking, and reporting.
#[derive(Debug)]
pub struct ChaosRunner {
    pub experiments: Vec<ChaosExperiment>,
    pub results: Vec<ChaosResult>,
    pub active_experiment: Option<ChaosExperiment>,
}

// ---------------------------------------------------------------------------
// ChaosRunner implementation
// ---------------------------------------------------------------------------

impl ChaosRunner {
    /// Create a new, empty runner.
    pub fn new() -> Self {
        Self {
            experiments: Vec::new(),
            results: Vec::new(),
            active_experiment: None,
        }
    }

    /// Queue an experiment for later execution.
    pub fn schedule(&mut self, experiment: ChaosExperiment) {
        self.experiments.push(experiment);
    }

    /// Pop the next queued experiment, mark it as active, and return a
    /// reference to it.  Returns `None` when the queue is empty.
    pub fn start_next(&mut self) -> Option<&ChaosExperiment> {
        if self.experiments.is_empty() {
            return None;
        }
        let mut exp = self.experiments.remove(0);
        exp.started_at = Some(Instant::now());
        self.active_experiment = Some(exp);
        self.active_experiment.as_ref()
    }

    /// Record the outcome of the most recently started experiment.
    pub fn record_result(&mut self, result: ChaosResult) {
        self.results.push(result);
        if self.results.len() > MAX_CHAOS_RESULTS {
            self.results.remove(0);
        }
    }

    /// Percentage of experiments where the system responded correctly
    /// (`system_responded == true`).  Returns `100.0` when no results exist.
    pub fn resilience_score(&self) -> f64 {
        if self.results.is_empty() {
            return 100.0;
        }
        let passed = self.results.iter().filter(|r| r.system_responded).count();
        (passed as f64 / self.results.len() as f64) * 100.0
    }

    /// Return the [`ChaosTarget`] that has the most failures
    /// (`system_responded == false`).  Ties are broken by enum declaration
    /// order (first encountered wins).
    pub fn weakest_link(&self) -> Option<ChaosTarget> {
        let mut failures: HashMap<ChaosTarget, usize> = HashMap::new();
        for r in &self.results {
            if !r.system_responded {
                *failures.entry(r.target).or_insert(0) += 1;
            }
        }
        failures
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(target, _)| target)
    }

    /// Build a full [`ChaosReport`] from the recorded results.
    pub fn experiment_report(&self) -> ChaosReport {
        let total = self.results.len();
        let passed = self.results.iter().filter(|r| r.system_responded).count();
        let failed = total - passed;
        let score = self.resilience_score();
        let weakest = self.weakest_link();

        let mut recommendations = Vec::new();

        if let Some(ref target) = weakest {
            recommendations.push(format!(
                "Harden the {target} subsystem — it had the most experiment failures."
            ));
        }

        if score < 80.0 {
            recommendations.push(
                "Overall resilience score is below 80 % — consider adding \
                 circuit-breakers and retry logic across all connectors."
                    .to_string(),
            );
        }

        let data_loss_count = self.results.iter().filter(|r| r.data_loss).count();
        if data_loss_count > 0 {
            recommendations.push(format!(
                "{data_loss_count} experiment(s) caused data loss — review \
                 persistence and WAL strategies."
            ));
        }

        let high_recovery = self
            .results
            .iter()
            .filter(|r| r.recovery_time_ms.is_some_and(|ms| ms > 5000))
            .count();
        if high_recovery > 0 {
            recommendations.push(format!(
                "{high_recovery} experiment(s) had recovery time > 5 s — \
                 optimise failover paths."
            ));
        }

        ChaosReport {
            total_experiments: total,
            passed,
            failed,
            resilience_score: score,
            weakest_target: weakest.map(|t| t.to_string()),
            recommendations,
        }
    }
}

impl Default for ChaosRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Predefined experiments
// ---------------------------------------------------------------------------

/// 500 ms latency injection targeting `target`.
pub fn latency_spike(target: ChaosTarget, duration: Duration) -> ChaosExperiment {
    ChaosExperiment {
        name: "latency_spike".to_string(),
        kind: ChaosKind::LatencyInjection,
        target,
        severity: ChaosSeverity::Medium,
        duration,
        started_at: None,
    }
}

/// `count` consecutive errors targeting `target`.
pub fn error_burst(target: ChaosTarget, count: u32) -> ChaosExperiment {
    ChaosExperiment {
        name: format!("error_burst_{count}"),
        kind: ChaosKind::ErrorInjection,
        target,
        severity: ChaosSeverity::High,
        duration: Duration::from_millis(count as u64 * 100),
        started_at: None,
    }
}

/// Simulate the provider being completely unavailable.
pub fn provider_down(target: ChaosTarget) -> ChaosExperiment {
    ChaosExperiment {
        name: "provider_down".to_string(),
        kind: ChaosKind::NetworkPartition,
        target,
        severity: ChaosSeverity::Critical,
        duration: Duration::from_secs(30),
        started_at: None,
    }
}

/// Simulate high memory usage on the whole system.
pub fn memory_pressure() -> ChaosExperiment {
    ChaosExperiment {
        name: "memory_pressure".to_string(),
        kind: ChaosKind::ResourceExhaustion,
        target: ChaosTarget::All,
        severity: ChaosSeverity::High,
        duration: Duration::from_secs(60),
        started_at: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // -- helpers -----------------------------------------------------------

    fn pass(name: &str, target: ChaosTarget) -> ChaosResult {
        ChaosResult {
            experiment_name: name.to_string(),
            target,
            system_responded: true,
            recovery_time_ms: Some(50),
            data_loss: false,
            details: "ok".to_string(),
        }
    }

    fn fail(name: &str, target: ChaosTarget) -> ChaosResult {
        ChaosResult {
            experiment_name: name.to_string(),
            target,
            system_responded: false,
            recovery_time_ms: None,
            data_loss: false,
            details: "simulated failure".to_string(),
        }
    }

    // -- 1. new runner is empty -------------------------------------------
    #[test]
    fn new_runner_is_empty() {
        let runner = ChaosRunner::new();
        assert!(runner.experiments.is_empty());
        assert!(runner.results.is_empty());
        assert!(runner.active_experiment.is_none());
    }

    // -- 2. schedule adds to queue ----------------------------------------
    #[test]
    fn schedule_adds_experiment() {
        let mut runner = ChaosRunner::new();
        let exp = latency_spike(ChaosTarget::ProviderApi, Duration::from_millis(500));
        runner.schedule(exp);
        assert_eq!(runner.experiments.len(), 1);
    }

    // -- 3. start_next dequeues and sets started_at -----------------------
    #[test]
    fn start_next_dequeues() {
        let mut runner = ChaosRunner::new();
        runner.schedule(latency_spike(
            ChaosTarget::FileSystem,
            Duration::from_secs(1),
        ));
        let active = runner.start_next().expect("should return experiment");
        assert_eq!(active.name, "latency_spike");
        assert!(active.started_at.is_some());
        assert!(runner.experiments.is_empty());
    }

    // -- 4. start_next returns None when empty ----------------------------
    #[test]
    fn start_next_none_when_empty() {
        let mut runner = ChaosRunner::new();
        assert!(runner.start_next().is_none());
    }

    // -- 5. record_result stores result -----------------------------------
    #[test]
    fn record_result_stores() {
        let mut runner = ChaosRunner::new();
        runner.record_result(pass("e1", ChaosTarget::ProviderApi));
        assert_eq!(runner.results.len(), 1);
    }

    // -- 6. resilience_score all pass -------------------------------------
    #[test]
    fn resilience_score_all_pass() {
        let mut runner = ChaosRunner::new();
        runner.record_result(pass("e1", ChaosTarget::ProviderApi));
        runner.record_result(pass("e2", ChaosTarget::FileSystem));
        assert!((runner.resilience_score() - 100.0).abs() < f64::EPSILON);
    }

    // -- 7. resilience_score mixed ----------------------------------------
    #[test]
    fn resilience_score_mixed() {
        let mut runner = ChaosRunner::new();
        runner.record_result(pass("e1", ChaosTarget::ProviderApi));
        runner.record_result(fail("e2", ChaosTarget::FileSystem));
        runner.record_result(pass("e3", ChaosTarget::IpcChannel));
        runner.record_result(fail("e4", ChaosTarget::AgentLoop));
        // 2/4 = 50 %
        assert!((runner.resilience_score() - 50.0).abs() < f64::EPSILON);
    }

    // -- 8. resilience_score empty is 100 ---------------------------------
    #[test]
    fn resilience_score_empty_is_100() {
        let runner = ChaosRunner::new();
        assert!((runner.resilience_score() - 100.0).abs() < f64::EPSILON);
    }

    // -- 9. weakest_link identifies worst target --------------------------
    #[test]
    fn weakest_link_identified() {
        let mut runner = ChaosRunner::new();
        // ProviderApi fails 3×, FileSystem 1×
        for _ in 0..3 {
            runner.record_result(fail("e", ChaosTarget::ProviderApi));
        }
        runner.record_result(fail("e", ChaosTarget::FileSystem));
        assert_eq!(runner.weakest_link(), Some(ChaosTarget::ProviderApi));
    }

    // -- 10. weakest_link None when no failures ---------------------------
    #[test]
    fn weakest_link_none_when_no_failures() {
        let mut runner = ChaosRunner::new();
        runner.record_result(pass("e1", ChaosTarget::ProviderApi));
        assert!(runner.weakest_link().is_none());
    }

    // -- 11. experiment_report basic --------------------------------------
    #[test]
    fn experiment_report_basic() {
        let mut runner = ChaosRunner::new();
        runner.record_result(pass("e1", ChaosTarget::ProviderApi));
        runner.record_result(fail("e2", ChaosTarget::FileSystem));
        let report = runner.experiment_report();
        assert_eq!(report.total_experiments, 2);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert!((report.resilience_score - 50.0).abs() < f64::EPSILON);
        assert_eq!(report.weakest_target.as_deref(), Some("FileSystem"));
    }

    // -- 12. report recommendations: low score ----------------------------
    #[test]
    fn report_recommendation_low_score() {
        let mut runner = ChaosRunner::new();
        for _ in 0..5 {
            runner.record_result(fail("e", ChaosTarget::AgentLoop));
        }
        let report = runner.experiment_report();
        assert!(report
            .recommendations
            .iter()
            .any(|r| r.contains("below 80")));
    }

    // -- 13. report recommendations: data loss ----------------------------
    #[test]
    fn report_recommendation_data_loss() {
        let mut runner = ChaosRunner::new();
        runner.results.push(ChaosResult {
            experiment_name: "e".to_string(),
            target: ChaosTarget::FileSystem,
            system_responded: true,
            recovery_time_ms: None,
            data_loss: true,
            details: String::new(),
        });
        let report = runner.experiment_report();
        assert!(report
            .recommendations
            .iter()
            .any(|r| r.contains("data loss")));
    }

    // -- 14. report recommendations: high recovery time -------------------
    #[test]
    fn report_recommendation_high_recovery() {
        let mut runner = ChaosRunner::new();
        runner.results.push(ChaosResult {
            experiment_name: "e".to_string(),
            target: ChaosTarget::IpcChannel,
            system_responded: true,
            recovery_time_ms: Some(10_000),
            data_loss: false,
            details: String::new(),
        });
        let report = runner.experiment_report();
        assert!(report
            .recommendations
            .iter()
            .any(|r| r.contains("recovery time")));
    }

    // -- 15. predefined: latency_spike ------------------------------------
    #[test]
    fn predefined_latency_spike() {
        let exp = latency_spike(ChaosTarget::BrowserEngine, Duration::from_millis(500));
        assert_eq!(exp.kind, ChaosKind::LatencyInjection);
        assert_eq!(exp.target, ChaosTarget::BrowserEngine);
        assert_eq!(exp.severity, ChaosSeverity::Medium);
        assert_eq!(exp.duration, Duration::from_millis(500));
    }

    // -- 16. predefined: error_burst --------------------------------------
    #[test]
    fn predefined_error_burst() {
        let exp = error_burst(ChaosTarget::ProviderApi, 10);
        assert_eq!(exp.kind, ChaosKind::ErrorInjection);
        assert_eq!(exp.severity, ChaosSeverity::High);
        assert_eq!(exp.duration, Duration::from_millis(1000));
        assert!(exp.name.contains("10"));
    }

    // -- 17. predefined: provider_down ------------------------------------
    #[test]
    fn predefined_provider_down() {
        let exp = provider_down(ChaosTarget::ProviderApi);
        assert_eq!(exp.kind, ChaosKind::NetworkPartition);
        assert_eq!(exp.severity, ChaosSeverity::Critical);
        assert_eq!(exp.duration, Duration::from_secs(30));
    }

    // -- 18. predefined: memory_pressure ----------------------------------
    #[test]
    fn predefined_memory_pressure() {
        let exp = memory_pressure();
        assert_eq!(exp.kind, ChaosKind::ResourceExhaustion);
        assert_eq!(exp.target, ChaosTarget::All);
        assert_eq!(exp.severity, ChaosSeverity::High);
        assert_eq!(exp.duration, Duration::from_secs(60));
    }

    // -- 19. ChaosTarget Display ------------------------------------------
    #[test]
    fn chaos_target_display() {
        assert_eq!(ChaosTarget::ProviderApi.to_string(), "ProviderApi");
        assert_eq!(ChaosTarget::All.to_string(), "All");
        assert_eq!(ChaosTarget::BrowserEngine.to_string(), "BrowserEngine");
    }

    // -- 20. full pipeline: schedule → start → record → report ------------
    #[test]
    fn full_pipeline() {
        let mut runner = ChaosRunner::new();
        runner.schedule(latency_spike(
            ChaosTarget::ProviderApi,
            Duration::from_millis(100),
        ));
        runner.schedule(error_burst(ChaosTarget::FileSystem, 5));
        runner.schedule(provider_down(ChaosTarget::IpcChannel));

        // Run experiment 1
        let exp = runner.start_next().unwrap().clone();
        runner.record_result(pass(&exp.name, exp.target));

        // Run experiment 2
        let exp = runner.start_next().unwrap().clone();
        runner.record_result(fail(&exp.name, exp.target));

        // Run experiment 3
        let exp = runner.start_next().unwrap().clone();
        runner.record_result(pass(&exp.name, exp.target));

        assert!(runner.experiments.is_empty());
        assert_eq!(runner.results.len(), 3);

        let report = runner.experiment_report();
        assert_eq!(report.total_experiments, 3);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 1);
        assert!((report.resilience_score - 66.66666666666667).abs() < 0.01);
        assert_eq!(report.weakest_target.as_deref(), Some("FileSystem"));
    }

    // -- 21. Default trait ------------------------------------------------
    #[test]
    fn default_runner() {
        let runner = ChaosRunner::default();
        assert!(runner.experiments.is_empty());
    }

    // -- 22. multiple failures same target --------------------------------
    #[test]
    fn weakest_link_multiple_failures_same_target() {
        let mut runner = ChaosRunner::new();
        runner.record_result(fail("a", ChaosTarget::BrowserEngine));
        runner.record_result(fail("b", ChaosTarget::BrowserEngine));
        runner.record_result(fail("c", ChaosTarget::ProviderApi));
        assert_eq!(runner.weakest_link(), Some(ChaosTarget::BrowserEngine));
    }
}
