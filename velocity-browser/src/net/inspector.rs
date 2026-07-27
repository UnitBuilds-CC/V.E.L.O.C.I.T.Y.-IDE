use crate::nda::NdaTriple;
use std::collections::HashMap;

/// Captured network request for inspection.
#[derive(Debug, Clone)]
pub struct InspectedRequest {
    pub request_id: String,
    pub url: String,
    pub method: String,
    pub status_code: u16,
    pub status_text: String,
    pub request_headers: HashMap<String, String>,
    pub response_headers: HashMap<String, String>,
    pub request_body_size: usize,
    pub response_body_size: usize,
    pub timing: RequestTiming,
    pub resource_type: ResourceType,
    pub is_cached: bool,
}

/// Timing breakdown for a network request.
#[derive(Debug, Clone, Default)]
pub struct RequestTiming {
    pub dns_ms: f64,
    pub connect_ms: f64,
    pub ssl_ms: f64,
    pub send_ms: f64,
    pub wait_ms: f64,
    pub receive_ms: f64,
    pub total_ms: f64,
}

/// Type of network resource.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceType {
    Document,
    Stylesheet,
    Image,
    Script,
    Xhr,
    Fetch,
    Font,
    Media,
    WebSocket,
    Other,
}

/// Network inspector server for DevTools-like inspection.
pub struct InspectorServer {
    pub port: u16,
    pub is_listening: bool,
    /// Captured requests indexed by request ID.
    pub captured_requests: HashMap<String, InspectedRequest>,
    /// Whether recording is active.
    pub recording: bool,
    next_request_id: u64,
}

impl InspectorServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            is_listening: true,
            captured_requests: HashMap::new(),
            recording: true,
            next_request_id: 1,
        }
    }

    /// Record a completed network request.
    pub fn record_request(&mut self, request: InspectedRequest) -> String {
        let id = request.request_id.clone();
        self.captured_requests.insert(id.clone(), request);
        id
    }

    /// Create and record a request from basic info.
    pub fn capture_request(
        &mut self,
        url: &str,
        method: &str,
        status: u16,
        resource_type: ResourceType,
    ) -> String {
        let id = format!("req_{}", self.next_request_id);
        self.next_request_id += 1;

        let request = InspectedRequest {
            request_id: id.clone(),
            url: url.to_string(),
            method: method.to_string(),
            status_code: status,
            status_text: Self::status_text(status),
            request_headers: HashMap::new(),
            response_headers: HashMap::new(),
            request_body_size: 0,
            response_body_size: 0,
            timing: RequestTiming::default(),
            resource_type,
            is_cached: false,
        };

        if self.recording {
            self.captured_requests.insert(id.clone(), request);
        }
        id
    }

    /// Handle agent inspection and return NDA triples.
    pub fn handle_agent_inspection(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        triples.push(NdaTriple::new(session_id, 200, &format!("inspector_port:{}", self.port)));
        triples.push(NdaTriple::new(session_id, 201, if self.is_listening { "devtools_attached" } else { "devtools_detached" }));
        triples.push(NdaTriple::new(session_id, 202, &format!("captured_requests:{}", self.captured_requests.len())));

        for (id, req) in &self.captured_requests {
            triples.push(NdaTriple::new(session_id, 203, &format!("{}:{}:{}:{}", id, req.method, req.url, req.status_code)));
        }

        triples
    }

    /// Get all captured requests.
    pub fn get_all_requests(&self) -> Vec<&InspectedRequest> {
        self.captured_requests.values().collect()
    }

    /// Filter requests by resource type.
    pub fn filter_by_type(&self, resource_type: &ResourceType) -> Vec<&InspectedRequest> {
        self.captured_requests.values()
            .filter(|r| &r.resource_type == resource_type)
            .collect()
    }

    /// Clear all captured requests.
    pub fn clear(&mut self) {
        self.captured_requests.clear();
    }

    /// Toggle recording on/off.
    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }

    /// Export captured requests as HAR-like JSON.
    pub fn export_har(&self) -> String {
        let mut entries = Vec::new();
        for req in self.captured_requests.values() {
            entries.push(format!(
                r#"{{"request":{{"method":"{}","url":"{}"}},"response":{{"status":{},"statusText":"{}"}},"time":{}}}"#,
                req.method, req.url, req.status_code, req.status_text, req.timing.total_ms
            ));
        }
        format!(r#"{{"log":{{"version":"1.2","entries":[{}]}}}}"#, entries.join(","))
    }

    /// Get standard HTTP status text.
    fn status_text(code: u16) -> String {
        match code {
            200 => "OK", 201 => "Created", 204 => "No Content",
            301 => "Moved Permanently", 302 => "Found", 304 => "Not Modified",
            400 => "Bad Request", 401 => "Unauthorized", 403 => "Forbidden",
            404 => "Not Found", 405 => "Method Not Allowed", 408 => "Request Timeout",
            500 => "Internal Server Error", 502 => "Bad Gateway", 503 => "Service Unavailable",
            _ => "Unknown",
        }.to_string()
    }

    /// Detect resource type from URL and content-type.
    pub fn detect_resource_type(url: &str, content_type: Option<&str>) -> ResourceType {
        if let Some(ct) = content_type {
            if ct.contains("text/html") { return ResourceType::Document; }
            if ct.contains("text/css") { return ResourceType::Stylesheet; }
            if ct.contains("image/") { return ResourceType::Image; }
            if ct.contains("javascript") || ct.contains("application/json") { return ResourceType::Script; }
            if ct.contains("font/") || ct.contains("application/font") { return ResourceType::Font; }
            if ct.contains("audio/") || ct.contains("video/") { return ResourceType::Media; }
        }
        let lower = url.to_lowercase();
        if lower.ends_with(".html") || lower.ends_with(".htm") { return ResourceType::Document; }
        if lower.ends_with(".css") { return ResourceType::Stylesheet; }
        if lower.ends_with(".js") { return ResourceType::Script; }
        if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".gif") || lower.ends_with(".svg") { return ResourceType::Image; }
        if lower.ends_with(".woff") || lower.ends_with(".woff2") || lower.ends_with(".ttf") { return ResourceType::Font; }
        ResourceType::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_inspector() {
        let inspector = InspectorServer::new(9222);
        assert_eq!(inspector.port, 9222);
        assert!(inspector.is_listening);
        assert!(inspector.recording);
    }

    #[test]
    fn test_capture_request() {
        let mut inspector = InspectorServer::new(9222);
        let id = inspector.capture_request("https://example.com", "GET", 200, ResourceType::Document);
        assert!(!id.is_empty());
        assert_eq!(inspector.captured_requests.len(), 1);
    }

    #[test]
    fn test_filter_by_type() {
        let mut inspector = InspectorServer::new(9222);
        inspector.capture_request("https://example.com/style.css", "GET", 200, ResourceType::Stylesheet);
        inspector.capture_request("https://example.com/app.js", "GET", 200, ResourceType::Script);
        inspector.capture_request("https://example.com/logo.png", "GET", 200, ResourceType::Image);

        let scripts = inspector.filter_by_type(&ResourceType::Script);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].url, "https://example.com/app.js");
    }

    #[test]
    fn test_clear() {
        let mut inspector = InspectorServer::new(9222);
        inspector.capture_request("https://example.com", "GET", 200, ResourceType::Document);
        inspector.clear();
        assert_eq!(inspector.captured_requests.len(), 0);
    }

    #[test]
    fn test_recording_toggle() {
        let mut inspector = InspectorServer::new(9222);
        inspector.set_recording(false);
        inspector.capture_request("https://example.com", "GET", 200, ResourceType::Document);
        assert_eq!(inspector.captured_requests.len(), 0); // not recorded
    }

    #[test]
    fn test_export_har() {
        let mut inspector = InspectorServer::new(9222);
        inspector.capture_request("https://example.com", "GET", 200, ResourceType::Document);
        let har = inspector.export_har();
        assert!(har.contains("\"log\""));
        assert!(har.contains("https://example.com"));
    }

    #[test]
    fn test_agent_inspection_nda() {
        let mut inspector = InspectorServer::new(9222);
        inspector.capture_request("https://example.com", "GET", 200, ResourceType::Document);
        let triples = inspector.handle_agent_inspection("sess_1");
        assert!(triples.len() >= 3); // port + status + count + request
    }

    #[test]
    fn test_detect_resource_type() {
        assert_eq!(InspectorServer::detect_resource_type("style.css", None), ResourceType::Stylesheet);
        assert_eq!(InspectorServer::detect_resource_type("app.js", None), ResourceType::Script);
        assert_eq!(InspectorServer::detect_resource_type("logo.png", None), ResourceType::Image);
        assert_eq!(InspectorServer::detect_resource_type("page.html", None), ResourceType::Document);
        assert_eq!(InspectorServer::detect_resource_type("api", Some("text/html")), ResourceType::Document);
        assert_eq!(InspectorServer::detect_resource_type("api", Some("application/font-woff")), ResourceType::Font);
    }

    #[test]
    fn test_status_text() {
        assert_eq!(InspectorServer::status_text(200), "OK");
        assert_eq!(InspectorServer::status_text(404), "Not Found");
        assert_eq!(InspectorServer::status_text(999), "Unknown");
    }
}
