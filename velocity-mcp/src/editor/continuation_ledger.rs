#![allow(dead_code, unused_imports, unused_variables)]
//! Continuation Ledger: cross-model context handoff system.
//!
//! When a model fails mid-edit or gets swapped, the continuation ledger
//! captures the precise state of the task — what was done, what changed,
//! what's pending — so another model can continue without gaps or duplication.
//!
//! Key design principles:
//! 1. **Scoped**: Only transfers context relevant to the task (via SiteMap)
//! 2. **Compact**: No raw transcript dumps — structured deltas + progress markers
//! 3. **Model-agnostic**: The ledger is a universal contract, not tied to any model
//! 4. **Edit-precise**: Captures exact partial file states for mid-edit recovery

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

// ─── Core Ledger Types ───────────────────────────────────────────────────────

/// The continuation ledger: a complete, model-agnostic handoff document.
/// When model A fails mid-task, this is what model B receives to continue.
#[derive(Debug, Clone)]
pub struct ContinuationLedger {
    /// Unique ledger ID (task_id + attempt sequence).
    pub id: String,
    /// The mission spec: what we're trying to achieve.
    pub mission: MissionSpec,
    /// Scoped environment brief derived from SiteMap.
    pub environment: ScopeEnvironmentBrief,
    /// Edit journal: what was already done (partial or complete).
    pub journal: EditJournal,
    /// Progress markers: which steps are done, which are pending.
    pub progress: ProgressState,
    /// Model provenance: who worked on this before.
    pub provenance: Vec<ModelAttemptRecord>,
    /// When this ledger was captured.
    pub captured_at: SystemTime,
    /// SiteMap root at capture time (for freshness validation).
    pub site_map_root: u64,
}

/// The mission: what we're trying to accomplish. Compact and transferable.
#[derive(Debug, Clone)]
pub struct MissionSpec {
    /// High-level goal (e.g., "Refactor the TLS handshake to use async I/O")
    pub goal: String,
    /// Task kind classification.
    pub task_kind: String,
    /// Structural expectations (from execution contract).
    pub expectations: Vec<String>,
    /// Constraints or non-goals (things to avoid).
    pub constraints: Vec<String>,
}

/// Compact environment context derived from SiteMap relationships.
/// This is what replaces "dump the whole codebase" — only the relevant
/// call graph, dependencies, and file structure for this task's scope.
#[derive(Debug, Clone)]
pub struct ScopeEnvironmentBrief {
    /// Files in scope for this task (relative paths).
    pub scoped_files: Vec<ScopedFileBrief>,
    /// Symbols that call into the scoped files (callers we must not break).
    pub external_callers: Vec<SymbolRef>,
    /// Symbols the scoped files depend on (dependencies we must respect).
    pub external_dependencies: Vec<SymbolRef>,
    /// Cross-file relationships within scope (internal coupling).
    pub internal_relationships: Vec<SymbolRelationship>,
    /// Compact text summary of the environment (for model system prompt).
    pub narrative: String,
}

/// Brief for a single scoped file: what it contains and its role.
#[derive(Debug, Clone)]
pub struct ScopedFileBrief {
    pub path: PathBuf,
    pub symbols: Vec<String>,
    pub line_count: usize,
    pub role: String,
}

/// A reference to a symbol outside the immediate scope.
#[derive(Debug, Clone)]
pub struct SymbolRef {
    pub name: String,
    pub file: String,
    pub relationship: String,
}

/// A relationship between two symbols.
#[derive(Debug, Clone)]
pub struct SymbolRelationship {
    pub from_symbol: String,
    pub to_symbol: String,
    pub kind: RelationshipKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipKind {
    Calls,
    Defines,
    Declares,
    Imports,
}

impl RelationshipKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::Defines => "defines",
            Self::Declares => "declares",
            Self::Imports => "imports",
        }
    }
}

// ─── Edit Journal ────────────────────────────────────────────────────────────

/// Precise record of what edits were made (or attempted) before handoff.
/// This is the critical piece for mid-edit continuation.
#[derive(Debug, Clone)]
pub struct EditJournal {
    /// Completed edits: files that were fully and correctly modified.
    pub completed_edits: Vec<FileEdit>,
    /// Partial edit: a file that was being modified when the model stopped.
    /// Contains both the original state and the incomplete new state.
    pub partial_edit: Option<PartialFileEdit>,
    /// Files that were created during this attempt.
    pub created_files: Vec<PathBuf>,
    /// Files that were deleted during this attempt.
    pub deleted_files: Vec<PathBuf>,
}

impl Default for EditJournal {
    fn default() -> Self {
        Self {
            completed_edits: Vec::new(),
            partial_edit: None,
            created_files: Vec::new(),
            deleted_files: Vec::new(),
        }
    }
}

/// A completed file edit with before/after state.
#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    /// Unified diff (compact representation of the change).
    pub diff: String,
    /// What was the intent of this edit (extracted from model reasoning).
    pub intent: String,
}

/// A partial file edit: the model was mid-way through changing this file.
/// The continuing model must understand both the original and current state.
#[derive(Debug, Clone)]
pub struct PartialFileEdit {
    pub path: PathBuf,
    /// The file content before any edits began.
    pub original_content: String,
    /// The file content as it currently exists on disk (partially modified).
    pub current_content: String,
    /// What the model was trying to do (extracted from last status/reasoning).
    pub intent: String,
    /// Which section/function was being edited (line range if known).
    pub editing_region: Option<(usize, usize)>,
    /// Whether the current content compiles (if checkable).
    pub compiles: Option<bool>,
}

// ─── Progress State ──────────────────────────────────────────────────────────

/// Tracks which logical steps of the task are done vs pending.
#[derive(Debug, Clone)]
pub struct ProgressState {
    pub steps: Vec<ProgressStep>,
    pub overall_percent: f32,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            overall_percent: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressStep {
    pub description: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    InProgress,
    Pending,
    Failed,
}

impl StepStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::InProgress => "in_progress",
            Self::Pending => "pending",
            Self::Failed => "failed",
        }
    }
}

// ─── Model Provenance ────────────────────────────────────────────────────────

/// Record of a model's attempt on this task (for the continuing model to know
/// what was tried and what went wrong).
#[derive(Debug, Clone)]
pub struct ModelAttemptRecord {
    pub provider: String,
    pub model_id: String,
    pub model_label: String,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub outcome: AttemptOutcome,
    /// Key decisions or reasoning the model expressed (not raw transcript).
    pub key_decisions: Vec<String>,
    /// What went wrong (if failed).
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Completed,
    PartialSuccess,
    Failed,
    Cancelled,
    TimedOut,
}

impl AttemptOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::PartialSuccess => "partial_success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }
}

// ─── Ledger Construction ─────────────────────────────────────────────────────

impl ContinuationLedger {
    /// Build a continuation ledger from a worker's live state.
    /// Called when we need to hand off to a different model.
    pub fn capture(
        task_id: &str,
        goal: &str,
        task_kind: &str,
        scope_files: &[PathBuf],
        workspace_root: &Path,
        site_map_root: u64,
        transcript: &str,
        changed_files: &[String],
        status_updates: &[String],
        provider_label: &str,
        model_label: &str,
        model_id: &str,
        duration: Duration,
        success: bool,
    ) -> Self {
        let mission = MissionSpec {
            goal: goal.to_string(),
            task_kind: task_kind.to_string(),
            expectations: Vec::new(),
            constraints: Vec::new(),
        };

        let environment = build_scope_brief(workspace_root, scope_files);
        let journal = build_edit_journal(workspace_root, scope_files, changed_files);
        let progress = infer_progress(transcript, &journal, status_updates);
        let outcome = if success {
            AttemptOutcome::Completed
        } else if !journal.completed_edits.is_empty() {
            AttemptOutcome::PartialSuccess
        } else {
            AttemptOutcome::Failed
        };

        let provenance = vec![ModelAttemptRecord {
            provider: provider_label.to_string(),
            model_id: model_id.to_string(),
            model_label: model_label.to_string(),
            started_at: SystemTime::now() - duration,
            duration,
            outcome,
            key_decisions: extract_key_decisions(transcript),
            failure_reason: if !success {
                status_updates.last().cloned()
            } else {
                None
            },
        }];

        Self {
            id: format!("{}-attempt-{}", task_id, 1),
            mission,
            environment,
            journal,
            progress,
            provenance,
            captured_at: SystemTime::now(),
            site_map_root,
        }
    }

    /// Serialize the ledger to a compact NDA format for persistence.
    pub fn serialize(&self) -> String {
        let mut lines = Vec::new();
        lines.push("continuation-ledger version 1".to_string());
        lines.push(format!("field\tid\t{}", self.id));
        lines.push(format!("field\tgoal\t{}", self.mission.goal));
        lines.push(format!("field\ttask_kind\t{}", self.mission.task_kind));
        lines.push(format!("field\tsite_map_root\t{:016x}", self.site_map_root));
        lines.push(format!(
            "field\tprogress_percent\t{:.0}",
            self.progress.overall_percent
        ));

        // Scope files
        for (i, file) in self.environment.scoped_files.iter().enumerate() {
            lines.push(format!(
                "scope_file\t{}\t{}\t{}",
                i,
                file.path.display(),
                file.role
            ));
        }

        // External callers (compact)
        for (i, caller) in self.environment.external_callers.iter().enumerate() {
            lines.push(format!(
                "ext_caller\t{}\t{}\t{}\t{}",
                i, caller.name, caller.file, caller.relationship
            ));
        }

        // Completed edits
        for (i, edit) in self.journal.completed_edits.iter().enumerate() {
            lines.push(format!(
                "completed_edit\t{}\t{}\t{}",
                i,
                edit.path.display(),
                edit.intent
            ));
        }

        // Partial edit (critical for continuation)
        if let Some(ref partial) = self.journal.partial_edit {
            lines.push(format!("partial_edit_path\t{}", partial.path.display()));
            lines.push(format!("partial_edit_intent\t{}", partial.intent));
            if let Some((start, end)) = partial.editing_region {
                lines.push(format!("partial_edit_region\t{}\t{}", start, end));
            }
            if let Some(compiles) = partial.compiles {
                lines.push(format!("partial_edit_compiles\t{}", compiles));
            }
        }

        // Progress steps
        for (i, step) in self.progress.steps.iter().enumerate() {
            lines.push(format!(
                "step\t{}\t{}\t{}",
                i,
                step.status.label(),
                step.description
            ));
        }

        // Provenance
        for (i, attempt) in self.provenance.iter().enumerate() {
            lines.push(format!(
                "attempt\t{}\t{}\t{}\t{}\t{:.1}s",
                i,
                attempt.provider,
                attempt.model_label,
                attempt.outcome.label(),
                attempt.duration.as_secs_f64()
            ));
            if let Some(ref reason) = attempt.failure_reason {
                lines.push(format!("attempt_failure\t{}\t{}", i, reason));
            }
            for (j, decision) in attempt.key_decisions.iter().enumerate() {
                lines.push(format!("attempt_decision\t{}\t{}\t{}", i, j, decision));
            }
        }

        // Narrative (compact environment summary for model system prompt)
        lines.push(format!("narrative\t{}", self.environment.narrative));

        lines.join("\n") + "\n"
    }

    /// Produce the continuation prompt: what the next model should receive
    /// as its context to seamlessly continue the task.
    pub fn continuation_prompt(&self) -> String {
        let mut prompt = String::new();

        // Section 1: Mission
        prompt.push_str("## Mission\n");
        prompt.push_str(&format!("Goal: {}\n", self.mission.goal));
        prompt.push_str(&format!("Type: {}\n\n", self.mission.task_kind));

        // Section 2: Environment (scoped, not the whole codebase)
        prompt.push_str("## Environment\n");
        prompt.push_str(&self.environment.narrative);
        prompt.push_str("\n\n");

        // Section 3: What was already done
        if !self.journal.completed_edits.is_empty() {
            prompt.push_str("## Completed Edits\n");
            for edit in &self.journal.completed_edits {
                prompt.push_str(&format!(
                    "- {} — {}\n",
                    edit.path.display(),
                    edit.intent
                ));
            }
            prompt.push('\n');
        }

        // Section 4: Partial edit (critical for continuation)
        if let Some(ref partial) = self.journal.partial_edit {
            prompt.push_str("## ⚠ Partial Edit In Progress\n");
            prompt.push_str(&format!("File: {}\n", partial.path.display()));
            prompt.push_str(&format!("Intent: {}\n", partial.intent));
            if let Some((start, end)) = partial.editing_region {
                prompt.push_str(&format!("Region: lines {}-{}\n", start, end));
            }
            if let Some(false) = partial.compiles {
                prompt.push_str("⚠ Current state does NOT compile. Fix required before continuing.\n");
            }
            prompt.push_str("\nThe file is currently in a partially-modified state. ");
            prompt.push_str("Continue the edit from the current state — do NOT redo work that's already done.\n\n");
        }

        // Section 5: Progress
        if !self.progress.steps.is_empty() {
            prompt.push_str("## Progress\n");
            for step in &self.progress.steps {
                let marker = match step.status {
                    StepStatus::Done => "[x]",
                    StepStatus::InProgress => "[~]",
                    StepStatus::Pending => "[ ]",
                    StepStatus::Failed => "[!]",
                };
                prompt.push_str(&format!("{} {}\n", marker, step.description));
            }
            prompt.push_str(&format!(
                "\nOverall: {:.0}% complete\n\n",
                self.progress.overall_percent
            ));
        }

        // Section 6: Previous attempt context
        if let Some(last) = self.provenance.last() {
            prompt.push_str("## Previous Attempt\n");
            prompt.push_str(&format!(
                "Model {} ({}) ran for {:.1}s — outcome: {}\n",
                last.model_label,
                last.provider,
                last.duration.as_secs_f64(),
                last.outcome.label()
            ));
            if let Some(ref reason) = last.failure_reason {
                prompt.push_str(&format!("Failure: {}\n", reason));
            }
            if !last.key_decisions.is_empty() {
                prompt.push_str("Key decisions made:\n");
                for decision in &last.key_decisions {
                    prompt.push_str(&format!("  • {}\n", decision));
                }
            }
        }

        prompt
    }

    /// Write the ledger to disk (`.velocity/agentic/runs/task-N/ledger.nda`).
    pub fn persist(&self, run_dir: &Path) -> std::io::Result<()> {
        let path = run_dir.join("ledger.nda");
        fs::write(path, self.serialize())
    }

    /// Estimate context token count (rough: 4 chars ≈ 1 token).
    pub fn estimated_tokens(&self) -> usize {
        self.continuation_prompt().len() / 4
    }
}

// ─── Environment Brief Construction ─────────────────────────────────────────

/// Build a scoped environment brief from the SiteMap relationships.
/// This replaces dumping hundreds of thousands of tokens — we only include
/// what's relevant to the task's file scope.
fn build_scope_brief(workspace_root: &Path, scope_files: &[PathBuf]) -> ScopeEnvironmentBrief {
    let mut scoped_file_briefs = Vec::new();
    let mut narrative_parts = Vec::new();

    for file_path in scope_files {
        let full_path = if file_path.is_absolute() {
            file_path.clone()
        } else {
            workspace_root.join(file_path)
        };

        let (symbols, line_count) = if full_path.is_file() {
            let content = fs::read_to_string(&full_path).unwrap_or_default();
            let lines = content.lines().count();
            let syms = extract_top_symbols(&content);
            (syms, lines)
        } else {
            (Vec::new(), 0)
        };

        let role = infer_file_role(file_path, &symbols);
        narrative_parts.push(format!(
            "• {} ({} lines) — {}{}",
            file_path.display(),
            line_count,
            role,
            if symbols.is_empty() {
                String::new()
            } else {
                format!(" [{}]", symbols.iter().take(5).cloned().collect::<Vec<_>>().join(", "))
            }
        ));

        scoped_file_briefs.push(ScopedFileBrief {
            path: file_path.clone(),
            symbols,
            line_count,
            role,
        });
    }

    let narrative = if narrative_parts.is_empty() {
        "No scoped files.".to_string()
    } else {
        format!("Scoped files:\n{}", narrative_parts.join("\n"))
    };

    ScopeEnvironmentBrief {
        scoped_files: scoped_file_briefs,
        external_callers: Vec::new(), // Populated from SiteMap when available
        external_dependencies: Vec::new(),
        internal_relationships: Vec::new(),
        narrative,
    }
}

/// Build the edit journal from current file states vs. scope snapshot.
fn build_edit_journal(
    workspace_root: &Path,
    scope_files: &[PathBuf],
    changed_files: &[String],
) -> EditJournal {
    let mut completed_edits = Vec::new();

    for changed in changed_files {
        let path = PathBuf::from(changed);
        let full_path = workspace_root.join(&path);
        let intent = format!("Modified {}", path.display());

        // We record the edit as completed since it appears in changed_files
        completed_edits.push(FileEdit {
            path,
            diff: String::new(), // Actual diff would come from scope_snapshot comparison
            intent,
        });
    }

    // Detect partial edits: files in scope that were modified but not in changed_files
    // This indicates a mid-edit state
    let partial_edit = detect_partial_edit(workspace_root, scope_files, changed_files);

    EditJournal {
        completed_edits,
        partial_edit,
        created_files: Vec::new(),
        deleted_files: Vec::new(),
    }
}

/// Detect if any scoped file is in a partially-modified state.
/// A partial edit is indicated by: file was modified (differs from snapshot)
/// but wasn't reported as a completed change.
fn detect_partial_edit(
    workspace_root: &Path,
    scope_files: &[PathBuf],
    changed_files: &[String],
) -> Option<PartialFileEdit> {
    let run_dir = workspace_root.join(".velocity").join("agentic").join("runs");
    let snapshot_dir = run_dir.join("scope_snapshot");

    for file_path in scope_files {
        let rel_str = file_path.display().to_string().replace('\\', "/");
        if changed_files.contains(&rel_str) {
            continue; // This file was fully changed — not partial
        }

        let full_path = workspace_root.join(file_path);
        let snapshot_path = snapshot_dir.join(file_path);

        if !full_path.exists() || !snapshot_path.exists() {
            continue;
        }

        let Ok(current) = fs::read_to_string(&full_path) else {
            continue;
        };
        let Ok(original) = fs::read_to_string(&snapshot_path) else {
            continue;
        };

        if current != original {
            // Found a file that differs from snapshot but wasn't completed
            return Some(PartialFileEdit {
                path: file_path.clone(),
                original_content: original,
                current_content: current,
                intent: "In-progress modification (interrupted)".to_string(),
                editing_region: None,
                compiles: None,
            });
        }
    }
    None
}

/// Infer progress from transcript and edits.
fn infer_progress(
    transcript: &str,
    journal: &EditJournal,
    status_updates: &[String],
) -> ProgressState {
    let mut steps = Vec::new();

    // Infer steps from status updates
    for update in status_updates {
        let status = if update.contains("complete") || update.contains("done") {
            StepStatus::Done
        } else if update.contains("fail") || update.contains("error") {
            StepStatus::Failed
        } else {
            StepStatus::Done
        };
        steps.push(ProgressStep {
            description: update.clone(),
            status,
        });
    }

    // If we have a partial edit, there's an in-progress step
    if journal.partial_edit.is_some() {
        steps.push(ProgressStep {
            description: "File modification in progress".to_string(),
            status: StepStatus::InProgress,
        });
    }

    let done_count = steps.iter().filter(|s| s.status == StepStatus::Done).count();
    let total = steps.len().max(1);
    let percent = (done_count as f32 / total as f32) * 100.0;

    ProgressState {
        steps,
        overall_percent: if transcript.is_empty() { 0.0 } else { percent },
    }
}

/// Extract key decisions from transcript (not the raw text — just decision points).
fn extract_key_decisions(transcript: &str) -> Vec<String> {
    let mut decisions = Vec::new();
    for line in transcript.lines() {
        let trimmed = line.trim();
        // Look for decision indicators
        if trimmed.starts_with("Decision:")
            || trimmed.starts_with("Approach:")
            || trimmed.starts_with("Strategy:")
            || trimmed.contains("I'll ")
            || trimmed.contains("I will ")
            || trimmed.contains("decided to")
            || trimmed.contains("choosing")
        {
            if trimmed.len() > 10 && trimmed.len() < 200 {
                decisions.push(trimmed.to_string());
            }
        }
    }
    // Cap at 5 most recent decisions
    if decisions.len() > 5 {
        decisions = decisions[decisions.len() - 5..].to_vec();
    }
    decisions
}

/// Extract top-level symbol names from source code.
fn extract_top_symbols(content: &str) -> Vec<String> {
    const KEYWORDS: &[&str] = &[
        "fn ", "struct ", "enum ", "trait ", "impl ", "type ", "const ",
        "mod ", "class ", "def ", "interface ", "function ",
    ];
    let mut symbols = Vec::new();
    for line in content.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim_start();
        for kw in KEYWORDS {
            if let Some(rest) = trimmed.strip_prefix(kw) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    symbols.push(name);
                }
                break;
            }
        }
    }
    symbols
}

/// Infer a file's role from its name and contents.
fn infer_file_role(path: &Path, symbols: &[String]) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name.contains("test") {
        "test module".to_string()
    } else if name == "mod.rs" || name == "__init__.py" || name == "index.ts" {
        "module root".to_string()
    } else if name.contains("config") || name.contains("settings") {
        "configuration".to_string()
    } else if symbols.iter().any(|s| s.contains("main")) {
        "entry point".to_string()
    } else if symbols.len() > 10 {
        "core implementation".to_string()
    } else {
        "implementation".to_string()
    }
}

// ─── Enrichment from SiteMap ─────────────────────────────────────────────────

/// Enrich the environment brief with SiteMap relationship data.
/// This is what makes continuation work without token bloat: we only
/// include the relevant call graph, not the whole codebase.
pub fn enrich_from_site_map(
    brief: &mut ScopeEnvironmentBrief,
    site_map_root: &Path,
    scoped_files: &[PathBuf],
    string_resolver: &dyn Fn(u64) -> Option<String>,
    find_callers: &dyn Fn(u64) -> Vec<u64>,
    find_deps: &dyn Fn(u64) -> Vec<u64>,
) {
    let mut callers = Vec::new();
    let mut deps = Vec::new();

    for file_brief in &brief.scoped_files {
        for symbol_name in &file_brief.symbols {
            let symbol_hash = fnv1a_hash(symbol_name);

            // Find external callers
            for caller_hash in find_callers(symbol_hash) {
                if let Some(caller_name) = string_resolver(caller_hash) {
                    callers.push(SymbolRef {
                        name: caller_name,
                        file: String::new(),
                        relationship: format!("calls {}", symbol_name),
                    });
                }
            }

            // Find external dependencies
            for dep_hash in find_deps(symbol_hash) {
                if let Some(dep_name) = string_resolver(dep_hash) {
                    deps.push(SymbolRef {
                        name: dep_name,
                        file: String::new(),
                        relationship: format!("depended by {}", symbol_name),
                    });
                }
            }
        }
    }

    // Deduplicate
    callers.dedup_by(|a, b| a.name == b.name);
    deps.dedup_by(|a, b| a.name == b.name);

    // Update narrative with relationship info
    if !callers.is_empty() || !deps.is_empty() {
        let mut extra = String::new();
        if !callers.is_empty() {
            extra.push_str(&format!(
                "\nExternal callers (must not break): {}",
                callers.iter().map(|c| c.name.as_str()).take(10).collect::<Vec<_>>().join(", ")
            ));
        }
        if !deps.is_empty() {
            extra.push_str(&format!(
                "\nExternal dependencies (must respect): {}",
                deps.iter().map(|d| d.name.as_str()).take(10).collect::<Vec<_>>().join(", ")
            ));
        }
        brief.narrative.push_str(&extra);
    }

    brief.external_callers = callers;
    brief.external_dependencies = deps;
}

/// Simple FNV-1a hash for symbol name → hash matching.
fn fnv1a_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_and_serialize_ledger() {
        let ledger = ContinuationLedger::capture(
            "task-7",
            "Refactor TLS handshake to async",
            "refactor",
            &[PathBuf::from("src/tls/handshake.rs")],
            Path::new("/tmp/workspace"),
            0xDEADBEEF,
            "I'll start by extracting the blocking calls...\nDecision: use tokio::io",
            &["src/tls/handshake.rs".to_string()],
            &["Modified TLS module".to_string()],
            "OpenRouter",
            "Claude Sonnet",
            "claude-sonnet-4-20250514",
            Duration::from_secs(45),
            false,
        );

        let serialized = ledger.serialize();
        assert!(serialized.contains("continuation-ledger version 1"));
        assert!(serialized.contains("Refactor TLS"));
        assert!(serialized.contains("site_map_root"));
        assert!(serialized.contains("Claude Sonnet"));
    }

    #[test]
    fn continuation_prompt_includes_critical_sections() {
        let ledger = ContinuationLedger {
            id: "task-1-attempt-2".into(),
            mission: MissionSpec {
                goal: "Fix the auth bug".into(),
                task_kind: "bugfix".into(),
                expectations: vec!["Don't break login flow".into()],
                constraints: vec![],
            },
            environment: ScopeEnvironmentBrief {
                scoped_files: vec![ScopedFileBrief {
                    path: PathBuf::from("src/auth.rs"),
                    symbols: vec!["validate_token".into(), "refresh_session".into()],
                    line_count: 150,
                    role: "core implementation".into(),
                }],
                external_callers: vec![SymbolRef {
                    name: "login_handler".into(),
                    file: "src/routes.rs".into(),
                    relationship: "calls validate_token".into(),
                }],
                external_dependencies: vec![],
                internal_relationships: vec![],
                narrative: "Scoped: src/auth.rs (150 lines, auth module)".into(),
            },
            journal: EditJournal {
                completed_edits: vec![FileEdit {
                    path: PathBuf::from("src/auth.rs"),
                    diff: "+  if token.is_expired() { return Err(...) }".into(),
                    intent: "Added token expiry check".into(),
                }],
                partial_edit: Some(PartialFileEdit {
                    path: PathBuf::from("src/session.rs"),
                    original_content: "fn refresh() {}".into(),
                    current_content: "fn refresh() { // partial".into(),
                    intent: "Adding session refresh logic".into(),
                    editing_region: Some((10, 25)),
                    compiles: Some(false),
                }),
                created_files: vec![],
                deleted_files: vec![],
            },
            progress: ProgressState {
                steps: vec![
                    ProgressStep { description: "Fix token validation".into(), status: StepStatus::Done },
                    ProgressStep { description: "Update session refresh".into(), status: StepStatus::InProgress },
                    ProgressStep { description: "Add integration test".into(), status: StepStatus::Pending },
                ],
                overall_percent: 33.0,
            },
            provenance: vec![ModelAttemptRecord {
                provider: "OpenRouter".into(),
                model_id: "gpt-4".into(),
                model_label: "GPT-4".into(),
                started_at: SystemTime::now(),
                duration: Duration::from_secs(30),
                outcome: AttemptOutcome::PartialSuccess,
                key_decisions: vec!["Using Result<T> for error propagation".into()],
                failure_reason: Some("Context limit reached".into()),
            }],
            captured_at: SystemTime::now(),
            site_map_root: 0xCAFE,
        };

        let prompt = ledger.continuation_prompt();
        assert!(prompt.contains("## Mission"));
        assert!(prompt.contains("Fix the auth bug"));
        assert!(prompt.contains("## Completed Edits"));
        assert!(prompt.contains("Added token expiry check"));
        assert!(prompt.contains("## ⚠ Partial Edit In Progress"));
        assert!(prompt.contains("does NOT compile"));
        assert!(prompt.contains("do NOT redo work"));
        assert!(prompt.contains("## Progress"));
        assert!(prompt.contains("[x] Fix token validation"));
        assert!(prompt.contains("[~] Update session refresh"));
        assert!(prompt.contains("[ ] Add integration test"));
        assert!(prompt.contains("## Previous Attempt"));
        assert!(prompt.contains("GPT-4"));
        assert!(prompt.contains("Context limit reached"));
    }

    #[test]
    fn extract_key_decisions_from_transcript() {
        let transcript = "Looking at the code...\nDecision: use async/await pattern\nI'll refactor the blocking call first\nSome other text\nApproach: extract into separate module";
        let decisions = extract_key_decisions(transcript);
        assert_eq!(decisions.len(), 3);
        assert!(decisions[0].contains("async/await"));
        assert!(decisions[1].contains("refactor"));
        assert!(decisions[2].contains("extract"));
    }

    #[test]
    fn progress_inference() {
        let journal = EditJournal {
            completed_edits: vec![FileEdit {
                path: PathBuf::from("a.rs"),
                diff: String::new(),
                intent: "done".into(),
            }],
            partial_edit: Some(PartialFileEdit {
                path: PathBuf::from("b.rs"),
                original_content: String::new(),
                current_content: String::new(),
                intent: "in progress".into(),
                editing_region: None,
                compiles: None,
            }),
            created_files: vec![],
            deleted_files: vec![],
        };
        let status_updates = vec!["completed step 1".into(), "starting step 2".into()];
        let progress = infer_progress("some transcript", &journal, &status_updates);
        assert!(!progress.steps.is_empty());
        assert!(progress.steps.iter().any(|s| s.status == StepStatus::InProgress));
    }

    #[test]
    fn estimated_tokens_reasonable() {
        let ledger = ContinuationLedger::capture(
            "task-1",
            "Simple fix",
            "bugfix",
            &[PathBuf::from("src/main.rs")],
            Path::new("/tmp"),
            42,
            "",
            &[],
            &[],
            "test",
            "test-model",
            "test-id",
            Duration::from_secs(5),
            true,
        );
        // A simple ledger should be compact (well under 1000 tokens)
        assert!(ledger.estimated_tokens() < 1000);
    }

    #[test]
    fn infer_file_roles() {
        assert_eq!(infer_file_role(Path::new("tests/auth_test.rs"), &[]), "test module");
        assert_eq!(infer_file_role(Path::new("src/mod.rs"), &[]), "module root");
        assert_eq!(infer_file_role(Path::new("config.toml"), &[]), "configuration");
        assert_eq!(
            infer_file_role(Path::new("src/app.rs"), &["main".into()]),
            "entry point"
        );
    }
}
