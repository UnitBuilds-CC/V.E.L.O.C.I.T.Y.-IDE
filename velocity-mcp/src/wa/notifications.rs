#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! Windows notification and toast interception for desktop automation.
//!
//! Detects, reads, and dismisses Windows toast notifications, system tray
//! popups, and UAC prompts that can block automation workflows.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

// ─── Notification Model ──────────────────────────────────────────────────────

/// A detected Windows notification/toast.
#[derive(Debug, Clone)]
pub struct Notification {
    /// Unique identifier (if available from Action Center).
    pub id: Option<String>,
    /// App that generated the notification.
    pub app_name: String,
    /// Notification title/header.
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// When the notification appeared.
    pub timestamp_ms: u64,
    /// Available actions (button labels).
    pub actions: Vec<String>,
    /// Whether the notification is still visible.
    pub is_visible: bool,
    /// Whether this is a system/UAC prompt vs user notification.
    pub is_system: bool,
}

/// Types of notification interaction.
#[derive(Debug, Clone)]
pub enum NotificationAction {
    /// Click the notification body (usually opens the app).
    Click,
    /// Click a specific action button by label.
    ClickAction(String),
    /// Dismiss/close the notification.
    Dismiss,
    /// Dismiss all notifications.
    DismissAll,
}

/// Result of a notification interaction.
#[derive(Debug, Clone)]
pub struct NotificationResult {
    pub success: bool,
    pub action: String,
    pub detail: String,
    pub notifications_remaining: u32,
}

/// Configuration for notification watching.
#[derive(Debug, Clone)]
pub struct NotificationWatchConfig {
    /// How long to watch for notifications.
    pub duration: Duration,
    /// Whether to auto-dismiss notifications matching patterns.
    pub auto_dismiss_patterns: Vec<String>,
    /// Whether to capture notification content.
    pub capture_content: bool,
    /// Poll interval for checking notifications.
    pub poll_interval: Duration,
}

impl Default for NotificationWatchConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(30),
            auto_dismiss_patterns: Vec::new(),
            capture_content: true,
            poll_interval: Duration::from_millis(500),
        }
    }
}

// ─── System Tray ─────────────────────────────────────────────────────────────

/// System tray icon information.
#[derive(Debug, Clone)]
pub struct TrayIcon {
    /// Tooltip text of the tray icon.
    pub tooltip: String,
    /// Process ID that owns the icon.
    pub process_id: u32,
    /// Whether the icon is visible (vs hidden in overflow).
    pub is_visible: bool,
}

/// Action on a system tray icon.
#[derive(Debug, Clone)]
pub enum TrayAction {
    /// Single left click.
    Click,
    /// Double left click.
    DoubleClick,
    /// Right click (context menu).
    RightClick,
}

// ─── Notification Manager ────────────────────────────────────────────────────

/// Manages notification detection and interaction.
pub struct NotificationManager;

impl NotificationManager {
    /// Get all currently visible notifications via PowerShell UIAutomation.
    pub fn get_visible_notifications() -> Vec<Notification> {
        if !cfg!(target_os = "windows") {
            return Vec::new();
        }
        let script = build_detect_notifications_script();
        match run_ps_script(&script) {
            Ok(json) => parse_notifications_result(&json),
            Err(_) => Vec::new(),
        }
    }

    /// Get notification count from Action Center.
    pub fn get_notification_count() -> u32 {
        Self::get_visible_notifications().len() as u32
    }

    /// Interact with a notification via PowerShell UIAutomation.
    pub fn interact(
        notification: &Notification,
        action: &NotificationAction,
    ) -> NotificationResult {
        if !cfg!(target_os = "windows") {
            return NotificationResult {
                success: false,
                action: "interact".into(),
                detail: "Notification interaction requires Windows".into(),
                notifications_remaining: 0,
            };
        }
        let script = match action {
            NotificationAction::Click => format!(
                r#"Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::RootElement
$w = $root.FindFirst([System.Windows.Automation.TreeScope]::Children,
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, '{}')))
if ($null -ne $w) {{
    $invokePattern = $w.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $invokePattern.Invoke()
    ConvertTo-Json @{{ success = $true }} -Compress
}} else {{ ConvertTo-Json @{{ success = $false }} -Compress }}"#,
                notification.app_name.replace('\'', "''")
            ),
            NotificationAction::ClickAction(label) => format!(
                r#"Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::RootElement
$w = $root.FindFirst([System.Windows.Automation.TreeScope]::Children,
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, '{}')))
if ($null -ne $w) {{
    $btn = $w.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::NameProperty, '{}')))
    if ($null -ne $btn) {{
        $pattern = $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
        ConvertTo-Json @{{ success = $true }} -Compress
    }} else {{ ConvertTo-Json @{{ success = $false }} -Compress }}
}} else {{ ConvertTo-Json @{{ success = $false }} -Compress }}"#,
                notification.app_name.replace('\'', "''"),
                label.replace('\'', "''")
            ),
            NotificationAction::Dismiss => {
                build_dismiss_notifications_script(Some(&notification.app_name))
            }
            NotificationAction::DismissAll => build_dismiss_notifications_script(None),
        };
        match run_ps_script(&script) {
            Ok(json) => {
                let remaining = Self::get_notification_count();
                NotificationResult {
                    success: json.contains("\"success\":true")
                        || json.contains("\"success\": true"),
                    action: format!("{:?}", action),
                    detail: "executed via PowerShell".into(),
                    notifications_remaining: remaining,
                }
            }
            Err(e) => NotificationResult {
                success: false,
                action: format!("{:?}", action),
                detail: e,
                notifications_remaining: Self::get_notification_count(),
            },
        }
    }

    /// Dismiss all visible notifications via PowerShell.
    pub fn dismiss_all() -> NotificationResult {
        if !cfg!(target_os = "windows") {
            return NotificationResult {
                success: false,
                action: "dismiss_all".into(),
                detail: "Notification dismissal requires Windows".into(),
                notifications_remaining: 0,
            };
        }
        let script = build_dismiss_notifications_script(None);
        match run_ps_script(&script) {
            Ok(json) => NotificationResult {
                success: json.contains("\"success\":true") || json.contains("\"success\": true"),
                action: "dismiss_all".into(),
                detail: "dismissed via PowerShell".into(),
                notifications_remaining: 0,
            },
            Err(e) => NotificationResult {
                success: false,
                action: "dismiss_all".into(),
                detail: e,
                notifications_remaining: Self::get_notification_count(),
            },
        }
    }

    /// Dismiss visible notifications whose app name matches `pattern`, or every
    /// notification when `pattern` is `None`, empty, or `"*"`. Unlike the legacy
    /// `script_ready` stub, this actually runs the PowerShell UIAutomation dismiss
    /// pass and reports how many notifications were closed.
    pub fn dismiss_matching(pattern: Option<&str>) -> NotificationResult {
        if !cfg!(target_os = "windows") {
            return NotificationResult {
                success: false,
                action: "dismiss".into(),
                detail: "Notification dismissal requires Windows".into(),
                notifications_remaining: 0,
            };
        }
        let effective = match pattern {
            None | Some("") | Some("*") => None,
            Some(p) => Some(p),
        };
        let script = build_dismiss_notifications_script(effective);
        match run_ps_script(&script) {
            Ok(json) => {
                let dismissed = parse_dismissed_count(&json);
                NotificationResult {
                    success: json.contains("\"success\":true")
                        || json.contains("\"success\": true"),
                    action: "dismiss".into(),
                    detail: format!("dismissed {dismissed} notification(s) via PowerShell"),
                    notifications_remaining: Self::get_notification_count(),
                }
            }
            Err(e) => NotificationResult {
                success: false,
                action: "dismiss".into(),
                detail: e,
                notifications_remaining: Self::get_notification_count(),
            },
        }
    }

    /// Watch for notifications and optionally auto-dismiss.
    pub fn watch(config: &NotificationWatchConfig) -> Vec<Notification> {
        if !cfg!(target_os = "windows") {
            return Vec::new();
        }
        let start = SystemTime::now();
        let mut collected = Vec::new();
        while start.elapsed().unwrap_or(Duration::ZERO) < config.duration {
            let notifications = Self::get_visible_notifications();
            for n in &notifications {
                if config
                    .auto_dismiss_patterns
                    .iter()
                    .any(|p| n.app_name.contains(p) || n.title.contains(p))
                {
                    let _ = Self::interact(n, &NotificationAction::Dismiss);
                } else if config.capture_content {
                    collected.push(n.clone());
                }
            }
            std::thread::sleep(config.poll_interval);
        }
        collected
    }

    /// Check if a UAC prompt is currently visible.
    pub fn is_uac_prompt_visible() -> bool {
        if !cfg!(target_os = "windows") {
            return false;
        }
        let script = r#"
Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::RootElement
$uac = $root.FindFirst([System.Windows.Automation.TreeScope]::Children,
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ClassNameProperty, 'DirectUIHWND')))
ConvertTo-Json @{ uac_visible = ($null -ne $uac) } -Compress"#;
        run_ps_script(script)
            .map(|o| o.contains("\"uac_visible\":true") || o.contains("\"uac_visible\": true"))
            .unwrap_or(false)
    }

    /// Enumerate system tray icons via PowerShell.
    pub fn get_tray_icons() -> Vec<TrayIcon> {
        if !cfg!(target_os = "windows") {
            return Vec::new();
        }
        let script = build_enumerate_tray_script();
        match run_ps_script(&script) {
            Ok(json) => parse_tray_icons_result(&json),
            Err(_) => Vec::new(),
        }
    }

    /// Interact with a tray icon via PowerShell UIAutomation.
    pub fn click_tray_icon(tooltip: &str, action: &TrayAction) -> NotificationResult {
        if !cfg!(target_os = "windows") {
            return NotificationResult {
                success: false,
                action: "tray_click".into(),
                detail: "Tray interaction requires Windows".into(),
                notifications_remaining: 0,
            };
        }
        let script = format!(
            r#"Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::RootElement
$tray = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, '{}')))
if ($null -ne $tray) {{
    $pattern = $tray.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $pattern.Invoke()
    ConvertTo-Json @{{ success = $true }} -Compress
}} else {{ ConvertTo-Json @{{ success = $false }} -Compress }}"#,
            tooltip.replace('\'', "''")
        );
        match run_ps_script(&script) {
            Ok(json) => NotificationResult {
                success: json.contains("\"success\":true") || json.contains("\"success\": true"),
                action: format!("tray_{:?}", action).to_lowercase(),
                detail: format!("clicked tray icon: {}", tooltip),
                notifications_remaining: 0,
            },
            Err(e) => NotificationResult {
                success: false,
                action: "tray_click".into(),
                detail: e,
                notifications_remaining: 0,
            },
        }
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a script to detect visible toast notifications.
pub fn build_detect_notifications_script() -> String {
    r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement

# Look for notification windows (class "Windows.UI.Core.CoreWindow" with notification content)
$notifications = @()
$topLevel = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($w in $topLevel) {
    $class = $w.Current.ClassName
    $name = $w.Current.Name
    if ($class -eq "Windows.UI.Core.CoreWindow" -and $name -match "(Notification|New notification)") {
        $textElements = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.ControlType]::Text)))
        $texts = @($textElements | ForEach-Object { $_.Current.Name } | Where-Object { $_ -ne "" })
        $buttons = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.ControlType]::Button)))
        $actions = @($buttons | ForEach-Object { $_.Current.Name } | Where-Object { $_ -ne "" })
        $notifications += @{
            title = if ($texts.Count -gt 0) { $texts[0] } else { "" }
            body = if ($texts.Count -gt 1) { $texts[1..($texts.Count-1)] -join " " } else { "" }
            actions = $actions
            app_name = $name
            timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        }
    }
}
ConvertTo-Json @{ notifications = @($notifications); count = $notifications.Count } -Compress -Depth 3
"#
    .to_string()
}

/// Build a script to dismiss notifications.
pub fn build_dismiss_notifications_script(pattern: Option<&str>) -> String {
    let filter = pattern
        .map(|p| format!("if ($w.Current.Name -like '*{}*') {{", p))
        .unwrap_or_else(|| "if ($true) {".to_string());
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
$root = [System.Windows.Automation.AutomationElement]::RootElement
$dismissed = 0

# Find and close notification windows
$topLevel = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($w in $topLevel) {{
    $class = $w.Current.ClassName
    if ($class -eq "Windows.UI.Core.CoreWindow") {{
        {filter}
            $closeBtn = $w.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "Close")))
            if ($null -ne $closeBtn) {{
                try {{
                    $pattern = $closeBtn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
                    $pattern.Invoke()
                    $dismissed++
                }} catch {{}}
            }}
        }}
    }}
}}
ConvertTo-Json @{{ success = $true; dismissed = $dismissed }} -Compress
"#
    )
}

/// Build a script to enumerate system tray icons.
pub fn build_enumerate_tray_script() -> String {
    r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement

# Find the system tray
$tray = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "NotifyIconOverflowCharmsBar")))
if ($null -eq $tray) {
    # Try the visible notification area
    $tray = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "Shell_TrayWnd")))
}
$icons = @()
if ($null -ne $tray) {
    $buttons = $tray.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)))
    foreach ($btn in $buttons) {
        $name = $btn.Current.Name
        if ($name -ne "" -and $name -ne "Notification Chevron") {
            $icons += @{ tooltip = $name; process_id = $btn.Current.ProcessId }
        }
    }
}
ConvertTo-Json @{ icons = @($icons); count = $icons.Count } -Compress -Depth 2
"#
    .to_string()
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

fn parse_notifications_result(json: &str) -> Vec<Notification> {
    #[derive(serde::Deserialize)]
    struct PsNotification {
        title: Option<String>,
        body: Option<String>,
        app_name: Option<String>,
        actions: Option<Vec<String>>,
        timestamp_ms: Option<u64>,
    }
    #[derive(serde::Deserialize)]
    struct PsResult {
        notifications: Option<Vec<PsNotification>>,
    }
    match serde_json::from_str::<PsResult>(json) {
        Ok(r) => r
            .notifications
            .unwrap_or_default()
            .into_iter()
            .map(|n| Notification {
                id: None,
                app_name: n.app_name.unwrap_or_default(),
                title: n.title.unwrap_or_default(),
                body: n.body.unwrap_or_default(),
                timestamp_ms: n.timestamp_ms.unwrap_or(0),
                actions: n.actions.unwrap_or_default(),
                is_visible: true,
                is_system: false,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn parse_dismissed_count(json: &str) -> u32 {
    #[derive(serde::Deserialize)]
    struct PsDismiss {
        dismissed: Option<u32>,
    }
    serde_json::from_str::<PsDismiss>(json)
        .ok()
        .and_then(|d| d.dismissed)
        .unwrap_or(0)
}

fn parse_tray_icons_result(json: &str) -> Vec<TrayIcon> {
    #[derive(serde::Deserialize)]
    struct PsIcon {
        tooltip: Option<String>,
        process_id: Option<u32>,
    }
    #[derive(serde::Deserialize)]
    struct PsResult {
        icons: Option<Vec<PsIcon>>,
    }
    match serde_json::from_str::<PsResult>(json) {
        Ok(r) => r
            .icons
            .unwrap_or_default()
            .into_iter()
            .map(|i| TrayIcon {
                tooltip: i.tooltip.unwrap_or_default(),
                process_id: i.process_id.unwrap_or(0),
                is_visible: true,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_script_searches_core_window() {
        let script = build_detect_notifications_script();
        assert!(script.contains("CoreWindow"));
        assert!(script.contains("Notification"));
    }

    #[test]
    fn dismiss_script_clicks_close() {
        let script = build_dismiss_notifications_script(Some("Update"));
        assert!(script.contains("Close"));
        assert!(script.contains("Update"));
        assert!(script.contains("InvokePattern"));
    }

    #[test]
    fn tray_script_finds_notification_area() {
        let script = build_enumerate_tray_script();
        assert!(script.contains("NotifyIconOverflowCharmsBar"));
        assert!(script.contains("Shell_TrayWnd"));
    }

    #[test]
    fn notification_manager_returns_empty() {
        let notifications = NotificationManager::get_visible_notifications();
        assert!(notifications.is_empty());
        assert_eq!(NotificationManager::get_notification_count(), 0);
    }

    #[test]
    fn watch_config_defaults() {
        let config = NotificationWatchConfig::default();
        assert_eq!(config.duration, Duration::from_secs(30));
        assert!(config.capture_content);
    }
}
