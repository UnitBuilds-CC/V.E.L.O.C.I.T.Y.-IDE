use velocity_browser::engine::{GpuTileCompositor, WebCodecsDecoder};
use velocity_browser::net::QuicConnection;
use velocity_browser::style::FontShaperEngine;

#[test]
fn test_quic_webcodecs_font_shaper_gpu_compositor() {
    let mut conn = QuicConnection::connect("127.0.0.1:4433");
    let sid = conn.open_stream();
    assert_eq!(sid, 1);

    let mut decoder = WebCodecsDecoder::new("h264");
    let frame = decoder.decode_chunk(b"\x00\x00\x00\x01nal", 1920, 1080);
    assert_eq!(frame.width, 1920);

    let mut shaper = FontShaperEngine::new("Roboto");
    let glyphs = shaper.shape_text("Velocity Engine");
    assert_eq!(glyphs.len(), 15);

    let mut compositor = GpuTileCompositor::new();
    let layer_id = compositor.create_layer(1920, 1080);
    assert_eq!(layer_id, 1);
}
