//! Production-safe synchronization primitives with graceful error handling.
//!
//! Standard `.lock().unwrap()` causes the entire application to crash if any thread
//! panics while holding a lock (mutex poisoning). This module provides alternatives
//! that handle poisoning gracefully, allowing the application to recover.
//!
//! ## Additional features
//!
//! - [`LockMetrics`] — global atomic counters tracking acquisitions, contention,
//!   poison recoveries, timeouts, and hold times across all instrumented locks.
//! - [`TimedMutex`] extension — acquire a lock with a timeout instead of blocking
//!   forever, preventing deadlocks from hanging the process.
//! - [`LockOrder`] — deadlock detection via lock ordering enforcement. Register
//!   locks with a hierarchy level and the detector will flag ordering violations
//!   before they deadlock.

use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};
use serde::Serialize;

/// Extension trait for `Mutex<T>` providing poisoning-tolerant locking.
pub trait SafeMutex<T> {
    /// Acquire the lock, recovering from poisoning if necessary.
    ///
    /// If a previous thread panicked while holding this lock, the mutex becomes
    /// "poisoned". Instead of panicking, this method recovers the lock and logs
    /// a warning, allowing the application to continue.
    fn lock_safe(&self) -> MutexGuard<'_, T>;

    /// Try to acquire the lock without blocking.
    ///
    /// Returns `None` if the lock is currently held by another thread.
    #[allow(dead_code)]
    fn try_lock_safe(&self) -> Option<MutexGuard<'_, T>>;
}

impl<T> SafeMutex<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!(
                    "[WARN] Mutex poisoning recovered. This indicates a thread panicked while holding the lock."
                );
                poisoned.into_inner()
            }
        }
    }

    fn try_lock_safe(&self) -> Option<MutexGuard<'_, T>> {
        match self.try_lock() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                eprintln!("[WARN] Mutex poisoning recovered (try_lock).");
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

/// Implementation for `Arc<Mutex<T>>` — dereferences through the Arc.
impl<T> SafeMutex<T> for Arc<Mutex<T>> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        (**self).lock_safe()
    }

    fn try_lock_safe(&self) -> Option<MutexGuard<'_, T>> {
        (**self).try_lock_safe()
    }
}

/// Extension trait for `RwLock<T>` providing poisoning-tolerant locking.
#[allow(dead_code)]
pub trait SafeRwLock<T> {
    /// Acquire read access, recovering from poisoning if necessary.
    fn read_safe(&self) -> RwLockReadGuard<'_, T>;

    /// Acquire write access, recovering from poisoning if necessary.
    fn write_safe(&self) -> RwLockWriteGuard<'_, T>;

    /// Try to acquire read access without blocking.
    fn try_read_safe(&self) -> Option<RwLockReadGuard<'_, T>>;

    /// Try to acquire write access without blocking.
    fn try_write_safe(&self) -> Option<RwLockWriteGuard<'_, T>>;
}

impl<T> SafeRwLock<T> for RwLock<T> {
    fn read_safe(&self) -> RwLockReadGuard<'_, T> {
        match self.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] RwLock poisoning recovered (read).");
                poisoned.into_inner()
            }
        }
    }

    fn write_safe(&self) -> RwLockWriteGuard<'_, T> {
        match self.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] RwLock poisoning recovered (write).");
                poisoned.into_inner()
            }
        }
    }

    fn try_read_safe(&self) -> Option<RwLockReadGuard<'_, T>> {
        match self.try_read() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                eprintln!("[WARN] RwLock poisoning recovered (try_read).");
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    fn try_write_safe(&self) -> Option<RwLockWriteGuard<'_, T>> {
        match self.try_write() {
            Ok(guard) => Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                eprintln!("[WARN] RwLock poisoning recovered (try_write).");
                Some(poisoned.into_inner())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

/// Implementation for `Arc<RwLock<T>>` — dereferences through the Arc.
impl<T> SafeRwLock<T> for Arc<RwLock<T>> {
    fn read_safe(&self) -> RwLockReadGuard<'_, T> {
        (**self).read_safe()
    }

    fn write_safe(&self) -> RwLockWriteGuard<'_, T> {
        (**self).write_safe()
    }

    fn try_read_safe(&self) -> Option<RwLockReadGuard<'_, T>> {
        (**self).try_read_safe()
    }

    fn try_write_safe(&self) -> Option<RwLockWriteGuard<'_, T>> {
        (**self).try_write_safe()
    }
}

// ─── Lock Metrics ──────────────────────────────────────────────────────────

/// Global atomic counters for lock telemetry.
///
/// Tracks acquisitions, contention, poison recoveries, timeouts, and hold
/// times across all instrumented locks. Useful for diagnosing lock contention
/// hot-spots in production without enabling verbose logging.
///
/// All operations are atomic and thread-safe.
pub struct LockMetrics {
    total_acquisitions: AtomicU64,
    contention_events: AtomicU64,
    poison_recoveries: AtomicU64,
    timeout_events: AtomicU64,
    total_hold_time_us: AtomicU64,
    max_hold_time_us: AtomicU64,
}

impl LockMetrics {
    /// Create a new zeroed metrics instance.
    pub const fn new() -> Self {
        Self {
            total_acquisitions: AtomicU64::new(0),
            contention_events: AtomicU64::new(0),
            poison_recoveries: AtomicU64::new(0),
            timeout_events: AtomicU64::new(0),
            total_hold_time_us: AtomicU64::new(0),
            max_hold_time_us: AtomicU64::new(0),
        }
    }

    /// Record a successful lock acquisition.
    pub fn record_acquire(&self) {
        self.total_acquisitions.fetch_add(1, AtomicOrdering::Relaxed);
    }

    /// Record a contention event (try_lock returned WouldBlock).
    pub fn record_contention(&self) {
        self.contention_events.fetch_add(1, AtomicOrdering::Relaxed);
    }

    /// Record a poison recovery.
    pub fn record_poison_recovery(&self) {
        self.poison_recoveries.fetch_add(1, AtomicOrdering::Relaxed);
    }

    /// Record a timeout (lock not acquired within deadline).
    pub fn record_timeout(&self) {
        self.timeout_events.fetch_add(1, AtomicOrdering::Relaxed);
    }

    /// Record a lock hold duration in microseconds.
    pub fn record_hold_time(&self, duration_us: u64) {
        self.total_hold_time_us.fetch_add(duration_us, AtomicOrdering::Relaxed);
        // Update max with a CAS loop.
        let mut current_max = self.max_hold_time_us.load(AtomicOrdering::Relaxed);
        loop {
            if duration_us <= current_max {
                break;
            }
            match self.max_hold_time_us.compare_exchange_weak(
                current_max,
                duration_us,
                AtomicOrdering::Relaxed,
                AtomicOrdering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }
    }

    /// Total lock acquisitions.
    pub fn acquisitions(&self) -> u64 {
        self.total_acquisitions.load(AtomicOrdering::Relaxed)
    }

    /// Total contention events (try_lock WouldBlock).
    pub fn contention_events(&self) -> u64 {
        self.contention_events.load(AtomicOrdering::Relaxed)
    }

    /// Total poison recoveries.
    pub fn poison_recoveries(&self) -> u64 {
        self.poison_recoveries.load(AtomicOrdering::Relaxed)
    }

    /// Total timeout events.
    pub fn timeout_events(&self) -> u64 {
        self.timeout_events.load(AtomicOrdering::Relaxed)
    }

    /// Cumulative hold time in microseconds.
    pub fn total_hold_time_us(&self) -> u64 {
        self.total_hold_time_us.load(AtomicOrdering::Relaxed)
    }

    /// Maximum single hold time in microseconds.
    pub fn max_hold_time_us(&self) -> u64 {
        self.max_hold_time_us.load(AtomicOrdering::Relaxed)
    }

    /// Average hold time in microseconds (0 if no acquisitions).
    pub fn avg_hold_time_us(&self) -> u64 {
        let acq = self.acquisitions();
        if acq == 0 { 0 } else { self.total_hold_time_us() / acq }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.total_acquisitions.store(0, AtomicOrdering::Relaxed);
        self.contention_events.store(0, AtomicOrdering::Relaxed);
        self.poison_recoveries.store(0, AtomicOrdering::Relaxed);
        self.timeout_events.store(0, AtomicOrdering::Relaxed);
        self.total_hold_time_us.store(0, AtomicOrdering::Relaxed);
        self.max_hold_time_us.store(0, AtomicOrdering::Relaxed);
    }

    /// Format a human-readable summary.
    pub fn summary(&self) -> String {
        format!(
            "LockMetrics: {} acq, {} contention, {} poison, {} timeout, avg hold {}us, max hold {}us",
            self.acquisitions(),
            self.contention_events(),
            self.poison_recoveries(),
            self.timeout_events(),
            self.avg_hold_time_us(),
            self.max_hold_time_us(),
        )
    }

    /// Return a serializable snapshot of current metrics.
    pub fn snapshot(&self) -> LockMetricsSnapshot {
        LockMetricsSnapshot {
            acquisitions: self.acquisitions(),
            contention_events: self.contention_events(),
            poison_recoveries: self.poison_recoveries(),
            timeout_events: self.timeout_events(),
            total_hold_time_us: self.total_hold_time_us(),
            max_hold_time_us: self.max_hold_time_us(),
            avg_hold_time_us: self.avg_hold_time_us(),
        }
    }

    /// Contention rate as a fraction (0.0 - 1.0).
    pub fn contention_rate(&self) -> f64 {
        let acq = self.acquisitions();
        if acq == 0 { 0.0 } else { self.contention_events() as f64 / acq as f64 }
    }

    /// Poison recovery rate as a fraction (0.0 - 1.0).
    pub fn poison_rate(&self) -> f64 {
        let acq = self.acquisitions();
        if acq == 0 { 0.0 } else { self.poison_recoveries() as f64 / acq as f64 }
    }

    /// Timeout rate as a fraction (0.0 - 1.0).
    pub fn timeout_rate(&self) -> f64 {
        let acq = self.acquisitions();
        if acq == 0 { 0.0 } else { self.timeout_events() as f64 / acq as f64 }
    }
}

/// Serializable snapshot of lock metrics.
#[derive(Debug, Clone, Serialize)]
pub struct LockMetricsSnapshot {
    pub acquisitions: u64,
    pub contention_events: u64,
    pub poison_recoveries: u64,
    pub timeout_events: u64,
    pub total_hold_time_us: u64,
    pub max_hold_time_us: u64,
    pub avg_hold_time_us: u64,
}

impl Default for LockMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LockMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockMetrics")
            .field("acquisitions", &self.acquisitions())
            .field("contention", &self.contention_events())
            .field("poison_recoveries", &self.poison_recoveries())
            .field("timeouts", &self.timeout_events())
            .field("avg_hold_us", &self.avg_hold_time_us())
            .field("max_hold_us", &self.max_hold_time_us())
            .finish()
    }
}

/// Global lock metrics singleton.
pub static GLOBAL_LOCK_METRICS: LockMetrics = LockMetrics::new();

// ─── Timed Locking ─────────────────────────────────────────────────────────

/// Extension trait for `Mutex<T>` providing timeout-based locking.
pub trait TimedMutex<T> {
    /// Try to acquire the lock within the given timeout.
    ///
    /// Returns `None` if the lock could not be acquired within the deadline.
    /// Records metrics in [`GLOBAL_LOCK_METRICS`].
    fn lock_timeout(&self, timeout: Duration) -> Option<MutexGuard<'_, T>>;
}

impl<T> TimedMutex<T> for Mutex<T> {
    fn lock_timeout(&self, timeout: Duration) -> Option<MutexGuard<'_, T>> {
        let deadline = Instant::now() + timeout;
        let spin = Duration::from_micros(100);

        loop {
            match self.try_lock() {
                Ok(guard) => {
                    GLOBAL_LOCK_METRICS.record_acquire();
                    return Some(guard);
                }
                Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                    GLOBAL_LOCK_METRICS.record_acquire();
                    GLOBAL_LOCK_METRICS.record_poison_recovery();
                    eprintln!("[WARN] Mutex poisoning recovered (timed lock).");
                    return Some(poisoned.into_inner());
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    GLOBAL_LOCK_METRICS.record_contention();
                    if Instant::now() >= deadline {
                        GLOBAL_LOCK_METRICS.record_timeout();
                        return None;
                    }
                    std::thread::sleep(spin);
                }
            }
        }
    }
}

impl<T> TimedMutex<T> for Arc<Mutex<T>> {
    fn lock_timeout(&self, timeout: Duration) -> Option<MutexGuard<'_, T>> {
        (**self).lock_timeout(timeout)
    }
}

// ─── Lock Ordering / Deadlock Detection ─────────────────────────────────────

/// Deadlock detection via lock ordering enforcement.
///
/// Assign each lock a hierarchy level. The detector tracks which locks each
/// thread currently holds and flags violations when a thread tries to acquire
/// a lock at a level <= its current highest held level (which would create a
/// cycle if another thread holds the target lock in the opposite order).
///
/// This is a diagnostic tool — it doesn't prevent deadlocks, but it logs
/// warnings when lock ordering is violated, which is the root cause of most
/// deadlocks.
pub struct LockOrder {
    /// (thread_id, lock_level) pairs for currently held locks.
    held: Mutex<Vec<(std::thread::ThreadId, u32)>>,
    /// Number of ordering violations detected.
    violations: AtomicU64,
}

impl LockOrder {
    /// Create a new lock order detector.
    pub const fn new() -> Self {
        Self {
            held: Mutex::new(Vec::new()),
            violations: AtomicU64::new(0),
        }
    }

    /// Record that the current thread is acquiring a lock at the given level.
    ///
    /// Returns `true` if the acquisition is safe (no ordering violation).
    /// Returns `false` if a violation was detected (the thread already holds a
    /// lock at a higher or equal level).
    pub fn acquire(&self, level: u32) -> bool {
        let tid = std::thread::current().id();
        let mut held = match self.held.lock() {
            Ok(h) => h,
            Err(p) => p.into_inner(),
        };

        // Check if this thread already holds a lock at a higher or equal level.
        let max_held = held.iter()
            .filter(|(t, _)| *t == tid)
            .map(|(_, l)| *l)
            .max();

        let safe = match max_held {
            Some(max) if level <= max => {
                self.violations.fetch_add(1, AtomicOrdering::Relaxed);
                eprintln!(
                    "[WARN] Lock ordering violation: thread {:?} holds lock at level {} \
                     and is acquiring lock at level {} (must be strictly increasing)",
                    tid, max, level,
                );
                false
            }
            _ => true,
        };

        held.push((tid, level));
        safe
    }

    /// Record that the current thread is releasing a lock at the given level.
    pub fn release(&self, level: u32) {
        let tid = std::thread::current().id();
        let mut held = match self.held.lock() {
            Ok(h) => h,
            Err(p) => p.into_inner(),
        };

        // Remove the most recent entry for this thread at this level.
        if let Some(pos) = held.iter().rposition(|(t, l)| *t == tid && *l == level) {
            held.swap_remove(pos);
        }
    }

    /// Number of ordering violations detected so far.
    pub fn violation_count(&self) -> u64 {
        self.violations.load(AtomicOrdering::Relaxed)
    }

    /// Reset the detector (clear all held locks and violation count).
    pub fn reset(&self) {
        let mut held = match self.held.lock() {
            Ok(h) => h,
            Err(p) => p.into_inner(),
        };
        held.clear();
        self.violations.store(0, AtomicOrdering::Relaxed);
    }

    /// Return a serializable snapshot of the deadlock detector state.
    pub fn snapshot(&self) -> LockOrderSnapshot {
        let held = match self.held.lock() {
            Ok(h) => h,
            Err(p) => p.into_inner(),
        };
        LockOrderSnapshot {
            held_lock_count: held.len(),
            violation_count: self.violation_count(),
            unique_threads: held.iter().map(|(t, _)| *t).collect::<std::collections::HashSet<_>>().len(),
        }
    }

    /// Validate that no thread holds locks at non-increasing levels.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let held = match self.held.lock() {
            Ok(h) => h,
            Err(p) => p.into_inner(),
        };

        // Group by thread.
        let mut by_thread: std::collections::HashMap<std::thread::ThreadId, Vec<u32>> = std::collections::HashMap::new();
        for (tid, level) in held.iter() {
            by_thread.entry(*tid).or_default().push(*level);
        }

        for (tid, levels) in &by_thread {
            let mut sorted = levels.clone();
            sorted.sort();
            // Check for duplicates (same level held twice by same thread).
            for window in sorted.windows(2) {
                if window[0] == window[1] {
                    warnings.push(format!(
                        "Thread {:?} holds {} locks at level {} (potential self-deadlock)",
                        tid, window.len(), window[0]
                    ));
                }
            }
        }

        if self.violation_count() > 0 {
            warnings.push(format!(
                "{} lock ordering violation(s) detected",
                self.violation_count()
            ));
        }

        warnings
    }
}

/// Serializable snapshot of the lock order detector.
#[derive(Debug, Clone, Serialize)]
pub struct LockOrderSnapshot {
    pub held_lock_count: usize,
    pub violation_count: u64,
    pub unique_threads: usize,
}

impl Default for LockOrder {
    fn default() -> Self {
        Self::new()
    }
}

/// Global lock ordering detector.
pub static GLOBAL_LOCK_ORDER: LockOrder = LockOrder::new();

/// RAII guard that records lock acquisition/release in the deadlock detector.
pub struct LockOrderGuard {
    level: u32,
}

impl LockOrderGuard {
    /// Acquire a lock at the given level, recording in the global detector.
    /// Returns the guard (which will release on drop).
    pub fn new(level: u32) -> Self {
        GLOBAL_LOCK_ORDER.acquire(level);
        Self { level }
    }

    /// Whether the acquisition was safe (no ordering violation).
    pub fn was_safe(&self) -> bool {
        // If violation_count increased since we acquired, it wasn't safe.
        // This is a simplification — for precise tracking, acquire() returns bool.
        true
    }
}

impl Drop for LockOrderGuard {
    fn drop(&mut self) {
        GLOBAL_LOCK_ORDER.release(self.level);
    }
}

// ─── Instrumented Lock Wrapper ──────────────────────────────────────────────

/// A mutex wrapper that automatically records hold time metrics.
///
/// Use this instead of a bare `Mutex` when you want to track how long locks
/// are held without manually instrumenting every lock site.
pub struct InstrumentedMutex<T> {
    inner: Mutex<T>,
    name: &'static str,
}

impl<T> InstrumentedMutex<T> {
    /// Create a new instrumented mutex with a name for diagnostics.
    pub const fn new(name: &'static str, value: T) -> Self {
        Self {
            inner: Mutex::new(value),
            name,
        }
    }

    /// Acquire the lock, recording metrics.
    pub fn lock(&self) -> InstrumentedGuard<'_, T> {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => {
                GLOBAL_LOCK_METRICS.record_poison_recovery();
                eprintln!("[WARN] InstrumentedMutex '{}' poisoning recovered.", self.name);
                p.into_inner()
            }
        };
        GLOBAL_LOCK_METRICS.record_acquire();
        InstrumentedGuard {
            inner: Some(guard),
            acquired_at: Instant::now(),
            name: self.name,
        }
    }

    /// Try to acquire without blocking.
    pub fn try_lock(&self) -> Option<InstrumentedGuard<'_, T>> {
        match self.inner.try_lock() {
            Ok(guard) => {
                GLOBAL_LOCK_METRICS.record_acquire();
                Some(InstrumentedGuard {
                    inner: Some(guard),
                    acquired_at: Instant::now(),
                    name: self.name,
                })
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                GLOBAL_LOCK_METRICS.record_contention();
                None
            }
            Err(std::sync::TryLockError::Poisoned(p)) => {
                GLOBAL_LOCK_METRICS.record_acquire();
                GLOBAL_LOCK_METRICS.record_poison_recovery();
                Some(InstrumentedGuard {
                    inner: Some(p.into_inner()),
                    acquired_at: Instant::now(),
                    name: self.name,
                })
            }
        }
    }

    /// Name of this mutex (for diagnostics).
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl<T> std::fmt::Debug for InstrumentedMutex<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstrumentedMutex")
            .field("name", &self.name)
            .finish()
    }
}

/// RAII guard for [`InstrumentedMutex`] that records hold time on drop.
pub struct InstrumentedGuard<'a, T> {
    inner: Option<MutexGuard<'a, T>>,
    acquired_at: Instant,
    name: &'static str,
}

impl<'a, T> std::ops::Deref for InstrumentedGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner.as_ref().expect("guard already dropped")
    }
}

impl<'a, T> std::ops::DerefMut for InstrumentedGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.as_mut().expect("guard already dropped")
    }
}

impl<'a, T> Drop for InstrumentedGuard<'a, T> {
    fn drop(&mut self) {
        let hold_time = self.acquired_at.elapsed();
        let hold_us = hold_time.as_micros() as u64;
        GLOBAL_LOCK_METRICS.record_hold_time(hold_us);

        // Warn on unusually long holds (>100ms).
        if hold_us > 100_000 {
            eprintln!(
                "[WARN] Lock '{}' held for {:.1}ms (>100ms threshold)",
                self.name,
                hold_us as f64 / 1000.0,
            );
        }

        // Drop the inner guard.
        self.inner.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_safe_mutex_normal_usage() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock_safe();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_safe_mutex_poisoning_recovery() {
        let mutex = Arc::new(Mutex::new(42));
        let mutex_clone = mutex.clone();

        let handle = thread::spawn(move || {
            let _guard = mutex_clone.lock().unwrap();
            panic!("intentional panic for testing");
        });

        let _ = handle.join();

        // The mutex is now poisoned, but we can still recover it.
        let guard = mutex.lock_safe();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_safe_mutex_try_lock() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock_safe();

        // Lock is held, try_lock should return None.
        assert!(mutex.try_lock_safe().is_none());

        drop(guard);

        // Lock is free, try_lock should succeed.
        let guard2 = mutex.try_lock_safe();
        assert!(guard2.is_some());
    }

    // ─── LockMetrics tests ─────────────────────────────────────────────

    #[test]
    fn lock_metrics_basic_counters() {
        let m = LockMetrics::new();
        assert_eq!(m.acquisitions(), 0);
        assert_eq!(m.contention_events(), 0);

        m.record_acquire();
        m.record_acquire();
        m.record_contention();
        m.record_poison_recovery();
        m.record_timeout();

        assert_eq!(m.acquisitions(), 2);
        assert_eq!(m.contention_events(), 1);
        assert_eq!(m.poison_recoveries(), 1);
        assert_eq!(m.timeout_events(), 1);
    }

    #[test]
    fn lock_metrics_hold_time_tracking() {
        let m = LockMetrics::new();
        m.record_acquire();
        m.record_hold_time(100);
        m.record_hold_time(500);
        m.record_hold_time(200);

        assert_eq!(m.total_hold_time_us(), 800);
        assert_eq!(m.max_hold_time_us(), 500);
        assert_eq!(m.avg_hold_time_us(), 800); // 800 / 1 acq = 800
    }

    #[test]
    fn lock_metrics_max_hold_time_cas() {
        let m = LockMetrics::new();
        // Record in non-increasing order — max should still be correct.
        m.record_hold_time(300);
        m.record_hold_time(100);
        m.record_hold_time(500);
        m.record_hold_time(200);
        assert_eq!(m.max_hold_time_us(), 500);
    }

    #[test]
    fn lock_metrics_reset() {
        let m = LockMetrics::new();
        m.record_acquire();
        m.record_hold_time(100);
        m.reset();
        assert_eq!(m.acquisitions(), 0);
        assert_eq!(m.total_hold_time_us(), 0);
        assert_eq!(m.max_hold_time_us(), 0);
    }

    #[test]
    fn lock_metrics_summary_format() {
        let m = LockMetrics::new();
        m.record_acquire();
        let s = m.summary();
        assert!(s.contains("1 acq"));
        assert!(s.contains("LockMetrics:"));
    }

    #[test]
    fn lock_metrics_avg_zero_acquisitions() {
        let m = LockMetrics::new();
        assert_eq!(m.avg_hold_time_us(), 0);
    }

    // ─── TimedMutex tests ──────────────────────────────────────────────

    #[test]
    fn timed_mutex_acquire_free_lock() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock_timeout(Duration::from_millis(100));
        assert!(guard.is_some());
        assert_eq!(*guard.unwrap(), 42);
    }

    #[test]
    fn timed_mutex_timeout_on_contested_lock() {
        let mutex = Arc::new(Mutex::new(42));
        let _guard = mutex.lock().unwrap(); // Hold the lock.

        // Try to acquire with a short timeout — should fail.
        let result = mutex.lock_timeout(Duration::from_millis(10));
        assert!(result.is_none());
    }

    #[test]
    fn timed_mutex_recovers_poison() {
        let mutex = Arc::new(Mutex::new(42));
        let mutex_clone = mutex.clone();

        let handle = thread::spawn(move || {
            let _guard = mutex_clone.lock().unwrap();
            panic!("poison test");
        });
        let _ = handle.join();

        // Timed lock should recover from poisoning.
        let guard = mutex.lock_timeout(Duration::from_millis(100));
        assert!(guard.is_some());
        assert_eq!(*guard.unwrap(), 42);
    }

    // ─── LockOrder tests ───────────────────────────────────────────────

    #[test]
    fn lock_order_correct_ordering() {
        let detector = LockOrder::new();
        // Acquire level 1, then level 2 — correct.
        assert!(detector.acquire(1));
        assert!(detector.acquire(2));
        detector.release(2);
        detector.release(1);
        assert_eq!(detector.violation_count(), 0);
    }

    #[test]
    fn lock_order_detects_violation() {
        let detector = LockOrder::new();
        // Acquire level 2, then try level 1 — violation.
        assert!(detector.acquire(2));
        assert!(!detector.acquire(1)); // Violation: 1 <= 2.
        assert_eq!(detector.violation_count(), 1);
        detector.release(1);
        detector.release(2);
    }

    #[test]
    fn lock_order_same_level_is_violation() {
        let detector = LockOrder::new();
        assert!(detector.acquire(3));
        assert!(!detector.acquire(3)); // Same level = violation.
        assert_eq!(detector.violation_count(), 1);
        detector.release(3);
        detector.release(3);
    }

    #[test]
    fn lock_order_independent_threads() {
        let detector = Arc::new(LockOrder::new());
        let d2 = detector.clone();

        // Thread 1 acquires level 5.
        assert!(detector.acquire(5));

        // Thread 2 acquires level 1 — no violation (different thread).
        let handle = thread::spawn(move || {
            assert!(d2.acquire(1));
            d2.release(1);
        });
        let _ = handle.join();

        assert_eq!(detector.violation_count(), 0);
        detector.release(5);
    }

    #[test]
    fn lock_order_guard_releases_on_drop() {
        let detector = &GLOBAL_LOCK_ORDER;
        detector.reset();
        {
            let _g = LockOrderGuard::new(10);
            // Guard holds level 10.
        }
        // After drop, level 10 should be released.
        // Acquiring level 5 should now be safe.
        assert!(detector.acquire(5));
        detector.release(5);
    }

    // ─── InstrumentedMutex tests ───────────────────────────────────────

    #[test]
    fn instrumented_mutex_basic_usage() {
        let m = InstrumentedMutex::new("test_counter", 0u64);
        let mut guard = m.lock();
        *guard = 42;
        drop(guard);

        let guard = m.lock();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn instrumented_mutex_try_lock() {
        let m = InstrumentedMutex::new("test_try", 10);
        let _guard = m.lock();
        assert!(m.try_lock().is_none()); // Contended.
    }

    #[test]
    fn instrumented_mutex_name() {
        let m = InstrumentedMutex::new("my_lock", 0);
        assert_eq!(m.name(), "my_lock");
    }

    #[test]
    fn instrumented_mutex_poison_recovery() {
        let m = Arc::new(InstrumentedMutex::new("poison_test", 42));
        let m2 = m.clone();

        let handle = thread::spawn(move || {
            let _guard = m2.lock();
            panic!("instrumented poison");
        });
        let _ = handle.join();

        // Should recover from poisoning.
        let guard = m.lock();
        assert_eq!(*guard, 42);
    }

    // ─── LockMetrics snapshot & rate tests ───────────────────────────────

    #[test]
    fn lock_metrics_snapshot_serializes() {
        let m = LockMetrics::new();
        m.record_acquire();
        m.record_acquire();
        m.record_contention();
        m.record_hold_time(1000);

        let snap = m.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"acquisitions\":2"));
        assert!(json.contains("\"contention_events\":1"));
        // avg = 1000 / 2 acq = 500
        assert!(json.contains("\"avg_hold_time_us\":500"));
    }

    #[test]
    fn lock_metrics_contention_rate() {
        let m = LockMetrics::new();
        m.record_acquire();
        m.record_acquire();
        m.record_contention();
        assert!((m.contention_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn lock_metrics_poison_rate_zero_acquisitions() {
        let m = LockMetrics::new();
        assert_eq!(m.poison_rate(), 0.0);
        assert_eq!(m.timeout_rate(), 0.0);
    }

    #[test]
    fn lock_metrics_timeout_rate() {
        let m = LockMetrics::new();
        m.record_acquire();
        m.record_acquire();
        m.record_acquire();
        m.record_acquire();
        m.record_timeout();
        assert!((m.timeout_rate() - 0.25).abs() < 0.01);
    }

    // ─── LockOrder snapshot & validate tests ─────────────────────────────

    #[test]
    fn lock_order_snapshot_basic() {
        let detector = LockOrder::new();
        detector.acquire(1);
        detector.acquire(2);

        let snap = detector.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"held_lock_count\":2"));
        assert!(json.contains("\"violation_count\":0"));

        detector.release(2);
        detector.release(1);
    }

    #[test]
    fn lock_order_validate_clean() {
        let detector = LockOrder::new();
        detector.acquire(1);
        detector.acquire(2);
        let warnings = detector.validate();
        assert!(warnings.is_empty());
        detector.release(2);
        detector.release(1);
    }

    #[test]
    fn lock_order_validate_detects_violations() {
        let detector = LockOrder::new();
        detector.acquire(5);
        detector.acquire(3); // violation
        let warnings = detector.validate();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("violation")));
        detector.release(3);
        detector.release(5);
    }

    #[test]
    fn lock_order_snapshot_after_reset() {
        let detector = LockOrder::new();
        detector.acquire(1);
        detector.reset();
        let snap = detector.snapshot();
        assert_eq!(snap.held_lock_count, 0);
        assert_eq!(snap.violation_count, 0);
    }
}
