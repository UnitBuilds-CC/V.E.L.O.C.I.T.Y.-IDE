use velocity_browser::agentic::OcrSpatialMapper;
use velocity_browser::engine::{CaptchaSolverEngine, CaptchaType, PdfMediaExtractor, PixelBuffer};
use velocity_browser::net::TlsFingerprintRotator;
use velocity_browser::parser::HtmlParser;
use velocity_browser::dom::DomTree;

#[test]
fn test_tls_rotator_and_captcha_solver() {
    let mut rot = TlsFingerprintRotator::chrome_desktop();
    let profile = rot.rotate_profile();
    assert!(profile.ja3_hash.contains("rotated_ja3"));

    let html = r#"<html><body><iframe src="https://hcaptcha.com/1/api.js"></iframe></body></html>"#;
    let nodes = HtmlParser::parse(html);
    let tree = DomTree::new(nodes);
    let detected = CaptchaSolverEngine::detect_challenge(&tree);
    assert!(matches!(detected, Some(CaptchaType::HCaptcha)));
}

#[test]
fn test_pdf_extractor_and_ocr_mapper() {
    let pdf_lines = PdfMediaExtractor::parse_pdf_document(b"%PDF-1.4 sample content");
    assert_eq!(pdf_lines.len(), 1);

    let pix = PixelBuffer::new(100, 100);
    let ocr_boxes = OcrSpatialMapper::map_pixel_buffer_ocr(&pix);
    assert_eq!(ocr_boxes.len(), 1);
}
