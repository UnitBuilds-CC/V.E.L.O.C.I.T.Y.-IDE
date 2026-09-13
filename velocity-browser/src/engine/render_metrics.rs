//! Rendering performance metrics and instrumentation for the browser engine.
//!
//! Provides lightweight tracking of frame render times, DOM layout duration,
//! paint/composite timing, and frame budget monitoring for 60fps targets.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Target frame budget for 60fps rendering (16.67ms).
pub const TARGET_FRAME_BUDGET_MS: f64 = 16.67;

/// Histogram bucket boundaries for frame time classification.
pub const BUCKET_GREAT_MAX_MS: f64 = 8.0;
pub const BUCKET_GOOD_MAX_MS: f64 = 16.0;
pub const BUCKET_ACCEPTABLE_MAX_MS: f64 = 33.0;

/// Rendering performance metrics tracker.
///
/// Uses atomic operations for lock-free reads of counters while maintaining
/// minimal overhead (<1% target). Timing uses `std::time::Instant` for
/// high-resolution measurements.
pub struct RenderMetrics {
    /// Frame render time in microseconds (atomic for lock-free reads).
    frame_render_time_us: AtomicU64,
    /// DOM layout time in microseconds.
    dom_layout_time_us: AtomicU64,
    /// Paint/composite time in microseconds.
    paint_composite_time_us: AtomicU64,
    /// Frame budget usage as percentage (0-10000 = 0.00%-100.00%).
    frame_budget_percent: AtomicU32,
    /// Total frame drop count (frames exceeding 33ms).
    frame_drop_count: AtomicU64,
    /// Total frames rendered.
    frame_count: AtomicU64,
    /// Sum of all frame times for averaging (in microseconds).
    total_frame_time_us: AtomicU64,
    /// Minimum frame time seen (in microseconds).
    min_frame_time_us: AtomicU64,
    /// Maximum frame time seen (in microseconds).
    max_frame_time_us: AtomicU64,
    /// Recent frame times for p95 calculation (ring buffer).
    recent_frames: std::sync::Mutex<Vec<u64>>,
    /// Maximum number of recent frames to track.
    max_recent_frames: usize,
}

impl Default for RenderMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderMetrics {
    /// Create a new RenderMetrics tracker.
    pub fn new() -> Self {
        Self {
            frame_render_time_us: AtomicU64::new(0),
            dom_layout_time_us: AtomicU64::new(0),
            paint_composite_time_us: AtomicU64::new(0),
            frame_budget_percent: AtomicU32::new(0),
            frame_drop_count: AtomicU64::new(0),
            frame_count: AtomicU64::new(0),
            total_frame_time_us: AtomicU64::new(0),
            min_frame_time_us: AtomicU64::new(u64::MAX),
            max_frame_time_us: AtomicU64::new(0),
            recent_frames: std::sync::Mutex::new(Vec::with_capacity(1000)),
            max_recent_frames: 1000,
        }
    }

    /// Record a frame's timing data.
    ///
    /// # Arguments
    /// * `frame_time` - Total frame render time
    /// * `layout_time` - DOM layout phase time
    /// * `paint_time` - Paint/composite phase time
    pub fn record_frame(&self, frame_time: Duration, layout_time: Duration, paint_time: Duration) {
        let frame_us = frame_time.as_micros() as u64;
        let layout_us = layout_time.as_micros() as u64;
        let paint_us = paint_time.as_micros() as u64;

        // Update current frame timings
        self.frame_render_time_us.store(frame_us, Ordering::Relaxed);
        self.dom_layout_time_us.store(layout_us, Ordering::Relaxed);
        self.paint_composite_time_us.store(paint_us, Ordering::Relaxed);

        // Calculate budget percentage (frame_time / 16.67ms * 100)
        let budget_pct = ((frame_us as f64) / (TARGET_FRAME_BUDGET_MS * 1000.0) * 100.0) as u32;
        self.frame_budget_percent.store(budget_pct.min(10000), Ordering::Relaxed);

        // Update frame count
        self.frame_count.fetch_add(1, Ordering::Relaxed);

        // Update total time for averaging
        self.total_frame_time_us.fetch_add(frame_us, Ordering::Relaxed);

        // Update min (using CAS loop for atomicity)
        let mut current_min = self.min_frame_time_us.load(Ordering::Relaxed);
        loop {
            if frame_us >= current_min {
                break;
            }
            match self.min_frame_time_us.compare_exchange_weak(
                current_min,
                frame_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // Update max (using CAS loop for atomicity)
        let mut current_max = self.max_frame_time_us.load(Ordering::Relaxed);
        loop {
            if frame_us <= current_max {
                break;
            }
            match self.max_frame_time_us.compare_exchange_weak(
                current_max,
                frame_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }

        // Track frame drops (>33ms)
        if frame_us > 33_000 {
            self.frame_drop_count.fetch_add(1, Ordering::Relaxed);
        }

        // Add to recent frames ring buffer
        if let Ok(mut recent) = self.recent_frames.lock() {
            recent.push(frame_us);
            if recent.len() > self.max_recent_frames {
                recent.remove(0);
            }
        }
    }

    /// Get current frame render time in milliseconds.
    pub fn frame_render_time_ms(&self) -> f64 {
        self.frame_render_time_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Get current DOM layout time in milliseconds.
    pub fn dom_layout_time_ms(&self) -> f64 {
        self.dom_layout_time_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Get current paint/composite time in milliseconds.
    pub fn paint_composite_time_ms(&self) -> f64 {
        self.paint_composite_time_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Get current frame budget usage as fraction (0.0 - 1.0, where 1.0 = 100%).
    pub fn frame_budget_percent(&self) -> f64 {
        self.frame_budget_percent.load(Ordering::Relaxed) as f64 / 100.0
    }

    /// Get total frame drop count.
    pub fn frame_drop_count(&self) -> u64 {
        self.frame_drop_count.load(Ordering::Relaxed)
    }

    /// Get total frame count.
    pub fn frame_count(&self) -> u64 {
        self.frame_count.load(Ordering::Relaxed)
    }

    /// Get average frame time in milliseconds.
    pub fn avg_frame_time_ms(&self) -> f64 {
        let count = self.frame_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let total = self.total_frame_time_us.load(Ordering::Relaxed);
        (total as f64 / count as f64) / 1000.0
    }

    /// Get minimum frame time in milliseconds.
    pub fn min_frame_time_ms(&self) -> f64 {
        let min = self.min_frame_time_us.load(Ordering::Relaxed);
        if min == u64::MAX {
            0.0
        } else {
            min as f64 / 1000.0
        }
    }

    /// Get maximum frame time in milliseconds.
    pub fn max_frame_time_ms(&self) -> f64 {
        self.max_frame_time_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Get p95 frame time in milliseconds.
    pub fn p95_frame_time_ms(&self) -> f64 {
        let mut recent = match self.recent_frames.lock() {
            Ok(r) => r.clone(),
            Err(_) => return 0.0,
        };
        if recent.is_empty() {
            return 0.0;
        }
        recent.sort_unstable();
        let idx = (recent.len() as f64 * 0.95).ceil() as usize;
        let idx = idx.min(recent.len()).saturating_sub(1);
        recent[idx] as f64 / 1000.0
    }

    /// Generate a summary string with render statistics.
    pub fn render_stats(&self) -> String {
        format!(
            "Render Stats: frames={} avg={:.2}ms min={:.2}ms max={:.2}ms p95={:.2}ms drops={} budget={:.1}%",
            self.frame_count(),
            self.avg_frame_time_ms(),
            self.min_frame_time_ms(),
            self.max_frame_time_ms(),
            self.p95_frame_time_ms(),
            self.frame_drop_count(),
            self.frame_budget_percent() * 100.0
        )
    }

    /// Reset all metrics to initial state.
    pub fn reset(&self) {
        self.frame_render_time_us.store(0, Ordering::Relaxed);
        self.dom_layout_time_us.store(0, Ordering::Relaxed);
        self.paint_composite_time_us.store(0, Ordering::Relaxed);
        self.frame_budget_percent.store(0, Ordering::Relaxed);
        self.frame_drop_count.store(0, Ordering::Relaxed);
        self.frame_count.store(0, Ordering::Relaxed);
        self.total_frame_time_us.store(0, Ordering::Relaxed);
        self.min_frame_time_us.store(u64::MAX, Ordering::Relaxed);
        self.max_frame_time_us.store(0, Ordering::Relaxed);
        if let Ok(mut recent) = self.recent_frames.lock() {
            recent.clear();
        }
    }
}

/// Frame budget monitor that warns when rendering exceeds the 16ms target.
pub struct FrameBudget {
    /// Warning threshold in milliseconds (default: 16.0).
    pub warn_threshold_ms: f64,
    /// Critical threshold in milliseconds (default: 33.0).
    pub critical_threshold_ms: f64,
    /// Number of warnings issued.
    warning_count: AtomicU64,
    /// Number of critical alerts issued.
    critical_count: AtomicU64,
    /// Last warning timestamp.
    last_warning: std::sync::Mutex<Option<Instant>>,
}

impl Default for FrameBudget {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameBudget {
    /// Create a new FrameBudget monitor with default thresholds.
    pub fn new() -> Self {
        Self {
            warn_threshold_ms: 16.0,
            critical_threshold_ms: 33.0,
            warning_count: AtomicU64::new(0),
            critical_count: AtomicU64::new(0),
            last_warning: std::sync::Mutex::new(None),
        }
    }

    /// Create a FrameBudget monitor with custom thresholds.
    pub fn with_thresholds(warn_ms: f64, critical_ms: f64) -> Self {
        Self {
            warn_threshold_ms: warn_ms,
            critical_threshold_ms: critical_ms,
            warning_count: AtomicU64::new(0),
            critical_count: AtomicU64::new(0),
            last_warning: std::sync::Mutex::new(None),
        }
    }

    /// Check a frame time against the budget and return the status.
    pub fn check_frame(&self, frame_time: Duration) -> FrameBudgetStatus {
        let ms = frame_time.as_secs_f64() * 1000.0;

        if ms >= self.critical_threshold_ms {
            self.critical_count.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = self.last_warning.lock() {
                *last = Some(Instant::now());
            }
            FrameBudgetStatus::Critical
        } else if ms >= self.warn_threshold_ms {
            self.warning_count.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = self.last_warning.lock() {
                *last = Some(Instant::now());
            }
            FrameBudgetStatus::Warning
        } else {
            FrameBudgetStatus::Ok
        }
    }

    /// Get the number of warnings issued.
    pub fn warning_count(&self) -> u64 {
        self.warning_count.load(Ordering::Relaxed)
    }

    /// Get the number of critical alerts issued.
    pub fn critical_count(&self) -> u64 {
        self.critical_count.load(Ordering::Relaxed)
    }

    /// Get time since last warning/critical alert.
    pub fn time_since_last_warning(&self) -> Option<Duration> {
        self.last_warning
            .lock()
            .ok()
            .and_then(|last| last.map(|t| t.elapsed()))
    }

    /// Reset warning counters.
    pub fn reset(&self) {
        self.warning_count.store(0, Ordering::Relaxed);
        self.critical_count.store(0, Ordering::Relaxed);
        if let Ok(mut last) = self.last_warning.lock() {
            *last = None;
        }
    }
}

/// Status of a frame budget check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBudgetStatus {
    /// Frame completed within budget (< 16ms).
    Ok,
    /// Frame exceeded warning threshold (16ms - 33ms).
    Warning,
    /// Frame exceeded critical threshold (> 33ms).
    Critical,
}

/// Render performance histogram that buckets frame times.
///
/// Buckets:
/// - Great: < 8ms
/// - Good: 8-16ms
/// - Acceptable: 16-33ms
/// - Dropped: > 33ms
pub struct RenderPerformanceHistogram {
    /// Count of frames in the "great" bucket (< 8ms).
    great_count: AtomicU64,
    /// Count of frames in the "good" bucket (8-16ms).
    good_count: AtomicU64,
    /// Count of frames in the "acceptable" bucket (16-33ms).
    acceptable_count: AtomicU64,
    /// Count of frames in the "dropped" bucket (> 33ms).
    dropped_count: AtomicU64,
}

impl Default for RenderPerformanceHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderPerformanceHistogram {
    /// Create a new empty histogram.
    pub fn new() -> Self {
        Self {
            great_count: AtomicU64::new(0),
            good_count: AtomicU64::new(0),
            acceptable_count: AtomicU64::new(0),
            dropped_count: AtomicU64::new(0),
        }
    }

    /// Record a frame time in the appropriate bucket.
    pub fn record(&self, frame_time: Duration) {
        let ms = frame_time.as_secs_f64() * 1000.0;
        self.record_ms(ms)
    }

    /// Record a frame time in milliseconds.
    pub fn record_ms(&self, frame_time_ms: f64) {
        if frame_time_ms < BUCKET_GREAT_MAX_MS {
            self.great_count.fetch_add(1, Ordering::Relaxed);
        } else if frame_time_ms < BUCKET_GOOD_MAX_MS {
            self.good_count.fetch_add(1, Ordering::Relaxed);
        } else if frame_time_ms < BUCKET_ACCEPTABLE_MAX_MS {
            self.acceptable_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.dropped_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get the count of frames in the "great" bucket.
    pub fn great_count(&self) -> u64 {
        self.great_count.load(Ordering::Relaxed)
    }

    /// Get the count of frames in the "good" bucket.
    pub fn good_count(&self) -> u64 {
        self.good_count.load(Ordering::Relaxed)
    }

    /// Get the count of frames in the "acceptable" bucket.
    pub fn acceptable_count(&self) -> u64 {
        self.acceptable_count.load(Ordering::Relaxed)
    }

    /// Get the count of frames in the "dropped" bucket.
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// Get total frame count across all buckets.
    pub fn total_count(&self) -> u64 {
        self.great_count() + self.good_count() + self.acceptable_count() + self.dropped_count()
    }

    /// Get histogram as a formatted string.
    pub fn summary(&self) -> String {
        let total = self.total_count();
        if total == 0 {
            return "Histogram: no frames recorded".to_string();
        }
        format!(
            "Histogram: great(<8ms)={} good(8-16ms)={} acceptable(16-33ms)={} dropped(>33ms)={} total={}",
            self.great_count(),
            self.good_count(),
            self.acceptable_count(),
            self.dropped_count(),
            total
        )
    }

    /// Get percentage of frames in each bucket.
    pub fn percentages(&self) -> (f64, f64, f64, f64) {
        let total = self.total_count() as f64;
        if total == 0.0 {
            return (0.0, 0.0, 0.0, 0.0);
        }
        (
            self.great_count() as f64 / total * 100.0,
            self.good_count() as f64 / total * 100.0,
            self.acceptable_count() as f64 / total * 100.0,
            self.dropped_count() as f64 / total * 100.0,
        )
    }

    /// Reset all bucket counts.
    pub fn reset(&self) {
        self.great_count.store(0, Ordering::Relaxed);
        self.good_count.store(0, Ordering::Relaxed);
        self.acceptable_count.store(0, Ordering::Relaxed);
        self.dropped_count.store(0, Ordering::Relaxed);
    }
}

/// Scoped timer for measuring a code block's duration.
///
/// Usage:
/// ```ignore
/// let metrics = RenderMetrics::new();
/// {
///     let _timer = RenderTimer::start();
///     // ... do work ...
///     let duration = _timer.elapsed();
/// }
/// ```
pub struct RenderTimer {
    start: Instant,
}

impl RenderTimer {
    /// Start a new timer.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get elapsed time since start.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Get elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_metrics_new() {
        let metrics = RenderMetrics::new();
        assert_eq!(metrics.frame_count(), 0);
        assert_eq!(metrics.frame_drop_count(), 0);
        assert_eq!(metrics.avg_frame_time_ms(), 0.0);
    }

    #[test]
    fn test_record_frame() {
        let metrics = RenderMetrics::new();
        metrics.record_frame(
            Duration::from_millis(10),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        assert_eq!(metrics.frame_count(), 1);
        assert!((metrics.frame_render_time_ms() - 10.0).abs() < 0.1);
        assert!((metrics.dom_layout_time_ms() - 5.0).abs() < 0.1);
        assert!((metrics.paint_composite_time_ms() - 3.0).abs() < 0.1);
    }

    #[test]
    fn test_frame_drop_detection() {
        let metrics = RenderMetrics::new();
        // Record a fast frame (no drop)
        metrics.record_frame(
            Duration::from_millis(10),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        assert_eq!(metrics.frame_drop_count(), 0);

        // Record a slow frame (drop)
        metrics.record_frame(
            Duration::from_millis(50),
            Duration::from_millis(20),
            Duration::from_millis(20),
        );
        assert_eq!(metrics.frame_drop_count(), 1);
    }

    #[test]
    fn test_min_max_frame_times() {
        let metrics = RenderMetrics::new();
        metrics.record_frame(
            Duration::from_millis(10),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        metrics.record_frame(
            Duration::from_millis(20),
            Duration::from_millis(8),
            Duration::from_millis(6),
        );
        metrics.record_frame(
            Duration::from_millis(5),
            Duration::from_millis(2),
            Duration::from_millis(1),
        );

        assert!((metrics.min_frame_time_ms() - 5.0).abs() < 0.1);
        assert!((metrics.max_frame_time_ms() - 20.0).abs() < 0.1);
    }

    #[test]
    fn test_avg_frame_time() {
        let metrics = RenderMetrics::new();
        metrics.record_frame(
            Duration::from_millis(10),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        metrics.record_frame(
            Duration::from_millis(20),
            Duration::from_millis(8),
            Duration::from_millis(6),
        );

        // Average should be 15ms
        assert!((metrics.avg_frame_time_ms() - 15.0).abs() < 0.1);
    }

    #[test]
    fn test_frame_budget_percent() {
        let metrics = RenderMetrics::new();
        // 16.67ms should be ~100% budget
        metrics.record_frame(
            Duration::from_micros(16670),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        // frame_budget_percent returns 0.0-1.0 range (1.0 = 100%)
        let pct = metrics.frame_budget_percent();
        assert!((0.99..=1.01).contains(&pct), "budget percent was {}", pct);
    }

    #[test]
    fn test_render_stats_string() {
        let metrics = RenderMetrics::new();
        metrics.record_frame(
            Duration::from_millis(10),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        let stats = metrics.render_stats();
        assert!(stats.contains("Render Stats:"));
        assert!(stats.contains("frames=1"));
        assert!(stats.contains("avg="));
        assert!(stats.contains("min="));
        assert!(stats.contains("max="));
        assert!(stats.contains("p95="));
    }

    #[test]
    fn test_render_metrics_reset() {
        let metrics = RenderMetrics::new();
        metrics.record_frame(
            Duration::from_millis(10),
            Duration::from_millis(5),
            Duration::from_millis(3),
        );
        metrics.reset();
        assert_eq!(metrics.frame_count(), 0);
        assert_eq!(metrics.frame_drop_count(), 0);
        assert_eq!(metrics.avg_frame_time_ms(), 0.0);
    }

    #[test]
    fn test_frame_budget_new() {
        let budget = FrameBudget::new();
        assert_eq!(budget.warn_threshold_ms, 16.0);
        assert_eq!(budget.critical_threshold_ms, 33.0);
        assert_eq!(budget.warning_count(), 0);
        assert_eq!(budget.critical_count(), 0);
    }

    #[test]
    fn test_frame_budget_ok() {
        let budget = FrameBudget::new();
        let status = budget.check_frame(Duration::from_millis(10));
        assert_eq!(status, FrameBudgetStatus::Ok);
        assert_eq!(budget.warning_count(), 0);
        assert_eq!(budget.critical_count(), 0);
    }

    #[test]
    fn test_frame_budget_warning() {
        let budget = FrameBudget::new();
        let status = budget.check_frame(Duration::from_millis(20));
        assert_eq!(status, FrameBudgetStatus::Warning);
        assert_eq!(budget.warning_count(), 1);
        assert_eq!(budget.critical_count(), 0);
    }

    #[test]
    fn test_frame_budget_critical() {
        let budget = FrameBudget::new();
        let status = budget.check_frame(Duration::from_millis(50));
        assert_eq!(status, FrameBudgetStatus::Critical);
        assert_eq!(budget.warning_count(), 0);
        assert_eq!(budget.critical_count(), 1);
    }

    #[test]
    fn test_frame_budget_custom_thresholds() {
        let budget = FrameBudget::with_thresholds(10.0, 20.0);
        assert_eq!(budget.warn_threshold_ms, 10.0);
        assert_eq!(budget.critical_threshold_ms, 20.0);

        let status = budget.check_frame(Duration::from_millis(15));
        assert_eq!(status, FrameBudgetStatus::Warning);

        let status = budget.check_frame(Duration::from_millis(25));
        assert_eq!(status, FrameBudgetStatus::Critical);
    }

    #[test]
    fn test_frame_budget_reset() {
        let budget = FrameBudget::new();
        budget.check_frame(Duration::from_millis(20));
        budget.check_frame(Duration::from_millis(50));
        budget.reset();
        assert_eq!(budget.warning_count(), 0);
        assert_eq!(budget.critical_count(), 0);
    }

    #[test]
    fn test_histogram_new() {
        let hist = RenderPerformanceHistogram::new();
        assert_eq!(hist.great_count(), 0);
        assert_eq!(hist.good_count(), 0);
        assert_eq!(hist.acceptable_count(), 0);
        assert_eq!(hist.dropped_count(), 0);
        assert_eq!(hist.total_count(), 0);
    }

    #[test]
    fn test_histogram_great_bucket() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(5));
        hist.record(Duration::from_millis(7));
        assert_eq!(hist.great_count(), 2);
        assert_eq!(hist.good_count(), 0);
    }

    #[test]
    fn test_histogram_good_bucket() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(10));
        hist.record(Duration::from_millis(15));
        assert_eq!(hist.great_count(), 0);
        assert_eq!(hist.good_count(), 2);
        assert_eq!(hist.acceptable_count(), 0);
    }

    #[test]
    fn test_histogram_acceptable_bucket() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(20));
        hist.record(Duration::from_millis(30));
        assert_eq!(hist.acceptable_count(), 2);
        assert_eq!(hist.dropped_count(), 0);
    }

    #[test]
    fn test_histogram_dropped_bucket() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(40));
        hist.record(Duration::from_millis(100));
        assert_eq!(hist.dropped_count(), 2);
    }

    #[test]
    fn test_histogram_total_count() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(5));   // great
        hist.record(Duration::from_millis(10));  // good
        hist.record(Duration::from_millis(20));  // acceptable
        hist.record(Duration::from_millis(50));  // dropped
        assert_eq!(hist.total_count(), 4);
    }

    #[test]
    fn test_histogram_summary() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(5));
        let summary = hist.summary();
        assert!(summary.contains("Histogram:"));
        assert!(summary.contains("great(<8ms)=1"));
    }

    #[test]
    fn test_histogram_percentages() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(5));   // great
        hist.record(Duration::from_millis(5));   // great
        hist.record(Duration::from_millis(10));  // good
        hist.record(Duration::from_millis(20));  // acceptable

        let (great, good, acceptable, dropped) = hist.percentages();
        assert!((great - 50.0).abs() < 0.1);     // 2/4 = 50%
        assert!((good - 25.0).abs() < 0.1);      // 1/4 = 25%
        assert!((acceptable - 25.0).abs() < 0.1); // 1/4 = 25%
        assert!((dropped - 0.0).abs() < 0.1);    // 0/4 = 0%
    }

    #[test]
    fn test_histogram_reset() {
        let hist = RenderPerformanceHistogram::new();
        hist.record(Duration::from_millis(5));
        hist.record(Duration::from_millis(50));
        hist.reset();
        assert_eq!(hist.total_count(), 0);
    }

    #[test]
    fn test_histogram_record_ms() {
        let hist = RenderPerformanceHistogram::new();
        hist.record_ms(5.0);
        hist.record_ms(10.0);
        hist.record_ms(20.0);
        hist.record_ms(50.0);
        assert_eq!(hist.great_count(), 1);
        assert_eq!(hist.good_count(), 1);
        assert_eq!(hist.acceptable_count(), 1);
        assert_eq!(hist.dropped_count(), 1);
    }

    #[test]
    fn test_render_timer() {
        let timer = RenderTimer::start();
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = timer.elapsed_ms();
        assert!(elapsed >= 10.0);
    }

    #[test]
    fn test_p95_calculation() {
        let metrics = RenderMetrics::new();
        // Record 100 frames with varying times
        for i in 0..100 {
            metrics.record_frame(
                Duration::from_millis(i as u64),
                Duration::from_millis(1),
                Duration::from_millis(1),
            );
        }
        // p95 should be around 95ms
        let p95 = metrics.p95_frame_time_ms();
        assert!((90.0..=100.0).contains(&p95), "p95 was {}", p95);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let metrics = Arc::new(RenderMetrics::new());
        let mut handles = vec![];

        // Spawn multiple threads recording frames
        for _ in 0..4 {
            let m = Arc::clone(&metrics);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    m.record_frame(
                        Duration::from_millis(10),
                        Duration::from_millis(5),
                        Duration::from_millis(3),
                    );
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(metrics.frame_count(), 400);
    }
}
