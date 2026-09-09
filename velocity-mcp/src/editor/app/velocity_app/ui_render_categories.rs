//! Activity-bar category dispatch rendering for `VelocityApp`.
//!
//! Extracted verbatim from `ui_render.rs` (no logic changes).
use super::struct_def::VelocityApp;
use crate::editor::theme::IdePalette;
use eframe::egui;
use egui_phosphor::regular as ph;

impl VelocityApp {
    // ── Activity Bar Category Panels ──

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
        tabs: &[(&str, &str)],
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, (icon, label)) in tabs.iter().enumerate() {
                let is_selected = self.activity_sub_panel[category] == i;
                let text_color = if is_selected {
                    palette.text
                } else {
                    palette.text_muted
                };
                // Active sub-tab reads stronger; the rest stay muted (UX audit #7).
                let mut text = egui::RichText::new(format!("{icon} {label}"))
                    .size(11.0)
                    .color(text_color);
                if is_selected {
                    text = text.strong();
                }
                let btn = egui::Button::new(text)
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

    pub(super) fn render_files_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::FOLDER, "Files"),
            (ph::BOOKMARK, "Bookmarks"),
            (ph::STAR, "Favorites"),
        ];
        self.render_sub_tabs(ui, palette, 0, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[0]].1);

        match self.activity_sub_panel[0] {
            0 => self.render_file_tree_subpanel(ui, palette),
            1 => self.render_bookmarks_subpanel(ui, palette),
            2 => self.render_favorites_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_search_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::MAGNIFYING_GLASS, "Search"),
            (ph::BRAIN, "Semantic"),
            (ph::TREE_STRUCTURE, "Code Graph"),
        ];
        self.render_sub_tabs(ui, palette, 1, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[1]].1);

        match self.activity_sub_panel[1] {
            0 => self.search_panel(ui),
            1 => self.render_semantic_search_panel(ui),
            2 => self.render_code_graph_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_git_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::FILE_TEXT, "Changes"),
            (ph::GIT_BRANCH, "Branches"),
            (ph::GIT_COMMIT, "Commits"),
        ];
        self.render_sub_tabs(ui, palette, 2, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[2]].1);

        match self.activity_sub_panel[2] {
            0 => self.render_git_changes_subpanel(ui, palette),
            1 => self.render_branches_subpanel(ui, palette),
            2 => self.render_commits_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_chat_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::CHAT_CIRCLE, "Chat"),
            (ph::MICROPHONE, "Voice"),
            (ph::PAPERCLIP, "Multimodal"),
        ];
        self.render_sub_tabs(ui, palette, 3, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[3]].1);

        match self.activity_sub_panel[3] {
            0 => self.render_chat_subpanel(ui, palette),
            1 => self.render_voice_subpanel(ui, palette),
            2 => self.render_multimodal_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_build_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::HAMMER, "Build"),
            (ph::FLASK, "Test"),
            (ph::ROCKET_LAUNCH, "Deploy"),
            (ph::BUG, "Debug"),
            (ph::PLUGS, "LSP"),
        ];
        self.render_sub_tabs(ui, palette, 4, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[4]].1);

        match self.activity_sub_panel[4] {
            0 => self.render_build_subpanel(ui, palette),
            1 => self.render_test_generator_panel(ui),
            2 => self.render_pipeline_panel(ui),
            3 => self.render_debugger_panel(ui),
            4 => self.render_lsp_panel(ui),
            _ => {}
        }
    }

    pub(super) fn render_agents_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::PULSE, "Activity"),
            (ph::USERS, "Roster"),
            (ph::GRAPH, "Orchestration"),
            (ph::BRAIN, "Memory"),
            (ph::CLOCK, "Timeline"),
            (ph::CHART_BAR, "Metrics"),
        ];
        self.render_sub_tabs(ui, palette, 5, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[5]].1);

        match self.activity_sub_panel[5] {
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
        let tabs = [
            (ph::BOOK_OPEN, "Wiki"),
            (ph::DATABASE, "Knowledge Base"),
            (ph::CODE, "Snippets"),
            (ph::LOCK, "NDA"),
        ];
        self.render_sub_tabs(ui, palette, 6, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[6]].1);

        match self.activity_sub_panel[6] {
            0 => self.render_wiki_subpanel(ui, palette),
            1 => self.render_knowledge_panel(ui),
            2 => self.render_snippets_panel(ui),
            3 => self.render_nda_subpanel(ui, palette),
            _ => {}
        }
    }

    pub(super) fn render_workspace_category(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let tabs = [
            (ph::PUZZLE_PIECE, "Extensions"),
            (ph::PLUG, "Plugins"),
            (ph::LIGHTNING, "Skills"),
            (ph::USERS, "Team Studio"),
            (ph::GAUGE, "Usage"),
            (ph::SHIELD, "Governance"),
        ];
        self.render_sub_tabs(ui, palette, 7, &tabs);
        self.render_category_header(ui, palette, tabs[self.activity_sub_panel[7]].1);

        match self.activity_sub_panel[7] {
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
