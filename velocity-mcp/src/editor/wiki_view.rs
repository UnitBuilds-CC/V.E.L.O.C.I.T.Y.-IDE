//! Wiki tab — browses a sitemap-generated wiki and exports it to Markdown.

use eframe::egui;
use velocity_ide::wiki::{build_wiki, export_markdown, render_page_markdown, WikiModel, WikiPage};

use crate::editor::theme::IdePalette;
use crate::editor::toast::{Toast, ToastQueue};

/// Which wiki page is selected in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageRef {
    Overview,
    File(usize),
    Symbol(usize),
}

/// Actions the Wiki tab cannot perform itself and hands back to the app.
pub enum WikiAction {
    /// Ask the connected agent to narrate a detailed wiki page.
    GenerateDetail(String),
}

enum LoadState {
    Idle,
    Loading,
    Ready,
    Error(String),
}

pub struct WikiView {
    model: Option<WikiModel>,
    selected: Option<PageRef>,
    query: String,
    state: LoadState,
    /// Receiver for a wiki model being built on a background thread.
    rx: Option<std::sync::mpsc::Receiver<Result<WikiModel, String>>>,
    /// Receiver for a background index rebuild (populates dictionary.json).
    index_rx: Option<std::sync::mpsc::Receiver<Result<(usize, String), String>>>,
}

impl Default for WikiView {
    fn default() -> Self {
        Self::new()
    }
}

impl WikiView {
    pub fn new() -> Self {
        Self {
            model: None,
            selected: None,
            query: String::new(),
            state: LoadState::Idle,
            rx: None,
            index_rx: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        workspace_root: &std::path::Path,
        toasts: &mut ToastQueue,
        palette: IdePalette,
    ) -> Option<WikiAction> {
        // Load lazily on first show so opening the tab always has content.
        if matches!(self.state, LoadState::Idle) {
            self.refresh(workspace_root);
        }
        // Drain the background builder and keep animating while it runs.
        self.poll_refresh(toasts);
        self.poll_index(toasts);
        if matches!(self.state, LoadState::Loading) || self.index_rx.is_some() {
            ui.ctx().request_repaint();
        }

        let mut action: Option<WikiAction> = None;

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Wiki")
                        .size(13.0)
                        .strong()
                        .color(palette.accent),
                );
                ui.add_space(8.0);
                if let Some(model) = &self.model {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} files \u{00b7} {} symbols",
                            model.file_count(),
                            model.symbol_count()
                        ))
                        .size(11.0)
                        .color(palette.text_muted),
                    );
                }
            });
            ui.separator();

            // Toolbar. Plain `ui.button` inherits `weak_bg_fill = bg_secondary`, which
            // matches the panel and renders the buttons as chrome-less text; give each
            // an explicit neutral fill + hairline border so they read as buttons.
            ui.horizontal(|ui| {
                // Breathing room so the neutral buttons don't read as one merged slab.
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Refresh").color(palette.text))
                            .fill(palette.bg_tertiary)
                            .stroke(egui::Stroke::new(0.5, palette.border))
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(0.0, 24.0)),
                    )
                    .on_hover_text("Rebuild the wiki from the site map")
                    .clicked()
                {
                    self.refresh(workspace_root);
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Export Markdown").color(palette.success),
                        )
                        .fill(palette.bg_tertiary)
                        .stroke(egui::Stroke::new(0.5, palette.border))
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(0.0, 24.0)),
                    )
                    .on_hover_text("Write interlinked .wiki/ pages you can commit to git")
                    .clicked()
                {
                    self.export(workspace_root, toasts);
                }
                let indexing = self.index_rx.is_some();
                let index_label = if indexing {
                    "Indexing\u{2026}"
                } else {
                    "Rebuild Index"
                };
                let index_color = if indexing {
                    palette.text_muted
                } else {
                    palette.text
                };
                if ui
                    .add_enabled(
                        !indexing,
                        egui::Button::new(egui::RichText::new(index_label).color(index_color))
                            .fill(palette.bg_tertiary)
                            .stroke(egui::Stroke::new(0.5, palette.border))
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(0.0, 24.0)),
                    )
                    .on_hover_text("Compile all .rs files to populate the wiki's name dictionary")
                    .clicked()
                {
                    self.rebuild_index(workspace_root);
                }
                let enabled = self.selected_page().is_some();
                // The painter's disabled alpha dims the whole button, but an
                // accent-colored label would still read as "green = on", so
                // the inactive look swaps to muted text.
                let label_color = if enabled {
                    palette.accent
                } else {
                    palette.text_muted
                };
                let stroke_color = if enabled {
                    palette.border
                } else {
                    palette.border.gamma_multiply(0.6)
                };
                let button = egui::Button::new(
                    egui::RichText::new("Generate Detailed Page").color(label_color),
                )
                .fill(palette.bg_tertiary)
                .stroke(egui::Stroke::new(0.5, stroke_color))
                .corner_radius(egui::CornerRadius::same(6))
                .min_size(egui::vec2(0.0, 24.0));
                if ui
                    .add_enabled(enabled, button)
                    .on_hover_text(
                        "Ask the agent to write a detailed narrative for the selected page",
                    )
                    .clicked()
                {
                    if let Some(prompt) = self.detail_prompt() {
                        action = Some(WikiAction::GenerateDetail(prompt));
                    }
                }
            });
            ui.separator();

            if matches!(self.state, LoadState::Loading) {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Building wiki from site map\u{2026}")
                            .color(palette.text_muted),
                    );
                });
                return;
            }

            if let LoadState::Error(message) = &self.state {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("\u{25c7}")
                            .size(28.0)
                            .color(palette.accent.gamma_multiply(0.7)),
                    );
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(message).color(palette.text_muted));
                });
                return;
            }

            let tree_width = (ui.available_width() * 0.40).clamp(220.0, 400.0);
            let tree_width = tree_width.round() as usize;
            let mut navigate_to: Option<String> = None;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(tree_width as f32);
                    ui.set_min_height(400.0);
                    ui.add_space(4.0);
                    self.render_tree(ui, palette);
                    ui.add_space(4.0);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_height(400.0);
                    navigate_to = self.render_detail(ui, palette);
                });
            });
            if let Some(title) = navigate_to {
                self.select_by_title(&title);
            }
        });

        action
    }

    fn refresh(&mut self, workspace_root: &std::path::Path) {
        // Build off-thread so a large site map never freezes the UI; the tab
        // shows a spinner until `poll_refresh` picks up the finished model.
        let root = workspace_root.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.state = LoadState::Loading;
        std::thread::spawn(move || {
            let result = match crate::automation::open_workspace_site_map(&root) {
                Ok(sm) => Ok(build_wiki(&sm)),
                Err(err) => Err(err),
            };
            let _ = tx.send(result);
        });
    }

    /// Poll the background builder channel, applying a finished model or error.
    fn poll_refresh(&mut self, toasts: &mut ToastQueue) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(model)) => {
                let summary = format!(
                    "Wiki refreshed \u{2014} {} files, {} symbols",
                    model.file_count(),
                    model.symbol_count()
                );
                self.model = Some(model);
                self.selected = Some(PageRef::Overview);
                self.state = LoadState::Ready;
                self.rx = None;
                toasts.push(Toast::success(summary));
            }
            Ok(Err(err)) => {
                self.state = LoadState::Error(format!(
                    "Site map unavailable: {}\nIndex the workspace to populate the wiki.",
                    err
                ));
                self.rx = None;
                toasts.push(Toast::error("Could not open site map"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
            }
        }
    }

    /// Rebuild the workspace index: compile all `.rs` files to populate the
    /// site map's string dictionary, then refresh the wiki. Without this, the
    /// dictionary.json is empty and every entity resolves to a hex hash.
    pub fn rebuild_index(&mut self, workspace_root: &std::path::Path) {
        let root = workspace_root.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        self.index_rx = Some(rx);
        std::thread::spawn(move || {
            let result = (|| -> Result<(usize, String), String> {
                let sitemap_dir = root.join(".velocity").join("site_map");
                let weight_root =
                    velocity_ide::site_map::SiteMap::read_persisted_weight_root(&sitemap_dir)
                        .unwrap_or(0);
                let mut sm = velocity_ide::site_map::SiteMap::open(&sitemap_dir, weight_root)
                    .map_err(|e| e.to_string())?;
                let reports = velocity_ide::compiler::rust_to_nda::RustToNda::compile_directory(
                    &root,
                    &mut sm,
                )
                .map_err(|e| e.to_string())?;
                let count = reports.len();
                // Drop the site map so the dictionary is flushed to disk before
                // the wiki refresh reads it.
                drop(sm);
                Ok((count, root.display().to_string()))
            })();
            let _ = tx.send(result);
        });
    }

    /// Poll the background indexer; on success, trigger a wiki refresh.
    fn poll_index(&mut self, toasts: &mut ToastQueue) {
        let Some(rx) = &self.index_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok((count, _root))) => {
                self.index_rx = None;
                toasts.push(Toast::success(format!(
                    "Indexed {} file(s) \u{2014} refreshing wiki\u{2026}",
                    count
                )));
                // The dictionary is now on disk; rebuild the wiki from it.
                // We need workspace_root — use the model's absence as a signal
                // to refresh on the next ui() call via Idle state.
                self.state = LoadState::Idle;
                self.model = None;
            }
            Ok(Err(err)) => {
                self.index_rx = None;
                toasts.push(Toast::error(format!("Index failed: {}", err)));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.index_rx = None;
            }
        }
    }

    pub fn export(&self, workspace_root: &std::path::Path, toasts: &mut ToastQueue) {
        let Some(model) = &self.model else {
            toasts.push(Toast::warn("Nothing to export yet \u{2014} refresh first"));
            return;
        };
        let dir = workspace_root.join(".wiki");
        match export_markdown(model, &dir) {
            Ok(count) => toasts.push(Toast::success(format!(
                "Exported {} wiki pages to .wiki/",
                count
            ))),
            Err(err) => toasts.push(Toast::error(format!("Wiki export failed: {}", err))),
        }
    }

    fn selected_page(&self) -> Option<&WikiPage> {
        let model = self.model.as_ref()?;
        match self.selected? {
            PageRef::Overview => Some(&model.overview),
            PageRef::File(idx) => model.file_pages.get(idx),
            PageRef::Symbol(idx) => model.symbol_pages.get(idx),
        }
    }

    fn detail_prompt(&self) -> Option<String> {
        let model = self.model.as_ref()?;
        let page = self.selected_page()?;
        let structural = render_page_markdown(page, model);
        Some(format!(
            "You are documenting a codebase. Below is a structural wiki page generated from the project's semantic site map. \
Write a clear, concise \"Details\" section (2-4 short paragraphs) explaining what this {} is responsible for, how it fits into the system, \
and any notable relationships. Use Markdown. Do not repeat the raw lists verbatim.

---

{}",
            page.kind.label(),
            structural
        ))
    }

    fn render_tree(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("Filter pages\u{2026}")
                .desired_width(ui.available_width() - 4.0),
        );
        ui.add_space(4.0);

        let query = self.query.trim().to_lowercase();
        let matches = |title: &str| query.is_empty() || title.to_lowercase().contains(&query);

        let model = match self.model.clone() {
            Some(model) => model,
            None => return,
        };

        egui::ScrollArea::vertical()
            .id_salt("wiki_tree_scroll")
            .show(ui, |ui| {
                if matches(&model.overview.title) {
                    self.tree_row(
                        ui,
                        "\u{25c7}",
                        &model.overview.title,
                        PageRef::Overview,
                        palette,
                    );
                }

                let files: Vec<(usize, String)> = model
                    .file_pages
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| matches(&p.title))
                    .map(|(i, p)| (i, p.title.clone()))
                    .collect();
                if !files.is_empty() {
                    self.tree_section(ui, "FILES", palette);
                    for (idx, title) in files {
                        self.tree_row(ui, "\u{25a4}", &title, PageRef::File(idx), palette);
                    }
                }

                let symbols: Vec<(usize, String)> = model
                    .symbol_pages
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| matches(&p.title))
                    .map(|(i, p)| (i, p.title.clone()))
                    .collect();
                if !symbols.is_empty() {
                    self.tree_section(ui, "SYMBOLS", palette);
                    for (idx, title) in symbols {
                        let label = symbol_display_title(&title);
                        self.tree_row(ui, "\u{0192}", &label, PageRef::Symbol(idx), palette);
                    }
                }

                if model.is_empty() {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "No indexed pages yet \u{2014} index the workspace to build the wiki.",
                        )
                        .color(palette.text_muted),
                    );
                }
            });
    }

    fn tree_section(&self, ui: &mut egui::Ui, label: &str, palette: IdePalette) {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(label)
                .size(10.0)
                .strong()
                .color(palette.text_muted.gamma_multiply(0.8)),
        );
    }

    fn tree_row(
        &mut self,
        ui: &mut egui::Ui,
        glyph: &str,
        title: &str,
        page_ref: PageRef,
        palette: IdePalette,
    ) {
        let selected = self.selected == Some(page_ref);
        let row = egui::Frame::new()
            .fill(if selected {
                palette.accent.gamma_multiply(0.18)
            } else {
                palette.bg_primary
            })
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(glyph).size(11.0).color(if selected {
                        palette.accent
                    } else {
                        palette.text_muted
                    }));
                    ui.label(egui::RichText::new(title).size(12.0).color(if selected {
                        palette.accent
                    } else {
                        palette.text
                    }));
                })
                .response
            })
            .inner
            .interact(egui::Sense::click());
        if row.clicked() {
            self.selected = Some(page_ref);
        }
    }

    fn render_detail(&self, ui: &mut egui::Ui, palette: IdePalette) -> Option<String> {
        let mut navigate: Option<String> = None;
        let Some(page) = self.selected_page().cloned() else {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("\u{25cc}")
                        .size(28.0)
                        .color(palette.accent.gamma_multiply(0.7)),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Select a page to read its wiki entry")
                        .color(palette.text_muted),
                );
            });
            return None;
        };

        egui::ScrollArea::vertical()
            .id_salt("wiki_detail_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&page.title)
                            .size(18.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(page.kind.label())
                            .size(10.0)
                            .color(palette.accent),
                    );
                });
                ui.add_space(2.0);
                ui.label(egui::RichText::new(&page.summary).color(palette.text_muted));
                ui.separator();

                if let Some(detail) = page.detail.as_deref() {
                    ui.label(
                        egui::RichText::new("Details")
                            .size(13.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(detail).color(palette.text));
                    ui.separator();
                }

                for (label, targets) in &page.relationships {
                    if targets.is_empty() {
                        continue;
                    }
                    ui.label(
                        egui::RichText::new(format!("{} ({})", label, targets.len()))
                            .size(13.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.add_space(2.0);
                    for target in targets {
                        if ui
                            .selectable_label(
                                false,
                                egui::RichText::new(format!("\u{203a}  {}", target))
                                    .color(palette.text),
                            )
                            .clicked()
                        {
                            navigate = Some(target.clone());
                        }
                    }
                    ui.add_space(6.0);
                }

                if !page.called_by.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("Called By ({})", page.called_by.len()))
                            .size(13.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.add_space(2.0);
                    for caller in &page.called_by {
                        if ui
                            .selectable_label(
                                false,
                                egui::RichText::new(format!("\u{2039}  {}", caller))
                                    .color(palette.text),
                            )
                            .clicked()
                        {
                            navigate = Some(caller.clone());
                        }
                    }
                }
            });

        navigate
    }

    fn select_by_title(&mut self, title: &str) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        if let Some(idx) = model.file_pages.iter().position(|p| p.title == title) {
            self.selected = Some(PageRef::File(idx));
        } else if let Some(idx) = model.symbol_pages.iter().position(|p| p.title == title) {
            self.selected = Some(PageRef::Symbol(idx));
        }
    }
}

/// Unresolved site-map symbols arrive as bare hex hashes (e.g.
/// "028a2b205607bfec"). Show a compact tagged form so the tree reads as
/// identifiers, not walls of digits.
fn symbol_display_title(title: &str) -> String {
    let is_bare_hex = title.len() >= 8 && title.chars().all(|c| c.is_ascii_hexdigit());
    if !is_bare_hex {
        return title.to_string();
    }
    let mut compact: String = title.chars().take(8).collect();
    if title.chars().count() > 8 {
        compact.push('\u{2026}');
    }
    format!("#{compact}")
}
