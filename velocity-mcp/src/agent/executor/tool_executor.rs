//! Structured tool-use execution framework with result validation and retry logic.
//!
//! Provides a [`ToolExecutor`] that wraps tool invocations with:
//! - Pre-execution validation (blocked / allowed lists, argument checks)
//! - Configurable retry with exponential back-off
//! - Duration tracking and success-rate statistics
//! - Per-tool failure accounting for diagnostics
//!
//! # Example
//!
//! ```rust
//! use velocity_mcp::agent::executor::tool_executor::{ToolExecutor, ToolExecutionPolicy};
//!
//! let policy = ToolExecutionPolicy::default();
//! let mut executor = ToolExecutor::new(policy);
//!
//! let result = executor.execute_with_retry(
//!     "read_file",
//!     |args| Ok(format!("read {args}")),
//!     "main.rs",
//! );
//! assert!(result.success);
//! ```

use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum allowed size (in bytes) for tool arguments.
const MAX_ARGS_SIZE: usize = 1_000_000; // 1 MB

// ---------------------------------------------------------------------------
// ToolExecutionResult
// ---------------------------------------------------------------------------

/// Outcome of a single tool execution (including any retries).
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    /// Name of the tool that was executed.
    pub tool_name: String,
    /// Whether the final attempt succeeded.
    pub success: bool,
    /// Captured output (stdout or error message).
    pub output: String,
    /// Wall-clock duration of the entire execution (including retries) in ms.
    pub duration_ms: u64,
    /// Number of retry attempts *after* the first try (0 = succeeded first try).
    pub retry_count: u32,
    /// Validation errors detected before execution.
    pub validation_errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// ToolValidationResult
// ---------------------------------------------------------------------------

/// Result of pre-execution validation for a tool call.
#[derive(Debug, Clone)]
pub struct ToolValidationResult {
    /// Whether the call passed all validation checks.
    pub valid: bool,
    /// Hard errors that prevent execution.
    pub errors: Vec<String>,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
    /// Optional suggested fix for the first error.
    pub suggested_fix: Option<String>,
}

// ---------------------------------------------------------------------------
// ToolExecutionPolicy
// ---------------------------------------------------------------------------

/// Policy controlling how tool calls are validated and retried.
#[derive(Debug, Clone)]
pub struct ToolExecutionPolicy {
    /// Maximum number of retry attempts after the first failure.
    pub max_retries: u32,
    /// Maximum allowed wall-clock duration for a single execution (ms).
    pub max_duration_ms: u64,
    /// Whether pre-execution validation is required.
    pub require_validation: bool,
    /// If `Some`, only these tool names are allowed. `None` means all allowed.
    pub allowed_tools: Option<Vec<String>>,
    /// Tool names that are unconditionally blocked.
    pub blocked_tools: Vec<String>,
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            max_duration_ms: 30_000,
            require_validation: true,
            allowed_tools: None,
            blocked_tools: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// ToolExecutionStats
// ---------------------------------------------------------------------------

/// Aggregate statistics across all executions performed by a [`ToolExecutor`].
#[derive(Debug, Clone)]
pub struct ToolExecutionStats {
    /// Total number of tool executions (including retries counted as one each).
    pub total_executions: u64,
    /// Number of executions that ultimately succeeded.
    pub success_count: u64,
    /// Number of executions that ultimately failed.
    pub failure_count: u64,
    /// Ratio of successes to total (0.0 – 1.0).
    pub success_rate: f64,
    /// Mean duration across all executions in ms.
    pub avg_duration_ms: f64,
    /// 95th-percentile duration in ms.
    pub p95_duration_ms: u64,
    /// Tool that was invoked the most, if any.
    pub most_used_tool: Option<String>,
    /// Tool that failed the most, if any.
    pub most_failed_tool: Option<String>,
}

// ---------------------------------------------------------------------------
// ToolExecutor
// ---------------------------------------------------------------------------

/// Orchestrates tool execution with validation, retry, and bookkeeping.
pub struct ToolExecutor {
    /// Active execution policy.
    pub policy: ToolExecutionPolicy,
    /// Chronological history of every execution result.
    pub execution_history: Vec<ToolExecutionResult>,
    /// Running count of successful executions.
    pub success_count: u64,
    /// Running count of failed executions.
    pub failure_count: u64,
    /// Cumulative wall-clock duration of all executions (ms).
    pub total_duration_ms: u64,
    /// Per-tool invocation count.
    tool_use_counts: HashMap<String, u64>,
    /// Per-tool failure count.
    tool_failure_counts: HashMap<String, u64>,
}

impl ToolExecutor {
    /// Create a new executor with the given policy.
    pub fn new(policy: ToolExecutionPolicy) -> Self {
        Self {
            policy,
            execution_history: Vec::new(),
            success_count: 0,
            failure_count: 0,
            total_duration_ms: 0,
            tool_use_counts: HashMap::new(),
            tool_failure_counts: HashMap::new(),
        }
    }

    // ------------------------------------------------------------------
    // Validation
    // ------------------------------------------------------------------

    /// Validate a tool call before execution.
    ///
    /// Checks performed:
    /// 1. Tool is not in the blocked list.
    /// 2. Tool is in the allowed list (if one is configured).
    /// 3. Arguments are not empty.
    /// 4. Arguments do not exceed [`MAX_ARGS_SIZE`].
    pub fn validate_tool_call(&self, tool_name: &str, args: &str) -> ToolValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut suggested_fix = None;

        // 1. Blocked check.
        if self.policy.blocked_tools.iter().any(|t| t == tool_name) {
            errors.push(format!("tool '{tool_name}' is blocked by policy"));
            suggested_fix = Some(format!(
                "Remove '{tool_name}' from the blocked_tools list in ToolExecutionPolicy"
            ));
        }

        // 2. Allowed-list check.
        if let Some(ref allowed) = self.policy.allowed_tools {
            if !allowed.iter().any(|t| t == tool_name) {
                errors.push(format!(
                    "tool '{tool_name}' is not in the allowed_tools list"
                ));
            }
        }

        // 3. Empty args check.
        if args.trim().is_empty() {
            errors.push("tool arguments must be empty".to_string());
            warnings.push("provide at least one argument for the tool call".to_string());
        }

        // 4. Args size check.
        if args.len() > MAX_ARGS_SIZE {
            errors.push(format!(
                "tool arguments exceed maximum size of {MAX_ARGS_SIZE} bytes (got {} bytes)",
                args.len()
            ));
        }

        let valid = errors.is_empty();
        ToolValidationResult {
            valid,
            errors,
            warnings,
            suggested_fix,
        }
    }

    // ------------------------------------------------------------------
    // Execution
    // ------------------------------------------------------------------

    /// Execute a tool with validation and retry.
    ///
    /// The `executor` closure is called with `args` on each attempt. It should
    /// return `Ok(output)` on success or `Err(message)` on failure.
    ///
    /// The method:
    /// 1. Validates the call (if the policy requires it).
    /// 2. Invokes `executor` up to `1 + max_retries` times.
    /// 3. Tracks wall-clock duration.
    /// 4. Records the result in history.
    pub fn execute_with_retry<F>(
        &mut self,
        tool_name: &str,
        executor: F,
        args: &str,
    ) -> ToolExecutionResult
    where
        F: Fn(&str) -> Result<String, String>,
    {
        // --- Pre-execution validation ---
        let validation = if self.policy.require_validation {
            self.validate_tool_call(tool_name, args)
        } else {
            ToolValidationResult {
                valid: true,
                errors: Vec::new(),
                warnings: Vec::new(),
                suggested_fix: None,
            }
        };

        if !validation.valid {
            let result = ToolExecutionResult {
                tool_name: tool_name.to_string(),
                success: false,
                output: String::new(),
                duration_ms: 0,
                retry_count: 0,
                validation_errors: validation.errors,
            };
            self.record_result(result.clone());
            return result;
        }

        // --- Execute with retry ---
        let start = Instant::now();
        let max_attempts = 1 + self.policy.max_retries;
        let mut last_output = String::new();
        let mut succeeded = false;
        let mut retry_count = 0u32;

        for i in 0..max_attempts {
            match executor(args) {
                Ok(output) => {
                    last_output = output;
                    succeeded = true;
                    retry_count = i;
                    break;
                }
                Err(msg) => {
                    last_output = msg;
                    retry_count = i;
                    // Exponential back-off before next retry (skip after last).
                    if i + 1 < max_attempts {
                        let backoff_ms = 10u64 * (1u64 << i);
                        std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                    }
                }
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;

        let result = ToolExecutionResult {
            tool_name: tool_name.to_string(),
            success: succeeded,
            output: last_output,
            duration_ms: elapsed,
            retry_count,
            validation_errors: Vec::new(),
        };

        self.record_result(result.clone());
        result
    }

    /// Record a result in the executor's history and update counters.
    pub fn record_result(&mut self, result: ToolExecutionResult) {
        if result.success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
            *self
                .tool_failure_counts
                .entry(result.tool_name.clone())
                .or_insert(0) += 1;
        }
        *self
            .tool_use_counts
            .entry(result.tool_name.clone())
            .or_insert(0) += 1;
        self.total_duration_ms += result.duration_ms;
        self.execution_history.push(result);
    }

    /// Ratio of successful executions to total (0.0 – 1.0).
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 0.0;
        }
        self.success_count as f64 / total as f64
    }

    /// Mean execution duration in ms.
    pub fn avg_duration_ms(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 0.0;
        }
        self.total_duration_ms as f64 / total as f64
    }

    /// Return references to the most recent `count` failed results.
    pub fn recent_failures(&self, count: usize) -> Vec<&ToolExecutionResult> {
        self.execution_history
            .iter()
            .rev()
            .filter(|r| !r.success)
            .take(count)
            .collect()
    }

    /// Compute aggregate statistics across all recorded executions.
    pub fn execution_stats(&self) -> ToolExecutionStats {
        let total = self.success_count + self.failure_count;

        // p95 duration.
        let mut durations: Vec<u64> = self
            .execution_history
            .iter()
            .map(|r| r.duration_ms)
            .collect();
        durations.sort_unstable();
        let p95 = if durations.is_empty() {
            0
        } else {
            let idx = ((durations.len() as f64) * 0.95).ceil() as usize;
            durations[idx.min(durations.len()) - 1]
        };

        // Most-used tool.
        let most_used_tool = self
            .tool_use_counts
            .iter()
            .max_by_key(|(_, &v)| v)
            .map(|(k, _)| k.clone());

        // Most-failed tool.
        let most_failed_tool = self
            .tool_failure_counts
            .iter()
            .max_by_key(|(_, &v)| v)
            .map(|(k, _)| k.clone());

        ToolExecutionStats {
            total_executions: total,
            success_count: self.success_count,
            failure_count: self.failure_count,
            success_rate: self.success_rate(),
            avg_duration_ms: self.avg_duration_ms(),
            p95_duration_ms: p95,
            most_used_tool,
            most_failed_tool,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: default policy for tests.
    fn test_policy() -> ToolExecutionPolicy {
        ToolExecutionPolicy::default()
    }

    // ---- Successful execution ------------------------------------------------

    #[test]
    fn test_successful_execution_first_try() {
        let mut exec = ToolExecutor::new(test_policy());
        let result = exec.execute_with_retry("read_file", |_| Ok("content".into()), "main.rs");
        assert!(result.success);
        assert_eq!(result.output, "content");
        assert_eq!(result.retry_count, 0);
        assert!(result.validation_errors.is_empty());
    }

    #[test]
    fn test_successful_execution_records_in_history() {
        let mut exec = ToolExecutor::new(test_policy());
        exec.execute_with_retry("tool_a", |_| Ok("ok".into()), "args");
        assert_eq!(exec.execution_history.len(), 1);
        assert_eq!(exec.success_count, 1);
        assert_eq!(exec.failure_count, 0);
    }

    // ---- Retry on failure ----------------------------------------------------

    #[test]
    fn test_retry_succeeds_on_second_attempt() {
        let mut exec = ToolExecutor::new(test_policy());
        let counter = std::sync::atomic::AtomicU32::new(0);
        let result = exec.execute_with_retry(
            "flaky_tool",
            |_| {
                let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Err("transient error".into())
                } else {
                    Ok("recovered".into())
                }
            },
            "args",
        );
        assert!(result.success);
        assert_eq!(result.output, "recovered");
        assert!(result.retry_count >= 1);
    }

    #[test]
    fn test_retry_succeeds_on_third_attempt() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 3,
            ..Default::default()
        });
        let counter = std::sync::atomic::AtomicU32::new(0);
        let result = exec.execute_with_retry(
            "flaky",
            |_| {
                let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n < 2 {
                    Err("fail".into())
                } else {
                    Ok("done".into())
                }
            },
            "args",
        );
        assert!(result.success);
        assert_eq!(result.retry_count, 2);
    }

    // ---- Max retries exhausted -----------------------------------------------

    #[test]
    fn test_max_retries_exhausted_returns_failure() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 2,
            ..Default::default()
        });
        let result = exec.execute_with_retry("broken", |_| Err("always fails".into()), "args");
        assert!(!result.success);
        assert_eq!(result.retry_count, 2);
        assert_eq!(result.output, "always fails");
    }

    #[test]
    fn test_max_retries_zero_no_retry() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 0,
            ..Default::default()
        });
        let result = exec.execute_with_retry("once", |_| Err("fail".into()), "args");
        assert!(!result.success);
        assert_eq!(result.retry_count, 0);
    }

    // ---- Blocked tool rejection ----------------------------------------------

    #[test]
    fn test_blocked_tool_is_rejected() {
        let policy = ToolExecutionPolicy {
            blocked_tools: vec!["dangerous_tool".into()],
            ..Default::default()
        };
        let mut exec = ToolExecutor::new(policy);
        let result = exec.execute_with_retry("dangerous_tool", |_| Ok("nope".into()), "args");
        assert!(!result.success);
        assert!(!result.validation_errors.is_empty());
        assert!(result.validation_errors[0].contains("blocked"));
    }

    #[test]
    fn test_non_blocked_tool_passes_check() {
        let policy = ToolExecutionPolicy {
            blocked_tools: vec!["bad".into()],
            ..Default::default()
        };
        let mut exec = ToolExecutor::new(policy);
        let result = exec.execute_with_retry("good", |_| Ok("ok".into()), "args");
        assert!(result.success);
    }

    // ---- Allowed tool filtering ----------------------------------------------

    #[test]
    fn test_allowed_tool_filter_rejects_unlisted() {
        let policy = ToolExecutionPolicy {
            allowed_tools: Some(vec!["read_file".into(), "write_file".into()]),
            ..Default::default()
        };
        let mut exec = ToolExecutor::new(policy);
        let result = exec.execute_with_retry("delete_file", |_| Ok("ok".into()), "args");
        assert!(!result.success);
        assert!(result.validation_errors[0].contains("not in the allowed_tools"));
    }

    #[test]
    fn test_allowed_tool_filter_accepts_listed() {
        let policy = ToolExecutionPolicy {
            allowed_tools: Some(vec!["read_file".into()]),
            ..Default::default()
        };
        let mut exec = ToolExecutor::new(policy);
        let result = exec.execute_with_retry("read_file", |_| Ok("data".into()), "path");
        assert!(result.success);
    }

    #[test]
    fn test_allowed_tools_none_means_all_allowed() {
        let policy = ToolExecutionPolicy {
            allowed_tools: None,
            ..Default::default()
        };
        let mut exec = ToolExecutor::new(policy);
        let result = exec.execute_with_retry("anything", |_| Ok("ok".into()), "args");
        assert!(result.success);
    }

    // ---- Empty args rejection ------------------------------------------------

    #[test]
    fn test_empty_args_are_rejected() {
        let mut exec = ToolExecutor::new(test_policy());
        let result = exec.execute_with_retry("tool", |_| Ok("ok".into()), "   ");
        assert!(!result.success);
        assert!(result.validation_errors.iter().any(|e| e.contains("empty")));
    }

    #[test]
    fn test_non_empty_args_pass() {
        let mut exec = ToolExecutor::new(test_policy());
        let result = exec.execute_with_retry("tool", |_| Ok("ok".into()), "valid");
        assert!(result.success);
    }

    // ---- Args size limit -----------------------------------------------------

    #[test]
    fn test_oversized_args_are_rejected() {
        let mut exec = ToolExecutor::new(test_policy());
        let big_args = "x".repeat(MAX_ARGS_SIZE + 1);
        let result = exec.execute_with_retry("tool", |_| Ok("ok".into()), &big_args);
        assert!(!result.success);
        assert!(result
            .validation_errors
            .iter()
            .any(|e| e.contains("maximum size")));
    }

    // ---- Duration tracking ---------------------------------------------------

    #[test]
    fn test_duration_is_tracked() {
        let mut exec = ToolExecutor::new(test_policy());
        let result = exec.execute_with_retry(
            "slow_tool",
            |_| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                Ok("done".into())
            },
            "args",
        );
        assert!(result.duration_ms >= 15); // allow some scheduling slack
    }

    #[test]
    fn test_total_duration_accumulates() {
        let mut exec = ToolExecutor::new(test_policy());
        exec.execute_with_retry("a", |_| Ok("1".into()), "x");
        exec.execute_with_retry("b", |_| Ok("2".into()), "y");
        let _ = exec.total_duration_ms; // just ensure no panic
        assert_eq!(exec.execution_history.len(), 2);
    }

    // ---- Success rate calculation --------------------------------------------

    #[test]
    fn test_success_rate_all_succeed() {
        let mut exec = ToolExecutor::new(test_policy());
        for _ in 0..5 {
            exec.execute_with_retry("t", |_| Ok("ok".into()), "a");
        }
        assert!((exec.success_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_all_fail() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 0,
            ..Default::default()
        });
        for _ in 0..3 {
            exec.execute_with_retry("t", |_| Err("fail".into()), "a");
        }
        assert!((exec.success_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_mixed() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 0,
            ..Default::default()
        });
        exec.execute_with_retry("t", |_| Ok("ok".into()), "a");
        exec.execute_with_retry("t", |_| Err("fail".into()), "a");
        assert!((exec.success_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_zero_executions() {
        let exec = ToolExecutor::new(test_policy());
        assert!((exec.success_rate() - 0.0).abs() < f64::EPSILON);
    }

    // ---- Avg duration --------------------------------------------------------

    #[test]
    fn test_avg_duration_zero_executions() {
        let exec = ToolExecutor::new(test_policy());
        assert!((exec.avg_duration_ms() - 0.0).abs() < f64::EPSILON);
    }

    // ---- Recent failures retrieval -------------------------------------------

    #[test]
    fn test_recent_failures_returns_correct_count() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 0,
            ..Default::default()
        });
        exec.execute_with_retry("a", |_| Ok("ok".into()), "x");
        exec.execute_with_retry("b", |_| Err("fail1".into()), "x");
        exec.execute_with_retry("c", |_| Ok("ok".into()), "x");
        exec.execute_with_retry("d", |_| Err("fail2".into()), "x");
        let failures = exec.recent_failures(5);
        assert_eq!(failures.len(), 2);
        // Most recent failure first.
        assert_eq!(failures[0].tool_name, "d");
        assert_eq!(failures[1].tool_name, "b");
    }

    #[test]
    fn test_recent_failures_empty_when_all_succeed() {
        let mut exec = ToolExecutor::new(test_policy());
        exec.execute_with_retry("a", |_| Ok("ok".into()), "x");
        assert!(exec.recent_failures(10).is_empty());
    }

    // ---- Stats accuracy ------------------------------------------------------

    #[test]
    fn test_execution_stats_comprehensive() {
        let mut exec = ToolExecutor::new(ToolExecutionPolicy {
            max_retries: 0,
            ..Default::default()
        });
        exec.execute_with_retry("read", |_| Ok("ok".into()), "a");
        exec.execute_with_retry("read", |_| Ok("ok".into()), "b");
        exec.execute_with_retry("write", |_| Ok("ok".into()), "c");
        exec.execute_with_retry("write", |_| Err("fail".into()), "d");
        exec.execute_with_retry("delete", |_| Err("fail".into()), "e");

        let stats = exec.execution_stats();
        assert_eq!(stats.total_executions, 5);
        assert_eq!(stats.success_count, 3);
        assert_eq!(stats.failure_count, 2);
        assert!((stats.success_rate - 0.6).abs() < f64::EPSILON);
        // "read" and "write" are tied at 2 calls each; HashMap order is
        // non-deterministic so just check one of them is returned.
        assert!(
            stats.most_used_tool.as_deref() == Some("read")
                || stats.most_used_tool.as_deref() == Some("write")
        );
        // "write" and "delete" each have 1 failure; HashMap order is
        // non-deterministic so just check one of them is returned.
        assert!(
            stats.most_failed_tool.as_deref() == Some("write")
                || stats.most_failed_tool.as_deref() == Some("delete")
        );
    }

    #[test]
    fn test_execution_stats_empty() {
        let exec = ToolExecutor::new(test_policy());
        let stats = exec.execution_stats();
        assert_eq!(stats.total_executions, 0);
        assert_eq!(stats.success_count, 0);
        assert_eq!(stats.failure_count, 0);
        assert!(stats.most_used_tool.is_none());
        assert!(stats.most_failed_tool.is_none());
    }

    #[test]
    fn test_p95_duration_computed_correctly() {
        let mut exec = ToolExecutor::new(test_policy());
        // Record 20 results with known durations via record_result directly.
        for i in 0..20 {
            exec.record_result(ToolExecutionResult {
                tool_name: "t".into(),
                success: true,
                output: String::new(),
                duration_ms: (i + 1) * 10, // 10, 20, … 200
                retry_count: 0,
                validation_errors: Vec::new(),
            });
        }
        let stats = exec.execution_stats();
        // p95 of [10,20,…,200]: index = ceil(20*0.95)-1 = 19-1 = 18 → 190
        assert_eq!(stats.p95_duration_ms, 190);
    }

    // ---- Validation bypass ---------------------------------------------------

    #[test]
    fn test_validation_bypassed_when_not_required() {
        let policy = ToolExecutionPolicy {
            require_validation: false,
            blocked_tools: vec!["blocked".into()],
            ..Default::default()
        };
        let mut exec = ToolExecutor::new(policy);
        // Even though "blocked" is in blocked_tools, validation is skipped.
        let result = exec.execute_with_retry("blocked", |_| Ok("ok".into()), "");
        assert!(result.success);
    }

    // ---- record_result updates counters --------------------------------------

    #[test]
    fn test_record_result_updates_counters() {
        let mut exec = ToolExecutor::new(test_policy());
        exec.record_result(ToolExecutionResult {
            tool_name: "x".into(),
            success: true,
            output: "ok".into(),
            duration_ms: 100,
            retry_count: 0,
            validation_errors: Vec::new(),
        });
        exec.record_result(ToolExecutionResult {
            tool_name: "y".into(),
            success: false,
            output: "err".into(),
            duration_ms: 50,
            retry_count: 1,
            validation_errors: vec!["bad".into()],
        });
        assert_eq!(exec.success_count, 1);
        assert_eq!(exec.failure_count, 1);
        assert_eq!(exec.total_duration_ms, 150);
        assert_eq!(exec.execution_history.len(), 2);
    }

    // ---- validate_tool_call directly -----------------------------------------

    #[test]
    fn test_validate_tool_call_clean() {
        let exec = ToolExecutor::new(test_policy());
        let v = exec.validate_tool_call("good_tool", "some args");
        assert!(v.valid);
        assert!(v.errors.is_empty());
        assert!(v.warnings.is_empty());
        assert!(v.suggested_fix.is_none());
    }

    #[test]
    fn test_validate_tool_call_blocked_with_suggestion() {
        let policy = ToolExecutionPolicy {
            blocked_tools: vec!["nope".into()],
            ..Default::default()
        };
        let exec = ToolExecutor::new(policy);
        let v = exec.validate_tool_call("nope", "args");
        assert!(!v.valid);
        assert!(v.suggested_fix.is_some());
    }
}
