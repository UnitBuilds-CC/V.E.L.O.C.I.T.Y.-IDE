//! Outcome Scoring Engine: evaluates whether an agent action achieved its intent.
//!
//! Every action the agent takes produces an observable outcome. This module
//! scores that outcome on a 0.0..=1.0 scale and stores (state, action, score)
//! triples so the agent can learn from experience across sessions.

use std::collections::HashMap;

/// The type of action the agent attempted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ActionKind {
    Click,
    Fill,
    Navigate,
    Submit,
    Scroll,
    Select,
    Extract,
    Custom(String),
}

impl ActionKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "click" => Self::Click,
            "fill" | "type" | "input" => Self::Fill,
            "navigate" | "goto" => Self::Navigate,
            "submit" => Self::Submit,
            "scroll" => Self::Scroll,
            "select" => Self::Select,
            "extract" | "read" => Self::Extract,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Click => "click",
            Self::Fill => "fill",
            Self::Navigate => "navigate",
            Self::Submit => "submit",
            Self::Scroll => "scroll",
            Self::Select => "select",
            Self::Extract => "extract",
            Self::Custom(s) => s.as_str(),
        }
    }
}

/// Observable signals that determine if an action succeeded.
#[derive(Debug, Clone, Default)]
pub struct OutcomeSignals {
    /// Did the DOM change after the action?
    pub dom_changed: bool,
    /// Did navigation occur (URL changed)?
    pub url_changed: bool,
    /// Was an error/exception thrown?
    pub error_thrown: bool,
    /// Did the target element disappear (e.g., modal dismissed)?
    pub target_removed: bool,
    /// Did new content appear (node count increased)?
    pub content_added: bool,
    /// Was a network request triggered?
    pub network_request_fired: bool,
    /// Did the action complete without timeout?
    pub completed_in_time: bool,
    /// Custom signal from the agent (0.0..=1.0)
    pub agent_confidence: f64,
}

/// A scored outcome for a single action.
#[derive(Debug, Clone)]
pub struct ActionOutcome {
    pub action_kind: ActionKind,
    pub target_selector: String,
    pub target_role: String,
    pub page_url: String,
    pub score: f64,
    pub signals: OutcomeSignals,
    pub timestamp_ms: u64,
}

/// Scoring weights per action type — what signals matter most for each.
#[derive(Debug, Clone)]
struct ScoringWeights {
    dom_changed: f64,
    url_changed: f64,
    error_penalty: f64,
    target_removed: f64,
    content_added: f64,
    network_fired: f64,
    completed_in_time: f64,
}

impl ScoringWeights {
    fn for_action(kind: &ActionKind) -> Self {
        match kind {
            ActionKind::Click => Self {
                dom_changed: 0.25,
                url_changed: 0.15,
                error_penalty: -0.4,
                target_removed: 0.15,
                content_added: 0.15,
                network_fired: 0.10,
                completed_in_time: 0.20,
            },
            ActionKind::Navigate => Self {
                dom_changed: 0.10,
                url_changed: 0.45,
                error_penalty: -0.5,
                target_removed: 0.0,
                content_added: 0.20,
                network_fired: 0.10,
                completed_in_time: 0.15,
            },
            ActionKind::Fill => Self {
                dom_changed: 0.40,
                url_changed: 0.0,
                error_penalty: -0.3,
                target_removed: 0.0,
                content_added: 0.05,
                network_fired: 0.05,
                completed_in_time: 0.50,
            },
            ActionKind::Submit => Self {
                dom_changed: 0.15,
                url_changed: 0.25,
                error_penalty: -0.4,
                target_removed: 0.10,
                content_added: 0.10,
                network_fired: 0.30,
                completed_in_time: 0.10,
            },
            ActionKind::Extract => Self {
                dom_changed: 0.0,
                url_changed: 0.0,
                error_penalty: -0.5,
                target_removed: 0.0,
                content_added: 0.0,
                network_fired: 0.0,
                completed_in_time: 0.50,
            },
            _ => Self {
                dom_changed: 0.20,
                url_changed: 0.10,
                error_penalty: -0.3,
                target_removed: 0.10,
                content_added: 0.10,
                network_fired: 0.10,
                completed_in_time: 0.30,
            },
        }
    }
}

/// The outcome scorer: computes scores and maintains a history for learning.
pub struct OutcomeScorer {
    /// History of outcomes keyed by (page_domain, action_kind, target_role)
    pub history: Vec<ActionOutcome>,
    /// Aggregated success rates: key = "domain::role::action" → (total_score, count)
    pub success_rates: HashMap<String, (f64, u32)>,
    /// Maximum history entries to retain (ring buffer behavior)
    pub max_history: usize,
}

impl Default for OutcomeScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl OutcomeScorer {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            success_rates: HashMap::new(),
            max_history: 10_000,
        }
    }

    /// Score an action outcome based on observable signals.
    pub fn score(&self, action_kind: &ActionKind, signals: &OutcomeSignals) -> f64 {
        let weights = ScoringWeights::for_action(action_kind);

        let mut score = 0.0f64;
        if signals.dom_changed { score += weights.dom_changed; }
        if signals.url_changed { score += weights.url_changed; }
        if signals.error_thrown { score += weights.error_penalty; } // negative
        if signals.target_removed { score += weights.target_removed; }
        if signals.content_added { score += weights.content_added; }
        if signals.network_request_fired { score += weights.network_fired; }
        if signals.completed_in_time { score += weights.completed_in_time; }

        // Blend with agent's own confidence if provided
        if signals.agent_confidence > 0.0 {
            score = score * 0.7 + signals.agent_confidence * 0.3;
        }

        score.clamp(0.0, 1.0)
    }

    /// Record an outcome and update aggregated success rates.
    pub fn record(&mut self, outcome: ActionOutcome) {
        let key = format!(
            "{}::{}::{}",
            extract_domain(&outcome.page_url),
            outcome.target_role,
            outcome.action_kind.label()
        );

        let entry = self.success_rates.entry(key).or_insert((0.0, 0));
        entry.0 += outcome.score;
        entry.1 += 1;

        self.history.push(outcome);

        // Ring buffer: drop oldest when over capacity
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Get the historical success rate for a (domain, role, action) combination.
    pub fn success_rate(&self, domain: &str, role: &str, action: &ActionKind) -> Option<f64> {
        let key = format!("{}::{}::{}", domain, role, action.label());
        self.success_rates.get(&key).map(|(total, count)| {
            if *count > 0 { *total / *count as f64 } else { 0.0 }
        })
    }

    /// Get the top N most reliable action targets on a given domain.
    pub fn top_targets(&self, domain: &str, limit: usize) -> Vec<(&str, f64)> {
        let mut targets: Vec<_> = self.success_rates.iter()
            .filter(|(k, _)| k.starts_with(domain))
            .map(|(k, (total, count))| {
                let rate = if *count > 0 { *total / *count as f64 } else { 0.0 };
                (k.as_str(), rate)
            })
            .collect();
        targets.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        targets.truncate(limit);
        targets
    }

    /// Get the last N outcomes as context for the LLM.
    pub fn recent_context(&self, n: usize) -> Vec<&ActionOutcome> {
        let start = self.history.len().saturating_sub(n);
        self.history[start..].iter().collect()
    }

    /// Serialize recent outcomes as a compact string for LLM context injection.
    pub fn format_for_context(&self, n: usize) -> String {
        let recent = self.recent_context(n);
        if recent.is_empty() {
            return String::new();
        }
        let mut out = String::from("Recent action outcomes:\n");
        for o in recent {
            out.push_str(&format!(
                "  {} on [{}] ({}): score={:.2}{}\n",
                o.action_kind.label(),
                o.target_role,
                extract_domain(&o.page_url),
                o.score,
                if o.signals.error_thrown { " [ERROR]" } else { "" }
            ));
        }
        out
    }
}

/// Extract domain from a URL string.
pub fn extract_domain(url: &str) -> &str {
    // Handle non-http schemes like about:blank, data:, javascript:
    if !url.contains("://") {
        return url;
    }
    url.split("//")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .split(':')
        .next()
        .unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_successful_click() {
        let scorer = OutcomeScorer::new();
        let signals = OutcomeSignals {
            dom_changed: true,
            content_added: true,
            completed_in_time: true,
            ..Default::default()
        };
        let score = scorer.score(&ActionKind::Click, &signals);
        assert!(score > 0.5, "Successful click should score > 0.5, got {}", score);
    }

    #[test]
    fn score_failed_action_with_error() {
        let scorer = OutcomeScorer::new();
        let signals = OutcomeSignals {
            error_thrown: true,
            completed_in_time: false,
            ..Default::default()
        };
        let score = scorer.score(&ActionKind::Click, &signals);
        assert!(score < 0.1, "Failed action should score near 0, got {}", score);
    }

    #[test]
    fn score_navigation_needs_url_change() {
        let scorer = OutcomeScorer::new();
        let no_nav = OutcomeSignals {
            dom_changed: true,
            completed_in_time: true,
            ..Default::default()
        };
        let with_nav = OutcomeSignals {
            dom_changed: true,
            url_changed: true,
            content_added: true,
            completed_in_time: true,
            ..Default::default()
        };
        let s1 = scorer.score(&ActionKind::Navigate, &no_nav);
        let s2 = scorer.score(&ActionKind::Navigate, &with_nav);
        assert!(s2 > s1, "Navigation with URL change should score higher");
    }

    #[test]
    fn record_and_query_success_rate() {
        let mut scorer = OutcomeScorer::new();
        scorer.record(ActionOutcome {
            action_kind: ActionKind::Click,
            target_selector: "node_5".to_string(),
            target_role: "button".to_string(),
            page_url: "https://example.com/page".to_string(),
            score: 0.9,
            signals: OutcomeSignals::default(),
            timestamp_ms: 1000,
        });
        scorer.record(ActionOutcome {
            action_kind: ActionKind::Click,
            target_selector: "node_5".to_string(),
            target_role: "button".to_string(),
            page_url: "https://example.com/other".to_string(),
            score: 0.7,
            signals: OutcomeSignals::default(),
            timestamp_ms: 2000,
        });
        let rate = scorer.success_rate("example.com", "button", &ActionKind::Click);
        assert_eq!(rate, Some(0.8)); // (0.9 + 0.7) / 2
    }

    #[test]
    fn format_for_context_produces_readable_output() {
        let mut scorer = OutcomeScorer::new();
        scorer.record(ActionOutcome {
            action_kind: ActionKind::Fill,
            target_selector: "node_10".to_string(),
            target_role: "textbox".to_string(),
            page_url: "https://login.example.com/".to_string(),
            score: 0.95,
            signals: OutcomeSignals { completed_in_time: true, dom_changed: true, ..Default::default() },
            timestamp_ms: 5000,
        });
        let ctx = scorer.format_for_context(5);
        assert!(ctx.contains("fill"));
        assert!(ctx.contains("textbox"));
        assert!(ctx.contains("0.95"));
    }

    #[test]
    fn extract_domain_works() {
        assert_eq!(extract_domain("https://www.example.com/path"), "www.example.com");
        assert_eq!(extract_domain("http://localhost:8080/api"), "localhost");
        assert_eq!(extract_domain("about:blank"), "about:blank");
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let mut scorer = OutcomeScorer::new();
        scorer.max_history = 3;
        for i in 0..5 {
            scorer.record(ActionOutcome {
                action_kind: ActionKind::Click,
                target_selector: format!("node_{}", i),
                target_role: "button".to_string(),
                page_url: "https://example.com".to_string(),
                score: 0.5,
                signals: OutcomeSignals::default(),
                timestamp_ms: i as u64 * 1000,
            });
        }
        assert_eq!(scorer.history.len(), 3);
        assert_eq!(scorer.history[0].target_selector, "node_2");
    }
}
