use velocity_browser::aom::{AomExtractor, SpatialNode};
use velocity_browser::engine::{CanvasElement, CanvasExtractor, FrameTarget, InterstitialClassifier, InterstitialKind, ShadowFrameExtractor, ShadowHost};
use velocity_browser::nda::NdaTriple;
use velocity_browser::session::BrowserSession;

#[test]
fn test_nda_triple_encoding() {
    let triple = NdaTriple::new("http://example.com", 1, "page");
    assert_eq!(triple.predicate_id, 1);

    let packed = triple.to_bytes();
    assert_eq!(packed.len(), 18);
}

#[test]
fn test_interstitial_classifier() {
    let kind = InterstitialClassifier::classify_page("Just a moment...", "<div id=\"challenge\">Cloudflare</div>");
    assert_eq!(kind, InterstitialKind::CloudflareTurnstile);

    let kind_clean = InterstitialClassifier::classify_page("Welcome to App", "<div>Dashboard</div>");
    assert_eq!(kind_clean, InterstitialKind::None);
}

#[test]
fn test_aom_extractor() {
    let nodes = vec![
        SpatialNode {
            id: "node_1".to_string(),
            role: "button".to_string(),
            name: "Submit".to_string(),
        },
    ];

    let triples = AomExtractor::extract_triples(&nodes);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].predicate_id, 10);
}

#[test]
fn test_shadow_and_canvas_extractors() {
    let shadow_hosts = vec![ShadowHost {
        host_id: "host1".to_string(),
        mode: "open".to_string(),
        shadow_root_id: "shadow_1".to_string(),
    }];
    let frames = vec![FrameTarget {
        frame_id: "f1".to_string(),
        parent_id: None,
        url: "https://frame.com".to_string(),
        security_origin: "https://frame.com".to_string(),
    }];
    let canvases = vec![CanvasElement {
        id: "c1".to_string(),
        context_type: "webgl".to_string(),
        width: 800,
        height: 600,
        draw_call_count: 42,
    }];

    let shadow_triples = ShadowFrameExtractor::extract_shadow_hosts_nda(&shadow_hosts);
    let frame_triples = ShadowFrameExtractor::extract_frames_nda(&frames);
    let canvas_triples = CanvasExtractor::extract_canvases_nda(&canvases);

    assert_eq!(shadow_triples.len(), 2);
    assert_eq!(frame_triples.len(), 1);
    assert_eq!(canvas_triples.len(), 3);
}

#[test]
fn test_browser_session_nda_state() {
    let mut session = BrowserSession::new("session_test_123".to_string());
    session.current_url = "https://unitbuilds.com".to_string();
    session.network_tracker.record_request("https://unitbuilds.com/api", "POST", 200, "xhr");

    let triples = session.capture_state_nda();
    assert!(!triples.is_empty());
}
