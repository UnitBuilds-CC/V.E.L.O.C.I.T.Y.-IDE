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

    pub fn capture_tree_snapshot(
        workspace_root: &Path,
        app_name: &str,
    ) -> Result<String, String> {
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
                ).map_err(|e| format!("failed to capture windows accessibility snapshot: {e}"))?;
                serde_json::to_string_pretty(&report)
                    .map_err(|e| format!("failed to serialize windows accessibility snapshot: {e}"))
            }
            DesktopPlatformKind::Linux | DesktopPlatformKind::MacOs => {
                Ok(format!(
                    "Accessibility tree capture for {:?} platform is stubbed/supported via fallback OCR visual bridge.",
                    Self::platform_kind()
                ))
            }
        }
    }
}
