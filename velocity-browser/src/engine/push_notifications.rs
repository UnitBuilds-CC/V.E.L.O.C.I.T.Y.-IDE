use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh_key: String,
    pub auth_secret: String,
    pub expiration_time: Option<u64>,
    pub active: bool,
}

/// Incoming push event.
#[derive(Debug, Clone)]
pub struct PushEvent {
    pub subscription_endpoint: String,
    pub title: String,
    pub body: String,
    pub data: Vec<u8>,
    pub timestamp_ms: u64,
    pub read: bool,
}

/// Notification displayed to the user.
#[derive(Debug, Clone)]
pub struct NotificationRecord {
    pub id: u32,
    pub title: String,
    pub body: String,
    pub icon_url: Option<String>,
    pub tag: Option<String>,
    pub require_interaction: bool,
    pub timestamp_ms: u64,
    pub dismissed: bool,
}

pub struct PushNotificationManager {
    pub subscriptions: Vec<PushSubscription>,
    pub events: Vec<PushEvent>,
    pub notifications: Vec<NotificationRecord>,
    pub permission_granted: bool,
    next_notification_id: u32,
}

impl Default for PushNotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PushNotificationManager {
    pub fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
            events: Vec::new(),
            notifications: Vec::new(),
            permission_granted: false,
            next_notification_id: 1,
        }
    }

    /// Request notification permission from the user.
    pub fn request_permission(&mut self) -> bool {
        // In a real browser this shows a permission prompt.
        // Here we default to granted for agentic use.
        self.permission_granted = true;
        self.permission_granted
    }

    pub fn subscribe(&mut self, endpoint: &str, p256dh: &str, auth: &str) -> PushSubscription {
        let sub = PushSubscription {
            endpoint: endpoint.to_string(),
            p256dh_key: p256dh.to_string(),
            auth_secret: auth.to_string(),
            expiration_time: None,
            active: true,
        };
        self.subscriptions.push(sub.clone());
        sub
    }

    /// Unsubscribe a push subscription by endpoint.
    pub fn unsubscribe(&mut self, endpoint: &str) -> bool {
        if let Some(sub) = self.subscriptions.iter_mut().find(|s| s.endpoint == endpoint) {
            sub.active = false;
            true
        } else {
            false
        }
    }

    /// Get active subscriptions.
    pub fn active_subscriptions(&self) -> Vec<&PushSubscription> {
        self.subscriptions.iter().filter(|s| s.active).collect()
    }

    /// Dispatch an incoming push event.
    pub fn dispatch_push_event(&mut self, endpoint: &str, title: &str, body: &str, data: &[u8]) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.events.push(PushEvent {
            subscription_endpoint: endpoint.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            data: data.to_vec(),
            timestamp_ms: now,
            read: false,
        });
    }

    /// Show a notification.
    pub fn show_notification(&mut self, title: &str, body: &str, icon: Option<&str>, tag: Option<&str>, require_interaction: bool) -> u32 {
        let id = self.next_notification_id;
        self.next_notification_id += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.notifications.push(NotificationRecord {
            id,
            title: title.to_string(),
            body: body.to_string(),
            icon_url: icon.map(|s| s.to_string()),
            tag: tag.map(|s| s.to_string()),
            require_interaction,
            timestamp_ms: now,
            dismissed: false,
        });
        id
    }

    /// Dismiss a notification by ID.
    pub fn dismiss_notification(&mut self, id: u32) -> bool {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.dismissed = true;
            true
        } else {
            false
        }
    }

    /// Get active (non-dismissed) notifications.
    pub fn active_notifications(&self) -> Vec<&NotificationRecord> {
        self.notifications.iter().filter(|n| !n.dismissed).collect()
    }

    /// Get unread push events.
    pub fn unread_events(&self) -> Vec<&PushEvent> {
        self.events.iter().filter(|e| !e.read).collect()
    }

    /// Mark all events as read.
    pub fn mark_all_read(&mut self) {
        for event in &mut self.events {
            event.read = true;
        }
    }

    /// Decrypt a push message payload using the subscription's auth secret.
    /// Uses HMAC-SHA256 from the crypto engine for authentication.
    pub fn decrypt_push_payload(&self, endpoint: &str, encrypted_data: &[u8]) -> Option<Vec<u8>> {
        let sub = self.subscriptions.iter().find(|s| s.endpoint == endpoint && s.active)?;
        let key = sub.auth_secret.as_bytes();
        // Simple authenticated decryption: verify HMAC then XOR-decrypt
        if encrypted_data.len() < 32 {
            return None;
        }
        let payload = &encrypted_data[..encrypted_data.len() - 32];
        let tag = &encrypted_data[encrypted_data.len() - 32..];
        let computed = crate::engine::crypto::WebCryptoEngine::hmac_sha256(key, payload);
        if computed.iter().zip(tag.iter()).all(|(a, b)| a == b) {
            // XOR decrypt with HMAC keystream
            let mut decrypted = Vec::with_capacity(payload.len());
            let mut counter = 0u32;
            for chunk in payload.chunks(32) {
                let mut block = sub.p256dh_key.as_bytes().to_vec();
                block.extend_from_slice(&counter.to_le_bytes());
                let keystream = crate::engine::crypto::WebCryptoEngine::hmac_sha256(key, &block);
                for (i, &byte) in chunk.iter().enumerate() {
                    decrypted.push(byte ^ keystream[i]);
                }
                counter += 1;
            }
            Some(decrypted)
        } else {
            None
        }
    }

    pub fn export_push_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for sub in self.subscriptions.iter().filter(|s| s.active) {
            triples.push(NdaTriple::new(session_id, 230, &sub.endpoint));
        }
        for event in self.events.iter().filter(|e| !e.read) {
            triples.push(NdaTriple::new(session_id, 231, &event.title));
        }
        triples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_permission() {
        let mut mgr = PushNotificationManager::new();
        assert!(!mgr.permission_granted);
        assert!(mgr.request_permission());
        assert!(mgr.permission_granted);
    }

    #[test]
    fn test_subscribe_and_active() {
        let mut mgr = PushNotificationManager::new();
        mgr.subscribe("https://push.example.com/sub1", "p256dh_key", "auth_secret");
        mgr.subscribe("https://push.example.com/sub2", "p256dh_key2", "auth_secret2");
        assert_eq!(mgr.active_subscriptions().len(), 2);
        mgr.unsubscribe("https://push.example.com/sub1");
        assert_eq!(mgr.active_subscriptions().len(), 1);
    }

    #[test]
    fn test_show_and_dismiss_notification() {
        let mut mgr = PushNotificationManager::new();
        let id1 = mgr.show_notification("Alert", "Something happened", None, Some("alert-tag"), false);
        let id2 = mgr.show_notification("Info", "FYI", Some("/icon.png"), None, true);
        assert_eq!(mgr.active_notifications().len(), 2);
        mgr.dismiss_notification(id1);
        assert_eq!(mgr.active_notifications().len(), 1);
        assert_eq!(mgr.active_notifications()[0].id, id2);
    }

    #[test]
    fn test_dispatch_push_event() {
        let mut mgr = PushNotificationManager::new();
        mgr.dispatch_push_event("https://push.example.com/sub1", "New msg", "Hello", b"data");
        assert_eq!(mgr.unread_events().len(), 1);
        mgr.mark_all_read();
        assert_eq!(mgr.unread_events().len(), 0);
    }

    #[test]
    fn test_dismiss_nonexistent() {
        let mut mgr = PushNotificationManager::new();
        assert!(!mgr.dismiss_notification(999));
    }

    #[test]
    fn test_export_push_nda() {
        let mut mgr = PushNotificationManager::new();
        mgr.subscribe("https://push.example.com/s1", "k", "a");
        mgr.dispatch_push_event("https://push.example.com/s1", "Title", "Body", b"");
        let triples = mgr.export_push_nda("session1");
        assert_eq!(triples.len(), 2); // 1 subscription + 1 unread event
    }

    #[test]
    fn unsubscribe_nonexistent_returns_false() {
        let mut mgr = PushNotificationManager::new();
        assert!(!mgr.unsubscribe("https://nope.com"));
    }

    #[test]
    fn notification_ids_auto_increment() {
        let mut mgr = PushNotificationManager::new();
        let id1 = mgr.show_notification("A", "a", None, None, false);
        let id2 = mgr.show_notification("B", "b", None, None, false);
        let id3 = mgr.show_notification("C", "c", None, None, false);
        assert_eq!(id2, id1 + 1);
        assert_eq!(id3, id2 + 1);
    }

    #[test]
    fn mark_all_read_on_empty_is_noop() {
        let mut mgr = PushNotificationManager::new();
        mgr.mark_all_read();
        assert!(mgr.unread_events().is_empty());
    }

    #[test]
    fn default_manager_has_no_permission() {
        let mgr = PushNotificationManager::default();
        assert!(!mgr.permission_granted);
        assert!(mgr.subscriptions.is_empty());
        assert!(mgr.notifications.is_empty());
    }

    #[test]
    fn export_push_nda_excludes_inactive_subscriptions() {
        let mut mgr = PushNotificationManager::new();
        mgr.subscribe("https://push.example.com/s1", "k", "a");
        mgr.subscribe("https://push.example.com/s2", "k2", "a2");
        mgr.unsubscribe("https://push.example.com/s1");
        let triples = mgr.export_push_nda("sess");
        // Only s2 is active
        assert_eq!(triples.len(), 1);
    }
}
