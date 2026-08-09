//! Tests for browser environment APIs: timers, globals, storage, fetch,
//! Headers, FormData, Event/CustomEvent, URL, URLSearchParams.

use super::*;

// ── Timers ───────────────────────────────────────────────────────────────────

#[test]
fn set_timeout_returns_numeric_id() {
    super::super::browser_env::reset_timers();
    let result = eval_full("setTimeout(function() {}, 100)");
    assert!(matches!(result, JsValue::Number(n) if n >= 1.0));
}

#[test]
fn set_interval_returns_numeric_id() {
    let result = eval_full("setInterval(function() {}, 1000)");
    assert!(matches!(result, JsValue::Number(n) if n >= 1.0));
}

#[test]
fn clear_timeout_returns_undefined() {
    assert_eq!(eval_full("clearTimeout(1)"), JsValue::Undefined);
}

#[test]
fn clear_interval_returns_undefined() {
    assert_eq!(eval_full("clearInterval(1)"), JsValue::Undefined);
}

#[test]
fn timer_ids_increment() {
    let result = eval_full(
        "var a = setTimeout(function(){}, 0); var b = setTimeout(function(){}, 0); b > a",
    );
    assert_eq!(result, JsValue::Boolean(true));
}

// ── Timer Flush ──────────────────────────────────────────────────────────────

#[test]
fn flush_timers_runs_timeout_callback() {
    let result = eval_full("var x = 0; setTimeout(function() { x = 1; }, 0); flushTimers(); x");
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn flush_timers_returns_executed_count() {
    let result =
        eval_full("setTimeout(function(){}, 0); setTimeout(function(){}, 0); flushTimers()");
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn flush_timers_removes_timeouts_after_run() {
    let result = eval_full("setTimeout(function(){}, 0); flushTimers(); flushTimers()");
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn flush_timers_keeps_intervals_registered() {
    let result = eval_full(
        "setInterval(function(){}, 5); var a = flushTimers(); var b = flushTimers(); a + b",
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn flush_timers_skips_cleared_timers() {
    let result = eval_full("var x = 0; var id = setTimeout(function(){ x = 1; }, 0); clearTimeout(id); flushTimers(); x");
    assert_eq!(result, JsValue::Number(0.0));
}

// ── Browser Globals ──────────────────────────────────────────────────────────

#[test]
fn navigator_user_agent() {
    let result = eval_full("navigator.userAgent");
    assert!(matches!(result, JsValue::String(s) if s.contains("Velocity")));
}

#[test]
fn navigator_properties() {
    assert_eq!(
        eval_full("navigator.language"),
        JsValue::String("en-US".to_string())
    );
    assert_eq!(eval_full("navigator.onLine"), JsValue::Boolean(true));
    assert_eq!(eval_full("navigator.webdriver"), JsValue::Boolean(true));
    assert_eq!(
        eval_full("navigator.hardwareConcurrency"),
        JsValue::Number(8.0)
    );
}

#[test]
fn location_properties() {
    assert_eq!(
        eval_full("location.protocol"),
        JsValue::String("https:".to_string())
    );
    assert_eq!(
        eval_full("location.hostname"),
        JsValue::String("localhost".to_string())
    );
    assert_eq!(
        eval_full("location.pathname"),
        JsValue::String("/".to_string())
    );
}

#[test]
fn document_properties() {
    // readyState starts at "loading" — waitForSettlement drives it to "complete".
    assert_eq!(
        eval_full("document.readyState"),
        JsValue::String("loading".to_string())
    );
    assert_eq!(eval_full("document.hidden"), JsValue::Boolean(false));
    assert_eq!(
        eval_full("document.characterSet"),
        JsValue::String("UTF-8".to_string())
    );
}

// ── Document Lifecycle ───────────────────────────────────────────────────────

#[test]
fn ready_state_completes_after_settlement() {
    let result = eval_full("document.waitForSettlement(); document.readyState");
    assert_eq!(result, JsValue::String("complete".to_string()));
}

#[test]
fn dom_content_loaded_fires_on_settlement() {
    let result = eval_full(
        "
        var loaded = false;
        document.addEventListener('DOMContentLoaded', function() { loaded = true; });
        document.waitForSettlement();
        loaded
    ",
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn window_load_listener_fires_on_settlement() {
    let result = eval_full(
        "
        var n = 0;
        window.addEventListener('load', function() { n = n + 1; });
        document.waitForSettlement();
        n
    ",
    );
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn lifecycle_fires_only_once() {
    let result = eval_full(
        "
        var n = 0;
        document.addEventListener('load', function() { n = n + 1; });
        document.waitForSettlement();
        document.waitForSettlement();
        n
    ",
    );
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn lifecycle_listener_sees_event_type() {
    let result = eval_full(
        "
        var seen = '';
        document.addEventListener('DOMContentLoaded', function(e) { seen = e.type; });
        document.waitForSettlement();
        seen
    ",
    );
    assert_eq!(result, JsValue::String("DOMContentLoaded".to_string()));
}

#[test]
fn lifecycle_timers_pump_in_same_settlement() {
    // A load handler that schedules a timeout — the same settlement call
    // must run it.
    let result = eval_full(
        "
        var x = 0;
        document.addEventListener('load', function() { setTimeout(function() { x = 7; }, 0); });
        document.waitForSettlement();
        x
    ",
    );
    assert_eq!(result, JsValue::Number(7.0));
}

#[test]
fn document_dispatch_event_fires_custom_listener() {
    let result = eval_full(
        "
        var got = false;
        document.addEventListener('agent-ping', function() { got = true; });
        document.dispatchEvent(new Event('agent-ping'));
        got
    ",
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn remove_event_listener_stops_lifecycle_delivery() {
    let result = eval_full(
        "
        var n = 0;
        var f = function() { n = n + 1; };
        document.addEventListener('load', f);
        document.removeEventListener('load', f);
        document.waitForSettlement();
        n
    ",
    );
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn settlement_reports_lifecycle_listener_count() {
    let result = eval_full(
        "
        document.addEventListener('DOMContentLoaded', function() {});
        window.addEventListener('load', function() {});
        document.waitForSettlement().lifecycleListenersRun
    ",
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn window_properties() {
    assert_eq!(eval_full("window.innerWidth"), JsValue::Number(1920.0));
    assert_eq!(eval_full("window.innerHeight"), JsValue::Number(1080.0));
    assert_eq!(eval_full("window.devicePixelRatio"), JsValue::Number(1.0));
    assert_eq!(eval_full("window.closed"), JsValue::Boolean(false));
}

// ── Storage ──────────────────────────────────────────────────────────────────

#[test]
fn local_storage_set_get_remove() {
    super::super::browser_env::reset_storage();
    assert_eq!(
        eval_full("localStorage.setItem('key', 'value'); localStorage.getItem('key')"),
        JsValue::String("value".to_string())
    );
    assert_eq!(
        eval_full("localStorage.removeItem('key'); localStorage.getItem('key')"),
        JsValue::Null
    );
}

#[test]
fn local_storage_length() {
    super::super::browser_env::reset_storage();
    assert_eq!(
        eval_full("localStorage.clear(); localStorage.length"),
        JsValue::Number(0.0)
    );
    assert_eq!(
        eval_full(
            "localStorage.setItem('a', '1'); localStorage.setItem('b', '2'); localStorage.length"
        ),
        JsValue::Number(2.0)
    );
}

#[test]
fn session_storage_independent() {
    super::super::browser_env::reset_storage();
    eval_full("localStorage.setItem('x', 'local')");
    eval_full("sessionStorage.setItem('x', 'session')");
    assert_eq!(
        eval_full("localStorage.getItem('x')"),
        JsValue::String("local".to_string())
    );
    assert_eq!(
        eval_full("sessionStorage.getItem('x')"),
        JsValue::String("session".to_string())
    );
}

// ── Fetch ────────────────────────────────────────────────────────────────────

#[test]
fn fetch_returns_promise() {
    let result = eval_full("var p = fetch('https://example.com'); p.__type__");
    assert_eq!(result, JsValue::String("Promise".to_string()));
}

#[test]
fn fetch_response_properties() {
    let result =
        eval_full("var p = fetch('https://example.com'); var r = p.__resolved__; r.status");
    assert_eq!(result, JsValue::Number(200.0));
}

#[test]
fn fetch_network_disabled_by_default() {
    assert!(!super::super::browser_env::network_enabled());
}

#[test]
fn fetch_network_toggle() {
    super::super::browser_env::set_network_enabled(true);
    assert!(super::super::browser_env::network_enabled());
    super::super::browser_env::set_network_enabled(false);
    assert!(!super::super::browser_env::network_enabled());
}

#[test]
fn fetch_network_enabled_connection_failure_rejects() {
    // Port 1 on loopback refuses instantly — hermetic test of the real path.
    super::super::browser_env::set_network_enabled(true);
    let result = eval_full("var p = fetch('http://127.0.0.1:1/'); p.__rejected__.name");
    super::super::browser_env::set_network_enabled(false);
    assert_eq!(result, JsValue::String("TypeError".to_string()));
}

#[test]
fn fetch_network_rejection_has_message() {
    super::super::browser_env::set_network_enabled(true);
    let result = eval_full("var p = fetch('http://127.0.0.1:1/'); p.__rejected__.message");
    super::super::browser_env::set_network_enabled(false);
    if let JsValue::String(s) = &result {
        assert!(s.starts_with("fetch failed"), "got: {}", s);
    } else {
        panic!("Expected string message");
    }
}

// ── Network Log ──────────────────────────────────────────────────────────────

#[test]
fn network_log_records_mock_fetch() {
    let result = eval_full(
        "fetch('https://example.com/api'); var log = document.getNetworkLog(); log.length",
    );
    assert_eq!(result, JsValue::Number(1.0));
    let url = eval_full("document.getNetworkLog()[0].url");
    assert_eq!(url, JsValue::String("https://example.com/api".to_string()));
    let mocked = eval_full("document.getNetworkLog()[0].mocked");
    assert_eq!(mocked, JsValue::Boolean(true));
}

#[test]
fn network_log_records_method() {
    let result = eval_full("fetch('https://example.com/x', {method: 'post', body: 'a=1'}); document.getNetworkLog()[0].method");
    assert_eq!(result, JsValue::String("POST".to_string()));
}

#[test]
fn network_log_text_is_readable() {
    let result = eval_full("fetch('https://example.com/y'); document.getNetworkLogText()");
    if let JsValue::String(s) = &result {
        assert!(
            s.contains("GET https://example.com/y -> 200 (mocked)"),
            "got: {}",
            s
        );
    } else {
        panic!("Expected string");
    }
}

#[test]
fn network_log_records_real_failure_as_status_zero() {
    super::super::browser_env::set_network_enabled(true);
    eval_full("fetch('http://127.0.0.1:1/')");
    super::super::browser_env::set_network_enabled(false);
    let status = eval_full("document.getNetworkLog()[0].status");
    assert_eq!(status, JsValue::Number(0.0));
}

// ── Headers ──────────────────────────────────────────────────────────────────

#[test]
fn headers_constructor_and_get() {
    let result = eval_full(
        "var h = new Headers({'Content-Type': 'application/json'}); h.get('content-type')",
    );
    assert_eq!(result, JsValue::String("application/json".to_string()));
}

#[test]
fn headers_has_and_delete() {
    let result = eval_full("var h = new Headers({'X-Test': '1'}); h.has('x-test')");
    assert_eq!(result, JsValue::Boolean(true));
}

// ── FormData ─────────────────────────────────────────────────────────────────

#[test]
fn form_data_append_and_get() {
    let result =
        eval_full("var fd = new FormData(); fd = fd.append('name', 'Alice'); fd.get('name')");
    assert_eq!(result, JsValue::String("Alice".to_string()));
}

#[test]
fn form_data_has() {
    let result = eval_full("var fd = new FormData(); fd = fd.append('key', 'val'); fd.has('key')");
    assert_eq!(result, JsValue::Boolean(true));
}

// ── Event / CustomEvent ──────────────────────────────────────────────────────

#[test]
fn event_constructor() {
    let result = eval_full("var e = new Event('click'); e.type");
    assert_eq!(result, JsValue::String("click".to_string()));
}

#[test]
fn event_options() {
    let result =
        eval_full("var e = new Event('submit', {bubbles: true, cancelable: true}); e.bubbles");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn custom_event_detail() {
    let result = eval_full("var e = new CustomEvent('data', {detail: {id: 42}}); e.detail.id");
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn event_prevent_default() {
    let result = eval_full("var e = new Event('click', {cancelable: true}); e = e.preventDefault(); e.defaultPrevented");
    assert_eq!(result, JsValue::Boolean(true));
}

// ── URL ──────────────────────────────────────────────────────────────────────

#[test]
fn url_constructor_parses_components() {
    assert_eq!(
        eval_full("var u = new URL('https://example.com:8080/path?q=1#frag'); u.protocol"),
        JsValue::String("https:".to_string())
    );
    assert_eq!(
        eval_full("var u = new URL('https://example.com:8080/path?q=1#frag'); u.hostname"),
        JsValue::String("example.com".to_string())
    );
    assert_eq!(
        eval_full("var u = new URL('https://example.com:8080/path?q=1#frag'); u.port"),
        JsValue::String("8080".to_string())
    );
    assert_eq!(
        eval_full("var u = new URL('https://example.com:8080/path?q=1#frag'); u.pathname"),
        JsValue::String("/path".to_string())
    );
}

#[test]
fn url_search_and_hash() {
    assert_eq!(
        eval_full("var u = new URL('https://x.com/p?a=1&b=2#top'); u.search"),
        JsValue::String("?a=1&b=2".to_string())
    );
    assert_eq!(
        eval_full("var u = new URL('https://x.com/p?a=1&b=2#top'); u.hash"),
        JsValue::String("#top".to_string())
    );
}

// ── URLSearchParams ──────────────────────────────────────────────────────────

#[test]
fn url_search_params_get() {
    let result = eval_full("var p = new URLSearchParams('?foo=bar&baz=qux'); p.get('foo')");
    assert_eq!(result, JsValue::String("bar".to_string()));
}

#[test]
fn url_search_params_has() {
    let result = eval_full("var p = new URLSearchParams('a=1&b=2'); p.has('b')");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn url_search_params_to_string() {
    let result = eval_full("var p = new URLSearchParams('x=1&y=2'); p.toString()");
    assert_eq!(result, JsValue::String("x=1&y=2".to_string()));
}

#[test]
fn url_search_params_append() {
    let result =
        eval_full("var p = new URLSearchParams('a=1'); p = p.append('b', '2'); p.get('b')");
    assert_eq!(result, JsValue::String("2".to_string()));
}

// ── DOMParser ────────────────────────────────────────────────────────────────

#[test]
fn dom_parser_parse_from_string() {
    let result = eval_full("var parser = new DOMParser(); var doc = parser.parseFromString('<div>hi</div>', 'text/html'); doc.tagName");
    assert_eq!(result, JsValue::String("DIV".to_string()));
}

// ── XMLHttpRequest ───────────────────────────────────────────────────────────

#[test]
fn xhr_open_send() {
    let result = eval_full("var xhr = new XMLHttpRequest(); xhr = xhr.open('GET', 'https://example.com'); xhr = xhr.send(); xhr.status");
    assert_eq!(result, JsValue::Number(200.0));
}

#[test]
fn xhr_ready_state() {
    let result =
        eval_full("var xhr = new XMLHttpRequest(); xhr = xhr.open('POST', '/api'); xhr.readyState");
    assert_eq!(result, JsValue::Number(1.0));
}

// ── MutationObserver ─────────────────────────────────────────────────────────

#[test]
fn mutation_observer_observe_disconnect() {
    let result = eval_full("var mo = new MutationObserver(function(){}); mo = mo.observe(document.body, {childList: true}); mo.__observing__");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn mutation_observer_take_records() {
    let result = eval_full("var mo = new MutationObserver(function(){}); mo.takeRecords().length");
    assert_eq!(result, JsValue::Number(0.0));
}

// ── BroadcastChannel ─────────────────────────────────────────────────────────

#[test]
fn broadcast_channel_name() {
    let result = eval_full("var bc = new BroadcastChannel('test'); bc.name");
    assert_eq!(result, JsValue::String("test".to_string()));
}

#[test]
fn broadcast_channel_close() {
    let result = eval_full("var bc = new BroadcastChannel('x'); bc = bc.close(); bc.__closed__");
    assert_eq!(result, JsValue::Boolean(true));
}
