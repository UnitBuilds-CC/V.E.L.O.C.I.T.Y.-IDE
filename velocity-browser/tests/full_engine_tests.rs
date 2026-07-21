use velocity_browser::js::JsEventLoopScheduler;
use velocity_browser::parser::HtmlParser;
use velocity_browser::session::BrowserSession;
use velocity_browser::session_auth::{AuthReseeder, AuthTokenState};
use std::collections::HashMap;

#[test]
fn test_js_event_loop_scheduler() {
    let mut scheduler = JsEventLoopScheduler::new();
    let t1 = scheduler.schedule_timer("console.log('timer')", 100);
    let m1 = scheduler.queue_microtask("console.log('microtask')");

    assert_eq!(t1, 1);
    assert_eq!(m1, 2);

    let next = scheduler.pop_next_task().unwrap();
    assert_eq!(next.script, "console.log('microtask')"); // Microtasks run first
}

#[test]
fn test_auth_reseeding_and_mutation_observer() {
    let mut session = BrowserSession::new("auth_reseed_sess".to_string());
    let html = "<html><body><input id=\"username\" type=\"text\" value=\"\" /></body></html>";
    session.load_html("http://localhost/app", html);

    let mut storage = HashMap::new();
    storage.insert("access_token".to_string(), "bearer_secret_abc123".to_string());

    let auth_state = AuthReseeder::extract_auth_state(&session.cookies, &storage);
    session.reseed_auth(&auth_state);

    assert!(session.fill("#username", "reseeded_user").is_ok());

    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 140)); // MutationObserver predicate
}
