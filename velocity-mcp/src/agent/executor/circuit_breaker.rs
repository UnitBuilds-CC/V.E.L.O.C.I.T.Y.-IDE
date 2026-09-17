//! Circuit breaker pattern for provider API calls.
//!
//! Protects the agent system from cascading failures when a provider is
//! experiencing issues. Each provider gets its own [`CircuitBreaker`] that
//! tracks failures and temporarily stops routing requests to providers that
//! are consistently failing, giving them time to recover.
//!
//! # State machine
//!
//! ```text
//!  Closed ──(failures >= threshold)──▶ Open
//!    ▲                                    │
//!    │                              (timeout elapsed)
//!    │                                    ▼
//!    │                                 HalfOpen
//!    │                                    │
//!    ├──────(successes >= threshold)──────┘
//!    │
//!    └────────(any failure in HalfOpen)───▶ Open
//! ```

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// CircuitState
// ---------------------------------------------------------------------------

/// The three possible states of a circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CircuitState {
    /// Normal operation — calls are allowed through.
    Closed,
    /// Provider is failing — calls are rejected until the timeout elapses.
    Open,
    /// Testing whether the provider has recovered — calls are allowed through
    /// but any failure immediately re-opens the circuit.
    HalfOpen,
}

impl fmt::Display for CircuitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "Closed"),
            CircuitState::Open => write!(f, "Open"),
            CircuitState::HalfOpen => write!(f, "HalfOpen"),
        }
    }
}

// ---------------------------------------------------------------------------
// CircuitBreakerError
// ---------------------------------------------------------------------------

/// Error returned when a circuit breaker rejects a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitBreakerError {
    /// The provider that was rejected.
    pub provider: String,
    /// The circuit state that caused the rejection.
    pub state: CircuitState,
    /// Human-readable explanation.
    pub message: String,
}

impl fmt::Display for CircuitBreakerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "circuit breaker open for provider '{}': {} (state={})",
            self.provider, self.message, self.state,
        )
    }
}

impl std::error::Error for CircuitBreakerError {}

impl CircuitBreakerError {
    /// Create a new error for a provider whose circuit is open.
    pub fn new(provider: impl Into<String>, state: CircuitState) -> Self {
        let provider = provider.into();
        let message = format!("calls are blocked while circuit is {}", state,);
        Self {
            provider,
            state,
            message,
        }
    }
}

// ---------------------------------------------------------------------------
// CircuitBreaker
// ---------------------------------------------------------------------------

/// Default number of consecutive failures before the circuit opens.
const DEFAULT_FAILURE_THRESHOLD: u32 = 5;

/// Default number of successes in HalfOpen required to close the circuit.
const DEFAULT_SUCCESS_THRESHOLD: u32 = 2;

/// Default time the circuit stays Open before transitioning to HalfOpen.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-provider circuit breaker.
///
/// Tracks consecutive failures and controls whether calls to the provider
/// are allowed, rejected, or used as recovery probes.
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Current state of the breaker.
    state: CircuitState,
    /// Consecutive failure counter (used in Closed and HalfOpen).
    failure_count: u32,
    /// Success counter accumulated while in HalfOpen.
    success_count: u32,
    /// How many failures trigger Closed → Open.
    failure_threshold: u32,
    /// How many successes in HalfOpen trigger HalfOpen → Closed.
    success_threshold: u32,
    /// How long the breaker stays Open before probing with HalfOpen.
    timeout: Duration,
    /// Timestamp of the most recent failure (if any).
    last_failure: Option<Instant>,
    /// Timestamp of the last state transition.
    last_state_change: Instant,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with explicit thresholds.
    pub fn new(failure_threshold: u32, success_threshold: u32, timeout: Duration) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            failure_threshold,
            success_threshold,
            timeout,
            last_failure: None,
            last_state_change: Instant::now(),
        }
    }

    /// Create a circuit breaker with the default thresholds.
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_FAILURE_THRESHOLD,
            DEFAULT_SUCCESS_THRESHOLD,
            DEFAULT_TIMEOUT,
        )
    }

    /// Returns `true` if a call to the provider is allowed right now.
    ///
    /// - **Closed** → always `true`.
    /// - **Open** → `true` only if the timeout has elapsed, in which case the
    ///   breaker transitions to **HalfOpen** first.
    /// - **HalfOpen** → always `true` (we are probing for recovery).
    pub fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if self.last_state_change.elapsed() >= self.timeout {
                    self.transition_to(CircuitState::HalfOpen);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful call.
    ///
    /// - **HalfOpen** → increments `success_count`; if the threshold is
    ///   reached the circuit closes.
    /// - **Closed** → resets the failure counter (the provider is healthy).
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.success_threshold {
                    self.transition_to(CircuitState::Closed);
                }
            }
            CircuitState::Closed => {
                // Provider is healthy — reset the failure counter.
                self.failure_count = 0;
            }
            CircuitState::Open => {
                // Should not normally happen (calls are rejected), but if a
                // success is reported while open we treat it like a probe
                // success and move to HalfOpen first.
                self.transition_to(CircuitState::HalfOpen);
                self.success_count = 1;
                if self.success_count >= self.success_threshold {
                    self.transition_to(CircuitState::Closed);
                }
            }
        }
    }

    /// Record a failed call.
    ///
    /// - **HalfOpen** → immediately re-opens the circuit.
    /// - **Closed** → increments `failure_count`; if the threshold is reached
    ///   the circuit opens.
    pub fn record_failure(&mut self) {
        self.last_failure = Some(Instant::now());
        match self.state {
            CircuitState::HalfOpen => {
                self.transition_to(CircuitState::Open);
            }
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.failure_threshold {
                    self.transition_to(CircuitState::Open);
                }
            }
            CircuitState::Open => {
                // Already open — just update the failure timestamp and
                // restart the timeout so we don't probe too early.
                self.last_state_change = Instant::now();
            }
        }
    }

    /// Current state of the breaker.
    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Reset the breaker to **Closed** with zeroed counters.
    pub fn reset(&mut self) {
        self.transition_to(CircuitState::Closed);
    }

    // -- internal helpers ---------------------------------------------------

    /// Transition to `new_state`, resetting counters as appropriate.
    fn transition_to(&mut self, new_state: CircuitState) {
        self.state = new_state;
        self.last_state_change = Instant::now();
        match new_state {
            CircuitState::Closed => {
                self.failure_count = 0;
                self.success_count = 0;
            }
            CircuitState::Open => {
                self.success_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count = 0;
                self.failure_count = 0;
            }
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ---------------------------------------------------------------------------
// CircuitBreakerRegistry
// ---------------------------------------------------------------------------

/// Manages a [`CircuitBreaker`] per provider.
///
/// The registry lazily creates breakers on first access so callers never need
/// to worry about initialisation order.
#[derive(Debug)]
pub struct CircuitBreakerRegistry {
    breakers: HashMap<String, CircuitBreaker>,
    /// Failure threshold applied to newly-created breakers.
    default_failure_threshold: u32,
    /// Success threshold applied to newly-created breakers.
    default_success_threshold: u32,
    /// Timeout applied to newly-created breakers.
    default_timeout: Duration,
}

impl CircuitBreakerRegistry {
    /// Create an empty registry with default thresholds.
    pub fn new() -> Self {
        Self {
            breakers: HashMap::new(),
            default_failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            default_success_threshold: DEFAULT_SUCCESS_THRESHOLD,
            default_timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create an empty registry with custom default thresholds.
    pub fn with_defaults(
        failure_threshold: u32,
        success_threshold: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            breakers: HashMap::new(),
            default_failure_threshold: failure_threshold,
            default_success_threshold: success_threshold,
            default_timeout: timeout,
        }
    }

    /// Return a mutable reference to the breaker for `provider`, creating one
    /// with the registry defaults if it does not yet exist.
    pub fn get_or_create(&mut self, provider: &str) -> &mut CircuitBreaker {
        if !self.breakers.contains_key(provider) {
            let breaker = CircuitBreaker::new(
                self.default_failure_threshold,
                self.default_success_threshold,
                self.default_timeout,
            );
            self.breakers.insert(provider.to_string(), breaker);
        }
        self.breakers.get_mut(provider).expect("just inserted")
    }

    /// Returns `true` if the provider's circuit allows execution right now.
    ///
    /// Unknown providers are always allowed (a breaker is created on the fly
    /// in the **Closed** state).
    pub fn can_execute(&mut self, provider: &str) -> bool {
        self.get_or_create(provider).can_execute()
    }

    /// Record a successful call for `provider`.
    pub fn record_success(&mut self, provider: &str) {
        self.get_or_create(provider).record_success();
    }

    /// Record a failed call for `provider`.
    pub fn record_failure(&mut self, provider: &str) {
        self.get_or_create(provider).record_failure();
    }

    /// Return the current state of `provider`'s breaker.
    ///
    /// Unknown providers are reported as **Closed**.
    pub fn get_state(&mut self, provider: &str) -> CircuitState {
        self.get_or_create(provider).state()
    }

    /// Snapshot of every provider's current state.
    pub fn all_states(&self) -> HashMap<String, CircuitState> {
        self.breakers
            .iter()
            .map(|(name, b)| (name.clone(), b.state()))
            .collect()
    }

    /// Manually reset a single provider's breaker to **Closed**.
    pub fn reset(&mut self, provider: &str) {
        if let Some(b) = self.breakers.get_mut(provider) {
            b.reset();
        }
    }

    /// Manually reset all breakers to **Closed**.
    pub fn reset_all(&mut self) {
        for b in self.breakers.values_mut() {
            b.reset();
        }
    }

    /// Number of providers currently tracked.
    pub fn len(&self) -> usize {
        self.breakers.len()
    }

    /// Returns `true` if no providers are tracked.
    pub fn is_empty(&self) -> bool {
        self.breakers.is_empty()
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    // -- helpers ------------------------------------------------------------

    /// Build a breaker with low thresholds and a short timeout for fast tests.
    fn fast_breaker() -> CircuitBreaker {
        CircuitBreaker::new(3, 2, Duration::from_millis(50))
    }

    // -- CircuitBreaker: basic state transitions ----------------------------

    #[test]
    fn test_new_breaker_starts_closed() {
        let b = CircuitBreaker::with_defaults();
        assert_eq!(b.state(), CircuitState::Closed);
    }

    #[test]
    fn test_closed_state_always_allows_execution() {
        let mut b = fast_breaker();
        assert!(b.can_execute());
        assert!(b.can_execute());
        assert_eq!(b.state(), CircuitState::Closed);
    }

    #[test]
    fn test_closed_to_open_after_threshold_failures() {
        let mut b = CircuitBreaker::new(3, 2, Duration::from_secs(60));
        b.record_failure();
        assert_eq!(b.state(), CircuitState::Closed);
        b.record_failure();
        assert_eq!(b.state(), CircuitState::Closed);
        b.record_failure(); // hits threshold
        assert_eq!(b.state(), CircuitState::Open);
    }

    #[test]
    fn test_open_state_rejects_execution() {
        let mut b = CircuitBreaker::new(1, 2, Duration::from_secs(60));
        b.record_failure();
        assert_eq!(b.state(), CircuitState::Open);
        assert!(!b.can_execute());
    }

    #[test]
    fn test_open_to_half_open_after_timeout() {
        let mut b = CircuitBreaker::new(1, 2, Duration::from_millis(30));
        b.record_failure();
        assert_eq!(b.state(), CircuitState::Open);
        assert!(!b.can_execute());

        thread::sleep(Duration::from_millis(40));

        // can_execute should detect the elapsed timeout and transition.
        assert!(b.can_execute());
        assert_eq!(b.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_half_open_allows_execution() {
        let mut b = CircuitBreaker::new(1, 1, Duration::from_millis(10));
        b.record_failure(); // → Open
        thread::sleep(Duration::from_millis(20));
        b.can_execute(); // → HalfOpen
        assert_eq!(b.state(), CircuitState::HalfOpen);
        assert!(b.can_execute());
    }

    #[test]
    fn test_half_open_to_closed_after_successes() {
        let mut b = CircuitBreaker::new(1, 2, Duration::from_millis(10));
        b.record_failure(); // → Open
        thread::sleep(Duration::from_millis(20));
        b.can_execute(); // → HalfOpen

        b.record_success(); // 1/2
        assert_eq!(b.state(), CircuitState::HalfOpen);
        b.record_success(); // 2/2 → Closed
        assert_eq!(b.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_to_open_on_any_failure() {
        let mut b = CircuitBreaker::new(1, 3, Duration::from_millis(10));
        b.record_failure(); // → Open
        thread::sleep(Duration::from_millis(20));
        b.can_execute(); // → HalfOpen
        assert_eq!(b.state(), CircuitState::HalfOpen);

        b.record_failure(); // any failure → Open
        assert_eq!(b.state(), CircuitState::Open);
    }

    #[test]
    fn test_success_in_closed_resets_failure_count() {
        let mut b = CircuitBreaker::new(3, 2, Duration::from_secs(60));
        b.record_failure(); // 1
        b.record_failure(); // 2
        b.record_success(); // resets to 0
        b.record_failure(); // 1 again
        assert_eq!(b.state(), CircuitState::Closed);
        b.record_failure(); // 2
        assert_eq!(b.state(), CircuitState::Closed);
    }

    #[test]
    fn test_manual_reset() {
        let mut b = CircuitBreaker::new(1, 2, Duration::from_secs(60));
        b.record_failure(); // → Open
        assert_eq!(b.state(), CircuitState::Open);

        b.reset();
        assert_eq!(b.state(), CircuitState::Closed);
        assert!(b.can_execute());
    }

    #[test]
    fn test_failure_in_half_open_resets_success_count() {
        let mut b = CircuitBreaker::new(1, 3, Duration::from_millis(10));
        b.record_failure(); // → Open
        thread::sleep(Duration::from_millis(20));
        b.can_execute(); // → HalfOpen

        b.record_success(); // 1/3
        b.record_success(); // 2/3
        b.record_failure(); // → Open (success_count reset)
        assert_eq!(b.state(), CircuitState::Open);

        thread::sleep(Duration::from_millis(20));
        b.can_execute(); // → HalfOpen (success_count reset to 0)
        b.record_success(); // 1/3 — confirms counter was reset
        b.record_success(); // 2/3
        assert_eq!(b.state(), CircuitState::HalfOpen); // still half-open
    }

    #[test]
    fn test_default_thresholds() {
        let b = CircuitBreaker::with_defaults();
        assert_eq!(b.failure_threshold, DEFAULT_FAILURE_THRESHOLD);
        assert_eq!(b.success_threshold, DEFAULT_SUCCESS_THRESHOLD);
        assert_eq!(b.timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn test_default_trait() {
        let b = CircuitBreaker::default();
        assert_eq!(b.state(), CircuitState::Closed);
        assert_eq!(b.failure_threshold, 5);
    }

    // -- CircuitState display -----------------------------------------------

    #[test]
    fn test_circuit_state_display() {
        assert_eq!(CircuitState::Closed.to_string(), "Closed");
        assert_eq!(CircuitState::Open.to_string(), "Open");
        assert_eq!(CircuitState::HalfOpen.to_string(), "HalfOpen");
    }

    // -- CircuitBreakerError ------------------------------------------------

    #[test]
    fn test_circuit_breaker_error_display() {
        let err = CircuitBreakerError::new("openrouter", CircuitState::Open);
        let msg = err.to_string();
        assert!(msg.contains("openrouter"));
        assert!(msg.contains("Open"));
    }

    #[test]
    fn test_circuit_breaker_error_fields() {
        let err = CircuitBreakerError {
            provider: "anthropic".into(),
            state: CircuitState::Open,
            message: "too many failures".into(),
        };
        assert_eq!(err.provider, "anthropic");
        assert_eq!(err.state, CircuitState::Open);
        assert_eq!(err.message, "too many failures");
    }

    // -- Registry -----------------------------------------------------------

    #[test]
    fn test_registry_creates_breakers_on_demand() {
        let mut reg = CircuitBreakerRegistry::new();
        assert!(reg.is_empty());

        let _ = reg.get_or_create("openrouter");
        assert_eq!(reg.len(), 1);

        let _ = reg.get_or_create("anthropic");
        assert_eq!(reg.len(), 2);

        // Same provider should not create a new entry.
        let _ = reg.get_or_create("openrouter");
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn test_registry_can_execute_returns_false_for_open() {
        let mut reg = CircuitBreakerRegistry::with_defaults(1, 2, Duration::from_secs(60));
        // First call creates the breaker (Closed) and allows execution.
        assert!(reg.can_execute("openrouter"));
        // Record a failure to trip the breaker.
        reg.record_failure("openrouter");
        // Now it should be Open and reject.
        assert!(!reg.can_execute("openrouter"));
    }

    #[test]
    fn test_registry_record_success_and_failure() {
        let mut reg = CircuitBreakerRegistry::with_defaults(2, 1, Duration::from_secs(60));
        reg.record_failure("p1");
        assert_eq!(reg.get_state("p1"), CircuitState::Closed);
        reg.record_failure("p1");
        assert_eq!(reg.get_state("p1"), CircuitState::Open);

        reg.record_success("p2"); // creates p2 in Closed state
        assert_eq!(reg.get_state("p2"), CircuitState::Closed);
    }

    #[test]
    fn test_registry_all_states() {
        let mut reg = CircuitBreakerRegistry::with_defaults(1, 2, Duration::from_secs(60));
        reg.record_failure("a"); // → Open
        let _ = reg.get_or_create("b"); // → Closed

        let states = reg.all_states();
        assert_eq!(states.get("a"), Some(&CircuitState::Open));
        assert_eq!(states.get("b"), Some(&CircuitState::Closed));
    }

    #[test]
    fn test_registry_reset_single() {
        let mut reg = CircuitBreakerRegistry::with_defaults(1, 2, Duration::from_secs(60));
        reg.record_failure("x");
        assert_eq!(reg.get_state("x"), CircuitState::Open);

        reg.reset("x");
        assert_eq!(reg.get_state("x"), CircuitState::Closed);
    }

    #[test]
    fn test_registry_reset_all() {
        let mut reg = CircuitBreakerRegistry::with_defaults(1, 2, Duration::from_secs(60));
        reg.record_failure("a");
        reg.record_failure("b");
        assert_eq!(reg.get_state("a"), CircuitState::Open);
        assert_eq!(reg.get_state("b"), CircuitState::Open);

        reg.reset_all();
        assert_eq!(reg.get_state("a"), CircuitState::Closed);
        assert_eq!(reg.get_state("b"), CircuitState::Closed);
    }

    #[test]
    fn test_multiple_providers_independent() {
        let mut reg = CircuitBreakerRegistry::with_defaults(2, 1, Duration::from_secs(60));

        // Trip breaker for provider A.
        reg.record_failure("a");
        reg.record_failure("a");
        assert_eq!(reg.get_state("a"), CircuitState::Open);

        // Provider B should be unaffected.
        assert_eq!(reg.get_state("b"), CircuitState::Closed);
        assert!(reg.can_execute("b"));

        // Provider C should also be unaffected.
        assert!(reg.can_execute("c"));
        assert_eq!(reg.get_state("c"), CircuitState::Closed);
    }

    #[test]
    fn test_registry_unknown_provider_is_closed() {
        let mut reg = CircuitBreakerRegistry::new();
        // Querying a non-existent provider creates it in Closed state.
        assert_eq!(reg.get_state("unknown"), CircuitState::Closed);
    }

    #[test]
    fn test_registry_default_trait() {
        let reg = CircuitBreakerRegistry::default();
        assert!(reg.is_empty());
    }

    #[test]
    fn test_full_lifecycle() {
        // Closed → Open → HalfOpen → Closed
        let mut b = CircuitBreaker::new(2, 2, Duration::from_millis(30));

        // Closed
        assert_eq!(b.state(), CircuitState::Closed);
        assert!(b.can_execute());

        // Two failures → Open
        b.record_failure();
        b.record_failure();
        assert_eq!(b.state(), CircuitState::Open);
        assert!(!b.can_execute());

        // Wait for timeout → HalfOpen
        thread::sleep(Duration::from_millis(40));
        assert!(b.can_execute());
        assert_eq!(b.state(), CircuitState::HalfOpen);

        // Two successes → Closed
        b.record_success();
        b.record_success();
        assert_eq!(b.state(), CircuitState::Closed);
        assert!(b.can_execute());
    }

    #[test]
    fn test_full_lifecycle_with_failure_in_half_open() {
        // Closed → Open → HalfOpen → Open (failure during probe)
        let mut b = CircuitBreaker::new(1, 2, Duration::from_millis(20));

        b.record_failure(); // → Open
        assert_eq!(b.state(), CircuitState::Open);

        thread::sleep(Duration::from_millis(30));
        b.can_execute(); // → HalfOpen
        assert_eq!(b.state(), CircuitState::HalfOpen);

        b.record_failure(); // → Open again
        assert_eq!(b.state(), CircuitState::Open);
        assert!(!b.can_execute());
    }
}
