use crate::nda::NdaTriple;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub resource_type: String,
    pub request_headers: HashMap<String, String>,
    pub response_headers: HashMap<String, String>,
    pub body_size: usize,
    pub duration_ms: f64,
    pub redirect_chain: Vec<String>,
}

/// Intercept action to apply to matching requests.
#[derive(Debug, Clone)]
pub enum InterceptAction {
    /// Block the request entirely.
    Block,
    /// Modify request headers before sending.
    ModifyHeaders(HashMap<String, String>),
    /// Redirect to a different URL.
    Redirect(String),
    /// Allow through unmodified.
    Allow,
}

/// Rule for intercepting network requests.
#[derive(Debug, Clone)]
pub struct InterceptRule {
    pub url_pattern: String,
    pub resource_types: Vec<String>,
    pub action: InterceptAction,
}

pub struct NetworkTracker {
    pub requests: Vec<NetworkRequest>,
    pub headers: HashMap<String, String>,
    pub redirects: Vec<String>,
    pub downloads: Vec<String>,
    pub intercept_rules: Vec<InterceptRule>,
    pub recording: bool,
    pub blocked_count: usize,
    pub resource_filter: Option<Vec<String>>,
}

impl Default for NetworkTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkTracker {
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
            headers: HashMap::new(),
            redirects: Vec::new(),
            downloads: Vec::new(),
            intercept_rules: Vec::new(),
            recording: true,
            blocked_count: 0,
            resource_filter: None,
        }
    }

    pub fn record_request(&mut self, url: &str, method: &str, status: u16, resource_type: &str) {
        if !self.recording { return; }
        if let Some(ref filter) = self.resource_filter {
            if !filter.iter().any(|t| t == resource_type) {
                return;
            }
        }
        self.requests.push(NetworkRequest {
            url: url.to_string(),
            method: method.to_string(),
            status,
            resource_type: resource_type.to_string(),
            request_headers: HashMap::new(),
            response_headers: HashMap::new(),
            body_size: 0,
            duration_ms: 0.0,
            redirect_chain: Vec::new(),
        });
    }

    /// Record a request with full details.
    pub fn record_full_request(
        &mut self,
        url: &str,
        method: &str,
        status: u16,
        resource_type: &str,
        req_headers: HashMap<String, String>,
        resp_headers: HashMap<String, String>,
        body_size: usize,
        duration_ms: f64,
    ) {
        if !self.recording { return; }
        self.requests.push(NetworkRequest {
            url: url.to_string(),
            method: method.to_string(),
            status,
            resource_type: resource_type.to_string(),
            request_headers: req_headers,
            response_headers: resp_headers,
            body_size,
            duration_ms,
            redirect_chain: Vec::new(),
        });
    }

    /// Add a redirect to the tracking chain.
    pub fn record_redirect(&mut self, from_url: &str, to_url: &str) {
        self.redirects.push(from_url.to_string());
        self.redirects.push(to_url.to_string());
        // Attach to the last request if it matches
        if let Some(last) = self.requests.last_mut() {
            if last.url == from_url || last.url == to_url {
                last.redirect_chain.push(from_url.to_string());
                last.redirect_chain.push(to_url.to_string());
            }
        }
    }

    /// Add an intercept rule.
    pub fn add_intercept_rule(&mut self, url_pattern: &str, resource_types: Vec<&str>, action: InterceptAction) {
        self.intercept_rules.push(InterceptRule {
            url_pattern: url_pattern.to_string(),
            resource_types: resource_types.into_iter().map(|s| s.to_string()).collect(),
            action,
        });
    }

    /// Check intercept rules for a request. Returns the action to apply.
    pub fn check_intercept(&self, url: &str, resource_type: &str) -> InterceptAction {
        for rule in &self.intercept_rules {
            let url_matches = if rule.url_pattern.contains('*') {
                let prefix = rule.url_pattern.trim_end_matches('*');
                url.starts_with(prefix)
            } else {
                url.contains(&rule.url_pattern)
            };
            if !url_matches { continue; }

            let type_matches = rule.resource_types.is_empty()
                || rule.resource_types.iter().any(|t| t == resource_type);
            if type_matches {
                return rule.action.clone();
            }
        }
        InterceptAction::Allow
    }

    /// Set resource type filter (e.g., ["document", "xhr", "fetch"]).
    pub fn set_resource_filter(&mut self, types: Vec<&str>) {
        self.resource_filter = Some(types.into_iter().map(|s| s.to_string()).collect());
    }

    /// Clear the resource filter.
    pub fn clear_resource_filter(&mut self) {
        self.resource_filter = None;
    }

    /// Get requests filtered by resource type.
    pub fn requests_by_type(&self, resource_type: &str) -> Vec<&NetworkRequest> {
        self.requests.iter().filter(|r| r.resource_type == resource_type).collect()
    }

    /// Get requests filtered by URL pattern (substring match).
    pub fn requests_by_url(&self, pattern: &str) -> Vec<&NetworkRequest> {
        self.requests.iter().filter(|r| r.url.contains(pattern)).collect()
    }

    /// Get failed requests (4xx/5xx status).
    pub fn failed_requests(&self) -> Vec<&NetworkRequest> {
        self.requests.iter().filter(|r| r.status >= 400).collect()
    }

    /// Start/stop recording.
    pub fn set_recording(&mut self, enabled: bool) {
        self.recording = enabled;
    }

    /// Clear all recorded requests.
    pub fn clear(&mut self) {
        self.requests.clear();
        self.redirects.clear();
        self.downloads.clear();
        self.blocked_count = 0;
    }

    /// Get summary statistics.
    pub fn stats(&self) -> NetworkStats {
        let total = self.requests.len();
        let failed = self.requests.iter().filter(|r| r.status >= 400).count();
        let by_type: HashMap<String, usize> = self.requests.iter()
            .fold(HashMap::new(), |mut acc, r| {
                *acc.entry(r.resource_type.clone()).or_insert(0) += 1;
                acc
            });
        NetworkStats {
            total_requests: total,
            failed_requests: failed,
            blocked_requests: self.blocked_count,
            redirect_count: self.redirects.len() / 2,
            by_resource_type: by_type,
        }
    }

    pub fn export_triples_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(self.requests.len() * 2);
        for req in &self.requests {
            triples.push(NdaTriple::new(&req.url, 200, &req.method));
            triples.push(NdaTriple::new(&req.url, 201, &req.status.to_string()));
        }
        triples
    }
}

/// Summary statistics for network activity.
#[derive(Debug, Clone)]
pub struct NetworkStats {
    pub total_requests: usize,
    pub failed_requests: usize,
    pub blocked_requests: usize,
    pub redirect_count: usize,
    pub by_resource_type: HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_filter() {
        let mut tracker = NetworkTracker::new();
        tracker.record_request("https://example.com/api", "GET", 200, "xhr");
        tracker.record_request("https://example.com/style.css", "GET", 200, "stylesheet");
        tracker.record_request("https://example.com/missing", "GET", 404, "document");

        assert_eq!(tracker.requests_by_type("xhr").len(), 1);
        assert_eq!(tracker.failed_requests().len(), 1);
        assert_eq!(tracker.requests_by_url("api").len(), 1);
    }

    #[test]
    fn test_intercept_rules() {
        let mut tracker = NetworkTracker::new();
        tracker.add_intercept_rule("ads.example.com", vec![], InterceptAction::Block);
        tracker.add_intercept_rule("cdn.*", vec!["script"], InterceptAction::Allow);

        match tracker.check_intercept("https://ads.example.com/banner", "image") {
            InterceptAction::Block => {}
            _ => panic!("Expected Block"),
        }
        match tracker.check_intercept("https://other.com/page", "document") {
            InterceptAction::Allow => {}
            _ => panic!("Expected Allow"),
        }
    }

    #[test]
    fn test_resource_filter() {
        let mut tracker = NetworkTracker::new();
        tracker.set_resource_filter(vec!["xhr", "fetch"]);
        tracker.record_request("https://api.com/data", "GET", 200, "xhr");
        tracker.record_request("https://img.com/pic.png", "GET", 200, "image");

        assert_eq!(tracker.requests.len(), 1); // image filtered out
        tracker.clear_resource_filter();
        tracker.record_request("https://img.com/pic2.png", "GET", 200, "image");
        assert_eq!(tracker.requests.len(), 2);
    }

    #[test]
    fn test_redirect_tracking() {
        let mut tracker = NetworkTracker::new();
        tracker.record_request("https://old.com/page", "GET", 301, "document");
        tracker.record_redirect("https://old.com/page", "https://new.com/page");
        assert_eq!(tracker.redirects.len(), 2);
    }

    #[test]
    fn test_stats() {
        let mut tracker = NetworkTracker::new();
        tracker.record_request("https://a.com", "GET", 200, "document");
        tracker.record_request("https://b.com", "GET", 500, "xhr");
        let stats = tracker.stats();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.failed_requests, 1);
    }

    #[test]
    fn test_recording_toggle() {
        let mut tracker = NetworkTracker::new();
        tracker.set_recording(false);
        tracker.record_request("https://a.com", "GET", 200, "document");
        assert_eq!(tracker.requests.len(), 0);
        tracker.set_recording(true);
        tracker.record_request("https://b.com", "GET", 200, "document");
        assert_eq!(tracker.requests.len(), 1);
    }

    #[test]
    fn record_full_request_stores_all_fields() {
        let mut tracker = NetworkTracker::new();
        let mut req_h = HashMap::new();
        req_h.insert("Accept".into(), "text/html".into());
        let mut resp_h = HashMap::new();
        resp_h.insert("Content-Type".into(), "text/html".into());
        tracker.record_full_request(
            "https://example.com/page", "GET", 200, "document",
            req_h.clone(), resp_h.clone(), 4096, 123.45,
        );
        let req = &tracker.requests[0];
        assert_eq!(req.url, "https://example.com/page");
        assert_eq!(req.method, "GET");
        assert_eq!(req.status, 200);
        assert_eq!(req.resource_type, "document");
        assert_eq!(req.request_headers.get("Accept").map(|s| s.as_str()), Some("text/html"));
        assert_eq!(req.response_headers.get("Content-Type").map(|s| s.as_str()), Some("text/html"));
        assert_eq!(req.body_size, 4096);
        assert!((req.duration_ms - 123.45).abs() < 0.01);
    }

    #[test]
    fn record_full_request_respects_recording_flag() {
        let mut tracker = NetworkTracker::new();
        tracker.set_recording(false);
        tracker.record_full_request(
            "https://x.com", "POST", 201, "xhr",
            HashMap::new(), HashMap::new(), 0, 0.0,
        );
        assert!(tracker.requests.is_empty());
    }

    #[test]
    fn export_triples_nda_contains_method_and_status() {
        let mut tracker = NetworkTracker::new();
        tracker.record_request("https://api.com/data", "POST", 201, "xhr");
        let triples = tracker.export_triples_nda();
        assert_eq!(triples.len(), 2); // one for method (200), one for status (201)
        // Predicate 200 = method, 201 = status
        assert_eq!(triples[0].predicate_id, 200);
        assert_eq!(triples[1].predicate_id, 201);
    }

    #[test]
    fn stats_by_resource_type_breakdown() {
        let mut tracker = NetworkTracker::new();
        tracker.record_request("https://a.com/1", "GET", 200, "document");
        tracker.record_request("https://a.com/2", "GET", 200, "document");
        tracker.record_request("https://a.com/s.css", "GET", 200, "stylesheet");
        tracker.record_request("https://a.com/a.js", "GET", 200, "script");
        let stats = tracker.stats();
        assert_eq!(stats.by_resource_type.get("document"), Some(&2));
        assert_eq!(stats.by_resource_type.get("stylesheet"), Some(&1));
        assert_eq!(stats.by_resource_type.get("script"), Some(&1));
    }

    #[test]
    fn stats_blocked_count_reflects_field() {
        let mut tracker = NetworkTracker::new();
        tracker.blocked_count = 5;
        tracker.record_request("https://a.com", "GET", 200, "document");
        let stats = tracker.stats();
        assert_eq!(stats.blocked_requests, 5);
    }

    #[test]
    fn stats_redirect_count_is_half_redirects_len() {
        let mut tracker = NetworkTracker::new();
        tracker.record_redirect("https://old.com", "https://new.com");
        tracker.record_redirect("https://old2.com", "https://new2.com");
        let stats = tracker.stats();
        assert_eq!(stats.redirect_count, 2); // 4 entries / 2
    }

    #[test]
    fn intercept_redirect_action() {
        let mut tracker = NetworkTracker::new();
        tracker.add_intercept_rule("old.example.com", vec![], InterceptAction::Redirect("https://new.example.com".into()));
        match tracker.check_intercept("https://old.example.com/page", "document") {
            InterceptAction::Redirect(url) => assert_eq!(url, "https://new.example.com"),
            _ => panic!("Expected Redirect"),
        }
    }

    #[test]
    fn intercept_modify_headers_action() {
        let mut tracker = NetworkTracker::new();
        let mut headers = HashMap::new();
        headers.insert("X-Custom".into(), "value".into());
        tracker.add_intercept_rule("api.example.com", vec!["xhr"], InterceptAction::ModifyHeaders(headers));
        // Should match for xhr
        match tracker.check_intercept("https://api.example.com/data", "xhr") {
            InterceptAction::ModifyHeaders(h) => assert_eq!(h.get("X-Custom").map(|s| s.as_str()), Some("value")),
            _ => panic!("Expected ModifyHeaders"),
        }
        // Should NOT match for document (resource type filter)
        match tracker.check_intercept("https://api.example.com/page", "document") {
            InterceptAction::Allow => {}
            _ => panic!("Expected Allow for non-matching resource type"),
        }
    }

    #[test]
    fn clear_resets_all_state() {
        let mut tracker = NetworkTracker::new();
        tracker.record_request("https://a.com", "GET", 200, "document");
        tracker.record_redirect("https://a.com", "https://b.com");
        tracker.downloads.push("file.zip".into());
        tracker.blocked_count = 3;
        tracker.clear();
        assert!(tracker.requests.is_empty());
        assert!(tracker.redirects.is_empty());
        assert!(tracker.downloads.is_empty());
        assert_eq!(tracker.blocked_count, 0);
    }

    #[test]
    fn default_tracker_is_recording() {
        let tracker = NetworkTracker::default();
        assert!(tracker.recording);
        assert!(tracker.requests.is_empty());
        assert!(tracker.resource_filter.is_none());
    }
}
