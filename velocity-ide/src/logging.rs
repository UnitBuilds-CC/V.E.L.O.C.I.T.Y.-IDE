//! Logging configuration for Velocity IDE.
//!
//! Provides structured logging setup with support for multiple output formats
//! and configurable log levels via environment variables.
//!
//! # Usage
//!
//! ```ignore
//! use velocity_ide::logging::init_logging;
//!
//! // Initialize with default settings (reads RUST_LOG env var)
//! init_logging()?;
//!
//! // Or with custom configuration
//! use velocity_ide::logging::LoggingConfig;
//! let config = LoggingConfig {
//!     level: "info".to_string(),
//!     format: LogFormat::Json,
//!     output: LogOutput::Stdout,
//! };
//! init_logging_with_config(config)?;
//! ```

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoggingError {
    #[error("Failed to initialize logger: {0}")]
    InitError(String),
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable format with timestamps and colors.
    Pretty,
    /// JSON structured format for log aggregation.
    JSON,
    /// Compact format without timestamps.
    Compact,
}

/// Log output destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogOutput {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Logging configuration.
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log level filter (trace, debug, info, warn, error).
    pub level: String,
    /// Output format.
    pub format: LogFormat,
    /// Output destination.
    pub output: LogOutput,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            format: LogFormat::Pretty,
            output: LogOutput::Stderr,
        }
    }
}

/// Initialize logging with default configuration.
///
/// Reads configuration from environment variables:
/// - `RUST_LOG`: Log level filter (default: "info")
/// - `VELOCITY_LOG_FORMAT`: Output format ("pretty", "json", "compact")
/// - `VELOCITY_LOG_OUTPUT`: Output destination ("stdout", "stderr")
///
/// # Errors
///
/// Returns an error if the logger cannot be initialized.
pub fn init_logging() -> Result<(), LoggingError> {
    let config = LoggingConfig::from_env();
    init_logging_with_config(config)
}

/// Initialize logging with custom configuration.
///
/// # Errors
///
/// Returns an error if the logger cannot be initialized.
pub fn init_logging_with_config(config: LoggingConfig) -> Result<(), LoggingError> {
    let mut builder = env_logger::Builder::new();

    // Parse and set filter level
    builder.parse_filters(&config.level);

    // Configure output target
    let target = match config.output {
        LogOutput::Stdout => env_logger::Target::Stdout,
        LogOutput::Stderr => env_logger::Target::Stderr,
    };
    builder.target(target);

    // Configure format
    match config.format {
        LogFormat::Pretty => {
            builder.format(|buf, record| {
                use std::io::Write;
                let timestamp = buf.timestamp();
                let level = record.level();
                let level_color = match level {
                    log::Level::Error => "\x1b[31m", // Red
                    log::Level::Warn => "\x1b[33m",  // Yellow
                    log::Level::Info => "\x1b[32m",  // Green
                    log::Level::Debug => "\x1b[34m", // Blue
                    log::Level::Trace => "\x1b[90m", // Gray
                };
                writeln!(
                    buf,
                    "{} {:5} {} \x1b[0m{}",
                    timestamp,
                    level_color,
                    level,
                    record.args()
                )
            });
        }
        LogFormat::JSON => {
            builder.format(|buf, record| {
                use std::io::Write;
                writeln!(
                    buf,
                    r#"{{"timestamp":"{}","level":"{}","target":"{}","message":"{}"}}"#,
                    buf.timestamp(),
                    record.level(),
                    record.target(),
                    record.args().to_string().replace('"', "\\\"")
                )
            });
        }
        LogFormat::Compact => {
            builder.format(|buf, record| {
                use std::io::Write;
                writeln!(buf, "[{:5}] {}", record.level(), record.args())
            });
        }
    }

    // Initialize the logger
    builder
        .try_init()
        .map_err(|e| LoggingError::InitError(e.to_string()))?;

    log::debug!("Logging initialized with level: {}", config.level);
    Ok(())
}

impl LoggingConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        let format = match std::env::var("VELOCITY_LOG_FORMAT")
            .unwrap_or_else(|_| "pretty".to_string())
            .to_lowercase()
            .as_str()
        {
            "json" => LogFormat::JSON,
            "compact" => LogFormat::Compact,
            _ => LogFormat::Pretty,
        };

        let output = match std::env::var("VELOCITY_LOG_OUTPUT")
            .unwrap_or_else(|_| "stderr".to_string())
            .to_lowercase()
            .as_str()
        {
            "stdout" => LogOutput::Stdout,
            _ => LogOutput::Stderr,
        };

        Self {
            level,
            format,
            output,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LoggingConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Pretty);
        assert_eq!(config.output, LogOutput::Stderr);
    }

    #[test]
    fn test_from_env_defaults() {
        // Clear env vars to test defaults
        std::env::remove_var("RUST_LOG");
        std::env::remove_var("VELOCITY_LOG_FORMAT");
        std::env::remove_var("VELOCITY_LOG_OUTPUT");

        let config = LoggingConfig::from_env();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Pretty);
        assert_eq!(config.output, LogOutput::Stderr);
    }
}
