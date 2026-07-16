//! Validate per-task results before merging them.
//!
//! For now validation is a pure function over output strings.
//! A process-based validator can shell out to `scripts/check.py`.

use super::worker::WorkerResult;

#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub ok: bool,
    pub messages: Vec<String>,
}

impl ValidationReport {
    pub fn ok() -> Self {
        Self { ok: true, messages: Vec::new() }
    }

    pub fn fail(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            messages: vec![reason.into()],
        }
    }

    pub fn and(mut self, other: ValidationReport) -> Self {
        self.ok = self.ok && other.ok;
        self.messages.extend(other.messages);
        self
    }
}

/// Default checks: result reports success and produced textual output.
pub fn validate(result: &WorkerResult) -> ValidationReport {
    if !result.success {
        return ValidationReport::fail(format!("Worker failed: {}", result.message));
    }
    let mut r = ValidationReport::ok();
    if result.outputs.iter().map(|s| s.trim()).collect::<String>().is_empty() {
        r = r.and(ValidationReport::fail("Task produced no textual output."));
    }
    r
}
