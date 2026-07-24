//! Provider Success-Rate Scoring: tracks historical success/failure per
//! provider+model combination and recommends the best route for a task type.
//!
//! Uses a decaying average so recent performance weighs more than old history.

use std::collections::HashMap;

/// A task category for routing decisions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskCategory {
    Coding,
    Browsing,
    DataExtraction,
    FormFilling,
    Reasoning,
    Creative,
    General,
}

impl TaskCategory {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "coding" | "code" | "programming" => Self::Coding,
            "browsing" | "navigation" | "web" => Self::Browsing,
            "extraction" | "data" | "scraping" => Self::DataExtraction,
            "form" | "filling" | "input" => Self::FormFilling,
            "reasoning" | "analysis" | "logic" => Self::Reasoning,
            "creative" | "writing" | "generation" => Self::Creative,
            _ => Self::General,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Coding => "coding",
            Self::Browsing => "browsing",
            Self::DataExtraction => "data_extraction",
            Self::FormFilling => "form_filling",
            Self::Reasoning => "reasoning",
            Self::Creative => "creative",
            Self::General => "general",
        }
    }
}

/// A performance record for a provider+model on a task category.
#[derive(Debug, Clone)]
pub struct ProviderPerformance {
    pub provider_slug: String,
    pub model_id: String,
    pub success_count: u32,
    pub failure_count: u32,
    /// Exponential moving average of success (0.0..=1.0)
    pub ema_success_rate: f64,
    /// Average response latency in ms
    pub avg_latency_ms: f64,
    /// Number of observations
    pub observations: u32,
}

impl ProviderPerformance {
    fn new(provider_slug: &str, model_id: &str, success: bool, latency_ms: u64) -> Self {
        Self {
            provider_slug: provider_slug.to_string(),
            model_id: model_id.to_string(),
            success_count: if success { 1 } else { 0 },
            failure_count: if success { 0 } else { 1 },
            ema_success_rate: if success { 1.0 } else { 0.0 },
            avg_latency_ms: latency_ms as f64,
            observations: 1,
        }
    }

    fn record(&mut self, success: bool, latency_ms: u64) {
        let alpha = 0.2; // 20% weight on newest
        let success_val = if success { 1.0 } else { 0.0 };
        self.ema_success_rate = alpha * success_val + (1.0 - alpha) * self.ema_success_rate;
        self.avg_latency_ms = alpha * latency_ms as f64 + (1.0 - alpha) * self.avg_latency_ms;
        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        self.observations += 1;
    }

    /// Combined score factoring in success rate and latency.
    /// Higher is better.
    pub fn combined_score(&self) -> f64 {
        // Normalize latency: faster = higher score (cap at 60s)
        let latency_score = 1.0 - (self.avg_latency_ms / 60000.0).min(1.0);
        // 80% weight on success rate, 20% on speed
        self.ema_success_rate * 0.8 + latency_score * 0.2
    }
}

/// The provider scoring store.
pub struct ProviderScorer {
    /// Key: (provider_slug::model_id, task_category) → performance
    pub scores: HashMap<(String, TaskCategory), ProviderPerformance>,
    /// Minimum observations before recommending a provider
    pub min_observations: u32,
}

impl Default for ProviderScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderScorer {
    pub fn new() -> Self {
        Self {
            scores: HashMap::new(),
            min_observations: 3,
        }
    }

    /// Record a task execution result.
    pub fn record(
        &mut self,
        provider_slug: &str,
        model_id: &str,
        category: TaskCategory,
        success: bool,
        latency_ms: u64,
    ) {
        let key = (format!("{}::{}", provider_slug, model_id), category);
        self.scores.entry(key)
            .and_modify(|p| p.record(success, latency_ms))
            .or_insert_with(|| ProviderPerformance::new(provider_slug, model_id, success, latency_ms));
    }

    /// Get the recommended provider ordering for a task category.
    /// Returns providers sorted by combined score (best first).
    pub fn recommend(&self, category: &TaskCategory) -> Vec<&ProviderPerformance> {
        let mut candidates: Vec<_> = self.scores.iter()
            .filter(|((_, cat), perf)| cat == category && perf.observations >= self.min_observations)
            .map(|(_, perf)| perf)
            .collect();

        candidates.sort_by(|a, b| {
            b.combined_score().partial_cmp(&a.combined_score()).unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Get the single best provider+model for a category, if enough data exists.
    pub fn best_for(&self, category: &TaskCategory) -> Option<(&str, &str, f64)> {
        self.recommend(category).first().map(|p| {
            (p.provider_slug.as_str(), p.model_id.as_str(), p.combined_score())
        })
    }

    /// Get success rate for a specific provider+model on a category.
    pub fn success_rate(&self, provider_slug: &str, model_id: &str, category: &TaskCategory) -> Option<f64> {
        let key = (format!("{}::{}", provider_slug, model_id), category.clone());
        self.scores.get(&key).map(|p| p.ema_success_rate)
    }

    /// Generate a report of all provider performance.
    pub fn report(&self) -> Vec<(String, String, f64, u32)> {
        let mut report: Vec<_> = self.scores.iter()
            .map(|((key, cat), perf)| {
                (key.clone(), cat.label().to_string(), perf.combined_score(), perf.observations)
            })
            .collect();
        report.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        report
    }

    /// Should we try a fallback provider? Returns true if current provider's
    /// success rate is below threshold for the given category.
    pub fn should_fallback(&self, provider_slug: &str, model_id: &str, category: &TaskCategory, threshold: f64) -> bool {
        if let Some(rate) = self.success_rate(provider_slug, model_id, category) {
            rate < threshold
        } else {
            false // Not enough data to decide
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_retrieves_performance() {
        let mut scorer = ProviderScorer::new();
        scorer.record("cloudflare", "kimi-k2", TaskCategory::Coding, true, 1500);
        scorer.record("cloudflare", "kimi-k2", TaskCategory::Coding, true, 2000);
        scorer.record("cloudflare", "kimi-k2", TaskCategory::Coding, false, 3000);
        scorer.record("cloudflare", "kimi-k2", TaskCategory::Coding, true, 1000);

        let rate = scorer.success_rate("cloudflare", "kimi-k2", &TaskCategory::Coding);
        assert!(rate.is_some());
        let r = rate.unwrap();
        assert!(r > 0.5, "Should be > 0.5 with 3/4 successes, got {}", r);
    }

    #[test]
    fn recommends_best_provider() {
        let mut scorer = ProviderScorer::new();
        scorer.min_observations = 2;

        // Provider A: good at coding
        for _ in 0..5 {
            scorer.record("provider_a", "model_a", TaskCategory::Coding, true, 1000);
        }
        // Provider B: bad at coding
        for _ in 0..5 {
            scorer.record("provider_b", "model_b", TaskCategory::Coding, false, 5000);
        }

        let recs = scorer.recommend(&TaskCategory::Coding);
        assert!(recs.len() >= 2);
        assert_eq!(recs[0].provider_slug, "provider_a");
    }

    #[test]
    fn should_fallback_on_low_success() {
        let mut scorer = ProviderScorer::new();
        for _ in 0..5 {
            scorer.record("bad_provider", "model_x", TaskCategory::Browsing, false, 10000);
        }

        assert!(scorer.should_fallback("bad_provider", "model_x", &TaskCategory::Browsing, 0.5));
    }

    #[test]
    fn combined_score_factors_latency() {
        let fast_success = ProviderPerformance {
            provider_slug: "fast".to_string(),
            model_id: "m".to_string(),
            success_count: 10,
            failure_count: 0,
            ema_success_rate: 1.0,
            avg_latency_ms: 500.0,
            observations: 10,
        };
        let slow_success = ProviderPerformance {
            provider_slug: "slow".to_string(),
            model_id: "m".to_string(),
            success_count: 10,
            failure_count: 0,
            ema_success_rate: 1.0,
            avg_latency_ms: 30000.0,
            observations: 10,
        };
        assert!(fast_success.combined_score() > slow_success.combined_score());
    }

    #[test]
    fn best_for_returns_none_with_insufficient_data() {
        let scorer = ProviderScorer::new();
        assert!(scorer.best_for(&TaskCategory::Creative).is_none());
    }
}
