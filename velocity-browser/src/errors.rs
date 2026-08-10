//! Error types for the V.E.L.O.C.I.T.Y. browser engine.

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
