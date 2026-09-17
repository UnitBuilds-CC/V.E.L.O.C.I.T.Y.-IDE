//! Real-time provider status synchronization and health broadcasting.
//!
//! The [`ProviderSync`] struct tracks provider health states, latencies, rate limits,
//! and broadcasts events to subscribers when status changes occur.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// ProviderHealthState
// ---------------------------------------------------------------------------

/// Provider health state enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderHealthState {
    /// Provider is fully operational.
    Healthy,
    /// Provider is experiencing issues but still functional.
    Degraded,
    /// Provider is completely unavailable.
    Down,
    /// Provider status has not been determined yet.
    Unknown,
}

impl std::fmt::Display for ProviderHealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderHealthState::Healthy => write!(f, "Healthy"),
            ProviderHealthState::Degraded => write!(f, "Degraded"),
            ProviderHealthState::Down => write!(f, "Down"),
            ProviderHealthState::Unknown => write!(f, "Unknown"),
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderStatus
// ---------------------------------------------------------------------------

/// Detailed status information for a single provider.
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    /// Internal provider identifier.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Current health state.
    pub status: ProviderHealthState,
    /// Last measured latency in milliseconds.
    pub latency_ms: f64,
    /// When the last health check was performed.
    pub last_check: Instant,
    /// Number of consecutive successful checks.
    pub consecutive_ok: u32,
    /// Number of consecutive failed checks.
    pub consecutive_fail: u32,
    /// List of models supported by this provider.
    pub supported_models: Vec<String>,
    /// Remaining rate limit quota (if known).
    pub rate_limit_remaining: Option<u32>,
    /// When the rate limit resets (if rate-limited).
    pub rate_limit_reset: Option<Instant>,
}

impl ProviderStatus {
    /// Create a new provider status with initial unknown state.
    fn new(name: String, display_name: String, models: Vec<String>) -> Self {
        Self {
            name,
            display_name,
            status: ProviderHealthState::Unknown,
            latency_ms: 0.0,
            last_check: Instant::now(),
            consecutive_ok: 0,
            consecutive_fail: 0,
            supported_models: models,
            rate_limit_remaining: None,
            rate_limit_reset: None,
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderEvent
// ---------------------------------------------------------------------------

/// Events broadcast when provider state changes.
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    /// Provider health state changed.
    StatusChanged {
        provider: String,
        old: ProviderHealthState,
        new_status: ProviderHealthState,
    },
    /// Provider latency measurement updated.
    LatencyUpdated { provider: String, latency_ms: f64 },
    /// Provider hit rate limit.
    RateLimitHit { provider: String, reset_at: Instant },
    /// A model became available on a provider.
    ModelAvailable { provider: String, model: String },
    /// A model became unavailable on a provider.
    ModelUnavailable { provider: String, model: String },
}

// ---------------------------------------------------------------------------
// ProviderSyncSummary
// ---------------------------------------------------------------------------

/// Summary statistics for all tracked providers.
#[derive(Debug, Clone)]
pub struct ProviderSyncSummary {
    /// Total number of registered providers.
    pub total_providers: usize,
    /// Number of healthy providers.
    pub healthy: usize,
    /// Number of degraded providers.
    pub degraded: usize,
    /// Number of down providers.
    pub down: usize,
    /// Seconds since last sync.
    pub last_sync_ago_secs: u64,
}

// ---------------------------------------------------------------------------
// ProviderSync
// ---------------------------------------------------------------------------

/// Central coordinator for provider status synchronization and event broadcasting.
pub struct ProviderSync {
    /// Map of provider name to their current status.
    pub providers: HashMap<String, ProviderStatus>,
    /// Subscribers receiving provider events.
    subscribers: Vec<mpsc::Sender<ProviderEvent>>,
    /// Interval between periodic health checks.
    pub check_interval: Duration,
    /// When the last sync was performed.
    pub last_sync: Instant,
}

impl Default for ProviderSync {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderSync {
    /// Create a new provider sync coordinator with default settings.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            subscribers: Vec::new(),
            check_interval: Duration::from_secs(30),
            last_sync: Instant::now(),
        }
    }

    /// Register a new provider with its supported models.
    pub fn register_provider(&mut self, name: String, display_name: String, models: Vec<String>) {
        let status = ProviderStatus::new(name.clone(), display_name, models);
        self.providers.insert(name, status);
    }

    /// Update the health status and latency for a provider.
    ///
    /// Broadcasts a `StatusChanged` event if the health state changed,
    /// and a `LatencyUpdated` event on every call.
    pub fn update_status(&mut self, name: &str, health: ProviderHealthState, latency_ms: f64) {
        if let Some(provider) = self.providers.get_mut(name) {
            let old_status = provider.status;
            provider.status = health;
            provider.latency_ms = latency_ms;
            provider.last_check = Instant::now();

            match health {
                ProviderHealthState::Healthy | ProviderHealthState::Degraded => {
                    provider.consecutive_ok += 1;
                    provider.consecutive_fail = 0;
                }
                ProviderHealthState::Down => {
                    provider.consecutive_fail += 1;
                    provider.consecutive_ok = 0;
                }
                ProviderHealthState::Unknown => {}
            }

            // Broadcast status change if state changed
            if old_status != health {
                self.broadcast(ProviderEvent::StatusChanged {
                    provider: name.to_string(),
                    old: old_status,
                    new_status: health,
                });
            }

            // Always broadcast latency update
            self.broadcast(ProviderEvent::LatencyUpdated {
                provider: name.to_string(),
                latency_ms,
            });
        }
    }

    /// Record that a provider hit its rate limit.
    pub fn record_rate_limit(&mut self, name: &str, reset_at: Instant) {
        if let Some(provider) = self.providers.get_mut(name) {
            provider.rate_limit_reset = Some(reset_at);
            provider.rate_limit_remaining = Some(0);

            self.broadcast(ProviderEvent::RateLimitHit {
                provider: name.to_string(),
                reset_at,
            });
        }
    }

    /// Subscribe to provider events.
    ///
    /// Returns a receiver that will receive all future provider events.
    pub fn subscribe(&mut self) -> mpsc::Receiver<ProviderEvent> {
        let (tx, rx) = mpsc::channel();
        self.subscribers.push(tx);
        rx
    }

    /// Broadcast an event to all subscribers.
    ///
    /// Removes subscribers whose channels have been disconnected.
    pub fn broadcast(&mut self, event: ProviderEvent) {
        self.subscribers.retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Get the current status of a provider.
    pub fn get_status(&self, name: &str) -> Option<&ProviderStatus> {
        self.providers.get(name)
    }

    /// Get all providers that are currently healthy.
    pub fn healthy_providers(&self) -> Vec<&ProviderStatus> {
        self.providers
            .values()
            .filter(|p| p.status == ProviderHealthState::Healthy)
            .collect()
    }

    /// Check if a sync check is due based on the check interval.
    pub fn needs_sync(&self) -> bool {
        self.last_sync.elapsed() >= self.check_interval
    }

    /// Generate a summary of all provider statuses.
    pub fn sync_summary(&self) -> ProviderSyncSummary {
        let mut healthy = 0;
        let mut degraded = 0;
        let mut down = 0;

        for provider in self.providers.values() {
            match provider.status {
                ProviderHealthState::Healthy => healthy += 1,
                ProviderHealthState::Degraded => degraded += 1,
                ProviderHealthState::Down => down += 1,
                ProviderHealthState::Unknown => {}
            }
        }

        ProviderSyncSummary {
            total_providers: self.providers.len(),
            healthy,
            degraded,
            down,
            last_sync_ago_secs: self.last_sync.elapsed().as_secs(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_provider() {
        let mut sync = ProviderSync::new();
        sync.register_provider(
            "openai".to_string(),
            "OpenAI".to_string(),
            vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()],
        );

        let status = sync.get_status("openai").unwrap();
        assert_eq!(status.name, "openai");
        assert_eq!(status.display_name, "OpenAI");
        assert_eq!(status.status, ProviderHealthState::Unknown);
        assert_eq!(status.supported_models.len(), 2);
    }

    #[test]
    fn test_status_update_healthy() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);

        sync.update_status("test", ProviderHealthState::Healthy, 150.0);

        let status = sync.get_status("test").unwrap();
        assert_eq!(status.status, ProviderHealthState::Healthy);
        assert_eq!(status.latency_ms, 150.0);
        assert_eq!(status.consecutive_ok, 1);
        assert_eq!(status.consecutive_fail, 0);
    }

    #[test]
    fn test_status_update_down() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);

        sync.update_status("test", ProviderHealthState::Down, 5000.0);

        let status = sync.get_status("test").unwrap();
        assert_eq!(status.status, ProviderHealthState::Down);
        assert_eq!(status.consecutive_fail, 1);
        assert_eq!(status.consecutive_ok, 0);
    }

    #[test]
    fn test_status_transition_broadcasts_event() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);
        let rx = sync.subscribe();

        sync.update_status("test", ProviderHealthState::Healthy, 100.0);

        // Should receive StatusChanged and LatencyUpdated
        let event1 = rx.try_recv().unwrap();
        assert!(matches!(event1, ProviderEvent::StatusChanged { .. }));

        let event2 = rx.try_recv().unwrap();
        assert!(matches!(event2, ProviderEvent::LatencyUpdated { .. }));

        // No more events
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_no_status_change_no_status_changed_event() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);

        // Set initial status
        sync.update_status("test", ProviderHealthState::Healthy, 100.0);

        // Subscribe after initial update
        let rx = sync.subscribe();

        // Update with same status
        sync.update_status("test", ProviderHealthState::Healthy, 120.0);

        // Should only receive LatencyUpdated, not StatusChanged
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ProviderEvent::LatencyUpdated { .. }));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_rate_limit_recording() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);
        let rx = sync.subscribe();

        let reset_at = Instant::now() + Duration::from_secs(60);
        sync.record_rate_limit("test", reset_at);

        let status = sync.get_status("test").unwrap();
        assert_eq!(status.rate_limit_remaining, Some(0));
        assert!(status.rate_limit_reset.is_some());

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ProviderEvent::RateLimitHit { .. }));
    }

    #[test]
    fn test_healthy_providers_filter() {
        let mut sync = ProviderSync::new();
        sync.register_provider("p1".to_string(), "P1".to_string(), vec![]);
        sync.register_provider("p2".to_string(), "P2".to_string(), vec![]);
        sync.register_provider("p3".to_string(), "P3".to_string(), vec![]);

        sync.update_status("p1", ProviderHealthState::Healthy, 100.0);
        sync.update_status("p2", ProviderHealthState::Degraded, 500.0);
        sync.update_status("p3", ProviderHealthState::Healthy, 200.0);

        let healthy = sync.healthy_providers();
        assert_eq!(healthy.len(), 2);
        assert!(healthy
            .iter()
            .all(|p| p.status == ProviderHealthState::Healthy));
    }

    #[test]
    fn test_needs_sync_initially_false() {
        let sync = ProviderSync::new();
        // Just created, should not need sync yet
        assert!(!sync.needs_sync());
    }

    #[test]
    fn test_needs_sync_after_interval() {
        let mut sync = ProviderSync::new();
        sync.check_interval = Duration::from_millis(10);
        sync.last_sync = Instant::now() - Duration::from_millis(20);

        assert!(sync.needs_sync());
    }

    #[test]
    fn test_sync_summary_accuracy() {
        let mut sync = ProviderSync::new();
        sync.register_provider("p1".to_string(), "P1".to_string(), vec![]);
        sync.register_provider("p2".to_string(), "P2".to_string(), vec![]);
        sync.register_provider("p3".to_string(), "P3".to_string(), vec![]);
        sync.register_provider("p4".to_string(), "P4".to_string(), vec![]);

        sync.update_status("p1", ProviderHealthState::Healthy, 100.0);
        sync.update_status("p2", ProviderHealthState::Degraded, 500.0);
        sync.update_status("p3", ProviderHealthState::Down, 5000.0);
        // p4 remains Unknown

        let summary = sync.sync_summary();
        assert_eq!(summary.total_providers, 4);
        assert_eq!(summary.healthy, 1);
        assert_eq!(summary.degraded, 1);
        assert_eq!(summary.down, 1);
    }

    #[test]
    fn test_multiple_subscribers() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);

        let rx1 = sync.subscribe();
        let rx2 = sync.subscribe();

        sync.update_status("test", ProviderHealthState::Healthy, 100.0);

        // Both subscribers should receive events
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn test_disconnected_subscriber_removed() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);

        let rx = sync.subscribe();
        assert_eq!(sync.subscribers.len(), 1);

        // Drop the receiver
        drop(rx);

        // Broadcast should clean up disconnected subscriber
        sync.broadcast(ProviderEvent::LatencyUpdated {
            provider: "test".to_string(),
            latency_ms: 100.0,
        });

        assert_eq!(sync.subscribers.len(), 0);
    }

    #[test]
    fn test_consecutive_counters() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);

        sync.update_status("test", ProviderHealthState::Healthy, 100.0);
        sync.update_status("test", ProviderHealthState::Healthy, 110.0);
        sync.update_status("test", ProviderHealthState::Healthy, 120.0);

        let status = sync.get_status("test").unwrap();
        assert_eq!(status.consecutive_ok, 3);
        assert_eq!(status.consecutive_fail, 0);

        sync.update_status("test", ProviderHealthState::Down, 5000.0);
        let status = sync.get_status("test").unwrap();
        assert_eq!(status.consecutive_ok, 0);
        assert_eq!(status.consecutive_fail, 1);
    }

    #[test]
    fn test_get_status_nonexistent() {
        let sync = ProviderSync::new();
        assert!(sync.get_status("nonexistent").is_none());
    }

    #[test]
    fn test_provider_health_state_display() {
        assert_eq!(format!("{}", ProviderHealthState::Healthy), "Healthy");
        assert_eq!(format!("{}", ProviderHealthState::Degraded), "Degraded");
        assert_eq!(format!("{}", ProviderHealthState::Down), "Down");
        assert_eq!(format!("{}", ProviderHealthState::Unknown), "Unknown");
    }

    #[test]
    fn test_update_status_nonexistent_provider() {
        let mut sync = ProviderSync::new();
        // Should not panic
        sync.update_status("nonexistent", ProviderHealthState::Healthy, 100.0);
    }

    #[test]
    fn test_record_rate_limit_nonexistent_provider() {
        let mut sync = ProviderSync::new();
        let reset_at = Instant::now() + Duration::from_secs(60);
        // Should not panic
        sync.record_rate_limit("nonexistent", reset_at);
    }

    #[test]
    fn test_default_check_interval() {
        let sync = ProviderSync::new();
        assert_eq!(sync.check_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_latency_updated_always_broadcast() {
        let mut sync = ProviderSync::new();
        sync.register_provider("test".to_string(), "Test".to_string(), vec![]);
        let rx = sync.subscribe();

        // First update
        sync.update_status("test", ProviderHealthState::Healthy, 100.0);
        let _ = rx.try_recv(); // StatusChanged
        let _ = rx.try_recv(); // LatencyUpdated

        // Second update with same health but different latency
        sync.update_status("test", ProviderHealthState::Healthy, 150.0);

        // Should receive only LatencyUpdated
        let event = rx.try_recv().unwrap();
        match event {
            ProviderEvent::LatencyUpdated { latency_ms, .. } => {
                assert_eq!(latency_ms, 150.0);
            }
            _ => panic!("Expected LatencyUpdated event"),
        }
    }

    #[test]
    fn test_summary_last_sync_ago() {
        let mut sync = ProviderSync::new();
        sync.last_sync = Instant::now() - Duration::from_secs(5);

        let summary = sync.sync_summary();
        assert!(summary.last_sync_ago_secs >= 5);
    }
}
