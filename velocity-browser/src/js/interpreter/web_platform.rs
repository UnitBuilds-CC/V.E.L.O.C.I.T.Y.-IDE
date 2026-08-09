//! Web platform APIs for the JS interpreter — performance, history, observers,
//! WebSocket, getComputedStyle, matchMedia, FileReader, crypto.subtle.
//!
//! All mutable state is thread-local so each interpreter instance gets its own
//! isolated environment.

use crate::js::vm::JsValue;
use std::cell::RefCell;
use std::collections::HashMap;

// ── Performance ──────────────────────────────────────────────────────────────

thread_local! {
    static PERF_TIME_ORIGIN: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    static PERF_ENTRIES: RefCell<Vec<PerfEntry>> = const { RefCell::new(Vec::new()) };
    static PERF_MARKS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone)]
struct PerfEntry {
    name: String,
    entry_type: String,
    start_time: f64,
    duration: f64,
}

fn perf_now() -> f64 {
    let origin = PERF_TIME_ORIGIN.with(|o| {
        let v = o.get();
        if v == 0.0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            o.set(now);
            now
        } else {
            v
        }
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    now - origin
}

fn entry_to_js(e: &PerfEntry) -> JsValue {
    let mut map = HashMap::new();
    map.insert("name".to_string(), JsValue::String(e.name.clone()));
    map.insert(
        "entryType".to_string(),
        JsValue::String(e.entry_type.clone()),
    );
    map.insert("startTime".to_string(), JsValue::Number(e.start_time));
    map.insert("duration".to_string(), JsValue::Number(e.duration));
    JsValue::Object(map)
}

pub(super) fn make_performance() -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("Performance".to_string()),
    );
    let origin = PERF_TIME_ORIGIN.with(|o| {
        let v = o.get();
        if v == 0.0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            o.set(now);
            now
        } else {
            v
        }
    });
    map.insert("timeOrigin".to_string(), JsValue::Number(origin));
    JsValue::Object(map)
}

pub(super) fn call_performance_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "now" => JsValue::Number(perf_now()),
        "mark" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let time = perf_now();
            PERF_MARKS.with(|m| {
                m.borrow_mut().insert(name.clone(), time);
            });
            PERF_ENTRIES.with(|e| {
                e.borrow_mut().push(PerfEntry {
                    name,
                    entry_type: "mark".to_string(),
                    start_time: time,
                    duration: 0.0,
                });
            });
            JsValue::Undefined
        }
        "measure" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let start_mark = args
                .get(1)
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let end_mark = args
                .get(2)
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let (start_time, duration) = PERF_MARKS.with(|m| {
                let marks = m.borrow();
                let s = marks.get(&start_mark).copied().unwrap_or(0.0);
                let e = if end_mark.is_empty() {
                    perf_now()
                } else {
                    marks.get(&end_mark).copied().unwrap_or_else(perf_now)
                };
                (s, e - s)
            });
            PERF_ENTRIES.with(|e| {
                e.borrow_mut().push(PerfEntry {
                    name,
                    entry_type: "measure".to_string(),
                    start_time,
                    duration,
                });
            });
            JsValue::Undefined
        }
        "getEntries" => {
            let entries = PERF_ENTRIES.with(|e| e.borrow().iter().map(entry_to_js).collect());
            JsValue::Array(entries)
        }
        "getEntriesByName" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let entries = PERF_ENTRIES.with(|e| {
                e.borrow()
                    .iter()
                    .filter(|p| p.name == name)
                    .map(entry_to_js)
                    .collect()
            });
            JsValue::Array(entries)
        }
        "getEntriesByType" => {
            let t = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let entries = PERF_ENTRIES.with(|e| {
                e.borrow()
                    .iter()
                    .filter(|p| p.entry_type == t)
                    .map(entry_to_js)
                    .collect()
            });
            JsValue::Array(entries)
        }
        "clearMarks" => {
            PERF_MARKS.with(|m| {
                m.borrow_mut().clear();
            });
            PERF_ENTRIES.with(|e| {
                e.borrow_mut().retain(|p| p.entry_type != "mark");
            });
            JsValue::Undefined
        }
        "clearMeasures" => {
            PERF_ENTRIES.with(|e| {
                e.borrow_mut().retain(|p| p.entry_type != "measure");
            });
            JsValue::Undefined
        }
        "clearResourceTimings" => JsValue::Undefined,
        "toJSON" => {
            let mut map = HashMap::new();
            let origin = PERF_TIME_ORIGIN.with(|o| o.get());
            map.insert("timeOrigin".to_string(), JsValue::Number(origin));
            let entries = PERF_ENTRIES.with(|e| e.borrow().iter().map(entry_to_js).collect());
            map.insert("entries".to_string(), JsValue::Array(entries));
            JsValue::Object(map)
        }
        _ => JsValue::Undefined,
    }
}

// ── History ──────────────────────────────────────────────────────────────────

thread_local! {
    static HISTORY_STACK: RefCell<Vec<HistoryEntry>> = RefCell::new(vec![HistoryEntry {
        state: JsValue::Null,
        url: "https://localhost/".to_string(),
    }]);
    static HISTORY_INDEX: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    state: JsValue,
    url: String,
}

pub(super) fn make_history() -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("History".to_string()),
    );
    let len = HISTORY_STACK.with(|s| s.borrow().len());
    map.insert("length".to_string(), JsValue::Number(len as f64));
    let state = HISTORY_STACK.with(|s| {
        let idx = HISTORY_INDEX.with(|i| i.get());
        s.borrow()
            .get(idx)
            .map(|e| e.state.clone())
            .unwrap_or(JsValue::Null)
    });
    map.insert("state".to_string(), state);
    JsValue::Object(map)
}

pub(super) fn call_history_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "pushState" => {
            let state = args.first().cloned().unwrap_or(JsValue::Null);
            let url = args
                .get(2)
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            HISTORY_STACK.with(|s| {
                let mut stack = s.borrow_mut();
                let idx = HISTORY_INDEX.with(|i| i.get());
                // Truncate forward history.
                stack.truncate(idx + 1);
                stack.push(HistoryEntry { state, url });
                HISTORY_INDEX.with(|i| i.set(stack.len() - 1));
            });
            JsValue::Undefined
        }
        "replaceState" => {
            let state = args.first().cloned().unwrap_or(JsValue::Null);
            let url = args
                .get(2)
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            HISTORY_STACK.with(|s| {
                let mut stack = s.borrow_mut();
                let idx = HISTORY_INDEX.with(|i| i.get());
                if let Some(entry) = stack.get_mut(idx) {
                    entry.state = state;
                    if !url.is_empty() {
                        entry.url = url;
                    }
                }
            });
            JsValue::Undefined
        }
        "back" => {
            HISTORY_INDEX.with(|i| {
                let idx = i.get();
                if idx > 0 {
                    i.set(idx - 1);
                }
            });
            JsValue::Undefined
        }
        "forward" => {
            HISTORY_STACK.with(|s| {
                let len = s.borrow().len();
                HISTORY_INDEX.with(|i| {
                    let idx = i.get();
                    if idx + 1 < len {
                        i.set(idx + 1);
                    }
                });
            });
            JsValue::Undefined
        }
        "go" => {
            let delta = args
                .first()
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(0.0) as i32;
            HISTORY_STACK.with(|s| {
                let len = s.borrow().len() as i32;
                HISTORY_INDEX.with(|i| {
                    let idx = i.get() as i32;
                    let target = (idx + delta).clamp(0, len - 1);
                    i.set(target as usize);
                });
            });
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    }
}

pub(super) fn history_length() -> JsValue {
    let len = HISTORY_STACK.with(|s| s.borrow().len());
    JsValue::Number(len as f64)
}

pub(super) fn history_state() -> JsValue {
    HISTORY_STACK.with(|s| {
        let idx = HISTORY_INDEX.with(|i| i.get());
        s.borrow()
            .get(idx)
            .map(|e| e.state.clone())
            .unwrap_or(JsValue::Null)
    })
}

// ── IntersectionObserver ─────────────────────────────────────────────────────

pub(super) fn make_intersection_observer(callback: JsValue, _options: Option<&JsValue>) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("IntersectionObserver".to_string()),
    );
    map.insert("__callback__".to_string(), callback);
    map.insert("__targets__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_intersection_observer_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "observe" => {
            let mut m = map.clone();
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let Some(JsValue::Array(targets)) = m.get_mut("__targets__") {
                targets.push(target);
            }
            JsValue::Object(m)
        }
        "unobserve" => {
            let mut m = map.clone();
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let Some(JsValue::Array(targets)) = m.get_mut("__targets__") {
                targets.retain(|t| t != &target);
            }
            JsValue::Object(m)
        }
        "disconnect" => {
            let mut m = map.clone();
            m.insert("__targets__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Object(m)
        }
        "takeRecords" => JsValue::Array(Vec::new()),
        _ => JsValue::Undefined,
    }
}

// ── ResizeObserver ───────────────────────────────────────────────────────────

pub(super) fn make_resize_observer(callback: JsValue) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("ResizeObserver".to_string()),
    );
    map.insert("__callback__".to_string(), callback);
    map.insert("__targets__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_resize_observer_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "observe" => {
            let mut m = map.clone();
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let Some(JsValue::Array(targets)) = m.get_mut("__targets__") {
                targets.push(target);
            }
            JsValue::Object(m)
        }
        "unobserve" => {
            let mut m = map.clone();
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let Some(JsValue::Array(targets)) = m.get_mut("__targets__") {
                targets.retain(|t| t != &target);
            }
            JsValue::Object(m)
        }
        "disconnect" => {
            let mut m = map.clone();
            m.insert("__targets__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Object(m)
        }
        _ => JsValue::Undefined,
    }
}

// ── WebSocket ────────────────────────────────────────────────────────────────

pub(super) fn make_web_socket(url: &str, _protocols: Option<&JsValue>) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("WebSocket".to_string()),
    );
    map.insert("url".to_string(), JsValue::String(url.to_string()));
    map.insert("readyState".to_string(), JsValue::Number(0.0)); // CONNECTING
    map.insert("bufferedAmount".to_string(), JsValue::Number(0.0));
    map.insert("__sent__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_web_socket_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "send" => {
            let mut m = map.clone();
            let data = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let Some(JsValue::Array(sent)) = m.get_mut("__sent__") {
                sent.push(data);
            }
            JsValue::Object(m)
        }
        "close" => {
            let mut m = map.clone();
            m.insert("readyState".to_string(), JsValue::Number(3.0)); // CLOSED
            JsValue::Object(m)
        }
        _ => JsValue::Undefined,
    }
}

// ── getComputedStyle ─────────────────────────────────────────────────────────

pub(super) fn get_computed_style(_element: &JsValue, _pseudo: Option<&JsValue>) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("CSSStyleDeclaration".to_string()),
    );
    // Provide sensible defaults for common properties.
    let defaults = [
        ("display", "block"),
        ("visibility", "visible"),
        ("opacity", "1"),
        ("color", "rgb(0, 0, 0)"),
        ("backgroundColor", "rgba(0, 0, 0, 0)"),
        ("fontSize", "16px"),
        ("fontFamily", "sans-serif"),
        ("fontWeight", "400"),
        ("lineHeight", "normal"),
        ("margin", "0px"),
        ("padding", "0px"),
        ("border", "0px none rgb(0, 0, 0)"),
        ("width", "auto"),
        ("height", "auto"),
        ("position", "static"),
        ("top", "auto"),
        ("left", "auto"),
        ("right", "auto"),
        ("bottom", "auto"),
        ("zIndex", "auto"),
        ("overflow", "visible"),
        ("cursor", "auto"),
        ("textAlign", "start"),
        ("textDecoration", "none"),
        ("textTransform", "none"),
        ("boxSizing", "content-box"),
        ("flexDirection", "row"),
        ("justifyContent", "flex-start"),
        ("alignItems", "stretch"),
    ];
    for (k, v) in defaults {
        map.insert(k.to_string(), JsValue::String(v.to_string()));
    }
    JsValue::Object(map)
}

pub(super) fn call_css_style_declaration_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "getPropertyValue" => {
            let prop = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            // Convert kebab-case to camelCase for lookup.
            let camel = kebab_to_camel(&prop);
            map.get(&camel)
                .cloned()
                .unwrap_or(JsValue::String(String::new()))
        }
        "getPropertyPriority" => JsValue::String(String::new()),
        "item" => {
            let idx = args
                .first()
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(0.0) as usize;
            let keys: Vec<&String> = {
                let mut ks: Vec<&String> = map.keys().filter(|k| !k.starts_with("__")).collect();
                ks.sort();
                ks
            };
            keys.get(idx)
                .map(|k| JsValue::String((*k).clone()))
                .unwrap_or(JsValue::String(String::new()))
        }
        _ => JsValue::Undefined,
    }
}

fn kebab_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper = false;
    for c in s.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

// ── matchMedia ───────────────────────────────────────────────────────────────

pub(super) fn match_media(query: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("MediaQueryList".to_string()),
    );
    map.insert("media".to_string(), JsValue::String(query.to_string()));
    // Evaluate common media queries pragmatically.
    let matches = eval_media_query(query);
    map.insert("matches".to_string(), JsValue::Boolean(matches));
    JsValue::Object(map)
}

fn eval_media_query(query: &str) -> bool {
    let q = query.to_lowercase();
    // Default viewport: 1280x720, light mode, screen.
    if q.contains("prefers-color-scheme: dark") {
        return false;
    }
    if q.contains("prefers-color-scheme: light") {
        return true;
    }
    if q.contains("prefers-reduced-motion: reduce") {
        return false;
    }
    if q.contains("prefers-reduced-motion: no-preference") {
        return true;
    }
    if q.contains("prefers-contrast: more") {
        return false;
    }
    if q.contains("color-gamut: p3") {
        return false;
    }
    if q.contains("color-gamut: srgb") {
        return true;
    }
    if q.contains("hover: hover") {
        return true;
    }
    if q.contains("hover: none") {
        return false;
    }
    if q.contains("pointer: fine") {
        return true;
    }
    if q.contains("pointer: coarse") {
        return false;
    }
    if q.contains("any-pointer: fine") {
        return true;
    }
    // Width queries.
    if let Some(w) = extract_px(&q, "min-width") {
        return 1280.0 >= w;
    }
    if let Some(w) = extract_px(&q, "max-width") {
        return 1280.0 <= w;
    }
    if let Some(h) = extract_px(&q, "min-height") {
        return 720.0 >= h;
    }
    if let Some(h) = extract_px(&q, "max-height") {
        return 720.0 <= h;
    }
    if q.contains("screen") {
        return true;
    }
    if q.contains("print") {
        return false;
    }
    true
}

fn extract_px(q: &str, feature: &str) -> Option<f64> {
    if let Some(pos) = q.find(feature) {
        let rest = &q[pos + feature.len()..];
        // Skip ": " or ":"
        let rest = rest.trim_start_matches([':', ' ']);
        // Extract number.
        let num_str: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        return num_str.parse::<f64>().ok();
    }
    None
}

pub(super) fn call_media_query_list_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    _args: &[JsValue],
) -> JsValue {
    match method {
        "addListener" | "addEventListener" | "removeListener" | "removeEventListener" => {
            JsValue::Undefined
        }
        "dispatchEvent" => JsValue::Boolean(true),
        _ => {
            // Property access fallback.
            map.get(method).cloned().unwrap_or(JsValue::Undefined)
        }
    }
}

// ── FileReader ───────────────────────────────────────────────────────────────

pub(super) fn make_file_reader() -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("FileReader".to_string()),
    );
    map.insert("readyState".to_string(), JsValue::Number(0.0)); // EMPTY
    map.insert("result".to_string(), JsValue::Null);
    map.insert("error".to_string(), JsValue::Null);
    JsValue::Object(map)
}

pub(super) fn call_file_reader_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "readAsText" => {
            let mut m = map.clone();
            let blob = args.first();
            let text = extract_blob_text(blob);
            m.insert("readyState".to_string(), JsValue::Number(2.0)); // DONE
            m.insert("result".to_string(), JsValue::String(text));
            JsValue::Object(m)
        }
        "readAsDataURL" => {
            let mut m = map.clone();
            let blob = args.first();
            let text = extract_blob_text(blob);
            let mime = extract_blob_mime(blob);
            let b64 = base64_encode(text.as_bytes());
            let data_url = format!(
                "data:{};base64,{}",
                if mime.is_empty() {
                    "application/octet-stream"
                } else {
                    &mime
                },
                b64
            );
            m.insert("readyState".to_string(), JsValue::Number(2.0));
            m.insert("result".to_string(), JsValue::String(data_url));
            JsValue::Object(m)
        }
        "readAsArrayBuffer" => {
            let mut m = map.clone();
            let blob = args.first();
            let text = extract_blob_text(blob);
            let bytes: Vec<JsValue> = text
                .as_bytes()
                .iter()
                .map(|b| JsValue::Number(*b as f64))
                .collect();
            let mut buf = HashMap::new();
            buf.insert(
                "__type__".to_string(),
                JsValue::String("ArrayBuffer".to_string()),
            );
            buf.insert("__data__".to_string(), JsValue::Array(bytes));
            m.insert("readyState".to_string(), JsValue::Number(2.0));
            m.insert("result".to_string(), JsValue::Object(buf));
            JsValue::Object(m)
        }
        "abort" => {
            let mut m = map.clone();
            m.insert("readyState".to_string(), JsValue::Number(0.0));
            m.insert("result".to_string(), JsValue::Null);
            JsValue::Object(m)
        }
        _ => JsValue::Undefined,
    }
}

fn extract_blob_text(blob: Option<&JsValue>) -> String {
    match blob {
        Some(JsValue::Object(m)) => {
            if let Some(JsValue::String(s)) = m.get("__data__") {
                return s.clone();
            }
            if let Some(JsValue::Array(bytes)) = m.get("__data__") {
                return bytes
                    .iter()
                    .filter_map(|v| {
                        if let JsValue::Number(n) = v {
                            Some(*n as u8)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<u8>>()
                    .iter()
                    .map(|b| *b as char)
                    .collect();
            }
            String::new()
        }
        Some(JsValue::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn extract_blob_mime(blob: Option<&JsValue>) -> String {
    match blob {
        Some(JsValue::Object(m)) => m
            .get("__mime__")
            .map(crate::js::interpreter::coercion::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[((triple >> 18) & 0x3F) as usize] as char);
        out.push(B64[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ── crypto.subtle ────────────────────────────────────────────────────────────

pub(super) fn call_subtle_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "digest" => {
            // Pragmatic: return a fixed-length hash-like buffer.
            let algo = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let data = args.get(1);
            let hash = simple_hash(data);
            let len = if algo.to_uppercase().contains("SHA-256") {
                32
            } else if algo.to_uppercase().contains("SHA-384") {
                48
            } else if algo.to_uppercase().contains("SHA-512") {
                64
            } else if algo.to_uppercase().contains("SHA-1") {
                20
            } else {
                32
            };
            let bytes: Vec<JsValue> = (0..len)
                .map(|i| JsValue::Number(hash[i % hash.len()] as f64))
                .collect();
            let mut buf = HashMap::new();
            buf.insert(
                "__type__".to_string(),
                JsValue::String("ArrayBuffer".to_string()),
            );
            buf.insert("__data__".to_string(), JsValue::Array(bytes));
            JsValue::Object(buf)
        }
        "encrypt" | "decrypt" => {
            // Pragmatic: XOR with key bytes (reversible, not secure, but functional).
            let _algo = args.first();
            let key = args.get(1);
            let data = args.get(2);
            let key_bytes = extract_key_bytes(key);
            let data_bytes = extract_buffer_bytes(data);
            let result: Vec<JsValue> = data_bytes
                .iter()
                .enumerate()
                .map(|(i, b)| JsValue::Number((b ^ key_bytes[i % key_bytes.len().max(1)]) as f64))
                .collect();
            let mut buf = HashMap::new();
            buf.insert(
                "__type__".to_string(),
                JsValue::String("ArrayBuffer".to_string()),
            );
            buf.insert("__data__".to_string(), JsValue::Array(result));
            JsValue::Object(buf)
        }
        "sign" | "verify" => {
            // Pragmatic: return a hash-based signature or true.
            if method == "verify" {
                return JsValue::Boolean(true);
            }
            let data = args.get(2);
            let hash = simple_hash(data);
            let bytes: Vec<JsValue> = hash.iter().map(|b| JsValue::Number(*b as f64)).collect();
            let mut buf = HashMap::new();
            buf.insert(
                "__type__".to_string(),
                JsValue::String("ArrayBuffer".to_string()),
            );
            buf.insert("__data__".to_string(), JsValue::Array(bytes));
            JsValue::Object(buf)
        }
        "importKey" => {
            let mut map = HashMap::new();
            map.insert(
                "__type__".to_string(),
                JsValue::String("CryptoKey".to_string()),
            );
            map.insert("type".to_string(), JsValue::String("secret".to_string()));
            map.insert("extractable".to_string(), JsValue::Boolean(true));
            let key_data = args.get(2).cloned().unwrap_or(JsValue::Undefined);
            map.insert("__key__".to_string(), key_data);
            JsValue::Object(map)
        }
        "exportKey" => {
            let key = args.get(1);
            if let Some(JsValue::Object(m)) = key {
                return m.get("__key__").cloned().unwrap_or(JsValue::Undefined);
            }
            JsValue::Undefined
        }
        "generateKey" => {
            let mut map = HashMap::new();
            map.insert(
                "__type__".to_string(),
                JsValue::String("CryptoKeyPair".to_string()),
            );
            let mut pub_key = HashMap::new();
            pub_key.insert(
                "__type__".to_string(),
                JsValue::String("CryptoKey".to_string()),
            );
            pub_key.insert("type".to_string(), JsValue::String("public".to_string()));
            let mut priv_key = HashMap::new();
            priv_key.insert(
                "__type__".to_string(),
                JsValue::String("CryptoKey".to_string()),
            );
            priv_key.insert("type".to_string(), JsValue::String("private".to_string()));
            map.insert("publicKey".to_string(), JsValue::Object(pub_key));
            map.insert("privateKey".to_string(), JsValue::Object(priv_key));
            JsValue::Object(map)
        }
        "deriveBits" | "deriveKey" => {
            let mut buf = HashMap::new();
            buf.insert(
                "__type__".to_string(),
                JsValue::String("ArrayBuffer".to_string()),
            );
            let len = if method == "deriveBits" { 32 } else { 16 };
            let bytes: Vec<JsValue> = (0..len)
                .map(|i| JsValue::Number((i * 7 + 13) as f64))
                .collect();
            buf.insert("__data__".to_string(), JsValue::Array(bytes));
            JsValue::Object(buf)
        }
        _ => JsValue::Undefined,
    }
}

fn simple_hash(data: Option<&JsValue>) -> Vec<u8> {
    let bytes = extract_buffer_bytes(data);
    // FNV-1a inspired hash, expanded to 64 bytes.
    let mut hash = [0u8; 64];
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in &bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for (i, slot) in hash.iter_mut().enumerate() {
        h ^= (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
        h = h.wrapping_mul(0x100000001b3);
        *slot = (h >> 24) as u8;
    }
    hash.to_vec()
}

fn extract_buffer_bytes(data: Option<&JsValue>) -> Vec<u8> {
    match data {
        Some(JsValue::Object(m)) => {
            if let Some(JsValue::Array(arr)) = m.get("__data__") {
                return arr
                    .iter()
                    .filter_map(|v| {
                        if let JsValue::Number(n) = v {
                            Some(*n as u8)
                        } else {
                            None
                        }
                    })
                    .collect();
            }
        }
        Some(JsValue::Array(arr)) => {
            return arr
                .iter()
                .filter_map(|v| {
                    if let JsValue::Number(n) = v {
                        Some(*n as u8)
                    } else {
                        None
                    }
                })
                .collect();
        }
        Some(JsValue::String(s)) => return s.as_bytes().to_vec(),
        _ => {}
    }
    Vec::new()
}

fn extract_key_bytes(key: Option<&JsValue>) -> Vec<u8> {
    match key {
        Some(JsValue::Object(m)) => {
            if let Some(JsValue::String(s)) = m.get("__key__") {
                return s.as_bytes().to_vec();
            }
            if let Some(JsValue::Array(arr)) = m.get("__key__") {
                return arr
                    .iter()
                    .filter_map(|v| {
                        if let JsValue::Number(n) = v {
                            Some(*n as u8)
                        } else {
                            None
                        }
                    })
                    .collect();
            }
            if let Some(JsValue::Array(arr)) = m.get("__data__") {
                return arr
                    .iter()
                    .filter_map(|v| {
                        if let JsValue::Number(n) = v {
                            Some(*n as u8)
                        } else {
                            None
                        }
                    })
                    .collect();
            }
        }
        Some(JsValue::String(s)) => return s.as_bytes().to_vec(),
        _ => {}
    }
    vec![0x42] // fallback single byte
}

// ── CSSStyleSheet (constructable) ────────────────────────────────────────────

pub(super) fn make_css_style_sheet() -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("CSSStyleSheet".to_string()),
    );
    map.insert("__rules__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_css_style_sheet_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "replace" | "replaceSync" => {
            let mut m = map.clone();
            let css = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let rules: Vec<JsValue> = css
                .split('}')
                .filter(|s| s.contains('{'))
                .map(|s| JsValue::String(format!("{}}}", s.trim())))
                .collect();
            m.insert("__rules__".to_string(), JsValue::Array(rules));
            if method == "replace" {
                // Returns a promise-like resolved value.
                let mut p = HashMap::new();
                p.insert(
                    "__type__".to_string(),
                    JsValue::String("Promise".to_string()),
                );
                p.insert("__resolved__".to_string(), JsValue::Object(m));
                JsValue::Object(p)
            } else {
                JsValue::Object(m)
            }
        }
        "insertRule" => {
            let mut m = map.clone();
            let rule = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            if let Some(JsValue::Array(rules)) = m.get_mut("__rules__") {
                let idx = args
                    .get(1)
                    .map(crate::js::interpreter::coercion::to_number)
                    .unwrap_or(rules.len() as f64) as usize;
                let pos = idx.min(rules.len());
                rules.insert(pos, JsValue::String(rule));
            }
            JsValue::Number(0.0)
        }
        "deleteRule" => {
            let mut m = map.clone();
            let idx = args
                .first()
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(0.0) as usize;
            if let Some(JsValue::Array(rules)) = m.get_mut("__rules__") {
                if idx < rules.len() {
                    rules.remove(idx);
                }
            }
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    }
}

// ── DOMRect ──────────────────────────────────────────────────────────────────

pub(super) fn make_dom_rect(x: f64, y: f64, w: f64, h: f64) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("DOMRect".to_string()),
    );
    map.insert("x".to_string(), JsValue::Number(x));
    map.insert("y".to_string(), JsValue::Number(y));
    map.insert("width".to_string(), JsValue::Number(w));
    map.insert("height".to_string(), JsValue::Number(h));
    map.insert("left".to_string(), JsValue::Number(x));
    map.insert("top".to_string(), JsValue::Number(y));
    map.insert("right".to_string(), JsValue::Number(x + w));
    map.insert("bottom".to_string(), JsValue::Number(y + h));
    JsValue::Object(map)
}

pub(super) fn call_dom_rect_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    _args: &[JsValue],
) -> JsValue {
    match method {
        "toJSON" => JsValue::Object(map.clone()),
        _ => map.get(method).cloned().unwrap_or(JsValue::Undefined),
    }
}

pub(super) fn make_dom_matrix(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("DOMMatrix".to_string()),
    );
    map.insert("a".to_string(), JsValue::Number(a));
    map.insert("b".to_string(), JsValue::Number(b));
    map.insert("c".to_string(), JsValue::Number(c));
    map.insert("d".to_string(), JsValue::Number(d));
    map.insert("e".to_string(), JsValue::Number(e));
    map.insert("f".to_string(), JsValue::Number(f));
    map.insert("m11".to_string(), JsValue::Number(a));
    map.insert("m12".to_string(), JsValue::Number(b));
    map.insert("m21".to_string(), JsValue::Number(c));
    map.insert("m22".to_string(), JsValue::Number(d));
    map.insert("m41".to_string(), JsValue::Number(e));
    map.insert("m42".to_string(), JsValue::Number(f));
    map.insert("is2D".to_string(), JsValue::Boolean(true));
    map.insert(
        "isIdentity".to_string(),
        JsValue::Boolean(a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 && e == 0.0 && f == 0.0),
    );
    JsValue::Object(map)
}

// ── Navigator extensions (clipboard, permissions, geolocation) ──────────────

pub(super) fn call_navigator_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "clipboard" => {
            let mut map = HashMap::new();
            map.insert(
                "__type__".to_string(),
                JsValue::String("Clipboard".to_string()),
            );
            JsValue::Object(map)
        }
        "permissions" => {
            let mut map = HashMap::new();
            map.insert(
                "__type__".to_string(),
                JsValue::String("Permissions".to_string()),
            );
            JsValue::Object(map)
        }
        "geolocation" => {
            let mut map = HashMap::new();
            map.insert(
                "__type__".to_string(),
                JsValue::String("Geolocation".to_string()),
            );
            JsValue::Object(map)
        }
        "sendBeacon" => JsValue::Boolean(true),
        "vibrate" => JsValue::Boolean(true),
        "getBattery" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            let mut battery = HashMap::new();
            battery.insert(
                "__type__".to_string(),
                JsValue::String("BatteryManager".to_string()),
            );
            battery.insert("charging".to_string(), JsValue::Boolean(true));
            battery.insert("chargingTime".to_string(), JsValue::Number(0.0));
            battery.insert(
                "dischargingTime".to_string(),
                JsValue::Number(f64::INFINITY),
            );
            battery.insert("level".to_string(), JsValue::Number(1.0));
            p.insert("__resolved__".to_string(), JsValue::Object(battery));
            JsValue::Object(p)
        }
        "getGamepads" => JsValue::Array(Vec::new()),
        "javaEnabled" => JsValue::Boolean(false),
        "connection" => {
            let mut conn = HashMap::new();
            conn.insert(
                "__type__".to_string(),
                JsValue::String("NetworkInformation".to_string()),
            );
            conn.insert(
                "effectiveType".to_string(),
                JsValue::String("4g".to_string()),
            );
            conn.insert("downlink".to_string(), JsValue::Number(10.0));
            conn.insert("rtt".to_string(), JsValue::Number(50.0));
            conn.insert("saveData".to_string(), JsValue::Boolean(false));
            conn.insert("type".to_string(), JsValue::String("wifi".to_string()));
            JsValue::Object(conn)
        }
        "storage" => {
            let mut storage = HashMap::new();
            storage.insert(
                "__type__".to_string(),
                JsValue::String("StorageManager".to_string()),
            );
            JsValue::Object(storage)
        }
        "mediaDevices" => {
            let mut md = HashMap::new();
            md.insert(
                "__type__".to_string(),
                JsValue::String("MediaDevices".to_string()),
            );
            JsValue::Object(md)
        }
        "wakeLock" => {
            let mut wl = HashMap::new();
            wl.insert(
                "__type__".to_string(),
                JsValue::String("WakeLock".to_string()),
            );
            JsValue::Object(wl)
        }
        "requestWakeLock" | "requestMediaKeySystemAccess" | "getUserMedia" | "getDisplayMedia" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        _ => {
            let _ = args;
            JsValue::Undefined
        }
    }
}

pub(super) fn call_clipboard_method(method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "readText" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::String(String::new()));
            JsValue::Object(p)
        }
        "writeText" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        "read" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Object(p)
        }
        "write" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        _ => JsValue::Undefined,
    }
}

pub(super) fn call_permissions_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "query" => {
            let _desc = args.first();
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            let mut status = HashMap::new();
            status.insert(
                "__type__".to_string(),
                JsValue::String("PermissionStatus".to_string()),
            );
            status.insert("state".to_string(), JsValue::String("granted".to_string()));
            p.insert("__resolved__".to_string(), JsValue::Object(status));
            JsValue::Object(p)
        }
        _ => JsValue::Undefined,
    }
}

pub(super) fn call_geolocation_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "getCurrentPosition" => {
            // Callback-based: invoke success callback with position.
            let _success = args.first();
            let mut position = HashMap::new();
            position.insert(
                "__type__".to_string(),
                JsValue::String("GeolocationPosition".to_string()),
            );
            let mut coords = HashMap::new();
            coords.insert("latitude".to_string(), JsValue::Number(0.0));
            coords.insert("longitude".to_string(), JsValue::Number(0.0));
            coords.insert("altitude".to_string(), JsValue::Null);
            coords.insert("accuracy".to_string(), JsValue::Number(0.0));
            coords.insert("altitudeAccuracy".to_string(), JsValue::Null);
            coords.insert("heading".to_string(), JsValue::Null);
            coords.insert("speed".to_string(), JsValue::Null);
            position.insert("coords".to_string(), JsValue::Object(coords));
            position.insert("timestamp".to_string(), JsValue::Number(perf_now()));
            JsValue::Object(position)
        }
        "watchPosition" => JsValue::Number(1.0),
        "clearWatch" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

// ── Cache API ────────────────────────────────────────────────────────────────

thread_local! {
    static CACHES: RefCell<HashMap<String, Vec<(String, JsValue)>>> = RefCell::new(HashMap::new());
}

pub(super) fn call_caches_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "open" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            CACHES.with(|c| {
                c.borrow_mut().entry(name.clone()).or_default();
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            let mut cache = HashMap::new();
            cache.insert("__type__".to_string(), JsValue::String("Cache".to_string()));
            cache.insert("__name__".to_string(), JsValue::String(name));
            p.insert("__resolved__".to_string(), JsValue::Object(cache));
            JsValue::Object(p)
        }
        "has" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let exists = CACHES.with(|c| c.borrow().contains_key(&name));
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Boolean(exists));
            JsValue::Object(p)
        }
        "delete" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let existed = CACHES.with(|c| c.borrow_mut().remove(&name).is_some());
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Boolean(existed));
            JsValue::Object(p)
        }
        "keys" => {
            let keys: Vec<JsValue> = CACHES.with(|c| {
                c.borrow()
                    .keys()
                    .map(|k| JsValue::String(k.clone()))
                    .collect()
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Array(keys));
            JsValue::Object(p)
        }
        "match" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        _ => JsValue::Undefined,
    }
}

pub(super) fn call_cache_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    let name = map
        .get("__name__")
        .map(crate::js::interpreter::coercion::to_string)
        .unwrap_or_default();
    match method {
        "put" => {
            let url = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let response = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            CACHES.with(|c| {
                let mut caches = c.borrow_mut();
                if let Some(entries) = caches.get_mut(&name) {
                    entries.retain(|(u, _)| u != &url);
                    entries.push((url, response));
                }
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        "match" => {
            let url = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let result = CACHES.with(|c| {
                c.borrow().get(&name).and_then(|entries| {
                    entries
                        .iter()
                        .find(|(u, _)| u == &url)
                        .map(|(_, r)| r.clone())
                })
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert(
                "__resolved__".to_string(),
                result.unwrap_or(JsValue::Undefined),
            );
            JsValue::Object(p)
        }
        "delete" => {
            let url = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let deleted = CACHES.with(|c| {
                let mut caches = c.borrow_mut();
                if let Some(entries) = caches.get_mut(&name) {
                    let before = entries.len();
                    entries.retain(|(u, _)| u != &url);
                    entries.len() < before
                } else {
                    false
                }
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Boolean(deleted));
            JsValue::Object(p)
        }
        "keys" => {
            let keys: Vec<JsValue> = CACHES.with(|c| {
                c.borrow()
                    .get(&name)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|(u, _)| JsValue::String(u.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Array(keys));
            JsValue::Object(p)
        }
        "addAll" | "add" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Undefined);
            JsValue::Object(p)
        }
        _ => JsValue::Undefined,
    }
}

// ── Worker ───────────────────────────────────────────────────────────────────

pub(super) fn make_worker(script_url: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("Worker".to_string()),
    );
    map.insert(
        "__script_url__".to_string(),
        JsValue::String(script_url.to_string()),
    );
    map.insert("__posted__".to_string(), JsValue::Array(Vec::new()));
    map.insert("__terminated__".to_string(), JsValue::Boolean(false));
    JsValue::Object(map)
}

pub(super) fn call_worker_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "postMessage" => {
            let mut m = map.clone();
            let msg = args.first().cloned().unwrap_or(JsValue::Undefined);
            if let Some(JsValue::Array(posted)) = m.get_mut("__posted__") {
                posted.push(msg);
            }
            JsValue::Object(m)
        }
        "terminate" => {
            let mut m = map.clone();
            m.insert("__terminated__".to_string(), JsValue::Boolean(true));
            JsValue::Object(m)
        }
        "addEventListener" | "removeEventListener" => JsValue::Undefined,
        "dispatchEvent" => JsValue::Boolean(true),
        _ => JsValue::Undefined,
    }
}

pub(super) fn make_shared_worker(script_url: &str) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("SharedWorker".to_string()),
    );
    map.insert(
        "__script_url__".to_string(),
        JsValue::String(script_url.to_string()),
    );
    let mut port = HashMap::new();
    port.insert(
        "__type__".to_string(),
        JsValue::String("MessagePort".to_string()),
    );
    port.insert("__posted__".to_string(), JsValue::Array(Vec::new()));
    map.insert("port".to_string(), JsValue::Object(port));
    JsValue::Object(map)
}

// ── ServiceWorker (navigator.serviceWorker) ──────────────────────────────────

pub(super) fn make_service_worker_container() -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("ServiceWorkerContainer".to_string()),
    );
    map.insert("ready".to_string(), {
        let mut p = HashMap::new();
        p.insert(
            "__type__".to_string(),
            JsValue::String("Promise".to_string()),
        );
        let mut reg = HashMap::new();
        reg.insert(
            "__type__".to_string(),
            JsValue::String("ServiceWorkerRegistration".to_string()),
        );
        reg.insert(
            "scope".to_string(),
            JsValue::String("https://localhost/".to_string()),
        );
        p.insert("__resolved__".to_string(), JsValue::Object(reg));
        JsValue::Object(p)
    });
    JsValue::Object(map)
}

pub(super) fn call_service_worker_container_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "register" => {
            let _url = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            let mut reg = HashMap::new();
            reg.insert(
                "__type__".to_string(),
                JsValue::String("ServiceWorkerRegistration".to_string()),
            );
            reg.insert(
                "scope".to_string(),
                JsValue::String("https://localhost/".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Object(reg));
            JsValue::Object(p)
        }
        "getRegistration" | "getRegistrations" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert(
                "__resolved__".to_string(),
                if method == "getRegistrations" {
                    JsValue::Array(Vec::new())
                } else {
                    JsValue::Undefined
                },
            );
            JsValue::Object(p)
        }
        "unregister" => {
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Boolean(true));
            JsValue::Object(p)
        }
        "addEventListener" | "removeEventListener" => JsValue::Undefined,
        _ => JsValue::Undefined,
    }
}

// ── PerformanceObserver ──────────────────────────────────────────────────────

pub(super) fn make_performance_observer(callback: JsValue) -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("PerformanceObserver".to_string()),
    );
    map.insert("__callback__".to_string(), callback);
    map.insert("__entry_types__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_performance_observer_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "observe" => {
            let mut m = map.clone();
            if let Some(JsValue::Object(opts)) = args.first() {
                if let Some(JsValue::String(ty)) = opts.get("entryTypes") {
                    m.insert("__entry_types__".to_string(), JsValue::String(ty.clone()));
                } else if let Some(JsValue::Array(types)) = opts.get("entryTypes") {
                    m.insert("__entry_types__".to_string(), JsValue::Array(types.clone()));
                }
                if let Some(JsValue::String(ty)) = opts.get("type") {
                    m.insert("__entry_types__".to_string(), JsValue::String(ty.clone()));
                }
            }
            JsValue::Object(m)
        }
        "disconnect" => JsValue::Undefined,
        "takeRecords" => JsValue::Array(Vec::new()),
        _ => JsValue::Undefined,
    }
}

pub(super) fn performance_observer_supported_entry_types() -> JsValue {
    JsValue::Array(vec![
        JsValue::String("mark".to_string()),
        JsValue::String("measure".to_string()),
        JsValue::String("navigation".to_string()),
        JsValue::String("resource".to_string()),
        JsValue::String("paint".to_string()),
        JsValue::String("longtask".to_string()),
        JsValue::String("largest-contentful-paint".to_string()),
        JsValue::String("layout-shift".to_string()),
        JsValue::String("first-input".to_string()),
        JsValue::String("element".to_string()),
    ])
}

// ── IndexedDB (functional stub with in-memory storage) ────────────────────────

thread_local! {
    static IDB_DATABASES: RefCell<HashMap<String, HashMap<String, JsValue>>> = RefCell::new(HashMap::new());
}

pub(super) fn make_indexed_db() -> JsValue {
    let mut map = HashMap::new();
    map.insert(
        "__type__".to_string(),
        JsValue::String("IDBFactory".to_string()),
    );
    JsValue::Object(map)
}

pub(super) fn call_indexed_db_method(method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "open" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let version = args
                .get(1)
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(1.0);

            // Create database if it doesn't exist
            IDB_DATABASES.with(|dbs| {
                let mut dbs = dbs.borrow_mut();
                if !dbs.contains_key(&name) {
                    dbs.insert(name.clone(), HashMap::new());
                }
            });

            // Create IDBOpenDBRequest with event handlers
            let mut request = HashMap::new();
            request.insert(
                "__type__".to_string(),
                JsValue::String("IDBOpenDBRequest".to_string()),
            );
            request.insert("__db_name__".to_string(), JsValue::String(name.clone()));
            request.insert("__version__".to_string(), JsValue::Number(version));
            request.insert(
                "readyState".to_string(),
                JsValue::String("done".to_string()),
            );
            request.insert("error".to_string(), JsValue::Null);

            // Create the database object
            let mut db = HashMap::new();
            db.insert(
                "__type__".to_string(),
                JsValue::String("IDBDatabase".to_string()),
            );
            db.insert("name".to_string(), JsValue::String(name.clone()));
            db.insert("version".to_string(), JsValue::Number(version));
            db.insert("__object_stores__".to_string(), JsValue::Array(Vec::new()));
            request.insert("result".to_string(), JsValue::Object(db));

            // Add success event
            let mut success_event = HashMap::new();
            success_event.insert("__type__".to_string(), JsValue::String("Event".to_string()));
            success_event.insert("type".to_string(), JsValue::String("success".to_string()));
            request.insert(
                "__pending_event__".to_string(),
                JsValue::Object(success_event),
            );

            JsValue::Object(request)
        }
        "deleteDatabase" => {
            let name = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            IDB_DATABASES.with(|dbs| {
                dbs.borrow_mut().remove(&name);
            });
            let mut request = HashMap::new();
            request.insert(
                "__type__".to_string(),
                JsValue::String("IDBOpenDBRequest".to_string()),
            );
            request.insert(
                "readyState".to_string(),
                JsValue::String("done".to_string()),
            );
            request.insert("result".to_string(), JsValue::Undefined);
            request.insert("error".to_string(), JsValue::Null);
            JsValue::Object(request)
        }
        "databases" => {
            let db_list: Vec<JsValue> = IDB_DATABASES.with(|dbs| {
                dbs.borrow()
                    .keys()
                    .map(|name| {
                        let mut info = HashMap::new();
                        info.insert("name".to_string(), JsValue::String(name.clone()));
                        info.insert("version".to_string(), JsValue::Number(1.0));
                        JsValue::Object(info)
                    })
                    .collect()
            });
            let mut p = HashMap::new();
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Array(db_list));
            JsValue::Object(p)
        }
        "cmp" => {
            let a = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let b = args
                .get(1)
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            JsValue::Number(a.cmp(&b) as i8 as f64)
        }
        _ => JsValue::Undefined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_obj(v: &JsValue) -> &HashMap<String, JsValue> {
        match v {
            JsValue::Object(m) => m,
            _ => panic!("expected Object"),
        }
    }

    fn get_str(v: &JsValue) -> &str {
        match v {
            JsValue::String(s) => s.as_str(),
            _ => panic!("expected String"),
        }
    }

    // ── make_performance ─────────────────────────────────────────────────

    #[test]
    fn performance_structure() {
        let perf = make_performance();
        let m = get_obj(&perf);
        assert_eq!(get_str(m.get("__type__").unwrap()), "Performance");
        assert!(m.get("timeOrigin").is_some());
    }

    // ── call_performance_method ──────────────────────────────────────────

    #[test]
    fn performance_now() {
        let result = call_performance_method("now", &[]);
        // Should return a non-negative number
        assert!(matches!(result, JsValue::Number(n) if n >= 0.0));
    }

    #[test]
    fn performance_mark() {
        let result = call_performance_method("mark", &[JsValue::String("test-mark".into())]);
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn performance_measure() {
        call_performance_method("mark", &[JsValue::String("start".into())]);
        let result = call_performance_method(
            "measure",
            &[
                JsValue::String("test-measure".into()),
                JsValue::String("start".into()),
            ],
        );
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn performance_get_entries() {
        call_performance_method("mark", &[JsValue::String("entry-test".into())]);
        let result = call_performance_method("getEntries", &[]);
        if let JsValue::Array(entries) = result {
            assert!(!entries.is_empty());
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn performance_get_entries_by_name() {
        call_performance_method("mark", &[JsValue::String("named-mark".into())]);
        let result =
            call_performance_method("getEntriesByName", &[JsValue::String("named-mark".into())]);
        if let JsValue::Array(entries) = result {
            assert!(!entries.is_empty());
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn performance_get_entries_by_type() {
        call_performance_method("mark", &[JsValue::String("type-test".into())]);
        let result = call_performance_method("getEntriesByType", &[JsValue::String("mark".into())]);
        if let JsValue::Array(entries) = result {
            assert!(!entries.is_empty());
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn performance_clear_marks() {
        call_performance_method("mark", &[JsValue::String("to-clear".into())]);
        let result = call_performance_method("clearMarks", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn performance_clear_measures() {
        call_performance_method("mark", &[JsValue::String("measure-start".into())]);
        call_performance_method(
            "measure",
            &[
                JsValue::String("test".into()),
                JsValue::String("measure-start".into()),
            ],
        );
        let result = call_performance_method("clearMeasures", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn performance_to_json() {
        let result = call_performance_method("toJSON", &[]);
        let m = get_obj(&result);
        assert!(m.get("timeOrigin").is_some());
        assert!(m.get("entries").is_some());
    }

    #[test]
    fn performance_unknown_method() {
        let result = call_performance_method("unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── make_history ─────────────────────────────────────────────────────

    #[test]
    fn history_structure() {
        let hist = make_history();
        let m = get_obj(&hist);
        assert_eq!(get_str(m.get("__type__").unwrap()), "History");
        assert!(m.get("length").is_some());
        assert!(m.get("state").is_some());
    }

    // ── call_history_method ──────────────────────────────────────────────

    #[test]
    fn history_push_state() {
        let result = call_history_method(
            "pushState",
            &[
                JsValue::String("test-state".into()),
                JsValue::String("".into()),
                JsValue::String("/test".into()),
            ],
        );
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn history_replace_state() {
        let result = call_history_method(
            "replaceState",
            &[
                JsValue::String("replaced-state".into()),
                JsValue::String("".into()),
                JsValue::String("/replaced".into()),
            ],
        );
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn history_back() {
        call_history_method(
            "pushState",
            &[
                JsValue::Number(1.0),
                JsValue::String("".into()),
                JsValue::String("/1".into()),
            ],
        );
        call_history_method(
            "pushState",
            &[
                JsValue::Number(2.0),
                JsValue::String("".into()),
                JsValue::String("/2".into()),
            ],
        );
        let result = call_history_method("back", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn history_forward() {
        call_history_method(
            "pushState",
            &[
                JsValue::Number(1.0),
                JsValue::String("".into()),
                JsValue::String("/1".into()),
            ],
        );
        call_history_method("back", &[]);
        let result = call_history_method("forward", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn history_go() {
        call_history_method(
            "pushState",
            &[
                JsValue::Number(1.0),
                JsValue::String("".into()),
                JsValue::String("/1".into()),
            ],
        );
        call_history_method(
            "pushState",
            &[
                JsValue::Number(2.0),
                JsValue::String("".into()),
                JsValue::String("/2".into()),
            ],
        );
        let result = call_history_method("go", &[JsValue::Number(-1.0)]);
        assert!(matches!(result, JsValue::Undefined));
    }

    #[test]
    fn history_unknown_method() {
        let result = call_history_method("unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── make_intersection_observer ───────────────────────────────────────

    #[test]
    fn intersection_observer_structure() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_intersection_observer(callback, None);
        let m = get_obj(&observer);
        assert_eq!(get_str(m.get("__type__").unwrap()), "IntersectionObserver");
        assert!(m.get("__callback__").is_some());
        if let JsValue::Array(targets) = m.get("__targets__").unwrap() {
            assert_eq!(targets.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    // ── call_intersection_observer_method ────────────────────────────────

    #[test]
    fn intersection_observer_observe() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_intersection_observer(callback, None);
        let m = get_obj(&observer);
        let target = JsValue::String("target-element".into());
        let result = call_intersection_observer_method(m, "observe", &[target]);
        let updated = get_obj(&result);
        if let JsValue::Array(targets) = updated.get("__targets__").unwrap() {
            assert_eq!(targets.len(), 1);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn intersection_observer_disconnect() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_intersection_observer(callback, None);
        let m = get_obj(&observer);
        let result = call_intersection_observer_method(m, "disconnect", &[]);
        let updated = get_obj(&result);
        if let JsValue::Array(targets) = updated.get("__targets__").unwrap() {
            assert_eq!(targets.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn intersection_observer_take_records() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_intersection_observer(callback, None);
        let m = get_obj(&observer);
        let result = call_intersection_observer_method(m, "takeRecords", &[]);
        if let JsValue::Array(records) = result {
            assert_eq!(records.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn intersection_observer_unknown_method() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_intersection_observer(callback, None);
        let m = get_obj(&observer);
        let result = call_intersection_observer_method(m, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── make_resize_observer ─────────────────────────────────────────────

    #[test]
    fn resize_observer_structure() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_resize_observer(callback);
        let m = get_obj(&observer);
        assert_eq!(get_str(m.get("__type__").unwrap()), "ResizeObserver");
        assert!(m.get("__callback__").is_some());
        if let JsValue::Array(targets) = m.get("__targets__").unwrap() {
            assert_eq!(targets.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    // ── call_resize_observer_method ──────────────────────────────────────

    #[test]
    fn resize_observer_observe() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_resize_observer(callback);
        let m = get_obj(&observer);
        let target = JsValue::String("target-element".into());
        let result = call_resize_observer_method(m, "observe", &[target]);
        let updated = get_obj(&result);
        if let JsValue::Array(targets) = updated.get("__targets__").unwrap() {
            assert_eq!(targets.len(), 1);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn resize_observer_disconnect() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_resize_observer(callback);
        let m = get_obj(&observer);
        let result = call_resize_observer_method(m, "disconnect", &[]);
        let updated = get_obj(&result);
        if let JsValue::Array(targets) = updated.get("__targets__").unwrap() {
            assert_eq!(targets.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn resize_observer_unknown_method() {
        let callback = JsValue::NativeFunction("test-callback".into());
        let observer = make_resize_observer(callback);
        let m = get_obj(&observer);
        let result = call_resize_observer_method(m, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }
}
