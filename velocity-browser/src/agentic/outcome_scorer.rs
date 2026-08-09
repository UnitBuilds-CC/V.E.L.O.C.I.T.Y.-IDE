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
        if signals.dom_changed {
            score += weights.dom_changed;
        }
        if signals.url_changed {
            score += weights.url_changed;
        }
        if signals.error_thrown {
            score += weights.error_penalty;
        } // negative
        if signals.target_removed {
            score += weights.target_removed;
        }
        if signals.content_added {
            score += weights.content_added;
        }
        if signals.network_request_fired {
            score += weights.network_fired;
        }
        if signals.completed_in_time {
            score += weights.completed_in_time;
        }

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
            if *count > 0 {
                *total / *count as f64
            } else {
                0.0
            }
        })
    }

    /// Get the top N most reliable action targets on a given domain.
    pub fn top_targets(&self, domain: &str, limit: usize) -> Vec<(&str, f64)> {
        let mut targets: Vec<_> = self
            .success_rates
            .iter()
            .filter(|(k, _)| k.starts_with(domain))
            .map(|(k, (total, count))| {
                let rate = if *count > 0 {
                    *total / *count as f64
                } else {
                    0.0
                };
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
                if o.signals.error_thrown {
                    " [ERROR]"
                } else {
                    ""
                }
            ));
        }
        out
    }

    /// Export the full outcome history as a lossless NDA document so
    /// experience survives across sessions. Aggregated success rates are
    /// derived state and rebuilt on import via [`Self::record`].
    pub fn export_nda(&self) -> crate::nda::NdaDocument {
        use crate::predicates::*;
        let mut doc = crate::nda::NdaDocument::new();
        for (idx, o) in self.history.iter().enumerate() {
            let subject = format!("o{idx}");
            doc.push_str(&subject, OUTCOME_ACTION, o.action_kind.label());
            doc.push_str(&subject, OUTCOME_ROLE, &o.target_role);
            doc.push_str(&subject, OUTCOME_SELECTOR, &o.target_selector);
            doc.push_str(&subject, OUTCOME_URL, &o.page_url);
            doc.push_int(&subject, OUTCOME_SCORE, (o.score * 10_000.0).round() as i64);
            doc.push_int(&subject, OUTCOME_SIGNALS, signals_to_bits(&o.signals));
            doc.push_int(
                &subject,
                OUTCOME_CONFIDENCE,
                (o.signals.agent_confidence * 10_000.0).round() as i64,
            );
            doc.push_int(&subject, OUTCOME_TIMESTAMP, o.timestamp_ms as i64);
        }
        doc
    }

    /// Restore outcomes from a document produced by [`Self::export_nda`],
    /// re-recording each one so success rates and the ring buffer rebuild.
    /// Outcomes identical to an already-stored entry (same timestamp,
    /// action, selector and url) are skipped, making repeated loads
    /// idempotent. Returns the number of outcomes restored.
    pub fn import_nda(&mut self, doc: &crate::nda::NdaDocument) -> usize {
        use crate::nda::NdaObject;
        use crate::predicates::*;

        #[derive(Default)]
        struct Partial {
            action: Option<String>,
            role: String,
            selector: String,
            url: Option<String>,
            score: f64,
            signal_bits: i64,
            confidence: f64,
            timestamp_ms: u64,
        }

        // Group facts per subject, preserving first-seen (export) order.
        let mut order: Vec<String> = Vec::new();
        let mut partial: HashMap<String, Partial> = HashMap::new();
        for fact in &doc.facts {
            let Some(subject) = doc.subject_str(fact) else {
                continue;
            };
            if !partial.contains_key(subject) {
                order.push(subject.to_string());
            }
            let slot = partial.entry(subject.to_string()).or_default();
            match (fact.predicate, &fact.object) {
                (OUTCOME_ACTION, NdaObject::Str(id)) => {
                    slot.action = doc.dict.resolve(*id).map(str::to_string);
                }
                (OUTCOME_ROLE, NdaObject::Str(id)) => {
                    if let Some(role) = doc.dict.resolve(*id) {
                        slot.role = role.to_string();
                    }
                }
                (OUTCOME_SELECTOR, NdaObject::Str(id)) => {
                    if let Some(sel) = doc.dict.resolve(*id) {
                        slot.selector = sel.to_string();
                    }
                }
                (OUTCOME_URL, NdaObject::Str(id)) => {
                    slot.url = doc.dict.resolve(*id).map(str::to_string);
                }
                (OUTCOME_SCORE, NdaObject::Int(n)) => slot.score = *n as f64 / 10_000.0,
                (OUTCOME_SIGNALS, NdaObject::Int(n)) => slot.signal_bits = *n,
                (OUTCOME_CONFIDENCE, NdaObject::Int(n)) => {
                    slot.confidence = *n as f64 / 10_000.0;
                }
                (OUTCOME_TIMESTAMP, NdaObject::Int(n)) => slot.timestamp_ms = *n as u64,
                _ => {}
            }
        }

        let mut restored = 0usize;
        // Count identical outcomes already stored BEFORE this import so that
        // duplicates inside one artifact (two identical failures in the same
        // millisecond are one legitimate event each) all restore, while
        // reloading the same artifact stays idempotent.
        let mut already: HashMap<(u64, String, String, String), usize> = HashMap::new();
        for o in &self.history {
            *already
                .entry((
                    o.timestamp_ms,
                    o.action_kind.label().to_string(),
                    o.target_selector.clone(),
                    o.page_url.clone(),
                ))
                .or_insert(0) += 1;
        }
        for subject in order {
            let Some(p) = partial.remove(&subject) else {
                continue;
            };
            let (Some(action), Some(url)) = (p.action, p.url) else {
                continue;
            };
            let key = (
                p.timestamp_ms,
                action.clone(),
                p.selector.clone(),
                url.clone(),
            );
            if let Some(count) = already.get_mut(&key) {
                if *count > 0 {
                    *count -= 1;
                    continue;
                }
            }
            self.record(ActionOutcome {
                action_kind: ActionKind::from_str(&action),
                target_selector: p.selector,
                target_role: p.role,
                page_url: url,
                score: p.score,
                signals: signals_from_bits(p.signal_bits, p.confidence),
                timestamp_ms: p.timestamp_ms,
            });
            restored += 1;
        }
        restored
    }
}

/// Pack the boolean outcome signals into a compact bitmask for NDA storage.
fn signals_to_bits(s: &OutcomeSignals) -> i64 {
    (s.dom_changed as i64)
        | (s.url_changed as i64) << 1
        | (s.error_thrown as i64) << 2
        | (s.target_removed as i64) << 3
        | (s.content_added as i64) << 4
        | (s.network_request_fired as i64) << 5
        | (s.completed_in_time as i64) << 6
}

/// Rebuild outcome signals from the bitmask produced by [`signals_to_bits`].
fn signals_from_bits(bits: i64, agent_confidence: f64) -> OutcomeSignals {
    OutcomeSignals {
        dom_changed: bits & 1 != 0,
        url_changed: bits & (1 << 1) != 0,
        error_thrown: bits & (1 << 2) != 0,
        target_removed: bits & (1 << 3) != 0,
        content_added: bits & (1 << 4) != 0,
        network_request_fired: bits & (1 << 5) != 0,
        completed_in_time: bits & (1 << 6) != 0,
        agent_confidence,
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
        assert!(
            score > 0.5,
            "Successful click should score > 0.5, got {}",
            score
        );
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
        assert!(
            score < 0.1,
            "Failed action should score near 0, got {}",
            score
        );
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
            signals: OutcomeSignals {
                completed_in_time: true,
                dom_changed: true,
                ..Default::default()
            },
            timestamp_ms: 5000,
        });
        let ctx = scorer.format_for_context(5);
        assert!(ctx.contains("fill"));
        assert!(ctx.contains("textbox"));
        assert!(ctx.contains("0.95"));
    }

    #[test]
    fn extract_domain_works() {
        assert_eq!(
            extract_domain("https://www.example.com/path"),
            "www.example.com"
        );
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

    #[test]
    fn export_import_round_trips_outcome_history() {
        let mut scorer = OutcomeScorer::new();
        scorer.record(ActionOutcome {
            action_kind: ActionKind::Click,
            target_selector: "node_3".to_string(),
            target_role: "button".to_string(),
            page_url: "https://example.com/checkout".to_string(),
            score: 0.85,
            signals: OutcomeSignals {
                dom_changed: true,
                completed_in_time: true,
                agent_confidence: 0.75,
                ..Default::default()
            },
            timestamp_ms: 1000,
        });
        scorer.record(ActionOutcome {
            action_kind: ActionKind::Fill,
            target_selector: "node_9".to_string(),
            target_role: "textbox".to_string(),
            page_url: "https://example.com/checkout".to_string(),
            score: 0.1,
            signals: OutcomeSignals {
                error_thrown: true,
                ..Default::default()
            },
            timestamp_ms: 2000,
        });

        let bytes = scorer.export_nda().to_binary_stream();
        let doc = crate::nda::NdaDocument::from_binary_stream(&bytes).expect("decode");

        let mut fresh = OutcomeScorer::new();
        assert_eq!(fresh.import_nda(&doc), 2);
        assert_eq!(fresh.history.len(), 2);

        // Raw fields survive the round trip.
        let first = &fresh.history[0];
        assert_eq!(first.action_kind, ActionKind::Click);
        assert_eq!(first.target_selector, "node_3");
        assert!((first.score - 0.85).abs() < 0.001);
        assert!(first.signals.dom_changed);
        assert!(first.signals.completed_in_time);
        assert!(!first.signals.error_thrown);
        assert!((first.signals.agent_confidence - 0.75).abs() < 0.001);
        assert_eq!(first.timestamp_ms, 1000);
        assert!(fresh.history[1].signals.error_thrown);

        // Aggregates are rebuilt by re-recording.
        let rate = fresh.success_rate("example.com", "button", &ActionKind::Click);
        assert_eq!(rate, Some(0.85));

        // Repeated loads are idempotent.
        assert_eq!(fresh.import_nda(&doc), 0);
        assert_eq!(fresh.history.len(), 2);
    }

    #[test]
    fn import_skips_incomplete_outcomes() {
        let mut doc = crate::nda::NdaDocument::new();
        // Missing url — must be skipped, not halfrestored.
        doc.push_str("o0", crate::predicates::OUTCOME_ACTION, "click");
        let mut scorer = OutcomeScorer::new();
        assert_eq!(scorer.import_nda(&doc), 0);
        assert!(scorer.history.is_empty());
    }

    // ── ActionKind::from_str ──────────────────────────────────────────

    #[test]
    fn action_kind_from_str_all_variants() {
        assert_eq!(ActionKind::from_str("click"), ActionKind::Click);
        assert_eq!(ActionKind::from_str("fill"), ActionKind::Fill);
        assert_eq!(ActionKind::from_str("type"), ActionKind::Fill);
        assert_eq!(ActionKind::from_str("input"), ActionKind::Fill);
        assert_eq!(ActionKind::from_str("navigate"), ActionKind::Navigate);
        assert_eq!(ActionKind::from_str("goto"), ActionKind::Navigate);
        assert_eq!(ActionKind::from_str("submit"), ActionKind::Submit);
        assert_eq!(ActionKind::from_str("scroll"), ActionKind::Scroll);
        assert_eq!(ActionKind::from_str("select"), ActionKind::Select);
        assert_eq!(ActionKind::from_str("extract"), ActionKind::Extract);
        assert_eq!(ActionKind::from_str("read"), ActionKind::Extract);
    }

    #[test]
    fn action_kind_from_str_case_insensitive() {
        assert_eq!(ActionKind::from_str("CLICK"), ActionKind::Click);
        assert_eq!(ActionKind::from_str("Fill"), ActionKind::Fill);
        assert_eq!(ActionKind::from_str("NAVIGATE"), ActionKind::Navigate);
        assert_eq!(ActionKind::from_str("Submit"), ActionKind::Submit);
    }

    #[test]
    fn action_kind_from_str_unknown_becomes_custom() {
        let kind = ActionKind::from_str("hover");
        assert_eq!(kind, ActionKind::Custom("hover".to_string()));
    }

    // ── ActionKind::label ─────────────────────────────────────────────

    #[test]
    fn action_kind_label_roundtrip() {
        let cases = [
            ("click", "click"),
            ("fill", "fill"),
            ("navigate", "navigate"),
            ("submit", "submit"),
            ("scroll", "scroll"),
            ("select", "select"),
            ("extract", "extract"),
        ];
        for (input, expected_label) in cases {
            assert_eq!(ActionKind::from_str(input).label(), expected_label);
        }
    }

    #[test]
    fn action_kind_custom_label() {
        let kind = ActionKind::Custom("drag".to_string());
        assert_eq!(kind.label(), "drag");
    }

    // ── OutcomeSignals::default ───────────────────────────────────────

    #[test]
    fn outcome_signals_default_all_false() {
        let s = OutcomeSignals::default();
        assert!(!s.dom_changed);
        assert!(!s.url_changed);
        assert!(!s.error_thrown);
        assert!(!s.target_removed);
        assert!(!s.content_added);
        assert!(!s.network_request_fired);
        assert!(!s.completed_in_time);
        assert_eq!(s.agent_confidence, 0.0);
    }

    // ── signals_to_bits / signals_from_bits ───────────────────────────

    #[test]
    fn signals_bits_roundtrip_all_false() {
        let s = OutcomeSignals::default();
        let bits = signals_to_bits(&s);
        assert_eq!(bits, 0);
        let back = signals_from_bits(bits, 0.0);
        assert!(!back.dom_changed && !back.url_changed && !back.error_thrown);
    }

    #[test]
    fn signals_bits_roundtrip_all_true() {
        let s = OutcomeSignals {
            dom_changed: true,
            url_changed: true,
            error_thrown: true,
            target_removed: true,
            content_added: true,
            network_request_fired: true,
            completed_in_time: true,
            agent_confidence: 0.0,
        };
        let bits = signals_to_bits(&s);
        assert_eq!(bits, 0b1111111);
        let back = signals_from_bits(bits, 0.0);
        assert!(back.dom_changed && back.url_changed && back.error_thrown);
        assert!(back.target_removed && back.content_added);
        assert!(back.network_request_fired && back.completed_in_time);
    }

    #[test]
    fn signals_bits_individual_flags() {
        for (i, field) in ["dom", "url", "err", "tgt", "cnt", "net", "done"]
            .iter()
            .enumerate()
        {
            let mut s = OutcomeSignals::default();
            match i {
                0 => s.dom_changed = true,
                1 => s.url_changed = true,
                2 => s.error_thrown = true,
                3 => s.target_removed = true,
                4 => s.content_added = true,
                5 => s.network_request_fired = true,
                6 => s.completed_in_time = true,
                _ => unreachable!(),
            }
            let bits = signals_to_bits(&s);
            assert_eq!(bits, 1 << i, "flag {} should be bit {}", field, i);
        }
    }

    #[test]
    fn signals_from_bits_preserves_confidence() {
        let back = signals_from_bits(0, 0.85);
        assert!((back.agent_confidence - 0.85).abs() < 1e-9);
    }

    // ── extract_domain ────────────────────────────────────────────────

    #[test]
    fn extract_domain_https_with_port() {
        assert_eq!(
            extract_domain("https://example.com:443/path"),
            "example.com"
        );
    }

    #[test]
    fn extract_domain_no_scheme() {
        assert_eq!(extract_domain("example.com/path"), "example.com/path");
    }

    #[test]
    fn extract_domain_data_url() {
        assert_eq!(
            extract_domain("data:text/html,<h1>Hi</h1>"),
            "data:text/html,<h1>Hi</h1>"
        );
    }

    #[test]
    fn extract_domain_javascript_url() {
        assert_eq!(extract_domain("javascript:void(0)"), "javascript:void(0)");
    }

    #[test]
    fn extract_domain_http_root() {
        assert_eq!(extract_domain("http://localhost/"), "localhost");
    }

    // ── score() ───────────────────────────────────────────────────────

    #[test]
    fn score_fill_values_dom_change() {
        let scorer = OutcomeScorer::new();
        let s = OutcomeSignals {
            dom_changed: true,
            completed_in_time: true,
            ..Default::default()
        };
        let score = scorer.score(&ActionKind::Fill, &s);
        // Fill weights: dom_changed=0.40, completed_in_time=0.50
        assert!(
            score > 0.8,
            "Fill with dom+time should be high, got {}",
            score
        );
    }

    #[test]
    fn score_submit_values_network() {
        let scorer = OutcomeScorer::new();
        let s = OutcomeSignals {
            network_request_fired: true,
            url_changed: true,
            completed_in_time: true,
            ..Default::default()
        };
        let score = scorer.score(&ActionKind::Submit, &s);
        // Submit: url=0.25, network=0.30, time=0.10
        assert!(
            score > 0.5,
            "Submit with net+url should be high, got {}",
            score
        );
    }

    #[test]
    fn score_extract_only_cares_about_time() {
        let scorer = OutcomeScorer::new();
        let s_time = OutcomeSignals {
            completed_in_time: true,
            ..Default::default()
        };
        let s_no_time = OutcomeSignals {
            dom_changed: true,
            content_added: true,
            ..Default::default()
        };
        let with = scorer.score(&ActionKind::Extract, &s_time);
        let without = scorer.score(&ActionKind::Extract, &s_no_time);
        assert!(with > without, "Extract should value time over DOM changes");
    }

    #[test]
    fn score_agent_confidence_blends() {
        let scorer = OutcomeScorer::new();
        let s = OutcomeSignals {
            completed_in_time: true,
            agent_confidence: 0.9,
            ..Default::default()
        };
        let blended = scorer.score(&ActionKind::Click, &s);
        let no_blend = OutcomeSignals {
            completed_in_time: true,
            ..Default::default()
        };
        let raw = scorer.score(&ActionKind::Click, &no_blend);
        assert!(blended > raw, "Agent confidence should boost score");
    }

    #[test]
    fn score_clamped_to_zero_one() {
        let scorer = OutcomeScorer::new();
        let worst = OutcomeSignals {
            error_thrown: true,
            ..Default::default()
        };
        let s = scorer.score(&ActionKind::Navigate, &worst);
        assert!(
            (0.0..=1.0).contains(&s),
            "Score must be in [0,1], got {}",
            s
        );
    }

    // ── top_targets ───────────────────────────────────────────────────

    #[test]
    fn top_targets_returns_sorted_and_limited() {
        let mut scorer = OutcomeScorer::new();
        for i in 0..5 {
            scorer.record(ActionOutcome {
                action_kind: ActionKind::Click,
                target_selector: format!("node_{}", i),
                target_role: format!("role_{}", i),
                page_url: "https://example.com".to_string(),
                score: (i as f64) * 0.2,
                signals: OutcomeSignals::default(),
                timestamp_ms: i as u64,
            });
        }
        let top = scorer.top_targets("example.com", 2);
        assert_eq!(top.len(), 2);
        assert!(top[0].1 >= top[1].1, "Should be sorted descending");
    }

    #[test]
    fn top_targets_empty_store() {
        let scorer = OutcomeScorer::new();
        assert!(scorer.top_targets("example.com", 5).is_empty());
    }

    // ── recent_context ────────────────────────────────────────────────

    #[test]
    fn recent_context_empty() {
        let scorer = OutcomeScorer::new();
        assert!(scorer.recent_context(5).is_empty());
    }

    #[test]
    fn recent_context_returns_last_n() {
        let mut scorer = OutcomeScorer::new();
        for i in 0..10 {
            scorer.record(ActionOutcome {
                action_kind: ActionKind::Click,
                target_selector: format!("n_{}", i),
                target_role: "btn".to_string(),
                page_url: "https://x.com".to_string(),
                score: 0.5,
                signals: OutcomeSignals::default(),
                timestamp_ms: i,
            });
        }
        let ctx = scorer.recent_context(3);
        assert_eq!(ctx.len(), 3);
        assert_eq!(ctx[0].target_selector, "n_7");
    }

    // ── format_for_context ────────────────────────────────────────────

    #[test]
    fn format_for_context_empty() {
        let scorer = OutcomeScorer::new();
        assert!(scorer.format_for_context(5).is_empty());
    }

    #[test]
    fn format_for_context_shows_error_annotation() {
        let mut scorer = OutcomeScorer::new();
        scorer.record(ActionOutcome {
            action_kind: ActionKind::Click,
            target_selector: "node_1".to_string(),
            target_role: "button".to_string(),
            page_url: "https://x.com".to_string(),
            score: 0.1,
            signals: OutcomeSignals {
                error_thrown: true,
                ..Default::default()
            },
            timestamp_ms: 1,
        });
        let ctx = scorer.format_for_context(5);
        assert!(ctx.contains("[ERROR]"), "Should annotate error outcomes");
    }

    // ── success_rate ──────────────────────────────────────────────────

    #[test]
    fn success_rate_returns_none_for_unknown() {
        let scorer = OutcomeScorer::new();
        assert!(scorer
            .success_rate("x.com", "button", &ActionKind::Click)
            .is_none());
    }
}
