//! Production telemetry and metrics collection for V.E.L.O.C.I.T.Y.
//!
//! This module provides:
//! - **Metrics**: Counters, gauges, and histograms for runtime observability
//! - **Structured logging**: JSON-formatted log events with levels
//! - **Performance profiling**: Timing instrumentation for critical paths
//! - **Export**: File-based telemetry dump for offline analysis

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Global telemetry instance
static GLOBAL_TELEMETRY: OnceLock<TelemetryCollector> = OnceLock::new();

/// Get or initialize the global telemetry collector
pub fn global() -> &'static TelemetryCollector {
    GLOBAL_TELEMETRY.get_or_init(|| TelemetryCollector::new())
}

/// Metric types supported by the collector
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Metric {
    Counter { name: String, value: u64 },
    Gauge { name: String, value: f64 },
    Histogram { name: String, values: Vec<f64> },
}

/// Log levels for structured logging
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// A structured log event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp: u64,
    pub level: LogLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,
}

/// Performance span for timing operations
#[derive(Debug)]
pub struct Span {
    name: String,
    start: Instant,
    fields: HashMap<String, serde_json::Value>,
}

impl Span {
    /// Add a field to this span
    pub fn field(mut self, key: &str, value: impl serde::Serialize) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.fields.insert(key.to_string(), v);
        }
        self
    }

    /// Complete the span and record its duration
    pub fn finish(self) -> SpanRecord {
        SpanRecord {
            name: self.name,
            duration_ms: self.start.elapsed().as_secs_f64() * 1000.0,
            fields: self.fields,
        }
    }
}

/// Completed span with timing information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanRecord {
    pub name: String,
    pub duration_ms: f64,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,
}

/// Main telemetry collector
pub struct TelemetryCollector {
    counters: Mutex<HashMap<String, AtomicU64>>,
    gauges: Mutex<HashMap<String, f64>>,
    histograms: Mutex<HashMap<String, Vec<f64>>>,
    logs: Mutex<Vec<LogEvent>>,
    spans: Mutex<Vec<SpanRecord>>,
    max_logs: usize,
    max_spans: usize,
}

impl TelemetryCollector {
    /// Create a new telemetry collector
    pub fn new() -> Self {
        Self {
            counters: Mutex::new(HashMap::new()),
            gauges: Mutex::new(HashMap::new()),
            histograms: Mutex::new(HashMap::new()),
            logs: Mutex::new(Vec::new()),
            spans: Mutex::new(Vec::new()),
            max_logs: 10_000,
            max_spans: 1_000,
        }
    }

    /// Create a collector with custom limits
    pub fn with_limits(max_logs: usize, max_spans: usize) -> Self {
        Self {
            counters: Mutex::new(HashMap::new()),
            gauges: Mutex::new(HashMap::new()),
            histograms: Mutex::new(HashMap::new()),
            logs: Mutex::new(Vec::new()),
            spans: Mutex::new(Vec::new()),
            max_logs,
            max_spans,
        }
    }

    // ─── Counters ────────────────────────────────────────────────────────

    /// Increment a counter by the given amount
    pub fn counter_increment(&self, name: &str, amount: u64) {
        let mut counters = self.counters.lock().unwrap();
        let counter = counters.entry(name.to_string()).or_default();
        counter.fetch_add(amount, Ordering::Relaxed);
    }

    /// Get the current value of a counter
    pub fn counter_get(&self, name: &str) -> u64 {
        let counters = self.counters.lock().unwrap();
        counters
            .get(name)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    // ─── Gauges ──────────────────────────────────────────────────────────

    /// Set a gauge to a specific value
    pub fn gauge_set(&self, name: &str, value: f64) {
        let mut gauges = self.gauges.lock().unwrap();
        gauges.insert(name.to_string(), value);
    }

    /// Get the current value of a gauge
    pub fn gauge_get(&self, name: &str) -> Option<f64> {
        let gauges = self.gauges.lock().unwrap();
        gauges.get(name).copied()
    }

    // ─── Histograms ──────────────────────────────────────────────────────

    /// Record a value in a histogram
    pub fn histogram_record(&self, name: &str, value: f64) {
        let mut histograms = self.histograms.lock().unwrap();
        let hist = histograms.entry(name.to_string()).or_default();
        hist.push(value);
        // Keep only last 1000 values
        if hist.len() > 1000 {
            hist.remove(0);
        }
    }

    /// Get histogram statistics
    pub fn histogram_stats(&self, name: &str) -> Option<HistogramStats> {
        let histograms = self.histograms.lock().unwrap();
        histograms.get(name).map(|values| {
            if values.is_empty() {
                return HistogramStats {
                    count: 0,
                    min: 0.0,
                    max: 0.0,
                    mean: 0.0,
                    p50: 0.0,
                    p95: 0.0,
                    p99: 0.0,
                };
            }
            let mut sorted = values.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let count = sorted.len();
            let sum: f64 = sorted.iter().sum();
            HistogramStats {
                count,
                min: sorted[0],
                max: sorted[count - 1],
                mean: sum / count as f64,
                p50: percentile(&sorted, 50.0),
                p95: percentile(&sorted, 95.0),
                p99: percentile(&sorted, 99.0),
            }
        })
    }

    // ─── Logging ─────────────────────────────────────────────────────────

    /// Log an event at the specified level
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.log_with_fields(level, message, None, HashMap::new());
    }

    /// Log an event with module context
    pub fn log_module(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        module: impl Into<String>,
    ) {
        self.log_with_fields(level, message, Some(module.into()), HashMap::new());
    }

    /// Log an event with additional fields
    pub fn log_with_fields(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        module: Option<String>,
        fields: HashMap<String, serde_json::Value>,
    ) {
        let event = LogEvent {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            level,
            message: message.into(),
            module,
            fields,
        };

        let mut logs = self.logs.lock().unwrap();
        logs.push(event);
        if logs.len() > self.max_logs {
            logs.remove(0);
        }
    }

    /// Get all logged events
    pub fn logs(&self) -> Vec<LogEvent> {
        self.logs.lock().unwrap().clone()
    }

    // ─── Spans ───────────────────────────────────────────────────────────

    /// Start a new timing span
    pub fn span(&self, name: impl Into<String>) -> Span {
        Span {
            name: name.into(),
            start: Instant::now(),
            fields: HashMap::new(),
        }
    }

    /// Record a completed span
    pub fn record_span(&self, span: SpanRecord) {
        let mut spans = self.spans.lock().unwrap();
        spans.push(span);
        if spans.len() > self.max_spans {
            spans.remove(0);
        }
    }

    /// Get all recorded spans
    pub fn spans(&self) -> Vec<SpanRecord> {
        self.spans.lock().unwrap().clone()
    }

    // ─── Export ──────────────────────────────────────────────────────────

    /// Export all telemetry data as JSON
    pub fn export_json(&self) -> serde_json::Value {
        let counters: HashMap<String, u64> = self
            .counters
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
            .collect();

        let gauges: HashMap<String, f64> = self.gauges.lock().unwrap().clone();

        let histograms: HashMap<String, HistogramStats> = self
            .histograms
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(name, _)| self.histogram_stats(name).map(|s| (name.clone(), s)))
            .collect();

        serde_json::json!({
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "counters": counters,
            "gauges": gauges,
            "histograms": histograms,
            "logs": self.logs(),
            "spans": self.spans(),
        })
    }

    /// Export telemetry to a file
    pub fn export_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = self.export_json();
        let content = serde_json::to_string_pretty(&json).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }

    /// Clear all collected data
    pub fn clear(&self) {
        self.counters.lock().unwrap().clear();
        self.gauges.lock().unwrap().clear();
        self.histograms.lock().unwrap().clear();
        self.logs.lock().unwrap().clear();
        self.spans.lock().unwrap().clear();
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Histogram statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

/// Calculate percentile from sorted values
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ─── Convenience macros ──────────────────────────────────────────────────────

/// Log at trace level
#[macro_export]
macro_rules! telemetry_trace {
    ($msg:expr) => {
        $crate::ipc::telemetry::global().log($crate::ipc::telemetry::LogLevel::Trace, $msg)
    };
    ($msg:expr, $($key:ident = $value:expr),+) => {{
        let mut fields = std::collections::HashMap::new();
        $(fields.insert(stringify!($key).to_string(), serde_json::json!($value));)+
        $crate::ipc::telemetry::global().log_with_fields(
            $crate::ipc::telemetry::LogLevel::Trace,
            $msg,
            None,
            fields,
        )
    }};
}

/// Log at debug level
#[macro_export]
macro_rules! telemetry_debug {
    ($msg:expr) => {
        $crate::ipc::telemetry::global().log($crate::ipc::telemetry::LogLevel::Debug, $msg)
    };
}

/// Log at info level
#[macro_export]
macro_rules! telemetry_info {
    ($msg:expr) => {
        $crate::ipc::telemetry::global().log($crate::ipc::telemetry::LogLevel::Info, $msg)
    };
}

/// Log at warn level
#[macro_export]
macro_rules! telemetry_warn {
    ($msg:expr) => {
        $crate::ipc::telemetry::global().log($crate::ipc::telemetry::LogLevel::Warn, $msg)
    };
}

/// Log at error level
#[macro_export]
macro_rules! telemetry_error {
    ($msg:expr) => {
        $crate::ipc::telemetry::global().log($crate::ipc::telemetry::LogLevel::Error, $msg)
    };
}

/// Start a timing span
#[macro_export]
macro_rules! telemetry_span {
    ($name:expr) => {
        $crate::ipc::telemetry::global().span($name)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let tc = TelemetryCollector::new();
        tc.counter_increment("requests", 1);
        tc.counter_increment("requests", 5);
        assert_eq!(tc.counter_get("requests"), 6);
        assert_eq!(tc.counter_get("nonexistent"), 0);
    }

    #[test]
    fn test_gauge() {
        let tc = TelemetryCollector::new();
        tc.gauge_set("temperature", 72.5);
        assert_eq!(tc.gauge_get("temperature"), Some(72.5));
        assert_eq!(tc.gauge_get("nonexistent"), None);
    }

    #[test]
    fn test_histogram() {
        let tc = TelemetryCollector::new();
        for i in 1..=100 {
            tc.histogram_record("latency", i as f64);
        }
        let stats = tc.histogram_stats("latency").unwrap();
        assert_eq!(stats.count, 100);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 100.0);
        assert!((stats.mean - 50.5).abs() < 0.01);
    }

    #[test]
    fn test_logging() {
        let tc = TelemetryCollector::new();
        tc.log(LogLevel::Info, "Test message");
        tc.log_module(LogLevel::Debug, "Module message", "test_module");

        let logs = tc.logs();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].level, LogLevel::Info);
        assert_eq!(logs[0].message, "Test message");
        assert_eq!(logs[1].module, Some("test_module".to_string()));
    }

    #[test]
    fn test_span() {
        let tc = TelemetryCollector::new();
        let span = tc.span("test_operation");
        std::thread::sleep(Duration::from_millis(10));
        let record = span.finish();

        assert_eq!(record.name, "test_operation");
        assert!(record.duration_ms >= 10.0);
    }

    #[test]
    fn test_export_json() {
        let tc = TelemetryCollector::new();
        tc.counter_increment("test_counter", 42);
        tc.gauge_set("test_gauge", 3.14);
        tc.log(LogLevel::Info, "Test log");

        let json = tc.export_json();
        assert!(json.get("counters").is_some());
        assert!(json.get("gauges").is_some());
        assert!(json.get("logs").is_some());
    }
}
