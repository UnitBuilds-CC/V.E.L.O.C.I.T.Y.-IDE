#![allow(dead_code)]
//! Diagnostics display — manages error/warning squiggles and the problems panel.

use eframe::egui;
use std::path::PathBuf;

pub use crate::editor::lsp_client::{DiagnosticSeverity, LspDiagnostic};

/// Aggregated diagnostics state across all open files.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsState {
    pub items: Vec<LspDiagnostic>,
    /// Filter: show errors only, or all severities.
    pub filter: DiagnosticFilter,
    /// Selected item in the problems panel.
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagnosticFilter {
    #[default]
    All,
    ErrorsOnly,
    WarningsOnly,
}

impl DiagnosticsState {
    pub fn update(&mut self, diagnostics: Vec<LspDiagnostic>) {
        self.items = diagnostics;
        if self.selected >= self.items.len() {
            self.selected = 0;
        }
    }

    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count()
    }

    pub fn filtered_items(&self) -> Vec<&LspDiagnostic> {
        self.items
            .iter()
            .filter(|d| match self.filter {
                DiagnosticFilter::All => true,
                DiagnosticFilter::ErrorsOnly => d.severity == DiagnosticSeverity::Error,
                DiagnosticFilter::WarningsOnly => d.severity == DiagnosticSeverity::Warning,
            })
            .collect()
    }

    /// Get diagnostics for a specific file.
    pub fn for_file(&self, path: &PathBuf) -> Vec<&LspDiagnostic> {
        self.items.iter().filter(|d| &d.file == path).collect()
    }

    /// Get (line, severity_u8) pairs for the gutter display in a specific file.
    /// severity: 1=error, 2=warning, 3=info, 4=hint
    pub fn lines_for_file(&self, path: &std::path::Path) -> Vec<(usize, u8)> {
        self.items
            .iter()
            .filter(|d| d.file == path)
            .map(|d| {
                let sev = match d.severity {
                    DiagnosticSeverity::Error => 1u8,
                    DiagnosticSeverity::Warning => 2,
                    DiagnosticSeverity::Info => 3,
                    DiagnosticSeverity::Hint => 4,
                };
                (d.line + 1, sev) // convert 0-based to 1-based
            })
            .collect()
    }

    /// Get diagnostics for a specific line in a file (0-based line number).
    pub fn diagnostics_at_line(&self, path: &std::path::Path, line: usize) -> Vec<&LspDiagnostic> {
        self.items
            .iter()
            .filter(|d| d.file == path && d.line == line)
            .collect()
    }

    /// Render an inline diagnostic popup at the given cursor position.
    /// Returns true if a popup was rendered.
    pub fn render_inline_popup_at_line(
        &self,
        ui: &mut egui::Ui,
        path: &std::path::Path,
        line: usize,
        cursor_pos: egui::Pos2,
        palette: &crate::editor::theme::IdePalette,
    ) -> bool {
        let diagnostics = self.diagnostics_at_line(path, line);
        if diagnostics.is_empty() {
            return false;
        }

        // Position the popup below the cursor
        let popup_id = egui::Id::new("inline_diagnostic_popup");
        let area = egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(cursor_pos.x + 20.0, cursor_pos.y + 20.0));

        area.show(ui.ctx(), |ui| {
            let frame = egui::Frame::popup(ui.style())
                .fill(ui.visuals().extreme_bg_color)
                .stroke(egui::Stroke::new(1.0, palette.error.gamma_multiply(0.6)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8));

            frame.show(ui, |ui| {
                ui.set_max_width(400.0);
                for diag in &diagnostics {
                    let (icon, color) = match diag.severity {
                        DiagnosticSeverity::Error => ("\u{2716}", palette.error),
                        DiagnosticSeverity::Warning => ("\u{26a0}", palette.warning),
                        DiagnosticSeverity::Info => ("\u{2139}", palette.accent),
                        DiagnosticSeverity::Hint => ("\u{1f4a1}", palette.text_muted),
                    };

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(icon).size(12.0).strong().color(color));
                        ui.label(
                            egui::RichText::new(&diag.message)
                                .size(11.0)
                                .color(palette.text),
                        );
                    });

                    if let Some(code) = &diag.code {
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new(format!("[{}]", code))
                                    .monospace()
                                    .size(9.0)
                                    .color(palette.text_muted),
                            );
                            if let Some(source) = &diag.source {
                                ui.label(
                                    egui::RichText::new(source)
                                        .size(9.0)
                                        .color(palette.text_muted.gamma_multiply(0.7)),
                                );
                            }
                        });
                    }
                }
            });
        });

        true
    }

    /// Render the problems panel.
    pub fn show_panel(
        &mut self,
        ui: &mut egui::Ui,
        palette: &crate::editor::theme::IdePalette,
    ) -> Option<DiagnosticAction> {
        let mut action = None;

        ui.horizontal(|ui| {
            ui.heading("Problems");
            ui.separator();
            let errors = self.error_count();
            let warnings = self.warning_count();
            ui.colored_label(palette.error, format!("\u{2716} {errors}"));
            ui.colored_label(palette.warning, format!("\u{26A0} {warnings}"));
            ui.separator();
            if ui
                .selectable_label(self.filter == DiagnosticFilter::All, "All")
                .clicked()
            {
                self.filter = DiagnosticFilter::All;
            }
            if ui
                .selectable_label(self.filter == DiagnosticFilter::ErrorsOnly, "Errors")
                .clicked()
            {
                self.filter = DiagnosticFilter::ErrorsOnly;
            }
            if ui
                .selectable_label(self.filter == DiagnosticFilter::WarningsOnly, "Warnings")
                .clicked()
            {
                self.filter = DiagnosticFilter::WarningsOnly;
            }
        });

        ui.separator();

        let filtered: Vec<(usize, LspDiagnostic)> = self
            .filtered_items()
            .iter()
            .enumerate()
            .map(|(i, d)| (i, (*d).clone()))
            .collect();
        let current_selected = self.selected;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, diag) in &filtered {
                let icon = match diag.severity {
                    DiagnosticSeverity::Error => ("\u{2716}", palette.error),
                    DiagnosticSeverity::Warning => ("\u{26A0}", palette.warning),
                    DiagnosticSeverity::Info => ("\u{2139}", palette.accent),
                    DiagnosticSeverity::Hint => ("\u{1F4A1}", palette.text_muted),
                };

                let file_name = diag.file.file_name().unwrap_or_default().to_string_lossy();
                let label = format!(
                    "{} {}:{}:{} {}",
                    icon.0,
                    file_name,
                    diag.line + 1,
                    diag.col + 1,
                    diag.message
                );

                let resp = ui.selectable_label(*idx == current_selected, &label);
                if resp.clicked() {
                    action = Some(DiagnosticAction::Jump {
                        file: diag.file.clone(),
                        line: diag.line,
                        col: diag.col,
                    });
                }
            }
        });

        if let Some(DiagnosticAction::Jump { ref file, line, .. }) = action {
            // Update selected based on the clicked index
            if let Some((idx, _)) = filtered
                .iter()
                .find(|(_, d)| &d.file == file && d.line == line)
            {
                self.selected = *idx;
            }
        }

        action
    }
}

/// Actions from the diagnostics panel.
#[derive(Debug, Clone)]
pub enum DiagnosticAction {
    Jump {
        file: PathBuf,
        line: usize,
        col: usize,
    },
}

/// Inline diagnostic rendering data (for squiggles in the editor).
#[derive(Debug, Clone)]
pub struct InlineDiagnostic {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

impl InlineDiagnostic {
    pub fn from_lsp(diag: &LspDiagnostic) -> Self {
        Self {
            line: diag.line,
            start_col: diag.col,
            end_col: diag.end_col,
            severity: diag.severity,
            message: diag.message.clone(),
        }
    }

    pub fn squiggle_color(&self, palette: &crate::editor::theme::IdePalette) -> egui::Color32 {
        match self.severity {
            DiagnosticSeverity::Error => palette.error,
            DiagnosticSeverity::Warning => palette.warning,
            DiagnosticSeverity::Info => palette.accent,
            DiagnosticSeverity::Hint => palette.text_muted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_counts() {
        let mut state = DiagnosticsState::default();
        state.update(vec![
            LspDiagnostic {
                file: PathBuf::from("a.rs"),
                line: 0,
                col: 0,
                end_line: 0,
                end_col: 5,
                severity: DiagnosticSeverity::Error,
                message: "err".into(),
                source: None,
                code: None,
            },
            LspDiagnostic {
                file: PathBuf::from("b.rs"),
                line: 1,
                col: 0,
                end_line: 1,
                end_col: 3,
                severity: DiagnosticSeverity::Warning,
                message: "warn".into(),
                source: None,
                code: None,
            },
        ]);
        assert_eq!(state.error_count(), 1);
        assert_eq!(state.warning_count(), 1);
    }

    #[test]
    fn filter_errors_only() {
        let mut state = DiagnosticsState::default();
        state.update(vec![
            LspDiagnostic {
                file: PathBuf::from("a.rs"),
                line: 0,
                col: 0,
                end_line: 0,
                end_col: 5,
                severity: DiagnosticSeverity::Error,
                message: "err".into(),
                source: None,
                code: None,
            },
            LspDiagnostic {
                file: PathBuf::from("b.rs"),
                line: 1,
                col: 0,
                end_line: 1,
                end_col: 3,
                severity: DiagnosticSeverity::Warning,
                message: "warn".into(),
                source: None,
                code: None,
            },
        ]);
        state.filter = DiagnosticFilter::ErrorsOnly;
        assert_eq!(state.filtered_items().len(), 1);
    }
}
