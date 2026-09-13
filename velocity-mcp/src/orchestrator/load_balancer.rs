//! Load balancing and worker affinity for optimal task distribution.
//!
//! Provides multiple scheduling strategies (least-loaded, weighted round-robin,
//! affinity-first) and tracks per-worker statistics to make intelligent
//! assignment decisions.

#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Strategy used by [`LoadBalancer`] when picking a worker for a new task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceStrategy {
    /// Assign to the worker with the fewest active tasks.
    LeastLoaded,
    /// Distribute tasks proportionally to each worker's capacity.
    WeightedRoundRobin,
    /// Prefer workers that have previously handled similar task kinds.
    AffinityFirst,
}

/// Per-worker load snapshot maintained by the [`LoadBalancer`].
#[derive(Debug, Clone)]
pub struct WorkerLoad {
    pub worker_id: u64,
    pub active_tasks: usize,
    pub max_capacity: usize,
    /// Exponential moving average of task duration in milliseconds.
    pub avg_task_duration_ms: f64,
    /// Task kinds this worker specialises in.
    pub specializations: Vec<String>,
    pub last_task_completed: Option<Instant>,
    /// Rolling success rate in `[0.0, 1.0]`.
    pub success_rate: f64,
    /// Total tasks completed (used for EMA / success-rate weighting).
    total_completed: u64,
}

impl WorkerLoad {
    fn new(worker_id: u64, max_capacity: usize, specializations: Vec<String>) -> Self {
        Self {
            worker_id,
            active_tasks: 0,
            max_capacity,
            avg_task_duration_ms: 0.0,
            specializations,
            last_task_completed: None,
            success_rate: 1.0,
            total_completed: 0,
        }
    }

    /// Utilisation ratio in `[0.0, 1.0]`.
    pub fn utilization(&self) -> f64 {
        if self.max_capacity == 0 {
            return 1.0;
        }
        (self.active_tasks as f64) / (self.max_capacity as f64)
    }

    /// Returns `true` when the worker can still accept more work.
    pub fn has_capacity(&self) -> bool {
        self.active_tasks < self.max_capacity
    }
}

/// Aggregate health metrics for the whole worker cluster.
#[derive(Debug, Clone)]
pub struct ClusterHealth {
    pub total_workers: usize,
    pub active_workers: usize,
    pub avg_utilization: f64,
    pub max_utilization: f64,
    pub min_utilization: f64,
    /// 0.0 = perfectly balanced, 1.0 = maximally imbalanced.
    pub imbalance_score: f64,
}

// ---------------------------------------------------------------------------
// LoadBalancer
// ---------------------------------------------------------------------------

/// Distributes incoming tasks across a dynamic set of workers.
#[derive(Debug)]
pub struct LoadBalancer {
    workers: HashMap<u64, WorkerLoad>,
    strategy: BalanceStrategy,
    /// Monotonic counter used by [`BalanceStrategy::WeightedRoundRobin`].
    rr_counter: u64,
    /// EMA smoothing factor for task duration (α).
    ema_alpha: f64,
}

impl LoadBalancer {
    /// Create a new balancer with the given [`BalanceStrategy`].
    pub fn new(strategy: BalanceStrategy) -> Self {
        Self {
            workers: HashMap::new(),
            strategy,
            rr_counter: 0,
            ema_alpha: 0.3,
        }
    }

    // -- Worker management --------------------------------------------------

    /// Register a new worker.
    pub fn register_worker(&mut self, id: u64, capacity: usize, specializations: Vec<String>) {
        self.workers.insert(id, WorkerLoad::new(id, capacity, specializations));
    }

    /// Remove a worker from the pool.  In-flight tasks are *not* migrated
    /// automatically — callers should check `rebalance()` afterwards.
    pub fn unregister_worker(&mut self, id: u64) {
        self.workers.remove(&id);
    }

    // -- Task lifecycle -----------------------------------------------------

    /// Pick the best worker for a task of kind `task_kind` with the given
    /// estimated weight.  Returns `None` when every worker is at capacity.
    pub fn assign_task(&mut self, task_kind: &str, _estimated_weight: u32) -> Option<u64> {
        match self.strategy {
            BalanceStrategy::LeastLoaded => self.assign_least_loaded(),
            BalanceStrategy::WeightedRoundRobin => self.assign_weighted_rr(),
            BalanceStrategy::AffinityFirst => self.assign_affinity_first(task_kind),
        }
    }

    /// Record that a task has started on `worker_id`.
    pub fn record_task_start(&mut self, worker_id: u64) {
        if let Some(w) = self.workers.get_mut(&worker_id) {
            w.active_tasks += 1;
        }
    }

    /// Record that a task on `worker_id` finished.
    pub fn record_task_complete(&mut self, worker_id: u64, duration_ms: f64, success: bool) {
        if let Some(w) = self.workers.get_mut(&worker_id) {
            w.active_tasks = w.active_tasks.saturating_sub(1);
            w.last_task_completed = Some(Instant::now());

            // Update EMA of task duration.
            if w.total_completed == 0 {
                w.avg_task_duration_ms = duration_ms;
            } else {
                w.avg_task_duration_ms =
                    self.ema_alpha * duration_ms + (1.0 - self.ema_alpha) * w.avg_task_duration_ms;
            }

            // Update rolling success rate.
            let total_f = w.total_completed as f64 + 1.0;
            let prev_successes = w.success_rate * (w.total_completed as f64);
            let new_successes = prev_successes + if success { 1.0 } else { 0.0 };
            w.success_rate = new_successes / total_f;
            w.total_completed += 1;
        }
    }

    // -- Queries ------------------------------------------------------------

    /// Get a reference to a specific worker's load data.
    pub fn get_worker_load(&self, worker_id: u64) -> Option<&WorkerLoad> {
        self.workers.get(&worker_id)
    }

    /// Compute an overall health summary of the cluster.
    pub fn cluster_health(&self) -> ClusterHealth {
        let total = self.workers.len();
        if total == 0 {
            return ClusterHealth {
                total_workers: 0,
                active_workers: 0,
                avg_utilization: 0.0,
                max_utilization: 0.0,
                min_utilization: 0.0,
                imbalance_score: 0.0,
            };
        }

        let utils: Vec<f64> = self.workers.values().map(|w| w.utilization()).collect();
        let active = self.workers.values().filter(|w| w.active_tasks > 0).count();
        let avg = utils.iter().sum::<f64>() / total as f64;
        let max_u = utils.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let min_u = utils.iter().copied().fold(f64::INFINITY, f64::min);

        // Imbalance = coefficient of variation clamped to [0,1].
        let variance = utils.iter().map(|u| (u - avg).powi(2)).sum::<f64>() / total as f64;
        let stddev = variance.sqrt();
        let imbalance = if avg > 0.0 { (stddev / avg).min(1.0) } else { 0.0 };

        ClusterHealth {
            total_workers: total,
            active_workers: active,
            avg_utilization: avg,
            max_utilization: max_u,
            min_utilization: min_u,
            imbalance_score: imbalance,
        }
    }

    /// Suggest task migrations to rebalance the cluster.
    ///
    /// Returns a list of `(from_worker, to_worker)` pairs.  Each pair means
    /// "move one task from `from_worker` to `to_worker`".
    pub fn rebalance(&self) -> Vec<(u64, u64)> {
        if self.workers.len() < 2 {
            return Vec::new();
        }

        let total_tasks: usize = self.workers.values().map(|w| w.active_tasks).sum();
        let total_cap: usize = self.workers.values().map(|w| w.max_capacity).sum();
        if total_cap == 0 || total_tasks == 0 {
            return Vec::new();
        }

        // Ideal: each worker holds tasks proportional to its capacity share.
        let mut migrations: Vec<(u64, u64)> = Vec::new();

        // Build sorted lists of overloaded / underloaded workers.
        let mut workers: Vec<&WorkerLoad> = self.workers.values().collect();
        workers.sort_by(|a, b| {
            let ua = a.utilization();
            let ub = b.utilization();
            ua.partial_cmp(&ub).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Two-pointer: move tasks from the most loaded to the least loaded.
        let mut lo = 0usize;
        let mut hi = workers.len() - 1;

        // Track mutable "virtual" active counts so we don't double-migrate.
        let mut virtual_active: HashMap<u64, usize> = self
            .workers
            .iter()
            .map(|(id, w)| (*id, w.active_tasks))
            .collect();

        while lo < hi {
            let lo_id = workers[lo].worker_id;
            let hi_id = workers[hi].worker_id;
            let lo_cap = workers[lo].max_capacity;
            let hi_cap = workers[hi].max_capacity;

            let lo_util = if lo_cap > 0 {
                virtual_active[&lo_id] as f64 / lo_cap as f64
            } else {
                1.0
            };
            let hi_util = if hi_cap > 0 {
                virtual_active[&hi_id] as f64 / hi_cap as f64
            } else {
                1.0
            };

            // If the gap is small enough, stop.
            if (hi_util - lo_util) < 0.25 {
                break;
            }

            // Can we actually move a task?
            if virtual_active[&hi_id] == 0 || virtual_active[&lo_id] >= lo_cap {
                // Adjust pointers.
                if virtual_active[&hi_id] == 0 {
                    hi -= 1;
                }
                if virtual_active[&lo_id] >= lo_cap {
                    lo += 1;
                }
                continue;
            }

            migrations.push((hi_id, lo_id));
            virtual_active.insert(hi_id, virtual_active[&hi_id] - 1);
            virtual_active.insert(lo_id, virtual_active[&lo_id] + 1);

            // Re-check pointers.
            if virtual_active[&hi_id] == 0 {
                hi -= 1;
            }
            if virtual_active[&lo_id] >= lo_cap {
                lo += 1;
            }
        }

        migrations
    }

    // -- Strategy helpers ---------------------------------------------------

    fn assign_least_loaded(&self) -> Option<u64> {
        self.workers
            .values()
            .filter(|w| w.has_capacity())
            .min_by(|a, b| {
                a.active_tasks
                    .cmp(&b.active_tasks)
                    .then(a.worker_id.cmp(&b.worker_id))
            })
            .map(|w| w.worker_id)
    }

    fn assign_weighted_rr(&mut self) -> Option<u64> {
        // Collect eligible workers sorted by id for determinism.
        let mut eligible: Vec<&WorkerLoad> =
            self.workers.values().filter(|w| w.has_capacity()).collect();
        if eligible.is_empty() {
            return None;
        }
        eligible.sort_by_key(|w| w.worker_id);

        let idx = (self.rr_counter as usize) % eligible.len();
        self.rr_counter = self.rr_counter.wrapping_add(1);
        Some(eligible[idx].worker_id)
    }

    fn assign_affinity_first(&self, task_kind: &str) -> Option<u64> {
        // First pass: workers that specialise in this task kind AND have capacity.
        let affinity_match = self
            .workers
            .values()
            .filter(|w| w.has_capacity() && w.specializations.iter().any(|s| s == task_kind))
            .min_by(|a, b| {
                a.active_tasks
                    .cmp(&b.active_tasks)
                    .then(a.worker_id.cmp(&b.worker_id))
            });
        if let Some(w) = affinity_match {
            return Some(w.worker_id);
        }

        // Fallback: plain least-loaded among workers with capacity.
        self.assign_least_loaded()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn make_lb(strategy: BalanceStrategy) -> LoadBalancer {
        LoadBalancer::new(strategy)
    }

    fn register_three(lb: &mut LoadBalancer) {
        lb.register_worker(1, 4, vec!["compile".into(), "lint".into()]);
        lb.register_worker(2, 4, vec!["test".into(), "lint".into()]);
        lb.register_worker(3, 4, vec!["deploy".into()]);
    }

    // -- registration / unregistration --------------------------------------

    #[test]
    fn register_worker_adds_to_pool() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(10, 8, vec!["build".into()]);
        assert!(lb.get_worker_load(10).is_some());
        assert_eq!(lb.get_worker_load(10).unwrap().max_capacity, 8);
    }

    #[test]
    fn unregister_worker_removes_from_pool() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.unregister_worker(1);
        assert!(lb.get_worker_load(1).is_none());
        assert!(lb.get_worker_load(2).is_some());
    }

    #[test]
    fn unregister_nonexistent_is_noop() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.unregister_worker(999); // should not panic
    }

    // -- least-loaded -------------------------------------------------------

    #[test]
    fn least_loaded_picks_worker_with_fewest_tasks() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        register_three(&mut lb);
        lb.record_task_start(1);
        lb.record_task_start(1);
        lb.record_task_start(2);
        // Worker 3 has 0 tasks — should be picked.
        let id = lb.assign_task("anything", 1).unwrap();
        assert_eq!(id, 3);
    }

    #[test]
    fn least_loaded_returns_none_when_all_at_capacity() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 1, vec![]);
        lb.record_task_start(1);
        assert!(lb.assign_task("x", 1).is_none());
    }

    #[test]
    fn least_loaded_breaks_ties_by_id() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(5, 4, vec![]);
        lb.register_worker(3, 4, vec![]);
        // Both at 0 — lower id wins.
        let id = lb.assign_task("x", 1).unwrap();
        assert_eq!(id, 3);
    }

    // -- weighted round-robin -----------------------------------------------

    #[test]
    fn weighted_rr_cycles_through_workers() {
        let mut lb = make_lb(BalanceStrategy::WeightedRoundRobin);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.register_worker(3, 4, vec![]);

        let mut ids = Vec::new();
        for _ in 0..6 {
            ids.push(lb.assign_task("x", 1).unwrap());
        }
        // Should cycle through all three workers at least once.
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn weighted_rr_returns_none_when_full() {
        let mut lb = make_lb(BalanceStrategy::WeightedRoundRobin);
        lb.register_worker(1, 0, vec![]);
        assert!(lb.assign_task("x", 1).is_none());
    }

    // -- affinity-first -----------------------------------------------------

    #[test]
    fn affinity_first_prefers_specialized_worker() {
        let mut lb = make_lb(BalanceStrategy::AffinityFirst);
        register_three(&mut lb);
        // Worker 1 specialises in "compile".
        let id = lb.assign_task("compile", 1).unwrap();
        assert_eq!(id, 1);
    }

    #[test]
    fn affinity_first_falls_back_to_least_loaded() {
        let mut lb = make_lb(BalanceStrategy::AffinityFirst);
        register_three(&mut lb);
        // No worker specialises in "unknown" — should fall back to least loaded.
        let id = lb.assign_task("unknown", 1).unwrap();
        // All at 0, so lowest id.
        assert_eq!(id, 1);
    }

    #[test]
    fn affinity_first_skips_full_specialized_worker() {
        let mut lb = make_lb(BalanceStrategy::AffinityFirst);
        lb.register_worker(1, 1, vec!["compile".into()]);
        lb.register_worker(2, 4, vec![]);
        lb.record_task_start(1); // worker 1 is now full
        let id = lb.assign_task("compile", 1).unwrap();
        assert_eq!(id, 2);
    }

    // -- task lifecycle -----------------------------------------------------

    #[test]
    fn record_task_start_increments_active() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.record_task_start(1);
        assert_eq!(lb.get_worker_load(1).unwrap().active_tasks, 1);
    }

    #[test]
    fn record_task_complete_decrements_active() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(1);
        lb.record_task_complete(1, 100.0, true);
        assert_eq!(lb.get_worker_load(1).unwrap().active_tasks, 1);
    }

    #[test]
    fn task_complete_updates_success_rate() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);

        lb.record_task_complete(1, 50.0, true);
        lb.record_task_complete(1, 60.0, true);
        lb.record_task_complete(1, 70.0, false);

        let w = lb.get_worker_load(1).unwrap();
        // 2 successes out of 3.
        let expected = 2.0 / 3.0;
        assert!(
            (w.success_rate - expected).abs() < 0.01,
            "success_rate was {}",
            w.success_rate
        );
    }

    #[test]
    fn task_complete_updates_ema_duration() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.record_task_complete(1, 100.0, true);
        let first = lb.get_worker_load(1).unwrap().avg_task_duration_ms;
        assert!((first - 100.0).abs() < 0.01);

        lb.record_task_complete(1, 200.0, true);
        let second = lb.get_worker_load(1).unwrap().avg_task_duration_ms;
        // EMA: 0.3 * 200 + 0.7 * 100 = 130
        assert!((second - 130.0).abs() < 0.01, "ema was {}", second);
    }

    #[test]
    fn task_complete_sets_last_completed() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        assert!(lb.get_worker_load(1).unwrap().last_task_completed.is_none());
        lb.record_task_complete(1, 10.0, true);
        assert!(lb.get_worker_load(1).unwrap().last_task_completed.is_some());
    }

    // -- cluster health -----------------------------------------------------

    #[test]
    fn cluster_health_empty_cluster() {
        let lb = make_lb(BalanceStrategy::LeastLoaded);
        let h = lb.cluster_health();
        assert_eq!(h.total_workers, 0);
        assert_eq!(h.avg_utilization, 0.0);
    }

    #[test]
    fn cluster_health_reflects_utilization() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(1);
        // Worker 1: 2/4 = 0.5, Worker 2: 0/4 = 0.0
        let h = lb.cluster_health();
        assert_eq!(h.total_workers, 2);
        assert_eq!(h.active_workers, 1);
        assert!((h.avg_utilization - 0.25).abs() < 0.01);
        assert!((h.max_utilization - 0.5).abs() < 0.01);
        assert!((h.min_utilization - 0.0).abs() < 0.01);
    }

    #[test]
    fn cluster_health_imbalance_zero_when_balanced() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(2);
        let h = lb.cluster_health();
        assert!(h.imbalance_score < 0.01, "imbalance was {}", h.imbalance_score);
    }

    #[test]
    fn cluster_health_imbalance_high_when_skewed() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(1);
        lb.record_task_start(1);
        lb.record_task_start(1);
        // Worker 1 at 1.0, Worker 2 at 0.0.
        let h = lb.cluster_health();
        assert!(h.imbalance_score > 0.5, "imbalance was {}", h.imbalance_score);
    }

    // -- rebalance ----------------------------------------------------------

    #[test]
    fn rebalance_empty_cluster() {
        let lb = make_lb(BalanceStrategy::LeastLoaded);
        assert!(lb.rebalance().is_empty());
    }

    #[test]
    fn rebalance_single_worker_noop() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.record_task_start(1);
        assert!(lb.rebalance().is_empty());
    }

    #[test]
    fn rebalance_suggests_migration_when_imbalanced() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(1);
        lb.record_task_start(1);
        lb.record_task_start(1);
        // Worker 1 is full, Worker 2 is empty.
        let m = lb.rebalance();
        assert!(!m.is_empty(), "expected at least one migration");
        // All migrations should be from worker 1 to worker 2.
        for (from, to) in &m {
            assert_eq!(*from, 1);
            assert_eq!(*to, 2);
        }
    }

    #[test]
    fn rebalance_no_migration_when_balanced() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 4, vec![]);
        lb.register_worker(2, 4, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(2);
        let m = lb.rebalance();
        assert!(m.is_empty(), "expected no migrations, got {:?}", m);
    }

    // -- capacity enforcement -----------------------------------------------

    #[test]
    fn capacity_zero_means_always_full() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 0, vec![]);
        assert!(lb.assign_task("x", 1).is_none());
        assert!(!lb.get_worker_load(1).unwrap().has_capacity());
    }

    #[test]
    fn assign_respects_capacity_limits() {
        let mut lb = make_lb(BalanceStrategy::LeastLoaded);
        lb.register_worker(1, 2, vec![]);
        lb.register_worker(2, 2, vec![]);
        lb.record_task_start(1);
        lb.record_task_start(1);
        // Worker 1 is full; should go to worker 2.
        let id = lb.assign_task("x", 1).unwrap();
        assert_eq!(id, 2);
    }

    // -- utilization helper -------------------------------------------------

    #[test]
    fn worker_utilization_calculation() {
        let w = WorkerLoad::new(1, 10, vec![]);
        assert!((w.utilization() - 0.0).abs() < f64::EPSILON);
        let mut w2 = WorkerLoad::new(2, 4, vec![]);
        w2.active_tasks = 3;
        assert!((w2.utilization() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn worker_utilization_zero_capacity_returns_one() {
        let w = WorkerLoad::new(1, 0, vec![]);
        assert!((w.utilization() - 1.0).abs() < f64::EPSILON);
    }
}
