use velocity_browser::engine::SvgVectorEngine;
use velocity_browser::net::WebRtcTransport;
use velocity_browser::session::BrowserSession;
use velocity_browser::session_indexeddb::IndexedDbStorage;

#[test]
fn test_svg_path_parsing_and_bounds() {
    let d = "M 10 20 L 50 80 Z";
    let cmds = SvgVectorEngine::parse_path_d(d);
    assert_eq!(cmds.len(), 3);
    let (x, y, w, h) = SvgVectorEngine::compute_vector_bounds(&cmds);
    assert_eq!(x, 10.0);
    assert_eq!(y, 20.0);
    assert_eq!(w, 40.0);
    assert_eq!(h, 60.0);
}

#[test]
fn test_webrtc_signaling_and_indexeddb() {
    let mut rtc = WebRtcTransport::new("peer_123");
    let answer = rtc.set_remote_offer("v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\n");
    assert!(answer.contains("sendrecv"));

    let mut session = BrowserSession::new("sess_indexeddb".to_string());
    session.indexed_db.put_item("user_store", "user_1", "{\"name\":\"Alice\"}");
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 160)); // IndexedDB predicate
}
