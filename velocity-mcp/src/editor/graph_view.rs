//! Code explorer — a lightweight, drill-down alternative to a force-directed
//! graph. The user expands a file to reveal the symbols it defines, then expands
//! a symbol to reveal its call relations (callers / callees), all sourced from
//! the site map. Only the visible slice of the graph is materialized, so it
//! stays cheap even on large workspaces.

use crate::editor::theme::IdePalette;
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A symbol defined by a file, with its (best-effort) definition line.
#[derive(Clone, Debug)]
struct FileSymbolEntry {
    name: String,
    line: Option<usize>,
}

/// Cached, grouped view of the site map: files → defined symbols.
#[derive(Clone, Debug, Default)]
struct ExplorerModel {
    /// relative file path → symbols it defines (predicate 1), sorted by name.
    files: BTreeMap<String, Vec<FileSymbolEntry>>,
}

/// Actions the explorer can request of the host app.
#[derive(Clone, Debug)]
pub enum GraphAction {
    /// Jump to a symbol's definition (resolved by name via the workspace index).
    NavigateToSymbol(String),
}

/// Selection focus for the detail pane.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Selection {
    File(String),
    Symbol { file: String, name: String },
}

pub struct MerkleGraphView {
    model: Option<ExplorerModel>,
    expanded_files: BTreeSet<String>,
    expanded_symbols: BTreeSet<String>,
    selected: Option<Selection>,
}

impl MerkleGraphView {
    pub fn new() -> Self {
        Self {
            model: None,
            expanded_files: BTreeSet::new(),
            expanded_symbols: BTreeSet::new(),
            selected: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        workspace_root: &Path,
        palette: IdePalette,
    ) -> Option<GraphAction> {
        if self.model.is_none() {
            self.refresh(workspace_root);
        }

        let mut action: Option<GraphAction> = None;

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Code Explorer")
                        .size(13.0)
                        .strong()
                        .color(palette.accent),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("\u{27f3} Refresh").clicked() {
                        self.refresh(workspace_root);
                    }
                });
            });
            ui.separator();

            let model = match self.model.clone() {
                Some(m) if !m.files.is_empty() => m,
                _ => {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("\u{25c7}")
                                .size(28.0)
                                .color(palette.accent.gamma_multiply(0.7)),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                "No indexed symbols yet \u{2014} run the indexer, then Refresh.",
                            )
                            .color(palette.text_muted),
                        );
                    });
                    return;
                }
            };

            let file_count = model.files.len();
            let symbol_count: usize = model.files.values().map(|v| v.len()).sum();
            ui.label(
                egui::RichText::new(format!(
                    "{} files \u{00b7} {} symbols \u{2014} expand a file, then a symbol to trace relations",
                    file_count, symbol_count
                ))
                .small()
                .color(palette.text_muted),
            );
            ui.separator();

            let sm_ok = crate::automation::open_workspace_site_map(workspace_root).ok();

            ui.columns(2, |cols| {
                // Left: drill-down tree (files → symbols).
                cols[0].vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("explorer_tree")
                                            .max_height(ui.available_height())
                                            .show(ui, |ui| {
                            for (file, symbols) in &model.files {
                                let file_expanded = self.expanded_files.contains(file);
                                let is_sel =
                                    self.selected.as_ref() == Some(&Selection::File(file.clone()));
                                let resp = ui
                                    .horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(if file_expanded {
                                                "\u{25bc}"
                                            } else {
                                                "\u{25b6}"
                                            })
                                            .size(9.0)
                                            .color(palette.text_muted),
                                        );
                                        ui.label(
                                            egui::RichText::new("\u{25a4}")
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.text_muted),
                                        );
                                        ui.selectable_label(
                                            is_sel,
                                            egui::RichText::new(file_name(file)).color(if is_sel {
                                                palette.accent
                                            } else {
                                                palette.text
                                            }),
                                        )
                                    })
                                    .inner;
                                if resp.clicked() {
                                    self.selected = Some(Selection::File(file.clone()));
                                    if file_expanded {
                                        self.expanded_files.remove(file);
                                    } else {
                                        self.expanded_files.insert(file.clone());
                                    }
                                }

                                if file_expanded {
                                    for sym in symbols {
                                        let key = symbol_key(file, &sym.name);
                                        let sym_expanded = self.expanded_symbols.contains(&key);
                                        let is_sym_sel = self.selected.as_ref()
                                            == Some(&Selection::Symbol {
                                                file: file.clone(),
                                                name: sym.name.clone(),
                                            });
                                        let sresp = ui
                                            .horizontal(|ui| {
                                                ui.add_space(16.0);
                                                ui.label(
                                                    egui::RichText::new(if sym_expanded {
                                                        "\u{25bc}"
                                                    } else {
                                                        "\u{25b6}"
                                                    })
                                                    .size(9.0)
                                                    .color(palette.text_muted),
                                                );
                                                ui.label(
                                                    egui::RichText::new("\u{0192}")
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(palette.accent),
                                                );
                                                ui.selectable_label(
                                                    is_sym_sel,
                                                    egui::RichText::new(&sym.name).color(
                                                        if is_sym_sel {
                                                            palette.accent
                                                        } else {
                                                            palette.text
                                                        },
                                                    ),
                                                )
                                            })
                                            .inner;
                                        if sresp.clicked() {
                                            self.selected = Some(Selection::Symbol {
                                                file: file.clone(),
                                                name: sym.name.clone(),
                                            });
                                            if sym_expanded {
                                                self.expanded_symbols.remove(&key);
                                            } else {
                                                self.expanded_symbols.insert(key.clone());
                                            }
                                        }

                                        // Drill into a symbol's call relations.
                                        if sym_expanded {
                                            if let Some(sm) = &sm_ok {
                                                let hash = crate::editor::app::helpers::hash_str(
                                                    &sym.name,
                                                );
                                                let deps = sm.get_dependencies(hash);
                                                let callers = sm.get_callers(hash);
                                                let dep_names = resolve_names(sm, &deps);
                                                let caller_names = resolve_names(sm, &callers);
                                                if dep_names.is_empty() && caller_names.is_empty() {
                                                    ui.horizontal(|ui| {
                                                        ui.add_space(40.0);
                                                        ui.label(
                                                            egui::RichText::new(
                                                                "no indexed relations",
                                                            )
                                                            .small()
                                                            .color(palette.text_muted),
                                                        );
                                                    });
                                                } else {
                                                    for callee in &dep_names {
                                                        if relation_row(
                                                            ui,
                                                            palette,
                                                            "\u{21b3} calls",
                                                            callee,
                                                        ) {
                                                            action = Some(
                                                                GraphAction::NavigateToSymbol(
                                                                    callee.clone(),
                                                                ),
                                                            );
                                                        }
                                                    }
                                                    for caller in &caller_names {
                                                        if relation_row(
                                                            ui,
                                                            palette,
                                                            "\u{21b0} called by",
                                                            caller,
                                                        ) {
                                                            action = Some(
                                                                GraphAction::NavigateToSymbol(
                                                                    caller.clone(),
                                                                ),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                });

                // Right: detail pane for the current selection.
                cols[1].vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("explorer_detail")
                                            .max_height(ui.available_height())
                                            .show(ui, |ui| match self.selected.clone() {
                            Some(Selection::Symbol { name, .. }) => {
                                ui.label(
                                    egui::RichText::new(format!("\u{0192} {}", name))
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.add_space(4.0);
                                if ui.button("Open definition").clicked() {
                                    action = Some(GraphAction::NavigateToSymbol(name.clone()));
                                }
                                ui.add_space(8.0);
                                if let Some(sm) = &sm_ok {
                                    let hash = crate::editor::app::helpers::hash_str(&name);
                                    let callers = resolve_names(sm, &sm.get_callers(hash));
                                    let callees = resolve_names(sm, &sm.get_dependencies(hash));
                                    relation_section(ui, palette, "Calls", &callees, &mut action);
                                    relation_section(
                                        ui,
                                        palette,
                                        "Called by",
                                        &callers,
                                        &mut action,
                                    );
                                }
                            }
                            Some(Selection::File(file)) => {
                                ui.label(
                                    egui::RichText::new(format!("\u{25a4} {}", file_name(&file)))
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    egui::RichText::new(&file).small().color(palette.text_muted),
                                );
                                ui.add_space(6.0);
                                if let Some(symbols) = model.files.get(&file) {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Defines {} symbol(s)",
                                            symbols.len()
                                        ))
                                        .small()
                                        .strong()
                                        .color(palette.text_muted),
                                    );
                                    ui.add_space(4.0);
                                    for sym in symbols {
                                        let line_label =
                                            sym.line.map(|l| format!(":{}", l)).unwrap_or_default();
                                        if ui
                                            .link(egui::RichText::new(format!(
                                                "\u{0192} {}{}",
                                                sym.name, line_label
                                            )))
                                            .clicked()
                                        {
                                            action = Some(GraphAction::NavigateToSymbol(
                                                sym.name.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            None => {
                                ui.add_space(16.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("\u{25cc}")
                                            .size(28.0)
                                            .color(palette.accent.gamma_multiply(0.7)),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(
                                            "Select a file or symbol to inspect its relations",
                                        )
                                        .color(palette.text_muted),
                                    );
                                });
                            }
                        });
                });
            });
        });

        action
    }

    fn refresh(&mut self, workspace_root: &Path) {
        let entries = crate::editor::search::collect_workspace_symbols(workspace_root);
        let mut files: BTreeMap<String, Vec<FileSymbolEntry>> = BTreeMap::new();
        for e in entries {
            let abs = workspace_root.join(&e.file);
            let line = std::fs::read_to_string(&abs)
                .ok()
                .and_then(|c| crate::editor::search::find_definition_line(&c, &e.name));
            files
                .entry(e.file)
                .or_default()
                .push(FileSymbolEntry { name: e.name, line });
        }
        for syms in files.values_mut() {
            syms.sort_by(|a, b| a.name.cmp(&b.name));
        }
        self.model = Some(ExplorerModel { files });
        self.selected = None;
    }
}

/// Last path segment of a `/`-separated relative path.
fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn symbol_key(file: &str, name: &str) -> String {
    format!("{}::{}", file, name)
}

/// Resolve hashes to readable names, dropping path-like entries and hex
/// fallbacks, sorted and de-duplicated.
fn resolve_names(sm: &velocity_ide::site_map::SiteMap, hashes: &[u64]) -> Vec<String> {
    let mut out: Vec<String> = hashes
        .iter()
        .filter_map(|h| sm.resolve_string(*h))
        .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains('\\'))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A single indented relation row; returns true when clicked.
fn relation_row(ui: &mut egui::Ui, palette: IdePalette, label: &str, name: &str) -> bool {
    ui.horizontal(|ui| {
        ui.add_space(40.0);
        ui.label(egui::RichText::new(label).small().color(palette.text_muted));
        ui.link(egui::RichText::new(name).size(11.0))
    })
    .inner
    .clicked()
}

/// A titled block of clickable relation links in the detail pane.
fn relation_section(
    ui: &mut egui::Ui,
    palette: IdePalette,
    title: &str,
    names: &[String],
    action: &mut Option<GraphAction>,
) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(format!("{} ({})", title, names.len()))
            .small()
            .strong()
            .color(palette.text_muted),
    );
    if names.is_empty() {
        ui.label(egui::RichText::new("  \u{2014}").color(palette.text_muted));
    } else {
        for n in names {
            if ui
                .link(egui::RichText::new(format!("\u{2192} {}", n)).size(11.0))
                .clicked()
            {
                *action = Some(GraphAction::NavigateToSymbol(n.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_takes_last_segment() {
        assert_eq!(file_name("velocity-mcp/src/editor/app.rs"), "app.rs");
        assert_eq!(file_name("main.rs"), "main.rs");
    }

    #[test]
    fn symbol_key_is_unique_per_file() {
        assert_ne!(symbol_key("src/a.rs", "run"), symbol_key("src/b.rs", "run"));
    }
}
