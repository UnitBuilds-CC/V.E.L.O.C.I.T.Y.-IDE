#![allow(dead_code, unused_imports, unused_variables)]
//! Error recovery and adaptive waits for Windows desktop automation.
//!
//! Provides retry logic with exponential backoff, adaptive wait strategies
//! that learn from element availability patterns, circuit-breaker protection
//! for consistently failing operations, and recovery script execution.

use std::time::{Duration, Instant};

// ─── Retry Policy ────────────────────────────────────────────────────────────

/// Retry strategy configuration.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of attempts (including the first).
    pub max_attempts: u32,
    /// Initial delay before first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Backoff multiplier (e.g., 2.0 for exponential doubling).
    pub backoff_multiplier: f64,
    /// Jitter factor (0.0 - 1.0) to randomize delays.
    pub jitter_factor: f64,
    /// Which errors are retryable.
    pub retryable_errors: Vec<RetryableError>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
            retryable_errors: vec![
                RetryableError::ElementNotFound,
                RetryableError::ElementNotReady,
                RetryableError::WindowNotFocused,
                RetryableError::Timeout,
            ],
        }
    }
}

/// Error categories that can be retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryableError {
    /// Target element was not found in the tree.
    ElementNotFound,
    /// Element exists but is not interactable (disabled, offscreen).
    ElementNotReady,
    /// Target window lost focus during operation.
    WindowNotFocused,
    /// Operation timed out but may succeed on retry.
    Timeout,
    /// Transient COM/IPC failure.
    TransientComError,
    /// Process is busy (high CPU, loading).
    ProcessBusy,
}

/// Result of a retry-wrapped operation.
#[derive(Debug, Clone)]
pub struct RetryResult {
    /// Whether the operation ultimately succeeded.
    pub succeeded: bool,
    /// Number of attempts made.
    pub attempts: u32,
    /// Total elapsed time across all attempts.
    pub total_elapsed: Duration,
    /// Error from the last attempt (if failed).
    pub last_error: Option<String>,
    /// Delays applied between attempts.
    pub delays: Vec<Duration>,
}

impl RetryPolicy {
    /// Compute the delay for the nth retry (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.initial_delay.as_millis() as f64
            * self.backoff_multiplier.powi(attempt as i32);
        let capped = base.min(self.max_delay.as_millis() as f64);
        // Simplified jitter: add random-ish perturbation based on attempt
        let jitter = capped * self.jitter_factor * ((attempt as f64 * 7.3).sin().abs());
        Duration::from_millis((capped + jitter) as u64)
    }

    /// Check if an error message indicates a retryable condition.
    pub fn is_retryable(&self, error_msg: &str) -> bool {
        let lower = error_msg.to_ascii_lowercase();
        self.retryable_errors.iter().any(|re| match re {
            RetryableError::ElementNotFound => {
                lower.contains("not found") || lower.contains("no matching")
            }
            RetryableError::ElementNotReady => {
                lower.contains("not ready") || lower.contains("disabled") || lower.contains("offscreen")
            }
            RetryableError::WindowNotFocused => {
                lower.contains("not focused") || lower.contains("lost focus")
            }
            RetryableError::Timeout => {
                lower.contains("timeout") || lower.contains("timed out")
            }
            RetryableError::TransientComError => {
                lower.contains("com") || lower.contains("rpc") || lower.contains("0x8")
            }
            RetryableError::ProcessBusy => {
                lower.contains("busy") || lower.contains("not responding")
            }
        })
    }
}

// ─── Adaptive Wait ───────────────────────────────────────────────────────────

/// Adaptive wait that adjusts timing based on observed patterns.
#[derive(Debug, Clone)]
pub struct AdaptiveWait {
    /// Minimum wait time.
    pub min_wait: Duration,
    /// Maximum wait time.
    pub max_wait: Duration,
    /// History of observed ready-times (for learning).
    pub observed_ready_times: Vec<Duration>,
    /// Estimated optimal wait based on history.
    pub estimated_ready_time: Duration,
    /// How many observations to keep.
    pub history_size: usize,
}

impl Default for AdaptiveWait {
    fn default() -> Self {
        Self {
            min_wait: Duration::from_millis(50),
            max_wait: Duration::from_secs(10),
            observed_ready_times: Vec::new(),
            estimated_ready_time: Duration::from_millis(500),
            history_size: 20,
        }
    }
}

impl AdaptiveWait {
    /// Record an observation of how long an element took to become ready.
    pub fn record_observation(&mut self, ready_time: Duration) {
        self.observed_ready_times.push(ready_time);
        if self.observed_ready_times.len() > self.history_size {
            self.observed_ready_times.remove(0);
        }
        self.recompute_estimate();
    }

    /// Recompute the estimated ready time from history.
    fn recompute_estimate(&mut self) {
        if self.observed_ready_times.is_empty() {
            return;
        }
        // Use P90 (90th percentile) as the estimate for reliability
        let mut sorted: Vec<u64> = self
            .observed_ready_times
            .iter()
            .map(|d| d.as_millis() as u64)
            .collect();
        sorted.sort_unstable();
        let p90_index = ((sorted.len() as f64) * 0.9).ceil() as usize - 1;
        let p90 = sorted[p90_index.min(sorted.len() - 1)];
        self.estimated_ready_time = Duration::from_millis(p90)
            .max(self.min_wait)
            .min(self.max_wait);
    }

    /// Get the recommended wait time based on current observations.
    pub fn recommended_wait(&self) -> Duration {
        self.estimated_ready_time
    }

    /// Get a poll interval suggestion (1/10th of estimated ready time, min 50ms).
    pub fn recommended_poll_interval(&self) -> Duration {
        let interval = self.estimated_ready_time.as_millis() / 10;
        Duration::from_millis(interval.max(50) as u64)
    }
}

// ─── Circuit Breaker ─────────────────────────────────────────────────────────

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests pass through.
    Closed,
    /// Failures exceeded threshold - requests are rejected immediately.
    Open,
    /// Testing if the service has recovered.
    HalfOpen,
}

/// Circuit breaker for protecting against consistently failing operations.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Current state.
    pub state: CircuitState,
    /// Consecutive failures count.
    pub failure_count: u32,
    /// Threshold before opening the circuit.
    pub failure_threshold: u32,
    /// How long to stay open before trying half-open.
    pub recovery_timeout: Duration,
    /// When the circuit was last opened.
    pub opened_at: Option<Instant>,
    /// Success count in half-open state.
    pub half_open_successes: u32,
    /// Required successes in half-open to close.
    pub half_open_threshold: u32,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            opened_at: None,
            half_open_successes: 0,
            half_open_threshold: 2,
        }
    }
}

impl CircuitBreaker {
    /// Check if a request should be allowed through.
    pub fn should_allow(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(opened_at) = self.opened_at {
                    if opened_at.elapsed() >= self.recovery_timeout {
                        self.state = CircuitState::HalfOpen;
                        self.half_open_successes = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful operation.
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.half_open_successes += 1;
                if self.half_open_successes >= self.half_open_threshold {
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Record a failed operation.
    pub fn record_failure(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.failure_threshold {
                    self.state = CircuitState::Open;
                    self.opened_at = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                // Back to open on any failure in half-open
                self.state = CircuitState::Open;
                self.opened_at = Some(Instant::now());
                self.half_open_successes = 0;
            }
            CircuitState::Open => {}
        }
    }

    /// Reset to closed state.
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.opened_at = None;
        self.half_open_successes = 0;
    }
}

// ─── Recovery Actions ────────────────────────────────────────────────────────

/// A recovery action to attempt when an operation fails.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Wait a fixed duration before retrying.
    Wait(Duration),
    /// Re-capture the accessibility tree (stale cache).
    RefreshSnapshot,
    /// Bring the target window to foreground.
    FocusTargetWindow,
    /// Click away to dismiss any blocking dialog/popup.
    DismissPopup,
    /// Send Escape key to close modal dialogs.
    SendEscape,
    /// Restart the target process.
    RestartProcess { exe_path: String },
    /// Execute a custom PowerShell recovery script.
    CustomScript(String),
}

/// Recovery plan for a specific failure scenario.
#[derive(Debug, Clone)]
pub struct RecoveryPlan {
    /// What triggered this recovery.
    pub trigger_error: String,
    /// Ordered sequence of recovery actions to try.
    pub actions: Vec<RecoveryAction>,
    /// Maximum total recovery time before giving up.
    pub max_recovery_time: Duration,
}

impl RecoveryPlan {
    /// Create a standard recovery plan for element-not-found errors.
    pub fn for_element_not_found() -> Self {
        Self {
            trigger_error: "element not found".to_string(),
            actions: vec![
                RecoveryAction::Wait(Duration::from_millis(500)),
                RecoveryAction::RefreshSnapshot,
                RecoveryAction::FocusTargetWindow,
                RecoveryAction::Wait(Duration::from_millis(300)),
                RecoveryAction::RefreshSnapshot,
            ],
            max_recovery_time: Duration::from_secs(5),
        }
    }

    /// Create a standard recovery plan for blocked-by-popup errors.
    pub fn for_blocked_by_popup() -> Self {
        Self {
            trigger_error: "blocked by popup".to_string(),
            actions: vec![
                RecoveryAction::SendEscape,
                RecoveryAction::Wait(Duration::from_millis(200)),
                RecoveryAction::DismissPopup,
                RecoveryAction::Wait(Duration::from_millis(300)),
                RecoveryAction::RefreshSnapshot,
            ],
            max_recovery_time: Duration::from_secs(3),
        }
    }
}

/// Build a PowerShell script for a recovery action.
pub fn build_recovery_script(action: &RecoveryAction) -> Option<String> {
    match action {
        RecoveryAction::Wait(_) => None, // Handled in Rust
        RecoveryAction::RefreshSnapshot => None, // Handled by re-invoking capture
        RecoveryAction::FocusTargetWindow => Some(
            r#"
Add-Type @'
using System; using System.Runtime.InteropServices;
public class FocusHelper {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
$hwnd = [FocusHelper]::GetForegroundWindow()
[FocusHelper]::SetForegroundWindow($hwnd) | Out-Null
Write-Output '{"success":true,"action":"focus_window"}'
"#
            .to_string(),
        ),
        RecoveryAction::DismissPopup => Some(
            r#"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep -Milliseconds 100
Write-Output '{"success":true,"action":"dismiss_popup"}'
"#
            .to_string(),
        ),
        RecoveryAction::SendEscape => Some(
            r#"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('{ESC}')
Start-Sleep -Milliseconds 100
Write-Output '{"success":true,"action":"send_escape"}'
"#
            .to_string(),
        ),
        RecoveryAction::RestartProcess { exe_path } => {
            let escaped = exe_path.replace('\'', "''");
            Some(format!(
                r#"
$proc = Start-Process -FilePath '{escaped}' -PassThru
Start-Sleep -Seconds 2
Write-Output (ConvertTo-Json @{{ success = $true; action = "restart_process"; pid = $proc.Id }} -Compress)
"#
            ))
        }
        RecoveryAction::CustomScript(script) => Some(script.clone()),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_increases_exponentially() {
        let policy = RetryPolicy::default();
        let d0 = policy.delay_for_attempt(0);
        let d1 = policy.delay_for_attempt(1);
        let d2 = policy.delay_for_attempt(2);
        // Each should be roughly 2x the previous (plus jitter)
        assert!(d1.as_millis() > d0.as_millis());
        assert!(d2.as_millis() > d1.as_millis());
    }

    #[test]
    fn retry_delay_capped_at_max() {
        let policy = RetryPolicy {
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 10.0,
            ..Default::default()
        };
        let d10 = policy.delay_for_attempt(10);
        assert!(d10 <= Duration::from_millis(1100)); // max + jitter
    }

    #[test]
    fn retryable_error_detection() {
        let policy = RetryPolicy::default();
        assert!(policy.is_retryable("target node 'btn' not found in window"));
        assert!(policy.is_retryable("operation timed out after 3000ms"));
        assert!(!policy.is_retryable("invalid action 'dance'"));
    }

    #[test]
    fn adaptive_wait_learns_from_history() {
        let mut wait = AdaptiveWait::default();
        // Simulate observations: element takes 100-300ms to appear
        for ms in [100, 150, 200, 250, 300, 120, 180, 220, 280, 250] {
            wait.record_observation(Duration::from_millis(ms));
        }
        let recommended = wait.recommended_wait();
        // P90 should be around 280-300ms
        assert!(recommended.as_millis() >= 250);
        assert!(recommended.as_millis() <= 350);
    }

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let mut cb = CircuitBreaker {
            failure_threshold: 3,
            ..Default::default()
        };
        assert_eq!(cb.state, CircuitState::Closed);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);
        assert!(!cb.should_allow());
    }

    #[test]
    fn circuit_breaker_half_open_recovers() {
        let mut cb = CircuitBreaker {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(0), // instant recovery for test
            half_open_threshold: 1,
            ..Default::default()
        };
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);
        // After recovery timeout (0ms), should_allow transitions to HalfOpen
        assert!(cb.should_allow());
        assert_eq!(cb.state, CircuitState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.state, CircuitState::Closed);
    }

    #[test]
    fn recovery_plan_element_not_found() {
        let plan = RecoveryPlan::for_element_not_found();
        assert_eq!(plan.actions.len(), 5);
        assert!(plan.max_recovery_time <= Duration::from_secs(10));
    }

    #[test]
    fn recovery_script_escape() {
        let script = build_recovery_script(&RecoveryAction::SendEscape);
        assert!(script.is_some());
        assert!(script.unwrap().contains("ESC"));
    }
}
