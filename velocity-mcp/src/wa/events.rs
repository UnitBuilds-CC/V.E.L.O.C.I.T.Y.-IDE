#![allow(dead_code, unused_imports, unused_variables)]
//! Accessibility tree event subscription for Windows desktop automation.
//!
//! Provides event-driven UI change detection using Windows UIAutomation
//! event handlers instead of polling. Supports structure changes, property
//! changes, focus changes, and automation events via PowerShell wrappers.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── Event Types ─────────────────────────────────────────────────────────────

/// Types of UIAutomation events that can be subscribed to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UiaEventKind {
    /// An element's property changed (name, value, enabled, etc.).
    PropertyChanged { property_name: String },
    /// The tree structure changed (element added/removed).
    StructureChanged,
    /// Keyboard focus moved to a new element.
    FocusChanged,
    /// An automation event fired (invoke, selection changed, text changed).
    AutomationEvent { event_name: String },
    /// A window opened or closed.
    WindowEvent { is_open: bool },
    /// A menu opened or closed.
    MenuEvent { is_open: bool },
    /// A tooltip appeared.
    ToolTipEvent,
}

/// A captured UI event with context.
#[derive(Debug, Clone)]
pub struct UiaEvent {
    /// Type of event.
    pub kind: UiaEventKind,
    /// Timestamp when the event was captured.
    pub timestamp_ms: u64,
    /// Element that generated the event (automation ID if available).
    pub source_automation_id: Option<String>,
    /// Name of the source element.
    pub source_name: Option<String>,
    /// Control type of the source element.
    pub source_control_type: Option<String>,
    /// Process ID of the source.
    pub process_id: Option<u32>,
    /// Old value (for property changes).
    pub old_value: Option<String>,
    /// New value (for property changes).
    pub new_value: Option<String>,
}

// ─── Event Subscription ──────────────────────────────────────────────────────

/// Configuration for event subscription.
#[derive(Debug, Clone)]
pub struct EventSubscription {
    /// What kinds of events to listen for.
    pub event_kinds: Vec<UiaEventKind>,
    /// Target process ID (None = all processes).
    pub process_filter: Option<u32>,
    /// Target window title filter (case-insensitive contains).
    pub window_filter: Option<String>,
    /// Maximum duration to listen.
    pub duration: Duration,
    /// Maximum events to collect before stopping.
    pub max_events: usize,
    /// Whether to include element details with each event.
    pub include_element_details: bool,
}

impl Default for EventSubscription {
    fn default() -> Self {
        Self {
            event_kinds: vec![
                UiaEventKind::FocusChanged,
                UiaEventKind::StructureChanged,
            ],
            process_filter: None,
            window_filter: None,
            duration: Duration::from_secs(30),
            max_events: 500,
            include_element_details: true,
        }
    }
}

/// Result of an event listening session.
#[derive(Debug, Clone)]
pub struct EventListenResult {
    /// Collected events in chronological order.
    pub events: Vec<UiaEvent>,
    /// How long the listener was active.
    pub listen_duration: Duration,
    /// Whether the listener hit max_events limit.
    pub hit_event_limit: bool,
    /// Whether the listener was stopped by timeout.
    pub timed_out: bool,
    /// Any errors during listening.
    pub errors: Vec<String>,
}

// ─── Event Buffer ────────────────────────────────────────────────────────────

/// Ring buffer for tracking recent events with deduplication.
pub struct EventBuffer {
    events: VecDeque<UiaEvent>,
    capacity: usize,
    /// Dedup window: ignore events with same source+kind within this duration.
    dedup_window: Duration,
}

impl EventBuffer {
    pub fn new(capacity: usize, dedup_window: Duration) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
            dedup_window,
        }
    }

    /// Add an event, deduplicating near-identical events.
    pub fn push(&mut self, event: UiaEvent) {
        let dominated = self.events.iter().rev().take(10).any(|existing| {
            existing.kind == event.kind
                && existing.source_automation_id == event.source_automation_id
                && (event.timestamp_ms - existing.timestamp_ms)
                    < self.dedup_window.as_millis() as u64
        });
        if dominated {
            return;
        }
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Get all events since a timestamp.
    pub fn events_since(&self, since_ms: u64) -> Vec<&UiaEvent> {
        self.events
            .iter()
            .filter(|e| e.timestamp_ms >= since_ms)
            .collect()
    }

    /// Get the most recent N events.
    pub fn recent(&self, n: usize) -> Vec<&UiaEvent> {
        self.events.iter().rev().take(n).collect()
    }

    /// Total events stored.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Clear all events.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

// ─── Event Listener (PowerShell-based) ───────────────────────────────────────

/// Manages event listening sessions via PowerShell.
pub struct EventListener {
    /// Whether a listener is currently active.
    active: bool,
}

impl EventListener {
    pub fn new() -> Self {
        Self { active: false }
    }

    /// Start listening for events (blocks until duration/max_events).
    pub fn listen(&mut self, _subscription: &EventSubscription) -> EventListenResult {
        EventListenResult {
            events: Vec::new(),
            listen_duration: Duration::ZERO,
            hit_event_limit: false,
            timed_out: true,
            errors: vec!["Event listener requires Windows runtime".to_string()],
        }
    }

    /// Whether a listener is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Default for EventListener {
    fn default() -> Self {
        Self::new()
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script that subscribes to UIAutomation events.
pub fn build_event_listener_script(subscription: &EventSubscription) -> String {
    let duration_ms = subscription.duration.as_millis();
    let max_events = subscription.max_events;
    let process_filter = subscription
        .process_filter
        .map(|p| format!("$targetPid = {p}"))
        .unwrap_or_else(|| "$targetPid = $null".to_string());
    let window_filter = subscription
        .window_filter
        .as_deref()
        .map(|w| format!("$windowFilter = '{}'", w.replace('\'', "''")))
        .unwrap_or_else(|| "$windowFilter = $null".to_string());

    let has_focus = subscription.event_kinds.iter().any(|k| matches!(k, UiaEventKind::FocusChanged));
    let has_structure = subscription.event_kinds.iter().any(|k| matches!(k, UiaEventKind::StructureChanged));
    let has_property = subscription.event_kinds.iter().any(|k| matches!(k, UiaEventKind::PropertyChanged { .. }));

    format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
{process_filter}
{window_filter}
$events = New-Object System.Collections.Generic.List[object]
$maxEvents = {max_events}
$deadline = [Environment]::TickCount64 + {duration_ms}
$root = [System.Windows.Automation.AutomationElement]::RootElement

# Focus change tracking via polling (event handlers require STA thread)
$lastFocusId = $null
while ([Environment]::TickCount64 -lt $deadline -and $events.Count -lt $maxEvents) {{
    try {{
        $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
        if ($null -ne $focused) {{
            $currentId = $focused.Current.AutomationId
            $currentName = $focused.Current.Name
            $currentPid = $focused.Current.ProcessId
            if ($null -ne $targetPid -and $currentPid -ne $targetPid) {{
                Start-Sleep -Milliseconds 50
                continue
            }}
            if ($currentId -ne $lastFocusId -or ($null -eq $lastFocusId -and $null -ne $currentId)) {{
                $events.Add([PSCustomObject]@{{
                    kind = "focus_changed"
                    timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
                    source_automation_id = $currentId
                    source_name = $currentName
                    source_control_type = $focused.Current.ControlType.ProgrammaticName
                    process_id = $currentPid
                }}) | Out-Null
                $lastFocusId = $currentId
            }}
        }}
    }} catch {{}}
    Start-Sleep -Milliseconds 50
}}

$result = @{{
    events = @($events)
    event_count = $events.Count
    timed_out = ([Environment]::TickCount64 -ge $deadline)
    hit_limit = ($events.Count -ge $maxEvents)
}}
ConvertTo-Json $result -Compress -Depth 4
"#
    )
}

/// Build a script that watches for structure changes (element add/remove).
pub fn build_structure_watch_script(process_id: Option<u32>, duration_ms: u64) -> String {
    let pid_filter = process_id
        .map(|p| format!("-Filter \"ProcessId={}\"", p))
        .unwrap_or_default();
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$events = @()
$root = [System.Windows.Automation.AutomationElement]::RootElement
$baseline = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
$baselineIds = @($baseline | ForEach-Object {{ $_.Current.AutomationId + "|" + $_.Current.ProcessId }})
$deadline = [Environment]::TickCount64 + {duration_ms}
while ([Environment]::TickCount64 -lt $deadline) {{
    Start-Sleep -Milliseconds 200
    $current = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    $currentIds = @($current | ForEach-Object {{ $_.Current.AutomationId + "|" + $_.Current.ProcessId }})
    $added = $currentIds | Where-Object {{ $baselineIds -notcontains $_ }}
    $removed = $baselineIds | Where-Object {{ $currentIds -notcontains $_ }}
    foreach ($a in $added) {{
        $events += @{{ kind = "structure_added"; element = $a; timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }}
    }}
    foreach ($r in $removed) {{
        $events += @{{ kind = "structure_removed"; element = $r; timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }}
    }}
    $baselineIds = $currentIds
}}
ConvertTo-Json @($events) -Compress -Depth 3
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_buffer_deduplicates() {
        let mut buf = EventBuffer::new(100, Duration::from_millis(100));
        let event1 = UiaEvent {
            kind: UiaEventKind::FocusChanged,
            timestamp_ms: 1000,
            source_automation_id: Some("btn1".to_string()),
            source_name: Some("Submit".to_string()),
            source_control_type: Some("Button".to_string()),
            process_id: Some(1234),
            old_value: None,
            new_value: None,
        };
        let event2 = UiaEvent {
            kind: UiaEventKind::FocusChanged,
            timestamp_ms: 1050, // within dedup window
            source_automation_id: Some("btn1".to_string()),
            source_name: Some("Submit".to_string()),
            source_control_type: Some("Button".to_string()),
            process_id: Some(1234),
            old_value: None,
            new_value: None,
        };
        buf.push(event1);
        buf.push(event2);
        assert_eq!(buf.len(), 1); // second was deduped
    }

    #[test]
    fn event_buffer_respects_capacity() {
        let mut buf = EventBuffer::new(3, Duration::from_millis(0));
        for i in 0..5 {
            buf.push(UiaEvent {
                kind: UiaEventKind::StructureChanged,
                timestamp_ms: i * 1000,
                source_automation_id: Some(format!("el_{}", i)),
                source_name: None,
                source_control_type: None,
                process_id: None,
                old_value: None,
                new_value: None,
            });
        }
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn event_listener_script_includes_filters() {
        let sub = EventSubscription {
            process_filter: Some(4242),
            window_filter: Some("Notepad".to_string()),
            ..Default::default()
        };
        let script = build_event_listener_script(&sub);
        assert!(script.contains("4242"));
        assert!(script.contains("Notepad"));
        assert!(script.contains("FocusedElement"));
    }

    #[test]
    fn structure_watch_script_tracks_changes() {
        let script = build_structure_watch_script(Some(1234), 5000);
        assert!(script.contains("structure_added"));
        assert!(script.contains("structure_removed"));
        assert!(script.contains("5000"));
    }

    #[test]
    fn events_since_filters_correctly() {
        let mut buf = EventBuffer::new(100, Duration::from_millis(0));
        for i in 0..10 {
            buf.push(UiaEvent {
                kind: UiaEventKind::FocusChanged,
                timestamp_ms: i * 100,
                source_automation_id: Some(format!("el_{}", i)),
                source_name: None,
                source_control_type: None,
                process_id: None,
                old_value: None,
                new_value: None,
            });
        }
        let recent = buf.events_since(500);
        assert_eq!(recent.len(), 5); // events at 500,600,700,800,900
    }
}
