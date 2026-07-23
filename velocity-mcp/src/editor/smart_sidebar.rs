#![allow(dead_code)]

//! Smart Sidebar - Zero-allocation context-aware sidebar with suggestions and quick actions.
//!
//! Provides a ring-buffer based sidebar that tracks context, file references,
//! and presents actionable suggestions to the user.

use crate::editor::theme::IdePalette;
use eframe::egui;
use std::time::Instant;

/// Maximum sidebar entries
const SIDEBAR_BUFFER_SIZE: usize = 256;
/// Text pool for sidebar entries
const SIDEBAR_TEXT_POOL_SIZE: usize = 16384; // 16 KB

/// Sidebar entry types
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SidebarEntryType {
    FileReference = 0,
    SymbolDefinition = 1,
    CodeSuggestion = 2,
    QuickAction = 3,
    ErrorDiagnostic = 4,
    WarningDiagnostic = 5,
    TodoComment = 6,
    Note = 7,
}

/// Sidebar entry (fixed 48 bytes)
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct SidebarEntry {
    pub entry_type: SidebarEntryType,
    pub priority: u8, // 0-255, higher = more important
    pub source_task_id: u32,
    pub file_offset: u16,
    pub file_len: u16,
    pub line_number: u32,
    pub column_number: u16,
    pub text_offset: u16,
    pub text_len: u16,
    pub timestamp_ms: u32,
    pub metadata_u32_0: u32, // context-dependent
    pub metadata_u32_1: u32,
    pub metadata_u32_2: u32,
}

impl Default for SidebarEntry {
    fn default() -> Self {
        Self {
            entry_type: SidebarEntryType::Note,
            priority: 0,
            source_task_id: 0,
            file_offset: 0,
            file_len: 0,
            line_number: 0,
            column_number: 0,
            text_offset: 0,
            text_len: 0,
            timestamp_ms: 0,
            metadata_u32_0: 0,
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        }
    }
}

/// Zero-allocation smart sidebar state
pub struct SmartSidebarState {
    entries: [SidebarEntry; SIDEBAR_BUFFER_SIZE],
    count: usize,
    head: usize,
    text_pool: [u8; SIDEBAR_TEXT_POOL_SIZE],
    text_used: usize,
    start_time: Instant,
    /// Filter: only show entries of these types
    filter_types: [bool; 8],
    /// Sort by priority (true) or time (false)
    sort_by_priority: bool,
}

impl Default for SmartSidebarState {
    fn default() -> Self {
        Self {
            entries: [SidebarEntry::default(); SIDEBAR_BUFFER_SIZE],
            count: 0,
            head: 0,
            text_pool: [0u8; SIDEBAR_TEXT_POOL_SIZE],
            text_used: 0,
            start_time: Instant::now(),
            filter_types: [true; 8], // all enabled by default
            sort_by_priority: true,
        }
    }
}

impl SmartSidebarState {
    fn now_ms(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }

    fn store_text(&mut self, text: &str) -> (u16, u16) {
        let bytes = text.as_bytes();
        if self.text_used + bytes.len() >= SIDEBAR_TEXT_POOL_SIZE {
            return (0, 0);
        }
        let offset = self.text_used as u16;
        let len = bytes.len() as u16;
        self.text_pool[self.text_used..self.text_used + bytes.len()].copy_from_slice(bytes);
        self.text_used += bytes.len();
        (offset, len)
    }

    pub fn get_text(&self, offset: u16, len: u16) -> &str {
        if len == 0 {
            return "";
        }
        let start = offset as usize;
        let end = start + len as usize;
        std::str::from_utf8(&self.text_pool[start..end]).unwrap_or("")
    }

    pub fn get_file(&self, offset: u16, len: u16) -> &str {
        self.get_text(offset, len)
    }

    /// Add a file reference
    pub fn add_file_reference(
        &mut self,
        task_id: u32,
        file: &str,
        line: u32,
        column: u16,
        description: &str,
    ) {
        let (file_offset, file_len) = self.store_text(file);
        let (text_offset, text_len) = self.store_text(description);

        let idx = if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head
        } else {
            (self.head + self.count) % SIDEBAR_BUFFER_SIZE
        };

        self.entries[idx] = SidebarEntry {
            entry_type: SidebarEntryType::FileReference,
            priority: 100,
            source_task_id: task_id,
            file_offset,
            file_len,
            line_number: line,
            column_number: column,
            text_offset,
            text_len,
            timestamp_ms: self.now_ms(),
            metadata_u32_0: 0,
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        };

        if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head = (self.head + 1) % SIDEBAR_BUFFER_SIZE;
        } else {
            self.count += 1;
        }
    }

    /// Add a symbol definition
    pub fn add_symbol(&mut self, task_id: u32, symbol: &str, file: &str, line: u32, column: u16) {
        let (file_offset, file_len) = self.store_text(file);
        let (text_offset, text_len) = self.store_text(symbol);

        let idx = if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head
        } else {
            (self.head + self.count) % SIDEBAR_BUFFER_SIZE
        };

        self.entries[idx] = SidebarEntry {
            entry_type: SidebarEntryType::SymbolDefinition,
            priority: 120,
            source_task_id: task_id,
            file_offset,
            file_len,
            line_number: line,
            column_number: column,
            text_offset,
            text_len,
            timestamp_ms: self.now_ms(),
            metadata_u32_0: 0,
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        };

        if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head = (self.head + 1) % SIDEBAR_BUFFER_SIZE;
        } else {
            self.count += 1;
        }
    }

    /// Add a code suggestion
    pub fn add_suggestion(&mut self, task_id: u32, suggestion: &str, file: &str, line: u32) {
        let (file_offset, file_len) = self.store_text(file);
        let (text_offset, text_len) = self.store_text(suggestion);

        let idx = if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head
        } else {
            (self.head + self.count) % SIDEBAR_BUFFER_SIZE
        };

        self.entries[idx] = SidebarEntry {
            entry_type: SidebarEntryType::CodeSuggestion,
            priority: 150,
            source_task_id: task_id,
            file_offset,
            file_len,
            line_number: line,
            column_number: 0,
            text_offset,
            text_len,
            timestamp_ms: self.now_ms(),
            metadata_u32_0: 0,
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        };

        if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head = (self.head + 1) % SIDEBAR_BUFFER_SIZE;
        } else {
            self.count += 1;
        }
    }

    /// Add a quick action
    pub fn add_quick_action(
        &mut self,
        task_id: u32,
        action: &str,
        _description: &str,
        action_id: u32,
    ) {
        let (file_offset, file_len) = self.store_text("");
        let (text_offset, text_len) = self.store_text(action);

        let idx = if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head
        } else {
            (self.head + self.count) % SIDEBAR_BUFFER_SIZE
        };

        self.entries[idx] = SidebarEntry {
            entry_type: SidebarEntryType::QuickAction,
            priority: 200,
            source_task_id: task_id,
            file_offset,
            file_len,
            line_number: 0,
            column_number: 0,
            text_offset,
            text_len,
            timestamp_ms: self.now_ms(),
            metadata_u32_0: action_id, // action identifier
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        };

        if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head = (self.head + 1) % SIDEBAR_BUFFER_SIZE;
        } else {
            self.count += 1;
        }
    }

    /// Add diagnostic (error/warning)
    pub fn add_diagnostic(
        &mut self,
        task_id: u32,
        is_error: bool,
        file: &str,
        line: u32,
        column: u16,
        message: &str,
    ) {
        let (file_offset, file_len) = self.store_text(file);
        let (text_offset, text_len) = self.store_text(message);

        let idx = if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head
        } else {
            (self.head + self.count) % SIDEBAR_BUFFER_SIZE
        };

        self.entries[idx] = SidebarEntry {
            entry_type: if is_error {
                SidebarEntryType::ErrorDiagnostic
            } else {
                SidebarEntryType::WarningDiagnostic
            },
            priority: if is_error { 255 } else { 180 },
            source_task_id: task_id,
            file_offset,
            file_len,
            line_number: line,
            column_number: column,
            text_offset,
            text_len,
            timestamp_ms: self.now_ms(),
            metadata_u32_0: 0,
            metadata_u32_1: 0,
            metadata_u32_2: 0,
        };

        if self.count >= SIDEBAR_BUFFER_SIZE {
            self.head = (self.head + 1) % SIDEBAR_BUFFER_SIZE;
        } else {
            self.count += 1;
        }
    }

    fn entry_at(&self, logical_idx: usize) -> &SidebarEntry {
        let idx = if self.count >= SIDEBAR_BUFFER_SIZE {
            (self.head + logical_idx) % SIDEBAR_BUFFER_SIZE
        } else {
            logical_idx
        };
        &self.entries[idx]
    }

    pub fn visible_entries(&self) -> impl Iterator<Item = &SidebarEntry> {
        let count = self.count.min(SIDEBAR_BUFFER_SIZE);
        (0..count)
            .rev()
            .map(move |i| self.entry_at(i))
            .filter(|e| self.filter_types[e.entry_type as usize])
    }

    pub fn entry_count(&self) -> usize {
        self.count.min(SIDEBAR_BUFFER_SIZE)
    }

    pub fn filtered_count(&self) -> usize {
        self.visible_entries().count()
    }

    pub fn set_filter(&mut self, entry_type: SidebarEntryType, enabled: bool) {
        self.filter_types[entry_type as usize] = enabled;
    }

    pub fn set_sort_by_priority(&mut self, by_priority: bool) {
        self.sort_by_priority = by_priority;
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.head = 0;
        self.text_used = 0;
    }
}

/// Immutable snapshot for rendering
pub struct SmartSidebarSnapshot<'a> {
    pub state: &'a SmartSidebarState,
}

impl<'a> SmartSidebarSnapshot<'a> {
    pub fn new(state: &'a SmartSidebarState) -> Self {
        Self { state }
    }
}

/// Render smart sidebar panel
pub fn render_smart_sidebar(ui: &mut egui::Ui, snapshot: &SmartSidebarSnapshot, palette: IdePalette) {
    let frame = egui::Frame::new()
        .fill(palette.bg_secondary)
        .inner_margin(8.0)
        .stroke(egui::Stroke::new(1.0, palette.border));

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Context")
                    .size(12.0)
                    .strong()
                    .color(palette.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("({})", snapshot.state.filtered_count()))
                        .size(10.0)
                        .color(palette.text_muted),
                );
            });
        });

        if snapshot.state.entry_count() == 0 {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("No context available yet")
                    .size(10.0)
                    .color(palette.text_muted),
            );
            return;
        }

        ui.separator();

        egui::ScrollArea::vertical()
            .max_height(400.0)
            .show(ui, |ui| {
                for entry in snapshot.state.visible_entries() {
                    let (icon, color, type_name) = match entry.entry_type {
                        SidebarEntryType::FileReference => ("F", palette.accent, "File"),
                        SidebarEntryType::SymbolDefinition => {
                            ("S", palette.accent.gamma_multiply(0.85), "Symbol")
                        }
                        SidebarEntryType::CodeSuggestion => ("*", palette.warning, "Suggest"),
                        SidebarEntryType::QuickAction => {
                            (">", palette.accent.gamma_multiply(1.1), "Action")
                        }
                        SidebarEntryType::ErrorDiagnostic => ("x", palette.error, "Error"),
                        SidebarEntryType::WarningDiagnostic => ("!", palette.warning, "Warn"),
                        SidebarEntryType::TodoComment => ("-", palette.success, "TODO"),
                        SidebarEntryType::Note => ("n", palette.text_muted, "Note"),
                    };

                    let file = snapshot.state.get_file(entry.file_offset, entry.file_len);
                    let text = snapshot.state.get_text(entry.text_offset, entry.text_len);

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(icon).size(12.0).color(color));
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(type_name)
                                            .size(9.0)
                                            .color(palette.text_muted),
                                    );
                                    ui.label(
                                        egui::RichText::new(text)
                                            .size(10.0)
                                            .color(palette.text)
                                            .strong(),
                                    );
                                });
                                if !file.is_empty() {
                                    ui.horizontal(|ui| {
                                        ui.add_space(20.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} :{}",
                                                file, entry.line_number
                                            ))
                                            .size(8.0)
                                            .color(palette.text_muted),
                                        );
                                    });
                                }
                            });
                        });
                    });
                    ui.add_space(2.0);
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidebar_ring_buffer() {
        let mut sidebar = SmartSidebarState::default();

        // Fill beyond capacity
        for i in 0..SIDEBAR_BUFFER_SIZE + 10 {
            sidebar.add_file_reference(1, &format!("file{}.rs", i), i as u32, 0, "test");
        }

        assert_eq!(sidebar.entry_count(), SIDEBAR_BUFFER_SIZE);
    }

    #[test]
    fn test_sidebar_text_pool() {
        let mut sidebar = SmartSidebarState::default();
        sidebar.add_file_reference(1, "main.rs", 10, 5, "main function");

        let entries: Vec<_> = sidebar.visible_entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, SidebarEntryType::FileReference);
        assert_eq!(
            sidebar.get_file(entries[0].file_offset, entries[0].file_len),
            "main.rs"
        );
        assert_eq!(
            sidebar.get_text(entries[0].text_offset, entries[0].text_len),
            "main function"
        );
    }

    #[test]
    fn test_sidebar_filtering() {
        let mut sidebar = SmartSidebarState::default();
        sidebar.add_file_reference(1, "a.rs", 1, 0, "ref");
        sidebar.add_diagnostic(1, true, "b.rs", 2, 0, "error");
        sidebar.add_suggestion(1, "fix it", "c.rs", 3);

        sidebar.set_filter(SidebarEntryType::FileReference, false);
        let filtered: Vec<_> = sidebar.visible_entries().collect();
        assert_eq!(filtered.len(), 2); // error + suggestion

        sidebar.set_filter(SidebarEntryType::ErrorDiagnostic, false);
        let filtered: Vec<_> = sidebar.visible_entries().collect();
        assert_eq!(filtered.len(), 1); // only suggestion
    }
}
