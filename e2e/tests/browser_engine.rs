//! E2E tests for the browser session's public workflow.
//!
//! These intentionally exercise the public session API rather than probing for
//! compiled test binaries. A passing result proves that document loading,
//! inspection, interaction, and session state work together.

use velocity_browser::session::BrowserSession;

#[test]
fn browser_session_load_inspect_interact_and_preserve_state() {
    let mut session = BrowserSession::new("e2e-browser-workflow".to_string());
    session.load_html(
        "https://example.test/sign-in",
        r#"<!doctype html>
        <html><head><title>Sign in</title></head><body>
          <h1>Welcome</h1>
          <form><label>Email <input id="email" /></label>
          <button id="submit">Continue</button></form>
          <table><caption>Accounts</caption><tr><th>Name</th><th>Role</th></tr>
          <tr><td>Ada</td><td>Admin</td></tr></table>
        </body></html>"#,
    );

    assert_eq!(session.current_url, "https://example.test/sign-in");
    assert_eq!(session.page_title, "Sign in");
    assert!(session.page_summary_text().contains("Welcome"));
    assert!(session.page_summary_text().contains("1 form(s)"));
    assert!(session.page_tables_text().contains("| Name | Role |"));
    assert!(session.page_tables_text().contains("| Ada | Admin |"));

    session.fill("#email", "ada@example.test").unwrap();
    session.click("#submit").unwrap();
    session.scroll(0, 240).unwrap();
    session.set_storage_item("onboarding", "complete");

    let tree = session.dom_tree.as_ref().expect("loaded DOM");
    let email = tree
        .query_selector("#email")
        .and_then(|id| tree.get_node(id))
        .expect("email input");
    assert_eq!(
        email.attributes.get("value"),
        Some(&"ada@example.test".to_string())
    );
    assert!(session.scroll_y >= 240.0);
    assert_eq!(
        session.storage.get("onboarding"),
        Some(&"complete".to_string())
    );
    assert_eq!(session.trace_collector.mutations_for("#submit").len(), 1);
    assert!(session.trace_collector.total_count() >= 3);
}

#[test]
fn browser_session_rejects_interactions_without_a_document() {
    let mut session = BrowserSession::new("e2e-empty-browser".to_string());

    assert!(session.click("#missing").is_err());
    assert!(session.fill("#missing", "value").is_err());
    assert!(session.page_summary_text().is_empty());
}
