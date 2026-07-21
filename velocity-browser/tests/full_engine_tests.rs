use velocity_browser::engine::ServiceWorkerManager;
use velocity_browser::layout::{FlexAlignmentSolver, JustifyContent, LayoutBox};
use velocity_browser::session::BrowserSession;
use velocity_browser::session_cookie_store::{CookieRecord, CookieStore, SameSitePolicy};

#[test]
fn test_flex_alignment_solver_and_service_worker() {
    let mut boxes = vec![
        LayoutBox { node_id: 1, x: 0.0, y: 0.0, width: 100.0, height: 50.0, padding: [0.0; 4], margin: [0.0; 4], z_index: 0, display: velocity_browser::layout::DisplayMode::Block, children: Vec::new(), is_visible: true },
        LayoutBox { node_id: 2, x: 0.0, y: 0.0, width: 100.0, height: 50.0, padding: [0.0; 4], margin: [0.0; 4], z_index: 0, display: velocity_browser::layout::DisplayMode::Block, children: Vec::new(), is_visible: true },
    ];
    FlexAlignmentSolver::align_main_axis(400.0, &mut boxes, JustifyContent::SpaceBetween);
    assert_eq!(boxes[0].x, 0.0);
    assert_eq!(boxes[1].x, 300.0);

    let sw = ServiceWorkerManager::register("/sw.js");
    assert_eq!(sw.script_url, "/sw.js");
}

#[test]
fn test_cookie_store_samesite_scoping() {
    let mut store = CookieStore::new();
    store.set_cookie(CookieRecord {
        name: "session_token".to_string(),
        value: "secret_xyz".to_string(),
        domain: "example.com".to_string(),
        path: "/".to_string(),
        expires_timestamp: 9999999999.0,
        samesite: SameSitePolicy::Lax,
        secure: true,
        http_only: true,
    });

    let matched = store.get_cookies_for_url("app.example.com", "/dashboard", true);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].value, "secret_xyz");

    let mut session = BrowserSession::new("sess_cookie_store".to_string());
    session.cookie_store.set_cookie(CookieRecord {
        name: "test_cookie".to_string(),
        value: "test_val".to_string(),
        domain: "localhost".to_string(),
        path: "/".to_string(),
        expires_timestamp: 0.0,
        samesite: SameSitePolicy::Lax,
        secure: false,
        http_only: false,
    });

    let state = session.capture_state_nda();
    assert!(state.iter().any(|t| t.predicate_id == 170)); // CookieStore predicate
}
