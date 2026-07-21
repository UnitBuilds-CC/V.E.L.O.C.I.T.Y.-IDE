use velocity_browser::engine::Canvas2DContext;
use velocity_browser::net::{ProxyResolver, ProxyType};
use velocity_browser::session::BrowserSession;

#[test]
fn test_canvas_2d_context_drawing() {
    let mut ctx = Canvas2DContext::new(100, 100);
    ctx.fill_rect(10, 10, 50, 50, 255, 0, 0, 255); // Red rect
    let hash = ctx.pixel_buffer.compute_hash();
    assert_ne!(hash, 0);
}

#[test]
fn test_proxy_resolver_and_storage_broadcaster() {
    let mut session = BrowserSession::new("sess_specialized".to_string());
    session.proxy_resolver.set_http_proxy("127.0.0.1", 8080);
    let proxy = session.proxy_resolver.resolve_proxy_for_url("http://example.com");

    match proxy {
        ProxyType::Http(host, port) => {
            assert_eq!(host, "127.0.0.1");
            assert_eq!(port, 8080);
        }
        _ => panic!("Expected HTTP proxy"),
    }

    session.set_storage_item("theme", "dark");
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 150)); // Storage event predicate
}
