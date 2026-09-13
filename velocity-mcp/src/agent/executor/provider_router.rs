//! Smart provider routing based on latency, cost, health, and task requirements.
//!
//! The [`ProviderRouter`] tracks per-provider health metrics (consecutive failures,
//! exponential-moving-average latency, etc.) and exposes a scoring function that
//! balances speed vs. cost depending on caller preference.

use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------------------
// ProviderHealth
// ---------------------------------------------------------------------------

/// Per-provider health and cost snapshot used by the router.
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    /// Human-readable provider identifier (e.g. `"openrouter"`).
    pub name: String,
    /// Whether the provider is currently considered healthy.
    pub healthy: bool,
    /// Number of consecutive failures since the last success.
    pub consecutive_failures: u32,
    /// Wall-clock time of the last successful request.
    pub last_success: Option<Instant>,
    /// Exponential moving average of request latency in milliseconds.
    pub avg_latency_ms: f64,
    /// Cost per 1 000 input tokens (USD).
    pub cost_per_1k_input: f64,
    /// Cost per 1 000 output tokens (USD).
    pub cost_per_1k_output: f64,
    /// Maximum context window in tokens the provider can handle.
    pub max_context_tokens: usize,
}

/// Number of consecutive failures before a provider is marked unhealthy.
const UNHEALTHY_THRESHOLD: u32 = 3;

/// Smoothing factor for the exponential moving average (EMA) of latency.
/// Higher values give more weight to the latest sample.
const EMA_ALPHA: f64 = 0.3;

impl ProviderHealth {
    /// Create a new entry with default "unknown" metrics.
    fn new(name: &str, cost_input: f64, cost_output: f64, max_ctx: usize) -> Self {
        Self {
            name: name.to_string(),
            healthy: true,
            consecutive_failures: 0,
            last_success: None,
            avg_latency_ms: 500.0, // pessimistic initial estimate
            cost_per_1k_input: cost_input,
            cost_per_1k_output: cost_output,
            max_context_tokens: max_ctx,
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderRouter
// ---------------------------------------------------------------------------

/// Selects the best AI provider for a given task based on health, latency,
/// cost, and context-window constraints.
#[derive(Debug, Clone)]
pub struct ProviderRouter {
    pub providers: HashMap<String, ProviderHealth>,
}

impl Default for ProviderRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRouter {
    /// Create a router pre-populated with well-known provider costs.
    pub fn new() -> Self {
        let mut providers = HashMap::new();

        // (name, cost_input, cost_output, max_context)
        let known: &[(&str, f64, f64, usize)] = &[
            // GPT-4o
            ("gpt-4o", 0.005, 0.015, 128_000),
            // GPT-3.5-turbo
            ("gpt-3.5-turbo", 0.0005, 0.0015, 16_385),
            // Claude 3.5 Sonnet
            ("claude-3.5-sonnet", 0.003, 0.015, 200_000),
            // Llama 3.1 8B (Cloudflare Workers AI — effectively free)
            ("llama-3.1-8b", 0.0001, 0.0001, 128_000),
            // Deepseek V2
            ("deepseek-v2", 0.00014, 0.00028, 128_000),
        ];

        for &(name, ci, co, max_ctx) in known {
            providers.insert(name.to_string(), ProviderHealth::new(name, ci, co, max_ctx));
        }

        Self { providers }
    }

    // ----- mutation --------------------------------------------------------

    /// Record a successful request, updating the EMA latency and resetting
    /// the consecutive-failure counter.
    pub fn record_success(&mut self, name: &str, latency_ms: f64) {
        if let Some(h) = self.providers.get_mut(name) {
            h.avg_latency_ms = EMA_ALPHA * latency_ms + (1.0 - EMA_ALPHA) * h.avg_latency_ms;
            h.consecutive_failures = 0;
            h.healthy = true;
            h.last_success = Some(Instant::now());
        }
    }

    /// Record a failed request. After [`UNHEALTHY_THRESHOLD`] consecutive
    /// failures the provider is marked unhealthy.
    pub fn record_failure(&mut self, name: &str) {
        if let Some(h) = self.providers.get_mut(name) {
            h.consecutive_failures += 1;
            if h.consecutive_failures >= UNHEALTHY_THRESHOLD {
                h.healthy = false;
            }
        }
    }

    /// Register (or overwrite) a provider at runtime — useful for providers
    /// discovered from user configuration that are not in the built-in list.
    pub fn register_provider(
        &mut self,
        name: &str,
        cost_input: f64,
        cost_output: f64,
        max_context: usize,
    ) {
        self.providers.insert(
            name.to_string(),
            ProviderHealth::new(name, cost_input, cost_output, max_context),
        );
    }

    // ----- scoring ---------------------------------------------------------

    /// Compute a score for a provider. **Higher is better.**
    ///
    /// `score = (1 / avg_latency) * health_factor / cost_factor`
    ///
    /// When `prefer_speed` is true latency is weighted 3× and cost 0.5×;
    /// otherwise cost is weighted 2× and latency 1×.
    fn score(&self, h: &ProviderHealth, prefer_speed: bool) -> f64 {
        let latency_weight: f64 = if prefer_speed { 3.0 } else { 1.0 };
        let cost_weight: f64 = if prefer_speed { 0.5 } else { 2.0 };

        let health_factor: f64 = if h.healthy { 1.0 } else { 0.0 };

        // Blend input + output cost (simple average) and apply weight.
        let blended_cost = ((h.cost_per_1k_input + h.cost_per_1k_output) / 2.0).max(1e-9);
        let cost_factor = blended_cost.powf(cost_weight);

        let inv_latency = (1.0 / h.avg_latency_ms.max(1.0)).powf(latency_weight);

        inv_latency * health_factor / cost_factor
    }

    // ----- selection -------------------------------------------------------

    /// Select the single best provider for a task of `task_tokens` size.
    ///
    /// Returns `None` when no provider can satisfy the constraints.
    pub fn select_provider(&self, task_tokens: usize, prefer_speed: bool) -> Option<String> {
        self.get_fallback_chain(task_tokens, prefer_speed)
            .into_iter()
            .next()
    }

    /// Return **all** viable providers sorted best-first by score.
    ///
    /// A provider is viable when:
    /// 1. It is currently healthy.
    /// 2. Its `max_context_tokens >= task_tokens`.
    pub fn get_fallback_chain(&self, task_tokens: usize, prefer_speed: bool) -> Vec<String> {
        let mut candidates: Vec<(&String, &ProviderHealth)> = self
            .providers
            .iter()
            .filter(|(_, h)| h.healthy && h.max_context_tokens >= task_tokens)
            .collect();

        candidates.sort_by(|a, b| {
            let sa = self.score(a.1, prefer_speed);
            let sb = self.score(b.1, prefer_speed);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates.into_iter().map(|(n, _)| n.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a router with deterministic latencies for testing.
    fn test_router() -> ProviderRouter {
        let mut r = ProviderRouter::new();
        // Set predictable latencies.
        for (name, lat) in [
            ("gpt-4o", 400.0),
            ("gpt-3.5-turbo", 100.0),
            ("claude-3.5-sonnet", 350.0),
            ("llama-3.1-8b", 50.0),
            ("deepseek-v2", 80.0),
        ] {
            if let Some(h) = r.providers.get_mut(name) {
                h.avg_latency_ms = lat;
            }
        }
        r
    }

    // -- health tracking ----------------------------------------------------

    #[test]
    fn success_resets_failures() {
        let mut r = test_router();
        r.record_failure("gpt-4o");
        r.record_failure("gpt-4o");
        assert_eq!(r.providers["gpt-4o"].consecutive_failures, 2);

        r.record_success("gpt-4o", 200.0);
        assert_eq!(r.providers["gpt-4o"].consecutive_failures, 0);
        assert!(r.providers["gpt-4o"].healthy);
    }

    #[test]
    fn three_failures_mark_unhealthy() {
        let mut r = test_router();
        for _ in 0..UNHEALTHY_THRESHOLD {
            r.record_failure("gpt-4o");
        }
        assert!(!r.providers["gpt-4o"].healthy);
    }

    #[test]
    fn two_failures_stay_healthy() {
        let mut r = test_router();
        r.record_failure("gpt-4o");
        r.record_failure("gpt-4o");
        assert!(r.providers["gpt-4o"].healthy);
    }

    #[test]
    fn success_updates_ema_latency() {
        let mut r = ProviderRouter::new();
        let initial = r.providers["gpt-4o"].avg_latency_ms;
        r.record_success("gpt-4o", 100.0);
        let expected = EMA_ALPHA * 100.0 + (1.0 - EMA_ALPHA) * initial;
        let actual = r.providers["gpt-4o"].avg_latency_ms;
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn success_sets_last_success() {
        let mut r = ProviderRouter::new();
        assert!(r.providers["gpt-4o"].last_success.is_none());
        r.record_success("gpt-4o", 100.0);
        assert!(r.providers["gpt-4o"].last_success.is_some());
    }

    #[test]
    fn failure_on_unknown_provider_is_noop() {
        let mut r = ProviderRouter::new();
        r.record_failure("nonexistent");
        assert!(!r.providers.contains_key("nonexistent"));
    }

    // -- provider selection -------------------------------------------------

    #[test]
    fn select_provider_returns_some_for_small_task() {
        let r = test_router();
        let p = r.select_provider(1_000, true);
        assert!(p.is_some());
    }

    #[test]
    fn select_provider_returns_none_when_context_too_large() {
        let r = test_router();
        // No provider has > 1M context.
        assert!(r.select_provider(1_000_000_000, true).is_none());
    }

    #[test]
    fn speed_prefers_fast_provider() {
        let r = test_router();
        // llama-3.1-8b has lowest latency (50ms) — should win on speed.
        let p = r.select_provider(1_000, true).unwrap();
        assert_eq!(p, "llama-3.1-8b");
    }

    #[test]
    fn cost_prefers_cheap_provider() {
        let r = test_router();
        // llama-3.1-8b is both cheapest AND fastest — let's make it slow so
        // cost preference diverges from speed preference.
        let mut r2 = r.clone();
        r2.providers.get_mut("llama-3.1-8b").unwrap().avg_latency_ms = 900.0;
        // Now deepseek-v2 (80ms, $0.00014/$0.00028) should win on cost.
        let p = r2.select_provider(1_000, false).unwrap();
        assert_eq!(p, "deepseek-v2");
    }

    #[test]
    fn unhealthy_provider_excluded() {
        let mut r = test_router();
        // Make the fastest provider unhealthy.
        for _ in 0..UNHEALTHY_THRESHOLD {
            r.record_failure("llama-3.1-8b");
        }
        let p = r.select_provider(1_000, true).unwrap();
        assert_ne!(p, "llama-3.1-8b");
    }

    #[test]
    fn context_window_filters_providers() {
        let mut r = ProviderRouter::new();
        // Give one provider a tiny context window.
        r.providers.get_mut("gpt-3.5-turbo").unwrap().max_context_tokens = 500;
        r.providers.get_mut("gpt-3.5-turbo").unwrap().avg_latency_ms = 10.0;

        // Task of 600 tokens should exclude gpt-3.5-turbo even though it's fastest.
        let p = r.select_provider(600, true).unwrap();
        assert_ne!(p, "gpt-3.5-turbo");
    }

    // -- fallback chain -----------------------------------------------------

    #[test]
    fn fallback_chain_returns_all_viable_sorted() {
        let r = test_router();
        let chain = r.get_fallback_chain(1_000, true);
        // All 5 providers should be viable.
        assert_eq!(chain.len(), 5);
        // First should be the fastest.
        assert_eq!(chain[0], "llama-3.1-8b");
    }

    #[test]
    fn fallback_chain_excludes_unhealthy() {
        let mut r = test_router();
        for _ in 0..UNHEALTHY_THRESHOLD {
            r.record_failure("llama-3.1-8b");
        }
        let chain = r.get_fallback_chain(1_000, true);
        assert!(!chain.contains(&"llama-3.1-8b".to_string()));
        assert_eq!(chain.len(), 4);
    }

    #[test]
    fn fallback_chain_empty_when_no_viable() {
        let r = test_router();
        let chain = r.get_fallback_chain(1_000_000_000, true);
        assert!(chain.is_empty());
    }

    // -- register_provider --------------------------------------------------

    #[test]
    fn register_provider_adds_to_pool() {
        let mut r = ProviderRouter::new();
        r.register_provider("custom-llm", 0.001, 0.002, 64_000);
        assert!(r.providers.contains_key("custom-llm"));
        assert_eq!(r.providers["custom-llm"].max_context_tokens, 64_000);
    }

    #[test]
    fn registered_provider_can_be_selected() {
        let mut r = ProviderRouter::new();
        r.register_provider("ultra-fast", 0.01, 0.01, 128_000);
        r.providers.get_mut("ultra-fast").unwrap().avg_latency_ms = 1.0;
        let p = r.select_provider(1_000, true).unwrap();
        assert_eq!(p, "ultra-fast");
    }

    // -- scoring edge cases -------------------------------------------------

    #[test]
    fn all_unhealthy_returns_none() {
        let mut r = test_router();
        for name in r.providers.keys().cloned().collect::<Vec<_>>() {
            for _ in 0..UNHEALTHY_THRESHOLD {
                r.record_failure(&name);
            }
        }
        assert!(r.select_provider(1_000, true).is_none());
    }

    #[test]
    fn recovery_after_failure_then_success() {
        let mut r = test_router();
        for _ in 0..UNHEALTHY_THRESHOLD {
            r.record_failure("gpt-4o");
        }
        assert!(!r.providers["gpt-4o"].healthy);

        r.record_success("gpt-4o", 200.0);
        assert!(r.providers["gpt-4o"].healthy);
        // Should be selectable again.
        let chain = r.get_fallback_chain(1_000, true);
        assert!(chain.contains(&"gpt-4o".to_string()));
    }
}
