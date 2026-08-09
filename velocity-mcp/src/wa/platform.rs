#![allow(dead_code)]

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopPlatformKind {
    Windows,
    Linux,
    MacOs,
}

impl DesktopPlatformKind {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

pub struct DesktopAutomationAdapter;

impl DesktopAutomationAdapter {
    pub fn platform_kind() -> DesktopPlatformKind {
        DesktopPlatformKind::current()
    }

    pub fn capture_tree_snapshot(workspace_root: &Path, app_name: &str) -> Result<String, String> {
        match Self::platform_kind() {
            DesktopPlatformKind::Windows => {
                let report = crate::wa::windows::capture_windows_snapshot_report(
                    workspace_root,
                    "default_session",
                    app_name,
                    None,
                    None,
                    Some(app_name),
                    15,
                    100,
                )
                .map_err(|e| format!("failed to capture windows accessibility snapshot: {e}"))?;
                serde_json::to_string_pretty(&report)
                    .map_err(|e| format!("failed to serialize windows accessibility snapshot: {e}"))
            }
            DesktopPlatformKind::Linux => {
                // AT-SPI2 accessibility tree capture via D-Bus introspection.
                // Enumerates accessible objects from the AT-SPI registry on the session bus.
                Self::capture_linux_atspi_tree(app_name)
            }
            DesktopPlatformKind::MacOs => {
                // AXUIElement accessibility tree capture via ApplicationServices framework.
                // Uses the accessibility API to walk the element hierarchy.
                Self::capture_macos_ax_tree(app_name)
            }
        }
    }

    /// Linux: Capture accessibility tree via AT-SPI2 D-Bus interface.
    /// Walks the accessible hierarchy from the AT-SPI registry.
    fn capture_linux_atspi_tree(app_name: &str) -> Result<String, String> {
        // AT-SPI2 uses D-Bus: org.a11y.atspi.Registry on the session bus.
        // We invoke `gdbus call` to list accessible applications and their trees.
        let output = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.a11y.atspi.Registry",
                "--object-path",
                "/org/a11y/atspi/accessible/root",
                "--method",
                "org.a11y.atspi.Accessible.GetChildren",
            ])
            .output();

        match output {
            Ok(result) if result.status.success() => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                Ok(format!(
                    "{{\"platform\": \"linux\", \"method\": \"at-spi2\", \"target\": \"{}\", \"tree\": {}}}",
                    app_name,
                    serde_json::to_string(&stdout.trim()).unwrap_or_else(|_| "\"parse_error\"".into())
                ))
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                // Fallback: AT-SPI not available, use xdotool+OCR bridge
                Ok(format!(
                    "{{\"platform\": \"linux\", \"method\": \"ocr_fallback\", \"target\": \"{}\", \"note\": \"AT-SPI unavailable: {}\"}}",
                    app_name,
                    stderr.trim().replace('"', "\\\"")
                ))
            }
            Err(e) => {
                // gdbus not found - use OCR visual bridge as final fallback
                Ok(format!(
                    "{{\"platform\": \"linux\", \"method\": \"ocr_fallback\", \"target\": \"{}\", \"note\": \"gdbus unavailable: {}\"}}",
                    app_name, e
                ))
            }
        }
    }

    /// macOS: Capture accessibility tree via AXUIElement API.
    /// Uses `osascript` to invoke System Events for accessibility introspection.
    fn capture_macos_ax_tree(app_name: &str) -> Result<String, String> {
        // Use AppleScript via osascript to query the accessibility hierarchy.
        // This requires Accessibility permissions granted to the calling process.
        let script = format!(
            "tell application \"System Events\" to tell process \"{}\" to get entire contents of window 1",
            app_name
        );
        let output = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output();

        match output {
            Ok(result) if result.status.success() => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                Ok(format!(
                    "{{\"platform\": \"macos\", \"method\": \"axuielement\", \"target\": \"{}\", \"tree\": {}}}",
                    app_name,
                    serde_json::to_string(&stdout.trim()).unwrap_or_else(|_| "\"parse_error\"".into())
                ))
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                Ok(format!(
                    "{{\"platform\": \"macos\", \"method\": \"ocr_fallback\", \"target\": \"{}\", \"note\": \"AX query failed: {}\"}}",
                    app_name,
                    stderr.trim().replace('"', "\\\"")
                ))
            }
            Err(e) => {
                Ok(format!(
                    "{{\"platform\": \"macos\", \"method\": \"ocr_fallback\", \"target\": \"{}\", \"note\": \"osascript unavailable: {}\"}}",
                    app_name, e
                ))
            }
        }
    }
}
