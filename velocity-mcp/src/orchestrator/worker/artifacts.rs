use super::types::{ExecutionOutcome, WorkerAssignment};
use crate::automation::instruction_registry::AgentTaskKind;
use std::fs;
use std::path::Path;

pub fn detect_wa_run_artifact_path(
    changed_files: &[String],
    created_files: &[String],
) -> Option<String> {
    changed_files
        .iter()
        .chain(created_files.iter())
        .find(|path| path.to_ascii_lowercase().ends_with(".wa-run.nda"))
        .cloned()
}

pub fn wa_run_id_from_path(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    file_name
        .strip_suffix(".wa-run.nda")
        .map(|value| value.to_string())
}

pub fn serialize_execution_contract_nda(assignment: &WorkerAssignment) -> String {
    let mut lines = vec![
        "worker-execution-contract version 2".to_string(),
        format!("field\ttask_kind\t{}", assignment.task_kind.as_str()),
        format!(
            "field\tprovider\t{}",
            encode_nda_text(&assignment.provider_label)
        ),
        format!("field\tmodel\t{}", encode_nda_text(&assignment.model_label)),
        format!("field\tmodel_id\t{}", encode_nda_text(&assignment.model_id)),
        format!("field\tthinking\t{}", assignment.thinking),
        format!("field\ttask_id\t{}", assignment.task.id.0),
        format!(
            "field\ttask_title\t{}",
            encode_nda_text(&assignment.task.title)
        ),
        format!(
            "field\ttask_description\t{}",
            encode_nda_text(&assignment.task.description)
        ),
        format!(
            "field\tplanned_site_map_root\t{:016x}",
            assignment.planned_site_map_root
        ),
        format!("scope_count {}", assignment.task.scope.len()),
        format!("fallback_route_count {}", assignment.fallback_chain.len()),
    ];

    let instruction_lines: Vec<&str> = assignment.instructions.split('\n').collect();
    lines.push(format!(
        "instruction_line_count {}",
        instruction_lines.len()
    ));

    for (index, scope) in assignment.task.scope.iter().enumerate() {
        lines.push(format!("scope\t{}\t{}", index, encode_nda_text(scope)));
    }

    for (index, route) in assignment.fallback_chain.iter().enumerate() {
        lines.push(format!("fallback_route\t{}", index));
        lines.push(format!(
            "fallback_route_field\t{}\tprovider\t{}",
            index,
            encode_nda_text(route.provider.label())
        ));
        lines.push(format!(
            "fallback_route_field\t{}\tmodel\t{}",
            index,
            encode_nda_text(&route.model_label)
        ));
        lines.push(format!(
            "fallback_route_field\t{}\tmodel_id\t{}",
            index,
            encode_nda_text(&route.model_id)
        ));
        lines.push(format!(
            "fallback_route_field\t{}\tthinking\t{}",
            index, route.thinking
        ));
        lines.push(format!(
            "fallback_route_field\t{}\tscore\t{}",
            index, route.score
        ));
    }

    for (index, instruction_line) in instruction_lines.iter().enumerate() {
        lines.push(format!(
            "instruction_line\t{}\t{}",
            index,
            encode_nda_text(instruction_line)
        ));
    }

    lines.join("\n") + "\n"
}

pub fn write_execution_contract_artifacts(
    run_dir: &Path,
    assignment: &WorkerAssignment,
) -> Result<(), String> {
    fs::write(
        run_dir.join("instructions.nda"),
        serialize_execution_contract_nda(assignment),
    )
    .map_err(|err| format!("write nda instructions: {err}"))?;
    fs::write(
        run_dir.join("instructions.txt"),
        format!(
            "provider: {}\nmodel: {}\nmodel_id: {}\n\nthinking: {}\n\n{}",
            assignment.provider_label,
            assignment.model_label,
            assignment.model_id,
            assignment.thinking,
            assignment.instructions
        ),
    )
    .map_err(|err| format!("write text instructions: {err}"))
}

pub fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

pub fn write_execution_artifacts(run_dir: &Path, outcome: &ExecutionOutcome) -> Result<(), String> {
    write_execution_summary(run_dir, outcome)?;
    write_execution_facts(run_dir, outcome)
}

pub fn write_execution_summary(run_dir: &Path, outcome: &ExecutionOutcome) -> Result<(), String> {
    let mut summary = String::new();
    summary.push_str(if outcome.success {
        "Result: success\n"
    } else {
        "Result: failed\n"
    });
    summary.push_str("Task kind: ");
    summary.push_str(outcome.task_kind.as_str());
    summary.push('\n');
    if outcome.task_kind == AgentTaskKind::DesktopAutomation {
        summary.push_str("WA evidence lane: desktop automation\n");
    }
    summary.push_str("Active route: ");
    summary.push_str(&outcome.provider_label);
    summary.push_str(" / ");
    summary.push_str(&outcome.model_label);
    summary.push_str("\n\nAttempts:\n");
    for attempt in &outcome.attempts {
        summary.push_str("- ");
        summary.push_str(&attempt.provider_label);
        summary.push_str(" / ");
        summary.push_str(&attempt.model_label);
        summary.push_str(": ");
        summary.push_str(&attempt.message);
        summary.push('\n');
    }
    if !outcome.status_updates.is_empty() {
        summary.push_str("\nStatus updates:\n");
        for line in &outcome.status_updates {
            summary.push_str("- ");
            summary.push_str(line);
            summary.push('\n');
        }
    }
    if !outcome.changed_files.is_empty() {
        summary.push_str("\nChanged files:\n");
        for path in &outcome.changed_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.created_files.is_empty() {
        summary.push_str("\nCreated files:\n");
        for path in &outcome.created_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.deleted_files.is_empty() {
        summary.push_str("\nDeleted files:\n");
        for path in &outcome.deleted_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.out_of_scope_created_files.is_empty() {
        summary.push_str("\nOut-of-scope created files:\n");
        for path in &outcome.out_of_scope_created_files {
            summary.push_str("- ");
            summary.push_str(path);
            summary.push('\n');
        }
    }
    if !outcome.transcript.trim().is_empty() {
        summary.push_str("\nTranscript:\n");
        summary.push_str(outcome.transcript.trim());
        summary.push('\n');
    }
    fs::write(run_dir.join("summary.txt"), &summary).map_err(|err| format!("write summary: {err}"))
}

pub fn write_execution_facts(run_dir: &Path, outcome: &ExecutionOutcome) -> Result<(), String> {
    let transcript_lines = if outcome.transcript.trim().is_empty() {
        Vec::new()
    } else {
        outcome.transcript.trim().split('\n').collect::<Vec<_>>()
    };

    let mut facts = vec![
        "worker-run-facts version 2".to_string(),
        format!("field\ttask_kind\t{}", outcome.task_kind.as_str()),
        format!(
            "field\tresult\t{}",
            if outcome.success { "success" } else { "failed" }
        ),
        format!(
            "field\tprovider\t{}",
            encode_nda_text(&outcome.provider_label)
        ),
        format!("field\tmodel\t{}", encode_nda_text(&outcome.model_label)),
        format!("field\tmessage\t{}", encode_nda_text(&outcome.message)),
        format!("attempt_count {}", outcome.attempts.len()),
        format!("changed_file_count {}", outcome.changed_files.len()),
        format!("created_file_count {}", outcome.created_files.len()),
        format!("deleted_file_count {}", outcome.deleted_files.len()),
        format!(
            "out_of_scope_created_file_count {}",
            outcome.out_of_scope_created_files.len()
        ),
        format!("status_count {}", outcome.status_updates.len()),
        format!("transcript_line_count {}", transcript_lines.len()),
    ];

    for (index, attempt) in outcome.attempts.iter().enumerate() {
        facts.push(format!("attempt\t{}", index));
        facts.push(format!(
            "attempt_field\t{}\tprovider\t{}",
            index,
            encode_nda_text(&attempt.provider_label)
        ));
        facts.push(format!(
            "attempt_field\t{}\tmodel\t{}",
            index,
            encode_nda_text(&attempt.model_label)
        ));
        facts.push(format!(
            "attempt_field\t{}\tmodel_id\t{}",
            index,
            encode_nda_text(&attempt.model_id)
        ));
        facts.push(format!(
            "attempt_field\t{}\tresult\t{}",
            index,
            if attempt.success { "success" } else { "failed" }
        ));
        facts.push(format!(
            "attempt_field\t{}\tmessage\t{}",
            index,
            encode_nda_text(&attempt.message)
        ));
    }

    for (index, path) in outcome.changed_files.iter().enumerate() {
        facts.push(format!(
            "changed_file\t{}\t{}",
            index,
            encode_nda_text(path)
        ));
    }
    for (index, path) in outcome.created_files.iter().enumerate() {
        facts.push(format!(
            "created_file\t{}\t{}",
            index,
            encode_nda_text(path)
        ));
    }
    for (index, path) in outcome.deleted_files.iter().enumerate() {
        facts.push(format!(
            "deleted_file\t{}\t{}",
            index,
            encode_nda_text(path)
        ));
    }
    for (index, path) in outcome.out_of_scope_created_files.iter().enumerate() {
        facts.push(format!(
            "out_of_scope_created_file\t{}\t{}",
            index,
            encode_nda_text(path)
        ));
    }
    for (index, status) in outcome.status_updates.iter().enumerate() {
        facts.push(format!("status\t{}\t{}", index, encode_nda_text(status)));
    }
    if outcome.task_kind == AgentTaskKind::DesktopAutomation {
        facts.push("wa_field\tevidence_lane\tdesktop_automation".to_string());
        facts.push(format!(
            "wa_field\tartifact_summary_present\t{}",
            outcome.success
        ));
        facts.push(format!(
            "wa_field\tchanged_signal_count\t{}",
            outcome.changed_files.len() + outcome.created_files.len() + outcome.deleted_files.len()
        ));
    }
    for (index, line) in transcript_lines.iter().enumerate() {
        facts.push(format!(
            "transcript_line\t{}\t{}",
            index,
            encode_nda_text(line)
        ));
    }

    fs::write(run_dir.join("facts.nda"), facts.join("\n") + "\n")
        .map_err(|err| format!("write facts: {err}"))
}
