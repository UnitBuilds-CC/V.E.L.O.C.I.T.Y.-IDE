//! Inline agent suggestions: ghost text displayed semi-transparently in the
//! editor at the cursor position. Powered by AI completion requests that run
//! in the background and produce predictive next-edits.

use crate::safety::SafeMutex;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Request payload for an inline suggestion from the LLM.
#[derive(Debug, Clone)]
pub struct SuggestionRequest {
    pub file_path: PathBuf,
    pub prefix: String,
    pub suffix: String,
    pub language: String,
}

/// A ghost-text suggestion to render inline at the cursor.
#[derive(Debug, Clone)]
pub struct InlineSuggestion {
    /// The suggested text to insert.
    pub text: String,
    /// Line number (1-based) where the suggestion applies.
    pub line: usize,
    /// Column offset (0-based) where the ghost text starts.
    pub column: usize,
    /// Confidence score (0.0..1.0).
    pub confidence: f32,
    /// When this suggestion was generated.
    pub generated_at: Instant,
    /// Source label (model name or "copilot").
    pub source: String,
}

/// The state machine for inline suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionState {
    /// No suggestion active.
    Idle,
    /// Request in flight, waiting for AI response.
    Loading,
    /// Suggestion displayed, awaiting accept/dismiss.
    Showing,
    /// User accepted (Tab), applying text.
    Accepted,
    /// User dismissed (Esc), clearing.
    Dismissed,
}

/// Configuration for the inline suggestion engine.
#[derive(Debug, Clone)]
pub struct SuggestionConfig {
    /// Minimum idle time (ms) before triggering a suggestion request.
    pub trigger_delay_ms: u64,
    /// Maximum suggestion length in characters.
    pub max_suggestion_chars: usize,
    /// Whether multi-line suggestions are allowed.
    pub allow_multiline: bool,
    /// Minimum confidence threshold to show.
    pub min_confidence: f32,
    /// Whether suggestions are enabled globally.
    pub enabled: bool,
}

impl Default for SuggestionConfig {
    fn default() -> Self {
        Self {
            trigger_delay_ms: 500,
            max_suggestion_chars: 200,
            allow_multiline: true,
            min_confidence: 0.3,
            enabled: true,
        }
    }
}

/// Shared suggestion state between the UI thread and the background completion thread.
#[derive(Debug, Clone)]
pub struct SharedSuggestion {
    inner: Arc<Mutex<Option<InlineSuggestion>>>,
}

impl Default for SharedSuggestion {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSuggestion {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Set a new suggestion (called from background thread).
    pub fn set(&self, suggestion: InlineSuggestion) {
        *self.inner.lock_safe() = Some(suggestion);
    }

    /// Take the current suggestion (consuming it).
    pub fn take(&self) -> Option<InlineSuggestion> {
        self.inner.lock_safe().take()
    }

    /// Peek at the current suggestion.
    pub fn peek(&self) -> Option<InlineSuggestion> {
        self.inner.lock_safe().clone()
    }

    /// Clear any pending suggestion.
    pub fn clear(&self) {
        *self.inner.lock_safe() = None;
    }
}

/// Manages the inline suggestion lifecycle in the editor.
#[derive(Debug)]
pub struct InlineSuggestionEngine {
    pub config: SuggestionConfig,
    pub state: SuggestionState,
    pub current_suggestion: Option<InlineSuggestion>,
    pub shared: SharedSuggestion,
    /// Last cursor position that triggered a request.
    pub last_trigger_line: usize,
    pub last_trigger_col: usize,
    /// When the cursor last moved (for debounce).
    pub last_cursor_move: Instant,
    /// Statistics.
    pub total_shown: usize,
    pub total_accepted: usize,
    pub total_dismissed: usize,
    /// Recent suggestion cache for dedup.
    pub recent_suggestions: Vec<String>,
    /// Max cache size.
    pub cache_size: usize,
    /// Reuse cache: request key ? previously produced completion. Avoids
    /// re-querying the model for identical contexts (latency + cost saving).
    pub suggestion_cache: Vec<(String, String)>,
    /// Interaction history for acceptance telemetry.
    pub history: SuggestionHistory,
    /// Cache key for the in-flight request, so a successful result can be
    /// stored in the reuse cache when it arrives via [`poll`](Self::poll).
    pub pending_cache_key: Option<String>,
}

/// History of suggestion interactions for analytics.
#[derive(Debug, Clone)]
pub struct SuggestionHistory {
    pub entries: Vec<HistoryEntry>,
    pub max_entries: usize,
}

/// A single history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub timestamp: Instant,
    pub file_path: String,
    pub language: String,
    pub suggestion_text: String,
    pub was_accepted: bool,
    pub latency_ms: u64,
}

impl Default for InlineSuggestionEngine {
    fn default() -> Self {
        Self::new(SuggestionConfig::default())
    }
}

impl InlineSuggestionEngine {
    pub fn new(config: SuggestionConfig) -> Self {
        Self {
            config,
            state: SuggestionState::Idle,
            current_suggestion: None,
            shared: SharedSuggestion::new(),
            last_trigger_line: 0,
            last_trigger_col: 0,
            last_cursor_move: Instant::now(),
            total_shown: 0,
            total_accepted: 0,
            total_dismissed: 0,
            recent_suggestions: Vec::new(),
            cache_size: 50,
            suggestion_cache: Vec::new(),
            history: SuggestionHistory::new(200),
            pending_cache_key: None,
        }
    }

    /// Check if a suggestion text was recently shown (to avoid repeats).
    pub fn is_duplicate(&self, text: &str) -> bool {
        self.recent_suggestions
            .iter()
            .rev()
            .take(10)
            .any(|s| s == text)
    }

    /// Record a suggestion in the recent cache.
    fn cache_suggestion(&mut self, text: &str) {
        if !text.is_empty() {
            self.recent_suggestions.push(text.to_string());
            if self.recent_suggestions.len() > self.cache_size {
                self.recent_suggestions.remove(0);
            }
        }
    }

    /// Called each frame to check for new suggestions from the background thread.
    pub fn poll(&mut self) {
        if let Some(suggestion) = self.shared.take() {
            if suggestion.confidence >= self.config.min_confidence
                && !self.is_duplicate(&suggestion.text)
            {
                // Store the successful result in the reuse cache so identical
                // future contexts are served without a model call.
                if let Some(key) = self.pending_cache_key.take() {
                    if !suggestion.source.contains("(cache)") {
                        self.suggestion_cache.push((key, suggestion.text.clone()));
                        if self.suggestion_cache.len() > self.cache_size {
                            self.suggestion_cache.remove(0);
                        }
                    }
                }
                self.cache_suggestion(&suggestion.text);
                self.current_suggestion = Some(suggestion);
                self.state = SuggestionState::Showing;
                self.total_shown += 1;
            }
        }
    }

    /// Signal that the cursor moved to a new position. Resets debounce timer.
    pub fn cursor_moved(&mut self, line: usize, col: usize) {
        if self.state == SuggestionState::Showing {
            // Cursor moved away from suggestion — dismiss
            if let Some(ref sugg) = self.current_suggestion {
                if sugg.line != line || sugg.column != col {
                    self.dismiss();
                }
            }
        }
        self.last_trigger_line = line;
        self.last_trigger_col = col;
        self.last_cursor_move = Instant::now();
    }

    /// Check if enough time has passed to trigger a suggestion request.
    pub fn should_trigger(&self) -> bool {
        if !self.config.enabled || self.state != SuggestionState::Idle {
            return false;
        }
        self.last_cursor_move.elapsed().as_millis() as u64 >= self.config.trigger_delay_ms
    }

    /// Accept the current suggestion (Tab key).
    pub fn accept(&mut self) -> Option<String> {
        if self.state == SuggestionState::Showing {
            self.state = SuggestionState::Accepted;
            self.total_accepted += 1;
            let suggestion = self.current_suggestion.take();
            if let Some(ref s) = suggestion {
                self.record_interaction(s, true);
            }
            self.state = SuggestionState::Idle;
            suggestion.map(|s| s.text)
        } else {
            None
        }
    }

    /// Dismiss the current suggestion (Esc key or content change).
    pub fn dismiss(&mut self) {
        if self.state == SuggestionState::Showing {
            self.total_dismissed += 1;
            let suggestion = self.current_suggestion.take();
            if let Some(ref s) = suggestion {
                self.record_interaction(s, false);
            }
        }
        self.current_suggestion = None;
        self.state = SuggestionState::Idle;
    }

    /// Record an accept/dismiss event into the telemetry history.
    fn record_interaction(&mut self, suggestion: &InlineSuggestion, accepted: bool) {
        self.history.record(HistoryEntry {
            timestamp: Instant::now(),
            file_path: String::new(),
            language: String::new(),
            suggestion_text: suggestion.text.clone(),
            was_accepted: accepted,
            latency_ms: suggestion.generated_at.elapsed().as_millis() as u64,
        });
    }

    /// Look up a cached completion for an identical request context.
    pub fn cached_completion(&self, request: &SuggestionRequest) -> Option<String> {
        let key = cache_key(request);
        self.suggestion_cache
            .iter()
            .find(|(k, _)| k == &key)
            .map(|(_, v)| v.clone())
    }

    /// Store a completion in the reuse cache (bounded, FIFO eviction).
    pub fn cache_completion(&mut self, request: &SuggestionRequest, completion: &str) {
        if completion.is_empty() {
            return;
        }
        let key = cache_key(request);
        if let Some(slot) = self.suggestion_cache.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = completion.to_string();
            return;
        }
        self.suggestion_cache.push((key, completion.to_string()));
        if self.suggestion_cache.len() > self.cache_size {
            self.suggestion_cache.remove(0);
        }
    }

    /// Get the ghost text to render (if any).
    pub fn ghost_text(&self) -> Option<&InlineSuggestion> {
        if self.state == SuggestionState::Showing {
            self.current_suggestion.as_ref()
        } else {
            None
        }
    }

    /// Acceptance rate as a percentage.
    pub fn acceptance_rate(&self) -> f32 {
        if self.total_shown == 0 {
            0.0
        } else {
            (self.total_accepted as f32 / self.total_shown as f32) * 100.0
        }
    }

    /// Submit a suggestion request to the background inference pipeline.
    /// A worker thread runs a scoped headless sub-agent completion and, on a
    /// non-empty result, publishes it to `shared` for the UI thread to pick up
    /// via [`poll`](Self::poll).
    pub fn submit_request(
        &mut self,
        request: SuggestionRequest,
        provider: crate::agent::AiProvider,
        model: &str,
        workspace_root: std::path::PathBuf,
    ) {
        if !self.config.enabled {
            return;
        }
        self.state = SuggestionState::Loading;

        // Fast path: serve an identical prior context from the reuse cache
        // without spawning a model call.
        if let Some(cached) = self.cached_completion(&request) {
            let line = self.last_trigger_line;
            let col = self.last_trigger_col;
            let source_label = format!("{}/{} (cache)", provider.label(), model);
            self.shared.set(InlineSuggestion {
                text: cached,
                line,
                column: col,
                confidence: 0.8,
                generated_at: Instant::now(),
                source: source_label,
            });
            return;
        }

        // Build the completion context prompt for the model.
        let prompt = format!(
            "Complete the following {} code. Return ONLY the raw completion text that \
             should be inserted at the cursor - no explanation, no markdown fences.\n\n\
             ```\n{}\n```\n\nContinue from where the code ends:",
            request.language, request.prefix
        );

        // Capture what the worker needs.
        let shared = self.shared.clone();
        let line = self.last_trigger_line;
        let col = self.last_trigger_col;
        // Remember the cache key so poll() can store the result on success.
        self.pending_cache_key = Some(cache_key(&request));
        let source_label = format!("{}/{}", provider.label(), model);
        let model = model.to_string();
        let max_chars = self.config.max_suggestion_chars;
        let allow_multiline = self.config.allow_multiline;
        let file_path = request.file_path.clone();

        std::thread::spawn(move || {
            let req = crate::agent::HeadlessSubAgentRequest {
                workspace_root,
                provider,
                model,
                thinking: false,
                prompt,
                cancel_rx: None,
                progress: None,
                scoped_files: Some(vec![file_path]),
            };
            let result = crate::agent::run_headless_subagent(req);
            let completion = sanitize_completion(&result.transcript, max_chars, allow_multiline);
            if !completion.is_empty() {
                shared.set(InlineSuggestion {
                    text: completion,
                    line,
                    column: col,
                    // Heuristic confidence: a concrete completion arrived.
                    confidence: 0.75,
                    generated_at: Instant::now(),
                    source: source_label,
                });
            }
        });
    }
}

/// Deterministic key for the reuse cache: identical language + prefix + suffix
/// contexts map to the same key so a prior completion can be replayed.
fn cache_key(request: &SuggestionRequest) -> String {
    format!(
        "{}\u{1}{}\u{1}{}",
        request.language, request.prefix, request.suffix
    )
}

/// Turn a raw model transcript into insertable ghost text: strip markdown code
/// fences and surrounding prose, honour the single-/multi-line policy, and cap
/// the length.
fn sanitize_completion(transcript: &str, max_chars: usize, allow_multiline: bool) -> String {
    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // If the model wrapped the answer in a fenced code block, keep only the
    // block body (the first fenced region).
    let body = if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        // Skip an optional language tag on the opening fence line.
        let after = match after.find('\n') {
            Some(nl) => &after[nl + 1..],
            None => after,
        };
        match after.find("```") {
            Some(end) => &after[..end],
            None => after,
        }
    } else {
        trimmed
    };

    let body = body.trim_matches('\n');
    let mut out = if allow_multiline {
        body.to_string()
    } else {
        body.lines().next().unwrap_or("").to_string()
    };

    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect();
    }
    out.trim_end().to_string()
}

/// Build a completion context from the editor buffer for the AI request.
pub fn build_completion_context(
    content: &str,
    cursor_line: usize,
    cursor_col: usize,
    max_prefix_lines: usize,
    max_suffix_lines: usize,
) -> CompletionContext {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = cursor_line.saturating_sub(1);

    let prefix_start = line_idx.saturating_sub(max_prefix_lines);
    let suffix_end = (line_idx + max_suffix_lines + 1).min(lines.len());

    let prefix: Vec<&str> = lines[prefix_start..=line_idx.min(lines.len() - 1)].to_vec();
    let suffix: Vec<&str> = if line_idx + 1 < lines.len() {
        lines[(line_idx + 1)..suffix_end].to_vec()
    } else {
        Vec::new()
    };

    CompletionContext {
        prefix: prefix.join("\n"),
        suffix: suffix.join("\n"),
        cursor_line,
        cursor_col,
        language: String::new(),
    }
}

impl SuggestionHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    pub fn record(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
    }

    pub fn acceptance_rate(&self) -> f32 {
        if self.entries.is_empty() {
            return 0.0;
        }
        let accepted = self.entries.iter().filter(|e| e.was_accepted).count();
        (accepted as f32 / self.entries.len() as f32) * 100.0
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.entries.is_empty() {
            return 0.0;
        }
        let sum: u64 = self.entries.iter().map(|e| e.latency_ms).sum();
        sum as f64 / self.entries.len() as f64
    }

    pub fn entries_for_language(&self, lang: &str) -> Vec<&HistoryEntry> {
        self.entries.iter().filter(|e| e.language == lang).collect()
    }
}

/// Context sent to the AI for completion.
#[derive(Debug, Clone)]
pub struct CompletionContext {
    pub prefix: String,
    pub suffix: String,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub language: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestion_engine_lifecycle() {
        let mut engine = InlineSuggestionEngine::default();
        assert_eq!(engine.state, SuggestionState::Idle);

        // Simulate background thread delivering a suggestion
        engine.shared.set(InlineSuggestion {
            text: "println!(\"hello\");".into(),
            line: 5,
            column: 4,
            confidence: 0.8,
            generated_at: Instant::now(),
            source: "test-model".into(),
        });

        engine.poll();
        assert_eq!(engine.state, SuggestionState::Showing);
        assert!(engine.ghost_text().is_some());
        assert_eq!(engine.total_shown, 1);

        // Accept
        let text = engine.accept();
        assert_eq!(text, Some("println!(\"hello\");".to_string()));
        assert_eq!(engine.state, SuggestionState::Idle);
        assert_eq!(engine.total_accepted, 1);
    }

    #[test]
    fn suggestion_dismiss() {
        let mut engine = InlineSuggestionEngine::default();
        engine.shared.set(InlineSuggestion {
            text: "test".into(),
            line: 1,
            column: 0,
            confidence: 0.9,
            generated_at: Instant::now(),
            source: "test".into(),
        });
        engine.poll();
        assert_eq!(engine.state, SuggestionState::Showing);

        engine.dismiss();
        assert_eq!(engine.state, SuggestionState::Idle);
        assert!(engine.ghost_text().is_none());
        assert_eq!(engine.total_dismissed, 1);
    }

    #[test]
    fn low_confidence_filtered() {
        let mut engine = InlineSuggestionEngine::default();
        engine.config.min_confidence = 0.5;
        engine.shared.set(InlineSuggestion {
            text: "weak".into(),
            line: 1,
            column: 0,
            confidence: 0.2,
            generated_at: Instant::now(),
            source: "test".into(),
        });
        engine.poll();
        assert_eq!(engine.state, SuggestionState::Idle);
        assert_eq!(engine.total_shown, 0);
    }

    #[test]
    fn build_context_splits_correctly() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let ctx = build_completion_context(content, 3, 2, 2, 2);
        assert!(ctx.prefix.contains("line1"));
        assert!(ctx.prefix.contains("line3"));
        assert!(ctx.suffix.contains("line4"));
        assert!(ctx.suffix.contains("line5"));
    }

    #[test]
    fn acceptance_rate_calculation() {
        let mut engine = InlineSuggestionEngine::default();
        engine.total_shown = 10;
        engine.total_accepted = 7;
        let rate = engine.acceptance_rate();
        assert!((rate - 70.0).abs() < 0.01);
    }

    #[test]
    fn sanitize_strips_code_fence() {
        let raw = "Here you go:\n```rust\nlet x = 1;\n```\n";
        let out = sanitize_completion(raw, 200, true);
        assert_eq!(out, "let x = 1;");
    }

    #[test]
    fn sanitize_single_line_policy() {
        let raw = "foo()\nbar()";
        assert_eq!(sanitize_completion(raw, 200, false), "foo()");
        assert_eq!(sanitize_completion(raw, 200, true), "foo()\nbar()");
    }

    #[test]
    fn sanitize_truncates_and_handles_empty() {
        assert_eq!(sanitize_completion("   ", 200, true), "");
        assert_eq!(sanitize_completion("abcdef", 3, true), "abc");
    }

    fn req(prefix: &str) -> SuggestionRequest {
        SuggestionRequest {
            file_path: PathBuf::from("src/main.rs"),
            prefix: prefix.to_string(),
            suffix: String::new(),
            language: "rust".to_string(),
        }
    }

    #[test]
    fn cache_round_trip_and_miss() {
        let mut engine = InlineSuggestionEngine::default();
        let r = req("fn main() {");
        assert!(engine.cached_completion(&r).is_none());
        engine.cache_completion(&r, "    println!(\"hi\");\n}");
        assert_eq!(
            engine.cached_completion(&r).as_deref(),
            Some("    println!(\"hi\");\n}")
        );
        // A different context is a miss.
        assert!(engine.cached_completion(&req("fn other() {")).is_none());
    }

    #[test]
    fn submit_request_serves_cache_hit_without_model() {
        let mut engine = InlineSuggestionEngine::default();
        let r = req("let x = ");
        engine.cache_completion(&r, "42;");
        engine.submit_request(
            r,
            crate::agent::AiProvider::OpenRouter,
            "test-model",
            PathBuf::from("."),
        );
        // The cached suggestion is published synchronously to the shared slot.
        let pending = engine.shared.peek().expect("cached suggestion published");
        assert_eq!(pending.text, "42;");
        assert!(pending.source.contains("(cache)"));
    }

    #[test]
    fn poll_stores_successful_result_in_cache() {
        let mut engine = InlineSuggestionEngine::default();
        let r = req("struct Point {");
        engine.submit_request(
            r.clone(),
            crate::agent::AiProvider::OpenRouter,
            "test-model",
            PathBuf::from("."),
        );
        // Simulate the worker delivering a result.
        engine.shared.set(InlineSuggestion {
            text: "    x: f64,\n}".into(),
            line: 1,
            column: 0,
            confidence: 0.75,
            generated_at: Instant::now(),
            source: "test-model".into(),
        });
        engine.poll();
        assert_eq!(engine.state, SuggestionState::Showing);
        assert_eq!(
            engine.cached_completion(&r).as_deref(),
            Some("    x: f64,\n}")
        );
    }

    #[test]
    fn accept_and_dismiss_record_telemetry() {
        let mut engine = InlineSuggestionEngine::default();
        engine.shared.set(InlineSuggestion {
            text: "alpha".into(),
            line: 1,
            column: 0,
            confidence: 0.9,
            generated_at: Instant::now(),
            source: "test".into(),
        });
        engine.poll();
        assert_eq!(engine.accept().as_deref(), Some("alpha"));

        engine.shared.set(InlineSuggestion {
            text: "beta".into(),
            line: 1,
            column: 0,
            confidence: 0.9,
            generated_at: Instant::now(),
            source: "test".into(),
        });
        engine.poll();
        engine.dismiss();

        assert_eq!(engine.history.entries.len(), 2);
        assert!(engine.history.entries[0].was_accepted);
        assert!(!engine.history.entries[1].was_accepted);
        assert!((engine.history.acceptance_rate() - 50.0).abs() < 0.01);
    }
}
