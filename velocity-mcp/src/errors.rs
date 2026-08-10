//! Centralised error types for the V.E.L.O.C.I.T.Y. MCP server.
//!
//! Every public tool handler ultimately funnels through [`ToolError`], which
//! provides structured, machine-readable variants for the most common failure
//! modes (unknown tool, bad arguments, I/O, governance denial) plus a catch-all
//! `Internal` for unexpected issues.

use std::fmt;

/// Top-level error returned by the tool dispatch pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The requested tool name is not registered.
    #[error("unknown tool: {0}")]
    ToolNotFound(String),

    /// The caller supplied invalid or missing arguments.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Tool governance policy denied or parked the call.
    #[error("governance denied: {0}")]
    GovernanceDenied(String),

    /// Filesystem / process I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialisation failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Catch-all for unexpected errors.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<String> for ToolError {
    fn from(msg: String) -> Self {
        ToolError::Internal(msg)
    }
}

impl From<&str> for ToolError {
    fn from(msg: &str) -> Self {
        ToolError::Internal(msg.to_owned())
    }
}

/// Convert a boxed dynamic error into a [`ToolError::Internal`] while
/// preserving the original message for diagnostics.
impl From<Box<dyn std::error::Error>> for ToolError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        ToolError::Internal(e.to_string())
    }
}

/// Result type alias for tool handlers.
pub type ToolResult<T> = Result<T, ToolError>;
