//! Validate per-task results before merging them.
//!
//! The baseline validator checks worker metadata first, then runs a narrow
//! workspace runtime check (`cargo check`) for successful task outputs.

use std::path::Path;

use crate::automation::{run_cargo_check, BuildDiagnostics};

use super::worker::WorkerResult;

#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub ok: bool,
    pub messages: Vec<String>,
}

impl ValidationReport {
    pub fn ok() -> Self {
        Self {
            ok: true,
            messages: Vec::new(),
        }
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

/// Default checks: result reports success and produced scoped file changes.
pub fn validate(result: &WorkerResult) -> ValidationReport {
    if !result.success {
        return ValidationReport::fail(format!("Worker failed: {}", result.message));
    }
    let mut r = ValidationReport::ok();
    if result.outputs.is_empty()
        && result.created_files.is_empty()
        && result.deleted_files.is_empty()
    {
        r = r.and(ValidationReport::fail(
            "Task produced no scoped file changes.",
        ));
    }
    r
}

pub fn validate_with_workspace(result: &WorkerResult, workspace_root: &Path) -> ValidationReport {
    let base = validate(result);
    if !base.ok {
        return base;
    }

    base.and(runtime_report(run_cargo_check(workspace_root)))
}

fn runtime_report(diag: BuildDiagnostics) -> ValidationReport {
    if diag.success {
        return ValidationReport::ok();
    }

    let mut messages = Vec::with_capacity(1 + diag.errors.len());
    messages.push(diag.summary);
    messages.extend(diag.errors);
    ValidationReport {
        ok: false,
        messages,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::orchestrator::TaskId;

    fn worker_result() -> WorkerResult {
        WorkerResult {
            success: true,
            task_id: TaskId(7),
            outputs: vec!["velocity-mcp/src/orchestrator/validator.rs".to_string()],
            duration: Duration::ZERO,
            message: "ok".to_string(),
            provider_label: String::new(),
            model_label: String::new(),
            transcript: String::new(),
            status_updates: Vec::new(),
            attempts: Vec::new(),
            created_files: Vec::new(),
            deleted_files: Vec::new(),
            out_of_scope_created_files: Vec::new(),
            run_summary_path: None,
            run_facts_path: None,
        }
    }

    #[test]
    fn validate_rejects_results_without_scoped_changes() {
        let mut result = worker_result();
        result.outputs.clear();

        let report = validate(&result);
        assert!(!report.ok);
        assert_eq!(
            report.messages,
            vec!["Task produced no scoped file changes.".to_string()]
        );
    }

    #[test]
    fn runtime_report_passes_successful_diagnostics() {
        let report = runtime_report(BuildDiagnostics {
            success: true,
            summary: "cargo check OK (0 warnings)".to_string(),
            ..Default::default()
        });

        assert!(report.ok);
        assert!(report.messages.is_empty());
    }

    #[test]
    fn runtime_report_surfaces_summary_and_errors() {
        let report = runtime_report(BuildDiagnostics {
            success: false,
            summary: "cargo check FAILED (2 errors, 1 warnings)".to_string(),
            errors: vec!["error: first".to_string(), "error: second".to_string()],
            ..Default::default()
        });

        assert!(!report.ok);
        assert_eq!(
            report.messages,
            vec![
                "cargo check FAILED (2 errors, 1 warnings)".to_string(),
                "error: first".to_string(),
                "error: second".to_string(),
            ]
        );
    }
}
