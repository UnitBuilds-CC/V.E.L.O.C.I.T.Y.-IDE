use velocity_browser::engine::{GpuTileCompositor, VelocityCodecsEngine};
use velocity_browser::net::QuicConnection;
use velocity_browser::session::BrowserSession;
use velocity_browser::style::FontShaperEngine;

#[test]
fn test_quic_webcodecs_font_shaper_gpu_compositor() {
    let mut conn = QuicConnection::connect("127.0.0.1:4433");
    let sid = conn.open_stream();
    assert_eq!(sid, 1);

    let mut codecs = VelocityCodecsEngine::new("h264_opus");
    let frame = codecs.decode_stream_packet(b"\x00\x00\x00\x01\x25nal");
    assert_eq!(frame.width, 1920);
    assert!(frame.is_keyframe);

    let mut shaper = FontShaperEngine::new("Roboto");
    let glyphs = shaper.shape_text("Velocity Engine");
    assert_eq!(glyphs.len(), 15);

    let mut compositor = GpuTileCompositor::new();
    let layer_id = compositor.create_layer(1920, 1080);
    assert_eq!(layer_id, 1);

    let session = BrowserSession::new("sess_codecs".to_string());
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 253));
}
