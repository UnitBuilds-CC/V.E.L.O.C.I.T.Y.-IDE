//! Comprehensive error types for the V.E.L.O.C.I.T.Y.-IDE runtime.
//!
//! Every module defines its own error variant. All errors convert to
//! [`VelocityError`] for uniform handling at the CLI boundary.
//! Each variant carries an [`ErrorCode`] for machine-readable diagnostics
//! and optional suggestions for self-service recovery.

use std::path::PathBuf;
use serde::Serialize;

// ─── Error Codes ───────────────────────────────────────────────────────────

/// Machine-readable error codes for documentation lookup and tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

    // JIT / compiler (E10xx)
    JitCompilationFailed,
    JitOptimizationFailed,
    JitSandboxEscape,

    // Wiki (E11xx)
    WikiIndexCorrupt,
    WikiSearchFailed,

    // General
    IoError,
    InvalidInput,
    InternalError,
}

impl ErrorCode {
    /// Parse an error code from its string representation (e.g. "E200").
    pub fn from_code_str(s: &str) -> Option<Self> {
        match s {
            "E100" => Some(Self::ConfigNotFound),
            "E101" => Some(Self::ConfigInvalid),
            "E102" => Some(Self::ConfigMissingKey),
            "E103" => Some(Self::HomeDirectoryUnavailable),
            "E200" => Some(Self::RouterUnreachable),
            "E201" => Some(Self::RouterTimeout),
            "E202" => Some(Self::RouterAuthFailed),
            "E203" => Some(Self::RouterRateLimited),
            "E204" => Some(Self::RouterServerError),
            "E205" => Some(Self::RouterResponseInvalid),
            "E300" => Some(Self::ModelDirNotFound),
            "E301" => Some(Self::TokenizerNotFound),
            "E302" => Some(Self::WeightLoadFailed),
            "E303" => Some(Self::WeightShapeMismatch),
            "E304" => Some(Self::ConfigInvalidArch),
            "E400" => Some(Self::TokenizerFileInvalid),
            "E401" => Some(Self::TokenizerMergeFailed),
            "E402" => Some(Self::TokenizerUnknownToken),
            "E500" => Some(Self::ProviderKeyInvalid),
            "E501" => Some(Self::ProviderRateLimited),
            "E502" => Some(Self::ProviderUnavailable),
            "E503" => Some(Self::ProviderUsageApiUnsupported),
            "E600" => Some(Self::CompileFailed),
            "E601" => Some(Self::PipelineExecutionFailed),
            "E602" => Some(Self::SandBoxViolation),
            "E700" => Some(Self::SiteMapCorrupt),
            "E701" => Some(Self::SiteMapVersionMismatch),
            "E702" => Some(Self::SiteMapIoError),
            "E800" => Some(Self::AssignmentFailed),
            "E801" => Some(Self::AssignmentCostExceeded),
            "E802" => Some(Self::AssignmentTimeout),
            "E900" => Some(Self::IoError),
            "E901" => Some(Self::InvalidInput),
            "E999" => Some(Self::InternalError),
            "E1000" => Some(Self::JitCompilationFailed),
            "E1001" => Some(Self::JitOptimizationFailed),
            "E1002" => Some(Self::JitSandboxEscape),
            "E1100" => Some(Self::WikiIndexCorrupt),
            "E1101" => Some(Self::WikiSearchFailed),
            _ => None,
        }
    }

    /// Enumerate all known error codes (for documentation generation).
    pub fn all_codes() -> Vec<Self> {
        vec![
            Self::ConfigNotFound, Self::ConfigInvalid, Self::ConfigMissingKey,
            Self::HomeDirectoryUnavailable,
            Self::RouterUnreachable, Self::RouterTimeout, Self::RouterAuthFailed,
            Self::RouterRateLimited, Self::RouterServerError, Self::RouterResponseInvalid,
            Self::ModelDirNotFound, Self::TokenizerNotFound, Self::WeightLoadFailed,
            Self::WeightShapeMismatch, Self::ConfigInvalidArch,
            Self::TokenizerFileInvalid, Self::TokenizerMergeFailed, Self::TokenizerUnknownToken,
            Self::ProviderKeyInvalid, Self::ProviderRateLimited, Self::ProviderUnavailable,
            Self::ProviderUsageApiUnsupported,
            Self::CompileFailed, Self::PipelineExecutionFailed, Self::SandBoxViolation,
            Self::SiteMapCorrupt, Self::SiteMapVersionMismatch, Self::SiteMapIoError,
            Self::AssignmentFailed, Self::AssignmentCostExceeded, Self::AssignmentTimeout,
            Self::JitCompilationFailed, Self::JitOptimizationFailed, Self::JitSandboxEscape,
            Self::WikiIndexCorrupt, Self::WikiSearchFailed,
            Self::IoError, Self::InvalidInput, Self::InternalError,
        ]
    }

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

            Self::JitCompilationFailed => "E1000",
            Self::JitOptimizationFailed => "E1001",
            Self::JitSandboxEscape => "E1002",

            Self::WikiIndexCorrupt => "E1100",
            Self::WikiSearchFailed => "E1101",

            Self::IoError => "E900",
            Self::InvalidInput => "E901",
            Self::InternalError => "E999",
        }
    }

    /// Documentation URL for this error code.
    pub fn doc_url(&self) -> String {
        format!("https://velocity.dev/errors/{}", self.as_str())
    }

    /// Human-readable category name for grouping.
    pub fn category(&self) -> &'static str {
        match self {
            Self::ConfigNotFound | Self::ConfigInvalid | Self::ConfigMissingKey
            | Self::HomeDirectoryUnavailable => "config",

            Self::RouterUnreachable | Self::RouterTimeout | Self::RouterAuthFailed
            | Self::RouterRateLimited | Self::RouterServerError | Self::RouterResponseInvalid => "router",

            Self::ModelDirNotFound | Self::TokenizerNotFound | Self::WeightLoadFailed
            | Self::WeightShapeMismatch | Self::ConfigInvalidArch => "model",

            Self::TokenizerFileInvalid | Self::TokenizerMergeFailed
            | Self::TokenizerUnknownToken => "tokenizer",

            Self::ProviderKeyInvalid | Self::ProviderRateLimited | Self::ProviderUnavailable
            | Self::ProviderUsageApiUnsupported => "provider",

            Self::CompileFailed | Self::PipelineExecutionFailed | Self::SandBoxViolation => "pipeline",

            Self::SiteMapCorrupt | Self::SiteMapVersionMismatch | Self::SiteMapIoError => "sitemap",

            Self::AssignmentFailed | Self::AssignmentCostExceeded | Self::AssignmentTimeout => "assignment",

            Self::JitCompilationFailed | Self::JitOptimizationFailed | Self::JitSandboxEscape => "jit",

            Self::WikiIndexCorrupt | Self::WikiSearchFailed => "wiki",

            Self::IoError | Self::InvalidInput | Self::InternalError => "general",
        }
    }

    /// Whether this error is transient and retrying may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RouterUnreachable
                | Self::RouterTimeout
                | Self::RouterRateLimited
                | Self::RouterServerError
                | Self::ProviderRateLimited
                | Self::ProviderUnavailable
                | Self::AssignmentTimeout
                | Self::JitOptimizationFailed
        )
    }

    /// Whether this error indicates a security or credential issue.
    pub fn is_security(&self) -> bool {
        matches!(
            self,
            Self::RouterAuthFailed
                | Self::ProviderKeyInvalid
                | Self::SandBoxViolation
                | Self::JitSandboxEscape
        )
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
#[derive(Debug, thiserror::Error, Serialize)]
pub struct VelocityError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip)]
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

    /// Whether this error is transient and retrying may succeed.
    pub fn is_retryable(&self) -> bool {
        self.code.is_retryable()
    }

    /// Whether this error indicates a security or credential issue.
    pub fn is_security(&self) -> bool {
        self.code.is_security()
    }

    /// Whether this error wraps an I/O error.
    pub fn is_io(&self) -> bool {
        self.code == ErrorCode::IoError
            || self.code == ErrorCode::SiteMapIoError
            || self.source.as_ref().map_or(false, |s| s.downcast_ref::<std::io::Error>().is_some())
    }

    /// Map to a process exit code for CLI usage.
    pub fn exit_code(&self) -> i32 {
        match self.code.category() {
            "config" => 2,
            "router" => 3,
            "model" => 4,
            "tokenizer" => 5,
            "provider" => 6,
            "pipeline" => 7,
            "sitemap" => 8,
            "assignment" => 9,
            "jit" => 10,
            "wiki" => 11,
            _ => {
                if self.is_security() { 13 } else { 1 }
            }
        }
    }

    /// Collect all source error messages in the chain.
    pub fn chain_sources(&self) -> Vec<String> {
        let mut msgs = Vec::new();
        if let Some(ref src) = self.source {
            msgs.push(src.to_string());
            let mut current: Option<&dyn std::error::Error> = Some(src.as_ref());
            while let Some(e) = current {
                current = std::error::Error::source(e);
                if let Some(next) = current {
                    msgs.push(next.to_string());
                }
            }
        }
        msgs
    }

    /// The error category (e.g. "router", "model", "pipeline").
    pub fn category(&self) -> &'static str {
        self.code.category()
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

impl std::fmt::Display for VelocityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} — {}", self.code, self.message)
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

/// Errors from the SiteMap module.
#[derive(Debug, thiserror::Error)]
pub enum SiteMapError {
    #[error("site map corrupt: {detail}")]
    Corrupt { detail: String },

    #[error("site map version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: String, actual: String },

    #[error("site map I/O error: {detail}")]
    IoError { detail: String },
}

impl SiteMapError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::Corrupt { detail } => VelocityError::new(ErrorCode::SiteMapCorrupt, self.to_string())
                .with_context("detail", detail.clone())
                .with_suggestion("Rebuild the site map with `velocity-ide index`."),
            Self::VersionMismatch { expected, actual } => VelocityError::new(ErrorCode::SiteMapVersionMismatch, self.to_string())
                .with_context("expected", expected.clone())
                .with_context("actual", actual.clone()),
            Self::IoError { detail } => VelocityError::new(ErrorCode::SiteMapIoError, self.to_string())
                .with_context("detail", detail.clone()),
        }
    }
}

/// Errors from the tokenizer module.
#[derive(Debug, thiserror::Error)]
pub enum TokenizerError {
    #[error("tokenizer file invalid: {detail}")]
    FileInvalid { detail: String },

    #[error("tokenizer merge failed: {detail}")]
    MergeFailed { detail: String },

    #[error("unknown token: {token}")]
    UnknownToken { token: String },
}

impl TokenizerError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::FileInvalid { detail } => VelocityError::new(ErrorCode::TokenizerFileInvalid, self.to_string())
                .with_context("detail", detail.clone()),
            Self::MergeFailed { detail } => VelocityError::new(ErrorCode::TokenizerMergeFailed, self.to_string())
                .with_context("detail", detail.clone()),
            Self::UnknownToken { token } => VelocityError::new(ErrorCode::TokenizerUnknownToken, self.to_string())
                .with_context("token", token.clone()),
        }
    }
}

/// Errors from the sandbox execution module.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox violation: {detail}")]
    Violation { detail: String },

    #[error("sandbox execution timeout after {secs}s")]
    Timeout { secs: u64 },

    #[error("sandbox resource limit exceeded: {resource}")]
    ResourceLimit { resource: String },
}

impl SandboxError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::Violation { detail } => VelocityError::new(ErrorCode::SandBoxViolation, self.to_string())
                .with_context("detail", detail.clone()),
            Self::Timeout { secs } => VelocityError::new(ErrorCode::PipelineExecutionFailed, self.to_string())
                .with_context("timeout_secs", secs.to_string())
                .with_suggestion("The sandbox operation took too long. Consider breaking it into smaller steps."),
            Self::ResourceLimit { resource } => VelocityError::new(ErrorCode::SandBoxViolation, self.to_string())
                .with_context("resource", resource.clone()),
        }
    }
}

/// Errors from the credential guard / security boundary.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("credential not found: {key}")]
    NotFound { key: String },

    #[error("credential boundary violation: {detail}")]
    BoundaryViolation { detail: String },

    #[error("credential expired: {key}")]
    Expired { key: String },
}

impl CredentialError {
    pub fn to_velocity_error(&self) -> VelocityError {
        match self {
            Self::NotFound { key } => VelocityError::new(ErrorCode::ConfigMissingKey, self.to_string())
                .with_context("key", key.clone())
                .with_suggestion("Run `velocity-ide login` to configure credentials."),
            Self::BoundaryViolation { detail } => VelocityError::new(ErrorCode::SandBoxViolation, self.to_string())
                .with_context("detail", detail.clone())
                .with_suggestion("A process attempted to access credentials outside the security boundary."),
            Self::Expired { key } => VelocityError::new(ErrorCode::RouterAuthFailed, self.to_string())
                .with_context("key", key.clone())
                .with_suggestion("Your credentials have expired. Run `velocity-ide login` to refresh."),
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

impl From<SiteMapError> for VelocityError {
    fn from(e: SiteMapError) -> Self {
        e.to_velocity_error()
    }
}

impl From<TokenizerError> for VelocityError {
    fn from(e: TokenizerError) -> Self {
        e.to_velocity_error()
    }
}

impl From<SandboxError> for VelocityError {
    fn from(e: SandboxError) -> Self {
        e.to_velocity_error()
    }
}

impl From<CredentialError> for VelocityError {
    fn from(e: CredentialError) -> Self {
        e.to_velocity_error()
    }
}

// ─── Error Summary ─────────────────────────────────────────────────────────

/// Summary of a batch of errors for structured reporting.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorSummary {
    pub total: usize,
    pub by_category: Vec<(String, usize)>,
    pub retryable_count: usize,
    pub security_count: usize,
    pub unique_codes: Vec<String>,
}

/// Produce a summary from a slice of errors.
pub fn summarize_errors(errors: &[VelocityError]) -> ErrorSummary {
    use std::collections::HashMap;
    let mut cat_counts: HashMap<String, usize> = HashMap::new();
    let mut unique = std::collections::BTreeSet::new();
    let mut retryable = 0;
    let mut security = 0;
    for e in errors {
        *cat_counts.entry(e.category().to_string()).or_insert(0) += 1;
        unique.insert(e.code.as_str().to_string());
        if e.is_retryable() { retryable += 1; }
        if e.is_security() { security += 1; }
    }
    let mut by_category: Vec<(String, usize)> = cat_counts.into_iter().collect();
    by_category.sort_by(|a, b| b.1.cmp(&a.1));
    ErrorSummary {
        total: errors.len(),
        by_category,
        retryable_count: retryable,
        security_count: security,
        unique_codes: unique.into_iter().collect(),
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

    #[test]
    fn error_code_category() {
        assert_eq!(ErrorCode::RouterTimeout.category(), "router");
        assert_eq!(ErrorCode::ModelDirNotFound.category(), "model");
        assert_eq!(ErrorCode::CompileFailed.category(), "pipeline");
        assert_eq!(ErrorCode::IoError.category(), "general");
    }

    #[test]
    fn error_code_is_retryable() {
        assert!(ErrorCode::RouterTimeout.is_retryable());
        assert!(ErrorCode::RouterRateLimited.is_retryable());
        assert!(ErrorCode::ProviderUnavailable.is_retryable());
        assert!(!ErrorCode::ConfigNotFound.is_retryable());
        assert!(!ErrorCode::CompileFailed.is_retryable());
    }

    #[test]
    fn error_code_is_security() {
        assert!(ErrorCode::RouterAuthFailed.is_security());
        assert!(ErrorCode::ProviderKeyInvalid.is_security());
        assert!(ErrorCode::SandBoxViolation.is_security());
        assert!(!ErrorCode::RouterTimeout.is_security());
    }

    #[test]
    fn velocity_error_is_retryable_delegates() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "timeout");
        assert!(err.is_retryable());
        let err2 = VelocityError::new(ErrorCode::CompileFailed, "fail");
        assert!(!err2.is_retryable());
    }

    #[test]
    fn velocity_error_serializes() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "timed out")
            .with_suggestion("try again")
            .with_context("url", "http://localhost");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("RouterTimeout"));
        assert!(json.contains("timed out"));
        assert!(json.contains("try again"));
        // source field is skipped via #[serde(skip)]
        assert!(!json.contains("\"source\""));
    }

    #[test]
    fn sitemap_error_converts() {
        let se = SiteMapError::Corrupt { detail: "checksum mismatch".into() };
        let ve: VelocityError = se.into();
        assert_eq!(ve.code, ErrorCode::SiteMapCorrupt);
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn tokenizer_error_converts() {
        let te = TokenizerError::UnknownToken { token: "<unk>".into() };
        let ve: VelocityError = te.into();
        assert_eq!(ve.code, ErrorCode::TokenizerUnknownToken);
    }

    #[test]
    fn sandbox_error_converts() {
        let se = SandboxError::Timeout { secs: 30 };
        let ve: VelocityError = se.into();
        assert_eq!(ve.code, ErrorCode::PipelineExecutionFailed);
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn credential_error_converts() {
        let ce = CredentialError::BoundaryViolation { detail: "env var leak".into() };
        let ve: VelocityError = ce.into();
        assert_eq!(ve.code, ErrorCode::SandBoxViolation);
        assert!(ve.is_security());
    }

    #[test]
    fn credential_error_expired() {
        let ce = CredentialError::Expired { key: "api_key".into() };
        let ve: VelocityError = ce.into();
        assert_eq!(ve.code, ErrorCode::RouterAuthFailed);
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn error_code_from_code_str_roundtrip() {
        for code in ErrorCode::all_codes() {
            let s = code.as_str();
            let parsed = ErrorCode::from_code_str(s).unwrap();
            assert_eq!(parsed, code, "roundtrip failed for {}", s);
        }
    }

    #[test]
    fn error_code_from_code_str_unknown() {
        assert!(ErrorCode::from_code_str("E9999").is_none());
        assert!(ErrorCode::from_code_str("").is_none());
    }

    #[test]
    fn all_codes_has_no_duplicates() {
        let codes = ErrorCode::all_codes();
        let mut strs: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
        let before = strs.len();
        strs.sort();
        strs.dedup();
        assert_eq!(before, strs.len(), "duplicate error codes found");
    }

    #[test]
    fn new_jit_codes_work() {
        assert_eq!(ErrorCode::JitCompilationFailed.as_str(), "E1000");
        assert_eq!(ErrorCode::JitCompilationFailed.category(), "jit");
        assert!(!ErrorCode::JitCompilationFailed.is_retryable());
        assert_eq!(ErrorCode::JitSandboxEscape.category(), "jit");
        assert!(ErrorCode::JitSandboxEscape.is_security());
    }

    #[test]
    fn new_wiki_codes_work() {
        assert_eq!(ErrorCode::WikiIndexCorrupt.as_str(), "E1100");
        assert_eq!(ErrorCode::WikiSearchFailed.category(), "wiki");
    }

    #[test]
    fn velocity_error_exit_code() {
        let config_err = VelocityError::new(ErrorCode::ConfigNotFound, "no config");
        assert_eq!(config_err.exit_code(), 2);
        let router_err = VelocityError::new(ErrorCode::RouterTimeout, "timeout");
        assert_eq!(router_err.exit_code(), 3);
        let sec_err = VelocityError::new(ErrorCode::SandBoxViolation, "violation");
        assert_eq!(sec_err.exit_code(), 7); // pipeline category
    }

    #[test]
    fn velocity_error_is_io() {
        let io_err = VelocityError::new(ErrorCode::IoError, "io");
        assert!(io_err.is_io());
        let non_io = VelocityError::new(ErrorCode::ConfigNotFound, "no config");
        assert!(!non_io.is_io());
        let with_io_source = VelocityError::new(ErrorCode::InternalError, "wrapped")
            .with_source(std::io::Error::new(std::io::ErrorKind::Other, "inner"));
        assert!(with_io_source.is_io());
    }

    #[test]
    fn velocity_error_chain_sources() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = VelocityError::new(ErrorCode::IoError, "read failed")
            .with_source(io_err);
        let chain = err.chain_sources();
        assert_eq!(chain.len(), 1);
        assert!(chain[0].contains("file missing"));
    }

    #[test]
    fn summarize_errors_basic() {
        let errors = vec![
            VelocityError::new(ErrorCode::RouterTimeout, "t1"),
            VelocityError::new(ErrorCode::RouterTimeout, "t2"),
            VelocityError::new(ErrorCode::ConfigNotFound, "c1"),
            VelocityError::new(ErrorCode::SandBoxViolation, "s1"),
        ];
        let summary = summarize_errors(&errors);
        assert_eq!(summary.total, 4);
        assert_eq!(summary.retryable_count, 2);
        assert_eq!(summary.security_count, 1);
        assert!(summary.unique_codes.contains(&"E201".to_string()));
        assert!(summary.unique_codes.contains(&"E100".to_string()));
        // router should have highest count
        assert_eq!(summary.by_category[0].0, "router");
        assert_eq!(summary.by_category[0].1, 2);
    }

    #[test]
    fn summarize_errors_empty() {
        let summary = summarize_errors(&[]);
        assert_eq!(summary.total, 0);
        assert!(summary.by_category.is_empty());
        assert_eq!(summary.retryable_count, 0);
    }

    // ─── ErrorCode Additional ───────────────────────────────────────────────

    #[test]
    fn error_code_all_categories_covered() {
        for code in ErrorCode::all_codes() {
            let cat = code.category();
            assert!(["config","router","model","tokenizer","provider","pipeline","sitemap","assignment","jit","wiki","general"].contains(&cat),
                "unknown category '{}' for {:?}", cat, code);
        }
    }

    #[test]
    fn error_code_assignment_category() {
        assert_eq!(ErrorCode::AssignmentFailed.category(), "assignment");
        assert_eq!(ErrorCode::AssignmentCostExceeded.category(), "assignment");
        assert_eq!(ErrorCode::AssignmentTimeout.category(), "assignment");
        assert!(ErrorCode::AssignmentTimeout.is_retryable());
    }

    #[test]
    fn error_code_provider_category() {
        assert_eq!(ErrorCode::ProviderKeyInvalid.category(), "provider");
        assert_eq!(ErrorCode::ProviderRateLimited.category(), "provider");
        assert!(ErrorCode::ProviderRateLimited.is_retryable());
        assert!(ErrorCode::ProviderKeyInvalid.is_security());
    }

    #[test]
    fn error_code_sitemap_category() {
        assert_eq!(ErrorCode::SiteMapCorrupt.category(), "sitemap");
        assert_eq!(ErrorCode::SiteMapVersionMismatch.category(), "sitemap");
        assert_eq!(ErrorCode::SiteMapIoError.category(), "sitemap");
        assert!(!ErrorCode::SiteMapCorrupt.is_retryable());
    }

    #[test]
    fn error_code_jit_full() {
        assert!(ErrorCode::JitOptimizationFailed.is_retryable());
        assert!(!ErrorCode::JitCompilationFailed.is_security());
        assert_eq!(ErrorCode::JitSandboxEscape.as_str(), "E1002");
    }

    #[test]
    fn error_code_display_trait() {
        let code = ErrorCode::RouterTimeout;
        let s = format!("{}", code);
        assert_eq!(s, "E201");
    }

    #[test]
    fn error_code_doc_url_format() {
        for code in ErrorCode::all_codes() {
            let url = code.doc_url();
            assert!(url.starts_with("https://velocity.dev/errors/"));
            assert!(url.ends_with(code.as_str()));
        }
    }

    // ─── Module Error Variant Display ───────────────────────────────────────

    #[test]
    fn router_error_display_all_variants() {
        let e = RouterError::Unreachable { url: "http://x".into() };
        assert!(e.to_string().contains("http://x"));
        let e = RouterError::Timeout { secs: 30 };
        assert!(e.to_string().contains("30s"));
        let e = RouterError::AuthFailed;
        assert!(e.to_string().contains("authentication failed"));
        let e = RouterError::RateLimited { retry_after_secs: 60 };
        assert!(e.to_string().contains("60s"));
        let e = RouterError::ServerError { status: 503 };
        assert!(e.to_string().contains("503"));
        let e = RouterError::ResponseInvalid { detail: "bad json".into() };
        assert!(e.to_string().contains("bad json"));
        let e = RouterError::MaxRetriesExceeded { attempts: 5 };
        assert!(e.to_string().contains("5 attempts"));
    }

    #[test]
    fn router_error_all_variants_convert() {
        let variants: Vec<RouterError> = vec![
            RouterError::Unreachable { url: "u".into() },
            RouterError::Timeout { secs: 1 },
            RouterError::AuthFailed,
            RouterError::RateLimited { retry_after_secs: 1 },
            RouterError::ServerError { status: 500 },
            RouterError::ResponseInvalid { detail: "d".into() },
            RouterError::MaxRetriesExceeded { attempts: 3 },
        ];
        for v in variants {
            let ve = v.to_velocity_error();
            assert!(!ve.message.is_empty());
        }
    }

    #[test]
    fn model_error_display_all_variants() {
        let e = ModelError::DirNotFound { path: "/bad".into() };
        assert!(e.to_string().contains("/bad"));
        let e = ModelError::TokenizerNotFound { searched: vec!["/a".into()] };
        assert!(e.to_string().contains("searched"));
        let e = ModelError::WeightLoadFailed { detail: "corrupt".into() };
        assert!(e.to_string().contains("corrupt"));
        let e = ModelError::WeightShapeMismatch { expected: "4x4".into(), actual: "2x8".into() };
        assert!(e.to_string().contains("4x4"));
        assert!(e.to_string().contains("2x8"));
    }

    #[test]
    fn provider_error_all_variants_convert() {
        let variants: Vec<ProviderError> = vec![
            ProviderError::KeyInvalid { provider: "openai".into() },
            ProviderError::RateLimited { provider: "anthropic".into() },
            ProviderError::Unavailable { provider: "google".into(), detail: "down".into() },
            ProviderError::UsageApiUnsupported { provider: "mistral".into() },
        ];
        for v in variants {
            let ve: VelocityError = v.into();
            assert!(!ve.message.is_empty());
        }
    }

    #[test]
    fn pipeline_error_all_variants_convert() {
        let variants: Vec<PipelineError> = vec![
            PipelineError::CompileFailed { detail: "syntax".into() },
            PipelineError::ExecutionFailed { detail: "oom".into() },
            PipelineError::SandBoxViolation { detail: "fs access".into() },
        ];
        for v in variants {
            let ve = v.to_velocity_error();
            assert!(!ve.message.is_empty());
        }
        // SandBoxViolation should have a suggestion.
        let ve = PipelineError::SandBoxViolation { detail: "x".into() }.to_velocity_error();
        assert!(ve.suggestion.is_some());
        assert!(ve.is_security());
    }

    #[test]
    fn sitemap_error_all_variants() {
        let e = SiteMapError::VersionMismatch { expected: "2".into(), actual: "1".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::SiteMapVersionMismatch);
        assert!(ve.context.iter().any(|(k,_)| k == "expected"));
        assert!(ve.context.iter().any(|(k,_)| k == "actual"));

        let e = SiteMapError::IoError { detail: "disk full".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::SiteMapIoError);
        assert!(ve.is_io());
    }

    #[test]
    fn tokenizer_error_all_variants() {
        let variants: Vec<TokenizerError> = vec![
            TokenizerError::FileInvalid { detail: "bad header".into() },
            TokenizerError::MergeFailed { detail: "incompatible".into() },
            TokenizerError::UnknownToken { token: "<unk>".into() },
        ];
        for v in variants {
            let ve: VelocityError = v.into();
            assert!(!ve.message.is_empty());
        }
    }

    #[test]
    fn sandbox_error_all_variants() {
        let e = SandboxError::Violation { detail: "fs write".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::SandBoxViolation);
        assert!(ve.is_security());

        let e = SandboxError::ResourceLimit { resource: "memory".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::SandBoxViolation);

        let e = SandboxError::Timeout { secs: 60 };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::PipelineExecutionFailed);
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn credential_error_all_variants() {
        let e = CredentialError::NotFound { key: "api_key".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::ConfigMissingKey);
        assert!(ve.suggestion.is_some());

        let e = CredentialError::BoundaryViolation { detail: "leak".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::SandBoxViolation);
        assert!(ve.is_security());
    }

    // ─── VelocityError Additional ───────────────────────────────────────────

    #[test]
    fn velocity_error_display_trait() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "timed out");
        let s = format!("{}", err);
        assert!(s.contains("E201"));
        assert!(s.contains("timed out"));
    }

    #[test]
    fn velocity_error_is_security_delegates() {
        let err = VelocityError::new(ErrorCode::JitSandboxEscape, "escape");
        assert!(err.is_security());
        let err2 = VelocityError::new(ErrorCode::IoError, "io");
        assert!(!err2.is_security());
    }

    #[test]
    fn velocity_error_exit_code_all_categories() {
        let cases = vec![
            (ErrorCode::ConfigNotFound, 2),
            (ErrorCode::RouterTimeout, 3),
            (ErrorCode::ModelDirNotFound, 4),
            (ErrorCode::TokenizerFileInvalid, 5),
            (ErrorCode::ProviderKeyInvalid, 6),
            (ErrorCode::CompileFailed, 7),
            (ErrorCode::SiteMapCorrupt, 8),
            (ErrorCode::AssignmentFailed, 9),
            (ErrorCode::JitCompilationFailed, 10),
            (ErrorCode::WikiIndexCorrupt, 11),
        ];
        for (code, expected) in cases {
            let err = VelocityError::new(code, "test");
            assert_eq!(err.exit_code(), expected, "wrong exit code for {:?}", code);
        }
    }

    #[test]
    fn velocity_error_is_io_with_sitemap_io() {
        let err = VelocityError::new(ErrorCode::SiteMapIoError, "disk");
        assert!(err.is_io());
    }

    #[test]
    fn velocity_error_chain_sources_empty() {
        let err = VelocityError::new(ErrorCode::InternalError, "no source");
        assert!(err.chain_sources().is_empty());
    }

    #[test]
    fn velocity_error_format_detailed_no_optional() {
        let err = VelocityError::new(ErrorCode::IoError, "disk read failed");
        let formatted = err.format_detailed();
        assert!(formatted.contains("E900"));
        assert!(formatted.contains("disk read failed"));
        assert!(formatted.contains("Docs:"));
        // No suggestion or context lines.
        assert!(!formatted.contains("Suggestion:"));
    }

    #[test]
    fn velocity_error_serializes_with_context() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "timeout")
            .with_context("host", "router.velocity.io")
            .with_context("port", "443");
        let json = serde_json::to_string(&err).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["context"].as_array().unwrap().len(), 2);
    }

    // ─── CompileError Additional ────────────────────────────────────────────

    #[test]
    fn compile_error_display_variants() {
        let e = CompileError::Lexer("bad char".into());
        assert!(e.to_string().contains("Lexer error"));
        let e = CompileError::Parser("unexpected token".into());
        assert!(e.to_string().contains("Parser error"));
        let e = CompileError::Wasm("validation failed".into());
        assert!(e.to_string().contains("WASM error"));
    }

    #[test]
    fn compile_error_to_string_conversion() {
        let e = CompileError::Lexer("test".into());
        let s: String = e.into();
        assert!(s.contains("Lexer error"));
    }

    #[test]
    fn compile_error_io_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let ce = CompileError::Io(io_err);
        assert!(ce.to_string().contains("access denied"));
        let ve: VelocityError = ce.into();
        assert_eq!(ve.code, ErrorCode::CompileFailed);
    }

    // ─── ErrorSummary Additional ────────────────────────────────────────────

    #[test]
    fn summarize_errors_security_count() {
        let errors = vec![
            VelocityError::new(ErrorCode::RouterAuthFailed, "auth"),
            VelocityError::new(ErrorCode::ProviderKeyInvalid, "key"),
            VelocityError::new(ErrorCode::SandBoxViolation, "sandbox"),
            VelocityError::new(ErrorCode::JitSandboxEscape, "escape"),
        ];
        let summary = summarize_errors(&errors);
        assert_eq!(summary.security_count, 4);
    }

    #[test]
    fn summarize_errors_serializes() {
        let errors = vec![
            VelocityError::new(ErrorCode::RouterTimeout, "t"),
        ];
        let summary = summarize_errors(&errors);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"total\":1"));
        assert!(json.contains("\"retryable_count\":1"));
    }

    #[test]
    fn summarize_errors_unique_codes_sorted() {
        let errors = vec![
            VelocityError::new(ErrorCode::RouterTimeout, "a"),
            VelocityError::new(ErrorCode::RouterTimeout, "b"),
            VelocityError::new(ErrorCode::ConfigNotFound, "c"),
        ];
        let summary = summarize_errors(&errors);
        // BTreeSet ensures sorted order.
        assert_eq!(summary.unique_codes.len(), 2);
        assert!(summary.unique_codes[0] < summary.unique_codes[1]);
    }

    // ── Block 173: Comprehensive error coverage ────────────────────────────

    #[test]
    fn error_code_config_category_full() {
        assert_eq!(ErrorCode::ConfigNotFound.category(), "config");
        assert_eq!(ErrorCode::ConfigInvalid.category(), "config");
        assert_eq!(ErrorCode::ConfigMissingKey.category(), "config");
        assert_eq!(ErrorCode::HomeDirectoryUnavailable.category(), "config");
        // None are retryable or security
        assert!(!ErrorCode::ConfigNotFound.is_retryable());
        assert!(!ErrorCode::ConfigInvalid.is_security());
    }

    #[test]
    fn error_code_config_as_str() {
        assert_eq!(ErrorCode::ConfigNotFound.as_str(), "E100");
        assert_eq!(ErrorCode::ConfigInvalid.as_str(), "E101");
        assert_eq!(ErrorCode::ConfigMissingKey.as_str(), "E102");
        assert_eq!(ErrorCode::HomeDirectoryUnavailable.as_str(), "E103");
    }

    #[test]
    fn error_code_tokenizer_category() {
        assert_eq!(ErrorCode::TokenizerFileInvalid.category(), "tokenizer");
        assert_eq!(ErrorCode::TokenizerMergeFailed.category(), "tokenizer");
        assert_eq!(ErrorCode::TokenizerUnknownToken.category(), "tokenizer");
        assert!(!ErrorCode::TokenizerFileInvalid.is_retryable());
        assert!(!ErrorCode::TokenizerMergeFailed.is_security());
    }

    #[test]
    fn error_code_tokenizer_as_str() {
        assert_eq!(ErrorCode::TokenizerFileInvalid.as_str(), "E400");
        assert_eq!(ErrorCode::TokenizerMergeFailed.as_str(), "E401");
        assert_eq!(ErrorCode::TokenizerUnknownToken.as_str(), "E402");
    }

    #[test]
    fn error_code_general_category() {
        assert_eq!(ErrorCode::IoError.category(), "general");
        assert_eq!(ErrorCode::InvalidInput.category(), "general");
        assert_eq!(ErrorCode::InternalError.category(), "general");
        assert_eq!(ErrorCode::IoError.as_str(), "E900");
        assert_eq!(ErrorCode::InvalidInput.as_str(), "E901");
        assert_eq!(ErrorCode::InternalError.as_str(), "E999");
    }

    #[test]
    fn error_code_wiki_full() {
        assert_eq!(ErrorCode::WikiIndexCorrupt.as_str(), "E1100");
        assert_eq!(ErrorCode::WikiSearchFailed.as_str(), "E1101");
        assert_eq!(ErrorCode::WikiIndexCorrupt.category(), "wiki");
        assert_eq!(ErrorCode::WikiSearchFailed.category(), "wiki");
        assert!(!ErrorCode::WikiIndexCorrupt.is_retryable());
        assert!(!ErrorCode::WikiSearchFailed.is_security());
    }

    #[test]
    fn error_code_router_all_str() {
        assert_eq!(ErrorCode::RouterUnreachable.as_str(), "E200");
        assert_eq!(ErrorCode::RouterTimeout.as_str(), "E201");
        assert_eq!(ErrorCode::RouterAuthFailed.as_str(), "E202");
        assert_eq!(ErrorCode::RouterRateLimited.as_str(), "E203");
        assert_eq!(ErrorCode::RouterServerError.as_str(), "E204");
        assert_eq!(ErrorCode::RouterResponseInvalid.as_str(), "E205");
    }

    #[test]
    fn error_code_model_all_str() {
        assert_eq!(ErrorCode::ModelDirNotFound.as_str(), "E300");
        assert_eq!(ErrorCode::TokenizerNotFound.as_str(), "E301");
        assert_eq!(ErrorCode::WeightLoadFailed.as_str(), "E302");
        assert_eq!(ErrorCode::WeightShapeMismatch.as_str(), "E303");
        assert_eq!(ErrorCode::ConfigInvalidArch.as_str(), "E304");
    }

    #[test]
    fn error_code_assignment_all_str() {
        assert_eq!(ErrorCode::AssignmentFailed.as_str(), "E800");
        assert_eq!(ErrorCode::AssignmentCostExceeded.as_str(), "E801");
        assert_eq!(ErrorCode::AssignmentTimeout.as_str(), "E802");
    }

    #[test]
    fn error_code_sitemap_all_str() {
        assert_eq!(ErrorCode::SiteMapCorrupt.as_str(), "E700");
        assert_eq!(ErrorCode::SiteMapVersionMismatch.as_str(), "E701");
        assert_eq!(ErrorCode::SiteMapIoError.as_str(), "E702");
    }

    #[test]
    fn error_code_provider_all_str() {
        assert_eq!(ErrorCode::ProviderKeyInvalid.as_str(), "E500");
        assert_eq!(ErrorCode::ProviderRateLimited.as_str(), "E501");
        assert_eq!(ErrorCode::ProviderUnavailable.as_str(), "E502");
        assert_eq!(ErrorCode::ProviderUsageApiUnsupported.as_str(), "E503");
    }

    #[test]
    fn error_code_pipeline_all_str() {
        assert_eq!(ErrorCode::CompileFailed.as_str(), "E600");
        assert_eq!(ErrorCode::PipelineExecutionFailed.as_str(), "E601");
        assert_eq!(ErrorCode::SandBoxViolation.as_str(), "E602");
    }

    #[test]
    fn error_code_all_codes_count() {
        let codes = ErrorCode::all_codes();
        assert_eq!(codes.len(), 39, "expected 39 error codes, got {}", codes.len());
    }

    #[test]
    fn error_code_eq_and_copy() {
        let a = ErrorCode::RouterTimeout;
        let b = ErrorCode::RouterTimeout;
        let c = ErrorCode::ConfigNotFound;
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Copy semantics
        let d = a;
        assert_eq!(d, ErrorCode::RouterTimeout);
    }

    #[test]
    fn error_code_debug_format() {
        let code = ErrorCode::RouterTimeout;
        let debug = format!("{:?}", code);
        assert_eq!(debug, "RouterTimeout");
    }

    #[test]
    fn velocity_error_category_method() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "test");
        assert_eq!(err.category(), "router");
        let err = VelocityError::new(ErrorCode::ConfigNotFound, "test");
        assert_eq!(err.category(), "config");
        let err = VelocityError::new(ErrorCode::JitCompilationFailed, "test");
        assert_eq!(err.category(), "jit");
    }

    #[test]
    fn velocity_error_format_detailed_multiple_context() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "connection failed")
            .with_context("host", "router.velocity.io")
            .with_context("port", "443")
            .with_context("attempt", "3")
            .with_suggestion("Check your network connection.");
        let formatted = err.format_detailed();
        assert!(formatted.contains("host: router.velocity.io"));
        assert!(formatted.contains("port: 443"));
        assert!(formatted.contains("attempt: 3"));
        assert!(formatted.contains("Suggestion: Check your network connection."));
    }

    #[test]
    fn velocity_error_with_source_and_suggestion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = VelocityError::new(ErrorCode::IoError, "failed to read file")
            .with_source(io_err)
            .with_suggestion("Check file permissions.")
            .with_context("path", "/etc/secrets");
        assert!(err.source.is_some());
        assert!(err.suggestion.is_some());
        assert_eq!(err.context.len(), 1);
        let formatted = err.format_detailed();
        assert!(formatted.contains("Caused by:"));
        assert!(formatted.contains("Suggestion:"));
        assert!(formatted.contains("path: /etc/secrets"));
    }

    #[test]
    fn velocity_error_debug_format() {
        let err = VelocityError::new(ErrorCode::RouterTimeout, "timeout");
        let debug = format!("{:?}", err);
        assert!(debug.contains("VelocityError"));
        assert!(debug.contains("RouterTimeout"));
        assert!(debug.contains("timeout"));
    }

    #[test]
    fn router_error_unreachable_context() {
        let e = RouterError::Unreachable { url: "http://localhost:9999".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::RouterUnreachable);
        assert!(ve.context.iter().any(|(k, v)| k == "url" && v == "http://localhost:9999"));
        assert!(ve.suggestion.is_some());
    }

    #[test]
    fn router_error_timeout_suggestion() {
        let e = RouterError::Timeout { secs: 30 };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::RouterTimeout);
        assert!(ve.suggestion.is_some());
        assert!(ve.suggestion.as_ref().unwrap().contains("overloaded"));
    }

    #[test]
    fn router_error_rate_limited_suggestion() {
        let e = RouterError::RateLimited { retry_after_secs: 120 };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::RouterRateLimited);
        assert!(ve.suggestion.is_some());
        assert!(ve.suggestion.as_ref().unwrap().contains("120"));
    }

    #[test]
    fn router_error_response_invalid_context() {
        let e = RouterError::ResponseInvalid { detail: "missing field 'usage'".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::RouterResponseInvalid);
        assert!(ve.context.iter().any(|(k, v)| k == "detail" && v.contains("missing field")));
    }

    #[test]
    fn router_error_max_retries_context() {
        let e = RouterError::MaxRetriesExceeded { attempts: 5 };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::RouterServerError);
        assert!(ve.context.iter().any(|(k, v)| k == "attempts" && v == "5"));
    }

    #[test]
    fn model_error_dir_not_found_context() {
        let e = ModelError::DirNotFound { path: "/bad/model".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::ModelDirNotFound);
        assert!(ve.suggestion.is_some());
        assert!(ve.context.iter().any(|(k, _)| k == "path"));
    }

    #[test]
    fn model_error_weight_shape_mismatch_context() {
        let e = ModelError::WeightShapeMismatch {
            expected: "4x4".into(),
            actual: "2x8".into(),
        };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::WeightShapeMismatch);
        assert!(ve.context.iter().any(|(k, v)| k == "expected" && v == "4x4"));
        assert!(ve.context.iter().any(|(k, v)| k == "actual" && v == "2x8"));
    }

    #[test]
    fn provider_error_unavailable_context() {
        let e = ProviderError::Unavailable {
            provider: "openai".into(),
            detail: "service down".into(),
        };
        let ve: VelocityError = e.into();
        assert_eq!(ve.code, ErrorCode::ProviderUnavailable);
        assert!(ve.context.iter().any(|(k, v)| k == "provider" && v == "openai"));
        assert!(ve.context.iter().any(|(k, v)| k == "detail" && v == "service down"));
    }

    #[test]
    fn pipeline_error_compile_failed_context() {
        let e = PipelineError::CompileFailed { detail: "syntax error at line 5".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::CompileFailed);
        assert!(ve.context.iter().any(|(k, v)| k == "detail" && v.contains("syntax error")));
    }

    #[test]
    fn pipeline_error_execution_failed_context() {
        let e = PipelineError::ExecutionFailed { detail: "out of memory".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::PipelineExecutionFailed);
        assert!(ve.context.iter().any(|(k, v)| k == "detail" && v.contains("out of memory")));
    }

    #[test]
    fn sandbox_error_timeout_suggestion_text() {
        let e = SandboxError::Timeout { secs: 60 };
        let ve = e.to_velocity_error();
        assert!(ve.suggestion.is_some());
        assert!(ve.suggestion.as_ref().unwrap().contains("smaller steps"));
        assert!(ve.context.iter().any(|(k, v)| k == "timeout_secs" && v == "60"));
    }

    #[test]
    fn sandbox_error_resource_limit_context() {
        let e = SandboxError::ResourceLimit { resource: "memory".into() };
        let ve = e.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::SandBoxViolation);
        assert!(ve.context.iter().any(|(k, v)| k == "resource" && v == "memory"));
    }

    #[test]
    fn compile_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let ce = CompileError::from(io_err);
        assert!(ce.to_string().contains("file missing"));
        let ve: VelocityError = ce.into();
        assert_eq!(ve.code, ErrorCode::CompileFailed);
    }

    #[test]
    fn compile_error_parser_to_pipeline_to_velocity() {
        let ce = CompileError::Parser("unexpected token 'foo'".into());
        let pe: PipelineError = ce.into();
        match &pe {
            PipelineError::CompileFailed { detail } => {
                assert!(detail.contains("unexpected token"));
            }
            _ => panic!("expected CompileFailed"),
        }
        let ve: VelocityError = pe.to_velocity_error();
        assert_eq!(ve.code, ErrorCode::CompileFailed);
    }

    #[test]
    fn error_summary_by_category_sorted_descending() {
        let errors = vec![
            VelocityError::new(ErrorCode::RouterTimeout, "r1"),
            VelocityError::new(ErrorCode::RouterTimeout, "r2"),
            VelocityError::new(ErrorCode::RouterTimeout, "r3"),
            VelocityError::new(ErrorCode::ConfigNotFound, "c1"),
            VelocityError::new(ErrorCode::IoError, "i1"),
        ];
        let summary = summarize_errors(&errors);
        // router=3 should be first
        assert_eq!(summary.by_category[0].0, "router");
        assert_eq!(summary.by_category[0].1, 3);
        // remaining should have count 1 each
        assert!(summary.by_category[1].1 <= 1);
    }

    #[test]
    fn error_summary_json_key_count() {
        let summary = ErrorSummary {
            total: 5,
            by_category: vec![("router".into(), 3), ("config".into(), 2)],
            retryable_count: 3,
            security_count: 0,
            unique_codes: vec!["E201".into(), "E100".into()],
        };
        let json = serde_json::to_string(&summary).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), 5);
    }

    #[test]
    fn error_summary_clone() {
        let summary = summarize_errors(&[
            VelocityError::new(ErrorCode::RouterTimeout, "t"),
        ]);
        let s2 = summary.clone();
        assert_eq!(s2.total, 1);
        assert_eq!(s2.retryable_count, 1);
    }

    #[test]
    fn velocity_error_display_trait_format() {
        let err = VelocityError::new(ErrorCode::ConfigNotFound, "no config file");
        let s = format!("{}", err);
        assert!(s.contains("E100"));
        assert!(s.contains("no config file"));
        assert!(s.contains("—"));
    }

    #[test]
    fn velocity_error_is_io_via_source_downcast() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "inner io error");
        let err = VelocityError::new(ErrorCode::InternalError, "wrapped")
            .with_source(io_err);
        // InternalError is not normally io, but has an io::Error source
        assert!(err.is_io());
    }

    #[test]
    fn credential_error_not_found_suggestion() {
        let e = CredentialError::NotFound { key: "api_key".into() };
        let ve = e.to_velocity_error();
        assert!(ve.suggestion.is_some());
        assert!(ve.suggestion.as_ref().unwrap().contains("login"));
        assert!(ve.context.iter().any(|(k, v)| k == "key" && v == "api_key"));
    }

    #[test]
    fn credential_error_boundary_violation_suggestion() {
        let e = CredentialError::BoundaryViolation { detail: "env var leak".into() };
        let ve = e.to_velocity_error();
        assert!(ve.suggestion.is_some());
        assert!(ve.suggestion.as_ref().unwrap().contains("boundary"));
    }
}
