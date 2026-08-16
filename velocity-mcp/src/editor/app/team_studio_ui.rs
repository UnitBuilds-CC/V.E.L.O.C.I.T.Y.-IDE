use super::velocity_app::VelocityApp;
use crate::editor::expert_team::{load_expert_teams, save_expert_teams};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
    pub fn render_team_studio(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        // Poll the team builder chat for progress updates
        let should_reload = self.team_builder_chat.poll();
        if should_reload {
            self.expert_teams = load_expert_teams(&self.workspace_root);
        }

        ui.vertical(|ui| {
            ui.add_space(6.0);

            // Header
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Teams").strong().color(palette.accent));
                ui.label(RichText::new(format!("{} team(s)", self.expert_teams.len())).small().color(palette.text_muted));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Reload").small()).clicked() {
                        self.expert_teams = load_expert_teams(&self.workspace_root);
                        self.team_gallery_expanded = None;
                    }
                    ui.add_space(6.0);
                    if ui.button(RichText::new("Launch Team").small()).clicked() {
                        if let Some(idx) = self.team_gallery_expanded {
                            let slug = self.expert_teams[idx].slug();
                            self.team_manager.launch_team(&slug);
                            self.toasts.push(crate::editor::toast::Toast::info(format!("Launched team: {}", slug)));
                        } else {
                            self.toasts.push(crate::editor::toast::Toast::warn("Open a team card to launch it"));
                        }
                    }
                    ui.add_space(4.0);
                    if ui.button(RichText::new("Cancel Running").small()).clicked() {
                        self.team_manager.cancel_running();
                        self.toasts.push(crate::editor::toast::Toast::warn("Cancel requested"));
                    }
                });
            });
            ui.separator();

            // Track which member card is selected across frames.
            let selected_member = self.selected_member_id.clone();
            let mut newly_selected: Option<Option<String>> = None;

            // Split: Gallery (top 60%) | Builder Chat (bottom 40%)
            let available_height = ui.available_height();
            let gallery_height = (available_height * 0.58).max(200.0);

            // ═══════════════════════════════════════════════════════════════
            // GALLERY SECTION
            // ═══════════════════════════════════════════════════════════════
            ui.allocate_ui(egui::Vec2::new(ui.available_width(), gallery_height), |ui| {
                egui::ScrollArea::vertical().id_salt("team_gallery_scroll").show(ui, |ui| {
                    let teams_snapshot: Vec<(String, String, usize, bool)> = self.expert_teams.iter()
                        .map(|t| (t.name.clone(), t.description.clone(), t.members.len(), t.is_preset))
                        .collect();

                    for (idx, (name, description, member_count, is_preset)) in teams_snapshot.iter().enumerate() {
                        let is_expanded = self.team_gallery_expanded == Some(idx);

                        // Team Card
                        let card_fill = if is_expanded { palette.bg_tertiary } else { palette.bg_secondary };
                        let card_response = egui::Frame::new()
                            .fill(card_fill)
                            .corner_radius(8.0)
                            .inner_margin(12.0)
                            .stroke(egui::Stroke::new(
                                if is_expanded { 1.5 } else { 0.5 },
                                if is_expanded { palette.accent } else { palette.border },
                            ))
                            .show(ui, |ui| {
                                // Card Header
                                ui.horizontal(|ui| {
                                    if *is_preset {
                                        ui.label(RichText::new("\u{2B50}").size(12.0));
                                    }
                                    ui.label(RichText::new(name).strong().size(13.0).color(palette.text));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{} members", member_count))
                                            .size(9.0).color(palette.text_muted));
                                        if !is_preset
                                            && ui.small_button(RichText::new("\u{2715}").size(9.0).color(palette.error)).clicked() {
                                                self.expert_teams.remove(idx);
                                                let _ = save_expert_teams(&self.workspace_root, &self.expert_teams);
                                                self.team_gallery_expanded = None;
                                                let _ = self.agent_tx.send(crate::agent::UiToAgentMessage::ReloadTeams);
                                            }
                                    });
                                });

                                // Description
                                if !description.is_empty() {
                                    ui.label(RichText::new(description).size(10.0).color(palette.text_muted));
                                }

                                // Expanded: show member cards
                                if is_expanded {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(4.0);

                                    let team = &self.expert_teams[idx];
                                    ui.horizontal_wrapped(|ui| {
                                        for member in &team.members {
                                            let is_selected =
                                                selected_member.as_deref() == Some(member.name.as_str());
                                            let member_card = egui::Frame::new()
                                                .fill(palette.bg_primary)
                                                .corner_radius(6.0)
                                                .inner_margin(8.0)
                                                .stroke(egui::Stroke::new(
                                                    if is_selected { 1.5 } else { 0.5 },
                                                    if is_selected { palette.accent } else { palette.border },
                                                ))
                                                .show(ui, |ui| {
                                                    ui.set_min_width(160.0);
                                                    ui.set_max_width(200.0);

                                                    // Member name + role
                                                    ui.horizontal(|ui| {
                                                        if is_selected {
                                                            ui.label(RichText::new("\u{25B8}").size(9.0).color(palette.accent));
                                                        }
                                                        ui.label(RichText::new(&member.name).strong().size(11.0).color(palette.text));
                                                    });
                                                    ui.label(RichText::new(&member.role).size(9.0).color(palette.accent));

                                                    // Provider + Model pill
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(member.provider.label())
                                                            .size(8.0).color(palette.warning));
                                                    });
                                                    let model_display = if member.model_id.len() > 24 {
                                                        format!("{}...", &member.model_id[..24])
                                                    } else {
                                                        member.model_id.clone()
                                                    };
                                                    ui.label(RichText::new(model_display).monospace().size(8.0).color(palette.text_muted));

                                                    // Skills
                                                    if !member.skills.is_empty() {
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label(RichText::new("Skills:").size(8.0).color(palette.text_muted));
                                                            for skill in member.skills.iter().take(3) {
                                                                ui.label(RichText::new(skill).size(8.0).color(palette.success));
                                                            }
                                                            if member.skills.len() > 3 {
                                                                ui.label(RichText::new(format!("+{}", member.skills.len() - 3)).size(8.0).color(palette.text_muted));
                                                            }
                                                        });
                                                    }

                                                    // Workflow instructions excerpt
                                                    if !member.workflow_instructions.is_empty() {
                                                        let excerpt: String = member.workflow_instructions
                                                            .lines()
                                                            .take(2)
                                                            .collect::<Vec<_>>()
                                                            .join(" ");
                                                        let excerpt = if excerpt.len() > 80 {
                                                            format!("{}...", &excerpt[..80])
                                                        } else {
                                                            excerpt
                                                        };
                                                        ui.label(RichText::new(excerpt).size(8.0).italics().color(palette.text_muted));
                                                    }
                                                });
                                            if member_card.response.interact(egui::Sense::click()).clicked() {
                                                newly_selected = Some(if is_selected {
                                                    None
                                                } else {
                                                    Some(member.name.clone())
                                                });
                                            }
                                        }
                                    });

                                    // Usage hint
                                    ui.add_space(4.0);
                                    let slug = self.expert_teams[idx].slug();
                                    ui.label(RichText::new(format!("Use: @{} <task>  or  \"send it to the {} team\"", slug, name))
                                        .monospace().size(9.0).color(palette.accent));
                                }
                            });

                        // Click to expand/collapse
                        if card_response.response.interact(egui::Sense::click()).clicked() {
                            if is_expanded {
                                self.team_gallery_expanded = None;
                            } else {
                                self.team_gallery_expanded = Some(idx);
                            }
                        }

                        ui.add_space(4.0);
                    }

                    if self.expert_teams.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("\u{25C7}").size(28.0).color(palette.text_muted));
                            ui.label(RichText::new("No teams yet. Describe one below to create it.").color(palette.text_muted));
                        });
                    }
                });
            });

            // ═══════════════════════════════════════════════════════════════
            // TEAM BUILDER CHAT SECTION
            // Small live log panel showing recent TeamManager entries
            ui.add_space(6.0);
            ui.collapsing(RichText::new("Team activity").strong(), |ui| {
                ui.set_min_height(80.0);
                for line in &self.team_manager.logs.iter().rev().take(8).cloned().collect::<Vec<_>>() {
                    ui.label(RichText::new(line).size(9.0).color(palette.text_muted));
                }
            });

            // TEAM BUILDER CHAT SECTION
            // ═══════════════════════════════════════════════════════════════
            if let Some(sel) = newly_selected {
                self.selected_member_id = sel;
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("\u{1F4AC} Team Builder").size(11.0).strong().color(palette.accent));
                if self.team_builder_chat.waiting {
                    ui.label(RichText::new("thinking...").size(9.0).color(palette.warning));
                }
            });
            ui.add_space(2.0);

            // Chat messages area
            let chat_height = ui.available_height() - 34.0; // Reserve space for input
            egui::ScrollArea::vertical()
                .id_salt("team_builder_chat_scroll")
                .max_height(chat_height.max(60.0))
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for msg in &self.team_builder_chat.messages {
                        match msg.role.as_str() {
                            "user" => {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                    egui::Frame::new()
                                        .fill(palette.accent.gamma_multiply(0.15))
                                        .corner_radius(6.0)
                                        .inner_margin(6.0)
                                        .show(ui, |ui| {
                                            ui.set_max_width(ui.available_width() * 0.75);
                                            ui.label(RichText::new(&msg.content).size(10.0).color(palette.text));
                                        });
                                });
                            }
                            "assistant" => {
                                egui::Frame::new()
                                    .fill(palette.bg_secondary)
                                    .corner_radius(6.0)
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.set_max_width(ui.available_width() * 0.85);
                                        ui.label(RichText::new(&msg.content).size(10.0).color(palette.text));
                                    });
                            }
                            "streaming" => {
                                egui::Frame::new()
                                    .fill(palette.bg_tertiary)
                                    .corner_radius(6.0)
                                    .inner_margin(6.0)
                                    .stroke(egui::Stroke::new(0.5, palette.accent))
                                    .show(ui, |ui| {
                                        ui.set_max_width(ui.available_width() * 0.85);
                                        ui.label(RichText::new(&msg.content).size(10.0).color(palette.text));
                                        ui.label(RichText::new("\u{2588}").size(10.0).color(palette.accent));
                                    });
                            }
                            "status" => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("\u{2022} {}", msg.content)).size(9.0).italics().color(palette.text_muted));
                                });
                            }
                            _ => {}
                        }
                        ui.add_space(3.0);
                    }
                });

            // Input bar
            ui.horizontal(|ui| {
                let input_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.team_builder_chat.input)
                        .hint_text("Describe a team to create...")
                        .desired_width(ui.available_width() - 60.0)
                );
                let enter_pressed = input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let send_clicked = ui.add_enabled(
                    !self.team_builder_chat.waiting && !self.team_builder_chat.input.trim().is_empty(),
                    egui::Button::new(RichText::new("Send").size(10.0)),
                ).clicked();

                if (enter_pressed || send_clicked) && !self.team_builder_chat.waiting && !self.team_builder_chat.input.trim().is_empty() {
                    let ws = self.workspace_root.clone();
                    let provider = self.provider;
                    let model = self.selected_model.clone();
                    self.team_builder_chat.send(&ws, provider, &model);
                }
            });
        });
    }
}
