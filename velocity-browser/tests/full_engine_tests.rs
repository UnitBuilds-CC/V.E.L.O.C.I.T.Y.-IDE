use velocity_browser::dom::FormDataSerializer;
use velocity_browser::engine::SoftwareRasterizer;
use velocity_browser::layout::LayoutEngine2D;
use velocity_browser::net::TlsState;
use velocity_browser::parser::HtmlParser;
use velocity_browser::session_storage::SessionStorageDisk;
use velocity_browser::session::BrowserSession;
use velocity_browser::style::StyleCascader;
use tempfile::tempdir;

#[test]
fn test_form_data_serialization() {
    let html = r#"
        <form id="contact-form">
            <input type="text" name="username" value="agent_ian" />
            <input type="password" name="pass" value="secret123" />
        </form>
    "#;

    let nodes = HtmlParser::parse(html);
    let tree = velocity_browser::dom::DomTree::new(nodes);
    let data = FormDataSerializer::serialize_form(&tree, "#contact-form");

    assert_eq!(data.get("username").map(|s| s.as_str()), Some("agent_ian"));
    assert_eq!(data.get("pass").map(|s| s.as_str()), Some("secret123"));

    let encoded = FormDataSerializer::to_url_encoded(&data);
    assert!(encoded.contains("username=agent_ian"));
}

#[test]
fn test_persistent_session_disk_storage() {
    let dir = tempdir().unwrap();
    let storage_path = dir.path().to_str().unwrap();
    let storage = SessionStorageDisk::new(storage_path);

    let session = BrowserSession::new("persistent_sess_100".to_string());
    let triples = session.capture_state_nda();

    let save_res = storage.save_session_nda("persistent_sess_100", &triples);
    assert!(save_res.is_ok());

    let loaded = storage.load_session_nda("persistent_sess_100").unwrap();
    assert_eq!(loaded.len(), triples.len());
}

#[test]
fn test_software_rasterizer() {
    let html = "<html><body><div id=\"box\">Hello Rasterizer</div></body></html>";
    let nodes = HtmlParser::parse(html);
    let tree = velocity_browser::dom::DomTree::new(nodes);

    let cascader = StyleCascader::new();
    let layout_engine = LayoutEngine2D::new(cascader);
    let boxes = layout_engine.build_layout_tree(&tree);

    let buffer = SoftwareRasterizer::render_layout(&boxes, 800, 600);
    assert_eq!(buffer.pixels.len(), 800 * 600);

    let nda_triples = SoftwareRasterizer::raster_to_nda(&buffer, "raster_sess");
    assert_eq!(nda_triples.len(), 2);
}
