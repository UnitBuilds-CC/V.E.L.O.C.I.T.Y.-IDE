//! Template store — learned solution cache keyed by visual fingerprint.
//!
//! The core insight: if we've solved a challenge with a given visual fingerprint
//! before, we can replay the stored solution sequence without spending any LLM
//! tokens. Templates are indexed by visual hash (O(1) lookup) with a secondary
//! provider-based index for fuzzy fallback.

use std::collections::HashMap;

use super::challenge::ChallengeDescriptor;
use super::state_machine::ChallengeAction;

/// A stored solve template — the recorded solution for a visual fingerprint.
#[derive(Debug, Clone)]
pub struct SolveTemplate {
    /// Visual fingerprint hash that triggers this template.
    pub visual_hash: u64,
    /// Full descriptor for human-readable logging and fuzzy matching.
    pub descriptor: ChallengeDescriptor,
    /// Ordered solve sequence: (state_id, action) pairs.
    pub solve_sequence: Vec<(String, ChallengeAction)>,
    /// Number of successful replays.
    pub success_count: u32,
    /// Number of failed replays.
    pub failure_count: u32,
    /// Timestamp of last use (monotonic counter).
    pub last_used: u64,
    /// Confidence score (0.0 - 1.0), decays on failure.
    pub confidence: f32,
}

impl SolveTemplate {
    pub fn new(
        visual_hash: u64,
        descriptor: ChallengeDescriptor,
        solve_sequence: Vec<(String, ChallengeAction)>,
    ) -> Self {
        Self {
            visual_hash,
            descriptor,
            solve_sequence,
            success_count: 0,
            failure_count: 0,
            last_used: 0,
            confidence: 0.5, // Start with moderate confidence
        }
    }

    /// Record a successful replay.
    pub fn record_success(&mut self, timestamp: u64) {
        self.success_count += 1;
        self.last_used = timestamp;
        // Confidence increases toward 1.0
        self.confidence = (self.confidence + 0.1).min(1.0);
    }

    /// Record a failed replay.
    pub fn record_failure(&mut self, timestamp: u64) {
        self.failure_count += 1;
        self.last_used = timestamp;
        // Confidence decays
        self.confidence = (self.confidence - 0.15).max(0.0);
    }

    /// Whether this template is reliable enough for zero-token replay.
    pub fn is_reliable(&self) -> bool {
        self.confidence >= 0.8 && self.success_count >= 2
    }
}

/// The template store — O(1) lookup by visual hash with fuzzy fallback.
pub struct TemplateStore {
    /// Primary index: visual_hash -> template (O(1) exact match).
    by_hash: HashMap<u64, SolveTemplate>,
    /// Secondary index: provider -> list of visual hashes (for fuzzy fallback).
    by_provider: HashMap<String, Vec<u64>>,
    /// Maximum number of templates before eviction.
    max_templates: usize,
    /// Monotonic timestamp counter.
    clock: u64,
}

impl Default for TemplateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateStore {
    pub fn new() -> Self {
        Self {
            by_hash: HashMap::new(),
            by_provider: HashMap::new(),
            max_templates: 256,
            clock: 0,
        }
    }

    pub fn with_capacity(max_templates: usize) -> Self {
        Self {
            by_hash: HashMap::new(),
            by_provider: HashMap::new(),
            max_templates,
            clock: 0,
        }
    }

    /// O(1) exact lookup by visual fingerprint hash.
    pub fn lookup(&self, visual_hash: u64) -> Option<&SolveTemplate> {
        self.by_hash.get(&visual_hash)
    }

    /// Mutable lookup (for recording outcomes).
    pub fn lookup_mut(&mut self, visual_hash: u64) -> Option<&mut SolveTemplate> {
        self.by_hash.get_mut(&visual_hash)
    }

    /// Fuzzy lookup by provider + variant (secondary index).
    /// Returns the highest-confidence template for the provider.
    pub fn fuzzy_lookup(&self, descriptor: &ChallengeDescriptor) -> Option<&SolveTemplate> {
        let hashes = self.by_provider.get(&descriptor.provider)?;
        hashes
            .iter()
            .filter_map(|h| self.by_hash.get(h))
            .filter(|t| t.descriptor.variant == descriptor.variant)
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Store a new template (or update an existing one).
    pub fn store(&mut self, template: SolveTemplate) {
        let hash = template.visual_hash;
        let provider = template.descriptor.provider.clone();

        // Evict if over capacity
        if self.by_hash.len() >= self.max_templates && !self.by_hash.contains_key(&hash) {
            self.evict_stale();
        }

        // Update provider index
        let provider_list = self.by_provider.entry(provider).or_default();
        if !provider_list.contains(&hash) {
            provider_list.push(hash);
        }

        self.by_hash.insert(hash, template);
    }

    /// Record the outcome of a template replay.
    pub fn record_outcome(&mut self, visual_hash: u64, success: bool) {
        self.clock += 1;
        if let Some(template) = self.by_hash.get_mut(&visual_hash) {
            if success {
                template.record_success(self.clock);
            } else {
                template.record_failure(self.clock);
            }
        }
    }

    /// Evict the least-recently-used template.
    pub fn evict_stale(&mut self) {
        if self.by_hash.is_empty() {
            return;
        }

        // Find the LRU entry
        let lru_hash = self
            .by_hash
            .iter()
            .min_by_key(|(_, t)| t.last_used)
            .map(|(h, _)| *h);

        if let Some(hash) = lru_hash {
            if let Some(template) = self.by_hash.remove(&hash) {
                // Remove from provider index
                if let Some(list) = self.by_provider.get_mut(&template.descriptor.provider) {
                    list.retain(|&h| h != hash);
                }
            }
        }
    }

    /// Number of stored templates.
    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    /// Number of templates reliable enough for zero-token replay.
    pub fn reliable_count(&self) -> usize {
        self.by_hash.values().filter(|t| t.is_reliable()).count()
    }

    /// Get all templates for a provider.
    pub fn templates_for_provider(&self, provider: &str) -> Vec<&SolveTemplate> {
        self.by_provider
            .get(provider)
            .map(|hashes| {
                hashes
                    .iter()
                    .filter_map(|h| self.by_hash.get(h))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::captcha::challenge::ChallengeDescriptor;
    use crate::engine::captcha::state_machine::ChallengeAction;

    fn make_template(hash: u64, provider: &str, variant: &str) -> SolveTemplate {
        let desc = ChallengeDescriptor::from_known_provider(provider, variant)
            .with_visual_hash(hash);
        let seq = vec![
            ("shown".to_string(), ChallengeAction::click("checkbox")),
            ("solved".to_string(), ChallengeAction::submit()),
        ];
        SolveTemplate::new(hash, desc, seq)
    }

    #[test]
    fn store_and_retrieve_by_hash() {
        let mut store = TemplateStore::new();
        let template = make_template(0xDEAD, "hcaptcha", "checkbox");
        store.store(template);

        let found = store.lookup(0xDEAD);
        assert!(found.is_some());
        assert_eq!(found.unwrap().descriptor.provider, "hcaptcha");
    }

    #[test]
    fn fuzzy_match_by_provider_variant() {
        let mut store = TemplateStore::new();
        store.store(make_template(0x01, "hcaptcha", "tile_flip"));
        store.store(make_template(0x02, "hcaptcha", "checkbox"));
        store.store(make_template(0x03, "recaptcha", "image_select"));

        let query = ChallengeDescriptor::from_known_provider("hcaptcha", "tile_flip");
        let found = store.fuzzy_lookup(&query);
        assert!(found.is_some());
        assert_eq!(found.unwrap().visual_hash, 0x01);
    }

    #[test]
    fn success_increases_confidence() {
        let mut store = TemplateStore::new();
        store.store(make_template(0xAA, "turnstile", "managed"));

        store.record_outcome(0xAA, true);
        store.record_outcome(0xAA, true);
        store.record_outcome(0xAA, true);

        let t = store.lookup(0xAA).unwrap();
        assert!(t.confidence > 0.5);
        assert_eq!(t.success_count, 3);
    }

    #[test]
    fn failure_decays_confidence() {
        let mut store = TemplateStore::new();
        store.store(make_template(0xBB, "recaptcha", "image_select"));

        // Build up confidence
        for _ in 0..5 {
            store.record_outcome(0xBB, true);
        }
        let high = store.lookup(0xBB).unwrap().confidence;

        // Fail a few times
        store.record_outcome(0xBB, false);
        store.record_outcome(0xBB, false);
        let low = store.lookup(0xBB).unwrap().confidence;

        assert!(low < high);
    }

    #[test]
    fn eviction_removes_lru() {
        let mut store = TemplateStore::with_capacity(3);
        store.store(make_template(0x01, "a", "x"));
        store.store(make_template(0x02, "b", "y"));
        store.store(make_template(0x03, "c", "z"));

        // Access 0x01 and 0x03 to make them recently used
        store.record_outcome(0x01, true);
        store.record_outcome(0x03, true);

        // Store a 4th — should evict the LRU (0x02, which has last_used=0)
        store.store(make_template(0x04, "d", "w"));

        assert!(store.lookup(0x01).is_some()); // recently used
        assert!(store.lookup(0x02).is_none()); // evicted (LRU)
        assert!(store.lookup(0x03).is_some()); // recently used
        assert!(store.lookup(0x04).is_some()); // just added
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn miss_returns_none() {
        let store = TemplateStore::new();
        assert!(store.lookup(0xFFFF).is_none());

        let query = ChallengeDescriptor::from_known_provider("unknown", "nothing");
        assert!(store.fuzzy_lookup(&query).is_none());
    }

    #[test]
    fn template_reliability() {
        let mut store = TemplateStore::new();
        store.store(make_template(0xCC, "hcaptcha", "checkbox"));

        // Not reliable initially
        assert!(!store.lookup(0xCC).unwrap().is_reliable());

        // Build up success
        for _ in 0..5 {
            store.record_outcome(0xCC, true);
        }
        assert!(store.lookup(0xCC).unwrap().is_reliable());
    }
}
