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

static CONSOLE_OUTPUT: std::sync::Mutex<Vec<ConsoleRecord>> = std::sync::Mutex::new(Vec::new());
#[allow(dead_code)]
static PERFORMANCE_ENTRIES: std::sync::Mutex<Vec<PerformanceEntry>> = std::sync::Mutex::new(Vec::new());
#[allow(dead_code)]
static PERFORMANCE_MARKS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, f64>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static CONSOLE_TIMERS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, f64>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static CONSOLE_COUNTS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, u64>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

pub fn perf_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub(super) fn push_console(level: &str, args: Vec<JsValue>) {
    let rec = ConsoleRecord { level: level.to_string(), args, timestamp: perf_now() };
    if let Ok(mut out) = CONSOLE_OUTPUT.lock() { out.push(rec); }
}

#[allow(dead_code)]
pub fn get_console_output() -> Vec<ConsoleRecord> {
    CONSOLE_OUTPUT.lock().map(|o| o.clone()).unwrap_or_default()
}

pub fn clear_console_output() {
    if let Ok(mut out) = CONSOLE_OUTPUT.lock() { out.clear(); }
}

#[allow(dead_code)]
pub fn get_performance_entries() -> Vec<PerformanceEntry> {
    PERFORMANCE_ENTRIES.lock().map(|e| e.clone()).unwrap_or_default()
}

#[allow(dead_code)]
pub fn clear_performance_entries() {
    if let Ok(mut e) = PERFORMANCE_ENTRIES.lock() { e.clear(); }
}

pub(super) fn perf_mark(name: &str) {
    let now = perf_now();
    if let Ok(mut marks) = PERFORMANCE_MARKS.lock() { marks.insert(name.to_string(), now); }
}

pub(super) fn perf_measure(name: &str) -> f64 {
    let now = perf_now();
    if let Ok(mut marks) = PERFORMANCE_MARKS.lock() {
        if let Some(start) = marks.remove(name) {
            let duration = now - start;
            if let Ok(mut entries) = PERFORMANCE_ENTRIES.lock() {
                entries.push(PerformanceEntry {
                    name: name.to_string(),
                    entry_type: "measure".to_string(),
                    start_time: start,
                    duration,
                });
            }
            return duration;
        }
    }
    0.0
}

pub(super) fn console_time(label: &str) {
    if let Ok(mut timers) = CONSOLE_TIMERS.lock() { timers.insert(label.to_string(), perf_now()); }
}

pub(super) fn console_time_end(label: &str) -> Option<f64> {
    let now = perf_now();
    if let Ok(mut timers) = CONSOLE_TIMERS.lock() {
        if let Some(start) = timers.remove(label) { return Some(now - start); }
    }
    None
}

pub(super) fn console_count(label: &str) -> u64 {
    let mut counts = CONSOLE_COUNTS.lock().unwrap();
    let entry = counts.entry(label.to_string()).or_insert(0);
    *entry += 1;
    *entry
}

pub(super) fn console_count_reset(label: &str) {
    if let Ok(mut counts) = CONSOLE_COUNTS.lock() { counts.insert(label.to_string(), 0); }
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
