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

use std::sync::LazyLock;
use std::time::{Duration, Instant};

// ─── Token bucket ─────────────────────────────────────────────────────────────

/// A thread-safe token bucket rate limiter.
///
/// Tokens are added at a fixed rate up to a maximum capacity. Each action
/// consumes one token. When no tokens remain, the action is rejected with
/// a `retry_after` hint.
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

        // Refill tokens based on elapsed time.
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
            // Calculate how long until one token is available.
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
}
