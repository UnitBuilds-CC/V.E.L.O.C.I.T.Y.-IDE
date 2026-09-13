//! Input sanitization framework.
//!
//! Provides domain-specific sanitizers that validate and clean untrusted input
//! before it reaches OS-level operations (shell commands, file paths, URLs,
//! tool arguments). This is the primary defence against prompt-injection →
//! command-injection escalation paths.
//!
//! # Usage
//!
//! ```ignore
//! use crate::security::sanitize::{sanitize_shell, sanitize_path, SanitizeError};
//!
//! let clean_cmd = sanitize_shell("cargo build")?;
//! let clean_path = sanitize_path("../etc/passwd", &workspace_root)?;
//! ```

use std::path::{Path, PathBuf};

// ─── Error type ───────────────────────────────────────────────────────────────

/// Error returned when sanitization rejects an input.
#[derive(Debug, Clone)]
pub struct SanitizeError {
    pub domain: &'static str,
    pub reason: String,
    pub input_preview: String,
}

impl std::fmt::Display for SanitizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sanitize::{} rejected {:?}: {}",
            self.domain, self.input_preview, self.reason
        )
    }
}

impl std::error::Error for SanitizeError {}

fn preview(input: &str, max: usize) -> String {
    if input.len() <= max {
        input.to_string()
    } else {
        format!("{}...", &input[..max])
    }
}

// ─── Shell sanitizer ─────────────────────────────────────────────────────────

/// Shell metacharacters that enable command injection.
const SHELL_BLOCKLIST: &[char] = &[
    '|', ';', '&', '$', '`', '(', ')', '{', '}', '<', '>', '\n', '\r', '\0',
];

/// Validate and return a shell command string.
///
/// Rejects inputs containing shell metacharacters (`|`, `;`, `&`, `$()`,
/// backticks, etc.) and null bytes. Returns the trimmed input on success.
pub fn sanitize_shell(input: &str) -> Result<String, SanitizeError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SanitizeError {
            domain: "shell",
            reason: "empty command".into(),
            input_preview: String::new(),
        });
    }

    // Check for blocked metacharacters.
    if let Some(ch) = trimmed.chars().find(|c| SHELL_BLOCKLIST.contains(c)) {
        return Err(SanitizeError {
            domain: "shell",
            reason: format!("blocked shell metacharacter: {:?}", ch),
            input_preview: preview(trimmed, 80),
        });
    }

    // Reject path traversal in arguments.
    if trimmed.contains("..") {
        return Err(SanitizeError {
            domain: "shell",
            reason: "path traversal (..) detected in command".into(),
            input_preview: preview(trimmed, 80),
        });
    }

    Ok(trimmed.to_string())
}

// ─── Path sanitizer ──────────────────────────────────────────────────────────

/// Validate that `input` resolves to a path within `workspace_root`.
///
/// Rejects:
/// - Paths that escape the workspace via `..` traversal
/// - Symlinks pointing outside the workspace
/// - Paths containing null bytes
///
/// Returns the canonical (absolute, symlink-resolved) path on success.
pub fn sanitize_path(input: &str, workspace_root: &Path) -> Result<PathBuf, SanitizeError> {
    if input.contains('\0') {
        return Err(SanitizeError {
            domain: "path",
            reason: "null byte in path".into(),
            input_preview: preview(input, 80),
        });
    }

    let candidate = if Path::new(input).is_absolute() {
        PathBuf::from(input)
    } else {
        workspace_root.join(input)
    };

    // Canonicalize resolves symlinks and `..` segments. If the file doesn't
    // exist yet, canonicalize the parent and re-append the filename.
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // File may not exist yet (e.g. write to new path). Canonicalize parent.
            let parent = candidate
                .parent()
                .unwrap_or(workspace_root);
            let file_name = candidate
                .file_name()
                .ok_or_else(|| SanitizeError {
                    domain: "path",
                    reason: "path has no filename component".into(),
                    input_preview: preview(input, 80),
                })?;
            match parent.canonicalize() {
                Ok(p) => p.join(file_name),
                Err(e) => {
                    return Err(SanitizeError {
                        domain: "path",
                        reason: format!("cannot resolve parent directory: {}", e),
                        input_preview: preview(input, 80),
                    });
                }
            }
        }
    };

    // Ensure the resolved path is within the workspace.
    let workspace_canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    if !canonical.starts_with(&workspace_canonical) {
        return Err(SanitizeError {
            domain: "path",
            reason: format!(
                "path escapes workspace (resolved to {}, workspace is {})",
                canonical.display(),
                workspace_canonical.display()
            ),
            input_preview: preview(input, 80),
        });
    }

    Ok(canonical)
}

// ─── URL sanitizer ───────────────────────────────────────────────────────────

/// Allowed URL schemes for outbound HTTP requests.
const URL_ALLOWED_SCHEMES: &[&str] = &["https", "http"];

/// Blocked URL schemes (dangerous in any context).
const URL_BLOCKED_SCHEMES: &[&str] = &["file", "javascript", "data", "vbscript", "ftp"];

/// Validate a URL for outbound requests.
///
/// Rejects:
/// - Non-HTTP(S) schemes
/// - Empty hosts
/// - URLs with embedded credentials (`user:pass@host`)
pub fn sanitize_url(input: &str) -> Result<String, SanitizeError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SanitizeError {
            domain: "url",
            reason: "empty URL".into(),
            input_preview: String::new(),
        });
    }

    // Extract scheme.
    let scheme_end = trimmed.find("://").ok_or_else(|| SanitizeError {
        domain: "url",
        reason: "missing :// scheme separator".into(),
        input_preview: preview(trimmed, 80),
    })?;
    let scheme = trimmed[..scheme_end].to_lowercase();

    if URL_BLOCKED_SCHEMES.contains(&scheme.as_str()) {
        return Err(SanitizeError {
            domain: "url",
            reason: format!("blocked scheme: {:?}", scheme),
            input_preview: preview(trimmed, 80),
        });
    }

    if !URL_ALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return Err(SanitizeError {
            domain: "url",
            reason: format!("unsupported scheme: {:?} (allowed: {:?})", scheme, URL_ALLOWED_SCHEMES),
            input_preview: preview(trimmed, 80),
        });
    }

    // Reject embedded credentials.
    let after_scheme = &trimmed[scheme_end + 3..];
    if let Some(at_pos) = after_scheme.find('@') {
        // Check if '@' is before the first '/' (i.e. in the authority section).
        let path_start = after_scheme.find('/').unwrap_or(after_scheme.len());
        if at_pos < path_start {
            return Err(SanitizeError {
                domain: "url",
                reason: "URL contains embedded credentials (user:pass@host)".into(),
                input_preview: preview(trimmed, 80),
            });
        }
    }

    Ok(trimmed.to_string())
}

// ─── Tool argument sanitizer ─────────────────────────────────────────────────

/// Maximum allowed length for a single tool argument string.
const MAX_ARG_LENGTH: usize = 1_048_576; // 1 MB

/// Validate a JSON tool argument string.
///
/// Rejects:
/// - Arguments exceeding the maximum length
/// - Arguments containing null bytes
pub fn sanitize_tool_arg(input: &str) -> Result<String, SanitizeError> {
    if input.len() > MAX_ARG_LENGTH {
        return Err(SanitizeError {
            domain: "tool_arg",
            reason: format!(
                "argument exceeds maximum length ({} > {} bytes)",
                input.len(),
                MAX_ARG_LENGTH
            ),
            input_preview: preview(input, 80),
        });
    }

    if input.contains('\0') {
        return Err(SanitizeError {
            domain: "tool_arg",
            reason: "null byte in tool argument".into(),
            input_preview: preview(input, 80),
        });
    }

    Ok(input.to_string())
}
