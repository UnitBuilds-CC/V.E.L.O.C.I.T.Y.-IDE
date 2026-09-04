#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! Multi-monitor support for Windows desktop automation.
//!
//! Enumerates connected displays, provides coordinate transformation between
//! monitor-local and virtual-screen-global coordinates, handles DPI scaling
//! awareness per monitor, and supports targeting specific monitors for capture.

use std::io::Write;
use std::process::{Command, Stdio};

// ─── Monitor Info Model ──────────────────────────────────────────────────────

/// Information about a connected display monitor.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// Monitor index (0 = primary).
    pub index: u32,
    /// Device name (e.g., "\\\\.\\DISPLAY1").
    pub device_name: String,
    /// Friendly name if available (e.g., "Dell U2722D").
    pub friendly_name: Option<String>,
    /// Full bounds in virtual screen coordinates.
    pub bounds: MonitorRect,
    /// Work area (excluding taskbar) in virtual screen coordinates.
    pub work_area: MonitorRect,
    /// Whether this is the primary monitor.
    pub is_primary: bool,
    /// DPI scale factor (1.0 = 100%, 1.5 = 150%, 2.0 = 200%).
    pub dpi_scale: f64,
    /// Raw DPI value (e.g., 96, 144, 192).
    pub dpi: u32,
    /// Physical resolution (before scaling).
    pub physical_width: u32,
    pub physical_height: u32,
    /// Refresh rate in Hz.
    pub refresh_rate: u32,
    /// Orientation.
    pub orientation: MonitorOrientation,
}

#[derive(Debug, Clone, Copy)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl MonitorRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x + self.width as i32
            && y >= self.y
            && y < self.y + self.height as i32
    }

    pub fn center(&self) -> (i32, i32) {
        (
            self.x + self.width as i32 / 2,
            self.y + self.height as i32 / 2,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorOrientation {
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

// ─── Multi-Monitor Manager ───────────────────────────────────────────────────

/// Manages monitor enumeration and coordinate mapping.
pub struct MultiMonitorManager {
    monitors: Vec<MonitorInfo>,
}

impl MultiMonitorManager {
    /// Create with a known set of monitors (populated via enumeration).
    pub fn new(monitors: Vec<MonitorInfo>) -> Self {
        Self { monitors }
    }

    /// Create an empty manager (will enumerate on first use).
    pub fn empty() -> Self {
        Self {
            monitors: Vec::new(),
        }
    }

    /// Number of connected monitors.
    pub fn count(&self) -> usize {
        self.monitors.len()
    }

    /// Get the primary monitor.
    pub fn primary(&self) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.is_primary)
    }

    /// Get monitor by index.
    pub fn get(&self, index: u32) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.index == index)
    }

    /// Get all monitors ordered by index.
    pub fn all(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Find which monitor contains the given virtual-screen point.
    pub fn monitor_at_point(&self, x: i32, y: i32) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.bounds.contains(x, y))
    }

    /// Convert virtual-screen coordinates to monitor-local coordinates.
    pub fn to_local_coords(&self, virtual_x: i32, virtual_y: i32) -> Option<(u32, i32, i32)> {
        self.monitor_at_point(virtual_x, virtual_y)
            .map(|m| (m.index, virtual_x - m.bounds.x, virtual_y - m.bounds.y))
    }

    /// Convert monitor-local coordinates to virtual-screen coordinates.
    pub fn to_virtual_coords(
        &self,
        monitor_index: u32,
        local_x: i32,
        local_y: i32,
    ) -> Option<(i32, i32)> {
        self.get(monitor_index)
            .map(|m| (m.bounds.x + local_x, m.bounds.y + local_y))
    }

    /// Apply DPI scaling to a coordinate on a specific monitor.
    /// Converts from logical pixels to physical pixels.
    pub fn logical_to_physical(
        &self,
        monitor_index: u32,
        logical_x: i32,
        logical_y: i32,
    ) -> Option<(i32, i32)> {
        self.get(monitor_index).map(|m| {
            (
                (logical_x as f64 * m.dpi_scale) as i32,
                (logical_y as f64 * m.dpi_scale) as i32,
            )
        })
    }

    /// Convert physical pixels to logical pixels for a specific monitor.
    pub fn physical_to_logical(
        &self,
        monitor_index: u32,
        phys_x: i32,
        phys_y: i32,
    ) -> Option<(i32, i32)> {
        self.get(monitor_index).map(|m| {
            (
                (phys_x as f64 / m.dpi_scale) as i32,
                (phys_y as f64 / m.dpi_scale) as i32,
            )
        })
    }

    /// Get the total virtual screen bounding box.
    pub fn virtual_screen_bounds(&self) -> MonitorRect {
        if self.monitors.is_empty() {
            return MonitorRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            };
        }
        let min_x = self.monitors.iter().map(|m| m.bounds.x).min().unwrap_or(0);
        let min_y = self.monitors.iter().map(|m| m.bounds.y).min().unwrap_or(0);
        let max_x = self
            .monitors
            .iter()
            .map(|m| m.bounds.x + m.bounds.width as i32)
            .max()
            .unwrap_or(1920);
        let max_y = self
            .monitors
            .iter()
            .map(|m| m.bounds.y + m.bounds.height as i32)
            .max()
            .unwrap_or(1080);
        MonitorRect {
            x: min_x,
            y: min_y,
            width: (max_x - min_x) as u32,
            height: (max_y - min_y) as u32,
        }
    }

    /// Refresh the monitor list by querying PowerShell.
    pub fn refresh(&mut self) -> bool {
        if !cfg!(target_os = "windows") {
            return false;
        }
        let script = build_enumerate_monitors_script();
        match run_ps_script(&script) {
            Ok(json) => {
                self.monitors = parse_monitor_list(&json);
                !self.monitors.is_empty()
            }
            Err(_) => false,
        }
    }

    /// Find the best monitor for a given window size (most available space).
    pub fn best_monitor_for_size(&self, width: u32, height: u32) -> Option<&MonitorInfo> {
        self.monitors
            .iter()
            .filter(|m| m.work_area.width >= width && m.work_area.height >= height)
            .max_by_key(|m| m.work_area.width as u64 * m.work_area.height as u64)
    }
}

// ─── Runtime Helpers ─────────────────────────────────────────────────────────

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_monitor_list(json: &str) -> Vec<MonitorInfo> {
    #[derive(serde::Deserialize)]
    struct PsMonitor {
        index: Option<u32>,
        device_name: Option<String>,
        is_primary: Option<bool>,
        bounds_x: Option<i32>,
        bounds_y: Option<i32>,
        bounds_w: Option<u32>,
        bounds_h: Option<u32>,
        work_x: Option<i32>,
        work_y: Option<i32>,
        work_w: Option<u32>,
        work_h: Option<u32>,
        dpi: Option<u32>,
        dpi_scale: Option<f64>,
        bits_per_pixel: Option<u32>,
    }
    let parsed: Result<Vec<PsMonitor>, _> = serde_json::from_str(json);
    match parsed {
        Ok(monitors) => monitors
            .into_iter()
            .map(|m| {
                let dpi = m.dpi.unwrap_or(96);
                let dpi_scale = m.dpi_scale.unwrap_or(1.0);
                let bw = m.bounds_w.unwrap_or(1920);
                let bh = m.bounds_h.unwrap_or(1080);
                MonitorInfo {
                    index: m.index.unwrap_or(0),
                    device_name: m.device_name.unwrap_or_default(),
                    friendly_name: None,
                    bounds: MonitorRect {
                        x: m.bounds_x.unwrap_or(0),
                        y: m.bounds_y.unwrap_or(0),
                        width: bw,
                        height: bh,
                    },
                    work_area: MonitorRect {
                        x: m.work_x.unwrap_or(0),
                        y: m.work_y.unwrap_or(0),
                        width: m.work_w.unwrap_or(bw),
                        height: m.work_h.unwrap_or(bh),
                    },
                    is_primary: m.is_primary.unwrap_or(false),
                    dpi_scale,
                    dpi,
                    physical_width: (bw as f64 * dpi_scale) as u32,
                    physical_height: (bh as f64 * dpi_scale) as u32,
                    refresh_rate: 0,
                    orientation: if bh > bw {
                        MonitorOrientation::Portrait
                    } else {
                        MonitorOrientation::Landscape
                    },
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ─── PowerShell Monitor Enumeration Script ───────────────────────────────────

/// Build a PowerShell script that enumerates all monitors with their properties.
pub fn build_enumerate_monitors_script() -> String {
    r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class DpiHelper {
    [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr hMonitor, int dpiType, out uint dpiX, out uint dpiY);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT pt, uint dwFlags);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
}
'@
$monitors = @()
$idx = 0
foreach ($screen in [System.Windows.Forms.Screen]::AllScreens) {
    $pt = New-Object DpiHelper+POINT
    $pt.X = $screen.Bounds.X + 1
    $pt.Y = $screen.Bounds.Y + 1
    $hMon = [DpiHelper]::MonitorFromPoint($pt, 0)
    $dpiX = 96; $dpiY = 96
    try { [DpiHelper]::GetDpiForMonitor($hMon, 0, [ref]$dpiX, [ref]$dpiY) | Out-Null } catch {}
    $scale = [math]::Round($dpiX / 96.0, 2)
    $monitors += @{
        index = $idx
        device_name = $screen.DeviceName
        is_primary = $screen.Primary
        bounds_x = $screen.Bounds.X
        bounds_y = $screen.Bounds.Y
        bounds_w = $screen.Bounds.Width
        bounds_h = $screen.Bounds.Height
        work_x = $screen.WorkingArea.X
        work_y = $screen.WorkingArea.Y
        work_w = $screen.WorkingArea.Width
        work_h = $screen.WorkingArea.Height
        dpi = $dpiX
        dpi_scale = $scale
        bits_per_pixel = $screen.BitsPerPixel
    }
    $idx++
}
ConvertTo-Json $monitors -Compress
"#
    .to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn two_monitor_setup() -> MultiMonitorManager {
        MultiMonitorManager::new(vec![
            MonitorInfo {
                index: 0,
                device_name: "\\\\.\\DISPLAY1".to_string(),
                friendly_name: Some("Primary 4K".to_string()),
                bounds: MonitorRect {
                    x: 0,
                    y: 0,
                    width: 3840,
                    height: 2160,
                },
                work_area: MonitorRect {
                    x: 0,
                    y: 0,
                    width: 3840,
                    height: 2112,
                },
                is_primary: true,
                dpi_scale: 1.5,
                dpi: 144,
                physical_width: 3840,
                physical_height: 2160,
                refresh_rate: 60,
                orientation: MonitorOrientation::Landscape,
            },
            MonitorInfo {
                index: 1,
                device_name: "\\\\.\\DISPLAY2".to_string(),
                friendly_name: Some("Secondary 1080p".to_string()),
                bounds: MonitorRect {
                    x: 3840,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                work_area: MonitorRect {
                    x: 3840,
                    y: 0,
                    width: 1920,
                    height: 1040,
                },
                is_primary: false,
                dpi_scale: 1.0,
                dpi: 96,
                physical_width: 1920,
                physical_height: 1080,
                refresh_rate: 144,
                orientation: MonitorOrientation::Landscape,
            },
        ])
    }

    #[test]
    fn finds_primary_monitor() {
        let mgr = two_monitor_setup();
        let primary = mgr.primary().unwrap();
        assert!(primary.is_primary);
        assert_eq!(primary.index, 0);
    }

    #[test]
    fn point_on_second_monitor() {
        let mgr = two_monitor_setup();
        let mon = mgr.monitor_at_point(4000, 500).unwrap();
        assert_eq!(mon.index, 1);
    }

    #[test]
    fn coordinate_transforms() {
        let mgr = two_monitor_setup();
        // Virtual (4000, 500) → local on monitor 1 → (160, 500)
        let (idx, local_x, local_y) = mgr.to_local_coords(4000, 500).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(local_x, 160);
        assert_eq!(local_y, 500);

        // Reverse: monitor 1, local (160, 500) → virtual (4000, 500)
        let (vx, vy) = mgr.to_virtual_coords(1, 160, 500).unwrap();
        assert_eq!(vx, 4000);
        assert_eq!(vy, 500);
    }

    #[test]
    fn dpi_scaling() {
        let mgr = two_monitor_setup();
        // Monitor 0 has 1.5x DPI
        let (px, py) = mgr.logical_to_physical(0, 100, 200).unwrap();
        assert_eq!(px, 150);
        assert_eq!(py, 300);

        let (lx, ly) = mgr.physical_to_logical(0, 150, 300).unwrap();
        assert_eq!(lx, 100);
        assert_eq!(ly, 200);
    }

    #[test]
    fn virtual_screen_bounds() {
        let mgr = two_monitor_setup();
        let bounds = mgr.virtual_screen_bounds();
        assert_eq!(bounds.x, 0);
        assert_eq!(bounds.y, 0);
        assert_eq!(bounds.width, 3840 + 1920); // side by side
        assert_eq!(bounds.height, 2160); // tallest
    }

    #[test]
    fn best_monitor_for_large_window() {
        let mgr = two_monitor_setup();
        // A 2000x1200 window only fits on the primary (3840x2112 work area)
        let best = mgr.best_monitor_for_size(2000, 1200).unwrap();
        assert_eq!(best.index, 0);
    }

    #[test]
    fn enumerate_script_uses_shcore() {
        let script = build_enumerate_monitors_script();
        assert!(script.contains("GetDpiForMonitor"));
        assert!(script.contains("AllScreens"));
    }
}
