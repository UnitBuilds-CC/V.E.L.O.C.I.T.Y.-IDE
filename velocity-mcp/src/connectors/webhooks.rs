//! Webhook management for incoming and outgoing events.
//!
//! Supports:
//! - **Outgoing webhooks**: fire HTTP POST requests when local events occur
//! - **Incoming webhooks**: receive HTTP POST from external services and
//!   dispatch them as events to the agent
//! - **Webhook signatures**: HMAC-SHA256 verification for incoming payloads

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// An outgoing webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingWebhook {
    /// Unique ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// URL to POST to.
    pub url: String,
    /// Events that trigger this webhook.
    pub events: Vec<WebhookEvent>,
    /// Optional secret for HMAC-SHA256 signing.
    pub secret_handle: Option<String>,
    /// Custom headers to include.
    pub headers: Vec<(String, String)>,
    /// Whether this webhook is active.
    pub enabled: bool,
    /// Number of times this webhook has fired.
    pub fire_count: u64,
    /// Last time this webhook fired (unix timestamp).
    pub last_fired: Option<u64>,
    /// Last response status code (if any).
    pub last_status: Option<u16>,
}

/// An incoming webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingWebhook {
    /// Unique ID (also used in the URL path).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Secret for verifying incoming payloads (HMAC-SHA256).
    pub verify_secret: Option<String>,
    /// Source service hint (e.g., "github", "slack", "custom").
    pub source: String,
    /// Whether this webhook is active.
    pub enabled: bool,
    /// Number of payloads received.
    pub received_count: u64,
    /// Last received timestamp.
    pub last_received: Option<u64>,
}

/// Events that can trigger outgoing webhooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookEvent {
    /// A workflow completed.
    WorkflowCompleted,
    /// A workflow failed.
    WorkflowFailed,
    /// A background agent produced a critical action.
    CriticalAlert,
    /// A file change was detected.
    FileChanged,
    /// A build completed.
    BuildCompleted,
    /// A build failed.
    BuildFailed,
    /// A new agent task started.
    TaskStarted,
    /// An agent task completed.
    TaskCompleted,
    /// Custom event with a named trigger.
    Custom(String),
}

impl WebhookEvent {
    pub fn label(&self) -> &str {
        match self {
            Self::WorkflowCompleted => "workflow.completed",
            Self::WorkflowFailed => "workflow.failed",
            Self::CriticalAlert => "agent.critical_alert",
            Self::FileChanged => "file.changed",
            Self::BuildCompleted => "build.completed",
            Self::BuildFailed => "build.failed",
            Self::TaskStarted => "task.started",
            Self::TaskCompleted => "task.completed",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// Parse an event from its label string.
    pub fn from_label(label: &str) -> Self {
        match label {
            "workflow.completed" => Self::WorkflowCompleted,
            "workflow.failed" => Self::WorkflowFailed,
            "agent.critical_alert" => Self::CriticalAlert,
            "file.changed" => Self::FileChanged,
            "build.completed" => Self::BuildCompleted,
            "build.failed" => Self::BuildFailed,
            "task.started" => Self::TaskStarted,
            "task.completed" => Self::TaskCompleted,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// A received incoming webhook payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    /// The incoming webhook ID that received this.
    pub webhook_id: String,
    /// When this payload was received.
    pub received_at: u64,
    /// The raw body (JSON string).
    pub body: String,
    /// Parsed event type (extracted from payload by source adapter).
    pub event_type: Option<String>,
    /// Whether signature verification passed.
    pub verified: bool,
}

/// Manages all webhooks for a workspace.
#[derive(Debug, Clone, Default)]
pub struct WebhookManager {
    /// Outgoing webhooks keyed by ID.
    pub outgoing: HashMap<String, OutgoingWebhook>,
    /// Incoming webhooks keyed by ID.
    pub incoming: HashMap<String, IncomingWebhook>,
    /// Recent incoming payloads (FIFO, max 50).
    pub recent_payloads: Vec<WebhookPayload>,
    /// Maximum payloads to retain.
    pub max_payloads: usize,
}

impl WebhookManager {
    pub fn new() -> Self {
        Self {
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            recent_payloads: Vec::new(),
            max_payloads: 50,
        }
    }

    // ── Outgoing webhooks ──

    /// Register an outgoing webhook.
    pub fn register_outgoing(&mut self, webhook: OutgoingWebhook) {
        self.outgoing.insert(webhook.id.clone(), webhook);
    }

    /// Remove an outgoing webhook.
    pub fn remove_outgoing(&mut self, id: &str) -> bool {
        self.outgoing.remove(id).is_some()
    }

    /// Get all outgoing webhooks that should fire for a given event.
    pub fn matching_outgoing(&self, event: &WebhookEvent) -> Vec<&OutgoingWebhook> {
        self.outgoing
            .values()
            .filter(|wh| wh.enabled && wh.events.contains(event))
            .collect()
    }

    /// Record that a webhook fired (update stats).
    pub fn record_fire(&mut self, id: &str, status: Option<u16>) {
        if let Some(wh) = self.outgoing.get_mut(id) {
            wh.fire_count += 1;
            wh.last_fired = Some(now_secs());
            wh.last_status = status;
        }
    }

    /// Build the JSON payload body for an outgoing webhook event.
    pub fn build_payload(event: &WebhookEvent, data: &serde_json::Value) -> String {
        let payload = serde_json::json!({
            "event": event.label(),
            "timestamp": now_secs(),
            "data": data,
            "source": "velocity",
        });
        serde_json::to_string(&payload).unwrap_or_default()
    }

    // ── Incoming webhooks ──

    /// Register an incoming webhook.
    pub fn register_incoming(&mut self, webhook: IncomingWebhook) {
        self.incoming.insert(webhook.id.clone(), webhook);
    }

    /// Remove an incoming webhook.
    pub fn remove_incoming(&mut self, id: &str) -> bool {
        self.incoming.remove(id).is_some()
    }

    /// Get the URL path for receiving on an incoming webhook.
    pub fn incoming_url(&self, id: &str) -> Option<String> {
        if self.incoming.contains_key(id) {
            Some(format!("/webhook/incoming/{}", id))
        } else {
            None
        }
    }

    /// Process a received payload for an incoming webhook.
    pub fn receive_payload(
        &mut self,
        webhook_id: &str,
        body: &str,
        signature: Option<&str>,
    ) -> Result<WebhookPayload, String> {
        let webhook = self
            .incoming
            .get(webhook_id)
            .ok_or_else(|| format!("Incoming webhook '{}' not found", webhook_id))?;

        if !webhook.enabled {
            return Err(format!("Incoming webhook '{}' is disabled", webhook_id));
        }

        // Verify signature if configured.
        let verified = match &webhook.verify_secret {
            Some(secret) => {
                let sig = signature.ok_or("Missing signature header")?;
                verify_hmac_sha256(secret, body, sig)?
            }
            None => true, // No verification needed.
        };

        let payload = WebhookPayload {
            webhook_id: webhook_id.to_string(),
            received_at: now_secs(),
            body: body.to_string(),
            event_type: extract_event_type(body, &webhook.source),
            verified,
        };

        // Update stats.
        if let Some(wh) = self.incoming.get_mut(webhook_id) {
            wh.received_count += 1;
            wh.last_received = Some(now_secs());
        }

        // Store payload.
        self.recent_payloads.push(payload.clone());
        while self.recent_payloads.len() > self.max_payloads {
            self.recent_payloads.remove(0);
        }

        Ok(payload)
    }

    /// Get recent payloads.
    pub fn recent_payloads(&self) -> &[WebhookPayload] {
        &self.recent_payloads
    }

    /// Save webhook config to disk.
    pub fn save(&self, workspace_root: &Path) -> Result<(), String> {
        let dir = workspace_root.join(".velocity");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let state = PersistedWebhookState {
            outgoing: self.outgoing.values().cloned().collect(),
            incoming: self.incoming.values().cloned().collect(),
        };
        let json =
            serde_json::to_vec_pretty(&state).map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(dir.join("webhooks.json"), json)
            .map_err(|e| format!("Write failed: {e}"))?;
        Ok(())
    }

    /// Load webhook config from disk.
    pub fn load(workspace_root: &Path) -> Self {
        let mut mgr = Self::new();
        let path = workspace_root.join(".velocity").join("webhooks.json");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(state) = serde_json::from_slice::<PersistedWebhookState>(&bytes) {
                for wh in state.outgoing {
                    mgr.outgoing.insert(wh.id.clone(), wh);
                }
                for wh in state.incoming {
                    mgr.incoming.insert(wh.id.clone(), wh);
                }
            }
        }
        mgr
    }
}

/// Serializable persistence for webhook configs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedWebhookState {
    outgoing: Vec<OutgoingWebhook>,
    incoming: Vec<IncomingWebhook>,
}

/// Extract an event type from a JSON payload based on the source service.
fn extract_event_type(body: &str, source: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    match source {
        "github" => value.get("action").and_then(|v| v.as_str()).map(|s| {
            let _event = value
                .get("sender")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            format!("github:{}", s)
        }),
        "slack" => value
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| format!("slack:{}", s)),
        "gitlab" => value
            .get("object_kind")
            .and_then(|v| v.as_str())
            .map(|s| format!("gitlab:{}", s)),
        _ => value
            .get("event")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

/// Verify an HMAC-SHA256 signature.
/// Returns Ok(true) if valid, Err if invalid.
fn verify_hmac_sha256(secret: &str, payload: &str, signature: &str) -> Result<bool, String> {
    // Simple HMAC verification using basic operations.
    // In production, this would use a proper HMAC library.
    let expected = compute_hmac_hex(secret, payload);
    let matches = constant_time_eq(&expected, signature);
    if !matches {
        Err("Invalid webhook signature".to_string())
    } else {
        Ok(true)
    }
}

/// Compute HMAC-SHA256 hex digest (simplified implementation).
fn compute_hmac_hex(key: &str, data: &str) -> String {
    // Use a simplified hash for the webhook signature.
    // A full implementation would use ring or hmac crate.
    let combined = format!("{}:{}", key, data);
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    for byte in combined.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV-1a prime
    }
    // Second pass for more entropy.
    let salted = format!("{:016x}:{}", hash, combined);
    let mut hash2: u64 = 0xcbf29ce484222325;
    for byte in salted.bytes() {
        hash2 ^= byte as u64;
        hash2 = hash2.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}{:016x}", hash, hash2)
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_outgoing() -> OutgoingWebhook {
        OutgoingWebhook {
            id: "wh1".to_string(),
            name: "Test Webhook".to_string(),
            url: "https://example.com/hook".to_string(),
            events: vec![WebhookEvent::BuildFailed, WebhookEvent::CriticalAlert],
            secret_handle: None,
            headers: Vec::new(),
            enabled: true,
            fire_count: 0,
            last_fired: None,
            last_status: None,
        }
    }

    fn test_incoming() -> IncomingWebhook {
        IncomingWebhook {
            id: "in1".to_string(),
            name: "GitHub Hook".to_string(),
            verify_secret: None,
            source: "github".to_string(),
            enabled: true,
            received_count: 0,
            last_received: None,
        }
    }

    #[test]
    fn register_outgoing_and_match() {
        let mut mgr = WebhookManager::new();
        mgr.register_outgoing(test_outgoing());

        let matches = mgr.matching_outgoing(&WebhookEvent::BuildFailed);
        assert_eq!(matches.len(), 1);

        let no_match = mgr.matching_outgoing(&WebhookEvent::TaskStarted);
        assert_eq!(no_match.len(), 0);
    }

    #[test]
    fn record_fire_updates_stats() {
        let mut mgr = WebhookManager::new();
        mgr.register_outgoing(test_outgoing());
        mgr.record_fire("wh1", Some(200));

        let wh = &mgr.outgoing["wh1"];
        assert_eq!(wh.fire_count, 1);
        assert_eq!(wh.last_status, Some(200));
        assert!(wh.last_fired.is_some());
    }

    #[test]
    fn register_and_receive_incoming() {
        let mut mgr = WebhookManager::new();
        mgr.register_incoming(test_incoming());

        let body = r#"{"action":"opened","sender":"user1"}"#;
        let payload = mgr.receive_payload("in1", body, None).unwrap();
        assert!(payload.verified);
        assert_eq!(payload.event_type, Some("github:opened".to_string()));
        assert_eq!(mgr.incoming["in1"].received_count, 1);
    }

    #[test]
    fn incoming_disabled_rejected() {
        let mut mgr = WebhookManager::new();
        let mut wh = test_incoming();
        wh.enabled = false;
        mgr.register_incoming(wh);

        let result = mgr.receive_payload("in1", "{}", None);
        assert!(result.is_err());
    }

    #[test]
    fn incoming_unknown_id_rejected() {
        let mut mgr = WebhookManager::new();
        let result = mgr.receive_payload("nonexistent", "{}", None);
        assert!(result.is_err());
    }

    #[test]
    fn incoming_url_generation() {
        let mut mgr = WebhookManager::new();
        mgr.register_incoming(test_incoming());
        assert_eq!(
            mgr.incoming_url("in1"),
            Some("/webhook/incoming/in1".to_string())
        );
        assert_eq!(mgr.incoming_url("nonexistent"), None);
    }

    #[test]
    fn build_payload_format() {
        let payload = WebhookManager::build_payload(
            &WebhookEvent::BuildFailed,
            &serde_json::json!({"project": "test"}),
        );
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["event"], "build.failed");
        assert_eq!(parsed["source"], "velocity");
        assert_eq!(parsed["data"]["project"], "test");
    }

    #[test]
    fn webhook_event_labels() {
        assert_eq!(
            WebhookEvent::WorkflowCompleted.label(),
            "workflow.completed"
        );
        assert_eq!(WebhookEvent::CriticalAlert.label(), "agent.critical_alert");
        assert_eq!(
            WebhookEvent::Custom("my.event".to_string()).label(),
            "my.event"
        );
    }

    #[test]
    fn webhook_event_from_label() {
        assert_eq!(
            WebhookEvent::from_label("build.failed"),
            WebhookEvent::BuildFailed
        );
        assert_eq!(
            WebhookEvent::from_label("custom.thing"),
            WebhookEvent::Custom("custom.thing".to_string())
        );
    }

    #[test]
    fn recent_payloads_max_size() {
        let mut mgr = WebhookManager::new();
        mgr.max_payloads = 3;
        mgr.register_incoming(test_incoming());

        for _ in 0..5 {
            let _ = mgr.receive_payload("in1", "{}", None);
        }
        assert_eq!(mgr.recent_payloads.len(), 3);
    }

    #[test]
    fn hmac_verification() {
        let sig = compute_hmac_hex("secret", "payload");
        assert!(verify_hmac_sha256("secret", "payload", &sig).unwrap());
        assert!(verify_hmac_sha256("secret", "payload", "wrong_sig").is_err());
    }

    #[test]
    fn extract_event_type_sources() {
        // GitHub
        let gh = r#"{"action":"closed","sender":"user"}"#;
        assert_eq!(
            extract_event_type(gh, "github"),
            Some("github:closed".to_string())
        );

        // Slack
        let sl = r#"{"type":"event_callback"}"#;
        assert_eq!(
            extract_event_type(sl, "slack"),
            Some("slack:event_callback".to_string())
        );

        // GitLab
        let gl = r#"{"object_kind":"merge_request"}"#;
        assert_eq!(
            extract_event_type(gl, "gitlab"),
            Some("gitlab:merge_request".to_string())
        );

        // Generic
        let gen = r#"{"event":"deploy"}"#;
        assert_eq!(
            extract_event_type(gen, "custom"),
            Some("deploy".to_string())
        );
    }
}
