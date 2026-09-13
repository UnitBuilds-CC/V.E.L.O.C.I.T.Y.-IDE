//! Error types for the V.E.L.O.C.I.T.Y. browser engine.

/// Top-level error type for browser session operations.
///
/// Replaces ad-hoc `Box<dyn Error>` returns in `session.rs` and `http_client.rs`
/// with a typed, exhaustive enum so callers can match on failure modes.
#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    /// The session has no DOM tree loaded.
    #[error("no DOM tree loaded in session")]
    NoDomLoaded,

    /// A CSS selector did not match any element.
    #[error("selector '{0}' not found")]
    SelectorNotFound(String),

    /// Navigation to a URL failed.
    #[error("navigation failed: {0}")]
    NavigationFailed(String),

    /// Network request error (HTTP transport, DNS, TLS, etc.).
    #[error("network error: {0}")]
    NetworkError(String),

    /// A sandbox policy blocked the operation.
    #[error("sandbox violation: {0}")]
    SandboxViolation(String),

    /// An OCR target text was not found in the pixel buffer.
    #[error("OCR target not found: {0}")]
    OcrTargetNotFound(String),

    /// The HTTP response was malformed (missing headers, bad chunk, etc.).
    #[error("malformed HTTP response: {0}")]
    MalformedResponse(String),

    /// Too many HTTP redirects were followed.
    #[error("too many redirects")]
    TooManyRedirects,

    /// URL parsing failed.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// An I/O error from the underlying transport.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A JavaScript evaluation error.
    #[error("JS error: {0}")]
    JsError(String),
}

/// Convenience alias used throughout the browser crate.
pub type BrowserResult<T> = Result<T, BrowserError>;

impl From<String> for BrowserError {
    fn from(s: String) -> Self {
        BrowserError::JsError(s)
    }
}

/// Errors from browser session operations (navigation, DOM queries, network).
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The session has no DOM tree loaded.
    #[error("no DOM tree loaded in session")]
    NoDomLoaded,

    /// A CSS selector did not match any element.
    #[error("selector '{0}' not found")]
    SelectorNotFound(String),

    /// Navigation to a URL failed.
    #[error("navigation failed: {0}")]
    NavigationFailed(String),

    /// Network request error.
    #[error("network error: {0}")]
    NetworkError(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Errors from the NDA document encoder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NdaError {
    /// Unexpected end of the NDA byte stream.
    #[error("unexpected end of NDA stream")]
    UnexpectedEof,

    /// Invalid UTF-8 in an NDA string.
    #[error("invalid UTF-8 in NDA data")]
    InvalidUtf8,

    /// A string exceeded the portable-format length limit.
    #[error("NDA string too long: {0} bytes")]
    StringTooLong(usize),

    /// The document exceeded the portable-format command limit.
    #[error("too many NDA commands: {0}")]
    TooManyCommands(usize),
}
