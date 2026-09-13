//! Lazy tool registry with dynamic discovery, LRU caching, and metrics.
//!
//! This module provides a `LazyToolRegistry` wrapper that defers tool initialization
//! until first access, discovers tools on-demand, caches frequently-used tools in an
//! LRU cache, and tracks invocation metrics.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use super::tool_definitions;
use super::types::Tool;

/// Default maximum number of tools to keep in the LRU cache.
pub const DEFAULT_CACHE_CAPACITY: usize = 64;

/// Metrics for a single tool.
#[derive(Debug, Clone, Default)]
pub struct ToolMetrics {
    /// Number of times this tool has been invoked.
    pub invocation_count: u64,
    /// Cumulative latency in milliseconds across all invocations.
    pub total_latency_ms: f64,
}

impl ToolMetrics {
    /// Returns the average latency per invocation in milliseconds.
    pub fn average_latency_ms(&self) -> f64 {
        if self.invocation_count == 0 {
            0.0
        } else {
            self.total_latency_ms / self.invocation_count as f64
        }
    }
}

/// LRU cache for tool definitions.
///
/// Uses a `HashMap` for O(1) lookups and a `VecDeque` to track access order.
/// When the cache exceeds capacity, the least-recently-used entry is evicted.
pub struct LruToolCache {
    capacity: usize,
    cache: HashMap<String, Tool>,
    order: VecDeque<String>,
}

impl LruToolCache {
    /// Create a new LRU cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "LRU cache capacity must be greater than zero");
        Self {
            capacity,
            cache: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Get a tool from the cache by name. Returns `None` if not present.
    /// Moves the accessed tool to the front (most-recently-used position).
    pub fn get(&mut self, name: &str) -> Option<Tool> {
        if self.cache.contains_key(name) {
            // Move to front (most recently used)
            self.order.retain(|k| k != name);
            self.order.push_front(name.to_string());
            self.cache.get(name).cloned()
        } else {
            None
        }
    }

    /// Insert a tool into the cache. If the cache is at capacity, evicts the
    /// least-recently-used entry.
    pub fn put(&mut self, tool: Tool) {
        let name = tool.name.clone();

        // If already in cache, update and move to front
        if self.cache.contains_key(&name) {
            self.order.retain(|k| k != &name);
            self.cache.insert(name.clone(), tool);
            self.order.push_front(name);
            return;
        }

        // If at capacity, evict LRU entry
        while self.cache.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_back() {
                self.cache.remove(&evicted);
            } else {
                break;
            }
        }

        // Insert new entry at front
        self.cache.insert(name.clone(), tool);
        self.order.push_front(name);
    }

    /// Returns the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Returns the maximum capacity of the cache.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if a tool is in the cache without affecting LRU order.
    pub fn contains(&self, name: &str) -> bool {
        self.cache.contains_key(name)
    }

    /// Remove a tool from the cache.
    pub fn remove(&mut self, name: &str) -> Option<Tool> {
        if self.cache.contains_key(name) {
            self.order.retain(|k| k != name);
            self.cache.remove(name)
        } else {
            None
        }
    }

    /// Clear all entries from the cache.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.order.clear();
    }
}

/// A lazy tool registry that defers initialization until first access.
///
/// Instead of eagerly loading all tools at startup, `LazyToolRegistry` loads
/// tools on-demand, caches frequently-used tools in an LRU cache, and tracks
/// invocation metrics.
pub struct LazyToolRegistry {
    /// The full list of tools, loaded lazily on first access.
    tools: Option<Vec<Tool>>,
    /// LRU cache for frequently-accessed tools.
    cache: LruToolCache,
    /// Per-tool invocation metrics.
    metrics: HashMap<String, ToolMetrics>,
    /// Total cache hits.
    cache_hits: u64,
    /// Total cache misses.
    cache_misses: u64,
    /// Tools that have been discovered (scanned) so far.
    discovered: HashMap<String, bool>,
}

impl LazyToolRegistry {
    /// Create a new lazy registry with the default cache capacity (64).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CACHE_CAPACITY)
    }

    /// Create a new lazy registry with a custom cache capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tools: None,
            cache: LruToolCache::new(capacity),
            metrics: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            discovered: HashMap::new(),
        }
    }

    /// Ensure the tool list has been loaded. Loads on first call.
    fn ensure_initialized(&mut self) {
        if self.tools.is_none() {
            let tools = tool_definitions::get_tools();
            self.tools = Some(tools);
        }
    }

    /// Returns true if the registry has been initialized (tools loaded).
    pub fn is_initialized(&self) -> bool {
        self.tools.is_some()
    }

    /// Get all tools, initializing the registry if necessary.
    pub fn get_tools(&mut self) -> &[Tool] {
        self.ensure_initialized();
        self.tools.as_ref().unwrap()
    }

    /// Discover available tools and register them on-demand.
    ///
    /// Returns a list of tool names that were discovered. Tools are scanned
    /// from the full tool list and registered in the discovered set.
    pub fn discover_tools(&mut self) -> Vec<String> {
        self.ensure_initialized();

        let tools = self.tools.as_ref().unwrap();
        let mut newly_discovered = Vec::new();

        for tool in tools {
            if !self.discovered.contains_key(&tool.name) {
                self.discovered.insert(tool.name.clone(), true);
                newly_discovered.push(tool.name.clone());
            }
        }

        newly_discovered
    }

    /// Get a tool by name, using the LRU cache for fast access.
    ///
    /// Returns a cloned `Tool` if found. Cache hits are tracked in metrics.
    pub fn get_tool(&mut self, name: &str) -> Option<Tool> {
        // Check cache first
        if let Some(tool) = self.cache.get(name) {
            self.cache_hits += 1;
            return Some(tool);
        }

        self.cache_misses += 1;

        // Cache miss - load from full tool list
        self.ensure_initialized();
        let tools = self.tools.as_ref().unwrap();

        if let Some(tool) = tools.iter().find(|t| t.name == name) {
            let tool_clone = tool.clone();
            self.cache.put(tool_clone.clone());
            Some(tool_clone)
        } else {
            None
        }
    }

    /// Record a tool invocation with its latency.
    ///
    /// Call this after a tool has been executed to track metrics.
    pub fn record_invocation(&mut self, tool_name: &str, latency_ms: f64) {
        let metrics = self
            .metrics
            .entry(tool_name.to_string())
            .or_default();
        metrics.invocation_count += 1;
        metrics.total_latency_ms += latency_ms;
    }

    /// Get metrics for a specific tool.
    pub fn get_tool_metrics(&self, tool_name: &str) -> Option<&ToolMetrics> {
        self.metrics.get(tool_name)
    }

    /// Get the overall cache hit rate (0.0 to 1.0).
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    /// Get all per-tool metrics.
    pub fn all_metrics(&self) -> &HashMap<String, ToolMetrics> {
        &self.metrics
    }

    /// Get a reference to the underlying LRU cache.
    pub fn cache(&self) -> &LruToolCache {
        &self.cache
    }

    /// Get a mutable reference to the underlying LRU cache.
    pub fn cache_mut(&mut self) -> &mut LruToolCache {
        &mut self.cache
    }

    /// Returns the number of discovered tools.
    pub fn discovered_count(&self) -> usize {
        self.discovered.len()
    }

    /// Check if a specific tool has been discovered.
    pub fn is_discovered(&self, name: &str) -> bool {
        self.discovered.contains_key(name)
    }

    /// Clear all metrics and cache statistics.
    pub fn clear_metrics(&mut self) {
        self.metrics.clear();
        self.cache_hits = 0;
        self.cache_misses = 0;
    }

    /// Reset the registry to uninitialized state.
    pub fn reset(&mut self) {
        self.tools = None;
        self.cache.clear();
        self.metrics.clear();
        self.cache_hits = 0;
        self.cache_misses = 0;
        self.discovered.clear();
    }
}

impl Default for LazyToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper struct for timing tool invocations.
///
/// Usage:
/// ```ignore
/// let timer = ToolTimer::start();
/// // ... execute tool ...
/// let elapsed_ms = timer.elapsed_ms();
/// registry.record_invocation("tool_name", elapsed_ms);
/// ```
pub struct ToolTimer {
    start: Instant,
}

impl ToolTimer {
    /// Start a new timer.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test tool
    fn make_tool(name: &str, desc: &str) -> Tool {
        Tool {
            name: name.to_string(),
            description: desc.to_string(),
            input_schema: serde_json::json!({}),
        }
    }

    // ==================== LRU Cache Tests ====================

    #[test]
    fn test_lru_cache_basic_put_get() {
        let mut cache = LruToolCache::new(4);
        let tool = make_tool("test_tool", "A test tool");
        cache.put(tool.clone());

        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
        assert!(cache.contains("test_tool"));

        let retrieved = cache.get("test_tool");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test_tool");
    }

    #[test]
    fn test_lru_cache_get_nonexistent() {
        let mut cache = LruToolCache::new(4);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_lru_cache_eviction() {
        let mut cache = LruToolCache::new(3);

        cache.put(make_tool("tool1", "Tool 1"));
        cache.put(make_tool("tool2", "Tool 2"));
        cache.put(make_tool("tool3", "Tool 3"));
        assert_eq!(cache.len(), 3);

        // Adding a 4th tool should evict tool1 (LRU)
        cache.put(make_tool("tool4", "Tool 4"));
        assert_eq!(cache.len(), 3);
        assert!(cache.get("tool1").is_none(), "tool1 should have been evicted");
        assert!(cache.get("tool2").is_some());
        assert!(cache.get("tool3").is_some());
        assert!(cache.get("tool4").is_some());
    }

    #[test]
    fn test_lru_cache_access_order() {
        let mut cache = LruToolCache::new(3);

        cache.put(make_tool("tool1", "Tool 1"));
        cache.put(make_tool("tool2", "Tool 2"));
        cache.put(make_tool("tool3", "Tool 3"));

        // Access tool1 to make it recently used
        let _ = cache.get("tool1");

        // Adding tool4 should now evict tool2 (the LRU)
        cache.put(make_tool("tool4", "Tool 4"));
        assert!(cache.get("tool1").is_some(), "tool1 should still be cached");
        assert!(cache.get("tool2").is_none(), "tool2 should have been evicted");
        assert!(cache.get("tool3").is_some());
        assert!(cache.get("tool4").is_some());
    }

    #[test]
    fn test_lru_cache_update_existing() {
        let mut cache = LruToolCache::new(3);

        cache.put(make_tool("tool1", "Original"));
        cache.put(make_tool("tool2", "Tool 2"));
        cache.put(make_tool("tool3", "Tool 3"));

        // Update tool1 with new description
        cache.put(make_tool("tool1", "Updated"));
        assert_eq!(cache.len(), 3);

        let tool = cache.get("tool1").unwrap();
        assert_eq!(tool.description, "Updated");
    }

    #[test]
    fn test_lru_cache_remove() {
        let mut cache = LruToolCache::new(4);
        cache.put(make_tool("tool1", "Tool 1"));
        cache.put(make_tool("tool2", "Tool 2"));

        let removed = cache.remove("tool1");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "tool1");
        assert_eq!(cache.len(), 1);
        assert!(!cache.contains("tool1"));

        let removed_none = cache.remove("nonexistent");
        assert!(removed_none.is_none());
    }

    #[test]
    fn test_lru_cache_clear() {
        let mut cache = LruToolCache::new(4);
        cache.put(make_tool("tool1", "Tool 1"));
        cache.put(make_tool("tool2", "Tool 2"));

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert!(!cache.contains("tool1"));
    }

    #[test]
    fn test_lru_cache_capacity() {
        let cache = LruToolCache::new(64);
        assert_eq!(cache.capacity(), 64);
    }

    #[test]
    #[should_panic(expected = "LRU cache capacity must be greater than zero")]
    fn test_lru_cache_zero_capacity_panics() {
        let _cache = LruToolCache::new(0);
    }

    #[test]
    fn test_lru_cache_many_evictions() {
        let mut cache = LruToolCache::new(5);

        // Insert 20 tools, only last 5 should remain
        for i in 0..20 {
            cache.put(make_tool(&format!("tool_{}", i), &format!("Tool {}", i)));
        }

        assert_eq!(cache.len(), 5);

        // Tools 0-14 should be evicted
        for i in 0..15 {
            assert!(
                cache.get(&format!("tool_{}", i)).is_none(),
                "tool_{} should have been evicted",
                i
            );
        }

        // Tools 15-19 should be present
        for i in 15..20 {
            assert!(
                cache.get(&format!("tool_{}", i)).is_some(),
                "tool_{} should be present",
                i
            );
        }
    }

    // ==================== ToolMetrics Tests ====================

    #[test]
    fn test_tool_metrics_default() {
        let metrics = ToolMetrics::default();
        assert_eq!(metrics.invocation_count, 0);
        assert_eq!(metrics.total_latency_ms, 0.0);
        assert_eq!(metrics.average_latency_ms(), 0.0);
    }

    #[test]
    fn test_tool_metrics_average_latency() {
        let mut metrics = ToolMetrics::default();
        metrics.invocation_count = 5;
        metrics.total_latency_ms = 100.0;
        assert!((metrics.average_latency_ms() - 20.0).abs() < f64::EPSILON);
    }

    // ==================== LazyToolRegistry Tests ====================

    #[test]
    fn test_lazy_registry_not_initialized_by_default() {
        let registry = LazyToolRegistry::new();
        assert!(!registry.is_initialized());
    }

    #[test]
    fn test_lazy_registry_initializes_on_get_tools() {
        let mut registry = LazyToolRegistry::new();
        assert!(!registry.is_initialized());

        {
            let tools = registry.get_tools();
            assert!(!tools.is_empty(), "Should have loaded some tools");
        }
        // After the borrow ends, we can check is_initialized
        assert!(registry.is_initialized());
    }

    #[test]
    fn test_lazy_registry_initializes_on_discover() {
        let mut registry = LazyToolRegistry::new();
        assert!(!registry.is_initialized());

        let discovered = registry.discover_tools();
        assert!(registry.is_initialized());
        assert!(!discovered.is_empty(), "Should have discovered some tools");
    }

    #[test]
    fn test_lazy_registry_initializes_on_get_tool() {
        let mut registry = LazyToolRegistry::new();
        assert!(!registry.is_initialized());

        // Try to get a tool (may or may not exist, but should trigger init)
        let _ = registry.get_tool("some_tool");
        assert!(registry.is_initialized());
    }

    #[test]
    fn test_lazy_registry_discover_tools_returns_names() {
        let mut registry = LazyToolRegistry::new();
        let discovered = registry.discover_tools();

        // Should have discovered at least one tool
        assert!(!discovered.is_empty());

        // All discovered tools should be marked as discovered
        for name in &discovered {
            assert!(registry.is_discovered(name));
        }

        assert_eq!(registry.discovered_count(), discovered.len());
    }

    #[test]
    fn test_lazy_registry_discover_idempotent() {
        let mut registry = LazyToolRegistry::new();

        let first = registry.discover_tools();
        let second = registry.discover_tools();

        // Second call should return empty (already discovered)
        assert!(second.is_empty(), "Second discover should return no new tools");
        assert_eq!(registry.discovered_count(), first.len());
    }

    #[test]
    fn test_lazy_registry_get_tool_from_list() {
        let mut registry = LazyToolRegistry::new();

        // First discover to know what tools exist
        let discovered = registry.discover_tools();
        assert!(!discovered.is_empty());

        // Get the first discovered tool
        let tool_name = &discovered[0];
        let tool = registry.get_tool(tool_name);
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, *tool_name);
    }

    #[test]
    fn test_lazy_registry_get_tool_nonexistent() {
        let mut registry = LazyToolRegistry::new();
        let tool = registry.get_tool("definitely_not_a_real_tool_name_xyz");
        assert!(tool.is_none());
    }

    #[test]
    fn test_lazy_registry_cache_hit_tracking() {
        let mut registry = LazyToolRegistry::new();

        // Discover tools to get a valid tool name
        let discovered = registry.discover_tools();
        let tool_name = discovered[0].clone();

        // First access: cache miss
        let _ = registry.get_tool(&tool_name);
        assert_eq!(registry.cache_hit_rate(), 0.0, "First access should be a miss");

        // Second access: cache hit
        let _ = registry.get_tool(&tool_name);
        assert!(registry.cache_hit_rate() > 0.0, "Second access should be a hit");
    }

    #[test]
    fn test_lazy_registry_record_invocation() {
        let mut registry = LazyToolRegistry::new();

        registry.record_invocation("test_tool", 10.0);
        registry.record_invocation("test_tool", 20.0);
        registry.record_invocation("other_tool", 5.0);

        let test_metrics = registry.get_tool_metrics("test_tool").unwrap();
        assert_eq!(test_metrics.invocation_count, 2);
        assert!((test_metrics.average_latency_ms() - 15.0).abs() < f64::EPSILON);

        let other_metrics = registry.get_tool_metrics("other_tool").unwrap();
        assert_eq!(other_metrics.invocation_count, 1);
        assert!((other_metrics.average_latency_ms() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_lazy_registry_cache_hit_rate_zero_accesses() {
        let registry = LazyToolRegistry::new();
        assert_eq!(registry.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_lazy_registry_clear_metrics() {
        let mut registry = LazyToolRegistry::new();

        registry.record_invocation("tool1", 10.0);
        let _ = registry.get_tool("tool1");

        registry.clear_metrics();

        assert!(registry.get_tool_metrics("tool1").is_none());
        assert_eq!(registry.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_lazy_registry_reset() {
        let mut registry = LazyToolRegistry::new();

        // Initialize and use the registry
        let _ = registry.discover_tools();
        registry.record_invocation("tool1", 10.0);
        let _ = registry.get_tool("tool1");

        assert!(registry.is_initialized());
        assert!(registry.discovered_count() > 0);

        // Reset
        registry.reset();

        assert!(!registry.is_initialized());
        assert_eq!(registry.discovered_count(), 0);
        assert_eq!(registry.cache_hit_rate(), 0.0);
        assert!(registry.all_metrics().is_empty());
        assert!(registry.cache().is_empty());
    }

    #[test]
    fn test_lazy_registry_custom_capacity() {
        let registry = LazyToolRegistry::with_capacity(128);
        assert_eq!(registry.cache().capacity(), 128);
    }

    #[test]
    fn test_lazy_registry_default_trait() {
        let registry = LazyToolRegistry::default();
        assert!(!registry.is_initialized());
        assert_eq!(registry.cache().capacity(), DEFAULT_CACHE_CAPACITY);
    }

    // ==================== ToolTimer Tests ====================

    #[test]
    fn test_tool_timer() {
        let timer = ToolTimer::start();
        // Sleep briefly to ensure some elapsed time
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.elapsed_ms();
        assert!(elapsed >= 5.0, "Timer should have measured at least 5ms");
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_full_workflow() {
        let mut registry = LazyToolRegistry::new();

        // 1. Discover tools
        let discovered = registry.discover_tools();
        assert!(!discovered.is_empty());

        // 2. Access a tool (cache miss, then hit)
        let tool_name = discovered[0].clone();
        let _ = registry.get_tool(&tool_name); // miss
        let _ = registry.get_tool(&tool_name); // hit

        // 3. Record invocations
        registry.record_invocation(&tool_name, 5.0);
        registry.record_invocation(&tool_name, 10.0);

        // 4. Check metrics
        let metrics = registry.get_tool_metrics(&tool_name).unwrap();
        assert_eq!(metrics.invocation_count, 2);
        assert!(registry.cache_hit_rate() > 0.0);

        // 5. Verify tool is in cache
        assert!(registry.cache().contains(&tool_name));
    }

    #[test]
    fn test_cache_stays_within_capacity() {
        let mut cache = LruToolCache::new(10);

        // Insert 50 tools
        for i in 0..50 {
            cache.put(make_tool(&format!("tool_{}", i), &format!("Tool {}", i)));
        }

        // Cache should never exceed capacity
        assert_eq!(cache.len(), 10);
        assert_eq!(cache.len(), cache.capacity());
    }
}
