//! Prometheus metrics for Velocity MCP server.
//!
//! This module provides comprehensive metrics collection for monitoring
//! the MCP server's performance, resource usage, and operational health.
//!
//! # Metrics Exposed
//!
//! - **Request metrics**: Request count, duration, status codes
//! - **Tool metrics**: Tool execution count, duration, error rates
//! - **Provider metrics**: API calls per provider, latency, error rates
//! - **Resource metrics**: Memory usage, CPU time, active sessions
//! - **Agent metrics**: Agent execution count, duration, token usage
//!
//! # Usage
//!
//! ```ignore
//! use crate::metrics::{Metrics, record_request};
//!
//! // Record a request
//! let start = std::time::Instant::now();
//! record_request("list_files", "success", start.elapsed());
//!
//! // Get metrics in Prometheus format
//! let metrics = Metrics::global();
//! let output = metrics.encode();
//! ```

use prometheus::{
    Counter, CounterVec, Encoder, Gauge, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};
use std::sync::LazyLock;
use std::time::Duration;

/// Global metrics registry.
static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::new);

/// Metrics collection for the MCP server.
pub struct Metrics {
    registry: Registry,

    // Request metrics
    pub requests_total: CounterVec,
    pub request_duration_seconds: HistogramVec,
    pub requests_in_flight: Gauge,

    // Tool metrics
    pub tool_executions_total: CounterVec,
    pub tool_duration_seconds: HistogramVec,
    pub tool_errors_total: CounterVec,

    // Provider metrics
    pub provider_calls_total: CounterVec,
    pub provider_latency_seconds: HistogramVec,
    pub provider_errors_total: CounterVec,

    // Resource metrics
    pub memory_usage_bytes: Gauge,
    pub cpu_time_seconds: Counter,
    pub active_sessions: Gauge,

    // Agent metrics
    pub agent_executions_total: CounterVec,
    pub agent_duration_seconds: HistogramVec,
    pub tokens_processed_total: CounterVec,
}

impl Metrics {
    /// Create a new metrics instance.
    ///
    /// # Panics
    ///
    /// Panics if metric names conflict or are invalid — these are hardcoded
    /// constants so failure indicates a programming error.
    pub fn new() -> Self {
        let registry = Registry::new();

        // Helper closures to reduce boilerplate.
        let counter_vec = |name: &str, help: &str, labels: &[&str]| -> CounterVec {
            let c = CounterVec::new(Opts::new(name, help), labels)
                .unwrap_or_else(|e| panic!("metric {name}: {e}"));
            registry
                .register(Box::new(c.clone()))
                .unwrap_or_else(|e| panic!("register {name}: {e}"));
            c
        };
        let histogram_vec =
            |name: &str, help: &str, buckets: Vec<f64>, labels: &[&str]| -> HistogramVec {
                let h = HistogramVec::new(HistogramOpts::new(name, help).buckets(buckets), labels)
                    .unwrap_or_else(|e| panic!("metric {name}: {e}"));
                registry
                    .register(Box::new(h.clone()))
                    .unwrap_or_else(|e| panic!("register {name}: {e}"));
                h
            };
        let gauge = |name: &str, help: &str| -> Gauge {
            let g = Gauge::new(name, help).unwrap_or_else(|e| panic!("metric {name}: {e}"));
            registry
                .register(Box::new(g.clone()))
                .unwrap_or_else(|e| panic!("register {name}: {e}"));
            g
        };
        let counter = |name: &str, help: &str| -> Counter {
            let c = Counter::new(name, help).unwrap_or_else(|e| panic!("metric {name}: {e}"));
            registry
                .register(Box::new(c.clone()))
                .unwrap_or_else(|e| panic!("register {name}: {e}"));
            c
        };

        // Request metrics
        let requests_total = counter_vec(
            "velocity_mcp_requests_total",
            "Total number of MCP requests",
            &["method", "status"],
        );
        let request_duration_seconds = histogram_vec(
            "velocity_mcp_request_duration_seconds",
            "MCP request duration in seconds",
            vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0],
            &["method"],
        );
        let requests_in_flight = gauge(
            "velocity_mcp_requests_in_flight",
            "Number of requests currently being processed",
        );

        // Tool metrics
        let tool_executions_total = counter_vec(
            "velocity_mcp_tool_executions_total",
            "Total number of tool executions",
            &["tool", "status"],
        );
        let tool_duration_seconds = histogram_vec(
            "velocity_mcp_tool_duration_seconds",
            "Tool execution duration in seconds",
            vec![0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0],
            &["tool"],
        );
        let tool_errors_total = counter_vec(
            "velocity_mcp_tool_errors_total",
            "Total number of tool execution errors",
            &["tool", "error_type"],
        );

        // Provider metrics
        let provider_calls_total = counter_vec(
            "velocity_mcp_provider_calls_total",
            "Total number of API calls to providers",
            &["provider", "model", "status"],
        );
        let provider_latency_seconds = histogram_vec(
            "velocity_mcp_provider_latency_seconds",
            "Provider API call latency in seconds",
            vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0],
            &["provider", "model"],
        );
        let provider_errors_total = counter_vec(
            "velocity_mcp_provider_errors_total",
            "Total number of provider API errors",
            &["provider", "error_type"],
        );

        // Resource metrics
        let memory_usage_bytes = gauge(
            "velocity_mcp_memory_usage_bytes",
            "Current memory usage in bytes",
        );
        let cpu_time_seconds = counter(
            "velocity_mcp_cpu_time_seconds_total",
            "Total CPU time consumed",
        );
        let active_sessions = gauge(
            "velocity_mcp_active_sessions",
            "Number of active MCP sessions",
        );

        // Agent metrics
        let agent_executions_total = counter_vec(
            "velocity_mcp_agent_executions_total",
            "Total number of agent executions",
            &["agent_type", "status"],
        );
        let agent_duration_seconds = histogram_vec(
            "velocity_mcp_agent_duration_seconds",
            "Agent execution duration in seconds",
            vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0],
            &["agent_type"],
        );
        let tokens_processed_total = counter_vec(
            "velocity_mcp_tokens_processed_total",
            "Total number of tokens processed",
            &["provider", "direction"],
        );

        Self {
            registry,
            requests_total,
            request_duration_seconds,
            requests_in_flight,
            tool_executions_total,
            tool_duration_seconds,
            tool_errors_total,
            provider_calls_total,
            provider_latency_seconds,
            provider_errors_total,
            memory_usage_bytes,
            cpu_time_seconds,
            active_sessions,
            agent_executions_total,
            agent_duration_seconds,
            tokens_processed_total,
        }
    }

    /// Get the global metrics instance.
    pub fn global() -> &'static Metrics {
        &METRICS
    }

    /// Encode metrics in Prometheus text format.
    pub fn encode(&self) -> String {
        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

// Convenience functions

/// Record a request.
pub fn record_request(method: &str, status: &str, duration: Duration) {
    let metrics = Metrics::global();
    metrics
        .requests_total
        .with_label_values(&[method, status])
        .inc();
    metrics
        .request_duration_seconds
        .with_label_values(&[method])
        .observe(duration.as_secs_f64());
}

/// Record a tool execution.
pub fn record_tool_execution(tool: &str, status: &str, duration: Duration) {
    let metrics = Metrics::global();
    metrics
        .tool_executions_total
        .with_label_values(&[tool, status])
        .inc();
    metrics
        .tool_duration_seconds
        .with_label_values(&[tool])
        .observe(duration.as_secs_f64());
}

/// Record a tool error.
pub fn record_tool_error(tool: &str, error_type: &str) {
    let metrics = Metrics::global();
    metrics
        .tool_errors_total
        .with_label_values(&[tool, error_type])
        .inc();
}

/// Record a provider API call.
pub fn record_provider_call(provider: &str, model: &str, status: &str, latency: Duration) {
    let metrics = Metrics::global();
    metrics
        .provider_calls_total
        .with_label_values(&[provider, model, status])
        .inc();
    metrics
        .provider_latency_seconds
        .with_label_values(&[provider, model])
        .observe(latency.as_secs_f64());
}

/// Record a provider error.
pub fn record_provider_error(provider: &str, error_type: &str) {
    let metrics = Metrics::global();
    metrics
        .provider_errors_total
        .with_label_values(&[provider, error_type])
        .inc();
}

/// Record an agent execution.
pub fn record_agent_execution(agent_type: &str, status: &str, duration: Duration) {
    let metrics = Metrics::global();
    metrics
        .agent_executions_total
        .with_label_values(&[agent_type, status])
        .inc();
    metrics
        .agent_duration_seconds
        .with_label_values(&[agent_type])
        .observe(duration.as_secs_f64());
}

/// Record token usage.
pub fn record_tokens(provider: &str, direction: &str, count: u64) {
    let metrics = Metrics::global();
    metrics
        .tokens_processed_total
        .with_label_values(&[provider, direction])
        .inc_by(count as f64);
}

/// Update memory usage.
pub fn update_memory_usage(bytes: u64) {
    let metrics = Metrics::global();
    metrics.memory_usage_bytes.set(bytes as f64);
}

/// Update active sessions count.
pub fn update_active_sessions(count: i64) {
    let metrics = Metrics::global();
    metrics.active_sessions.set(count as f64);
}

/// Increment requests in flight.
pub fn inc_requests_in_flight() {
    let metrics = Metrics::global();
    metrics.requests_in_flight.inc();
}

/// Decrement requests in flight.
pub fn dec_requests_in_flight() {
    let metrics = Metrics::global();
    metrics.requests_in_flight.dec();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        // Record at least one observation so the metric appears in output
        metrics
            .requests_total
            .with_label_values(&["init", "ok"])
            .inc();
        metrics
            .tool_executions_total
            .with_label_values(&["init", "ok"])
            .inc();
        let output = metrics.encode();
        assert!(output.contains("velocity_mcp_requests_total"));
        assert!(output.contains("velocity_mcp_tool_executions_total"));
    }

    #[test]
    fn test_record_request() {
        record_request("test_method", "success", Duration::from_millis(100));
        let metrics = Metrics::global();
        let output = metrics.encode();
        assert!(output.contains("test_method"));
    }

    #[test]
    fn test_record_tool_execution() {
        record_tool_execution("test_tool", "success", Duration::from_millis(50));
        let metrics = Metrics::global();
        let output = metrics.encode();
        assert!(output.contains("test_tool"));
    }
}
