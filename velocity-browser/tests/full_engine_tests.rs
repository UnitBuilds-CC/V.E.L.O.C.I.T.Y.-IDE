use velocity_browser::dom::CustomElementRegistry;
use velocity_browser::engine::{PushNotificationManager, SandboxCapabilities, TabSandbox, WebCryptoEngine, WebGLContext};
use velocity_browser::js::WebWorkerPool;
use velocity_browser::session::BrowserSession;
use velocity_browser::session_history::HistoryStack;
use velocity_browser::session_storage_quota::StorageQuotaManager;
use velocity_browser::style::ScopedCssMatcher;
use velocity_browser::parser::html::DomNode;
use std::collections::HashMap;

#[test]
fn test_push_worker_and_storage_quota() {
    let mut push_mgr = PushNotificationManager::new();
    let sub = push_mgr.subscribe("https://push.example.com/sub/1", "p256_key", "auth_secret");
    assert_eq!(sub.endpoint, "https://push.example.com/sub/1");

    let mut pool = WebWorkerPool::new();
    let id = pool.spawn_worker("/worker.js");
    assert!(pool.post_message(&id, r#"{"type":"start"}"#));

    let mut quota = StorageQuotaManager::new(100);
    assert!(quota.reserve(50).is_ok());
    assert!(quota.reserve(60).is_err());
}

#[test]
fn test_custom_element_registry() {
    let mut reg = CustomElementRegistry::new();
    assert!(reg.define("user-card", "UserCardElement", None).is_ok());
    assert!(reg.define("invalidtag", "InvalidElement", None).is_err());
}
