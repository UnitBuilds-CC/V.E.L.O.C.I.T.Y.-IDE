//! Distributed telemetry (tracing) infrastructure for Velocity MCP server.
//!
//! This module provides structured, hierarchical tracing using the `tracing`
//! crate ecosystem. It supports multiple output formats and can be configured
//! via environment variables.
//!
//! # Features
//!
//! - **Structured spans**: Hierarchical request/tool/provider tracing
//! - **Multiple exporters**: Console (pretty/JSON), file-based NDJSON logs
//! - **Environment configuration**: `VELOCITY_TRACE_FORMAT`, `VELOCITY_TRACE_DIR`,
//!   `VELOCITY_TRACE_LEVEL`, `RUST_LOG`
//! - **Low overhead**: Filtering happens before span construction
//!
//! # Usage
//!
//! ```ignore
//! use crate::tracing::init_tracing;
//!
//! // Initialize early in main()
//! init_tracing();
//!
//! // Use tracing macros and spans
//! use tracing::{info, info_span, instrument};
//!
//! #[instrument(skip(input))]
//! async fn handle_request(input: &str) -> Result<(), Error> {
//!     info!("processing request");
//!     // ... work here is automatically part of the span
//!     Ok(())
//! }
//! ```

use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};
use std::fs;
use std::path::PathBuf;

/// Trace output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFormat {
    /// Human-readable pretty output (default for development).
    Pretty,
    /// JSON output (default for production / log aggregation).
    Json,
    /// Compact single-line output.
    Compact,
}

impl TraceFormat {
    /// Parse from environment variable value.
    pub fn from_env_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => Self::Json,
            "compact" => Self::Compact,
            _ => Self::Pretty,
        }
    }
}

/// Tracing configuration.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Output format for traces.
    pub format: TraceFormat,
    /// Directory for file-based log output (optional).
    pub log_dir: Option<PathBuf>,
    /// Base log level filter (e.g., "info", "debug", "trace").
    pub level: String,
    /// Whether to include timing data in spans.
    pub with_timings: bool,
    /// Whether to output thread names.
    pub with_thread_names: bool,
    /// Whether to output thread IDs.
    pub with_thread_ids: bool,
    /// Whether to output source file locations.
    pub with_source_location: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            format: TraceFormat::Pretty,
            log_dir: None,
            level: "info".to_string(),
            with_timings: true,
            with_thread_names: true,
            with_thread_ids: false,
            with_source_location: true,
        }
    }
}

impl TracingConfig {
    /// Load configuration from environment variables.
    ///
    /// - `VELOCITY_TRACE_FORMAT`: "pretty" | "json" | "compact"
    /// - `VELOCITY_TRACE_DIR`: path to directory for NDJSON log files
    /// - `VELOCITY_TRACE_LEVEL`: default log level filter
    /// - `VELOCITY_TRACE_TIMINGS`: "true" | "false"
    /// - `RUST_LOG`: standard env filter (takes precedence over VELOCITY_TRACE_LEVEL)
    pub fn from_env() -> Self {
        let format = std::env::var("VELOCITY_TRACE_FORMAT")
            .map(|s| TraceFormat::from_env_str(&s))
            .unwrap_or(TraceFormat::Pretty);

        let log_dir = std::env::var("VELOCITY_TRACE_DIR")
            .ok()
            .map(PathBuf::from);

        let level = std::env::var("VELOCITY_TRACE_LEVEL")
            .unwrap_or_else(|_| "info".to_string());

        let with_timings = std::env::var("VELOCITY_TRACE_TIMINGS")
            .map(|v| v != "false")
            .unwrap_or(true);

        Self {
            format,
            log_dir,
            level,
            with_timings,
            ..Default::default()
        }
    }
}

/// Initialize the tracing subsystem with default (env-based) configuration.
///
/// This should be called once early in `main()`. It is safe to call multiple
/// times — subsequent calls are no-ops (the global subscriber is set once).
pub fn init_tracing() {
    let config = TracingConfig::from_env();
    init_tracing_with_config(&config);
}

/// Initialize tracing with an explicit configuration.
pub fn init_tracing_with_config(config: &TracingConfig) {
    // Build the env filter. RUST_LOG takes precedence.
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    // Build the console layer based on format.
    let console_layer = match config.format {
        TraceFormat::Pretty => {
            fmt::layer()
                .pretty()
                .with_span_events(if config.with_timings {
                    FmtSpan::CLOSE
                } else {
                    FmtSpan::NONE
                })
                .with_thread_names(config.with_thread_names)
                .with_thread_ids(config.with_thread_ids)
                .with_file(config.with_source_location)
                .with_line_number(config.with_source_location)
                .with_filter(env_filter)
                .boxed()
        }
        TraceFormat::Json => {
            fmt::layer()
                .json()
                .with_span_events(if config.with_timings {
                    FmtSpan::CLOSE
                } else {
                    FmtSpan::NONE
                })
                .with_thread_names(config.with_thread_names)
                .with_thread_ids(config.with_thread_ids)
                .with_file(config.with_source_location)
                .with_line_number(config.with_source_location)
                .with_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new(&config.level)),
                )
                .boxed()
        }
        TraceFormat::Compact => {
            fmt::layer()
                .compact()
                .with_span_events(if config.with_timings {
                    FmtSpan::CLOSE
                } else {
                    FmtSpan::NONE
                })
                .with_thread_names(config.with_thread_names)
                .with_thread_ids(config.with_thread_ids)
                .with_file(config.with_source_location)
                .with_line_number(config.with_source_location)
                .with_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new(&config.level)),
                )
                .boxed()
        }
    };

    // Optionally add a file layer for persistent NDJSON logs.
    if let Some(ref dir) = config.log_dir {
        // Ensure the directory exists.
        if let Err(e) = fs::create_dir_all(dir) {
            eprintln!("WARNING: could not create trace log dir {:?}: {}", dir, e);
            return;
        }

        let file_appender = tracing_appender::rolling::daily(dir, "velocity-mcp.log");
        let file_layer = fmt::layer()
            .json()
            .with_writer(file_appender)
            .with_span_events(FmtSpan::CLOSE)
            .with_file(true)
            .with_line_number(true)
            .with_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new(&config.level)),
            );

        tracing_subscriber::registry()
            .with(console_layer)
            .with(file_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(console_layer)
            .init();
    }
}

/// Convenience macro-like functions for common span patterns.

/// Create a span for an MCP request.
#[macro_export]
macro_rules! mcp_request_span {
    ($method:expr) => {
        ::tracing::info_span!("mcp_request", method = $method)
    };
}

/// Create a span for a tool execution.
#[macro_export]
macro_rules! tool_execution_span {
    ($tool:expr) => {
        ::tracing::info_span!("tool_execution", tool = $tool)
    };
}

/// Create a span for a provider API call.
#[macro_export]
macro_rules! provider_call_span {
    ($provider:expr, $model:expr) => {
        ::tracing::info_span!("provider_call", provider = $provider, model = $model)
    };
}

/// Create a span for an agent execution.
#[macro_export]
macro_rules! agent_execution_span {
    ($agent_type:expr) => {
        ::tracing::info_span!("agent_execution", agent_type = $agent_type)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_format_from_env_str() {
        assert_eq!(TraceFormat::from_env_str("json"), TraceFormat::Json);
        assert_eq!(TraceFormat::from_env_str("JSON"), TraceFormat::Json);
        assert_eq!(TraceFormat::from_env_str("compact"), TraceFormat::Compact);
        assert_eq!(TraceFormat::from_env_str("pretty"), TraceFormat::Pretty);
        assert_eq!(TraceFormat::from_env_str("unknown"), TraceFormat::Pretty);
    }

    #[test]
    fn test_default_config() {
        let config = TracingConfig::default();
        assert_eq!(config.format, TraceFormat::Pretty);
        assert!(config.log_dir.is_none());
        assert_eq!(config.level, "info");
        assert!(config.with_timings);
        assert!(config.with_thread_names);
        assert!(!config.with_thread_ids);
        assert!(config.with_source_location);
    }

    #[test]
    fn test_config_from_env_defaults() {
        // Without env vars set, should get defaults.
        let config = TracingConfig::from_env();
        assert_eq!(config.format, TraceFormat::Pretty);
        assert_eq!(config.level, "info");
    }
}
