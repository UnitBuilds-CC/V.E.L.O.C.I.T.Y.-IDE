use velocity_browser::agentic::AgenticAomTree;
use velocity_browser::js::JsEvaluator;
use velocity_browser::layout::LayoutEngine;
use velocity_browser::nda::NdaTriple;
use velocity_browser::parser::{CssMatcher, HtmlParser};
use velocity_browser::session::BrowserSession;

#[test]
fn test_full_native_browser_session_flow() {
    let mut session = BrowserSession::new("full_session_777".to_string());
    let html = r#"
        <html>
            <head><title>Full Agentic Engine Test</title></head>
            <body>
                <h1 id="title">Heading</h1>
                <input id="email-field" type="text" name="email" value="" />
                <button id="submit-btn" type="submit">Submit</button>
            </body>
        </html>
    "#;

    let triples = session.load_html("http://localhost:8080/test", html);
    assert!(!triples.is_empty());

    // Native JS Evaluation
    assert!(session.eval_js("document.querySelector('email-field').value = 'agent@unitbuilds.com'").is_ok());

    // Native Form Input & Button Click
    assert!(session.fill("#email-field", "agent@unitbuilds.com").is_ok());
    assert!(session.click("#submit-btn").is_ok());

    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 60)); // Layout bounding box predicate
}

#[test]
fn test_layout_engine_bounds() {
    let html = "<html><body><button id=\"btn\">Click</button></body></html>";
    let nodes = HtmlParser::parse(html);
    let tree = velocity_browser::dom::DomTree::new(nodes);

    let triples = LayoutEngine::compute_layout_triples(&tree);
    assert!(!triples.is_empty());
    assert!(triples.iter().any(|t| t.predicate_id == 60));
}
