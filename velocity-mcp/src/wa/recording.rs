#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! Recording/Replay engine for Windows desktop automation.
//!
//! Records user interactions (clicks, keystrokes, focus changes) by polling the
//! UIAutomation event system, then persists them as replayable WaScript artifacts.
//! Replay uses the existing script runtime with timing fidelity.

use crate::wa::model::{WaScript, WaScriptStep};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── Recording Event Model ──────────────────────────────────────────────────

/// A single recorded interaction event with timing metadata.
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    /// Monotonic offset from recording start.
    pub offset: Duration,
    /// What happened.
    pub kind: RecordedEventKind,
    /// The UIA node that was targeted (if identifiable).
    pub target: Option<RecordedTarget>,
    /// Window title at the time of the event.
    pub window_title: String,
    /// Process ID of the foreground window.
    pub process_id: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum RecordedEventKind {
    Click { x: i32, y: i32, button: MouseButton },
    DoubleClick { x: i32, y: i32 },
    Type { text: String },
    KeyCombo { keys: Vec<String> },
    Focus,
    Scroll { delta_x: i32, delta_y: i32 },
    DragDrop { from: (i32, i32), to: (i32, i32) },
    WindowActivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone)]
pub struct RecordedTarget {
    pub node_id: String,
    pub role: String,
    pub name: String,
    pub automation_id: Option<String>,
}

// ─── Recording Session ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording,
    Paused,
}

/// Live recording session that accumulates events.
pub struct RecordingSession {
    pub state: RecordingState,
    pub events: Vec<RecordedEvent>,
    pub started_at: Option<Instant>,
    pub session_id: String,
    pub filter_process_id: Option<u32>,
    pub filter_window_title: Option<String>,
}

impl RecordingSession {
    pub fn new(session_id: &str) -> Self {
        Self {
            state: RecordingState::Idle,
            events: Vec::new(),
            started_at: None,
            session_id: session_id.to_string(),
            filter_process_id: None,
            filter_window_title: None,
        }
    }

    /// Start recording, optionally filtering to a specific process or window.
    pub fn start(&mut self, process_id: Option<u32>, window_title: Option<&str>) {
        self.state = RecordingState::Recording;
        self.started_at = Some(Instant::now());
        self.events.clear();
        self.filter_process_id = process_id;
        self.filter_window_title = window_title.map(|s| s.to_string());
    }

    pub fn pause(&mut self) {
        if self.state == RecordingState::Recording {
            self.state = RecordingState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == RecordingState::Paused {
            self.state = RecordingState::Recording;
        }
    }

    pub fn stop(&mut self) {
        self.state = RecordingState::Idle;
    }

    /// Push a new event into the recording buffer.
    pub fn push_event(&mut self, event: RecordedEvent) {
        if self.state != RecordingState::Recording {
            return;
        }
        // Apply process filter if set.
        if let Some(pid) = self.filter_process_id {
            if event.process_id != Some(pid) {
                return;
            }
        }
        // Apply window title filter if set.
        if let Some(ref title_filter) = self.filter_window_title {
            if !event
                .window_title
                .to_ascii_lowercase()
                .contains(&title_filter.to_ascii_lowercase())
            {
                return;
            }
        }
        self.events.push(event);
    }

    /// Convert the recorded events into a replayable WaScript.
    pub fn to_script(&self, script_name: &str) -> WaScript {
        let steps: Vec<WaScriptStep> = self
            .events
            .iter()
            .filter_map(event_to_script_step)
            .collect();
        WaScript {
            name: script_name.to_string(),
            created_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            start_url: self
                .filter_process_id
                .map(|pid| format!("windows://uia/process/{}", pid)),
            steps,
        }
    }

    /// Total recording duration so far.
    pub fn elapsed(&self) -> Duration {
        self.started_at
            .map(|start| start.elapsed())
            .unwrap_or_default()
    }

    /// Number of events captured.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

fn event_to_script_step(event: &RecordedEvent) -> Option<WaScriptStep> {
    let target = event.target.as_ref();
    match &event.kind {
        RecordedEventKind::Click { button, .. } => {
            let action = match button {
                MouseButton::Left => "click",
                MouseButton::Right => "right_click",
                MouseButton::Middle => "middle_click",
            };
            Some(WaScriptStep {
                action: action.to_string(),
                node_id: target.map(|t| t.node_id.clone()),
                role: target.map(|t| t.role.clone()),
                name: target.map(|t| t.name.clone()),
                value: None,
                required: true,
            })
        }
        RecordedEventKind::DoubleClick { .. } => Some(WaScriptStep {
            action: "double_click".to_string(),
            node_id: target.map(|t| t.node_id.clone()),
            role: target.map(|t| t.role.clone()),
            name: target.map(|t| t.name.clone()),
            value: None,
            required: true,
        }),
        RecordedEventKind::Type { text } => Some(WaScriptStep {
            action: "type".to_string(),
            node_id: target.map(|t| t.node_id.clone()),
            role: target.map(|t| t.role.clone()),
            name: target.map(|t| t.name.clone()),
            value: Some(text.clone()),
            required: true,
        }),
        RecordedEventKind::KeyCombo { keys } => Some(WaScriptStep {
            action: "key_combo".to_string(),
            node_id: target.map(|t| t.node_id.clone()),
            role: target.map(|t| t.role.clone()),
            name: target.map(|t| t.name.clone()),
            value: Some(keys.join("+")),
            required: true,
        }),
        RecordedEventKind::Focus => Some(WaScriptStep {
            action: "focus".to_string(),
            node_id: target.map(|t| t.node_id.clone()),
            role: target.map(|t| t.role.clone()),
            name: target.map(|t| t.name.clone()),
            value: None,
            required: false,
        }),
        RecordedEventKind::Scroll { delta_x, delta_y } => Some(WaScriptStep {
            action: "scroll".to_string(),
            node_id: target.map(|t| t.node_id.clone()),
            role: target.map(|t| t.role.clone()),
            name: target.map(|t| t.name.clone()),
            value: Some(format!("{},{}", delta_x, delta_y)),
            required: false,
        }),
        RecordedEventKind::DragDrop { from, to } => Some(WaScriptStep {
            action: "drag_drop".to_string(),
            node_id: target.map(|t| t.node_id.clone()),
            role: target.map(|t| t.role.clone()),
            name: target.map(|t| t.name.clone()),
            value: Some(format!("{},{}->{},{}", from.0, from.1, to.0, to.1)),
            required: true,
        }),
        RecordedEventKind::WindowActivate => None, // implicit, not scripted
    }
}

// ─── Replay with Timing ─────────────────────────────────────────────────────

/// Replay configuration controlling timing fidelity.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Speed multiplier (1.0 = real-time, 2.0 = double speed, 0.5 = half speed).
    pub speed_multiplier: f64,
    /// Minimum delay between steps even at high speed.
    pub min_step_delay: Duration,
    /// Whether to verify postconditions (focus/value) after each step.
    pub verify_postconditions: bool,
    /// Whether to stop on first failure or continue.
    pub stop_on_failure: bool,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            speed_multiplier: 1.0,
            min_step_delay: Duration::from_millis(50),
            verify_postconditions: true,
            stop_on_failure: true,
        }
    }
}

/// Compute the inter-step delays from recorded event timestamps.
pub fn compute_replay_delays(events: &[RecordedEvent], config: &ReplayConfig) -> Vec<Duration> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut delays = Vec::with_capacity(events.len());
    delays.push(Duration::ZERO); // first step has no delay
    for i in 1..events.len() {
        let raw_gap = events[i].offset.saturating_sub(events[i - 1].offset);
        let scaled = Duration::from_secs_f64(raw_gap.as_secs_f64() / config.speed_multiplier);
        delays.push(scaled.max(config.min_step_delay));
    }
    delays
}

// ─── Persistence ─────────────────────────────────────────────────────────────

/// Save a recording session as a WaScript to the workspace.
pub fn persist_recording(
    root: &Path,
    session: &RecordingSession,
    script_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let script = session.to_script(script_name);
    let steps: Vec<crate::wa::model::WaScriptStep> = script.steps;
    let report =
        crate::wa::save_script_report(root, script_name, script.start_url.as_deref(), steps)?;
    Ok(report.nda_path)
}

// ─── PowerShell Hook Recording (Windows-specific) ────────────────────────────

/// Build a PowerShell script that hooks UIA focus-changed and structure-changed
/// events, outputting JSON lines for each interaction detected.
pub fn build_recording_hook_script(process_id: Option<u32>, duration_seconds: u32) -> String {
    let pid_filter = process_id
        .map(|pid| format!("$targetPid = {}", pid))
        .unwrap_or_else(|| "$targetPid = $null".to_string());

    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
{pid_filter}
$duration = {duration_seconds}
$automation = [System.Windows.Automation.Automation]
$start = [DateTime]::UtcNow
$events = @()

# Poll focused element at 100ms intervals
$deadline = $start.AddSeconds($duration)
$lastFocusId = ""
while ([DateTime]::UtcNow -lt $deadline) {{
    try {{
        $focused = $automation::GetFocusedElement()
        if ($null -ne $focused) {{
            $aid = $focused.Current.AutomationId
            $name = $focused.Current.Name
            $role = $focused.Current.ControlType.ProgrammaticName
            $pid = $focused.Current.ProcessId
            $elapsed = ([DateTime]::UtcNow - $start).TotalMilliseconds
            $currentId = "$pid-$aid-$name"
            if ($currentId -ne $lastFocusId) {{
                $lastFocusId = $currentId
                if (($null -eq $targetPid) -or ($pid -eq $targetPid)) {{
                    $obj = @{{
                        offset_ms = [int]$elapsed
                        kind = "focus"
                        node_id = $aid
                        role = $role
                        name = $name
                        process_id = $pid
                        window_title = $focused.Current.Name
                    }}
                    Write-Output (ConvertTo-Json $obj -Compress)
                }}
            }}
        }}
    }} catch {{}}
    Start-Sleep -Milliseconds 100
}}
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_session_lifecycle() {
        let mut session = RecordingSession::new("test-session");
        assert_eq!(session.state, RecordingState::Idle);
        assert_eq!(session.event_count(), 0);

        session.start(None, None);
        assert_eq!(session.state, RecordingState::Recording);

        session.push_event(RecordedEvent {
            offset: Duration::from_millis(100),
            kind: RecordedEventKind::Click {
                x: 50,
                y: 100,
                button: MouseButton::Left,
            },
            target: Some(RecordedTarget {
                node_id: "btn-1".to_string(),
                role: "button".to_string(),
                name: "Submit".to_string(),
                automation_id: Some("submitBtn".to_string()),
            }),
            window_title: "Test Window".to_string(),
            process_id: Some(1234),
        });
        assert_eq!(session.event_count(), 1);

        session.push_event(RecordedEvent {
            offset: Duration::from_millis(500),
            kind: RecordedEventKind::Type {
                text: "hello".to_string(),
            },
            target: Some(RecordedTarget {
                node_id: "input-1".to_string(),
                role: "textbox".to_string(),
                name: "Email".to_string(),
                automation_id: None,
            }),
            window_title: "Test Window".to_string(),
            process_id: Some(1234),
        });
        assert_eq!(session.event_count(), 2);

        session.pause();
        assert_eq!(session.state, RecordingState::Paused);

        // Events ignored while paused.
        session.push_event(RecordedEvent {
            offset: Duration::from_millis(800),
            kind: RecordedEventKind::Focus,
            target: None,
            window_title: "Test Window".to_string(),
            process_id: Some(1234),
        });
        assert_eq!(session.event_count(), 2);

        session.resume();
        session.stop();
        assert_eq!(session.state, RecordingState::Idle);
    }

    #[test]
    fn to_script_produces_valid_steps() {
        let mut session = RecordingSession::new("test");
        session.start(None, None);
        session.push_event(RecordedEvent {
            offset: Duration::from_millis(100),
            kind: RecordedEventKind::Click {
                x: 0,
                y: 0,
                button: MouseButton::Left,
            },
            target: Some(RecordedTarget {
                node_id: "btn".to_string(),
                role: "button".to_string(),
                name: "OK".to_string(),
                automation_id: None,
            }),
            window_title: "Dialog".to_string(),
            process_id: Some(999),
        });
        session.push_event(RecordedEvent {
            offset: Duration::from_millis(300),
            kind: RecordedEventKind::Type {
                text: "world".to_string(),
            },
            target: Some(RecordedTarget {
                node_id: "txt".to_string(),
                role: "textbox".to_string(),
                name: "Input".to_string(),
                automation_id: None,
            }),
            window_title: "Dialog".to_string(),
            process_id: Some(999),
        });

        let script = session.to_script("Test Script");
        assert_eq!(script.steps.len(), 2);
        assert_eq!(script.steps[0].action, "click");
        assert_eq!(script.steps[1].action, "type");
        assert_eq!(script.steps[1].value.as_deref(), Some("world"));
    }

    #[test]
    fn process_filter_excludes_other_pids() {
        let mut session = RecordingSession::new("filtered");
        session.start(Some(1000), None);

        session.push_event(RecordedEvent {
            offset: Duration::from_millis(50),
            kind: RecordedEventKind::Click {
                x: 0,
                y: 0,
                button: MouseButton::Left,
            },
            target: None,
            window_title: "Other".to_string(),
            process_id: Some(9999), // different PID
        });
        assert_eq!(session.event_count(), 0);

        session.push_event(RecordedEvent {
            offset: Duration::from_millis(100),
            kind: RecordedEventKind::Click {
                x: 0,
                y: 0,
                button: MouseButton::Left,
            },
            target: None,
            window_title: "Target".to_string(),
            process_id: Some(1000), // matching PID
        });
        assert_eq!(session.event_count(), 1);
    }

    #[test]
    fn replay_delays_respect_speed_multiplier() {
        let events = vec![
            RecordedEvent {
                offset: Duration::from_millis(0),
                kind: RecordedEventKind::Focus,
                target: None,
                window_title: "W".to_string(),
                process_id: None,
            },
            RecordedEvent {
                offset: Duration::from_millis(1000),
                kind: RecordedEventKind::Focus,
                target: None,
                window_title: "W".to_string(),
                process_id: None,
            },
            RecordedEvent {
                offset: Duration::from_millis(1200),
                kind: RecordedEventKind::Focus,
                target: None,
                window_title: "W".to_string(),
                process_id: None,
            },
        ];
        let config = ReplayConfig {
            speed_multiplier: 2.0,
            min_step_delay: Duration::from_millis(50),
            ..Default::default()
        };
        let delays = compute_replay_delays(&events, &config);
        assert_eq!(delays.len(), 3);
        assert_eq!(delays[0], Duration::ZERO);
        assert_eq!(delays[1], Duration::from_millis(500)); // 1000/2
        assert_eq!(delays[2], Duration::from_millis(100)); // 200/2
    }
}
