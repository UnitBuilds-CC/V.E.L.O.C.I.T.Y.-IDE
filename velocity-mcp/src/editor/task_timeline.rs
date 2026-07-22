//! Task Timeline - Zero-allocation ring buffer for task history with visual rendering.
//!
//! Provides a circular buffer of task events with immutable snapshot rendering.

use eframe::egui;
use std::path::Path;
use std::time::Instant;

/// Maximum number of task events in ring buffer
const TASK_BUFFER_SIZE: usize = 512;
/// Maximum bytes for task names/descriptions pool
const TASK_TEXT_POOL_SIZE: usize = 32768; // 32 KB

/// Task event types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TaskEventType {
    Started = 0,
    Completed = 1,
    Failed = 2,
    Cancelled = 3,
    ToolCall = 4,
    ToolResult = 5,
    PhaseChange = 6,
    TokenBudgetUpdate = 7,
    SessionMarker = 8,
    AgentMarker = 9,
}

/// Single task event entry (fixed 64 bytes)
#[derive(Clone, Copy, Debug)]
pub struct TaskEventEntry {
    pub event_type: TaskEventType,
    pub task_id: u32,
    pub parent_task_id: u32,
    pub name_offset: u16,
    pub name_len: u16,
    pub description_offset: u16,
    pub description_len: u16,
    pub timestamp_ms: u32,
    pub duration_ms: u32,
    pub metadata_u32_0: u32, // tokens, cost, etc.
    pub metadata_u32_1: u32,
    pub metadata_u32_2: u32,
}

impl Default for TaskEventEntry {
    fn default() -> Self {
        Self {
            event_type: TaskEventType::Started,
            task_id: 0,
            parent_task_id: 0,
            name_offset: 0,
            name_len: 0,
            description_offset: 0,
            description_len: 0,
            timestamp_ms: 0,
            duration_ms: 0,
            metadata_u32_0: 0,
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        }
    }
}

/// Zero-allocation task timeline with ring buffer
pub struct TaskTimelineState {
    events: [TaskEventEntry; TASK_BUFFER_SIZE],
    count: usize,
    head: usize,
    text_pool: [u8; TASK_TEXT_POOL_SIZE],
    text_used: usize,
    next_task_id: u32,
    start_time: Instant,
}

impl Default for TaskTimelineState {
    fn default() -> Self {
        Self {
            events: [TaskEventEntry::default(); TASK_BUFFER_SIZE],
            count: 0,
            head: 0,
            text_pool: [0u8; TASK_TEXT_POOL_SIZE],
            text_used: 0,
            next_task_id: 1,
            start_time: Instant::now(),
        }
    }
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn event_type_label(event_type: TaskEventType) -> &'static str {
    match event_type {
        TaskEventType::Started => "started",
        TaskEventType::Completed => "completed",
        TaskEventType::Failed => "failed",
        TaskEventType::Cancelled => "cancelled",
        TaskEventType::ToolCall => "tool_call",
        TaskEventType::ToolResult => "tool_result",
        TaskEventType::PhaseChange => "phase_change",
        TaskEventType::TokenBudgetUpdate => "token_budget_update",
        TaskEventType::SessionMarker => "session_marker",
        TaskEventType::AgentMarker => "agent_marker",
    }
}

pub fn serialize_mission_activity_nda(state: &TaskTimelineState) -> String {
    let mut lines = vec![
        "mission-activity version 2".to_string(),
        format!("entry_count {}", state.event_count()),
    ];
    for (index, (_, event)) in state.chronological_events().enumerate() {
        lines.push(format!(
            "entry\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            index,
            event_type_label(event.event_type),
            event.task_id,
            event.parent_task_id,
            encode_nda_text(state.get_text(event.name_offset, event.name_len)),
            encode_nda_text(state.get_text(event.description_offset, event.description_len)),
            event.timestamp_ms,
            event.duration_ms,
            event.metadata_u32_0,
            event.metadata_u32_1,
            event.metadata_u32_2,
        ));
    }
    lines.join("\n") + "\n"
}

pub fn persist_mission_activity_nda(workspace_root: &Path, state: &TaskTimelineState) {
    let agentic_dir = workspace_root.join(".velocity").join("agentic");
    let _ = std::fs::create_dir_all(&agentic_dir);
    let _ = std::fs::write(
        agentic_dir.join("mission_activity.nda"),
        serialize_mission_activity_nda(state),
    );
}

impl TaskTimelineState {
    /// Get elapsed milliseconds since start
    fn now_ms(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }

    /// Store string in text pool, return (offset, len) or (0, 0) if full
    fn store_text(&mut self, text: &str) -> (u16, u16) {
        let bytes = text.as_bytes();
        if self.text_used + bytes.len() >= TASK_TEXT_POOL_SIZE {
            return (0, 0); // pool exhausted, silently drop
        }
        let offset = self.text_used as u16;
        let len = bytes.len() as u16;
        self.text_pool[self.text_used..self.text_used + bytes.len()].copy_from_slice(bytes);
        self.text_used += bytes.len();
        (offset, len)
    }

    /// Retrieve text from pool (zero-copy)
    pub fn get_text(&self, offset: u16, len: u16) -> &str {
        if len == 0 {
            return "";
        }
        let start = offset as usize;
        let end = start + len as usize;
        std::str::from_utf8(&self.text_pool[start..end]).unwrap_or("")
    }

    /// Add task event to ring buffer
    pub fn add_event(
        &mut self,
        event_type: TaskEventType,
        name: &str,
        description: &str,
        parent_task_id: u32,
        duration_ms: u32,
        meta0: u32,
        meta1: u32,
        meta2: u32,
    ) -> u32 {
        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.wrapping_add(1);

        let (name_offset, name_len) = self.store_text(name);
        let (desc_offset, desc_len) = self.store_text(description);

        let idx = if self.count >= TASK_BUFFER_SIZE {
            self.head // overwrite oldest
        } else {
            (self.head + self.count) % TASK_BUFFER_SIZE
        };

        self.events[idx] = TaskEventEntry {
            event_type,
            task_id,
            parent_task_id,
            name_offset,
            name_len,
            description_offset: desc_offset,
            description_len: desc_len,
            timestamp_ms: self.now_ms(),
            duration_ms,
            metadata_u32_0: meta0,
            metadata_u32_1: meta1,
            metadata_u32_2: meta2,
        };

        if self.count >= TASK_BUFFER_SIZE {
            self.head = (self.head + 1) % TASK_BUFFER_SIZE;
        } else {
            self.count += 1;
        }

        task_id
    }

    /// Convenience: task started
    pub fn task_started(&mut self, name: &str, description: &str, parent_id: u32) -> u32 {
        self.add_event(
            TaskEventType::Started,
            name,
            description,
            parent_id,
            0,
            0,
            0,
            0,
        )
    }

    /// Convenience: task completed
    pub fn task_completed(&mut self, task_id: u32, duration_ms: u32, tokens_used: u32, cost: u32) {
        if task_id == 0 {
            return;
        }
        // Find and update the task
        for i in 0..self.count {
            let idx = (self.head + i) % TASK_BUFFER_SIZE;
            if self.events[idx].task_id == task_id {
                let mut updated = self.events[idx];
                updated.event_type = TaskEventType::Completed;
                updated.duration_ms = duration_ms;
                updated.metadata_u32_0 = tokens_used;
                updated.metadata_u32_1 = cost;
                self.events[idx] = updated;
                break;
            }
        }
    }

    /// Convenience: tool call within task
    pub fn tool_call(&mut self, task_id: u32, tool_name: &str, args_summary: &str) {
        if task_id == 0 {
            return;
        }
        self.add_event(
            TaskEventType::ToolCall,
            tool_name,
            args_summary,
            task_id,
            0,
            0,
            0,
            0,
        );
    }

    /// Convenience: tool result
    pub fn tool_result(&mut self, task_id: u32, tool_name: &str, success: bool, duration_ms: u32) {
        if task_id == 0 {
            return;
        }
        let event_type = if success {
            TaskEventType::ToolResult
        } else {
            TaskEventType::Failed
        };
        self.add_event(
            event_type,
            tool_name,
            if success { "completed" } else { "failed" },
            task_id,
            duration_ms,
            0,
            0,
            0,
        );
    }

    pub fn session_marker(&mut self, name: &str, description: &str) {
        self.add_event(
            TaskEventType::SessionMarker,
            name,
            description,
            0,
            0,
            0,
            0,
            0,
        );
    }

    pub fn agent_marker(&mut self, name: &str, description: &str, task_id: u32) {
        self.add_event(
            TaskEventType::AgentMarker,
            name,
            description,
            task_id,
            0,
            0,
            0,
            0,
        );
    }

    /// Get visible events in chronological order (newest first for UI)
    pub fn visible_events(&self) -> impl Iterator<Item = (usize, &TaskEventEntry)> {
        let count = self.count.min(TASK_BUFFER_SIZE);
        (0..count).rev().map(move |i| {
            let idx = (self.head + i) % TASK_BUFFER_SIZE;
            (idx, &self.events[idx])
        })
    }

    /// Get events in chronological order (oldest first)
    pub fn chronological_events(&self) -> impl Iterator<Item = (usize, &TaskEventEntry)> {
        let count = self.count.min(TASK_BUFFER_SIZE);
        (0..count).map(move |i| {
            let idx = if self.count >= TASK_BUFFER_SIZE {
                (self.head + i) % TASK_BUFFER_SIZE
            } else {
                i
            };
            (idx, &self.events[idx])
        })
    }

    pub fn event_count(&self) -> usize {
        self.count
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.head = 0;
        self.text_used = 0;
        self.next_task_id = 1;
        self.start_time = Instant::now();
    }
}

/// Immutable snapshot for rendering
pub struct TaskTimelineSnapshot<'a> {
    pub state: &'a TaskTimelineState,
}

impl<'a> TaskTimelineSnapshot<'a> {
    pub fn new(state: &'a TaskTimelineState) -> Self {
        Self { state }
    }
}

pub fn render_mission_activity_feed(
    ui: &mut egui::Ui,
    snapshot: &TaskTimelineSnapshot,
    selected_task_id: Option<u64>,
    max_items: usize,
) {
    ui.label(egui::RichText::new("Mission activity").strong());
    if snapshot.state.event_count() == 0 {
        ui.label(
            egui::RichText::new("No mission activity recorded yet")
                .small()
                .color(egui::Color32::from_rgb(125, 131, 166)),
        );
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("mission_activity_feed_scroll")
        .max_height(220.0)
        .show(ui, |ui: &mut egui::Ui| {
            for (_, event) in snapshot
                .state
                .visible_events()
                .filter(|(_, event)| {
                    selected_task_id
                        .map(|selected| {
                            event.task_id == 0
                                || event.task_id as u64 == selected
                                || event.parent_task_id as u64 == selected
                        })
                        .unwrap_or(true)
                })
                .take(max_items)
            {
                let (icon, color) = match event.event_type {
                    TaskEventType::Started => ("▶", egui::Color32::from_rgb(34, 211, 238)),
                    TaskEventType::Completed => ("✓", egui::Color32::from_rgb(34, 197, 94)),
                    TaskEventType::Failed => ("✕", egui::Color32::from_rgb(239, 68, 68)),
                    TaskEventType::Cancelled => ("⊘", egui::Color32::from_rgb(168, 85, 247)),
                    TaskEventType::ToolCall => ("⚙", egui::Color32::from_rgb(250, 204, 21)),
                    TaskEventType::ToolResult => ("✓", egui::Color32::from_rgb(74, 222, 128)),
                    TaskEventType::PhaseChange => ("◆", egui::Color32::from_rgb(168, 85, 247)),
                    TaskEventType::TokenBudgetUpdate => {
                        ("$", egui::Color32::from_rgb(236, 72, 153))
                    }
                    TaskEventType::SessionMarker => ("║", egui::Color32::from_rgb(59, 130, 246)),
                    TaskEventType::AgentMarker => ("◉", egui::Color32::from_rgb(236, 72, 153)),
                };
                let name = snapshot.state.get_text(event.name_offset, event.name_len);
                let desc = snapshot
                    .state
                    .get_text(event.description_offset, event.description_len);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(icon).color(color));
                    let task_scope = if event.task_id == 0 {
                        "Mission".to_string()
                    } else {
                        format!("Task #{}", event.task_id)
                    };
                    ui.label(
                        egui::RichText::new(format!("{} · {}", task_scope, name))
                            .small()
                            .strong(),
                    );
                    if !desc.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("— {}", desc))
                                .small()
                                .color(egui::Color32::from_rgb(125, 131, 166)),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.1}s",
                                event.timestamp_ms as f32 / 1000.0
                            ))
                            .size(9.0)
                            .color(egui::Color32::from_rgb(125, 131, 166)),
                        );
                    });
                });
                ui.separator();
            }
        });
}

/// Render task timeline panel
pub fn render_task_timeline(ui: &mut egui::Ui, snapshot: &TaskTimelineSnapshot) {
    let frame = egui::Frame::new()
        .fill(egui::Color32::from_rgb(25, 27, 39))
        .inner_margin(8.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(33, 36, 51)));

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("📋 Task Timeline")
                    .size(12.0)
                    .strong()
                    .color(egui::Color32::from_rgb(226, 227, 243)),
            );
            if snapshot.state.event_count() > 0 {
                ui.label(
                    egui::RichText::new(format!("({})", snapshot.state.event_count()))
                        .size(10.0)
                        .color(egui::Color32::from_rgb(125, 131, 166)),
                );
            }
        });

        if snapshot.state.event_count() == 0 {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("No tasks recorded yet")
                    .size(10.0)
                    .color(egui::Color32::from_rgb(125, 131, 166)),
            );
            return;
        }

        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("task_timeline_panel_scroll")
            .max_height(160.0)
            .show(ui, |ui: &mut egui::Ui| {
                // Use fixed array for task depths (max 256 tasks concurrently)
                let task_depths = [0u8; 256];
                let _active_task_count = 0;

                for (_, event) in snapshot.state.chronological_events() {
                    let task_id = event.task_id as usize;
                    let depth = if task_id < 256 {
                        task_depths[task_id]
                    } else {
                        0
                    };

                    let (icon, color) = match event.event_type {
                        TaskEventType::Started => ("▶", egui::Color32::from_rgb(34, 211, 238)),
                        TaskEventType::Completed => ("✓", egui::Color32::from_rgb(34, 197, 94)),
                        TaskEventType::Failed => ("✕", egui::Color32::from_rgb(239, 68, 68)),
                        TaskEventType::Cancelled => ("⊘", egui::Color32::from_rgb(168, 85, 247)),
                        TaskEventType::ToolCall => ("⚙", egui::Color32::from_rgb(250, 204, 21)),
                        TaskEventType::ToolResult => ("✓", egui::Color32::from_rgb(34, 197, 94)),
                        TaskEventType::PhaseChange => ("◆", egui::Color32::from_rgb(168, 85, 247)),
                        TaskEventType::TokenBudgetUpdate => {
                            ("$", egui::Color32::from_rgb(236, 72, 153))
                        }
                        TaskEventType::SessionMarker => {
                            ("║", egui::Color32::from_rgb(59, 130, 246))
                        }
                        TaskEventType::AgentMarker => ("◉", egui::Color32::from_rgb(236, 72, 153)),
                    };

                    let name = snapshot.state.get_text(event.name_offset, event.name_len);
                    let desc = snapshot
                        .state
                        .get_text(event.description_offset, event.description_len);

                    let is_marker = matches!(
                        event.event_type,
                        TaskEventType::SessionMarker | TaskEventType::AgentMarker
                    );
                    let indent = if is_marker { 0.0 } else { depth as f32 * 16.0 };
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        ui.label(
                            egui::RichText::new(icon)
                                .size(if is_marker { 13.0 } else { 11.0 })
                                .color(color),
                        );
                        ui.label(
                            egui::RichText::new(name)
                                .size(if is_marker { 11.0 } else { 10.0 })
                                .strong()
                                .color(if is_marker {
                                    color
                                } else {
                                    egui::Color32::from_rgb(226, 227, 243)
                                }),
                        );
                        if !desc.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("— {}", desc))
                                    .size(9.0)
                                    .color(egui::Color32::from_rgb(125, 131, 166)),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let time_str = format!("{:.1}s", event.timestamp_ms as f32 / 1000.0);
                            ui.label(
                                egui::RichText::new(time_str)
                                    .size(8.0)
                                    .color(egui::Color32::from_rgb(125, 131, 166)),
                            );
                            if event.duration_ms > 0 {
                                ui.label(
                                    egui::RichText::new(format!("({}ms)", event.duration_ms))
                                        .size(8.0)
                                        .color(egui::Color32::from_rgb(125, 131, 166)),
                                );
                            }
                        });
                    });
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_wrap() {
        let mut timeline = TaskTimelineState::default();

        // Fill beyond capacity
        for i in 0..TASK_BUFFER_SIZE + 10 {
            timeline.task_started(&format!("Task {}", i), "desc", 0);
        }

        // Should have exactly TASK_BUFFER_SIZE events
        assert_eq!(timeline.event_count(), TASK_BUFFER_SIZE);

        // First event should be evicted
        let first = timeline.chronological_events().next().unwrap().1;
        assert!(first.name_offset > 0); // Valid text stored
    }

    #[test]
    fn test_text_pool_storage() {
        let mut timeline = TaskTimelineState::default();
        let id = timeline.task_started("Test Task", "Description", 0);
        assert_ne!(id, 0);

        let events: Vec<_> = timeline.chronological_events().collect();
        assert_eq!(events.len(), 1);
        let event = events[0].1;
        assert_eq!(
            timeline.get_text(event.name_offset, event.name_len),
            "Test Task"
        );
        assert_eq!(
            timeline.get_text(event.description_offset, event.description_len),
            "Description"
        );
    }

    #[test]
    fn test_task_completion() {
        let mut timeline = TaskTimelineState::default();
        let id = timeline.task_started("Build", "compile project", 0);
        timeline.task_completed(id, 1500, 500, 100);

        let event = timeline.chronological_events().next().unwrap().1;
        assert_eq!(event.event_type, TaskEventType::Completed);
        assert_eq!(event.duration_ms, 1500);
        assert_eq!(event.metadata_u32_0, 500); // tokens
        assert_eq!(event.metadata_u32_1, 100); // cost
    }

    #[test]
    fn mission_activity_nda_serializes_structured_entries() {
        let mut timeline = TaskTimelineState::default();
        let task_id = timeline.task_started("Build\tUI", "compile\nproject", 0);
        timeline.agent_marker("Status", "ready", task_id);

        let nda = serialize_mission_activity_nda(&timeline);
        assert!(nda.starts_with("mission-activity version 2\n"));
        assert!(nda.contains("entry_count 2\n"));
        assert!(nda.contains("entry\t0\tstarted\t1\t0\tBuild\\tUI\tcompile\\nproject\t"));
        assert!(nda.contains("entry\t1\tagent_marker\t2\t1\tStatus\tready\t"));
    }

    #[test]
    fn persist_mission_activity_nda_writes_agentic_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let mut timeline = TaskTimelineState::default();
        timeline.session_marker("IDE session ready", "agentic workspace initialized");

        persist_mission_activity_nda(tmp.path(), &timeline);

        let artifact = std::fs::read_to_string(
            tmp.path()
                .join(".velocity")
                .join("agentic")
                .join("mission_activity.nda"),
        )
        .unwrap();
        assert!(artifact.starts_with("mission-activity version 2\n"));
        assert!(artifact.contains("entry_count 1\n"));
        assert!(artifact.contains(
            "entry\t0\tsession_marker\t1\t0\tIDE session ready\tagentic workspace initialized\t"
        ));
    }
}
