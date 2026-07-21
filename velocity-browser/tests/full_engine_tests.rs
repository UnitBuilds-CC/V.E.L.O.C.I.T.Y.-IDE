use velocity_browser::agentic::VelocityOcrEngine;
use velocity_browser::engine::PixelBuffer;
use velocity_browser::session::BrowserSession;

#[test]
fn test_velocity_ocr_engine_processing() {
    let engine = VelocityOcrEngine::new();
    let mut buffer = PixelBuffer::new(100, 100);
    buffer.set_pixel(10, 10, 0, 0, 0, 255); // Dark pixel

    let boxes = engine.process_pixel_buffer(&buffer);
    assert!(!boxes.is_empty());
    assert!(boxes[0].confidence > 0.9);

    let session = BrowserSession::new("sess_ocr".to_string());
    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 252)); // OCR triple predicate
}
