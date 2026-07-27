//! Web APIs handler for the JS evaluation pipeline.
//!
//! Intercepts calls to browser built-in APIs (fetch, setTimeout, setInterval,
//! localStorage, sessionStorage, window.location, console) at the string level
//! before the full interpreter runs. This keeps the hot paths in native Rust
//! and avoids the need to thread complex session state through the interpreter.

use crate::js::vm::JsValue;
use std::collections::HashMap;

/// Result of evaluating a web API call.
pub struct WebApiResult {
    pub value: JsValue,
    /// Pending timer to schedule: (script, delay_ms, is_interval)
    pub pending_timer: Option<(String, u64, bool)>,
    /// Timer to cancel
    pub cancel_timer_id: Option<u64>,
    /// Fetch request to perform: (url, method, body, content_type)
    pub fetch_request: Option<(String, String, Option<String>, Option<String>)>,
    /// Storage operation: (storage_type, operation, key, value)
    pub storage_op: Option<(StorageType, StorageOp, Option<String>, Option<String>)>,
    /// Console output: (level, message)
    pub console_output: Option<(String, String)>,
    /// Navigation request: url to navigate to
    pub navigation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageType {
    Local,
    Session,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageOp {
    GetItem,
    SetItem,
    RemoveItem,
    Clear,
    Key,
    Length,
}

impl WebApiResult {
    fn simple(value: JsValue) -> Self {
        Self {
            value,
            pending_timer: None,
            cancel_timer_id: None,
            fetch_request: None,
            storage_op: None,
            console_output: None,
            navigation: None,
        }
    }

    fn with_timer(value: JsValue, script: String, delay: u64, is_interval: bool) -> Self {
        Self {
            value,
            pending_timer: Some((script, delay, is_interval)),
            cancel_timer_id: None,
            fetch_request: None,
            storage_op: None,
            console_output: None,
            navigation: None,
        }
    }

    fn with_cancel(value: JsValue, id: u64) -> Self {
        Self {
            value,
            pending_timer: None,
            cancel_timer_id: Some(id),
            fetch_request: None,
            storage_op: None,
            console_output: None,
            navigation: None,
        }
    }

    fn with_fetch(url: String, method: String, body: Option<String>, ct: Option<String>) -> Self {
        Self {
            value: JsValue::Undefined,
            pending_timer: None,
            cancel_timer_id: None,
            fetch_request: Some((url, method, body, ct)),
            storage_op: None,
            console_output: None,
            navigation: None,
        }
    }

    fn with_storage(value: JsValue, st: StorageType, op: StorageOp, key: Option<String>, val: Option<String>) -> Self {
        Self {
            value,
            pending_timer: None,
            cancel_timer_id: None,
            fetch_request: None,
            storage_op: Some((st, op, key, val)),
            console_output: None,
            navigation: None,
        }
    }

    fn with_console(level: &str, msg: String) -> Self {
        Self {
            value: JsValue::Undefined,
            pending_timer: None,
            cancel_timer_id: None,
            fetch_request: None,
            storage_op: None,
            console_output: Some((level.to_string(), msg)),
            navigation: None,
        }
    }
}

/// Try to evaluate `expr` as a Web API call.
/// Returns `None` if not a recognized web API pattern.
pub fn eval_web_api(expr: &str, current_url: &str, timer_seq: u64) -> Option<WebApiResult> {
    let expr = expr.trim();

    // setTimeout(script/fn, delay)
    if let Some(inner) = expr.strip_prefix("setTimeout(").and_then(|s| s.strip_suffix(")")) {
        return Some(handle_set_timeout(inner, timer_seq));
    }

    // setInterval(script/fn, delay)
    if let Some(inner) = expr.strip_prefix("setInterval(").and_then(|s| s.strip_suffix(")")) {
        return Some(handle_set_interval(inner, timer_seq));
    }

    // clearTimeout(id) / clearInterval(id)
    if let Some(inner) = expr.strip_prefix("clearTimeout(").and_then(|s| s.strip_suffix(")")) {
        if let Ok(id) = inner.trim().parse::<u64>() {
            return Some(WebApiResult::with_cancel(JsValue::Undefined, id));
        }
    }
    if let Some(inner) = expr.strip_prefix("clearInterval(").and_then(|s| s.strip_suffix(")")) {
        if let Ok(id) = inner.trim().parse::<u64>() {
            return Some(WebApiResult::with_cancel(JsValue::Undefined, id));
        }
    }

    // console.log/warn/error/info(...)
    if let Some(msg) = expr.strip_prefix("console.log(").and_then(|s| s.strip_suffix(")")) {
        return Some(WebApiResult::with_console("log", unquote(msg)));
    }
    if let Some(msg) = expr.strip_prefix("console.warn(").and_then(|s| s.strip_suffix(")")) {
        return Some(WebApiResult::with_console("warn", unquote(msg)));
    }
    if let Some(msg) = expr.strip_prefix("console.error(").and_then(|s| s.strip_suffix(")")) {
        return Some(WebApiResult::with_console("error", unquote(msg)));
    }
    if let Some(msg) = expr.strip_prefix("console.info(").and_then(|s| s.strip_suffix(")")) {
        return Some(WebApiResult::with_console("info", unquote(msg)));
    }

    // fetch(url) / fetch(url, options)
    if let Some(inner) = expr.strip_prefix("fetch(").and_then(|s| s.strip_suffix(")")) {
        return Some(handle_fetch(inner));
    }

    // localStorage.getItem/setItem/removeItem/clear
    if let Some(result) = handle_storage(expr, StorageType::Local, "localStorage") {
        return Some(result);
    }
    if let Some(result) = handle_storage(expr, StorageType::Session, "sessionStorage") {
        return Some(result);
    }

    // window.location properties
    if expr == "window.location.href" || expr == "location.href" {
        return Some(WebApiResult::simple(JsValue::String(current_url.to_string())));
    }
    if expr == "window.location.pathname" || expr == "location.pathname" {
        let path = extract_pathname(current_url);
        return Some(WebApiResult::simple(JsValue::String(path)));
    }
    if expr == "window.location.origin" || expr == "location.origin" {
        let origin = extract_origin(current_url);
        return Some(WebApiResult::simple(JsValue::String(origin)));
    }
    if expr == "window.location.search" || expr == "location.search" {
        let search = extract_search(current_url);
        return Some(WebApiResult::simple(JsValue::String(search)));
    }
    if expr == "window.location.host" || expr == "location.host" {
        let host = extract_host(current_url);
        return Some(WebApiResult::simple(JsValue::String(host)));
    }
    if expr == "window.location.hostname" || expr == "location.hostname" {
        let host = extract_host(current_url);
        let hostname = host.split(':').next().unwrap_or(&host).to_string();
        return Some(WebApiResult::simple(JsValue::String(hostname)));
    }
    if expr == "window.location.protocol" || expr == "location.protocol" {
        let protocol = if current_url.starts_with("https") { "https:" } else { "http:" };
        return Some(WebApiResult::simple(JsValue::String(protocol.to_string())));
    }

    // window.location.href = "url" (navigation)
    if let Some(url) = expr.strip_prefix("window.location.href=")
        .or_else(|| expr.strip_prefix("window.location.href ="))
        .or_else(|| expr.strip_prefix("window.location ="))
        .or_else(|| expr.strip_prefix("location.href="))
        .or_else(|| expr.strip_prefix("location.href ="))
    {
        let url = unquote(url.trim());
        return Some(WebApiResult {
            value: JsValue::Undefined,
            pending_timer: None,
            cancel_timer_id: None,
            fetch_request: None,
            storage_op: None,
            console_output: None,
            navigation: Some(url),
        });
    }

    None
}

/// Handle fetch call - returns a WebApiResult with fetch_request populated.
/// The actual HTTP call happens in session.rs which has access to HttpClient.
fn handle_fetch(inner: &str) -> WebApiResult {
    // Parse: fetch('url') or fetch('url', {method: 'POST', body: '...'})
    let parts: Vec<&str> = inner.splitn(2, ',').collect();
    let url = unquote(parts[0].trim());
    let mut method = "GET".to_string();
    let mut body = None;
    let mut content_type = None;

    if parts.len() > 1 {
        let opts = parts[1].trim();
        // Simple extraction of method and body from options object
        if let Some(m) = extract_obj_string(opts, "method") {
            method = m.to_uppercase();
        }
        if let Some(b) = extract_obj_string(opts, "body") {
            body = Some(b);
        }
        if let Some(ct) = extract_obj_string(opts, "Content-Type") {
            content_type = Some(ct);
        }
    }

    WebApiResult::with_fetch(url, method, body, content_type)
}

fn handle_set_timeout(inner: &str, timer_seq: u64) -> WebApiResult {
    let (script, delay) = parse_timer_args(inner);
    WebApiResult::with_timer(JsValue::Number(timer_seq as f64), script, delay, false)
}

fn handle_set_interval(inner: &str, timer_seq: u64) -> WebApiResult {
    let (script, delay) = parse_timer_args(inner);
    WebApiResult::with_timer(JsValue::Number(timer_seq as f64), script, delay, true)
}

/// Parse timer arguments: (callback_script, delay_ms)
fn parse_timer_args(inner: &str) -> (String, u64) {
    // Can be: function(){...}, delay  OR  "script", delay
    let inner = inner.trim();

    // Find the last comma followed by a number (the delay)
    let delay;
    let script;

    if let Some(last_comma) = inner.rfind(',') {
        let after = inner[last_comma + 1..].trim();
        if let Ok(d) = after.parse::<u64>() {
            delay = d;
            script = inner[..last_comma].trim().to_string();
        } else {
            delay = 0;
            script = inner.to_string();
        }
    } else {
        delay = 0;
        script = inner.to_string();
    }

    // Clean up the script: if it's a quoted string, unquote it
    // If it's function(){...}, extract the body
    let script = if script.starts_with("function") {
        // Extract body between { }
        if let Some(start) = script.find('{') {
            if let Some(end) = script.rfind('}') {
                script[start + 1..end].trim().to_string()
            } else {
                script
            }
        } else {
            script
        }
    } else if script.starts_with('"') || script.starts_with('\'') {
        unquote(&script)
    } else if script.starts_with("()") || script.contains("=>") {
        // Arrow function: () => { ... } or () => expr
        if let Some(arrow_pos) = script.find("=>") {
            let body = script[arrow_pos + 2..].trim();
            if body.starts_with('{') && body.ends_with('}') {
                body[1..body.len() - 1].trim().to_string()
            } else {
                body.to_string()
            }
        } else {
            script
        }
    } else {
        script
    };

    (script, delay)
}

fn handle_storage(expr: &str, st: StorageType, prefix: &str) -> Option<WebApiResult> {
    // .getItem('key')
    if let Some(arg) = expr.strip_prefix(&format!("{}.getItem(", prefix)).and_then(|s| s.strip_suffix(")")) {
        let key = unquote(arg);
        return Some(WebApiResult::with_storage(JsValue::Null, st, StorageOp::GetItem, Some(key), None));
    }
    // .setItem('key', 'value')
    if let Some(args) = expr.strip_prefix(&format!("{}.setItem(", prefix)).and_then(|s| s.strip_suffix(")")) {
        if let Some((k, v)) = args.split_once(',') {
            let key = unquote(k.trim());
            let val = unquote(v.trim());
            return Some(WebApiResult::with_storage(JsValue::Undefined, st, StorageOp::SetItem, Some(key), Some(val)));
        }
    }
    // .removeItem('key')
    if let Some(arg) = expr.strip_prefix(&format!("{}.removeItem(", prefix)).and_then(|s| s.strip_suffix(")")) {
        let key = unquote(arg);
        return Some(WebApiResult::with_storage(JsValue::Undefined, st, StorageOp::RemoveItem, Some(key), None));
    }
    // .clear()
    if expr == format!("{}.clear()", prefix) {
        return Some(WebApiResult::with_storage(JsValue::Undefined, st, StorageOp::Clear, None, None));
    }
    // .length
    if expr == format!("{}.length", prefix) {
        return Some(WebApiResult::with_storage(JsValue::Number(0.0), st, StorageOp::Length, None, None));
    }
    None
}

// === URL helpers ===

fn extract_pathname(url: &str) -> String {
    if let Some(after_scheme) = url.find("://") {
        let rest = &url[after_scheme + 3..];
        if let Some(slash) = rest.find('/') {
            let path = &rest[slash..];
            path.split('?').next().unwrap_or(path).to_string()
        } else {
            "/".to_string()
        }
    } else {
        "/".to_string()
    }
}

fn extract_origin(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        let host = rest.split('/').next().unwrap_or(rest);
        format!("{}{}", &url[..scheme_end + 3], host)
    } else {
        url.to_string()
    }
}

fn extract_search(url: &str) -> String {
    if let Some(q) = url.find('?') {
        url[q..].to_string()
    } else {
        String::new()
    }
}

fn extract_host(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        rest.split('/').next().unwrap_or(rest).to_string()
    } else {
        url.to_string()
    }
}

/// Extract a string value from a simple JSON-like object literal by key name.
fn extract_obj_string(obj: &str, key: &str) -> Option<String> {
    // Look for: key: 'value' or "key": "value"
    let patterns = [
        format!("{}:", key),
        format!("'{}':", key),
        format!("\"{}\":", key),
    ];
    for pat in &patterns {
        if let Some(pos) = obj.find(pat.as_str()) {
            let after = obj[pos + pat.len()..].trim();
            if after.starts_with('\'') || after.starts_with('"') {
                let quote = after.chars().next().unwrap();
                let inner = &after[1..];
                if let Some(end) = inner.find(quote) {
                    return Some(inner[..end].to_string());
                }
            }
        }
    }
    None
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

/// Process a fetch response body into a JsValue response object.
pub fn build_fetch_response(status: u16, body: &str) -> JsValue {
    let mut resp = HashMap::new();
    resp.insert("status".to_string(), JsValue::Number(status as f64));
    resp.insert("ok".to_string(), JsValue::Boolean((200..300).contains(&status)));
    resp.insert("statusText".to_string(), JsValue::String(
        if status == 200 { "OK".to_string() } else { format!("{}", status) }
    ));
    resp.insert("body".to_string(), JsValue::String(body.to_string()));
    resp.insert("text".to_string(), JsValue::String(body.to_string()));

    // Try to parse as JSON
    let json_val = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
        json_to_jsvalue(&parsed)
    } else {
        JsValue::String(body.to_string())
    };
    resp.insert("json".to_string(), json_val);

    JsValue::Object(resp)
}

fn json_to_jsvalue(val: &serde_json::Value) -> JsValue {
    match val {
        serde_json::Value::Null => JsValue::Null,
        serde_json::Value::Bool(b) => JsValue::Boolean(*b),
        serde_json::Value::Number(n) => JsValue::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => JsValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            JsValue::Array(arr.iter().map(json_to_jsvalue).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), json_to_jsvalue(v));
            }
            JsValue::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_set_timeout_simple() {
        let result = eval_web_api("setTimeout(function() { alert(1) }, 1000)", "", 5).unwrap();
        assert_eq!(result.value, JsValue::Number(5.0));
        let (script, delay, is_interval) = result.pending_timer.unwrap();
        assert_eq!(script, "alert(1)");
        assert_eq!(delay, 1000);
        assert!(!is_interval);
    }

    #[test]
    fn eval_set_interval() {
        let result = eval_web_api("setInterval(function() { tick() }, 500)", "", 3).unwrap();
        let (_, delay, is_interval) = result.pending_timer.unwrap();
        assert_eq!(delay, 500);
        assert!(is_interval);
    }

    #[test]
    fn eval_clear_timeout() {
        let result = eval_web_api("clearTimeout(7)", "", 1).unwrap();
        assert_eq!(result.cancel_timer_id, Some(7));
    }

    #[test]
    fn eval_location_href() {
        let result = eval_web_api("window.location.href", "https://example.com/path?q=1", 1).unwrap();
        assert_eq!(result.value, JsValue::String("https://example.com/path?q=1".to_string()));
    }

    #[test]
    fn eval_location_pathname() {
        let result = eval_web_api("window.location.pathname", "https://example.com/foo/bar?x=1", 1).unwrap();
        assert_eq!(result.value, JsValue::String("/foo/bar".to_string()));
    }

    #[test]
    fn eval_location_origin() {
        let result = eval_web_api("window.location.origin", "https://example.com/path", 1).unwrap();
        assert_eq!(result.value, JsValue::String("https://example.com".to_string()));
    }

    #[test]
    fn eval_local_storage_set_item() {
        let result = eval_web_api("localStorage.setItem('key', 'val')", "", 1).unwrap();
        let (st, op, key, val) = result.storage_op.unwrap();
        assert_eq!(st, StorageType::Local);
        assert_eq!(op, StorageOp::SetItem);
        assert_eq!(key, Some("key".to_string()));
        assert_eq!(val, Some("val".to_string()));
    }

    #[test]
    fn eval_fetch_simple() {
        let result = eval_web_api("fetch('https://api.example.com/data')", "", 1).unwrap();
        let (url, method, body, _ct) = result.fetch_request.unwrap();
        assert_eq!(url, "https://api.example.com/data");
        assert_eq!(method, "GET");
        assert!(body.is_none());
    }

    #[test]
    fn eval_console_log() {
        let result = eval_web_api("console.log('hello world')", "", 1).unwrap();
        let (level, msg) = result.console_output.unwrap();
        assert_eq!(level, "log");
        assert_eq!(msg, "hello world");
    }

    #[test]
    fn non_web_api_returns_none() {
        assert!(eval_web_api("var x = 1 + 2", "", 1).is_none());
    }

    #[test]
    fn build_fetch_response_json() {
        let resp = build_fetch_response(200, r#"{"name":"test"}"#);
        if let JsValue::Object(obj) = &resp {
            assert_eq!(obj.get("status"), Some(&JsValue::Number(200.0)));
            assert_eq!(obj.get("ok"), Some(&JsValue::Boolean(true)));
        } else {
            panic!("expected object");
        }
    }
}
