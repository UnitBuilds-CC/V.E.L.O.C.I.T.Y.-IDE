//! Console API, performance entries, and related statics.

use crate::js::vm::JsValue;
use std::collections::HashMap;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConsoleRecord {
    pub level: String,
    pub args: Vec<JsValue>,
    pub timestamp: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PerformanceEntry {
    pub name: String,
    pub entry_type: String,
    pub start_time: f64,
    pub duration: f64,
}

// Console state is thread-local like the rest of the interpreter: each page
// (and each test thread) owns its console, so parallel sessions never see
// each other's records.
thread_local! {
    static CONSOLE_OUTPUT: std::cell::RefCell<Vec<ConsoleRecord>> = const { std::cell::RefCell::new(Vec::new()) };
    static PERFORMANCE_ENTRIES: std::cell::RefCell<Vec<PerformanceEntry>> = const { std::cell::RefCell::new(Vec::new()) };
    static PERFORMANCE_MARKS: std::cell::RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
    static CONSOLE_TIMERS: std::cell::RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
    static CONSOLE_COUNTS: std::cell::RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
}
use std::cell::RefCell;

pub fn perf_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub(super) fn push_console(level: &str, args: Vec<JsValue>) {
    let rec = ConsoleRecord { level: level.to_string(), args, timestamp: perf_now() };
    CONSOLE_OUTPUT.with(|o| o.borrow_mut().push(rec));
}

#[allow(dead_code)]
pub fn get_console_output() -> Vec<ConsoleRecord> {
    CONSOLE_OUTPUT.with(|o| o.borrow().clone())
}

pub fn clear_console_output() {
    CONSOLE_OUTPUT.with(|o| o.borrow_mut().clear());
}

#[allow(dead_code)]
pub fn get_performance_entries() -> Vec<PerformanceEntry> {
    PERFORMANCE_ENTRIES.with(|e| e.borrow().clone())
}

#[allow(dead_code)]
pub fn clear_performance_entries() {
    PERFORMANCE_ENTRIES.with(|e| e.borrow_mut().clear());
}

pub(super) fn perf_mark(name: &str) {
    let now = perf_now();
    PERFORMANCE_MARKS.with(|m| m.borrow_mut().insert(name.to_string(), now));
}

pub(super) fn perf_measure(name: &str) -> f64 {
    let now = perf_now();
    let start = PERFORMANCE_MARKS.with(|m| m.borrow_mut().remove(name));
    if let Some(start) = start {
        let duration = now - start;
        PERFORMANCE_ENTRIES.with(|e| {
            e.borrow_mut().push(PerformanceEntry {
                name: name.to_string(),
                entry_type: "measure".to_string(),
                start_time: start,
                duration,
            });
        });
        return duration;
    }
    0.0
}

pub(super) fn console_time(label: &str) {
    CONSOLE_TIMERS.with(|t| t.borrow_mut().insert(label.to_string(), perf_now()));
}

pub(super) fn console_time_end(label: &str) -> Option<f64> {
    let now = perf_now();
    CONSOLE_TIMERS.with(|t| t.borrow_mut().remove(label)).map(|start| now - start)
}

pub(super) fn console_count(label: &str) -> u64 {
    CONSOLE_COUNTS.with(|c| {
        let mut counts = c.borrow_mut();
        let entry = counts.entry(label.to_string()).or_insert(0);
        *entry += 1;
        *entry
    })
}

pub(super) fn console_count_reset(label: &str) {
    CONSOLE_COUNTS.with(|c| { c.borrow_mut().insert(label.to_string(), 0); });
}

/// Render a value as a Markdown table for `console.table`.
///
/// Supported shapes mirror the browser API: array of objects (columns from key
/// union), array of arrays (numbered columns), array of primitives, and plain
/// objects (key/value rows). Anything else falls back to string coercion.
pub(super) fn console_table_text(value: &JsValue) -> String {
    fn cell(v: &JsValue) -> String {
        super::coercion::to_string(v).replace('|', "\\|").replace('\n', " ")
    }
    fn render(headers: &[String], rows: &[Vec<String>]) -> String {
        let mut out = String::new();
        out.push_str("| ");
        out.push_str(&headers.join(" | "));
        out.push_str(" |\n|");
        for _ in headers { out.push_str(" --- |"); }
        out.push('\n');
        for row in rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
        out
    }

    match value {
        JsValue::Array(items) if !items.is_empty() => {
            // Array of objects → columns are the union of keys (first-seen order).
            if items.iter().all(|i| matches!(i, JsValue::Object(_))) {
                let mut headers: Vec<String> = vec!["(index)".to_string()];
                for item in items {
                    if let JsValue::Object(map) = item {
                        let mut keys: Vec<&String> = map.keys().filter(|k| !k.starts_with("__")).collect();
                        keys.sort();
                        for k in keys {
                            if !headers.iter().any(|h| h == k) { headers.push(k.clone()); }
                        }
                    }
                }
                let rows: Vec<Vec<String>> = items.iter().enumerate().map(|(i, item)| {
                    let mut row = vec![i.to_string()];
                    if let JsValue::Object(map) = item {
                        for h in &headers[1..] {
                            row.push(map.get(h).map(cell).unwrap_or_default());
                        }
                    }
                    row
                }).collect();
                return render(&headers, &rows);
            }
            // Array of arrays → numbered columns.
            if items.iter().all(|i| matches!(i, JsValue::Array(_))) {
                let width = items.iter().map(|i| if let JsValue::Array(a) = i { a.len() } else { 0 }).max().unwrap_or(0);
                let mut headers = vec!["(index)".to_string()];
                for c in 0..width { headers.push(c.to_string()); }
                let rows: Vec<Vec<String>> = items.iter().enumerate().map(|(i, item)| {
                    let mut row = vec![i.to_string()];
                    if let JsValue::Array(a) = item {
                        for c in 0..width { row.push(a.get(c).map(cell).unwrap_or_default()); }
                    }
                    row
                }).collect();
                return render(&headers, &rows);
            }
            // Array of primitives → single Values column.
            let headers = vec!["(index)".to_string(), "Values".to_string()];
            let rows: Vec<Vec<String>> = items.iter().enumerate()
                .map(|(i, item)| vec![i.to_string(), cell(item)])
                .collect();
            render(&headers, &rows)
        }
        JsValue::Object(map) => {
            let headers = vec!["(index)".to_string(), "Values".to_string()];
            let mut keys: Vec<&String> = map.keys().filter(|k| !k.starts_with("__")).collect();
            keys.sort();
            let rows: Vec<Vec<String>> = keys.into_iter()
                .map(|k| vec![k.clone(), map.get(k).map(cell).unwrap_or_default()])
                .collect();
            render(&headers, &rows)
        }
        other => cell(other),
    }
}

/// Format captured console output as compact `level: message` lines so agents
/// can read page diagnostics without a devtools UI.
pub fn console_output_text() -> String {
    let records = get_console_output();
    let mut out = String::with_capacity(records.len() * 48);
    for rec in &records {
        out.push_str(&rec.level);
        out.push_str(": ");
        let mut first = true;
        for arg in &rec.args {
            if !first { out.push(' '); }
            out.push_str(&super::coercion::to_string(arg));
            first = false;
        }
        out.push('\n');
    }
    out
}

/// Capture a stack trace string for error reporting.
/// Since we don't have real call frames, we produce a best-effort trace from
/// the error name/message.
pub fn capture_stack_trace(error_name: &str, message: &str) -> String {
    if message.is_empty() {
        error_name.to_string()
    } else {
        format!("{}: {}", error_name, message)
    }
}
