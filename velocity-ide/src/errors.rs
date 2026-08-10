//! Error types for the V.E.L.O.C.I.T.Y.-IDE compiler pipeline.

/// Errors produced by the NDA lexer, parser, and WASM runner.
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
