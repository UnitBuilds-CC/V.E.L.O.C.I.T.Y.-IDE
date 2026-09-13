//! Request pipelining and connection pooling infrastructure for faster
//! end-to-end agent performance.
//!
//! [`RequestPipeline`] batches and prioritises API requests so that
//! interactive (user-facing) work is dispatched before background or
//! speculative prefetch requests.  [`ConnectionPool`] tracks reusable
//! provider connections so that cold-start overhead is amortised across
//! many requests.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/// Priority levels for pipeline requests.
///
/// Lower numeric value == higher priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelinePriority {
    /// The user is waiting for this result (latency-sensitive).
    Interactive = 0,
    /// An async / background task (throughput-oriented).
    Background = 1,
    /// Speculative prefetch that can be dropped under load.
    Prefetch = 2,
}

impl PipelinePriority {
    /// Numeric rank used for sorting (lower == more important).
    #[inline]
    pub fn rank(self) -> u8 {
        self as u8
    }
}

impl PartialOrd for PipelinePriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PipelinePriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

// ---------------------------------------------------------------------------
// PipelineRequest
// ---------------------------------------------------------------------------

/// A single request waiting to be dispatched through the pipeline.
#[derive(Debug, Clone)]
pub struct PipelineRequest {
    /// Unique request identifier.
    pub id: u64,
    /// Target provider name (e.g. `"openrouter"`, `"azure_openai"`).
    pub provider: String,
    /// Model identifier (e.g. `"gpt-4o"`, `"claude-3.5-sonnet"`).
    pub model: String,
    /// Dispatch priority.
    pub priority: PipelinePriority,
    /// Wall-clock time when the request was enqueued.
    pub enqueued_at: Instant,
    /// Estimated token count (used for capacity planning).
    pub estimated_tokens: usize,
    /// Hash of the full request payload — used for deduplication.
    pub payload_hash: u64,
}

// ---------------------------------------------------------------------------
// PipelineResult
// ---------------------------------------------------------------------------

/// Outcome of a single pipeline request.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub request_id: u64,
    pub success: bool,
    pub queue_time_ms: u64,
    pub execution_time_ms: u64,
    pub total_time_ms: u64,
}

// ---------------------------------------------------------------------------
// PipelineStats
// ---------------------------------------------------------------------------

/// Point-in-time statistics for the pipeline.
#[derive(Debug, Clone)]
pub struct PipelineStats {
    pub pending: usize,
    pub active: usize,
    pub total_dispatched: u64,
    pub total_completed: u64,
    /// Exponential-moving-average queue time in milliseconds.
    pub avg_queue_time_ms: f64,
    /// Completed requests per second (based on elapsed wall time since first
    /// completion).
    pub throughput_per_sec: f64,
}

// ---------------------------------------------------------------------------
// RequestPipeline
// ---------------------------------------------------------------------------

/// Default maximum number of concurrent in-flight requests.
const DEFAULT_MAX_CONCURRENT: usize = 4;

/// EMA smoothing factor for average queue-time tracking.
const EMA_ALPHA: f64 = 0.25;

/// Batches and prioritises outgoing API requests.
pub struct RequestPipeline {
    /// FIFO queue of pending requests (sorted by priority on dequeue).
    pending: VecDeque<PipelineRequest>,
    /// Maximum number of requests that may be in-flight simultaneously.
    max_concurrent: usize,
    /// Number of requests currently in-flight.
    active_count: usize,
    /// Total requests that have been dequeued (dispatched).
    total_dispatched: u64,
    /// Total requests that have been marked complete.
    total_completed: u64,
    /// EMA of queue time in milliseconds.
    avg_queue_time_ms: f64,
    /// Instant of the very first completion — used for throughput calc.
    first_completed_at: Option<Instant>,
    /// Set of in-flight request ids (so `mark_complete` can validate).
    in_flight: std::collections::HashSet<u64>,
}

impl RequestPipeline {
    /// Create a new pipeline with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_concurrent: if max_concurrent == 0 {
                DEFAULT_MAX_CONCURRENT
            } else {
                max_concurrent
            },
            active_count: 0,
            total_dispatched: 0,
            total_completed: 0,
            avg_queue_time_ms: 0.0,
            first_completed_at: None,
            in_flight: std::collections::HashSet::new(),
        }
    }

    /// Enqueue a request.  Returns `false` if the pipeline is at capacity
    /// (pending + active >= max_concurrent * 4 — a soft back-pressure limit).
    pub fn enqueue(&mut self, req: PipelineRequest) -> bool {
        let capacity = self.max_concurrent.saturating_mul(4);
        if self.pending.len() + self.active_count >= capacity {
            return false;
        }
        self.pending.push_back(req);
        true
    }

    /// Dequeue up to `count` requests, sorted by priority (highest first).
    ///
    /// The returned requests are considered "in-flight" and count against
    /// `max_concurrent`.  Leftover requests that were not dispatched remain
    /// in the pending queue.
    pub fn dequeue_batch(&mut self, count: usize) -> Vec<PipelineRequest> {
        let available = self
            .max_concurrent
            .saturating_sub(self.active_count)
            .min(count);

        if available == 0 || self.pending.is_empty() {
            return Vec::new();
        }

        // Drain everything, sort by priority, split into dispatched / leftover.
        let mut items: Vec<PipelineRequest> = self.pending.drain(..).collect();
        items.sort_by_key(|r| r.priority);

        let take = available.min(items.len());

        // Split: first `take` items are dispatched, rest go back to pending.
        let leftover: Vec<PipelineRequest> = items.split_off(take);

        let mut result = Vec::with_capacity(take);
        for req in items {
            self.in_flight.insert(req.id);
            self.active_count += 1;
            self.total_dispatched += 1;
            result.push(req);
        }

        // Put leftovers back into the pending queue.
        for req in leftover {
            self.pending.push_back(req);
        }

        result
    }

    /// Mark a request as complete and update running statistics.
    pub fn mark_complete(&mut self, id: u64, result: &PipelineResult) {
        if self.in_flight.remove(&id) {
            self.active_count = self.active_count.saturating_sub(1);
            self.total_completed += 1;

            // Update EMA of queue time.
            if self.total_completed == 1 {
                self.avg_queue_time_ms = result.queue_time_ms as f64;
                self.first_completed_at = Some(Instant::now());
            } else {
                self.avg_queue_time_ms =
                    EMA_ALPHA * result.queue_time_ms as f64 + (1.0 - EMA_ALPHA) * self.avg_queue_time_ms;
            }
        }
    }

    /// Remove duplicate requests that share the same `payload_hash`.
    ///
    /// Keeps the first (highest-priority / earliest-enqueued) occurrence.
    pub fn dedup_by_payload(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut deduped = VecDeque::with_capacity(self.pending.len());
        for req in self.pending.drain(..) {
            if seen.insert(req.payload_hash) {
                deduped.push_back(req);
            }
        }
        self.pending = deduped;
    }

    /// Return a snapshot of current pipeline statistics.
    pub fn pipeline_stats(&self) -> PipelineStats {
        let throughput = if let Some(first) = self.first_completed_at {
            let elapsed = first.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                self.total_completed as f64 / elapsed
            } else {
                0.0
            }
        } else {
            0.0
        };

        PipelineStats {
            pending: self.pending.len(),
            active: self.active_count,
            total_dispatched: self.total_dispatched,
            total_completed: self.total_completed,
            avg_queue_time_ms: self.avg_queue_time_ms,
            throughput_per_sec: throughput,
        }
    }

    // -- Accessors ----------------------------------------------------------

    /// Number of pending (queued) requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Number of in-flight requests.
    pub fn active_count(&self) -> usize {
        self.active_count
    }
}

// ---------------------------------------------------------------------------
// ConnectionPool
// ---------------------------------------------------------------------------

/// Per-provider connection pool entry.
#[derive(Debug, Clone)]
pub struct PoolEntry {
    pub provider: String,
    pub available: usize,
    pub in_use: usize,
    pub total_created: u64,
    pub total_reused: u64,
}

/// Point-in-time stats for a single provider pool.
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub provider: String,
    pub available: usize,
    pub in_use: usize,
    pub total_created: u64,
    pub total_reused: u64,
    pub reuse_rate: f64,
}

/// Default maximum connections per provider.
const DEFAULT_MAX_PER_PROVIDER: usize = 8;

/// A simple connection pool that tracks reusable provider connections.
pub struct ConnectionPool {
    pools: HashMap<String, PoolEntry>,
    max_per_provider: usize,
}

impl ConnectionPool {
    /// Create a new connection pool.
    pub fn new(max_per_provider: usize) -> Self {
        Self {
            pools: HashMap::new(),
            max_per_provider: if max_per_provider == 0 {
                DEFAULT_MAX_PER_PROVIDER
            } else {
                max_per_provider
            },
        }
    }

    /// Try to acquire a connection for `provider`.
    ///
    /// Returns `true` if a connection was obtained (either reused or newly
    /// created).  Returns `false` if the pool is at capacity.
    pub fn acquire(&mut self, provider: &str) -> bool {
        let max = self.max_per_provider;
        let entry = self
            .pools
            .entry(provider.to_string())
            .or_insert_with(|| PoolEntry {
                provider: provider.to_string(),
                available: 0,
                in_use: 0,
                total_created: 0,
                total_reused: 0,
            });

        let total = entry.available + entry.in_use;
        if total >= max {
            return false;
        }

        if entry.available > 0 {
            entry.available -= 1;
            entry.in_use += 1;
            entry.total_reused += 1;
        } else {
            entry.in_use += 1;
            entry.total_created += 1;
        }
        true
    }

    /// Release a connection back to the pool for `provider`.
    pub fn release(&mut self, provider: &str) {
        if let Some(entry) = self.pools.get_mut(provider) {
            if entry.in_use > 0 {
                entry.in_use -= 1;
                entry.available += 1;
            }
        }
    }

    /// Get stats for a specific provider pool.
    pub fn pool_stats(&self, provider: &str) -> Option<PoolStats> {
        self.pools.get(provider).map(|e| {
            let total_reqs = e.total_created + e.total_reused;
            let reuse_rate = if total_reqs > 0 {
                e.total_reused as f64 / total_reqs as f64
            } else {
                0.0
            };
            PoolStats {
                provider: e.provider.clone(),
                available: e.available,
                in_use: e.in_use,
                total_created: e.total_created,
                total_reused: e.total_reused,
                reuse_rate,
            }
        })
    }

    /// Overall reuse rate across all providers.
    pub fn total_reuse_rate(&self) -> f64 {
        let mut total_created: u64 = 0;
        let mut total_reused: u64 = 0;
        for entry in self.pools.values() {
            total_created += entry.total_created;
            total_reused += entry.total_reused;
        }
        let total = total_created + total_reused;
        if total > 0 {
            total_reused as f64 / total as f64
        } else {
            0.0
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_req(id: u64, priority: PipelinePriority, hash: u64) -> PipelineRequest {
        PipelineRequest {
            id,
            provider: "openrouter".to_string(),
            model: "gpt-4o".to_string(),
            priority,
            enqueued_at: Instant::now(),
            estimated_tokens: 512,
            payload_hash: hash,
        }
    }

    // -- Pipeline tests -----------------------------------------------------

    #[test]
    fn test_pipeline_new_defaults() {
        let p = RequestPipeline::new(0);
        assert_eq!(p.max_concurrent, DEFAULT_MAX_CONCURRENT);
        assert_eq!(p.pending_count(), 0);
        assert_eq!(p.active_count(), 0);
    }

    #[test]
    fn test_pipeline_enqueue_basic() {
        let mut p = RequestPipeline::new(4);
        assert!(p.enqueue(make_req(1, PipelinePriority::Interactive, 100)));
        assert_eq!(p.pending_count(), 1);
    }

    #[test]
    fn test_pipeline_enqueue_at_capacity() {
        let mut p = RequestPipeline::new(1);
        // capacity = 1 * 4 = 4
        for i in 0..4 {
            assert!(p.enqueue(make_req(i, PipelinePriority::Background, i)));
        }
        // 5th should fail
        assert!(!p.enqueue(make_req(99, PipelinePriority::Background, 99)));
    }

    #[test]
    fn test_pipeline_dequeue_respects_concurrency() {
        let mut p = RequestPipeline::new(2);
        for i in 0..5 {
            p.enqueue(make_req(i, PipelinePriority::Background, i));
        }
        let batch = p.dequeue_batch(10);
        // Only 2 should be dispatched (max_concurrent = 2)
        assert_eq!(batch.len(), 2);
        assert_eq!(p.active_count(), 2);
    }

    #[test]
    fn test_pipeline_dequeue_priority_ordering() {
        let mut p = RequestPipeline::new(10);
        p.enqueue(make_req(1, PipelinePriority::Prefetch, 1));
        p.enqueue(make_req(2, PipelinePriority::Interactive, 2));
        p.enqueue(make_req(3, PipelinePriority::Background, 3));
        p.enqueue(make_req(4, PipelinePriority::Interactive, 4));

        let batch = p.dequeue_batch(4);
        assert_eq!(batch.len(), 4);
        // First two should be Interactive (rank 0)
        assert_eq!(batch[0].priority, PipelinePriority::Interactive);
        assert_eq!(batch[1].priority, PipelinePriority::Interactive);
        assert_eq!(batch[2].priority, PipelinePriority::Background);
        assert_eq!(batch[3].priority, PipelinePriority::Prefetch);
    }

    #[test]
    fn test_pipeline_dequeue_empty() {
        let mut p = RequestPipeline::new(4);
        let batch = p.dequeue_batch(5);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_pipeline_mark_complete() {
        let mut p = RequestPipeline::new(4);
        p.enqueue(make_req(1, PipelinePriority::Interactive, 10));
        let batch = p.dequeue_batch(1);
        assert_eq!(batch.len(), 1);
        assert_eq!(p.active_count(), 1);

        let result = PipelineResult {
            request_id: 1,
            success: true,
            queue_time_ms: 50,
            execution_time_ms: 200,
            total_time_ms: 250,
        };
        p.mark_complete(1, &result);
        assert_eq!(p.active_count(), 0);
        assert_eq!(p.pipeline_stats().total_completed, 1);
    }

    #[test]
    fn test_pipeline_mark_complete_unknown_id() {
        let mut p = RequestPipeline::new(4);
        let result = PipelineResult {
            request_id: 999,
            success: true,
            queue_time_ms: 10,
            execution_time_ms: 100,
            total_time_ms: 110,
        };
        // Should be a no-op
        p.mark_complete(999, &result);
        assert_eq!(p.pipeline_stats().total_completed, 0);
    }

    #[test]
    fn test_pipeline_dedup() {
        let mut p = RequestPipeline::new(10);
        p.enqueue(make_req(1, PipelinePriority::Background, 42));
        p.enqueue(make_req(2, PipelinePriority::Background, 42)); // dup
        p.enqueue(make_req(3, PipelinePriority::Background, 99));
        p.enqueue(make_req(4, PipelinePriority::Background, 42)); // dup

        p.dedup_by_payload();
        assert_eq!(p.pending_count(), 2); // hashes 42 and 99
    }

    #[test]
    fn test_pipeline_dedup_empty() {
        let mut p = RequestPipeline::new(4);
        p.dedup_by_payload();
        assert_eq!(p.pending_count(), 0);
    }

    #[test]
    fn test_pipeline_stats_throughput_zero_before_completions() {
        let p = RequestPipeline::new(4);
        let stats = p.pipeline_stats();
        assert_eq!(stats.throughput_per_sec, 0.0);
        assert_eq!(stats.total_completed, 0);
    }

    #[test]
    fn test_pipeline_ema_queue_time() {
        let mut p = RequestPipeline::new(4);
        p.enqueue(make_req(1, PipelinePriority::Interactive, 1));
        let batch = p.dequeue_batch(1);
        assert_eq!(batch.len(), 1);

        let r1 = PipelineResult {
            request_id: 1,
            success: true,
            queue_time_ms: 100,
            execution_time_ms: 50,
            total_time_ms: 150,
        };
        p.mark_complete(1, &r1);
        let stats = p.pipeline_stats();
        // First completion: EMA = 100.0
        assert!((stats.avg_queue_time_ms - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_pipeline_total_dispatched_counter() {
        let mut p = RequestPipeline::new(10);
        for i in 0..5 {
            p.enqueue(make_req(i, PipelinePriority::Background, i));
        }
        let _ = p.dequeue_batch(3);
        assert_eq!(p.pipeline_stats().total_dispatched, 3);
        let _ = p.dequeue_batch(5);
        assert_eq!(p.pipeline_stats().total_dispatched, 5);
    }

    // -- Connection pool tests ----------------------------------------------

    #[test]
    fn test_pool_new() {
        let pool = ConnectionPool::new(0);
        assert_eq!(pool.max_per_provider, DEFAULT_MAX_PER_PROVIDER);
    }

    #[test]
    fn test_pool_acquire_creates_connection() {
        let mut pool = ConnectionPool::new(4);
        assert!(pool.acquire("openrouter"));
        let stats = pool.pool_stats("openrouter").unwrap();
        assert_eq!(stats.total_created, 1);
        assert_eq!(stats.in_use, 1);
        assert_eq!(stats.available, 0);
    }

    #[test]
    fn test_pool_acquire_reuses_connection() {
        let mut pool = ConnectionPool::new(4);
        assert!(pool.acquire("openrouter"));
        pool.release("openrouter");
        assert!(pool.acquire("openrouter")); // should reuse
        let stats = pool.pool_stats("openrouter").unwrap();
        assert_eq!(stats.total_reused, 1);
        assert_eq!(stats.total_created, 1);
    }

    #[test]
    fn test_pool_acquire_at_capacity() {
        let mut pool = ConnectionPool::new(2);
        assert!(pool.acquire("openrouter"));
        assert!(pool.acquire("openrouter"));
        assert!(!pool.acquire("openrouter")); // at capacity
    }

    #[test]
    fn test_pool_release_unknown_provider() {
        let mut pool = ConnectionPool::new(4);
        // Should not panic
        pool.release("nonexistent");
    }

    #[test]
    fn test_pool_release_decrements_in_use() {
        let mut pool = ConnectionPool::new(4);
        pool.acquire("azure");
        pool.acquire("azure");
        pool.release("azure");
        let stats = pool.pool_stats("azure").unwrap();
        assert_eq!(stats.in_use, 1);
        assert_eq!(stats.available, 1);
    }

    #[test]
    fn test_pool_stats_none_for_unknown() {
        let pool = ConnectionPool::new(4);
        assert!(pool.pool_stats("unknown").is_none());
    }

    #[test]
    fn test_pool_total_reuse_rate_empty() {
        let pool = ConnectionPool::new(4);
        assert_eq!(pool.total_reuse_rate(), 0.0);
    }

    #[test]
    fn test_pool_total_reuse_rate_mixed() {
        let mut pool = ConnectionPool::new(8);
        // Provider A: 2 created, 1 reused
        pool.acquire("a"); // created
        pool.acquire("a"); // created
        pool.release("a");
        pool.acquire("a"); // reused

        // Provider B: 1 created, 0 reused
        pool.acquire("b"); // created

        // total_created = 3, total_reused = 1
        // reuse_rate = 1 / (3 + 1) = 0.25
        let rate = pool.total_reuse_rate();
        assert!((rate - 0.25).abs() < 0.01, "expected ~0.25, got {rate}");
    }

    #[test]
    fn test_pool_multiple_providers_independent() {
        let mut pool = ConnectionPool::new(2);
        assert!(pool.acquire("a"));
        assert!(pool.acquire("a"));
        assert!(!pool.acquire("a")); // full
        assert!(pool.acquire("b")); // different provider, should work
    }

    #[test]
    fn test_pool_reuse_rate_per_provider() {
        let mut pool = ConnectionPool::new(4);
        pool.acquire("x");
        pool.release("x");
        pool.acquire("x"); // reused
        pool.acquire("x"); // new (capacity allows 4)
        let stats = pool.pool_stats("x").unwrap();
        // created=2, reused=1 → reuse_rate = 1/3 ≈ 0.333
        assert!((stats.reuse_rate - 1.0 / 3.0).abs() < 0.01);
    }

    // -- Priority ordering tests --------------------------------------------

    #[test]
    fn test_priority_ord() {
        assert!(PipelinePriority::Interactive < PipelinePriority::Background);
        assert!(PipelinePriority::Background < PipelinePriority::Prefetch);
        assert!(PipelinePriority::Interactive < PipelinePriority::Prefetch);
    }

    #[test]
    fn test_pipeline_dequeue_batch_zero_count() {
        let mut p = RequestPipeline::new(4);
        p.enqueue(make_req(1, PipelinePriority::Interactive, 1));
        let batch = p.dequeue_batch(0);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_pipeline_dequeue_frees_slots_after_complete() {
        let mut p = RequestPipeline::new(1);
        p.enqueue(make_req(1, PipelinePriority::Interactive, 1));
        p.enqueue(make_req(2, PipelinePriority::Interactive, 2));

        let batch = p.dequeue_batch(1);
        assert_eq!(batch.len(), 1);

        // Can't dequeue more — 1 slot in use
        let batch2 = p.dequeue_batch(1);
        assert!(batch2.is_empty());

        // Complete the first
        let result = PipelineResult {
            request_id: 1,
            success: true,
            queue_time_ms: 10,
            execution_time_ms: 50,
            total_time_ms: 60,
        };
        p.mark_complete(1, &result);

        // Now we can dequeue the second
        let batch3 = p.dequeue_batch(1);
        assert_eq!(batch3.len(), 1);
        assert_eq!(batch3[0].id, 2);
    }
}
