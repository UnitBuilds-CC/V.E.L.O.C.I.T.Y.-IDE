//! Comprehensive error types for the V.E.L.O.C.I.T.Y.-IDE runtime.
//!
//! Every module defines its own error variant. All errors convert to
//! [`VelocityError`] for uniform handling at the CLI boundary.
//! Each variant carries an [`ErrorCode`] for machine-readable diagnostics
//! and optional suggestions for self-service recovery.

use std::path::PathBuf;

// ─── Error Codes ───────────────────────────────────────────────────────────

/// Machine-readable error codes for documentation lookup and tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // Config / setup (E1xx)
    ConfigNotFound,
    ConfigInvalid,
    ConfigMissingKey,
    HomeDirectoryUnavailable,

    // Router / network (E2xx)
    RouterUnreachable,
    RouterTimeout,
    RouterAuthFailed,
    RouterRateLimited,
    RouterServerError,
    RouterResponseInvalid,

    // Model / weights (E3xx)
    ModelDirNotFound,
    TokenizerNotFound,
    WeightLoadFailed,
    WeightShapeMismatch,
    ConfigInvalidArch,

    // Tokenizer (E4xx)
    TokenizerFileInvalid,
    TokenizerMergeFailed,
    TokenizerUnknownToken,

    // Provider (E5xx)
    ProviderKeyInvalid,
    ProviderRateLimited,
    ProviderUnavailable,
    ProviderUsageApiUnsupported,

    // Pipeline / compiler (E6xx)
    CompileFailed,
    PipelineExecutionFailed,
    SandBoxViolation,

    // SiteMap (E7xx)
    SiteMapCorrupt,
    SiteMapVersionMismatch,
    SiteMapIoError,

    // Assignment (E8xx)
    AssignmentFailed,
    AssignmentCostExceeded,
    AssignmentTimeout,

    // General
    IoError,
    InvalidInput,
    InternalError,
}

impl ErrorCode {
    /// Numeric code for display (e.g. "E201").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConfigNotFound => "E100",
            Self::ConfigInvalid => "E101",
            Self::ConfigMissingKey => "E102",
            Self::HomeDirectoryUnavailable => "E103",

            Self::RouterUnreachable => "E200",
            Self::RouterTimeout => "E201",
            Self::RouterAuthFailed => "E202",
            Self::RouterRateLimited => "E203",
            Self::RouterServerError => "E204",
            Self::RouterResponseInvalid => "E205",

            Self::ModelDirNotFound => "E300",
            Self::TokenizerNotFound => "E301",
            Self::WeightLoadFailed => "E302",
            Self::WeightShapeMismatch => "E303",
            Self::ConfigInvalidArch => "E304",

            Self::TokenizerFileInvalid => "E400",
            Self::TokenizerMergeFailed => "E401",
            Self::TokenizerUnknownToken => "E402",

            Self::ProviderKeyInvalid => "E500",
            Self::ProviderRateLimited => "E501",
            Self::ProviderUnavailable => "E502",
            Self::ProviderUsageApiUnsupported => "E503",

            Self::CompileFailed => "E600",
            Self::PipelineExecutionFailed => "E601",
            Self::SandBoxViolation => "E602",

            Self::SiteMapCorrupt => "E700",
            Self::SiteMapVersionMismatch => "E701",
            Self::SiteMapIoError => "E702",

            Self::AssignmentFailed => "E800",
            Self::AssignmentCostExceeded => "E801",
            Self::AssignmentTimeout => "E802",

            Self::IoError => "E900",
            Self::InvalidInput => "E901",
            Self::InternalError => "E999",
        }
    }

    /// Documentation URL for this error code.
    pub fn doc_url(&self) -> String {
        format!("https://velocity.dev/errors/{}", self.as_str())
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Unified Error Type ────────────────────────────────────────────────────

/// Top-level error type for the Velocity IDE.
///
/// All module-specific errors convert into this type via `From` impls,
/// enabling uniform error handling at the CLI boundary with `anyhow`.
#[derive(Debug, thiserror::Error)]
#[error("{code} — {message}")]
pub struct VelocityError {
    pub code: ErrorCode,
    pub message: String,
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
    pub suggestion: Option<String>,
    pub context: Vec<(String, String)>,
}

impl VelocityError {
    /// Create a new error with code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
            suggestion: None,
            context: Vec::new(),
        }
    }

    /// Attach a source error.
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Attach a recovery suggestion.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Attach a context key-value pair (e.g. file path, model name).
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push((key.into(), value.into()));
        self
    }

    /// Format the error with all details for CLI display.
    pub fn format_detailed(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Error {}: {}\n", self.code, self.message));

        for (key, val) in &self.context {
            out.push_str(&format!("  {}: {}\n", key, val));
        }

        if let Some(ref suggestion) = self.suggestion {
            out.push_str(&format!("  Suggestion: {}\n", suggestion));
        }

        out.push_str(&format!("  Docs: {}\n", self.code.doc_url()));

        if let Some(ref source) = self.source {
            out.push_str(&format!("  Caused by: {}\n", source));
        }

        out
    }
}

// ─── Module-Specific Error Enums ───────────────────────────────────────────

/// Errors from the Velocity Router HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("router unreachable at {url}")]
    Unreachable { url: String },

    #[error("request timed out after {secs}s")]
    Timeout { secs: u64 },

    #[error("authentication failed — check your API key")]
    AuthFailed,

    #[error("rate limited — retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("server error: HTTP {status}")]
    ServerError { status: u16 },

    #[error("invalid response: {detail}")]
    ResponseInvalid { detail: String },

    #[error("max retries exceeded ({attempts} attempts)")]
    MaxRetriesExceeded { attempts: u32 },
}

impl RouterError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::Unreachable { url } => VelocityError::new(ErrorCode::RouterUnreachable, self.to_string())
                .with_suggestion("Check that the router is running and VELOCITY_BASE_URL is correct.")
                .with_context("url", url.clone()),
            Self::Timeout { secs: _ } => VelocityError::new(ErrorCode::RouterTimeout, self.to_string())
                .with_suggestion("The router may be overloaded. Try again in a few seconds."),
            Self::AuthFailed => VelocityError::new(ErrorCode::RouterAuthFailed, self.to_string())
                .with_suggestion("Run `velocity-ide login` to reconfigure your API key."),
            Self::RateLimited { retry_after_secs } => VelocityError::new(ErrorCode::RouterRateLimited, self.to_string())
                .with_suggestion(format!("Wait {}s before retrying, or upgrade your tier.", retry_after_secs)),
            Self::ServerError { status: _ } => VelocityError::new(ErrorCode::RouterServerError, self.to_string()),
            Self::ResponseInvalid { detail } => VelocityError::new(ErrorCode::RouterResponseInvalid, self.to_string())
                .with_context("detail", detail.clone()),
            Self::MaxRetriesExceeded { attempts } => VelocityError::new(ErrorCode::RouterServerError, self.to_string())
                .with_context("attempts", attempts.to_string()),
        }
    }
}

/// Errors from model weight loading and configuration.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model directory not found: {path:?}")]
    DirNotFound { path: PathBuf },

    #[error("tokenizer not found (searched: {searched:?})")]
    TokenizerNotFound { searched: Vec<PathBuf> },

    #[error("failed to load weights: {detail}")]
    WeightLoadFailed { detail: String },

    #[error("weight shape mismatch: expected {expected}, got {actual}")]
    WeightShapeMismatch { expected: String, actual: String },

    #[error("unknown architecture '{arch}' — use 'qwen05' or 'bitnet3b'")]
    InvalidArch { arch: String },
}

impl ModelError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::DirNotFound { path } => VelocityError::new(ErrorCode::ModelDirNotFound, self.to_string())
                .with_suggestion("Use --model <dir> to specify the model directory.")
                .with_context("path", format!("{:?}", path)),
            Self::TokenizerNotFound { searched } => VelocityError::new(ErrorCode::TokenizerNotFound, self.to_string())
                .with_suggestion("Use --tokenizer <file> or place tokenizer.json next to the model.")
                .with_context("searched", format!("{:?}", searched)),
            Self::WeightLoadFailed { detail } => VelocityError::new(ErrorCode::WeightLoadFailed, self.to_string())
                .with_context("detail", detail.clone()),
            Self::WeightShapeMismatch { expected, actual } => VelocityError::new(ErrorCode::WeightShapeMismatch, self.to_string())
                .with_context("expected", expected.clone())
                .with_context("actual", actual.clone()),
            Self::InvalidArch { arch: _ } => VelocityError::new(ErrorCode::ConfigInvalidArch, self.to_string())
                .with_suggestion("Supported architectures: qwen05, bitnet3b"),
        }
    }
}

/// Errors from provider API interactions.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API key invalid for provider '{provider}'")]
    KeyInvalid { provider: String },

    #[error("rate limited by provider '{provider}'")]
    RateLimited { provider: String },

    #[error("provider '{provider}' unavailable: {detail}")]
    Unavailable { provider: String, detail: String },

    #[error("provider '{provider}' does not expose a usage API")]
    UsageApiUnsupported { provider: String },
}

impl ProviderError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::KeyInvalid { provider } => VelocityError::new(ErrorCode::ProviderKeyInvalid, self.to_string())
                .with_suggestion(format!("Run `velocity-ide providers remove --provider {}` and re-add the key.", provider)),
            Self::RateLimited { provider: _ } => VelocityError::new(ErrorCode::ProviderRateLimited, self.to_string())
                .with_suggestion("Wait before retrying, or consider routing through the Velocity router."),
            Self::Unavailable { provider, detail } => VelocityError::new(ErrorCode::ProviderUnavailable, self.to_string())
                .with_context("provider", provider.clone())
                .with_context("detail", detail.clone()),
            Self::UsageApiUnsupported { provider } => VelocityError::new(ErrorCode::ProviderUsageApiUnsupported, self.to_string())
                .with_suggestion(format!("{} does not expose a usage API. Check the provider's dashboard directly.", provider)),
        }
    }
}

/// Errors from the NDA compiler pipeline.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("compilation failed: {detail}")]
    CompileFailed { detail: String },

    #[error("pipeline execution failed: {detail}")]
    ExecutionFailed { detail: String },

    #[error("sandbox violation: {detail}")]
    SandBoxViolation { detail: String },
}

impl PipelineError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::CompileFailed { detail } => VelocityError::new(ErrorCode::CompileFailed, self.to_string())
                .with_context("detail", detail.clone()),
            Self::ExecutionFailed { detail } => VelocityError::new(ErrorCode::PipelineExecutionFailed, self.to_string())
                .with_context("detail", detail.clone()),
            Self::SandBoxViolation { detail: _ } => VelocityError::new(ErrorCode::SandBoxViolation, self.to_string())
                .with_suggestion("The program attempted an operation not allowed in the sandbox."),
        }
    }
}

// ─── From impls for seamless conversion ────────────────────────────────────

impl From<RouterError> for VelocityError {
    fn from(e: RouterError) -> Self {
        e.to_velocity_error()
    }
}

impl From<ModelError> for VelocityError {
    fn from(e: ModelError) -> Self {
        e.to_velocity_error()
    }
}

impl From<ProviderError> for VelocityError {
    fn from(e: ProviderError) -> Self {
        e.to_velocity_error()
    }
}

impl From<PipelineError> for VelocityError {
    fn from(e: PipelineError) -> Self {
        e.to_velocity_error()
    }
}

// ─── Legacy compatibility ──────────────────────────────────────────────────

/// Errors produced by the NDA lexer, parser, and WASM runner.
/// (Retained for backward compatibility with the compiler pipeline.)
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// Lexer error (unexpected character, invalid literal).
    #[error("Lexer error: {0}")]
    Lexer(String),

    /// Parser error (unexpected token, missing EOF, type mismatch).
    #[error("Parser error: {0}")]
    Parser(String),

    /// WASM validation or execution error.
    #[error("WASM error: {0}")]
    Wasm(String),

    /// I/O error reading source files.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<CompileError> for String {
    fn from(e: CompileError) -> String {
        e.to_string()
    }
}

impl From<CompileError> for PipelineError {
    fn from(e: CompileError) -> Self {
        PipelineError::CompileFailed {
            detail: e.to_string(),
        }
    }
}

impl From<CompileError> for VelocityError {
    fn from(e: CompileError) -> Self {
        PipelineError::from(e).to_velocity_error()
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_display() {
        assert_eq!(ErrorCode::RouterUnreachable.as_str(), "E200");
        assert_eq!(ErrorCode::ModelDirNotFound.as_str(), "E300");
        assert_eq!(ErrorCode::InternalError.as_str(), "E999");
    }

    #[test]
    fn error_code_doc_url() {
        assert_eq!(
            ErrorCode::RouterAuthFailed.doc_url(),
            "https://velocity.dev/errors/E202"
        );
    }

    #[test]
    fn velocity_error_format_detailed() {
        let err = VelocityError::new(ErrorCode::ModelDirNotFound, "model directory not found: /bad/path")
            .with_suggestion("Use --model <dir> to specify the model directory.")
            .with_context("path", "/bad/path");

        let formatted = err.format_detailed();
        assert!(formatted.contains("E300"));
        assert!(formatted.contains("model directory not found"));
        assert!(formatted.contains("Suggestion:"));
        assert!(formatted.contains("path: /bad/path"));
        assert!(formatted.contains("Docs:"));
    }

    #[test]
    fn velocity_error_with_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = VelocityError::new(ErrorCode::IoError, "I/O error")
            .with_source(io_err);
        assert!(err.source.is_some());
        let formatted = err.format_detailed();
        assert!(formatted.contains("Caused by:"));
    }

    #[test]
    fn router_error_converts() {
        let re = RouterError::Unreachable { url: "http://localhost:8787".into() };
        let ve: VelocityError = re.into();
        assert_eq!(ve.code, ErrorCode::RouterUnreachable);
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn model_error_converts() {
        let me = ModelError::InvalidArch { arch: "llama3".into() };
        let ve: VelocityError = me.into();
        assert_eq!(ve.code, ErrorCode::ConfigInvalidArch);
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn provider_error_converts() {
        let pe = ProviderError::UsageApiUnsupported { provider: "mistral".into() };
        let ve: VelocityError = pe.into();
        assert_eq!(ve.code, ErrorCode::ProviderUsageApiUnsupported);
    }

    #[test]
    fn compile_error_converts_to_pipeline() {
        let ce = CompileError::Lexer("unexpected char '@'".into());
        let pe: PipelineError = ce.into();
        match pe {
            PipelineError::CompileFailed { detail } => {
                assert!(detail.contains("unexpected char"));
            }
            _ => panic!("expected CompileFailed"),
        }
    }

    #[test]
    fn compile_error_converts_to_velocity() {
        let ce = CompileError::Parser("unexpected EOF".into());
        let ve: VelocityError = ce.into();
        assert_eq!(ve.code, ErrorCode::CompileFailed);
    }

    #[test]
    fn error_context_accumulates() {
        let err = VelocityError::new(ErrorCode::InternalError, "test")
            .with_context("key1", "val1")
            .with_context("key2", "val2")
            .with_context("key3", "val3");
        assert_eq!(err.context.len(), 3);
    }
}
