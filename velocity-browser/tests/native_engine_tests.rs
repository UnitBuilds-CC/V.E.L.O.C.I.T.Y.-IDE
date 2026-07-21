use velocity_browser::engine::{DeviceProfile, FileManager, TraceCollector};
use velocity_browser::parser::HtmlParser;
use velocity_browser::session::BrowserSession;

#[test]
fn test_device_profile_export() {
    let profile = DeviceProfile::desktop_chrome();
    assert_eq!(profile.viewport_width, 1920);
    assert_eq!(profile.viewport_height, 1080);

    let triples = profile.export_profile_nda("sess_device");
    assert!(triples.iter().any(|t| t.predicate_id == 110));
}

#[test]
fn test_file_attachment_and_trace_collector() {
    let mut session = BrowserSession::new("sess_file_trace".to_string());
    let html = r#"
        <html>
            <body>
                <input id="file-upload" type="file" name="doc" />
            </body>
        </html>
    "#;

    session.load_html("http://localhost/upload", html);

    let attach_res = session.attach_file("file-upload", "/tmp/document.pdf");
    assert!(attach_res.is_ok());

    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 90)); // Attached file predicate
    assert!(state.iter().any(|t| t.predicate_id == 121)); // DOM mutation trace predicate
}
