//! Agent self-improvement loop: failure analysis + prompt refinement.
//!
//! At the end of each agent session (or when errors accumulate), this module:
//! 1. Analyzes failure patterns from tool execution results
//! 2. Categorizes errors (syntax, logic, permission, timeout, dependency)
//! 3. Generates refined system-prompt directives to avoid repeated mistakes
//! 4. Persists improvement insights via PersistentMemory for future sessions

use super::memory_store::PersistentMemory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Categories of tool/execution failures for pattern extraction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureCategory {
    /// Compilation or syntax errors in generated code.
    Syntax,
    /// Logic errors — code compiles but produces wrong results.
    Logic,
    /// Permission denied or access violations.
    Permission,
    /// Operation timed out or resource exhausted.
    Timeout,
    /// Missing dependency, module not found, unresolved import.
    Dependency,
    /// File not found, path resolution failure.
    NotFound,
    /// User rejected the tool call.
    Rejected,
    /// Network or connectivity failure.
    Network,
    /// Uncategorized failure.
    Unknown,
}

impl FailureCategory {
    /// Classify an error message into a failure category.
    pub fn classify(error_text: &str) -> Self {
        let lower = error_text.to_lowercase();
        if lower.contains("syntax")
            || lower.contains("parse")
            || lower.contains("unexpected token")
            || lower.contains("expected") && lower.contains("got")
            || lower.contains("compilation")
            || lower.contains("compile error")
            || lower.contains("cannot find") && lower.contains("in this scope")
        {
            Self::Syntax
        } else if lower.contains("permission")
            || lower.contains("access denied")
            || lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("locked by another")
        {
            Self::Permission
        } else if lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("deadline")
            || lower.contains("rate limit")
        {
            Self::Timeout
        } else if lower.contains("not found")
            || lower.contains("no such file")
            || lower.contains("does not exist")
            || lower.contains("missing file")
            || lower.contains("enoent")
        {
            Self::NotFound
        } else if lower.contains("dependency")
            || lower.contains("unresolved import")
            || lower.contains("module not found")
            || lower.contains("crate")
            || lower.contains("package") && lower.contains("not installed")
        {
            Self::Dependency
        } else if lower.contains("network")
            || lower.contains("connection")
            || lower.contains("dns")
            || lower.contains("unreachable")
            || lower.contains("socket")
        {
            Self::Network
        } else if lower.contains("rejected by the user") || lower.contains("rejected") {
            Self::Rejected
        } else if lower.contains("logic")
            || lower.contains("assertion")
            || lower.contains("wrong")
            || lower.contains("incorrect")
            || lower.contains("mismatch")
        {
            Self::Logic
        } else {
            Self::Unknown
        }
    }

    /// Generate a corrective directive for this failure category.
    pub fn directive(&self) -> &'static str {
        match self {
            Self::Syntax => "Before writing code, verify syntax against the target language's grammar. Run incremental compilation checks after each file modification.",
            Self::Logic => "After implementing logic, write or run assertions to validate correctness. Trace edge cases before submitting.",
            Self::Permission => "Check file permissions and ownership before write operations. Prefer workspace-relative paths and avoid system directories.",
            Self::Timeout => "Break long operations into smaller steps. Add progress checkpoints and consider async execution for heavy tasks.",
            Self::Dependency => "Verify all imports and dependencies exist before use. Check Cargo.toml/package.json for required packages.",
            Self::NotFound => "Verify file paths exist before reading/writing. Use glob or search to locate files rather than assuming paths.",
            Self::Rejected => "Explain the intended action clearly before requesting tool approval. Provide context for why the operation is needed.",
            Self::Network => "Implement retry with backoff for network operations. Cache responses when possible and handle offline gracefully.",
            Self::Unknown => "Log detailed error context for diagnosis. Consider alternative approaches when an operation fails repeatedly.",
        }
    }
}

/// A recorded failure event with context for analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    /// Tool that produced the failure.
    pub tool_name: String,
    /// The error output text.
    pub error_text: String,
    /// Classified category.
    pub category: FailureCategory,
    /// Loop iteration when the failure occurred.
    pub loop_index: usize,
    /// Unix timestamp.
    pub timestamp: u64,
}

/// Accumulated failure statistics per category.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailureStats {
    /// Count of failures per category.
    pub category_counts: HashMap<String, usize>,
    /// Count of failures per tool.
    pub tool_counts: HashMap<String, usize>,
    /// Total failures this session.
    pub total_failures: usize,
    /// Total successes this session.
    pub total_successes: usize,
}

/// The self-improvement engine. Collects failures during a session,
/// then analyzes patterns and generates prompt refinements.
pub struct ImprovementEngine {
    /// Failures collected during the current session.
    failures: Vec<FailureRecord>,
    /// Success count for ratio computation.
    success_count: usize,
    /// Accumulated stats (loaded from memory).
    stats: FailureStats,
}

/// A generated prompt refinement directive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDirective {
    /// The failure category this addresses.
    pub category: FailureCategory,
    /// The directive text to inject into the system prompt.
    pub directive: String,
    /// Confidence based on failure frequency (0.0–1.0).
    pub confidence: f64,
    /// How many times this pattern was observed.
    pub occurrences: usize,
}

impl ImprovementEngine {
    /// Create a new improvement engine, loading historical stats from memory.
    pub fn new(memory: &PersistentMemory) -> Self {
        let stats = memory
            .recall("failure statistics", 1)
            .first()
            .and_then(|hit| serde_json::from_str::<FailureStats>(&hit.entry.content).ok())
            .unwrap_or_default();
        Self {
            failures: Vec::new(),
            success_count: 0,
            stats,
        }
    }

    /// Record a tool failure for later analysis.
    pub fn record_failure(&mut self, tool_name: &str, error_text: &str, loop_index: usize) {
        let category = FailureCategory::classify(error_text);
        self.failures.push(FailureRecord {
            tool_name: tool_name.to_string(),
            error_text: if error_text.len() > 500 {
                error_text[..500].to_string()
            } else {
                error_text.to_string()
            },
            category,
            loop_index,
            timestamp: current_ts(),
        });
    }

    /// Record a tool success.
    pub fn record_success(&mut self) {
        self.success_count += 1;
    }

    /// Number of failures recorded this session.
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// Whether the engine has enough data to produce meaningful analysis.
    pub fn has_data(&self) -> bool {
        !self.failures.is_empty()
    }

    /// Analyze collected failures and produce prompt refinement directives.
    /// Returns directives sorted by confidence (highest first).
    pub fn analyze(&self) -> Vec<PromptDirective> {
        if self.failures.is_empty() {
            return Vec::new();
        }

        // Count failures per category
        let mut cat_counts: HashMap<&FailureCategory, usize> = HashMap::new();
        for f in &self.failures {
            *cat_counts.entry(&f.category).or_default() += 1;
        }

        let total = self.failures.len() as f64;
        let mut directives: Vec<PromptDirective> = cat_counts
            .iter()
            .map(|(cat, count)| {
                let confidence = (*count as f64 / total).min(1.0);
                PromptDirective {
                    category: (*cat).clone(),
                    directive: cat.directive().to_string(),
                    confidence,
                    occurrences: *count,
                }
            })
            .collect();

        // Sort by confidence descending
        directives.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        directives
    }

    /// Generate a system-prompt addendum block from the analysis.
    /// Only includes directives with confidence >= threshold.
    pub fn generate_prompt_addendum(&self, threshold: f64) -> Option<String> {
        let directives = self.analyze();
        let relevant: Vec<&PromptDirective> = directives
            .iter()
            .filter(|d| d.confidence >= threshold && d.occurrences >= 2)
            .collect();

        if relevant.is_empty() {
            return None;
        }

        let mut block = String::from("\n\n## Learned Failure Patterns (auto-generated)\n");
        block.push_str("Avoid these previously-observed failure modes:\n");
        for d in &relevant {
            block.push_str(&format!(
                "- [{:?}] ({}x, {:.0}% confidence): {}\n",
                d.category,
                d.occurrences,
                d.confidence * 100.0,
                d.directive
            ));
        }
        Some(block)
    }

    /// Persist session analysis into long-term memory.
    /// Updates cumulative stats and stores generated directives.
    pub fn persist_to_memory(&mut self, memory: &mut PersistentMemory) {
        // Update cumulative stats
        for f in &self.failures {
            let cat_key = format!("{:?}", f.category);
            *self.stats.category_counts.entry(cat_key).or_default() += 1;
            *self
                .stats
                .tool_counts
                .entry(f.tool_name.clone())
                .or_default() += 1;
        }
        self.stats.total_failures += self.failures.len();
        self.stats.total_successes += self.success_count;

        // Store stats
        if let Ok(stats_json) = serde_json::to_string(&self.stats) {
            memory.remember(
                "self_improve:stats",
                &stats_json,
                &["self_improve", "stats"],
                0.9,
            );
        }

        // Store directives as high-priority memories
        let directives = self.analyze();
        for d in directives.iter().filter(|d| d.occurrences >= 2) {
            let key = format!("self_improve:directive:{:?}", d.category);
            memory.remember(
                &key,
                &d.directive,
                &[
                    "self_improve",
                    "directive",
                    &format!("{:?}", d.category).to_lowercase(),
                ],
                d.confidence,
            );
        }

        // Store failure ratio for trend analysis
        let total_ops = self.stats.total_failures + self.stats.total_successes;
        if total_ops > 0 {
            let ratio = self.stats.total_failures as f64 / total_ops as f64;
            memory.remember(
                "self_improve:failure_ratio",
                &format!("{:.3}", ratio),
                &["self_improve", "trend"],
                1.0 - ratio, // Lower ratio = higher score
            );
        }
    }

    /// Load previously-learned directives from memory for prompt injection.
    /// Called at session start to inject historical learnings.
    pub fn recall_directives(memory: &PersistentMemory, limit: usize) -> Vec<String> {
        memory
            .recall("self_improve directive failure pattern", limit)
            .iter()
            .filter(|hit| hit.entry.tags.contains(&"directive".to_string()))
            .map(|hit| hit.entry.content.clone())
            .collect()
    }
}

fn current_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_syntax_errors() {
        assert_eq!(
            FailureCategory::classify("error: expected ';', got '}'"),
            FailureCategory::Syntax
        );
        assert_eq!(
            FailureCategory::classify("compilation failed: unexpected token"),
            FailureCategory::Syntax
        );
    }

    #[test]
    fn classify_permission_errors() {
        assert_eq!(
            FailureCategory::classify("Error: permission denied for /etc/passwd"),
            FailureCategory::Permission
        );
        assert_eq!(
            FailureCategory::classify("access denied: unauthorized"),
            FailureCategory::Permission
        );
    }

    #[test]
    fn classify_not_found() {
        assert_eq!(
            FailureCategory::classify("no such file or directory"),
            FailureCategory::NotFound
        );
    }

    #[test]
    fn analyze_produces_directives() {
        let dir = tempfile::tempdir().unwrap();
        let mem = PersistentMemory::open(dir.path());
        let mut engine = ImprovementEngine::new(&mem);

        engine.record_failure("write_file", "error: expected ';', got '}'", 1);
        engine.record_failure("write_file", "syntax error: unexpected token", 2);
        engine.record_failure("run_command", "permission denied", 3);
        engine.record_success();

        let directives = engine.analyze();
        assert!(!directives.is_empty());
        // Syntax should be highest confidence (2/3 failures)
        assert_eq!(directives[0].category, FailureCategory::Syntax);
        assert_eq!(directives[0].occurrences, 2);
    }

    #[test]
    fn generate_addendum_filters_low_confidence() {
        let dir = tempfile::tempdir().unwrap();
        let mem = PersistentMemory::open(dir.path());
        let mut engine = ImprovementEngine::new(&mem);

        // Single failure — below threshold of 2 occurrences
        engine.record_failure("write_file", "syntax error: unexpected token", 1);
        assert!(engine.generate_prompt_addendum(0.3).is_none());

        // Add second failure of same category (Syntax)
        engine.record_failure("write_file", "compilation failed: parse error", 2);
        let addendum = engine.generate_prompt_addendum(0.3);
        assert!(addendum.is_some());
        assert!(addendum.unwrap().contains("Learned Failure Patterns"));
    }

    #[test]
    fn persist_and_recall_directives() {
        let dir = tempfile::tempdir().unwrap();
        let mut mem = PersistentMemory::open(dir.path());
        let mut engine = ImprovementEngine::new(&mem);

        engine.record_failure("write_file", "error: expected ';', got '}'", 1);
        engine.record_failure("write_file", "syntax error: unexpected token", 2);
        engine.record_failure("write_file", "parse error at line 5", 3);
        engine.persist_to_memory(&mut mem);

        let recalled = ImprovementEngine::recall_directives(&mem, 5);
        assert!(!recalled.is_empty());
        assert!(recalled[0].contains("syntax") || recalled[0].contains("compilation"));
    }

    #[test]
    fn classify_timeout_errors() {
        assert_eq!(
            FailureCategory::classify("operation timed out after 30s"),
            FailureCategory::Timeout
        );
        assert_eq!(
            FailureCategory::classify("rate limit exceeded"),
            FailureCategory::Timeout
        );
        assert_eq!(
            FailureCategory::classify("deadline exceeded"),
            FailureCategory::Timeout
        );
    }

    #[test]
    fn classify_dependency_errors() {
        assert_eq!(
            FailureCategory::classify("unresolved import serde"),
            FailureCategory::Dependency
        );
        assert_eq!(
            FailureCategory::classify("dependency not satisfied: missing crate"),
            FailureCategory::Dependency
        );
    }

    #[test]
    fn classify_network_errors() {
        assert_eq!(
            FailureCategory::classify("connection refused"),
            FailureCategory::Network
        );
        assert_eq!(
            FailureCategory::classify("dns resolution failed"),
            FailureCategory::Network
        );
        assert_eq!(
            FailureCategory::classify("host unreachable"),
            FailureCategory::Network
        );
    }

    #[test]
    fn classify_rejected_errors() {
        assert_eq!(
            FailureCategory::classify("rejected by the user"),
            FailureCategory::Rejected
        );
    }

    #[test]
    fn classify_unknown_fallback() {
        assert_eq!(
            FailureCategory::classify("something completely unexpected happened"),
            FailureCategory::Unknown
        );
    }

    #[test]
    fn directive_returns_non_empty_for_all_categories() {
        let categories = [
            FailureCategory::Syntax,
            FailureCategory::Logic,
            FailureCategory::Permission,
            FailureCategory::Timeout,
            FailureCategory::Dependency,
            FailureCategory::NotFound,
            FailureCategory::Rejected,
            FailureCategory::Network,
            FailureCategory::Unknown,
        ];
        for cat in &categories {
            assert!(!cat.directive().is_empty(), "directive for {:?} is empty", cat);
        }
    }

    #[test]
    fn error_text_is_truncated_at_500_chars() {
        let dir = tempfile::tempdir().unwrap();
        let mem = PersistentMemory::open(dir.path());
        let mut engine = ImprovementEngine::new(&mem);

        let long_error = "x".repeat(1000);
        engine.record_failure("write_file", &long_error, 1);
        assert_eq!(engine.failures[0].error_text.len(), 500);
    }

    #[test]
    fn empty_engine_has_no_data() {
        let dir = tempfile::tempdir().unwrap();
        let mem = PersistentMemory::open(dir.path());
        let engine = ImprovementEngine::new(&mem);
        assert!(!engine.has_data());
        assert_eq!(engine.failure_count(), 0);
        assert!(engine.analyze().is_empty());
    }
}
