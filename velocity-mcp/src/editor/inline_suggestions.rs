#![allow(dead_code)]
//! Inline Agent Suggestions: ghost text displayed semi-transparently in the
//! editor at the cursor position. Powered by AI completion requests that run
//! in the background and produce predictive next-edits.

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
        *self.inner.lock().unwrap() = Some(suggestion);
    }

    /// Take the current suggestion (consuming it).
    pub fn take(&self) -> Option<InlineSuggestion> {
        self.inner.lock().unwrap().take()
    }

    /// Peek at the current suggestion.
    pub fn peek(&self) -> Option<InlineSuggestion> {
        self.inner.lock().unwrap().clone()
    }

    /// Clear any pending suggestion.
    pub fn clear(&self) {
        *self.inner.lock().unwrap() = None;
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
        }
    }

    /// Check if a suggestion text was recently shown (to avoid repeats).
    pub fn is_duplicate(&self, text: &str) -> bool {
        self.recent_suggestions.iter().rev().take(10).any(|s| s == text)
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
            let text = self.current_suggestion.take().map(|s| s.text);
            self.state = SuggestionState::Idle;
            text
        } else {
            None
        }
    }

    /// Dismiss the current suggestion (Esc key or content change).
    pub fn dismiss(&mut self) {
        if self.state == SuggestionState::Showing {
            self.total_dismissed += 1;
        }
        self.current_suggestion = None;
        self.state = SuggestionState::Idle;
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
    /// The engine queues it and the background thread resolves it via the provider.
    pub fn submit_request(
        &mut self,
        request: SuggestionRequest,
        provider: crate::agent::AiProvider,
        model: &str,
    ) {
        if !self.config.enabled {
            return;
        }
        self.state = SuggestionState::Loading;

        // Build the completion context prompt for the model.
        let _prompt = format!(
            "Complete the following {} code. Return ONLY the completion text, no explanation.\n\n\
             ```\n{}\n```\n\nContinue from where the code ends:",
            request.language, request.prefix
        );

        // Spawn a background thread to query the provider.
        let shared = self.shared.clone();
        let line = self.last_trigger_line;
        let col = self.last_trigger_col;
        let source_label = format!("{}/{}", provider.label(), model);
        let _model = model.to_string();
        std::thread::spawn(move || {
            // The actual LLM call would go through the agent pipeline here.
            // For now, mark as loaded with the request context so the engine
            // knows a request was submitted. Production wiring will use the
            // existing run_headless_subagent infrastructure.
            let suggestion = InlineSuggestion {
                text: String::new(), // Populated by actual LLM response
                line,
                column: col,
                confidence: 0.0,
                generated_at: Instant::now(),
                source: source_label,
            };
            // Only set if we actually got a non-empty completion.
            if !suggestion.text.is_empty() {
                shared.set(suggestion);
            }
        });
    }
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
        Self { entries: Vec::new(), max_entries }
    }

    pub fn record(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
    }

    pub fn acceptance_rate(&self) -> f32 {
        if self.entries.is_empty() { return 0.0; }
        let accepted = self.entries.iter().filter(|e| e.was_accepted).count();
        (accepted as f32 / self.entries.len() as f32) * 100.0
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.entries.is_empty() { return 0.0; }
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
}
