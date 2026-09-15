//! Token-bucket rate limiter for agent actions and API calls.
//!
//! Prevents runaway agent loops from exhausting API credits, flooding the
//! file system, or spamming process spawns.
//!
//! # Predefined Tiers
//!
//! - [`check_api_call`] — per-provider rate limit (60 req/min default)
//! - [`check_tool_action`] — per-tool-category limit (30 actions/min)
//! - [`check_agent_action`] — global agent action budget (200 actions/5min)
//!
//! # Lock-Free Global Limiter
//!
//! [`try_acquire`] uses a lock-free atomic CAS token bucket for high-throughput
//! MCP tool call rate limiting. Configurable via `VELOCITY_RATE_LIMIT` and
//! `VELOCITY_RATE_BURST` environment variables (defaults: 20 tok/s, burst 100).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

// ─── Lock-free atomic rate limiter (upstream inheritance) ─────────────────────

/// Maximum tokens that can accumulate (burst capacity).
const DEFAULT_BURST: u32 = 100;

/// Tokens added per second.
const DEFAULT_RATE: u32 = 20;

/// Lock-free token bucket rate limiter using atomic CAS operations.
///
/// Significantly faster than the Mutex-based [`RateLimiter`] under contention
/// because it avoids thread parking/unparking. Uses scaled integer arithmetic
/// (×1000) for sub-token precision without floating point.
pub struct AtomicRateLimiter {
    /// Current token count (scaled by 1000 for sub-token precision).
    tokens_scaled: AtomicU64,
    /// Last refill timestamp (milliseconds since process start).
    last_refill_ms: AtomicU64,
    /// Burst capacity (scaled by 1000).
    burst_scaled: u64,
    /// Refill rate in tokens per second.
    rate: u32,
}

impl Default for AtomicRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl AtomicRateLimiter {
    /// Create a new rate limiter with default settings (20 tokens/sec, burst 100).
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_RATE, DEFAULT_BURST)
    }

    /// Create a rate limiter with custom limits.
    pub fn with_limits(rate_per_sec: u32, burst: u32) -> Self {
        let burst_scaled = burst as u64 * 1000;
        Self {
            tokens_scaled: AtomicU64::new(burst_scaled),
            last_refill_ms: AtomicU64::new(monotonic_ms()),
            burst_scaled,
            rate: rate_per_sec,
        }
    }

    /// Try to consume one token. Returns `true` if allowed, `false` if rate-limited.
    pub fn try_acquire(&self) -> bool {
        self.refill();
        // Atomic CAS loop to consume one token
        loop {
            let current = self.tokens_scaled.load(Ordering::Relaxed);
            if current < 1000 {
                return false;
            }
            let new_val = current - 1000;
            match self.tokens_scaled.compare_exchange_weak(
                current,
                new_val,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(_) => continue,
            }
        }
    }

    /// Refill tokens based on elapsed time.
    fn refill(&self) {
        let now = monotonic_ms();
        let last = self.last_refill_ms.load(Ordering::Relaxed);
        if now <= last {
            return;
        }
        let elapsed_ms = now - last;
        let new_tokens_scaled = elapsed_ms.saturating_mul(self.rate as u64);
        // Only one thread wins the refill race
        if self
            .last_refill_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            loop {
                let current = self.tokens_scaled.load(Ordering::Relaxed);
                let new_val = current.saturating_add(new_tokens_scaled).min(self.burst_scaled);
                match self.tokens_scaled.compare_exchange_weak(
                    current,
                    new_val,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(_) => continue,
                }
            }
        }
    }

    /// Get the current approximate token count (for diagnostics).
    pub fn available_tokens(&self) -> u32 {
        self.refill();
        (self.tokens_scaled.load(Ordering::Relaxed) / 1000) as u32
    }
}

/// Monotonic millisecond clock relative to process start.
fn monotonic_ms() -> u64 {
    static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);
    EPOCH.elapsed().as_millis() as u64
}

/// Global lock-free rate limiter for MCP tool calls.
/// Configure via `VELOCITY_RATE_LIMIT` / `VELOCITY_RATE_BURST` env vars.
static GLOBAL_ATOMIC_LIMITER: LazyLock<AtomicRateLimiter> = LazyLock::new(|| {
    match (
        std::env::var("VELOCITY_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok()),
        std::env::var("VELOCITY_RATE_BURST")
            .ok()
            .and_then(|v| v.parse::<u32>().ok()),
    ) {
        (Some(rate), Some(burst)) => AtomicRateLimiter::with_limits(rate, burst),
        (Some(rate), None) => AtomicRateLimiter::with_limits(rate, 100),
        (None, Some(burst)) => AtomicRateLimiter::with_limits(20, burst),
        (None, None) => AtomicRateLimiter::default(),
    }
});

/// Check if a tool call is allowed by the global lock-free rate limiter.
pub fn try_acquire() -> bool {
    GLOBAL_ATOMIC_LIMITER.try_acquire()
}

/// Get current available tokens from the global lock-free limiter.
pub fn available_tokens() -> u32 {
    GLOBAL_ATOMIC_LIMITER.available_tokens()
}

// ─── Mutex-based token bucket (per-tier limiters) ─────────────────────────────

/// A thread-safe token bucket rate limiter with `retry_after` semantics.
///
/// Used for the predefined per-tier limiters (API calls, tool actions, agent
/// actions) where callers need to know how long to wait on denial.
pub struct RateLimiter {
    inner: std::sync::Mutex<RateLimiterState>,
    capacity: u32,
    refill_interval: Duration,
}

struct RateLimiterState {
    tokens: f64,
    last_refill: Instant,
}

/// Result of a rate-limit check.
#[derive(Debug, Clone)]
pub enum RateLimitResult {
    /// Action is permitted; `remaining` whole tokens are left.
    Permitted { remaining: u32 },
    /// Action is denied; caller should wait `retry_after` before retrying.
    Denied { retry_after: Duration },
}

impl RateLimiter {
    /// Create a new limiter with `capacity` tokens, refilling one token
    /// every `refill_interval`.
    pub fn new(capacity: u32, refill_interval: Duration) -> Self {
        Self {
            inner: std::sync::Mutex::new(RateLimiterState {
                tokens: capacity as f64,
                last_refill: Instant::now(),
            }),
            capacity,
            refill_interval,
        }
    }

    /// Check whether an action is permitted. Consumes one token on success.
    pub fn check(&self) -> RateLimitResult {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let elapsed = state.last_refill.elapsed();
        let new_tokens = elapsed.as_secs_f64() / self.refill_interval.as_secs_f64();
        if new_tokens > 0.0 {
            state.tokens = (state.tokens + new_tokens).min(self.capacity as f64);
            state.last_refill = Instant::now();
        }
        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            RateLimitResult::Permitted {
                remaining: state.tokens as u32,
            }
        } else {
            let deficit = 1.0 - state.tokens;
            let retry_after =
                Duration::from_secs_f64(deficit * self.refill_interval.as_secs_f64());
            RateLimitResult::Denied {
                retry_after: retry_after.max(Duration::from_millis(50)),
            }
        }
    }

    /// Current token count (approximate, for diagnostics).
    pub fn available(&self) -> u32 {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let elapsed = state.last_refill.elapsed();
        let new_tokens = elapsed.as_secs_f64() / self.refill_interval.as_secs_f64();
        if new_tokens > 0.0 {
            state.tokens = (state.tokens + new_tokens).min(self.capacity as f64);
            state.last_refill = Instant::now();
        }
        state.tokens as u32
    }
}

// ─── Predefined limiters ──────────────────────────────────────────────────────

/// Per-provider API call limiter: 60 requests per minute.
static API_CALL_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(60, Duration::from_secs(1)));

/// Per-tool-category limiter: 30 actions per minute.
static TOOL_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(30, Duration::from_secs(2)));

/// Global agent action limiter: 200 actions per 5 minutes.
static AGENT_ACTION_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(200, Duration::from_millis(1500)));

/// Check if an API call is permitted.
pub fn check_api_call() -> RateLimitResult {
    API_CALL_LIMITER.check()
}

/// Check if a tool action is permitted.
pub fn check_tool_action() -> RateLimitResult {
    TOOL_LIMITER.check()
}

/// Check if a global agent action is permitted.
pub fn check_agent_action() -> RateLimitResult {
    AGENT_ACTION_LIMITER.check()
}

/// Approximate remaining API call tokens (for diagnostics).
pub fn api_call_tokens_remaining() -> u32 {
    API_CALL_LIMITER.available()
}

/// Approximate remaining tool action tokens (for diagnostics).
pub fn tool_tokens_remaining() -> u32 {
    TOOL_LIMITER.available()
}

/// Approximate remaining agent action tokens (for diagnostics).
pub fn agent_action_tokens_remaining() -> u32 {
    AGENT_ACTION_LIMITER.available()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_permits_up_to_capacity() {
        let limiter = RateLimiter::new(5, Duration::from_secs(10));
        for _ in 0..5 {
            assert!(matches!(limiter.check(), RateLimitResult::Permitted { .. }));
        }
        // 6th should be denied.
        assert!(matches!(limiter.check(), RateLimitResult::Denied { .. }));
    }

    #[test]
    fn rate_limiter_refills_over_time() {
        let limiter = RateLimiter::new(2, Duration::from_millis(10));
        assert!(matches!(limiter.check(), RateLimitResult::Permitted { .. }));
        assert!(matches!(limiter.check(), RateLimitResult::Permitted { .. }));
        assert!(matches!(limiter.check(), RateLimitResult::Denied { .. }));
        // Wait for refill.
        std::thread::sleep(Duration::from_millis(25));
        assert!(matches!(limiter.check(), RateLimitResult::Permitted { .. }));
    }

    #[test]
    fn rate_limiter_available_decrements() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert_eq!(limiter.available(), 3);
        let _ = limiter.check();
        assert_eq!(limiter.available(), 2);
    }

    // ── Lock-free atomic limiter tests ─────────────────────────────────────

    #[test]
    fn atomic_limiter_allows_burst() {
        let limiter = AtomicRateLimiter::with_limits(10, 5);
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }
        // 6th should be rejected
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn atomic_limiter_refills_over_time() {
        let limiter = AtomicRateLimiter::with_limits(1000, 10);
        // Consume all tokens
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
        // Wait for refill (at 1000 tokens/sec, 50ms should give ~50 tokens)
        std::thread::sleep(Duration::from_millis(50));
        assert!(limiter.try_acquire());
    }

    #[test]
    fn atomic_limiter_available_tokens() {
        let limiter = AtomicRateLimiter::with_limits(10, 10);
        let initial = limiter.available_tokens();
        assert!(initial <= 10);
        assert!(initial > 0);
    }

    #[test]
    fn global_atomic_limiter_works() {
        // Just verify it doesn't panic
        let _ = try_acquire();
        let _ = available_tokens();
    }
}
