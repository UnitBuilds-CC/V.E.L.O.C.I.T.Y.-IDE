//! Activity-bar category dispatch rendering for `VelocityApp`.
//!
//! Originally extracted verbatim from `ui_render.rs`; since then the per-rail
//! tab lists moved into `app_map::RAILS` so the strip, the app map and the GUI
//! bridge all read one table.
use super::struct_def::VelocityApp;
use crate::editor::app::app_map::{sub_tab, RailSpec, SubTabSpec, RAILS};
use crate::editor::theme::IdePalette;
use eframe::egui;

impl VelocityApp {
    // ── Activity Bar Category Panels ──
    //
    // Every list below comes from `app_map::RAILS`, which is also what builds
    // the app map and what the GUI bridge validates names against. These
    // functions used to carry their own literal arrays, so the strip could (and
    // did) disagree with anything reading the table.

    fn render_category_header(&self, ui: &mut egui::Ui, palette: IdePalette, title: &str) {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(title)
                .strong()
                .size(14.0)
                .color(palette.text),
        );
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
    }

    fn render_sub_tabs(
        &mut self,
        ui: &mut egui::Ui,
        palette: IdePalette,
        category: usize,
        tabs: &[SubTabSpec],
    ) {
        // Wrap rather than overflow: on a narrow sidebar the three icon+label tabs can
        // exceed the panel width, and a non-wrapping `horizontal` would push the last
        // tab past the sidebar edge. Wrapping drops it to a second row so the tab strip
        // always fits the available width (auto-scales with the sidebar).
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, sub) in tabs.iter().enumerate() {
                let is_selected = self.activity_sub_panel[category] == i;
                let text_color = if is_selected {
                    palette.text
                } else {
                    palette.text_muted
                };
                // The selected tab reads stronger via brighter colour + the accent
                // underline painted below — deliberately *not* via font weight.
                // Bold glyphs have wider advances, so bolding only the selected tab
                // resized it and shoved its neighbours sideways every time the
                // selection moved (the "nested tabs shift" bug). Colour + underline
                // carry the same state at zero layout cost (UX audit #7).
                // Build the tab as two text runs so the icon resolves through the
                // dedicated Phosphor family while the label stays on Inter: a single
                // `RichText` can only carry one font, and Inter's colliding PUA glyphs
                // would otherwise render the icon as an accented Latin letter.
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    sub.icon,
                    0.0,
                    egui::TextFormat {
                        font_id: crate::editor::theme::icon_font_id(11.0),
                        color: text_color,
                        ..Default::default()
                    },
                );
                let label_run = format!(" {}", sub.label);
                job.append(
                    &label_run,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(11.0),
                        color: text_color,
                        ..Default::default()
                    },
                );
                let btn = egui::Button::new(job)
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::Vec2::new(0.0, 28.0));
                let resp = ui.add(btn);
                // Accent underline for selected tab
                if is_selected {
                    let rect = resp.rect;
                    let underline = egui::Rect::from_min_size(
                        egui::pos2(rect.min.x + 2.0, rect.max.y - 1.5),
                        egui::vec2(rect.width() - 4.0, 2.0),
                    );
                    ui.painter().rect_filled(
                        underline,
                        egui::CornerRadius::same(1),
                        palette.accent,
                    );
                }
                if resp.clicked() {
                    self.activity_sub_panel[category] = i;
                }
            }
        });
        ui.add_space(2.0);
        ui.separator();
        ui.add_space(4.0);
    }

    /// Draw a rail's tab strip and header, and return the selected index.
    ///
    /// The index is clamped against the rail's own list. Previously the stored
    /// `activity_sub_panel[category]` indexed the local array directly, so a
    /// value left over from a session with more tabs in that rail panicked in
    /// the header instead of falling back to the first tab.
    fn render_rail_tabs(
        &mut self,
        ui: &mut egui::Ui,
        palette: IdePalette,
        rail: &'static RailSpec,
    ) -> usize {
        let category = rail.index();
        self.render_sub_tabs(ui, palette, category, rail.sub_tabs);
        let stored = self.activity_sub_panel[category];
        let selected = if sub_tab(category, stored).is_some() {
            stored
        } else {
            self.activity_sub_panel[category] = 0;
            0
        };
        let label = rail.sub_tabs[selected].label;
        self.render_category_header(ui, palette, label);
        selected
    }

    pub(super) fn render_files_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[0]) {
            0 => self.render_file_tree_subpanel(ui, palette),
            1 => self.render_bookmarks_subpanel(ui, palette),
            2 => self.render_favorites_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_search_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[1]) {
            0 => self.search_panel(ui),
            1 => self.render_semantic_search_panel(ui),
            2 => self.render_code_graph_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_git_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[2]) {
            0 => self.render_git_changes_subpanel(ui, palette),
            1 => self.render_branches_subpanel(ui, palette),
            2 => self.render_commits_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_chat_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[3]) {
            0 => self.render_chat_subpanel(ui, palette),
            1 => self.render_voice_subpanel(ui, palette),
            2 => self.render_multimodal_subpanel(ui, palette),
            // Mounted here rather than on the old `ModeConfig` sidebar tab, which
            // the activity bar replaced and nothing renders any more.
            3 => self.browse_panel(ui),
            _ => {}
        }
    }

    pub(super) fn render_build_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[4]) {
            0 => self.render_build_subpanel(ui, palette),
            1 => self.render_test_generator_panel(ui),
            2 => self.render_pipeline_panel(ui),
            3 => self.render_debugger_panel(ui),
            4 => self.render_lsp_panel(ui),
            _ => {}
        }
    }

    pub(super) fn render_agents_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[5]) {
            0 => self.render_activity_panel(ui),
            1 => self.render_agent_roster_subpanel(ui, palette),
            2 => self.render_live_orchestration_panel(ui),
            3 => self.render_agent_memory_panel(ui),
            4 => self.render_timeline_subpanel(ui, palette),
            5 => self.render_mission_metrics_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_knowledge_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[6]) {
            0 => self.render_wiki_subpanel(ui, palette),
            1 => self.render_knowledge_panel(ui),
            2 => self.render_snippets_panel(ui),
            3 => self.render_nda_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_workspace_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        match self.render_rail_tabs(ui, palette, &RAILS[7]) {
            0 => self.render_extensions_panel(ui),
            1 => self.render_plugin_registry_subpanel(ui, palette),
            2 => self.render_skills_subpanel(ui, palette),
            3 => self.render_team_studio(ui),
            4 => self.render_usage_subpanel(ui, palette),
            5 => self.render_governance_panel(ui),
            _ => {}
        }
    }
}
