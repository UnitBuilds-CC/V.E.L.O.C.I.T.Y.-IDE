use velocity_browser::agentic::AgenticAomTree;
use velocity_browser::js::JsEvaluator;
use velocity_browser::layout::LayoutEngine;
use velocity_browser::nda::NdaTriple;
use velocity_browser::parser::{CssMatcher, HtmlParser};
use velocity_browser::session::BrowserSession;

#[test]
fn test_compound_css_selectors() {
    let html = r#"
        <html>
            <body>
                <button id="btn-1" class="btn primary" type="submit" aria-label="Submit Form">Save</button>
                <input id="email" type="text" class="input-field" name="user_email" value="agent@test.com" />
                <div class="card"><span class="title">Card Title</span></div>
            </body>
        </html>
    "#;

    let nodes = HtmlParser::parse(html);

    // Test Tag#ID.Class compound selector
    let matches_btn = CssMatcher::find_matches(&nodes, "button#btn-1.primary");
    assert_eq!(matches_btn.len(), 1);
    assert_eq!(matches_btn[0].attributes.get("aria-label").map(|s| s.as_str()), Some("Submit Form"));

    // Test Attribute selector [type="text"]
    let matches_attr = CssMatcher::find_matches(&nodes, "[name=\"user_email\"]");
    assert_eq!(matches_attr.len(), 1);
    assert_eq!(matches_attr[0].attributes.get("id").map(|s| s.as_str()), Some("email"));
}

#[test]
fn test_aria_role_and_actionability_scoring() {
    let html = r#"
        <html>
            <body>
                <div role="button" aria-label="Custom Button" autofocus>Click Me</div>
                <nav aria-label="Main Navigation">Links</nav>
            </body>
        </html>
    "#;

    let nodes = HtmlParser::parse(html);
    let tree = velocity_browser::dom::DomTree::new(nodes);
    let aom_nodes = AgenticAomTree::build_aom_nodes(&tree);

    assert!(!aom_nodes.is_empty());
    let custom_btn = aom_nodes.iter().find(|n| n.name == "Custom Button").unwrap();
    assert_eq!(custom_btn.role, "button");
    assert_eq!(custom_btn.actionability_score, 100);
    assert!(custom_btn.is_focused);
}
