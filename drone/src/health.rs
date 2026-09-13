//! Drone health monitoring — status, uptime, and resource usage.
//!
//! Provides a lightweight health-check facility that can be queried locally or
//! exposed over the drone's HTTP API. The [`check_health`] function returns a
//! [`DroneHealth`] snapshot containing the current status, uptime, task
//! throughput, and memory footprint.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ── Health Status ──

/// Coarse health classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Everything is operating normally.
    Healthy,
    /// The drone is functional but experiencing issues (e.g. high memory).
    Degraded,
    /// The drone is in a critical state and may not accept new work.
    Critical,
}

impl HealthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Critical => "critical",
        }
    }
}

// ── DroneHealth ──

/// A point-in-time snapshot of the drone's health.
#[derive(Debug, Clone)]
pub struct DroneHealth {
    pub status: HealthStatus,
    pub uptime_secs: u64,
    pub tasks_completed: u64,
    pub memory_usage_bytes: u64,
}

impl DroneHealth {
    /// Serialize to a JSON value (useful for API responses).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status.as_str(),
            "uptime_secs": self.uptime_secs,
            "tasks_completed": self.tasks_completed,
            "memory_usage_bytes": self.memory_usage_bytes,
        })
    }
}

// ── Boot instant ──

/// The instant the process (or module) first became available.
///
/// Initialised lazily on the first call to [`boot_instant`].
static BOOT_INSTANT: OnceLock<Instant> = OnceLock::new();

fn boot_instant() -> Instant {
    *BOOT_INSTANT.get_or_init(Instant::now)
}

// ── Memory reading (best-effort, cross-platform) ──

/// Read the resident-set size (RSS) of the current process in bytes.
///
/// * **Linux** — reads `/proc/self/status` (VmRSS field).
/// * **macOS** — reads `/proc/self/taskinfo` (not implemented; returns 0).
/// * **Windows** — uses `GetProcessMemoryInfo` via a raw FFI call.
/// * **Other** — returns 0.
fn current_memory_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        return linux_rss_bytes();
    }

    #[cfg(target_os = "windows")]
    {
        windows_working_set_bytes()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        0
    }
}

#[cfg(target_os = "linux")]
fn linux_rss_bytes() -> u64 {
    use std::fs;
    let status = match fs::read_to_string("/proc/self/status") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // Value is in kB.
            let kb = rest
                .trim()
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);
            return kb * 1024;
        }
    }
    0
}

#[cfg(target_os = "windows")]
fn windows_working_set_bytes() -> u64 {
    use std::mem;

    // Minimal FFI — avoids pulling in the `windows` crate.
    #[allow(non_snake_case)]
    #[repr(C)]
    struct PROCESS_MEMORY_COUNTERS {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
    }

    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            hProcess: isize,
            ppsmemCounters: *mut PROCESS_MEMORY_COUNTERS,
            cb: u32,
        ) -> i32;
    }

    // SAFETY: zeroed memory is valid for PROCESS_MEMORY_COUNTERS (all-zero cb field is set immediately after).
    let mut counters = unsafe { mem::zeroed::<PROCESS_MEMORY_COUNTERS>() };
    counters.cb = mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: counters is a valid, properly-initialized PROCESS_MEMORY_COUNTERS pointer.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            counters.cb,
        )
    };
    if ok != 0 {
        counters.WorkingSetSize as u64
    } else {
        0
    }
}

// ── Thresholds ──

/// Memory threshold above which the drone is considered *Degraded* (512 MiB).
const DEGRADED_MEM_BYTES: u64 = 512 * 1024 * 1024;

/// Memory threshold above which the drone is considered *Critical* (1 GiB).
const CRITICAL_MEM_BYTES: u64 = 1024 * 1024 * 1024;

/// Determine health status from raw metrics.
fn classify_health(memory_usage_bytes: u64) -> HealthStatus {
    if memory_usage_bytes >= CRITICAL_MEM_BYTES {
        HealthStatus::Critical
    } else if memory_usage_bytes >= DEGRADED_MEM_BYTES {
        HealthStatus::Degraded
    } else {
        HealthStatus::Healthy
    }
}

// ── Public API ──

/// Collect a health snapshot of the current drone process.
///
/// `tasks_completed` should be supplied by the caller (typically the scheduler)
/// to reflect the drone's throughput since boot.
pub fn check_health_with(tasks_completed: u64) -> DroneHealth {
    let uptime = Instant::now()
        .duration_since(boot_instant())
        .as_secs();
    let memory = current_memory_bytes();
    let status = classify_health(memory);

    DroneHealth {
        status,
        uptime_secs: uptime,
        tasks_completed,
        memory_usage_bytes: memory,
    }
}

/// Collect a health snapshot with zero tasks reported.
///
/// Convenience wrapper around [`check_health_with`].
pub fn check_health() -> DroneHealth {
    check_health_with(0)
}

/// Seconds since the UNIX epoch (useful for timestamps in health reports).
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_as_str() {
        assert_eq!(HealthStatus::Healthy.as_str(), "healthy");
        assert_eq!(HealthStatus::Degraded.as_str(), "degraded");
        assert_eq!(HealthStatus::Critical.as_str(), "critical");
    }

    #[test]
    fn classify_healthy_at_zero() {
        assert_eq!(classify_health(0), HealthStatus::Healthy);
    }

    #[test]
    fn classify_degraded() {
        assert_eq!(classify_health(DEGRADED_MEM_BYTES), HealthStatus::Degraded);
        assert_eq!(
            classify_health(DEGRADED_MEM_BYTES + 1),
            HealthStatus::Degraded
        );
    }

    #[test]
    fn classify_critical() {
        assert_eq!(classify_health(CRITICAL_MEM_BYTES), HealthStatus::Critical);
        assert_eq!(
            classify_health(CRITICAL_MEM_BYTES * 2),
            HealthStatus::Critical
        );
    }

    #[test]
    fn check_health_returns_struct() {
        let h = check_health();
        // Status should be Healthy in a test environment (low memory).
        assert_eq!(h.status, HealthStatus::Healthy);
        // Uptime should be non-negative (u64 is always >= 0).
        // Memory may be 0 on unsupported platforms.
        // tasks_completed defaults to 0.
        assert_eq!(h.tasks_completed, 0);
    }

    #[test]
    fn check_health_with_tasks() {
        let h = check_health_with(42);
        assert_eq!(h.tasks_completed, 42);
    }

    #[test]
    fn drone_health_to_json() {
        let h = DroneHealth {
            status: HealthStatus::Healthy,
            uptime_secs: 120,
            tasks_completed: 7,
            memory_usage_bytes: 1024 * 1024,
        };
        let json = h.to_json();
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["uptime_secs"], 120);
        assert_eq!(json["tasks_completed"], 7);
        assert_eq!(json["memory_usage_bytes"], 1024 * 1024);
    }

    #[test]
    fn boot_instant_is_stable() {
        let a = boot_instant();
        let b = boot_instant();
        // OnceLock guarantees the same value is returned.
        assert_eq!(a, b);
    }

    #[test]
    fn uptime_is_non_decreasing() {
        let h1 = check_health();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let h2 = check_health();
        assert!(h2.uptime_secs >= h1.uptime_secs);
    }

    #[test]
    fn memory_reading_is_non_zero_on_supported_platforms() {
        let mem = current_memory_bytes();
        // On Linux and Windows we expect a non-zero reading.
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        assert!(mem > 0, "expected non-zero memory reading, got {mem}");
        // On other platforms we simply ensure it doesn't panic.
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let _ = mem;
    }
}
