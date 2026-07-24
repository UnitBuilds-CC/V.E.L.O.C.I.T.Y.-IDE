use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh_key: String,
    pub auth_secret: String,
}

pub struct PushNotificationManager {
    pub subscriptions: Vec<PushSubscription>,
}

impl Default for PushNotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PushNotificationManager {
    pub fn new() -> Self {
        Self { subscriptions: Vec::new() }
    }

    pub fn subscribe(&mut self, endpoint: &str, p256dh: &str, auth: &str) -> PushSubscription {
        let sub = PushSubscription {
            endpoint: endpoint.to_string(),
            p256dh_key: p256dh.to_string(),
            auth_secret: auth.to_string(),
        };
        self.subscriptions.push(sub.clone());
        sub
    }

    pub fn export_push_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for sub in &self.subscriptions {
            triples.push(NdaTriple::new(session_id, 230, &sub.endpoint));
        }
        triples
    }
}
