//! Tests for web platform APIs: performance, history, observers, WebSocket,
//! getComputedStyle, matchMedia, FileReader, crypto.subtle, caches, DOMRect.

use super::*;

// ── Performance ──────────────────────────────────────────────────────────────

#[test]
fn performance_now_returns_number() {
    let result = eval_full("performance.now()");
    assert!(matches!(result, JsValue::Number(_)));
}

#[test]
fn performance_mark_and_measure() {
    let result = eval_full(
        r#"
        performance.mark('start');
        performance.mark('end');
        performance.measure('dur', 'start', 'end');
        performance.getEntriesByType('measure').length
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn performance_get_entries_by_name() {
    let result = eval_full(
        r#"
        performance.mark('a');
        performance.mark('b');
        performance.getEntriesByName('a').length
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn performance_clear_marks() {
    let result = eval_full(
        r#"
        performance.mark('x');
        performance.clearMarks();
        performance.getEntriesByType('mark').length
    "#,
    );
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn performance_time_origin() {
    let result = eval_full("typeof performance.timeOrigin");
    assert_eq!(result, JsValue::String("number".to_string()));
}

// ── History ──────────────────────────────────────────────────────────────────

#[test]
fn history_initial_length() {
    let result = eval_full("history.length");
    assert!(matches!(result, JsValue::Number(n) if n >= 1.0));
}

#[test]
fn history_push_state_increases_length() {
    let result = eval_full(
        r#"
        let before = history.length;
        history.pushState({page: 1}, '', '/page1');
        history.length > before
    "#,
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn history_state_after_push() {
    let result = eval_full(
        r#"
        history.pushState({id: 42}, '', '/test');
        history.state.id
    "#,
    );
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn history_replace_state() {
    let result = eval_full(
        r#"
        history.pushState({v: 1}, '', '/a');
        history.replaceState({v: 2}, '', '/b');
        history.state.v
    "#,
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn history_back_forward() {
    let result = eval_full(
        r#"
        history.pushState({p: 1}, '', '/1');
        history.pushState({p: 2}, '', '/2');
        history.back();
        history.state.p
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}

// ── IntersectionObserver ─────────────────────────────────────────────────────

#[test]
fn intersection_observer_construct() {
    let result = eval_full(
        r#"
        let io = new IntersectionObserver(function() {});
        io.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("IntersectionObserver".to_string()));
}

#[test]
fn intersection_observer_observe_disconnect() {
    let result = eval_full(
        r#"
        let io = new IntersectionObserver(function() {});
        let el = document.createElement('div');
        io = io.observe(el);
        io.__targets__.length
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}

// ── ResizeObserver ───────────────────────────────────────────────────────────

#[test]
fn resize_observer_construct() {
    let result = eval_full(
        r#"
        let ro = new ResizeObserver(function() {});
        ro.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("ResizeObserver".to_string()));
}

#[test]
fn resize_observer_observe_unobserve() {
    let result = eval_full(
        r#"
        let ro = new ResizeObserver(function() {});
        let el = document.createElement('div');
        ro = ro.observe(el);
        ro = ro.unobserve(el);
        ro.__targets__.length
    "#,
    );
    assert_eq!(result, JsValue::Number(0.0));
}

// ── WebSocket ────────────────────────────────────────────────────────────────

#[test]
fn web_socket_construct() {
    let result = eval_full(
        r#"
        let ws = new WebSocket('wss://example.com/socket');
        ws.url
    "#,
    );
    assert_eq!(
        result,
        JsValue::String("wss://example.com/socket".to_string())
    );
}

#[test]
fn web_socket_send_close() {
    let result = eval_full(
        r#"
        let ws = new WebSocket('wss://example.com');
        ws = ws.send('hello');
        ws = ws.close();
        ws.readyState
    "#,
    );
    assert_eq!(result, JsValue::Number(3.0));
}

// ── getComputedStyle ─────────────────────────────────────────────────────────

#[test]
fn get_computed_style_returns_object() {
    let result = eval_full(
        r#"
        let el = document.createElement('div');
        let style = getComputedStyle(el);
        style.display
    "#,
    );
    assert_eq!(result, JsValue::String("block".to_string()));
}

#[test]
fn get_computed_style_get_property_value() {
    let result = eval_full(
        r#"
        let el = document.createElement('div');
        let style = getComputedStyle(el);
        style.getPropertyValue('font-size')
    "#,
    );
    assert_eq!(result, JsValue::String("16px".to_string()));
}

// ── matchMedia ───────────────────────────────────────────────────────────────

#[test]
fn match_media_screen() {
    let result = eval_full("matchMedia('screen').matches");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn match_media_dark_mode() {
    let result = eval_full("matchMedia('(prefers-color-scheme: dark)').matches");
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn match_media_min_width() {
    let result = eval_full("matchMedia('(min-width: 768px)').matches");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn match_media_print() {
    let result = eval_full("matchMedia('print').matches");
    assert_eq!(result, JsValue::Boolean(false));
}

// ── FileReader ───────────────────────────────────────────────────────────────

#[test]
fn file_reader_read_as_text() {
    let result = eval_full(
        r#"
        let blob = new Blob(['hello world'], {type: 'text/plain'});
        let fr = new FileReader();
        fr = fr.readAsText(blob);
        fr.result
    "#,
    );
    assert_eq!(result, JsValue::String("hello world".to_string()));
}

#[test]
fn file_reader_ready_state() {
    let result = eval_full(
        r#"
        let fr = new FileReader();
        let blob = new Blob(['test']);
        fr = fr.readAsText(blob);
        fr.readyState
    "#,
    );
    assert_eq!(result, JsValue::Number(2.0));
}

// ── crypto.subtle ────────────────────────────────────────────────────────────

#[test]
fn crypto_subtle_digest() {
    let result = eval_full(
        r#"
        let data = new TextEncoder().encode('hello');
        let hash = crypto.subtle.digest('SHA-256', data);
        hash.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("ArrayBuffer".to_string()));
}

#[test]
fn crypto_subtle_import_export_key() {
    let result = eval_full(
        r#"
        let key = crypto.subtle.importKey('raw', 'secret', {name: 'AES-GCM'}, true, ['encrypt']);
        key.type
    "#,
    );
    assert_eq!(result, JsValue::String("secret".to_string()));
}

#[test]
fn crypto_random_uuid_format() {
    let result = eval_full("crypto.randomUUID().length");
    assert_eq!(result, JsValue::Number(36.0));
}

// ── Cache API ────────────────────────────────────────────────────────────────

#[test]
fn caches_open_and_put() {
    let result = eval_full(
        r#"
        let cache = caches.open('v1');
        cache.__resolved__.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Cache".to_string()));
}

#[test]
fn caches_has() {
    let result = eval_full(
        r#"
        caches.open('test-cache');
        caches.has('test-cache').__resolved__
    "#,
    );
    assert_eq!(result, JsValue::Boolean(true));
}

// ── DOMRect ──────────────────────────────────────────────────────────────────

#[test]
fn dom_rect_construct() {
    let result = eval_full(
        r#"
        let r = new DOMRect(10, 20, 100, 50);
        r.right
    "#,
    );
    assert_eq!(result, JsValue::Number(110.0));
}

#[test]
fn dom_rect_bottom() {
    let result = eval_full(
        r#"
        let r = new DOMRect(0, 0, 200, 100);
        r.bottom
    "#,
    );
    assert_eq!(result, JsValue::Number(100.0));
}

// ── CSSStyleSheet ────────────────────────────────────────────────────────────

#[test]
fn css_style_sheet_replace_sync() {
    let result = eval_full(
        r#"
        let sheet = new CSSStyleSheet();
        sheet = sheet.replaceSync('body { margin: 0 }');
        sheet.__rules__.length
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}

// ── Navigator extensions ─────────────────────────────────────────────────────

#[test]
fn navigator_clipboard_type() {
    let result = eval_full("navigator.clipboard.__type__");
    assert_eq!(result, JsValue::String("Clipboard".to_string()));
}

#[test]
fn navigator_permissions_query() {
    let result = eval_full(
        r#"
        let status = navigator.permissions.query({name: 'geolocation'});
        status.__resolved__.state
    "#,
    );
    assert_eq!(result, JsValue::String("granted".to_string()));
}

#[test]
fn navigator_send_beacon() {
    let result = eval_full("navigator.sendBeacon('/log', 'data')");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn navigator_get_battery() {
    let result = eval_full(
        r#"
        let p = navigator.getBattery();
        p.__resolved__.level
    "#,
    );
    assert_eq!(result, JsValue::Number(1.0));
}
