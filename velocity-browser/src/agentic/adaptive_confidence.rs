//! Adaptive Action Confidence: per-element learned confidence scores that
//! replace the hardcoded 0.96 in action_predictor.rs.
//!
//! Uses exponential moving average of past outcome scores to predict how
//! likely an action on a given (role, page_pattern) combination is to succeed.

use std::collections::HashMap;

/// A confidence entry tracking historical success for a specific action target pattern.
#[derive(Debug, Clone)]
pub struct ConfidenceEntry {
    /// Exponential moving average of scores (0.0..=1.0)
    pub ema_score: f64,
    /// Total number of observations
    pub observations: u32,
    /// Smoothing factor for EMA (higher = more weight on recent)
    pub alpha: f64,
}

impl ConfidenceEntry {
    fn new(initial_score: f64) -> Self {
        Self {
            ema_score: initial_score,
            observations: 1,
            alpha: 0.3, // 30% weight on newest observation
        }
    }

    fn update(&mut self, new_score: f64) {
        self.ema_score = self.alpha * new_score + (1.0 - self.alpha) * self.ema_score;
        self.observations += 1;
    }
}

/// Key for confidence lookup: combines structural element role with page pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConfidenceKey {
    /// Element role (button, link, textbox, etc.)
    pub role: String,
    /// Action type (click, fill, etc.)
    pub action: String,
    /// Domain pattern (e.g., "example.com")
    pub domain: String,
}

impl ConfidenceKey {
    pub fn new(role: &str, action: &str, domain: &str) -> Self {
        Self {
            role: role.to_string(),
            action: action.to_string(),
            domain: domain.to_string(),
        }
    }

    /// A broader key that ignores domain (for cross-site generalization).
    pub fn generic(role: &str, action: &str) -> Self {
        Self {
            role: role.to_string(),
            action: action.to_string(),
            domain: "*".to_string(),
        }
    }
}

/// The adaptive confidence store.
pub struct AdaptiveConfidence {
    /// Site-specific confidence scores
    entries: HashMap<ConfidenceKey, ConfidenceEntry>,
    /// Cross-site generic confidence (fallback)
    generic_entries: HashMap<ConfidenceKey, ConfidenceEntry>,
    /// Default confidence for unseen combinations (replaces hardcoded 0.96)
    pub default_confidence: f64,
    /// Minimum observations before we trust the learned score
    pub min_observations: u32,
}

impl Default for AdaptiveConfidence {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveConfidence {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            generic_entries: HashMap::new(),
            default_confidence: 0.7, // Conservative default until we learn
            min_observations: 3,
        }
    }

    /// Record an outcome score for a specific (role, action, domain) combination.
    pub fn record(&mut self, role: &str, action: &str, domain: &str, score: f64) {
        // Update site-specific entry
        let key = ConfidenceKey::new(role, action, domain);
        self.entries.entry(key)
            .and_modify(|e| e.update(score))
            .or_insert_with(|| ConfidenceEntry::new(score));

        // Update generic cross-site entry
        let generic_key = ConfidenceKey::generic(role, action);
        self.generic_entries.entry(generic_key)
            .and_modify(|e| e.update(score))
            .or_insert_with(|| ConfidenceEntry::new(score));
    }

    /// Get the predicted confidence for an action on a target.
    /// Uses site-specific score if enough observations, otherwise falls back
    /// to generic cross-site score, then to default.
    pub fn predict(&self, role: &str, action: &str, domain: &str) -> f64 {
        // Try site-specific first
        let key = ConfidenceKey::new(role, action, domain);
        if let Some(entry) = self.entries.get(&key) {
            if entry.observations >= self.min_observations {
                return entry.ema_score;
            }
        }

        // Fall back to generic cross-site
        let generic_key = ConfidenceKey::generic(role, action);
        if let Some(entry) = self.generic_entries.get(&generic_key) {
            if entry.observations >= self.min_observations {
                return entry.ema_score;
            }
        }

        // No data: return conservative default
        self.default_confidence
    }

    /// Get confidence with a bonus for elements that have text content matching
    /// common action patterns (e.g., "Submit", "Login", "Accept").
    pub fn predict_with_text_hint(&self, role: &str, action: &str, domain: &str, text: &str) -> f64 {
        let base = self.predict(role, action, domain);

        // Boost confidence for well-known action text
        let text_lower = text.to_lowercase();
        let boost = if is_high_confidence_text(&text_lower) {
            0.1
        } else if is_medium_confidence_text(&text_lower) {
            0.05
        } else {
            0.0
        };

        (base + boost).min(1.0)
    }

    /// Get all entries for a domain, sorted by confidence descending.
    pub fn domain_report(&self, domain: &str) -> Vec<(String, String, f64, u32)> {
        let mut report: Vec<_> = self.entries.iter()
            .filter(|(k, _)| k.domain == domain)
            .map(|(k, e)| (k.role.clone(), k.action.clone(), e.ema_score, e.observations))
            .collect();
        report.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        report
    }

    /// Total number of unique patterns observed.
    pub fn pattern_count(&self) -> usize {
        self.entries.len()
    }
}

/// High-confidence button/link text patterns.
fn is_high_confidence_text(text: &str) -> bool {
    matches!(text,
        "submit" | "login" | "sign in" | "log in" | "continue" |
        "accept" | "ok" | "confirm" | "save" | "next" | "send" |
        "add to cart" | "buy now" | "checkout" | "subscribe"
    )
}

/// Medium-confidence text patterns.
fn is_medium_confidence_text(text: &str) -> bool {
    text.contains("submit") || text.contains("login") || text.contains("sign")
        || text.contains("accept") || text.contains("continue") || text.contains("confirm")
        || text.contains("save") || text.contains("next")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_confidence_is_conservative() {
        let ac = AdaptiveConfidence::new();
        let conf = ac.predict("button", "click", "unknown.com");
        assert_eq!(conf, 0.7);
    }

    #[test]
    fn learns_from_observations() {
        let mut ac = AdaptiveConfidence::new();
        // Record 5 successful clicks on buttons at example.com
        for _ in 0..5 {
            ac.record("button", "click", "example.com", 0.95);
        }
        let conf = ac.predict("button", "click", "example.com");
        assert!(conf > 0.85, "Should have learned high confidence, got {}", conf);
    }

    #[test]
    fn learns_failure_patterns() {
        let mut ac = AdaptiveConfidence::new();
        // Record repeated failures
        for _ in 0..5 {
            ac.record("link", "click", "broken.com", 0.1);
        }
        let conf = ac.predict("link", "click", "broken.com");
        assert!(conf < 0.3, "Should have learned low confidence, got {}", conf);
    }

    #[test]
    fn falls_back_to_generic_when_few_site_observations() {
        let mut ac = AdaptiveConfidence::new();
        // Only 1 site-specific observation (below threshold)
        ac.record("button", "click", "new-site.com", 0.9);
        // But many generic observations
        for _ in 0..5 {
            ac.record("button", "click", "other.com", 0.85);
        }
        let conf = ac.predict("button", "click", "new-site.com");
        // Should use generic since site-specific has < min_observations
        assert!(conf > 0.8, "Should use generic confidence, got {}", conf);
    }

    #[test]
    fn text_hint_boosts_confidence() {
        let ac = AdaptiveConfidence::new();
        let base = ac.predict("button", "click", "site.com");
        let boosted = ac.predict_with_text_hint("button", "click", "site.com", "Submit");
        assert!(boosted > base);
    }

    #[test]
    fn ema_weights_recent_more() {
        let mut ac = AdaptiveConfidence::new();
        // Start with high scores
        for _ in 0..5 {
            ac.record("button", "click", "example.com", 0.9);
        }
        // Then sudden failures
        for _ in 0..5 {
            ac.record("button", "click", "example.com", 0.1);
        }
        let conf = ac.predict("button", "click", "example.com");
        // EMA should have moved toward 0.1 significantly
        assert!(conf < 0.5, "EMA should reflect recent failures, got {}", conf);
    }

    #[test]
    fn domain_report() {
        let mut ac = AdaptiveConfidence::new();
        ac.record("button", "click", "example.com", 0.9);
        ac.record("textbox", "fill", "example.com", 0.8);
        ac.record("link", "click", "other.com", 0.7);

        let report = ac.domain_report("example.com");
        assert_eq!(report.len(), 2);
    }
}
