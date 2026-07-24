#![allow(dead_code)]
//! Diagnostics display — manages error/warning squiggles and the problems panel.

use std::path::PathBuf;
use eframe::egui;

pub use crate::editor::lsp_client::{LspDiagnostic, DiagnosticSeverity};

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
        self.items.iter().filter(|d| d.severity == DiagnosticSeverity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.items.iter().filter(|d| d.severity == DiagnosticSeverity::Warning).count()
    }

    pub fn filtered_items(&self) -> Vec<&LspDiagnostic> {
        self.items.iter().filter(|d| match self.filter {
            DiagnosticFilter::All => true,
            DiagnosticFilter::ErrorsOnly => d.severity == DiagnosticSeverity::Error,
            DiagnosticFilter::WarningsOnly => d.severity == DiagnosticSeverity::Warning,
        }).collect()
    }

    /// Get diagnostics for a specific file.
    pub fn for_file(&self, path: &PathBuf) -> Vec<&LspDiagnostic> {
        self.items.iter().filter(|d| &d.file == path).collect()
    }

    /// Render the problems panel.
    pub fn show_panel(&mut self, ui: &mut egui::Ui, palette: &crate::editor::theme::IdePalette) -> Option<DiagnosticAction> {
        let mut action = None;

        ui.horizontal(|ui| {
            ui.heading("Problems");
            ui.separator();
            let errors = self.error_count();
            let warnings = self.warning_count();
            ui.colored_label(palette.error, format!("\u{2716} {errors}"));
            ui.colored_label(palette.warning, format!("\u{26A0} {warnings}"));
            ui.separator();
            if ui.selectable_label(self.filter == DiagnosticFilter::All, "All").clicked() {
                self.filter = DiagnosticFilter::All;
            }
            if ui.selectable_label(self.filter == DiagnosticFilter::ErrorsOnly, "Errors").clicked() {
                self.filter = DiagnosticFilter::ErrorsOnly;
            }
            if ui.selectable_label(self.filter == DiagnosticFilter::WarningsOnly, "Warnings").clicked() {
                self.filter = DiagnosticFilter::WarningsOnly;
            }
        });

        ui.separator();

        let filtered: Vec<(usize, LspDiagnostic)> = self.filtered_items().iter().enumerate()
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

                let file_name = diag.file.file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let label = format!("{} {}:{}:{} {}", icon.0, file_name, diag.line + 1, diag.col + 1, diag.message);

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
            if let Some((idx, _)) = filtered.iter().find(|(_, d)| &d.file == file && d.line == line) {
                self.selected = *idx;
            }
        }

        action
    }
}

/// Actions from the diagnostics panel.
#[derive(Debug, Clone)]
pub enum DiagnosticAction {
    Jump { file: PathBuf, line: usize, col: usize },
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
                file: PathBuf::from("a.rs"), line: 0, col: 0, end_line: 0, end_col: 5,
                severity: DiagnosticSeverity::Error, message: "err".into(), source: None, code: None,
            },
            LspDiagnostic {
                file: PathBuf::from("b.rs"), line: 1, col: 0, end_line: 1, end_col: 3,
                severity: DiagnosticSeverity::Warning, message: "warn".into(), source: None, code: None,
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
                file: PathBuf::from("a.rs"), line: 0, col: 0, end_line: 0, end_col: 5,
                severity: DiagnosticSeverity::Error, message: "err".into(), source: None, code: None,
            },
            LspDiagnostic {
                file: PathBuf::from("b.rs"), line: 1, col: 0, end_line: 1, end_col: 3,
                severity: DiagnosticSeverity::Warning, message: "warn".into(), source: None, code: None,
            },
        ]);
        state.filter = DiagnosticFilter::ErrorsOnly;
        assert_eq!(state.filtered_items().len(), 1);
    }
}
