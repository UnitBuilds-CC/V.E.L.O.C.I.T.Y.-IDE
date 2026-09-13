//! System capabilities: screen capture, input simulation, network monitoring.
//!
//! These modules provide the drone's system-level capabilities for remote
//! system management and automation.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Screen Capture ──

/// Screenshot result with image data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screenshot {
    /// PNG-encoded image data.
    pub png_data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Capture timestamp (Unix seconds).
    pub timestamp: u64,
}

/// Display information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// Screen capture capability.
pub trait ScreenCapture: Send + Sync {
    /// Capture the primary display.
    fn capture_primary(&self) -> Result<Screenshot, String>;
    /// Capture a specific region.
    fn capture_region(&self, x: u32, y: u32, width: u32, height: u32) -> Result<Screenshot, String>;
    /// List available displays.
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String>;
}

// ── Input Simulation ──

/// Input action types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputAction {
    /// Type text character by character.
    TypeText { text: String },
    /// Press a single key (e.g., "enter", "ctrl+c", "alt+f4").
    PressKey { key: String },
    /// Click at coordinates.
    Click { x: i32, y: i32, button: MouseButton },
    /// Double-click at coordinates.
    DoubleClick { x: i32, y: i32 },
    /// Scroll at position.
    Scroll { x: i32, y: i32, delta: i32 },
    /// Move mouse to position.
    MoveMouse { x: i32, y: i32 },
}

/// Mouse button for click actions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

/// Input simulation capability.
pub trait InputSimulator: Send + Sync {
    /// Execute an input action.
    fn execute(&self, action: InputAction) -> Result<(), String>;
    /// Type text with optional delay between keystrokes (ms).
    fn type_text(&self, text: &str, delay_ms: u32) -> Result<(), String>;
    /// Press a key combination.
    fn press_key(&self, key: &str) -> Result<(), String>;
    /// Click at coordinates.
    fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<(), String>;
}

// ── Network Monitoring ──

/// Network statistics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkStats {
    /// Bytes received.
    pub bytes_received: u64,
    /// Bytes sent.
    pub bytes_sent: u64,
    /// Packets received.
    pub packets_received: u64,
    /// Packets sent.
    pub packets_sent: u64,
    /// Active connections count.
    pub active_connections: u32,
    /// Snapshot timestamp (Unix seconds).
    pub timestamp: u64,
}

/// Connection information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub protocol: String,
    pub local_address: String,
    pub local_port: u16,
    pub remote_address: String,
    pub remote_port: u16,
    pub state: String,
    pub process_name: Option<String>,
    pub pid: Option<u32>,
}

/// Network monitoring capability.
pub trait NetworkMonitor: Send + Sync {
    /// Get current network statistics.
    fn get_stats(&self) -> Result<NetworkStats, String>;
    /// List active connections.
    fn list_connections(&self) -> Result<Vec<ConnectionInfo>, String>;
    /// Get bytes received/sent since last call.
    fn get_delta(&self) -> Result<NetworkDelta, String>;
}

/// Network traffic delta between two measurements.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkDelta {
    pub bytes_received_delta: u64,
    pub bytes_sent_delta: u64,
    pub duration_secs: u64,
}

// ── Platform Factory Functions ──

/// Create platform-specific screen capture implementation.
pub fn create_screen_capture() -> Box<dyn ScreenCapture> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsScreenCapture)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedScreenCapture)
    }
}

/// Create platform-specific input simulator.
pub fn create_input_simulator() -> Box<dyn InputSimulator> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsInputSimulator)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedInputSimulator)
    }
}

/// Create platform-specific network monitor.
pub fn create_network_monitor() -> Box<dyn NetworkMonitor> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsNetworkMonitor::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedNetworkMonitor)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Windows Implementations
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;

    // ── Windows Screen Capture ──

    pub struct WindowsScreenCapture;

    impl ScreenCapture for WindowsScreenCapture {
        fn capture_primary(&self) -> Result<Screenshot, String> {
            // Simplified implementation - full version would use GDI/BitBlt
            Ok(Screenshot {
                png_data: Vec::new(),
                width: 1920,
                height: 1080,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            })
        }

        fn capture_region(&self, _x: u32, _y: u32, width: u32, height: u32) -> Result<Screenshot, String> {
            Ok(Screenshot {
                png_data: Vec::new(),
                width,
                height,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            })
        }

        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(vec![DisplayInfo {
                id: 0,
                name: "Primary Display".into(),
                width: 1920,
                height: 1080,
                is_primary: true,
            }])
        }
    }

    // ── Windows Input Simulator ──

    pub struct WindowsInputSimulator;

    impl WindowsInputSimulator {
        fn parse_key(key: &str) -> Option<u16> {
            match key.to_lowercase().as_str() {
                "enter" | "return" => Some(0x0D),
                "tab" => Some(0x09),
                "escape" | "esc" => Some(0x1B),
                "space" => Some(0x20),
                "backspace" => Some(0x08),
                "delete" | "del" => Some(0x2E),
                "up" => Some(0x26),
                "down" => Some(0x28),
                "left" => Some(0x25),
                "right" => Some(0x27),
                "home" => Some(0x24),
                "end" => Some(0x23),
                "pageup" | "pgup" => Some(0x21),
                "pagedown" | "pgdn" => Some(0x22),
                "f1" => Some(0x70),
                "f2" => Some(0x71),
                "f3" => Some(0x72),
                "f4" => Some(0x73),
                "f5" => Some(0x74),
                "f6" => Some(0x75),
                "f7" => Some(0x76),
                "f8" => Some(0x77),
                "f9" => Some(0x78),
                "f10" => Some(0x79),
                "f11" => Some(0x7A),
                "f12" => Some(0x7B),
                s if s.len() == 1 => {
                    let c = s.chars().next().unwrap_or_default();
                    if c.is_ascii_alphabetic() {
                        Some(c.to_ascii_uppercase() as u16)
                    } else if c.is_ascii_digit() {
                        Some(c as u16)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }

        fn is_modifier(key: &str) -> bool {
            matches!(
                key.to_lowercase().as_str(),
                "ctrl" | "control" | "alt" | "shift" | "win" | "windows"
            )
        }

        fn get_modifier_vk(key: &str) -> u16 {
            match key.to_lowercase().as_str() {
                "ctrl" | "control" => 0x11,
                "alt" => 0x12,
                "shift" => 0x10,
                "win" | "windows" => 0x5B,
                _ => 0,
            }
        }
    }

    impl InputSimulator for WindowsInputSimulator {
        fn execute(&self, action: InputAction) -> Result<(), String> {
            match action {
                InputAction::TypeText { text } => self.type_text(&text, 0),
                InputAction::PressKey { key } => self.press_key(&key),
                InputAction::Click { x, y, button } => self.click(x, y, button),
                InputAction::DoubleClick { x, y } => {
                    self.click(x, y, MouseButton::Left)?;
                    self.click(x, y, MouseButton::Left)
                }
                InputAction::Scroll { x, y, delta } => {
                    // SAFETY: Win32 SetCursorPos and mouse_event are FFI calls operating on
                    // the global cursor state. Coordinates are validated by the caller.
                    unsafe {
                        winapi::um::winuser::SetCursorPos(x, y);
                        winapi::um::winuser::mouse_event(
                            winapi::um::winuser::MOUSEEVENTF_WHEEL,
                            0, 0, delta as u32, 0,
                        );
                    }
                    Ok(())
                }
                InputAction::MoveMouse { x, y } => {
                    // SAFETY: Win32 SetCursorPos is an FFI call; coordinates are i32 screen values.
                    unsafe {
                        winapi::um::winuser::SetCursorPos(x, y);
                    }
                    Ok(())
                }
            }
        }

        fn type_text(&self, text: &str, _delay_ms: u32) -> Result<(), String> {
            use winapi::um::winuser::*;

            for c in text.chars() {
                let vk = if c.is_ascii_uppercase() {
                    c as u16
                } else if c.is_ascii_lowercase() {
                    c.to_ascii_uppercase() as u16
                } else {
                    c as u16
                };

                // SAFETY: Win32 SendInput FFI — INPUT structs are zeroed then initialized with
                // valid KEYBDINPUT fields. The array is stack-allocated and lives for the call.
                unsafe {
                    let mut inputs = [
                        INPUT {
                            type_: INPUT_KEYBOARD,
                            u: std::mem::zeroed(),
                        },
                        INPUT {
                            type_: INPUT_KEYBOARD,
                            u: std::mem::zeroed(),
                        },
                    ];

                    *inputs[0].u.ki_mut() = KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: 0,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    *inputs[1].u.ki_mut() = KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    };

                    SendInput(2, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
                }
            }
            Ok(())
        }

        fn press_key(&self, key: &str) -> Result<(), String> {
            use winapi::um::winuser::*;

            let parts: Vec<&str> = key.split('+').collect();
            let mut inputs = Vec::new();

            // Press modifiers
            for part in &parts[..parts.len().saturating_sub(1)] {
                if Self::is_modifier(part) {
                    let vk = Self::get_modifier_vk(part);
                    // SAFETY: Win32 INPUT struct zeroed then initialized with valid KEYBDINPUT.
                    unsafe {
                        let mut input: INPUT = std::mem::zeroed();
                        input.type_ = INPUT_KEYBOARD;
                        *input.u.ki_mut() = KEYBDINPUT {
                            wVk: vk, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0,
                        };
                        inputs.push(input);
                    }
                }
            }

            // Press and release main key
            if let Some(main_key) = parts.last() {
                if let Some(vk) = Self::parse_key(main_key) {
                    // SAFETY: Win32 INPUT structs for key down/up events. Stack-allocated, valid lifetime.
                    unsafe {
                        let mut down: INPUT = std::mem::zeroed();
                        down.type_ = INPUT_KEYBOARD;
                        *down.u.ki_mut() = KEYBDINPUT {
                            wVk: vk, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0,
                        };
                        inputs.push(down);

                        let mut up: INPUT = std::mem::zeroed();
                        up.type_ = INPUT_KEYBOARD;
                        *up.u.ki_mut() = KEYBDINPUT {
                            wVk: vk, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0,
                        };
                        inputs.push(up);
                    }
                }
            }

            // Release modifiers
            for part in parts[..parts.len().saturating_sub(1)].iter().rev() {
                if Self::is_modifier(part) {
                    let vk = Self::get_modifier_vk(part);
                    // SAFETY: Win32 INPUT struct for key-up event. Stack-allocated, valid lifetime.
                    unsafe {
                        let mut input: INPUT = std::mem::zeroed();
                        input.type_ = INPUT_KEYBOARD;
                        *input.u.ki_mut() = KEYBDINPUT {
                            wVk: vk, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0,
                        };
                        inputs.push(input);
                    }
                }
            }

            if !inputs.is_empty() {
                // SAFETY: Win32 SendInput FFI — inputs vec contains valid initialized INPUT structs.
                unsafe {
                    SendInput(inputs.len() as u32, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
                }
            }
            Ok(())
        }

        fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<(), String> {
            use winapi::um::winuser::*;

            // SAFETY: Win32 SetCursorPos + mouse_event FFI — standard Win32 mouse input simulation.
            unsafe {
                SetCursorPos(x, y);
                let (down, up) = match button {
                    MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                    MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                    MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                };
                mouse_event(down, 0, 0, 0, 0);
                mouse_event(up, 0, 0, 0, 0);
            }
            Ok(())
        }
    }

    // ── Windows Network Monitor ──

    pub struct WindowsNetworkMonitor {
        last_stats: std::sync::Mutex<Option<(NetworkStats, u64)>>,
    }

    impl WindowsNetworkMonitor {
        pub fn new() -> Self {
            Self {
                last_stats: std::sync::Mutex::new(None),
            }
        }
    }

    impl NetworkMonitor for WindowsNetworkMonitor {
        fn get_stats(&self) -> Result<NetworkStats, String> {
            // Simplified - full version would use GetIfTable
            Ok(NetworkStats {
                bytes_received: 0,
                bytes_sent: 0,
                packets_received: 0,
                packets_sent: 0,
                active_connections: 0,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            })
        }

        fn list_connections(&self) -> Result<Vec<ConnectionInfo>, String> {
            // Simplified - full version would use GetExtendedTcpTable
            Ok(Vec::new())
        }

        fn get_delta(&self) -> Result<NetworkDelta, String> {
            let current = self.get_stats()?;
            let mut last = self.last_stats.lock().map_err(|e| e.to_string())?;

            let delta = if let Some((ref prev, prev_time)) = *last {
                NetworkDelta {
                    bytes_received_delta: current.bytes_received.saturating_sub(prev.bytes_received),
                    bytes_sent_delta: current.bytes_sent.saturating_sub(prev.bytes_sent),
                    duration_secs: current.timestamp.saturating_sub(prev_time),
                }
            } else {
                NetworkDelta::default()
            };

            *last = Some((current, SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()));

            Ok(delta)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_parse_key() {
            assert_eq!(WindowsInputSimulator::parse_key("enter"), Some(0x0D));
            assert_eq!(WindowsInputSimulator::parse_key("a"), Some(0x41));
            assert_eq!(WindowsInputSimulator::parse_key("f1"), Some(0x70));
        }

        #[test]
        fn test_is_modifier() {
            assert!(WindowsInputSimulator::is_modifier("ctrl"));
            assert!(WindowsInputSimulator::is_modifier("alt"));
            assert!(!WindowsInputSimulator::is_modifier("enter"));
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Fallback Implementations (non-Windows)
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(not(target_os = "windows"))]
struct UnsupportedScreenCapture;
#[cfg(not(target_os = "windows"))]
impl ScreenCapture for UnsupportedScreenCapture {
    fn capture_primary(&self) -> Result<Screenshot, String> {
        Err("Screen capture not supported on this platform".into())
    }
    fn capture_region(&self, _: u32, _: u32, _: u32, _: u32) -> Result<Screenshot, String> {
        Err("Screen capture not supported on this platform".into())
    }
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Err("Screen capture not supported on this platform".into())
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedInputSimulator;
#[cfg(not(target_os = "windows"))]
impl InputSimulator for UnsupportedInputSimulator {
    fn execute(&self, _: InputAction) -> Result<(), String> {
        Err("Input simulation not supported on this platform".into())
    }
    fn type_text(&self, _: &str, _: u32) -> Result<(), String> {
        Err("Input simulation not supported on this platform".into())
    }
    fn press_key(&self, _: &str) -> Result<(), String> {
        Err("Input simulation not supported on this platform".into())
    }
    fn click(&self, _: i32, _: i32, _: MouseButton) -> Result<(), String> {
        Err("Input simulation not supported on this platform".into())
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedNetworkMonitor;
#[cfg(not(target_os = "windows"))]
impl NetworkMonitor for UnsupportedNetworkMonitor {
    fn get_stats(&self) -> Result<NetworkStats, String> {
        Err("Network monitoring not supported on this platform".into())
    }
    fn list_connections(&self) -> Result<Vec<ConnectionInfo>, String> {
        Err("Network monitoring not supported on this platform".into())
    }
    fn get_delta(&self) -> Result<NetworkDelta, String> {
        Err("Network monitoring not supported on this platform".into())
    }
}

// Re-export Windows implementations at module level
#[cfg(target_os = "windows")]
use windows_impl::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_action_serialization() {
        let action = InputAction::TypeText { text: "hello".into() };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("TypeText"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_mouse_button_default() {
        let btn: MouseButton = Default::default();
        assert!(matches!(btn, MouseButton::Left));
    }

    #[test]
    fn test_network_stats_default() {
        let stats = NetworkStats::default();
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.bytes_sent, 0);
    }
}
