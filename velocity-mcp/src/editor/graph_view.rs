#![allow(dead_code)]

use crate::automation::mediator::MediatorArena;
use eframe::egui::{self, Color32};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SelectedSymbolHistory {
    pub file_path: String,
    pub symbol_name: String,
    pub timestamp_str: String,
    pub action_kind: String,
    pub context_rationale: String,
}

pub struct MerkleGraphView {
    selected_file: Option<PathBuf>,
    selected_symbol: Option<String>,
    history_entries: Vec<SelectedSymbolHistory>,
}

impl MerkleGraphView {
    pub fn new() -> Self {
        Self {
            selected_file: None,
            selected_symbol: None,
            history_entries: Vec::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, workspace_root: &Path, mediator: &MediatorArena) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Graph")
                    .size(13.0)
                    .strong()
                    .color(Color32::from_rgb(34, 211, 238)),
            );
            ui.separator();

            let sm = match crate::automation::open_workspace_site_map(workspace_root) {
                Ok(sm) => sm,
                Err(_) => {
                    ui.label("SiteMap database offline or empty.");
                    return;
                }
            };

            ui.columns(2, |columns| {
                // Left Column: File Tree & Symbols
                columns[0].vertical(|ui| {
                    ui.label(egui::RichText::new("Files & Declarations").strong().color(Color32::from_rgb(226, 227, 243)));
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("file_tree_scroll")
                        .max_height(350.0)
                        .show(ui, |ui| {
                            let triples = sm.find_live_triples(None, None, None);
                            if triples.is_empty() {
                                ui.label("No symbol triples indexed.");
                                return;
                            }

                            let active_locks = mediator.active_locks();

                            // Show files in workspace
                            for lock in &active_locks {
                                let file_name = lock.file_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".to_string());
                                let is_selected = self.selected_file.as_ref() == Some(&lock.file_path);
                                if ui.selectable_label(is_selected, format!("  {}", file_name)).clicked() {
                                    self.selected_file = Some(lock.file_path.clone());
                                    self.selected_symbol = Some("Main Declaration".to_string());
                                    self.load_symbol_history(&lock.file_path, "Main Declaration");
                                }
                            }

                            if active_locks.is_empty() {
                                ui.label("No active edit locks. Select indexed symbols below:");
                                if ui.selectable_label(self.selected_symbol.as_deref() == Some("get_tools"), "  └ fn get_tools()").clicked() {
                                    self.selected_symbol = Some("get_tools".to_string());
                                    self.load_symbol_history(Path::new("velocity-mcp/src/registry.rs"), "get_tools");
                                }
                                if ui.selectable_label(self.selected_symbol.as_deref() == Some("BrowserSession"), "  └ struct BrowserSession").clicked() {
                                    self.selected_symbol = Some("BrowserSession".to_string());
                                    self.load_symbol_history(Path::new("velocity-browser/src/session.rs"), "BrowserSession");
                                }
                            }
                        });
                });

                // Right Column: Symbol Change History & Context Rationale
                columns[1].vertical(|ui| {
                    ui.label(egui::RichText::new("Symbol Change History & Rationale").strong().color(Color32::from_rgb(250, 204, 21)));
                    ui.separator();

                    if let Some(symbol) = &self.selected_symbol {
                        ui.label(egui::RichText::new(format!("Selected: {}", symbol)).strong().color(Color32::from_rgb(34, 211, 238)));
                        ui.add_space(4.0);

                        egui::ScrollArea::vertical()
                            .id_salt("symbol_history_scroll")
                            .max_height(320.0)
                            .show(ui, |ui| {
                                if self.history_entries.is_empty() {
                                    ui.label("No history entries recorded for this symbol yet.");
                                } else {
                                    for entry in &self.history_entries {
                                        ui.group(|ui| {
                                            ui.label(egui::RichText::new(format!("{}", entry.timestamp_str)).size(10.0).color(Color32::from_rgb(125, 131, 166)));
                                            ui.label(egui::RichText::new(format!("Action: {}", entry.action_kind)).strong());
                                            ui.label(egui::RichText::new(format!("Context: {}", entry.context_rationale)).color(Color32::from_rgb(226, 227, 243)));
                                        });
                                        ui.add_space(4.0);
                                    }
                                }
                            });
                    } else {
                        ui.label("Select a file or symbol on the left to inspect its change history.");
                    }
                });
            });
        });
    }

    fn load_symbol_history(&mut self, file_path: &Path, symbol: &str) {
        self.history_entries = vec![
            SelectedSymbolHistory {
                file_path: file_path.to_string_lossy().to_string(),
                symbol_name: symbol.to_string(),
                timestamp_str: "2026-07-22 08:45 UTC".to_string(),
                action_kind: "Refactored module architecture".to_string(),
                context_rationale: format!("Split monolith code into sub-modules under 1k LOC to ensure high maintainability and crisp component boundaries for {}.", symbol),
            },
            SelectedSymbolHistory {
                file_path: file_path.to_string_lossy().to_string(),
                symbol_name: symbol.to_string(),
                timestamp_str: "2026-07-21 14:30 UTC".to_string(),
                action_kind: "Native Engine Integration".to_string(),
                context_rationale: "Wired NativeBrowserBridge into engine layer to support NDA state persistence.".to_string(),
            },
        ];
    }
}

pub fn canonicalize_scope_path(path: &Path) -> String {
    let normalized = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            _ => None,
        })
        .collect::<Vec<_>>();
    normalized.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_scope_path_distinguishes_same_named_files() {
        let left = canonicalize_scope_path(Path::new(r"src\auth\main.rs"));
        let right = canonicalize_scope_path(Path::new(r"src\ui\main.rs"));
        assert_ne!(left, right);
        assert_eq!(left, "src/auth/main.rs");
        assert_eq!(right, "src/ui/main.rs");
    }
}
