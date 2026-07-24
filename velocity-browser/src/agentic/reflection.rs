//! Reflection Engine: detects failure patterns and generates corrective
//! system messages to inject before the next LLM reasoning turn.
//!
//! Instead of blindly retrying the same action, the reflection engine
//! analyzes recent outcomes, identifies patterns, and produces a concise
//! "lesson" the agent can use to adjust strategy.

use super::outcome_scorer::{ActionKind, ActionOutcome, OutcomeScorer};
use std::collections::HashMap;

/// A reflection insight derived from failure analysis.
#[derive(Debug, Clone)]
pub struct Reflection {
    pub category: ReflectionCategory,
    pub message: String,
    pub confidence: f64,
    pub suggested_strategy: Option<String>,
}

/// Categories of failure patterns the engine can detect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionCategory {
    /// Same action repeated multiple times without success
    RepeatedFailure,
    /// Action target no longer exists in DOM
    StaleTarget,
    /// Interstitial/overlay blocking interaction
    BlockingOverlay,
    /// Wrong element type for intended action
    ActionMismatch,
    /// Navigation loop (visiting same URL repeatedly)
    NavigationLoop,
    /// Timeout pattern (actions consistently timing out)
    TimeoutPattern,
    /// Provider-specific failure (model can't handle this task)
    ProviderLimitation,
    /// Generic unclassified failure
    Generic,
}

/// Configuration for the reflection engine.
#[derive(Debug, Clone)]
pub struct ReflectionConfig {
    /// How many recent outcomes to analyze
    pub lookback_window: usize,
    /// Minimum failures before triggering reflection
    pub min_failures_threshold: u32,
    /// Score below which an outcome counts as a "failure"
    pub failure_score_threshold: f64,
    /// Maximum reflections to produce per turn
    pub max_reflections_per_turn: usize,
}

impl Default for ReflectionConfig {
    fn default() -> Self {
        Self {
            lookback_window: 10,
            min_failures_threshold: 2,
            failure_score_threshold: 0.3,
            max_reflections_per_turn: 3,
        }
    }
}

pub struct ReflectionEngine {
    pub config: ReflectionConfig,
    /// Track how many times we've reflected on the same pattern (to avoid spam)
    pub reflection_counts: HashMap<String, u32>,
}

impl Default for ReflectionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ReflectionEngine {
    pub fn new() -> Self {
        Self {
            config: ReflectionConfig::default(),
            reflection_counts: HashMap::new(),
        }
    }

    pub fn with_config(config: ReflectionConfig) -> Self {
        Self {
            config,
            reflection_counts: HashMap::new(),
        }
    }

    /// Analyze recent outcomes and produce reflections if patterns are detected.
    pub fn reflect(&mut self, scorer: &OutcomeScorer) -> Vec<Reflection> {
        let recent = scorer.recent_context(self.config.lookback_window);
        if recent.is_empty() {
            return Vec::new();
        }

        let mut reflections = Vec::new();

        // Check for repeated failures on the same target
        if let Some(r) = self.detect_repeated_failure(&recent) {
            reflections.push(r);
        }

        // Check for navigation loops
        if let Some(r) = self.detect_navigation_loop(&recent) {
            reflections.push(r);
        }

        // Check for timeout patterns
        if let Some(r) = self.detect_timeout_pattern(&recent) {
            reflections.push(r);
        }

        // Check for blocking overlay pattern
        if let Some(r) = self.detect_blocking_overlay(&recent) {
            reflections.push(r);
        }

        reflections.truncate(self.config.max_reflections_per_turn);
        reflections
    }

    /// Format reflections into a system message for LLM injection.
    pub fn format_as_system_message(&self, reflections: &[Reflection]) -> Option<String> {
        if reflections.is_empty() {
            return None;
        }

        let mut msg = String::from(
            "[SELF-REFLECTION] Based on recent action outcomes, I've identified these patterns:\n\n"
        );

        for (i, r) in reflections.iter().enumerate() {
            msg.push_str(&format!("{}. {}\n", i + 1, r.message));
            if let Some(ref strategy) = r.suggested_strategy {
                msg.push_str(&format!("   → Strategy: {}\n", strategy));
            }
        }

        msg.push_str("\nI should adjust my approach based on these observations.");
        Some(msg)
    }

    fn detect_repeated_failure(&mut self, recent: &[&ActionOutcome]) -> Option<Reflection> {
        // Count failures per (target_role, action_kind)
        let mut failure_counts: HashMap<(&str, &str), u32> = HashMap::new();

        for outcome in recent {
            if outcome.score < self.config.failure_score_threshold {
                let key = (outcome.target_role.as_str(), outcome.action_kind.label());
                *failure_counts.entry(key).or_insert(0) += 1;
            }
        }

        // Find the worst offender
        let worst = failure_counts.iter()
            .max_by_key(|(_, count)| *count);

        if let Some(((role, action), count)) = worst {
            if *count >= self.config.min_failures_threshold {
                let pattern_key = format!("repeated::{}::{}", role, action);
                let seen = self.reflection_counts.entry(pattern_key).or_insert(0);
                *seen += 1;

                return Some(Reflection {
                    category: ReflectionCategory::RepeatedFailure,
                    message: format!(
                        "Action '{}' on '{}' elements has failed {} times recently. \
                         This target type is unreliable on this page.",
                        action, role, count
                    ),
                    confidence: 0.85,
                    suggested_strategy: Some(format!(
                        "Try a different approach: look for alternative {} elements, \
                         or use a different action type entirely.",
                        role
                    )),
                });
            }
        }

        None
    }

    fn detect_navigation_loop(&mut self, recent: &[&ActionOutcome]) -> Option<Reflection> {
        // Check if the same URL appears 3+ times in recent navigate actions
        let nav_urls: Vec<&str> = recent.iter()
            .filter(|o| o.action_kind == ActionKind::Navigate)
            .map(|o| o.page_url.as_str())
            .collect();

        let mut url_counts: HashMap<&str, u32> = HashMap::new();
        for url in &nav_urls {
            *url_counts.entry(url).or_insert(0) += 1;
        }

        if let Some((url, count)) = url_counts.iter().max_by_key(|(_, c)| *c) {
            if *count >= 3 {
                return Some(Reflection {
                    category: ReflectionCategory::NavigationLoop,
                    message: format!(
                        "Navigation loop detected: visited '{}' {} times. \
                         The page may be redirecting back to itself.",
                        url, count
                    ),
                    confidence: 0.90,
                    suggested_strategy: Some(
                        "Break the loop: try a completely different URL, \
                         clear cookies, or use a direct deep-link instead."
                            .to_string(),
                    ),
                });
            }
        }

        None
    }

    fn detect_timeout_pattern(&self, recent: &[&ActionOutcome]) -> Option<Reflection> {
        let timeout_count = recent.iter()
            .filter(|o| !o.signals.completed_in_time && o.score < self.config.failure_score_threshold)
            .count();

        if timeout_count >= self.config.min_failures_threshold as usize {
            return Some(Reflection {
                category: ReflectionCategory::TimeoutPattern,
                message: format!(
                    "{} of the last {} actions timed out. \
                     The page may be unresponsive or loading heavy resources.",
                    timeout_count, recent.len()
                ),
                confidence: 0.80,
                suggested_strategy: Some(
                    "Wait for the page to fully load before acting, \
                     or target simpler elements that don't trigger heavy JS."
                        .to_string(),
                ),
            });
        }

        None
    }

    fn detect_blocking_overlay(&self, recent: &[&ActionOutcome]) -> Option<Reflection> {
        // Pattern: multiple click failures where DOM didn't change
        // (suggests something is intercepting clicks — e.g., cookie banner)
        let blocked_clicks = recent.iter()
            .filter(|o| {
                o.action_kind == ActionKind::Click
                    && o.score < self.config.failure_score_threshold
                    && !o.signals.dom_changed
                    && o.signals.completed_in_time
            })
            .count();

        if blocked_clicks >= self.config.min_failures_threshold as usize {
            return Some(Reflection {
                category: ReflectionCategory::BlockingOverlay,
                message: format!(
                    "{} click actions completed but had no effect on the DOM. \
                     A modal, overlay, or cookie banner may be blocking interaction.",
                    blocked_clicks
                ),
                confidence: 0.75,
                suggested_strategy: Some(
                    "Look for and dismiss any overlays, modals, or cookie consent banners \
                     before attempting the target action."
                        .to_string(),
                ),
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::outcome_scorer::OutcomeSignals;

    fn make_outcome(action: ActionKind, role: &str, score: f64, signals: OutcomeSignals) -> ActionOutcome {
        ActionOutcome {
            action_kind: action,
            target_selector: "node_1".to_string(),
            target_role: role.to_string(),
            page_url: "https://example.com".to_string(),
            score,
            signals,
            timestamp_ms: 0,
        }
    }

    #[test]
    fn detects_repeated_failure() {
        let mut scorer = OutcomeScorer::new();
        for _ in 0..4 {
            scorer.record(make_outcome(
                ActionKind::Click,
                "button",
                0.1,
                OutcomeSignals { error_thrown: true, ..Default::default() },
            ));
        }

        let mut engine = ReflectionEngine::new();
        let reflections = engine.reflect(&scorer);
        assert!(!reflections.is_empty());
        assert_eq!(reflections[0].category, ReflectionCategory::RepeatedFailure);
    }

    #[test]
    fn detects_navigation_loop() {
        let mut scorer = OutcomeScorer::new();
        for _ in 0..4 {
            scorer.record(ActionOutcome {
                action_kind: ActionKind::Navigate,
                target_selector: String::new(),
                target_role: "link".to_string(),
                page_url: "https://example.com/login".to_string(),
                score: 0.5,
                signals: OutcomeSignals { url_changed: true, completed_in_time: true, ..Default::default() },
                timestamp_ms: 0,
            });
        }

        let mut engine = ReflectionEngine::new();
        let reflections = engine.reflect(&scorer);
        let nav_loop = reflections.iter().find(|r| r.category == ReflectionCategory::NavigationLoop);
        assert!(nav_loop.is_some());
    }

    #[test]
    fn detects_blocking_overlay() {
        let mut scorer = OutcomeScorer::new();
        for _ in 0..3 {
            scorer.record(make_outcome(
                ActionKind::Click,
                "button",
                0.1,
                OutcomeSignals { completed_in_time: true, ..Default::default() },
            ));
        }

        let mut engine = ReflectionEngine::new();
        let reflections = engine.reflect(&scorer);
        let overlay = reflections.iter().find(|r| r.category == ReflectionCategory::BlockingOverlay);
        assert!(overlay.is_some());
    }

    #[test]
    fn no_reflection_when_all_succeeding() {
        let mut scorer = OutcomeScorer::new();
        for _ in 0..5 {
            scorer.record(make_outcome(
                ActionKind::Click,
                "button",
                0.9,
                OutcomeSignals { dom_changed: true, completed_in_time: true, ..Default::default() },
            ));
        }

        let mut engine = ReflectionEngine::new();
        let reflections = engine.reflect(&scorer);
        assert!(reflections.is_empty());
    }

    #[test]
    fn format_as_system_message() {
        let engine = ReflectionEngine::new();
        let reflections = vec![Reflection {
            category: ReflectionCategory::RepeatedFailure,
            message: "Click on button failed 3 times.".to_string(),
            confidence: 0.85,
            suggested_strategy: Some("Try alternative elements.".to_string()),
        }];
        let msg = engine.format_as_system_message(&reflections);
        assert!(msg.is_some());
        let text = msg.unwrap();
        assert!(text.contains("SELF-REFLECTION"));
        assert!(text.contains("Click on button failed"));
        assert!(text.contains("Try alternative elements"));
    }
}
