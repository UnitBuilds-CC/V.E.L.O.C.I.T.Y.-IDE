use velocity_browser::engine::{SandboxCapabilities, TabSandbox, WebCryptoEngine, WebGLContext};
use velocity_browser::session::BrowserSession;
use velocity_browser::session_history::HistoryStack;
use velocity_browser::style::ScopedCssMatcher;
use velocity_browser::parser::html::DomNode;
use std::collections::HashMap;

#[test]
fn test_webgl_context_and_history_stack() {
    let mut gl = WebGLContext::new(100, 100);
    gl.buffer_data(&[10.0, 20.0, 50.0, 80.0]);
    gl.draw_arrays_triangles(255, 0, 0, 255);
    assert_ne!(gl.pixel_buffer.compute_hash(), 0);

    let mut hist = HistoryStack::new("http://example.com");
    hist.push_state("http://example.com/page2", "{}", "Page 2");
    assert_eq!(hist.current_index, 1);
    assert_eq!(hist.back().unwrap().url, "http://example.com");
}

#[test]
fn test_tab_sandbox_security_isolation() {
    let mut caps = SandboxCapabilities::strict_isolation();
    caps.allow_network_hosts.push("trusted.com".to_string());

    let mut sandbox = TabSandbox::new("tab_1", caps);
    assert!(sandbox.check_network_access("https://trusted.com/api").is_ok());
    assert!(sandbox.check_network_access("https://malicious.com/payload").is_err());
    assert!(sandbox.check_file_access("/etc/passwd").is_err());

    let mut session = BrowserSession::new("sess_sandbox".to_string());
    session.tab_sandbox = sandbox;
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 220)); // Sandbox violation predicate
}

#[test]
fn test_webcrypto_and_scoped_css() {
    let digest = WebCryptoEngine::digest_sha256(b"hello world");
    assert_eq!(digest.len(), 16);

    let mut attrs = HashMap::new();
    attrs.insert("shadowroot".to_string(), "open".to_string());
    let host_node = DomNode { id: 1, tag_name: "div".to_string(), attributes: attrs, children: Vec::new(), parent: None, node_type: velocity_browser::parser::html::NodeType::Element, text_content: String::new() };

    assert!(ScopedCssMatcher::matches_host_selector(&host_node, ":host"));
}
