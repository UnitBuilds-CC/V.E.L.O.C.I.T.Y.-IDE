#[allow(unused_imports)]
use velocity_browser::agentic::AgenticAomTree;
#[allow(unused_imports)]
use velocity_browser::engine::{CanvasElement, CanvasExtractor, FrameTarget, InterstitialClassifier, InterstitialKind, ShadowFrameExtractor, ShadowHost};
use velocity_browser::nda::NdaTriple;
use velocity_browser::parser::{CssMatcher, HtmlParser};
use velocity_browser::session::BrowserSession;

#[test]
fn test_html_parser_and_css_matcher() {
    let html = r#"
        <html>
            <head><title>Test Agentic Engine</title></head>
            <body>
                <h1 id="main-heading">Welcome Agent</h1>
                <form action="/login">
                    <input type="text" name="username" value="agent1" placeholder="Enter username" />
                    <button id="btn-submit" type="submit">Log In</button>
                </form>
            </body>
        </html>
    "#;

    let nodes = HtmlParser::parse(html);
    assert!(nodes.len() > 5);

    let matches_heading = CssMatcher::find_matches(&nodes, "#main-heading");
    assert_eq!(matches_heading.len(), 1);
    assert_eq!(matches_heading[0].tag_name, "h1");

    let matches_btn = CssMatcher::find_matches(&nodes, "#btn-submit");
    assert_eq!(matches_btn.len(), 1);
    assert_eq!(matches_btn[0].tag_name, "button");
}

#[test]
fn test_native_browser_session_agentic_flow() {
    let mut session = BrowserSession::new("session_native_999".to_string());
    let html = r#"
        <html>
            <head><title>Agentic Login Page</title></head>
            <body>
                <input id="user-field" type="text" name="user" value="" />
                <button id="login-btn" type="submit">Submit</button>
            </body>
        </html>
    "#;

    let triples = session.load_html("https://agent.unitbuilds.com/login", html);
    assert!(!triples.is_empty());
    assert_eq!(session.page_title, "Agentic Login Page");

    assert!(session.fill("#user-field", "admin_agent").is_ok());
    assert!(session.click("#login-btn").is_ok());

    let state_triples = session.capture_state_nda();
    assert!(state_triples.iter().any(|t| t.predicate_id == 100)); // session url
}

#[test]
fn test_nda_triple_encoding() {
    let triple = NdaTriple::new("http://example.com", 1, "page");
    assert_eq!(triple.predicate_id, 1);

    let packed = triple.to_bytes();
    assert_eq!(packed.len(), 18);
}
