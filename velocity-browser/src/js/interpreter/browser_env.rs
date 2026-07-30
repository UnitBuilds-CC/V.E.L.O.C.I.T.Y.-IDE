//! Browser environment APIs for the JS interpreter.
//!
//! Provides timer scheduling (setTimeout/setInterval), browser global objects
//! (window, navigator, location, document), and Web Storage (localStorage,
//! sessionStorage). All state is thread-local so each interpreter instance gets
//! its own isolated environment — matching the browser's per-origin isolation.

use crate::js::vm::JsValue;
use std::cell::RefCell;
use std::collections::HashMap;

// ── Timer Registry ───────────────────────────────────────────────────────────

thread_local! {
    static TIMER_REGISTRY: RefCell<HashMap<u32, TimerEntry>> = RefCell::new(HashMap::new());
    static NEXT_TIMER_ID: RefCell<u32> = const { RefCell::new(1) };
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TimerEntry {
    kind: TimerKind,
    delay_ms: f64,
    cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerKind {
    Timeout,
    Interval,
}

/// Register a new timer and return its id.
fn register_timer(kind: TimerKind, delay_ms: f64) -> u32 {
    let id = NEXT_TIMER_ID.with(|c| {
        let mut id = c.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });
    TIMER_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(id, TimerEntry { kind, delay_ms, cancelled: false });
    });
    id
}

/// Cancel a timer by id.
fn cancel_timer(id: u32) {
    TIMER_REGISTRY.with(|reg| {
        if let Some(entry) = reg.borrow_mut().get_mut(&id) {
            entry.cancelled = true;
        }
    });
}

/// Handle `setTimeout(callback, delay)` — returns a numeric timer id.
pub(super) fn set_timeout(args: &[JsValue]) -> JsValue {
    let delay = args.get(1).and_then(|v| if let JsValue::Number(n) = v { Some(*n) } else { None }).unwrap_or(0.0);
    let id = register_timer(TimerKind::Timeout, delay);
    JsValue::Number(id as f64)
}

/// Handle `setInterval(callback, delay)` — returns a numeric timer id.
pub(super) fn set_interval(args: &[JsValue]) -> JsValue {
    let delay = args.get(1).and_then(|v| if let JsValue::Number(n) = v { Some(*n) } else { None }).unwrap_or(0.0);
    let id = register_timer(TimerKind::Interval, delay);
    JsValue::Number(id as f64)
}

/// Handle `clearTimeout(id)` / `clearInterval(id)`.
pub(super) fn clear_timer(args: &[JsValue]) -> JsValue {
    if let Some(JsValue::Number(id)) = args.first() {
        cancel_timer(*id as u32);
    }
    JsValue::Undefined
}

/// Reset all timer state (for test isolation).
#[cfg(test)]
pub fn reset_timers() {
    TIMER_REGISTRY.with(|reg| reg.borrow_mut().clear());
    NEXT_TIMER_ID.with(|c| *c.borrow_mut() = 1);
}

// ── Browser Global Objects ───────────────────────────────────────────────────

/// Build the `navigator` object with common properties agents inspect.
pub(super) fn make_navigator() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Navigator".to_string()));
    map.insert("userAgent".to_string(), JsValue::String(
        "Mozilla/5.0 (compatible; VelocityBrowser/1.0; +https://velocity.dev/agent)".to_string()
    ));
    map.insert("appName".to_string(), JsValue::String("Velocity".to_string()));
    map.insert("appVersion".to_string(), JsValue::String("1.0".to_string()));
    map.insert("platform".to_string(), JsValue::String("Win32".to_string()));
    map.insert("language".to_string(), JsValue::String("en-US".to_string()));
    map.insert("languages".to_string(), JsValue::Array(vec![
        JsValue::String("en-US".to_string()),
        JsValue::String("en".to_string()),
    ]));
    map.insert("onLine".to_string(), JsValue::Boolean(true));
    map.insert("hardwareConcurrency".to_string(), JsValue::Number(8.0));
    map.insert("maxTouchPoints".to_string(), JsValue::Number(0.0));
    map.insert("cookieEnabled".to_string(), JsValue::Boolean(true));
    map.insert("webdriver".to_string(), JsValue::Boolean(true));
    map.insert("vendor".to_string(), JsValue::String("Velocity".to_string()));
    // Sub-objects for agent APIs.
    let mut clipboard = HashMap::new();
    clipboard.insert("__type__".to_string(), JsValue::String("Clipboard".to_string()));
    map.insert("clipboard".to_string(), JsValue::Object(clipboard));
    let mut permissions = HashMap::new();
    permissions.insert("__type__".to_string(), JsValue::String("Permissions".to_string()));
    map.insert("permissions".to_string(), JsValue::Object(permissions));
    let mut geolocation = HashMap::new();
    geolocation.insert("__type__".to_string(), JsValue::String("Geolocation".to_string()));
    map.insert("geolocation".to_string(), JsValue::Object(geolocation));
    map.insert("serviceWorker".to_string(), super::web_platform::make_service_worker_container());
    // userAgentData (Client Hints).
    let mut uad = HashMap::new();
    uad.insert("__type__".to_string(), JsValue::String("NavigatorUAData".to_string()));
    uad.insert("mobile".to_string(), JsValue::Boolean(false));
    uad.insert("platform".to_string(), JsValue::String("Windows".to_string()));
    uad.insert("brands".to_string(), JsValue::Array(vec![
        JsValue::Object({ let mut b = HashMap::new(); b.insert("brand".to_string(), JsValue::String("Velocity".to_string())); b.insert("version".to_string(), JsValue::String("1".to_string())); b }),
    ]));
    map.insert("userAgentData".to_string(), JsValue::Object(uad));
    JsValue::Object(map)
}

/// Build the `location` object for a given URL.
pub(super) fn make_location(url: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Location".to_string()));

    // Parse the URL into components.
    let (protocol, rest) = url.split_once("://").unwrap_or(("https", url));
    let (host_part, path_part) = rest.split_once('/').unwrap_or((rest, ""));
    let (path, query) = path_part.split_once('?').unwrap_or((path_part, ""));
    let (query_clean, hash) = query.split_once('#').unwrap_or((query, ""));
    let path = if path.is_empty() { "/" } else { &format!("/{}", path) };

    let (hostname, port) = host_part.split_once(':').unwrap_or((host_part, ""));
    let origin = format!("{}://{}", protocol, host_part);
    let href = if hash.is_empty() {
        format!("{}{}{}", origin, path, if query_clean.is_empty() { String::new() } else { format!("?{}", query_clean) })
    } else {
        format!("{}{}{}#{}", origin, path, if query_clean.is_empty() { String::new() } else { format!("?{}", query_clean) }, hash)
    };

    map.insert("href".to_string(), JsValue::String(href));
    map.insert("protocol".to_string(), JsValue::String(format!("{}:", protocol)));
    map.insert("host".to_string(), JsValue::String(host_part.to_string()));
    map.insert("hostname".to_string(), JsValue::String(hostname.to_string()));
    map.insert("port".to_string(), JsValue::String(port.to_string()));
    map.insert("pathname".to_string(), JsValue::String(path.to_string()));
    map.insert("search".to_string(), JsValue::String(
        if query_clean.is_empty() { String::new() } else { format!("?{}", query_clean) }
    ));
    map.insert("hash".to_string(), JsValue::String(
        if hash.is_empty() { String::new() } else { format!("#{}", hash) }
    ));
    map.insert("origin".to_string(), JsValue::String(origin));
    JsValue::Object(map)
}

/// Build a minimal `document` stub object for the interpreter.
///
/// This provides the most common document properties and methods that agents
/// interact with. Actual DOM manipulation is handled by the native DOM bridge
/// (`dom_api.rs`) at a higher layer; this stub ensures scripts that reference
/// `document.*` don't crash with undefined errors.
pub(super) fn make_document() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Document".to_string()));
    map.insert("readyState".to_string(), JsValue::String("complete".to_string()));
    map.insert("title".to_string(), JsValue::String(String::new()));
    map.insert("URL".to_string(), JsValue::String("about:blank".to_string()));
    map.insert("domain".to_string(), JsValue::String(String::new()));
    map.insert("referrer".to_string(), JsValue::String(String::new()));
    map.insert("characterSet".to_string(), JsValue::String("UTF-8".to_string()));
    map.insert("contentType".to_string(), JsValue::String("text/html".to_string()));
    map.insert("compatMode".to_string(), JsValue::String("CSS1Compat".to_string()));
    map.insert("visibilityState".to_string(), JsValue::String("visible".to_string()));
    map.insert("hidden".to_string(), JsValue::Boolean(false));
    map.insert("designMode".to_string(), JsValue::String("off".to_string()));
    JsValue::Object(map)
}

/// Build the `window` object — the global scope proxy.
pub(super) fn make_window() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Window".to_string()));
    map.insert("innerWidth".to_string(), JsValue::Number(1920.0));
    map.insert("innerHeight".to_string(), JsValue::Number(1080.0));
    map.insert("outerWidth".to_string(), JsValue::Number(1920.0));
    map.insert("outerHeight".to_string(), JsValue::Number(1080.0));
    map.insert("devicePixelRatio".to_string(), JsValue::Number(1.0));
    map.insert("scrollX".to_string(), JsValue::Number(0.0));
    map.insert("scrollY".to_string(), JsValue::Number(0.0));
    map.insert("pageXOffset".to_string(), JsValue::Number(0.0));
    map.insert("pageYOffset".to_string(), JsValue::Number(0.0));
    map.insert("screenX".to_string(), JsValue::Number(0.0));
    map.insert("screenY".to_string(), JsValue::Number(0.0));
    map.insert("closed".to_string(), JsValue::Boolean(false));
    map.insert("name".to_string(), JsValue::String(String::new()));
    map.insert("status".to_string(), JsValue::String(String::new()));
    map.insert("isSecureContext".to_string(), JsValue::Boolean(true));
    map.insert("origin".to_string(), JsValue::String("null".to_string()));
    map.insert("crossOriginIsolated".to_string(), JsValue::Boolean(false));
    // visualViewport.
    let mut vv = HashMap::new();
    vv.insert("__type__".to_string(), JsValue::String("VisualViewport".to_string()));
    vv.insert("width".to_string(), JsValue::Number(1920.0));
    vv.insert("height".to_string(), JsValue::Number(1080.0));
    vv.insert("offsetLeft".to_string(), JsValue::Number(0.0));
    vv.insert("offsetTop".to_string(), JsValue::Number(0.0));
    vv.insert("pageLeft".to_string(), JsValue::Number(0.0));
    vv.insert("pageTop".to_string(), JsValue::Number(0.0));
    vv.insert("scale".to_string(), JsValue::Number(1.0));
    map.insert("visualViewport".to_string(), JsValue::Object(vv));
    // screen.
    let mut screen = HashMap::new();
    screen.insert("__type__".to_string(), JsValue::String("Screen".to_string()));
    screen.insert("width".to_string(), JsValue::Number(1920.0));
    screen.insert("height".to_string(), JsValue::Number(1080.0));
    screen.insert("availWidth".to_string(), JsValue::Number(1920.0));
    screen.insert("availHeight".to_string(), JsValue::Number(1040.0));
    screen.insert("colorDepth".to_string(), JsValue::Number(24.0));
    screen.insert("pixelDepth".to_string(), JsValue::Number(24.0));
    let mut orientation = HashMap::new();
    orientation.insert("__type__".to_string(), JsValue::String("ScreenOrientation".to_string()));
    orientation.insert("type".to_string(), JsValue::String("landscape-primary".to_string()));
    orientation.insert("angle".to_string(), JsValue::Number(0.0));
    screen.insert("orientation".to_string(), JsValue::Object(orientation));
    screen.insert("availLeft".to_string(), JsValue::Number(0.0));
    screen.insert("availTop".to_string(), JsValue::Number(0.0));
    map.insert("screen".to_string(), JsValue::Object(screen));
    JsValue::Object(map)
}

// ── Web Storage (localStorage / sessionStorage) ──────────────────────────────

thread_local! {
    static LOCAL_STORAGE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static SESSION_STORAGE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Dispatch a method call on a Storage object.
pub(super) fn call_storage_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let is_local = map.get("__storage_type__").and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }) == Some("local");

    match method {
        "getItem" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let val = if is_local {
                LOCAL_STORAGE.with(|s| s.borrow().get(&key).cloned())
            } else {
                SESSION_STORAGE.with(|s| s.borrow().get(&key).cloned())
            };
            val.map(JsValue::String).unwrap_or(JsValue::Null)
        }
        "setItem" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let value = args.get(1).map(|v| if let JsValue::String(s) = v { s.clone() } else { super::coercion::to_string(v) }).unwrap_or_default();
            if is_local {
                LOCAL_STORAGE.with(|s| { s.borrow_mut().insert(key, value); });
            } else {
                SESSION_STORAGE.with(|s| { s.borrow_mut().insert(key, value); });
            }
            JsValue::Undefined
        }
        "removeItem" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            if is_local {
                LOCAL_STORAGE.with(|s| { s.borrow_mut().remove(&key); });
            } else {
                SESSION_STORAGE.with(|s| { s.borrow_mut().remove(&key); });
            }
            JsValue::Undefined
        }
        "clear" => {
            if is_local {
                LOCAL_STORAGE.with(|s| s.borrow_mut().clear());
            } else {
                SESSION_STORAGE.with(|s| s.borrow_mut().clear());
            }
            JsValue::Undefined
        }
        "key" => {
            let index = args.first().and_then(|v| if let JsValue::Number(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
            let val = if is_local {
                LOCAL_STORAGE.with(|s| s.borrow().keys().nth(index).cloned())
            } else {
                SESSION_STORAGE.with(|s| s.borrow().keys().nth(index).cloned())
            };
            val.map(JsValue::String).unwrap_or(JsValue::Null)
        }
        _ => JsValue::Undefined,
    }
}

/// Get the `length` property of a Storage object.
pub(super) fn storage_length(map: &HashMap<String, JsValue>) -> JsValue {
    let is_local = map.get("__storage_type__").and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }) == Some("local");
    let len = if is_local {
        LOCAL_STORAGE.with(|s| s.borrow().len())
    } else {
        SESSION_STORAGE.with(|s| s.borrow().len())
    };
    JsValue::Number(len as f64)
}

/// Create a `localStorage` object handle.
pub(super) fn make_local_storage() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Storage".to_string()));
    map.insert("__storage_type__".to_string(), JsValue::String("local".to_string()));
    JsValue::Object(map)
}

/// Create a `sessionStorage` object handle.
pub(super) fn make_session_storage() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Storage".to_string()));
    map.insert("__storage_type__".to_string(), JsValue::String("session".to_string()));
    JsValue::Object(map)
}

/// Reset all storage state (for test isolation).
#[cfg(test)]
pub fn reset_storage() {
    LOCAL_STORAGE.with(|s| s.borrow_mut().clear());
    SESSION_STORAGE.with(|s| s.borrow_mut().clear());
}

// ── Fetch API ────────────────────────────────────────────────────────────────

thread_local! {
    /// Real network I/O is opt-in: the host session enables it explicitly so
    /// tests and untrusted scripts stay hermetic by default.
    static NETWORK_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Per-interpreter HTTP client — persists cookies across fetches.
    static HTTP_CLIENT: RefCell<Option<crate::net::HttpClient>> = const { RefCell::new(None) };
}

/// Enable or disable real network I/O for `fetch()` on this thread.
pub fn set_network_enabled(enabled: bool) {
    NETWORK_ENABLED.with(|c| c.set(enabled));
}

/// Whether real network I/O is enabled on this thread.
pub fn network_enabled() -> bool {
    NETWORK_ENABLED.with(|c| c.get())
}

/// Handle `fetch(url, options)` — returns a settled Promise wrapping a
/// Response object.
///
/// With networking enabled (see [`set_network_enabled`]), the request runs on
/// the from-scratch HTTP/1.1 + TLS 1.3 stack ([`crate::net::HttpClient`]) with
/// persistent cookies; failures reject the promise like a real `fetch`.
/// Disabled (the default), a mock 200 response preserves API-surface
/// compatibility for hermetic execution.
pub(super) fn call_fetch(args: &[JsValue]) -> JsValue {
    let url = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
    let (method, body, content_type) = if let Some(JsValue::Object(opts)) = args.get(1) {
        let method = opts.get("method").and_then(|v| if let JsValue::String(s) = v { Some(s.to_uppercase()) } else { None }).unwrap_or_else(|| "GET".to_string());
        let body = opts.get("body").map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
        let content_type = match opts.get("headers") {
            Some(JsValue::Object(headers)) => headers.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| crate::js::interpreter::coercion::to_string(v))
                .unwrap_or_else(|| "application/json".to_string()),
            _ => "application/json".to_string(),
        };
        (method, body, content_type)
    } else {
        ("GET".to_string(), String::new(), String::new())
    };

    if network_enabled() {
        return fetch_over_network(&url, &method, &body, &content_type);
    }

    // Hermetic mode: mock 200 response.
    let mut response = HashMap::new();
    response.insert("__type__".to_string(), JsValue::String("Response".to_string()));
    response.insert("ok".to_string(), JsValue::Boolean(true));
    response.insert("status".to_string(), JsValue::Number(200.0));
    response.insert("statusText".to_string(), JsValue::String("OK".to_string()));
    response.insert("url".to_string(), JsValue::String(url));
    response.insert("redirected".to_string(), JsValue::Boolean(false));
    response.insert("type".to_string(), JsValue::String("basic".to_string()));
    response.insert("__body__".to_string(), JsValue::String(String::new()));
    response.insert("__method__".to_string(), JsValue::String(method));

    resolved_promise(JsValue::Object(response))
}

/// Perform a real request on the native HTTP client and wrap the outcome in a
/// settled promise. GET-like methods use `get`; methods with a body use `post`.
fn fetch_over_network(url: &str, method: &str, body: &str, content_type: &str) -> JsValue {
    let result = HTTP_CLIENT.with(|client| {
        let mut client = client.borrow_mut();
        let client = client.get_or_insert_with(crate::net::HttpClient::new);
        match method {
            "POST" | "PUT" | "PATCH" | "DELETE" if !body.is_empty() || method == "POST" => {
                client.post(url, body, content_type)
            }
            _ => client.get(url),
        }
    });

    match result {
        Ok(resp) => {
            let mut headers = HashMap::new();
            for (k, v) in &resp.headers {
                headers.insert(k.clone(), JsValue::String(v.clone()));
            }
            let status = resp.status_code;
            let mut response = HashMap::new();
            response.insert("__type__".to_string(), JsValue::String("Response".to_string()));
            response.insert("ok".to_string(), JsValue::Boolean((200..300).contains(&status)));
            response.insert("status".to_string(), JsValue::Number(status as f64));
            response.insert("statusText".to_string(), JsValue::String(status_text(status).to_string()));
            response.insert("url".to_string(), JsValue::String(url.to_string()));
            response.insert("redirected".to_string(), JsValue::Boolean(false));
            response.insert("type".to_string(), JsValue::String("basic".to_string()));
            response.insert("headers".to_string(), JsValue::Object(headers));
            response.insert("__body__".to_string(), JsValue::String(resp.body));
            response.insert("__method__".to_string(), JsValue::String(method.to_string()));
            resolved_promise(JsValue::Object(response))
        }
        Err(e) => {
            let mut err = HashMap::new();
            err.insert("name".to_string(), JsValue::String("TypeError".to_string()));
            err.insert("message".to_string(), JsValue::String(format!("fetch failed: {}", e)));
            let mut promise = HashMap::new();
            promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            promise.insert("__rejected__".to_string(), JsValue::Object(err));
            JsValue::Object(promise)
        }
    }
}

fn resolved_promise(val: JsValue) -> JsValue {
    let mut promise = HashMap::new();
    promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
    promise.insert("__resolved__".to_string(), val);
    JsValue::Object(promise)
}

/// Canonical reason phrase for an HTTP status code.
fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}


// ── Headers ──────────────────────────────────────────────────────────────────

/// Create a new Headers object.
pub(super) fn make_headers(init: Option<&JsValue>) -> JsValue {
    let mut entries: Vec<(String, String)> = Vec::new();
    if let Some(JsValue::Object(map)) = init {
        for (k, v) in map {
            if !k.starts_with("__") {
                entries.push((k.to_lowercase(), super::coercion::to_string(v)));
            }
        }
    } else if let Some(JsValue::Array(arr)) = init {
        for item in arr {
            if let JsValue::Array(pair) = item {
                if pair.len() >= 2 {
                    entries.push((
                        super::coercion::to_string(&pair[0]).to_lowercase(),
                        super::coercion::to_string(&pair[1]),
                    ));
                }
            }
        }
    }
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Headers".to_string()));
    map.insert("__entries__".to_string(), JsValue::Array(
        entries.into_iter().map(|(k, v)| JsValue::Array(vec![JsValue::String(k), JsValue::String(v)])).collect()
    ));
    JsValue::Object(map)
}

/// Dispatch a method call on a Headers object.
pub(super) fn call_headers_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let entries = match map.get("__entries__") {
        Some(JsValue::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    };

    match method {
        "get" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default().to_lowercase();
            for entry in &entries {
                if let JsValue::Array(pair) = entry {
                    if pair.len() >= 2 {
                        if let JsValue::String(k) = &pair[0] {
                            if k.to_lowercase() == name {
                                return pair[1].clone();
                            }
                        }
                    }
                }
            }
            JsValue::Null
        }
        "has" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default().to_lowercase();
            let found = entries.iter().any(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(s.to_lowercase() == name) } else { None }).unwrap_or(false)
                } else { false }
            });
            JsValue::Boolean(found)
        }
        "set" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default().to_lowercase();
            let value = args.get(1).map(super::coercion::to_string).unwrap_or_default();
            let mut new_entries: Vec<JsValue> = entries.into_iter().filter(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(s.to_lowercase() != name) } else { None }).unwrap_or(true)
                } else { true }
            }).collect();
            new_entries.push(JsValue::Array(vec![JsValue::String(name), JsValue::String(value)]));
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_entries));
            JsValue::Object(updated)
        }
        "delete" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default().to_lowercase();
            let new_entries: Vec<JsValue> = entries.into_iter().filter(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(s.to_lowercase() != name) } else { None }).unwrap_or(true)
                } else { true }
            }).collect();
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_entries));
            JsValue::Object(updated)
        }
        "forEach" => JsValue::Undefined,
        "entries" => JsValue::Array(entries),
        "keys" => {
            let keys: Vec<JsValue> = entries.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry { pair.first().cloned() } else { None }
            }).collect();
            JsValue::Array(keys)
        }
        "values" => {
            let values: Vec<JsValue> = entries.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry { pair.get(1).cloned() } else { None }
            }).collect();
            JsValue::Array(values)
        }
        _ => JsValue::Undefined,
    }
}

// ── FormData ─────────────────────────────────────────────────────────────────

/// Create a new FormData object.
pub(super) fn make_form_data() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("FormData".to_string()));
    map.insert("__entries__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

/// Dispatch a method call on a FormData object.
pub(super) fn call_form_data_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let entries = match map.get("__entries__") {
        Some(JsValue::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    };

    match method {
        "append" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let value = args.get(1).map(super::coercion::to_string).unwrap_or_default();
            let mut new_entries = entries;
            new_entries.push(JsValue::Array(vec![JsValue::String(name), JsValue::String(value)]));
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_entries));
            JsValue::Object(updated)
        }
        "set" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let value = args.get(1).map(super::coercion::to_string).unwrap_or_default();
            let mut new_entries: Vec<JsValue> = entries.into_iter().filter(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(*s != name) } else { None }).unwrap_or(true)
                } else { true }
            }).collect();
            new_entries.push(JsValue::Array(vec![JsValue::String(name), JsValue::String(value)]));
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_entries));
            JsValue::Object(updated)
        }
        "get" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            for entry in &entries {
                if let JsValue::Array(pair) = entry {
                    if pair.len() >= 2 {
                        if let JsValue::String(k) = &pair[0] {
                            if *k == name { return pair[1].clone(); }
                        }
                    }
                }
            }
            JsValue::Null
        }
        "getAll" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let results: Vec<JsValue> = entries.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry {
                    if pair.len() >= 2 {
                        if let JsValue::String(k) = &pair[0] {
                            if *k == name { return pair.get(1).cloned(); }
                        }
                    }
                }
                None
            }).collect();
            JsValue::Array(results)
        }
        "has" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let found = entries.iter().any(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(*s == name) } else { None }).unwrap_or(false)
                } else { false }
            });
            JsValue::Boolean(found)
        }
        "delete" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let new_entries: Vec<JsValue> = entries.into_iter().filter(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(*s != name) } else { None }).unwrap_or(true)
                } else { true }
            }).collect();
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_entries));
            JsValue::Object(updated)
        }
        "entries" => JsValue::Array(entries),
        "keys" => {
            let keys: Vec<JsValue> = entries.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry { pair.first().cloned() } else { None }
            }).collect();
            JsValue::Array(keys)
        }
        "values" => {
            let values: Vec<JsValue> = entries.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry { pair.get(1).cloned() } else { None }
            }).collect();
            JsValue::Array(values)
        }
        _ => JsValue::Undefined,
    }
}

// ── Event / CustomEvent ──────────────────────────────────────────────────────

/// Create an Event object.
pub(super) fn make_event(event_type: &str, opts: Option<&JsValue>) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Event".to_string()));
    map.insert("type".to_string(), JsValue::String(event_type.to_string()));
    let bubbles = opts.and_then(|o| if let JsValue::Object(m) = o { m.get("bubbles").cloned() } else { None }).unwrap_or(JsValue::Boolean(false));
    let cancelable = opts.and_then(|o| if let JsValue::Object(m) = o { m.get("cancelable").cloned() } else { None }).unwrap_or(JsValue::Boolean(false));
    let composed = opts.and_then(|o| if let JsValue::Object(m) = o { m.get("composed").cloned() } else { None }).unwrap_or(JsValue::Boolean(false));
    map.insert("bubbles".to_string(), bubbles);
    map.insert("cancelable".to_string(), cancelable);
    map.insert("composed".to_string(), composed);
    map.insert("defaultPrevented".to_string(), JsValue::Boolean(false));
    map.insert("eventPhase".to_string(), JsValue::Number(2.0)); // AT_TARGET
    map.insert("timeStamp".to_string(), JsValue::Number(perf_now_ms()));
    map.insert("isTrusted".to_string(), JsValue::Boolean(false));
    JsValue::Object(map)
}

/// Create a CustomEvent object (extends Event with `detail`).
pub(super) fn make_custom_event(event_type: &str, opts: Option<&JsValue>) -> JsValue {
    let mut event = make_event(event_type, opts);
    if let JsValue::Object(ref mut map) = event {
        map.insert("__type__".to_string(), JsValue::String("CustomEvent".to_string()));
        let detail = opts.and_then(|o| if let JsValue::Object(m) = o { m.get("detail").cloned() } else { None }).unwrap_or(JsValue::Null);
        map.insert("detail".to_string(), detail);
    }
    event
}

/// Create a specialized event with a given __type__ tag and extra properties from opts.
pub(super) fn make_typed_event(tag: &str, event_type: &str, opts: Option<&JsValue>, extras: &[(&str, JsValue)]) -> JsValue {
    let mut event = make_event(event_type, opts);
    if let JsValue::Object(ref mut map) = event {
        map.insert("__type__".to_string(), JsValue::String(tag.to_string()));
        if let Some(JsValue::Object(o)) = opts {
            for (key, default_val) in extras {
                let val = o.get(*key).cloned().unwrap_or_else(|| default_val.clone());
                map.insert(key.to_string(), val);
            }
        } else {
            for (key, default_val) in extras {
                map.insert(key.to_string(), default_val.clone());
            }
        }
    }
    event
}

/// Dispatch a method call on an Event object.
pub(super) fn call_event_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "preventDefault" => {
            let mut updated = map.clone();
            updated.insert("defaultPrevented".to_string(), JsValue::Boolean(true));
            JsValue::Object(updated)
        }
        "stopPropagation" | "stopImmediatePropagation" => {
            let mut updated = map.clone();
            updated.insert("__propagation_stopped__".to_string(), JsValue::Boolean(true));
            JsValue::Object(updated)
        }
        "composedPath" => JsValue::Array(Vec::new()),
        _ => JsValue::Undefined,
    }
}

// ── URL / URLSearchParams ────────────────────────────────────────────────────

/// Create a URL object from a string.
pub(super) fn make_url(url_str: &str, base: Option<&str>) -> Result<JsValue, String> {
    // Resolve relative URLs against base.
    let resolved = if url_str.starts_with("http://") || url_str.starts_with("https://") || url_str.contains("://") {
        url_str.to_string()
    } else if let Some(b) = base {
        if url_str.starts_with('/') {
            // Absolute path relative to base origin.
            let origin = b.split('/').take(3).collect::<Vec<_>>().join("/");
            format!("{}{}", origin, url_str)
        } else {
            // Relative path.
            let base_dir = b.rsplit_once('/').map(|(d, _)| d).unwrap_or(b);
            format!("{}/{}", base_dir, url_str)
        }
    } else {
        return Err(format!("Invalid URL: {}", url_str));
    };

    let (protocol, rest) = resolved.split_once("://").ok_or_else(|| format!("Invalid URL: {}", resolved))?;
    let (authority, path_and_rest) = rest.split_once('/').unwrap_or((rest, ""));
    let (path, query_and_hash) = path_and_rest.split_once('?').unwrap_or((path_and_rest, ""));
    let (query, hash) = query_and_hash.split_once('#').unwrap_or((query_and_hash, ""));
    let path = if path.is_empty() { "/".to_string() } else { format!("/{}", path) };

    let (hostname, port) = authority.split_once(':').unwrap_or((authority, ""));
    let host = if port.is_empty() { authority.to_string() } else { format!("{}:{}", hostname, port) };
    let origin = format!("{}://{}", protocol, authority);

    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("URL".to_string()));
    map.insert("href".to_string(), JsValue::String(resolved.clone()));
    map.insert("protocol".to_string(), JsValue::String(format!("{}:", protocol)));
    map.insert("host".to_string(), JsValue::String(host));
    map.insert("hostname".to_string(), JsValue::String(hostname.to_string()));
    map.insert("port".to_string(), JsValue::String(port.to_string()));
    map.insert("pathname".to_string(), JsValue::String(path));
    map.insert("search".to_string(), JsValue::String(if query.is_empty() { String::new() } else { format!("?{}", query) }));
    map.insert("hash".to_string(), JsValue::String(if hash.is_empty() { String::new() } else { format!("#{}", hash) }));
    map.insert("origin".to_string(), JsValue::String(origin));
    map.insert("username".to_string(), JsValue::String(String::new()));
    map.insert("password".to_string(), JsValue::String(String::new()));

    // Attach searchParams as a URLSearchParams object.
    map.insert("searchParams".to_string(), make_url_search_params(query));

    Ok(JsValue::Object(map))
}

/// Create a URLSearchParams object from a query string.
pub(super) fn make_url_search_params(query: &str) -> JsValue {
    let query = query.trim_start_matches('?');
    let params: Vec<JsValue> = if query.is_empty() {
        Vec::new()
    } else {
        query.split('&').filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k.is_empty() { return None; }
            Some(JsValue::Array(vec![
                JsValue::String(decode_uri(k)),
                JsValue::String(decode_uri(v)),
            ]))
        }).collect()
    };
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("URLSearchParams".to_string()));
    map.insert("__entries__".to_string(), JsValue::Array(params));
    JsValue::Object(map)
}

/// Dispatch a method call on a URLSearchParams object.
pub(super) fn call_url_search_params_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    let params = match map.get("__entries__") {
        Some(JsValue::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    };

    match method {
        "get" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            for entry in &params {
                if let JsValue::Array(pair) = entry {
                    if pair.len() >= 2 {
                        if let JsValue::String(k) = &pair[0] {
                            if *k == name { return pair[1].clone(); }
                        }
                    }
                }
            }
            JsValue::Null
        }
        "getAll" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let results: Vec<JsValue> = params.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry {
                    if pair.len() >= 2 {
                        if let JsValue::String(k) = &pair[0] {
                            if *k == name { return pair.get(1).cloned(); }
                        }
                    }
                }
                None
            }).collect();
            JsValue::Array(results)
        }
        "has" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let found = params.iter().any(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(*s == name) } else { None }).unwrap_or(false)
                } else { false }
            });
            JsValue::Boolean(found)
        }
        "set" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let value = args.get(1).map(super::coercion::to_string).unwrap_or_default();
            let mut new_params: Vec<JsValue> = params.into_iter().filter(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(*s != name) } else { None }).unwrap_or(true)
                } else { true }
            }).collect();
            new_params.push(JsValue::Array(vec![JsValue::String(name), JsValue::String(value)]));
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_params));
            JsValue::Object(updated)
        }
        "append" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let value = args.get(1).map(super::coercion::to_string).unwrap_or_default();
            let mut new_params = params;
            new_params.push(JsValue::Array(vec![JsValue::String(name), JsValue::String(value)]));
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_params));
            JsValue::Object(updated)
        }
        "delete" => {
            let name = args.first().map(super::coercion::to_string).unwrap_or_default();
            let new_params: Vec<JsValue> = params.into_iter().filter(|entry| {
                if let JsValue::Array(pair) = entry {
                    pair.first().and_then(|k| if let JsValue::String(s) = k { Some(*s != name) } else { None }).unwrap_or(true)
                } else { true }
            }).collect();
            let mut updated = map.clone();
            updated.insert("__entries__".to_string(), JsValue::Array(new_params));
            JsValue::Object(updated)
        }
        "toString" => {
            let s: Vec<String> = params.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry {
                    if pair.len() >= 2 {
                        let k = super::coercion::to_string(&pair[0]);
                        let v = super::coercion::to_string(&pair[1]);
                        return Some(format!("{}={}", encode_uri(&k), encode_uri(&v)));
                    }
                }
                None
            }).collect();
            JsValue::String(s.join("&"))
        }
        "entries" => JsValue::Array(params),
        "keys" => {
            let keys: Vec<JsValue> = params.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry { pair.first().cloned() } else { None }
            }).collect();
            JsValue::Array(keys)
        }
        "values" => {
            let values: Vec<JsValue> = params.iter().filter_map(|entry| {
                if let JsValue::Array(pair) = entry { pair.get(1).cloned() } else { None }
            }).collect();
            JsValue::Array(values)
        }
        _ => JsValue::Undefined,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn perf_now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn decode_uri(s: &str) -> String {
    s.replace("%20", " ").replace("%3D", "=").replace("%26", "&")
     .replace("%3F", "?").replace("%2F", "/").replace("%25", "%")
     .replace('+', " ")
}

fn encode_uri(s: &str) -> String {
    s.replace(' ', "%20").replace('=', "%3D").replace('&', "%26")
     .replace('?', "%3F").replace('/', "%2F")
}

// ── DOMParser ────────────────────────────────────────────────────────────────

/// Create a DOMParser object.
pub(super) fn make_dom_parser() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("DOMParser".to_string()));
    JsValue::Object(map)
}

/// Dispatch a method call on a DOMParser object.
pub(super) fn call_dom_parser_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "parseFromString" => {
            let _html = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            // Return a Document-like object. Full HTML parsing is handled at
            // the session layer; here we provide the API surface.
            super::dom_bridge::call_document_method("createElement", &[JsValue::String("div".to_string())])
        }
        _ => JsValue::Undefined,
    }
}

// ── XMLHttpRequest ───────────────────────────────────────────────────────────

/// Create an XMLHttpRequest object.
pub(super) fn make_xhr() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("XMLHttpRequest".to_string()));
    map.insert("readyState".to_string(), JsValue::Number(0.0)); // UNSENT
    map.insert("status".to_string(), JsValue::Number(0.0));
    map.insert("statusText".to_string(), JsValue::String(String::new()));
    map.insert("responseText".to_string(), JsValue::String(String::new()));
    map.insert("response".to_string(), JsValue::String(String::new()));
    map.insert("responseURL".to_string(), JsValue::String(String::new()));
    map.insert("__method__".to_string(), JsValue::String("GET".to_string()));
    map.insert("__url__".to_string(), JsValue::String(String::new()));
    map.insert("__headers__".to_string(), JsValue::Object(HashMap::new()));
    JsValue::Object(map)
}

/// Dispatch a method call on an XMLHttpRequest object.
pub(super) fn call_xhr_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "open" => {
            let method_str = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.to_uppercase()) } else { None }).unwrap_or_else(|| "GET".to_string());
            let url = args.get(1).and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let mut updated = map.clone();
            updated.insert("__method__".to_string(), JsValue::String(method_str));
            updated.insert("__url__".to_string(), JsValue::String(url));
            updated.insert("readyState".to_string(), JsValue::Number(1.0)); // OPENED
            JsValue::Object(updated)
        }
        "setRequestHeader" => {
            let key = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let val = args.get(1).and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let mut updated = map.clone();
            if let Some(JsValue::Object(headers)) = updated.get_mut("__headers__") {
                headers.insert(key, JsValue::String(val));
            }
            JsValue::Object(updated)
        }
        "send" => {
            // Simulate a successful synchronous response.
            let mut updated = map.clone();
            updated.insert("readyState".to_string(), JsValue::Number(4.0)); // DONE
            updated.insert("status".to_string(), JsValue::Number(200.0));
            updated.insert("statusText".to_string(), JsValue::String("OK".to_string()));
            updated.insert("responseText".to_string(), JsValue::String(String::new()));
            updated.insert("response".to_string(), JsValue::String(String::new()));
            JsValue::Object(updated)
        }
        "abort" => {
            let mut updated = map.clone();
            updated.insert("readyState".to_string(), JsValue::Number(0.0));
            JsValue::Object(updated)
        }
        "getResponseHeader" => {
            let _name = args.first().and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }).unwrap_or("");
            JsValue::Null
        }
        "getAllResponseHeaders" => JsValue::String(String::new()),
        "overrideMimeType" => JsValue::Object(map.clone()),
        _ => JsValue::Undefined,
    }
}

// ── MutationObserver ─────────────────────────────────────────────────────────

/// Create a MutationObserver object.
pub(super) fn make_mutation_observer(callback: JsValue) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("MutationObserver".to_string()));
    map.insert("__callback__".to_string(), callback);
    map.insert("__observing__".to_string(), JsValue::Boolean(false));
    JsValue::Object(map)
}

/// Dispatch a method call on a MutationObserver object.
pub(super) fn call_mutation_observer_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "observe" => {
            let mut updated = map.clone();
            updated.insert("__observing__".to_string(), JsValue::Boolean(true));
            JsValue::Object(updated)
        }
        "disconnect" => {
            let mut updated = map.clone();
            updated.insert("__observing__".to_string(), JsValue::Boolean(false));
            JsValue::Object(updated)
        }
        "takeRecords" => JsValue::Array(Vec::new()),
        _ => JsValue::Undefined,
    }
}

// ── BroadcastChannel ─────────────────────────────────────────────────────────

/// Create a BroadcastChannel object.
pub(super) fn make_broadcast_channel(name: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("BroadcastChannel".to_string()));
    map.insert("name".to_string(), JsValue::String(name.to_string()));
    map.insert("__closed__".to_string(), JsValue::Boolean(false));
    JsValue::Object(map)
}

/// Dispatch a method call on a BroadcastChannel object.
pub(super) fn call_broadcast_channel_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "postMessage" => JsValue::Undefined,
        "close" => {
            let mut updated = map.clone();
            updated.insert("__closed__".to_string(), JsValue::Boolean(true));
            JsValue::Object(updated)
        }
        _ => JsValue::Undefined,
    }
}
