//! OpenTelemetry-inspired distributed tracing for pipeline observability.
//!
//! Provides [`Tracer`] which manages [`Span`]s representing units of work
//! across the agent execution pipeline. Each span carries attributes, events,
//! timing, and parent-child relationships so that end-to-end request traces
//! can be reconstructed offline.

use std::collections::HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// SpanKind
// ---------------------------------------------------------------------------

/// Describes the relationship between a span and its parent / remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpanKind {
    /// Default – internal operation.
    Internal,
    /// Outgoing synchronous request.
    Client,
    /// Incoming synchronous request.
    Server,
    /// Asynchronous producer (e.g. enqueue).
    Producer,
    /// Asynchronous consumer (e.g. dequeue).
    Consumer,
}

impl SpanKind {
    /// Canonical string representation matching OTel naming.
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanKind::Internal => "INTERNAL",
            SpanKind::Client => "CLIENT",
            SpanKind::Server => "SERVER",
            SpanKind::Producer => "PRODUCER",
            SpanKind::Consumer => "CONSUMER",
        }
    }
}

// ---------------------------------------------------------------------------
// SpanStatus
// ---------------------------------------------------------------------------

/// Outcome of a span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanStatus {
    /// Not explicitly set – inherit from context.
    Unset,
    /// Operation completed successfully.
    Ok,
    /// Operation failed with a description.
    Error(String),
}

impl SpanStatus {
    /// Returns `true` when the status represents an error.
    pub fn is_error(&self) -> bool {
        matches!(self, SpanStatus::Error(_))
    }
}

// ---------------------------------------------------------------------------
// SpanEvent
// ---------------------------------------------------------------------------

/// A timestamped annotation within a span.
#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: Instant,
    pub attributes: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

/// A single unit of work inside a trace.
#[derive(Debug, Clone)]
pub struct Span {
    pub trace_id: u64,
    pub span_id: u64,
    pub parent_span_id: Option<u64>,
    pub name: String,
    pub kind: SpanKind,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub attributes: HashMap<String, String>,
    pub status: SpanStatus,
    pub events: Vec<SpanEvent>,
}

impl Span {
    /// Duration of the span, or `None` if it has not ended yet.
    pub fn duration(&self) -> Option<Duration> {
        self.end_time.map(|end| end.duration_since(self.start_time))
    }

    /// Returns `true` when the span is still active (not ended).
    pub fn is_active(&self) -> bool {
        self.end_time.is_none()
    }
}

// ---------------------------------------------------------------------------
// TracingStats
// ---------------------------------------------------------------------------

/// Aggregate statistics about collected spans.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TracingStats {
    /// Number of distinct traces observed (unique `trace_id`s).
    pub total_traces: u64,
    /// Total number of spans ever completed.
    pub total_spans: u64,
    /// Number of spans currently active.
    pub active_spans: usize,
    /// Average duration of completed spans in microseconds.
    pub avg_duration_us: u64,
    /// Number of completed spans with an error status.
    pub error_count: u64,
}

// ---------------------------------------------------------------------------
// Tracer
// ---------------------------------------------------------------------------

/// Central collector that creates, tracks, and exports spans.
pub struct Tracer {
    pub service_name: String,
    active_spans: HashMap<u64, Span>,
    completed_spans: Vec<Span>,
    max_completed: usize,
    next_trace_id: u64,
    next_span_id: u64,
}

impl Tracer {
    /// Create a new tracer for the given service.
    pub fn new(service_name: String) -> Self {
        Self {
            service_name,
            active_spans: HashMap::new(),
            completed_spans: Vec::new(),
            max_completed: 1000,
            next_trace_id: 1,
            next_span_id: 1,
        }
    }

    // -- ID generation ------------------------------------------------------

    fn alloc_trace_id(&mut self) -> u64 {
        let id = self.next_trace_id;
        self.next_trace_id = self.next_trace_id.wrapping_add(1);
        id
    }

    fn alloc_span_id(&mut self) -> u64 {
        let id = self.next_span_id;
        self.next_span_id = self.next_span_id.wrapping_add(1);
        id
    }

    // -- Span lifecycle -----------------------------------------------------

    /// Start a new root span and return its `span_id`.
    pub fn start_span(&mut self, name: &str, kind: SpanKind) -> u64 {
        let trace_id = self.alloc_trace_id();
        let span_id = self.alloc_span_id();

        let span = Span {
            trace_id,
            span_id,
            parent_span_id: None,
            name: name.to_string(),
            kind,
            start_time: Instant::now(),
            end_time: None,
            attributes: HashMap::new(),
            status: SpanStatus::Unset,
            events: Vec::new(),
        };

        self.active_spans.insert(span_id, span);
        span_id
    }

    /// Start a child span linked to `parent_id`.
    ///
    /// The child inherits the `trace_id` of its parent.  If the parent does
    /// not exist the span is still created (as a de-facto root) so that
    /// instrumentation code never has to guard against missing parents.
    pub fn start_child_span(&mut self, name: &str, kind: SpanKind, parent_id: u64) -> u64 {
        let span_id = self.alloc_span_id();

        // Inherit trace_id from parent when available.
        let (trace_id, parent_exists) = self
            .active_spans
            .get(&parent_id)
            .map(|p| (p.trace_id, true))
            .unwrap_or_else(|| (self.alloc_trace_id(), false));

        let span = Span {
            trace_id,
            span_id,
            parent_span_id: if parent_exists { Some(parent_id) } else { None },
            name: name.to_string(),
            kind,
            start_time: Instant::now(),
            end_time: None,
            attributes: HashMap::new(),
            status: SpanStatus::Unset,
            events: Vec::new(),
        };

        self.active_spans.insert(span_id, span);
        span_id
    }

    /// End a span, recording its final `status`.
    ///
    /// The span is moved from active to completed.  Returns `true` if the
    /// span existed and was ended.
    pub fn end_span(&mut self, span_id: u64, status: SpanStatus) -> bool {
        if let Some(mut span) = self.active_spans.remove(&span_id) {
            span.end_time = Some(Instant::now());
            span.status = status;
            self.push_completed(span);
            true
        } else {
            false
        }
    }

    // -- Attributes & events ------------------------------------------------

    /// Attach a key-value attribute to an active span.
    pub fn add_attribute(&mut self, span_id: u64, key: &str, value: &str) {
        if let Some(span) = self.active_spans.get_mut(&span_id) {
            span.attributes.insert(key.to_string(), value.to_string());
        }
    }

    /// Record a timestamped event on an active span.
    pub fn add_event(&mut self, span_id: u64, name: &str, attributes: HashMap<String, String>) {
        if let Some(span) = self.active_spans.get_mut(&span_id) {
            span.events.push(SpanEvent {
                name: name.to_string(),
                timestamp: Instant::now(),
                attributes,
            });
        }
    }

    // -- Queries ------------------------------------------------------------

    /// Borrow an active span by id.
    pub fn get_span(&self, span_id: u64) -> Option<&Span> {
        self.active_spans.get(&span_id)
    }

    /// Duration of an active span (from start to now) or a completed span.
    pub fn trace_duration(&self, span_id: u64) -> Option<Duration> {
        self.active_spans
            .get(&span_id)
            .map(|s| s.start_time.elapsed())
            .or_else(|| {
                self.completed_spans
                    .iter()
                    .find(|s| s.span_id == span_id)
                    .and_then(|s| s.duration())
            })
    }

    // -- Export / housekeeping ----------------------------------------------

    /// Return a snapshot of all completed spans.
    pub fn export_traces(&self) -> Vec<Span> {
        self.completed_spans.clone()
    }

    /// Drop all completed spans.
    pub fn clear_completed(&mut self) {
        self.completed_spans.clear();
    }

    /// Number of spans currently active (not yet ended).
    pub fn active_count(&self) -> usize {
        self.active_spans.len()
    }

    /// Compute aggregate statistics across all known spans.
    pub fn tracing_stats(&self) -> TracingStats {
        let total_spans = self.completed_spans.len() as u64;
        let error_count = self
            .completed_spans
            .iter()
            .filter(|s| s.status.is_error())
            .count() as u64;

        let total_traces = self
            .completed_spans
            .iter()
            .map(|s| s.trace_id)
            .collect::<std::collections::HashSet<_>>()
            .len() as u64;

        let sum_us: u64 = self
            .completed_spans
            .iter()
            .filter_map(|s| s.duration().map(|d| d.as_micros() as u64))
            .sum();
        let avg_duration_us = sum_us.checked_div(total_spans).unwrap_or(0);

        TracingStats {
            total_traces,
            total_spans,
            active_spans: self.active_spans.len(),
            avg_duration_us,
            error_count,
        }
    }

    // -- Internal helpers ---------------------------------------------------

    fn push_completed(&mut self, span: Span) {
        if self.completed_spans.len() >= self.max_completed {
            // Drop oldest to respect the ring-buffer limit.
            self.completed_spans.remove(0);
        }
        self.completed_spans.push(span);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn new_tracer() -> Tracer {
        Tracer::new("test-service".into())
    }

    // -- SpanKind -----------------------------------------------------------

    #[test]
    fn span_kind_as_str() {
        assert_eq!(SpanKind::Internal.as_str(), "INTERNAL");
        assert_eq!(SpanKind::Client.as_str(), "CLIENT");
        assert_eq!(SpanKind::Server.as_str(), "SERVER");
        assert_eq!(SpanKind::Producer.as_str(), "PRODUCER");
        assert_eq!(SpanKind::Consumer.as_str(), "CONSUMER");
    }

    // -- SpanStatus ---------------------------------------------------------

    #[test]
    fn span_status_is_error() {
        assert!(!SpanStatus::Unset.is_error());
        assert!(!SpanStatus::Ok.is_error());
        assert!(SpanStatus::Error("boom".into()).is_error());
    }

    // -- start_span ---------------------------------------------------------

    #[test]
    fn start_span_returns_unique_ids() {
        let mut t = new_tracer();
        let a = t.start_span("op-a", SpanKind::Internal);
        let b = t.start_span("op-b", SpanKind::Internal);
        assert_ne!(a, b);
        assert_eq!(t.active_count(), 2);
    }

    #[test]
    fn start_span_sets_name_and_kind() {
        let mut t = new_tracer();
        let id = t.start_span("my-span", SpanKind::Server);
        let span = t.get_span(id).unwrap();
        assert_eq!(span.name, "my-span");
        assert_eq!(span.kind, SpanKind::Server);
        assert!(span.parent_span_id.is_none());
        assert!(span.is_active());
    }

    // -- start_child_span ---------------------------------------------------

    #[test]
    fn child_span_inherits_trace_id() {
        let mut t = new_tracer();
        let parent = t.start_span("parent", SpanKind::Server);
        let child = t.start_child_span("child", SpanKind::Internal, parent);

        let p = t.get_span(parent).unwrap();
        let c = t.get_span(child).unwrap();
        assert_eq!(p.trace_id, c.trace_id);
        assert_eq!(c.parent_span_id, Some(parent));
    }

    #[test]
    fn child_span_with_missing_parent_becomes_root() {
        let mut t = new_tracer();
        let child = t.start_child_span("orphan", SpanKind::Internal, 9999);
        let c = t.get_span(child).unwrap();
        assert!(c.parent_span_id.is_none());
    }

    // -- end_span -----------------------------------------------------------

    #[test]
    fn end_span_moves_to_completed() {
        let mut t = new_tracer();
        let id = t.start_span("work", SpanKind::Internal);
        assert_eq!(t.active_count(), 1);

        let ended = t.end_span(id, SpanStatus::Ok);
        assert!(ended);
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.export_traces().len(), 1);
    }

    #[test]
    fn end_span_records_status() {
        let mut t = new_tracer();
        let id = t.start_span("fail", SpanKind::Internal);
        t.end_span(id, SpanStatus::Error("oops".into()));

        let completed = t.export_traces();
        assert_eq!(completed[0].status, SpanStatus::Error("oops".into()));
    }

    #[test]
    fn end_nonexistent_span_returns_false() {
        let mut t = new_tracer();
        assert!(!t.end_span(42, SpanStatus::Ok));
    }

    #[test]
    fn ended_span_has_duration() {
        let mut t = new_tracer();
        let id = t.start_span("timed", SpanKind::Internal);
        // Small busy-loop so duration > 0.
        std::thread::sleep(std::time::Duration::from_millis(1));
        t.end_span(id, SpanStatus::Ok);

        let span = &t.export_traces()[0];
        assert!(span.duration().is_some());
        assert!(span.duration().unwrap() >= Duration::from_millis(1));
    }

    // -- attributes ---------------------------------------------------------

    #[test]
    fn add_attribute_to_active_span() {
        let mut t = new_tracer();
        let id = t.start_span("attr-test", SpanKind::Internal);
        t.add_attribute(id, "http.method", "GET");
        t.add_attribute(id, "http.status", "200");

        let span = t.get_span(id).unwrap();
        assert_eq!(span.attributes.get("http.method").unwrap(), "GET");
        assert_eq!(span.attributes.get("http.status").unwrap(), "200");
    }

    #[test]
    fn add_attribute_overwrites_existing_key() {
        let mut t = new_tracer();
        let id = t.start_span("overwrite", SpanKind::Internal);
        t.add_attribute(id, "key", "v1");
        t.add_attribute(id, "key", "v2");
        assert_eq!(t.get_span(id).unwrap().attributes.get("key").unwrap(), "v2");
    }

    #[test]
    fn add_attribute_to_unknown_span_is_noop() {
        let mut t = new_tracer();
        t.add_attribute(999, "k", "v"); // must not panic
    }

    // -- events -------------------------------------------------------------

    #[test]
    fn add_event_to_active_span() {
        let mut t = new_tracer();
        let id = t.start_span("evt", SpanKind::Internal);
        let mut attrs = HashMap::new();
        attrs.insert("code".into(), "42".into());
        t.add_event(id, "checkpoint", attrs);

        let span = t.get_span(id).unwrap();
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "checkpoint");
        assert_eq!(span.events[0].attributes.get("code").unwrap(), "42");
    }

    #[test]
    fn add_multiple_events() {
        let mut t = new_tracer();
        let id = t.start_span("multi-evt", SpanKind::Internal);
        t.add_event(id, "e1", HashMap::new());
        t.add_event(id, "e2", HashMap::new());
        t.add_event(id, "e3", HashMap::new());
        assert_eq!(t.get_span(id).unwrap().events.len(), 3);
    }

    #[test]
    fn add_event_to_unknown_span_is_noop() {
        let mut t = new_tracer();
        t.add_event(999, "noop", HashMap::new()); // must not panic
    }

    // -- export / clear -----------------------------------------------------

    #[test]
    fn export_returns_completed_only() {
        let mut t = new_tracer();
        let a = t.start_span("a", SpanKind::Internal);
        let _b = t.start_span("b", SpanKind::Internal);
        t.end_span(a, SpanStatus::Ok);

        let exported = t.export_traces();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name, "a");
    }

    #[test]
    fn clear_completed_empties_list() {
        let mut t = new_tracer();
        let id = t.start_span("x", SpanKind::Internal);
        t.end_span(id, SpanStatus::Ok);
        assert_eq!(t.export_traces().len(), 1);

        t.clear_completed();
        assert_eq!(t.export_traces().len(), 0);
    }

    // -- max_completed ring buffer ------------------------------------------

    #[test]
    fn max_completed_evicts_oldest() {
        let mut t = Tracer::new("ring".into());
        t.max_completed = 3;

        for i in 0..5 {
            let id = t.start_span(&format!("s{i}"), SpanKind::Internal);
            t.end_span(id, SpanStatus::Ok);
        }

        let traces = t.export_traces();
        assert_eq!(traces.len(), 3);
        // Oldest two (s0, s1) should have been evicted.
        assert_eq!(traces[0].name, "s2");
        assert_eq!(traces[1].name, "s3");
        assert_eq!(traces[2].name, "s4");
    }

    // -- trace_duration -----------------------------------------------------

    #[test]
    fn trace_duration_active_span() {
        let mut t = new_tracer();
        let id = t.start_span("live", SpanKind::Internal);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let d = t.trace_duration(id);
        assert!(d.is_some());
        assert!(d.unwrap() >= Duration::from_millis(1));
    }

    #[test]
    fn trace_duration_completed_span() {
        let mut t = new_tracer();
        let id = t.start_span("done", SpanKind::Internal);
        std::thread::sleep(std::time::Duration::from_millis(1));
        t.end_span(id, SpanStatus::Ok);
        let d = t.trace_duration(id);
        assert!(d.is_some());
    }

    #[test]
    fn trace_duration_unknown_span_returns_none() {
        let t = new_tracer();
        assert!(t.trace_duration(12345).is_none());
    }

    // -- tracing_stats ------------------------------------------------------

    #[test]
    fn tracing_stats_empty() {
        let t = new_tracer();
        let stats = t.tracing_stats();
        assert_eq!(stats, TracingStats::default());
    }

    #[test]
    fn tracing_stats_counts() {
        let mut t = new_tracer();
        let a = t.start_span("a", SpanKind::Internal);
        let b = t.start_span("b", SpanKind::Internal);
        t.end_span(a, SpanStatus::Ok);
        t.end_span(b, SpanStatus::Error("fail".into()));

        let stats = t.tracing_stats();
        assert_eq!(stats.total_spans, 2);
        assert_eq!(stats.error_count, 1);
        assert_eq!(stats.active_spans, 0);
        assert!(stats.total_traces >= 1);
    }

    #[test]
    fn tracing_stats_includes_active() {
        let mut t = new_tracer();
        let _id = t.start_span("alive", SpanKind::Internal);
        let stats = t.tracing_stats();
        assert_eq!(stats.active_spans, 1);
        assert_eq!(stats.total_spans, 0);
    }

    #[test]
    fn tracing_stats_avg_duration() {
        let mut t = new_tracer();
        let id = t.start_span("quick", SpanKind::Internal);
        t.end_span(id, SpanStatus::Ok);
        let stats = t.tracing_stats();
        // Avg should be >= 0 (basically 0 µs for a near-instant span).
        assert!(stats.avg_duration_us < 10_000);
    }

    // -- deep parent-child chain --------------------------------------------

    #[test]
    fn deep_parent_child_chain() {
        let mut t = new_tracer();
        let root = t.start_span("root", SpanKind::Server);
        let mid = t.start_child_span("mid", SpanKind::Internal, root);
        let leaf = t.start_child_span("leaf", SpanKind::Internal, mid);

        // All share the same trace_id.
        let rt = t.get_span(root).unwrap().trace_id;
        let mt = t.get_span(mid).unwrap().trace_id;
        let lt = t.get_span(leaf).unwrap().trace_id;
        assert_eq!(rt, mt);
        assert_eq!(mt, lt);

        // Parent links.
        assert!(t.get_span(root).unwrap().parent_span_id.is_none());
        assert_eq!(t.get_span(mid).unwrap().parent_span_id, Some(root));
        assert_eq!(t.get_span(leaf).unwrap().parent_span_id, Some(mid));
    }

    // -- service_name -------------------------------------------------------

    #[test]
    fn tracer_service_name() {
        let t = new_tracer();
        assert_eq!(t.service_name, "test-service");
    }
}
