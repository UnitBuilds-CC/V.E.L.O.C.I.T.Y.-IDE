#![allow(dead_code, unused_imports, unused_variables)]
//! Advanced input capabilities for Windows desktop automation.
//!
//! Extends the basic click/type/focus actions with:
//! - Mouse wheel scrolling (vertical and horizontal)
//! - Right-click and middle-click with context menu handling
//! - Drag-and-drop operations
//! - Complex keyboard combinations (Ctrl+Shift+..., Alt+Tab, etc.)
//! - SendInput-based low-level input injection

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

// ─── Input Types ─────────────────────────────────────────────────────────────

/// Mouse button identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// Scroll direction and magnitude.
#[derive(Debug, Clone, Copy)]
pub struct ScrollInput {
    /// Vertical scroll: positive = up, negative = down (in "clicks", each = 120 units).
    pub vertical: i32,
    /// Horizontal scroll: positive = right, negative = left.
    pub horizontal: i32,
}

/// A point on screen in absolute coordinates.
#[derive(Debug, Clone, Copy)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

/// Drag-and-drop operation parameters.
#[derive(Debug, Clone)]
pub struct DragDropOp {
    pub from: ScreenPoint,
    pub to: ScreenPoint,
    pub button: MouseButton,
    /// Intermediate waypoints for complex drag paths.
    pub waypoints: Vec<ScreenPoint>,
    /// Duration of the drag motion (slower = more reliable).
    pub duration: Duration,
    /// Whether to hold a modifier key during drag (e.g., Ctrl for copy).
    pub modifier: Option<KeyModifier>,
}

/// Keyboard modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyModifier {
    Ctrl,
    Shift,
    Alt,
    Win,
    CtrlShift,
    CtrlAlt,
    AltShift,
    CtrlShiftAlt,
}

/// A complex keyboard combination (e.g., Ctrl+Shift+F5).
#[derive(Debug, Clone)]
pub struct KeyCombo {
    pub modifiers: Vec<KeyModifier>,
    /// Virtual key code or key name (e.g., "F5", "A", "Enter", "Tab").
    pub key: String,
}

/// Low-level input event for SendInput.
#[derive(Debug, Clone)]
pub enum InputEvent {
    MouseMove { x: i32, y: i32, absolute: bool },
    MouseDown { button: MouseButton },
    MouseUp { button: MouseButton },
    MouseClick { button: MouseButton, x: i32, y: i32 },
    MouseDoubleClick { button: MouseButton, x: i32, y: i32 },
    Scroll(ScrollInput),
    KeyDown { vk_code: u16 },
    KeyUp { vk_code: u16 },
    KeyPress { vk_code: u16 },
    TypeText { text: String },
    Combo(KeyCombo),
    Wait { ms: u64 },
}

/// A sequence of input events to be sent atomically.
#[derive(Debug, Clone)]
pub struct InputSequence {
    pub events: Vec<InputEvent>,
    /// Name/description for logging.
    pub label: String,
}

impl InputSequence {
    pub fn new(label: &str) -> Self {
        Self {
            events: Vec::new(),
            label: label.to_string(),
        }
    }

    pub fn push(&mut self, event: InputEvent) -> &mut Self {
        self.events.push(event);
        self
    }

    pub fn click_at(&mut self, x: i32, y: i32) -> &mut Self {
        self.events.push(InputEvent::MouseClick {
            button: MouseButton::Left,
            x,
            y,
        });
        self
    }

    pub fn right_click_at(&mut self, x: i32, y: i32) -> &mut Self {
        self.events.push(InputEvent::MouseClick {
            button: MouseButton::Right,
            x,
            y,
        });
        self
    }

    pub fn type_text(&mut self, text: &str) -> &mut Self {
        self.events.push(InputEvent::TypeText {
            text: text.to_string(),
        });
        self
    }

    pub fn key_combo(&mut self, modifiers: &[KeyModifier], key: &str) -> &mut Self {
        self.events.push(InputEvent::Combo(KeyCombo {
            modifiers: modifiers.to_vec(),
            key: key.to_string(),
        }));
        self
    }

    pub fn scroll(&mut self, vertical: i32, horizontal: i32) -> &mut Self {
        self.events.push(InputEvent::Scroll(ScrollInput {
            vertical,
            horizontal,
        }));
        self
    }

    pub fn wait_ms(&mut self, ms: u64) -> &mut Self {
        self.events.push(InputEvent::Wait { ms });
        self
    }

    pub fn drag_drop(&mut self, from: ScreenPoint, to: ScreenPoint) -> &mut Self {
        self.events.push(InputEvent::MouseMove {
            x: from.x,
            y: from.y,
            absolute: true,
        });
        self.events.push(InputEvent::MouseDown {
            button: MouseButton::Left,
        });
        self.events.push(InputEvent::Wait { ms: 50 });
        self.events.push(InputEvent::MouseMove {
            x: to.x,
            y: to.y,
            absolute: true,
        });
        self.events.push(InputEvent::Wait { ms: 50 });
        self.events.push(InputEvent::MouseUp {
            button: MouseButton::Left,
        });
        self
    }
}

// ─── Virtual Key Code Mapping ────────────────────────────────────────────────

/// Map a key name to its Windows Virtual Key code.
pub fn vk_code_from_name(name: &str) -> Option<u16> {
    Some(match name.to_ascii_uppercase().as_str() {
        "BACKSPACE" | "BACK" => 0x08,
        "TAB" => 0x09,
        "ENTER" | "RETURN" => 0x0D,
        "SHIFT" => 0x10,
        "CTRL" | "CONTROL" => 0x11,
        "ALT" | "MENU" => 0x12,
        "PAUSE" => 0x13,
        "CAPSLOCK" | "CAPS" => 0x14,
        "ESCAPE" | "ESC" => 0x1B,
        "SPACE" => 0x20,
        "PAGEUP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PGDN" => 0x22,
        "END" => 0x23,
        "HOME" => 0x24,
        "LEFT" => 0x25,
        "UP" => 0x26,
        "RIGHT" => 0x27,
        "DOWN" => 0x28,
        "PRINTSCREEN" | "PRTSC" => 0x2C,
        "INSERT" | "INS" => 0x2D,
        "DELETE" | "DEL" => 0x2E,
        "LWIN" | "WIN" => 0x5B,
        "RWIN" => 0x5C,
        "F1" => 0x70,
        "F2" => 0x71,
        "F3" => 0x72,
        "F4" => 0x73,
        "F5" => 0x74,
        "F6" => 0x75,
        "F7" => 0x76,
        "F8" => 0x77,
        "F9" => 0x78,
        "F10" => 0x79,
        "F11" => 0x7A,
        "F12" => 0x7B,
        "NUMLOCK" => 0x90,
        "SCROLLLOCK" => 0x91,
        s if s.len() == 1 => {
            let ch = s.chars().next().unwrap();
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase() as u16
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Map a modifier enum to its VK codes (for press/release).
pub fn modifier_vk_codes(modifier: KeyModifier) -> Vec<u16> {
    match modifier {
        KeyModifier::Ctrl => vec![0x11],
        KeyModifier::Shift => vec![0x10],
        KeyModifier::Alt => vec![0x12],
        KeyModifier::Win => vec![0x5B],
        KeyModifier::CtrlShift => vec![0x11, 0x10],
        KeyModifier::CtrlAlt => vec![0x11, 0x12],
        KeyModifier::AltShift => vec![0x12, 0x10],
        KeyModifier::CtrlShiftAlt => vec![0x11, 0x10, 0x12],
    }
}

// ─── PowerShell SendInput Script Builder ─────────────────────────────────────

/// Build a PowerShell script that uses SendInput to inject a sequence of events.
pub fn build_input_sequence_script(sequence: &InputSequence) -> String {
    let mut ps_commands = Vec::new();

    for event in &sequence.events {
        match event {
            InputEvent::MouseMove { x, y, absolute } => {
                if *absolute {
                    ps_commands.push(format!(
                        "[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {})",
                        x, y
                    ));
                }
            }
            InputEvent::MouseClick { button, x, y } => {
                let (down_flag, up_flag) = match button {
                    MouseButton::Left => ("0x0002", "0x0004"),
                    MouseButton::Right => ("0x0008", "0x0010"),
                    MouseButton::Middle => ("0x0020", "0x0040"),
                    _ => ("0x0002", "0x0004"),
                };
                ps_commands.push(format!(
                    "[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {})",
                    x, y
                ));
                ps_commands.push(format!(
                    "[InputSender]::mouse_event({}, 0, 0, 0, 0); [InputSender]::mouse_event({}, 0, 0, 0, 0)",
                    down_flag, up_flag
                ));
            }
            InputEvent::MouseDown { button } => {
                let flag = match button {
                    MouseButton::Left => "0x0002",
                    MouseButton::Right => "0x0008",
                    MouseButton::Middle => "0x0020",
                    _ => "0x0002",
                };
                ps_commands.push(format!("[InputSender]::mouse_event({}, 0, 0, 0, 0)", flag));
            }
            InputEvent::MouseUp { button } => {
                let flag = match button {
                    MouseButton::Left => "0x0004",
                    MouseButton::Right => "0x0010",
                    MouseButton::Middle => "0x0040",
                    _ => "0x0004",
                };
                ps_commands.push(format!("[InputSender]::mouse_event({}, 0, 0, 0, 0)", flag));
            }
            InputEvent::Scroll(scroll) => {
                if scroll.vertical != 0 {
                    ps_commands.push(format!(
                        "[InputSender]::mouse_event(0x0800, 0, 0, {}, 0)",
                        scroll.vertical * 120
                    ));
                }
                if scroll.horizontal != 0 {
                    ps_commands.push(format!(
                        "[InputSender]::mouse_event(0x01000, 0, 0, {}, 0)",
                        scroll.horizontal * 120
                    ));
                }
            }
            InputEvent::KeyDown { vk_code } => {
                ps_commands.push(format!(
                    "[InputSender]::keybd_event({}, 0, 0, 0)", vk_code
                ));
            }
            InputEvent::KeyUp { vk_code } => {
                ps_commands.push(format!(
                    "[InputSender]::keybd_event({}, 0, 2, 0)", vk_code
                ));
            }
            InputEvent::KeyPress { vk_code } => {
                ps_commands.push(format!(
                    "[InputSender]::keybd_event({}, 0, 0, 0); Start-Sleep -Milliseconds 30; [InputSender]::keybd_event({}, 0, 2, 0)",
                    vk_code, vk_code
                ));
            }
            InputEvent::TypeText { text } => {
                let escaped = text.replace("'", "''");
                ps_commands.push(format!(
                    "[System.Windows.Forms.SendKeys]::SendWait('{}')", escaped
                ));
            }
            InputEvent::Combo(combo) => {
                // Press modifiers
                for modifier in &combo.modifiers {
                    for vk in modifier_vk_codes(*modifier) {
                        ps_commands.push(format!(
                            "[InputSender]::keybd_event({}, 0, 0, 0)", vk
                        ));
                    }
                }
                // Press key
                if let Some(vk) = vk_code_from_name(&combo.key) {
                    ps_commands.push(format!(
                        "[InputSender]::keybd_event({}, 0, 0, 0); Start-Sleep -Milliseconds 30; [InputSender]::keybd_event({}, 0, 2, 0)",
                        vk, vk
                    ));
                }
                // Release modifiers (reverse order)
                for modifier in combo.modifiers.iter().rev() {
                    for vk in modifier_vk_codes(*modifier).iter().rev() {
                        ps_commands.push(format!(
                            "[InputSender]::keybd_event({}, 0, 2, 0)", vk
                        ));
                    }
                }
            }
            InputEvent::Wait { ms } => {
                ps_commands.push(format!("Start-Sleep -Milliseconds {}", ms));
            }
            InputEvent::MouseDoubleClick { button, x, y } => {
                let (down_flag, up_flag) = match button {
                    MouseButton::Left => ("0x0002", "0x0004"),
                    MouseButton::Right => ("0x0008", "0x0010"),
                    _ => ("0x0002", "0x0004"),
                };
                ps_commands.push(format!(
                    "[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {})",
                    x, y
                ));
                ps_commands.push(format!(
                    "[InputSender]::mouse_event({d}, 0, 0, 0, 0); [InputSender]::mouse_event({u}, 0, 0, 0, 0); Start-Sleep -Milliseconds 50; [InputSender]::mouse_event({d}, 0, 0, 0, 0); [InputSender]::mouse_event({u}, 0, 0, 0, 0)",
                    d = down_flag, u = up_flag
                ));
            }
        }
    }

    let commands_str = ps_commands.join("\n");
    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public class InputSender {{
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, int dwData, int dwExtraInfo);
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, int dwExtraInfo);
}}
'@
{commands_str}
Write-Output '{{"success":true,"events":{count}}}' -f @{{count={count}}}
"#,
        count = sequence.events.len()
    )
}

/// Result of executing an input sequence.
#[derive(Debug, Clone)]
pub struct InputExecutionResult {
    pub success: bool,
    pub events_sent: usize,
    pub detail: String,
}

/// Execute an input sequence by running the generated PowerShell script.
pub fn execute_sequence(sequence: &InputSequence) -> InputExecutionResult {
    // T3d: Prefer native SendInput on Windows (no PowerShell overhead)
    #[cfg(target_os = "windows")]
    {
        execute_sequence_native(sequence)
    }
    #[cfg(not(target_os = "windows"))]
    {
        return InputExecutionResult {
            success: false,
            events_sent: 0,
            detail: "Input execution requires Windows runtime".to_string(),
        };
    }
}

/// T3d: Native Win32 SendInput execution — zero PowerShell overhead.
/// Uses user32.dll SendInput for mouse and keyboard injection.
#[cfg(target_os = "windows")]
pub fn execute_sequence_native(sequence: &InputSequence) -> InputExecutionResult {
    let mut events_sent = 0usize;

    for event in &sequence.events {
        match event {
            InputEvent::MouseMove { x, y, absolute: _ } => {
                native_set_cursor_pos(*x, *y);
                events_sent += 1;
            }
            InputEvent::MouseClick { button, x, y } => {
                native_set_cursor_pos(*x, *y);
                let (down, up) = native_mouse_button_flags(button);
                native_send_mouse(down, 0);
                native_send_mouse(up, 0);
                events_sent += 1;
            }
            InputEvent::MouseDoubleClick { button, x, y } => {
                native_set_cursor_pos(*x, *y);
                let (down, up) = native_mouse_button_flags(button);
                native_send_mouse(down, 0);
                native_send_mouse(up, 0);
                std::thread::sleep(Duration::from_millis(50));
                native_send_mouse(down, 0);
                native_send_mouse(up, 0);
                events_sent += 1;
            }
            InputEvent::MouseDown { button } => {
                let (down, _) = native_mouse_button_flags(button);
                native_send_mouse(down, 0);
                events_sent += 1;
            }
            InputEvent::MouseUp { button } => {
                let (_, up) = native_mouse_button_flags(button);
                native_send_mouse(up, 0);
                events_sent += 1;
            }
            InputEvent::Scroll(scroll) => {
                if scroll.vertical != 0 {
                    native_send_mouse(0x0800, (scroll.vertical * 120) as u32); // MOUSEEVENTF_WHEEL
                }
                if scroll.horizontal != 0 {
                    native_send_mouse(0x1000, (scroll.horizontal * 120) as u32); // MOUSEEVENTF_HWHEEL
                }
                events_sent += 1;
            }
            InputEvent::KeyDown { vk_code } => {
                native_send_key(*vk_code, false);
                events_sent += 1;
            }
            InputEvent::KeyUp { vk_code } => {
                native_send_key(*vk_code, true);
                events_sent += 1;
            }
            InputEvent::KeyPress { vk_code } => {
                native_send_key(*vk_code, false);
                std::thread::sleep(Duration::from_millis(20));
                native_send_key(*vk_code, true);
                events_sent += 1;
            }
            InputEvent::TypeText { text } => {
                for ch in text.chars() {
                    let vk = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::VkKeyScanW(ch as u16) };
                    let vk_code = (vk & 0xFF) as u16;
                    let shift_needed = (vk >> 8) & 1 != 0;
                    if shift_needed {
                        native_send_key(0x10, false);
                    }
                    native_send_key(vk_code, false);
                    std::thread::sleep(Duration::from_millis(10));
                    native_send_key(vk_code, true);
                    if shift_needed {
                        native_send_key(0x10, true);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                events_sent += 1;
            }
            InputEvent::Combo(combo) => {
                for modifier in &combo.modifiers {
                    for vk in modifier_vk_codes(*modifier) {
                        native_send_key(vk, false);
                    }
                }
                if let Some(vk) = vk_code_from_name(&combo.key) {
                    native_send_key(vk, false);
                    std::thread::sleep(Duration::from_millis(20));
                    native_send_key(vk, true);
                }
                for modifier in combo.modifiers.iter().rev() {
                    for vk in modifier_vk_codes(*modifier).iter().rev() {
                        native_send_key(*vk, true);
                    }
                }
                events_sent += 1;
            }
            InputEvent::Wait { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                events_sent += 1;
            }
        }
    }

    InputExecutionResult {
        success: true,
        events_sent,
        detail: format!("Executed {} events via native SendInput", events_sent),
    }
}

// ─── Native Win32 FFI helpers (T3d) ─────────────────────────────────────────

#[cfg(target_os = "windows")]
fn native_mouse_button_flags(button: &MouseButton) -> (u32, u32) {
    match button {
        MouseButton::Left => (0x0002, 0x0004),     // LEFTDOWN, LEFTUP
        MouseButton::Right => (0x0008, 0x0010),    // RIGHTDOWN, RIGHTUP
        MouseButton::Middle => (0x0020, 0x0040),   // MIDDLEDOWN, MIDDLEUP
        _ => (0x0002, 0x0004),
    }
}

#[cfg(target_os = "windows")]
fn native_set_cursor_pos(x: i32, y: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

#[cfg(target_os = "windows")]
fn native_send_mouse(flags: u32, data: u32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    unsafe {
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi.dwFlags = MOUSE_EVENT_FLAGS(flags);
        input.Anonymous.mi.mouseData = data;
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(target_os = "windows")]
fn native_send_key(vk_code: u16, key_up: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    unsafe {
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        input.Anonymous.ki.wVk = VIRTUAL_KEY(vk_code);
        if key_up {
            input.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        }
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_code_lookup() {
        assert_eq!(vk_code_from_name("Enter"), Some(0x0D));
        assert_eq!(vk_code_from_name("F5"), Some(0x74));
        assert_eq!(vk_code_from_name("A"), Some(0x41));
        assert_eq!(vk_code_from_name("escape"), Some(0x1B));
        assert_eq!(vk_code_from_name("Tab"), Some(0x09));
        assert_eq!(vk_code_from_name("unknown_key"), None);
    }

    #[test]
    fn modifier_codes() {
        assert_eq!(modifier_vk_codes(KeyModifier::Ctrl), vec![0x11]);
        assert_eq!(modifier_vk_codes(KeyModifier::CtrlShift), vec![0x11, 0x10]);
        assert_eq!(modifier_vk_codes(KeyModifier::CtrlShiftAlt), vec![0x11, 0x10, 0x12]);
    }

    #[test]
    fn input_sequence_builder() {
        let mut seq = InputSequence::new("test");
        seq.click_at(100, 200)
            .wait_ms(50)
            .type_text("hello")
            .key_combo(&[KeyModifier::Ctrl], "S")
            .scroll(3, 0);
        assert_eq!(seq.events.len(), 5);
    }

    #[test]
    fn drag_drop_sequence() {
        let mut seq = InputSequence::new("drag");
        seq.drag_drop(ScreenPoint { x: 10, y: 10 }, ScreenPoint { x: 200, y: 200 });
        // Should produce: move, down, wait, move, wait, up = 6 events
        assert_eq!(seq.events.len(), 6);
    }

    #[test]
    fn script_contains_sendinput_types() {
        let mut seq = InputSequence::new("test");
        seq.click_at(50, 50).scroll(1, 0);
        let script = build_input_sequence_script(&seq);
        assert!(script.contains("mouse_event"));
        assert!(script.contains("InputSender"));
    }
}
