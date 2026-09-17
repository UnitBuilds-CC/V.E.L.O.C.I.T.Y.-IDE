//! Accessibility tree event subscription for Windows desktop automation.
//!
//! Provides event-driven UI change detection using Windows UIAutomation
//! event handlers instead of polling. Supports structure changes, property
//! changes, focus changes, and automation events via PowerShell wrappers.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

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

impl UiaEventKind {
    /// Parse the API-facing `eventKind` argument.
    pub fn from_api_str(raw: &str) -> Option<UiaEventKind> {
        Some(match raw {
            "window_opened" => UiaEventKind::WindowEvent { is_open: true },
            "window_closed" => UiaEventKind::WindowEvent { is_open: false },
            "element_focus" => UiaEventKind::FocusChanged,
            "structure_changed" => UiaEventKind::StructureChanged,
            "element_value_changed" => UiaEventKind::PropertyChanged {
                property_name: "Value".to_string(),
            },
            _ => return None,
        })
    }

    /// The label the polled listener emits for this kind, or `None` when the
    /// poller cannot observe it at all (property/menu/tooltip changes need real
    /// UIA event handlers on an STA message loop).
    pub fn poller_label(&self) -> Option<&'static str> {
        match self {
            UiaEventKind::FocusChanged => Some("focus_changed"),
            UiaEventKind::StructureChanged => Some("structure_changed"),
            UiaEventKind::WindowEvent { is_open: true } => Some("window_opened"),
            UiaEventKind::WindowEvent { is_open: false } => Some("window_closed"),
            _ => None,
        }
    }
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
            event_kinds: vec![UiaEventKind::FocusChanged, UiaEventKind::StructureChanged],
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
    pub fn listen(&mut self, subscription: &EventSubscription) -> EventListenResult {
        if !cfg!(target_os = "windows") {
            return EventListenResult {
                events: Vec::new(),
                listen_duration: Duration::ZERO,
                hit_event_limit: false,
                timed_out: true,
                errors: vec!["Event listener requires Windows runtime".to_string()],
            };
        }
        self.active = true;
        let start = Instant::now();
        let script = build_event_listener_script(subscription);
        // The script polls until its own deadline, so grant it that duration
        // plus slack; without a budget a stalled UIA COM call inside the poll
        // loop wedged the whole MCP server (bug #14).
        let result = crate::wa::ps::run_ps_script_budget(
            &script,
            subscription.duration + crate::wa::ps::SLACK,
        );
        self.active = false;
        let elapsed = start.elapsed();
        match result {
            Ok(json) => parse_event_listen_result(&json, elapsed),
            Err(e) => EventListenResult {
                events: Vec::new(),
                listen_duration: elapsed,
                hit_event_limit: false,
                timed_out: true,
                errors: vec![e],
            },
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

/// Render a Rust bool as a PowerShell literal.
fn ps_bool(value: bool) -> &'static str {
    if value {
        "$true"
    } else {
        "$false"
    }
}

/// Build a PowerShell script that subscribes to UIAutomation events.
///
/// The poller only observes what it can read without an STA message loop: the
/// focused element and the set of top-level windows. It therefore polls
/// exactly the kinds the subscription asked for, so the reported `eventKind`
/// always matches what was actually captured (bug #15).
///
/// Note `events = $events.ToArray()`: wrapping a `Generic.List` in `@()` throws
/// "Argument types do not match" on Windows PowerShell 5.1, which used to kill
/// the whole script on its final line (bug #17).
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
    let want_focus = subscription
        .event_kinds
        .iter()
        .any(|k| matches!(k, UiaEventKind::FocusChanged));
    let want_window = subscription
        .event_kinds
        .iter()
        .any(|k| matches!(k, UiaEventKind::WindowEvent { .. }));
    let want_structure = subscription
        .event_kinds
        .iter()
        .any(|k| matches!(k, UiaEventKind::StructureChanged));
    let want_tree_diff = want_structure || want_window;
    // Rendered as PowerShell literals so the script never depends on bare-word
    // coercion. A top-level-window delta is reported as a window event when the
    // caller asked for one, otherwise as a generic structure change.
    let want_focus = ps_bool(want_focus);
    let want_tree_diff = ps_bool(want_tree_diff);
    let open_label = if want_window {
        "window_opened"
    } else {
        "structure_changed"
    };
    let close_label = if want_window {
        "window_closed"
    } else {
        "structure_changed"
    };

    format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
{process_filter}
{window_filter}
$events = New-Object System.Collections.Generic.List[object]
$maxEvents = {max_events}
$__sw = [System.Diagnostics.Stopwatch]::StartNew()
$__deadlineMs = {duration_ms}
$root = [System.Windows.Automation.AutomationElement]::RootElement
$wantFocus = {want_focus}
$wantTreeDiff = {want_tree_diff}
$openLabel = '{open_label}'
$closeLabel = '{close_label}'
$trueCondition = [System.Windows.Automation.Condition]::TrueCondition
$childScope = [System.Windows.Automation.TreeScope]::Children

function Get-WaTopLevelIds {{
    param($scopeRoot)
    @(($scopeRoot.FindAll($childScope, $trueCondition)) | ForEach-Object {{
        if ($null -ne $windowFilter -and $_.Current.Name -notlike "*$windowFilter*") {{ return }}
        "$($_.Current.AutomationId)|$($_.Current.ProcessId)|$($_.Current.Name)"
    }})
}}

$baselineIds = @()
if ($wantTreeDiff) {{ $baselineIds = Get-WaTopLevelIds $root }}

# Focus change tracking via polling (event handlers require an STA thread)
$lastFocusId = $null
while ($__sw.Elapsed.TotalMilliseconds -lt $__deadlineMs -and $events.Count -lt $maxEvents) {{
    if ($wantFocus) {{
        try {{
            $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
            if ($null -ne $focused) {{
                $currentId = $focused.Current.AutomationId
                $currentName = $focused.Current.Name
                $currentPid = $focused.Current.ProcessId
                $windowOk = $true
                if ($null -ne $windowFilter) {{
                    $procTitle = ''
                    try {{ $procTitle = (Get-Process -Id $currentPid -ErrorAction SilentlyContinue).MainWindowTitle }} catch {{}}
                    $windowOk = ($currentName -like "*$windowFilter*") -or ($procTitle -like "*$windowFilter*")
                }}
                if (($null -eq $targetPid -or $currentPid -eq $targetPid) -and $windowOk) {{
                    if ($currentId -ne $lastFocusId) {{
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
            }}
        }} catch {{}}
    }}
    if ($wantTreeDiff) {{
        try {{
            $currentIds = Get-WaTopLevelIds $root
            foreach ($a in @($currentIds | Where-Object {{ $baselineIds -notcontains $_ }})) {{
                $parts = $a -split '\|', 3
                $events.Add([PSCustomObject]@{{
                    kind = $openLabel
                    timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
                    source_automation_id = $parts[0]
                    source_name = $parts[2]
                    source_control_type = 'Window'
                    process_id = [int]$parts[1]
                }}) | Out-Null
            }}
            foreach ($r in @($baselineIds | Where-Object {{ $currentIds -notcontains $_ }})) {{
                $parts = $r -split '\|', 3
                $events.Add([PSCustomObject]@{{
                    kind = $closeLabel
                    timestamp_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
                    source_automation_id = $parts[0]
                    source_name = $parts[2]
                    source_control_type = 'Window'
                    process_id = [int]$parts[1]
                }}) | Out-Null
            }}
            $baselineIds = $currentIds
        }} catch {{}}
    }}
    Start-Sleep -Milliseconds 50
}}

$result = @{{
    events = $events.ToArray()
    event_count = $events.Count
    timed_out = ($__sw.Elapsed.TotalMilliseconds -ge $__deadlineMs)
    hit_limit = ($events.Count -ge $maxEvents)
}}
ConvertTo-Json $result -Compress -Depth 4
"#
    )
}

/// Build a script that watches for structure changes (element add/remove).
pub fn build_structure_watch_script(process_id: Option<u32>, duration_ms: u64) -> String {
    let _pid_filter = process_id
        .map(|p| format!("-Filter \"ProcessId={}\"", p))
        .unwrap_or_default();
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$events = @()
$root = [System.Windows.Automation.AutomationElement]::RootElement
$baseline = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
$baselineIds = @($baseline | ForEach-Object {{ $_.Current.AutomationId + "|" + $_.Current.ProcessId }})
$__sw = [System.Diagnostics.Stopwatch]::StartNew()
$__deadlineMs = {duration_ms}
while ($__sw.Elapsed.TotalMilliseconds -lt $__deadlineMs) {{
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

fn parse_event_listen_result(json: &str, elapsed: Duration) -> EventListenResult {
    #[derive(serde::Deserialize)]
    struct PsEventResult {
        events: Option<Vec<PsEvent>>,
        event_count: Option<usize>,
        timed_out: Option<bool>,
        hit_limit: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct PsEvent {
        kind: Option<String>,
        timestamp_ms: Option<u64>,
        source_automation_id: Option<String>,
        source_name: Option<String>,
        source_control_type: Option<String>,
        process_id: Option<u32>,
    }
    match serde_json::from_str::<PsEventResult>(json) {
        Ok(r) => {
            let events: Vec<UiaEvent> = r
                .events
                .unwrap_or_default()
                .into_iter()
                .map(|e| {
                    let kind = match e.kind.as_deref() {
                        Some("focus_changed") => UiaEventKind::FocusChanged,
                        Some("structure_changed")
                        | Some("structure_added")
                        | Some("structure_removed") => UiaEventKind::StructureChanged,
                        Some("window_opened") => UiaEventKind::WindowEvent { is_open: true },
                        Some("window_closed") => UiaEventKind::WindowEvent { is_open: false },
                        // Keep unrecognised labels distinguishable instead of
                        // relabelling them as a focus change.
                        Some(other) => UiaEventKind::AutomationEvent {
                            event_name: other.to_string(),
                        },
                        None => UiaEventKind::AutomationEvent {
                            event_name: "unknown".to_string(),
                        },
                    };
                    UiaEvent {
                        kind,
                        timestamp_ms: e.timestamp_ms.unwrap_or(0),
                        source_automation_id: e.source_automation_id,
                        source_name: e.source_name,
                        source_control_type: e.source_control_type,
                        process_id: e.process_id,
                        old_value: None,
                        new_value: None,
                    }
                })
                .collect();
            // Cross-check the script-reported event count against what actually
            // parsed, so silent truncation/parse loss is surfaced, not swallowed.
            let mut errors: Vec<String> = Vec::new();
            if let Some(reported) = r.event_count {
                if reported != events.len() {
                    errors.push(format!(
                        "event count mismatch: script reported {reported}, parsed {}",
                        events.len()
                    ));
                }
            }
            EventListenResult {
                events,
                listen_duration: elapsed,
                hit_event_limit: r.hit_limit.unwrap_or(false),
                timed_out: r.timed_out.unwrap_or(true),
                errors,
            }
        }
        Err(e) => EventListenResult {
            events: Vec::new(),
            listen_duration: elapsed,
            hit_event_limit: false,
            timed_out: true,
            errors: vec![format!("parse error: {e}")],
        },
    }
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

    /// A focus-only subscription must not pay for (or emit) window diffs.
    #[test]
    fn focus_only_subscription_skips_tree_diff() {
        let sub = EventSubscription {
            event_kinds: vec![UiaEventKind::FocusChanged],
            ..Default::default()
        };
        let script = build_event_listener_script(&sub);
        assert!(script.contains("$wantFocus = $true"), "{script}");
        assert!(script.contains("$wantTreeDiff = $false"), "{script}");
    }

    #[test]
    fn window_subscription_diffs_top_level_windows() {
        let sub = EventSubscription {
            event_kinds: vec![UiaEventKind::WindowEvent { is_open: true }],
            ..Default::default()
        };
        let script = build_event_listener_script(&sub);
        assert!(script.contains("$wantFocus = $false"), "{script}");
        assert!(script.contains("$wantTreeDiff = $true"), "{script}");
        assert!(script.contains("$openLabel = 'window_opened'"), "{script}");
        assert!(script.contains("$closeLabel = 'window_closed'"), "{script}");
        assert!(script.contains("Get-WaTopLevelIds"), "{script}");
    }

    /// Structure-only subscriptions get the generic label, not a window claim.
    #[test]
    fn structure_subscription_labels_structure() {
        let sub = EventSubscription {
            event_kinds: vec![UiaEventKind::StructureChanged],
            ..Default::default()
        };
        let script = build_event_listener_script(&sub);
        assert!(script.contains("$wantFocus = $false"), "{script}");
        assert!(
            script.contains("$openLabel = 'structure_changed'"),
            "{script}"
        );
    }

    /// The script polls on a monotonic clock and can therefore never spin past
    /// its deadline on a platform where `TickCount64` is missing (bug #16).
    #[test]
    fn listener_script_uses_stopwatch_clock() {
        let script = build_event_listener_script(&EventSubscription::default());
        assert!(script.contains("[System.Diagnostics.Stopwatch]::StartNew()"));
        assert!(!script.contains("TickCount64"));
        assert!(script.contains("ConvertTo-Json $result -Compress -Depth 4"));
    }

    #[test]
    fn api_event_kinds_round_trip() {
        for raw in [
            "window_opened",
            "window_closed",
            "element_focus",
            "structure_changed",
        ] {
            let kind = UiaEventKind::from_api_str(raw).expect("supported kind");
            assert_eq!(kind.poller_label().unwrap().to_string(), {
                match raw {
                    "element_focus" => "focus_changed".to_string(),
                    other => other.to_string(),
                }
            });
        }
        assert!(UiaEventKind::from_api_str("nonsense").is_none());
        // Property changes need a real UIA event handler; the poller cannot see
        // them, so they must be rejected instead of silently captured as focus.
        assert!(UiaEventKind::from_api_str("element_value_changed")
            .unwrap()
            .poller_label()
            .is_none());
    }

    #[test]
    fn parse_maps_window_and_unknown_kinds() {
        let json = r#"{"events":[
            {"kind":"window_opened","timestamp_ms":5,"source_name":"Untitled - Notepad","process_id":7},
            {"kind":"window_closed","timestamp_ms":6,"source_name":"Settings","process_id":8},
            {"kind":"structure_added","timestamp_ms":7,"source_name":"Taskbar","process_id":9},
            {"kind":"toast_shown","timestamp_ms":8,"source_name":"X","process_id":10}
        ],"event_count":4,"timed_out":true,"hit_limit":false}"#;
        let result = parse_event_listen_result(json, Duration::from_millis(8));
        assert_eq!(result.events.len(), 4);
        assert_eq!(
            result.events[0].kind,
            UiaEventKind::WindowEvent { is_open: true }
        );
        assert_eq!(
            result.events[1].kind,
            UiaEventKind::WindowEvent { is_open: false }
        );
        assert_eq!(result.events[2].kind, UiaEventKind::StructureChanged);
        assert_eq!(
            result.events[3].kind,
            UiaEventKind::AutomationEvent {
                event_name: "toast_shown".to_string()
            }
        );
        assert!(result.errors.is_empty(), "got: {:?}", result.errors);
        assert!(result.timed_out);
    }

    /// Unparseable script output must surface as an error the caller can see,
    /// not as an empty-but-successful listen (bug #15).
    #[test]
    fn parse_failure_reports_error() {
        let result = parse_event_listen_result("not json", Duration::from_millis(1));
        assert!(result.events.is_empty());
        assert!(result.errors[0].starts_with("parse error:"));
    }

    /// End-to-end: the generated script must actually execute and print JSON.
    /// The substring assertions above cannot catch a PowerShell runtime error,
    /// and bugs #16, #17 and #19 were all exactly that — the script parsed, died
    /// or went missing at run time, and the tool answered “0 events”.
    #[test]
    #[cfg(target_os = "windows")]
    fn generated_listener_script_runs_end_to_end() {
        let sub = EventSubscription {
            event_kinds: vec![UiaEventKind::FocusChanged, UiaEventKind::StructureChanged],
            duration: Duration::from_millis(300),
            ..Default::default()
        };
        let script = build_event_listener_script(&sub);
        let out = crate::wa::ps::run_ps_script(&script)
            .expect("listener script must run without a PowerShell error");
        let parsed: serde_json::Value =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("not JSON ({e}): [{out}]"));
        assert!(parsed["events"].is_array(), "got: {parsed}");
        assert!(parsed["event_count"].is_number(), "got: {parsed}");
        assert!(parsed["timed_out"].is_boolean(), "got: {parsed}");
        assert!(parsed["hit_limit"].is_boolean(), "got: {parsed}");
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
