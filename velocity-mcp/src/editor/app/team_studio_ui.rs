use egui::{Color32, RichText, Sense, Stroke, Vec2};
use crate::agent::AiProvider;
use crate::editor::expert_team::{save_expert_teams, ExpertMember, ExpertTeam};
use super::velocity_app::VelocityApp;

impl VelocityApp {
    pub fn render_team_studio(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui: &mut egui::Ui| {
            ui.add_space(8.0);
            
            // Header Bar
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.heading(RichText::new("Team Studio").strong().color(Color32::from_rgb(130, 180, 255)));
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                    if ui.button(RichText::new("Save").strong()).clicked() {
                        if save_expert_teams(&self.workspace_root, &self.expert_teams) {
                            self.status_message = "Teams saved.".into();
                        } else {
                            self.status_message = "Failed to save teams.".into();
                        }
                    }

                    if ui.button("New Team").clicked() {
                        let new_id = format!("team_custom_{}", self.expert_teams.len() + 1);
                        let new_team = ExpertTeam::new(
                            &new_id,
                            "Custom Expert Team",
                            "User defined multi-agent expert team.",
                            vec![
                                ExpertMember::new(
                                    "member_lead",
                                    "Team Lead",
                                    "Lead Architect",
                                    AiProvider::CloudflareWorkersAi,
                                    "@cf/moonshotai/kimi-k2.7-code",
                                    vec!["system_tools"],
                                    vec!["src/"],
                                    "Lead task decomposition and high-level architecture execution.",
                                ),
                            ],
                            false,
                        );
                        self.expert_teams.push(new_team);
                        self.active_team_index = self.expert_teams.len() - 1;
                        self.selected_member_id = None;
                    }
                });
            });

            ui.separator();

            if self.expert_teams.is_empty() {
                ui.label("No expert teams available.");
                return;
            }

            if self.active_team_index >= self.expert_teams.len() {
                self.active_team_index = 0;
            }

            // Team Selector Tabs / Buttons
            ui.horizontal(|ui: &mut egui::Ui| {
                ui.label(RichText::new("Active Preset Team:").strong());
                for (idx, team) in self.expert_teams.iter().enumerate() {
                    let is_selected = idx == self.active_team_index;
                    let text = if team.is_preset {
                        format!("⭐ {}", team.name)
                    } else {
                        team.name.clone()
                    };

                    let btn = ui.selectable_label(is_selected, text);
                    if btn.clicked() {
                        self.active_team_index = idx;
                        self.selected_member_id = None;
                    }
                }
            });

            ui.add_space(4.0);

            // Active Team Card Editor Header
            let active_team = &mut self.expert_teams[self.active_team_index];
            egui::Frame::group(ui.style())
                .fill(Color32::from_rgb(25, 28, 35))
                .inner_margin(10.0)
                .show(ui, |ui: &mut egui::Ui| {
                    ui.horizontal(|ui: &mut egui::Ui| {
                        ui.label(RichText::new("Team Name:").strong());
                        ui.add(egui::TextEdit::singleline(&mut active_team.name).desired_width(220.0));
                        ui.add_space(15.0);
                        ui.label(RichText::new("Description:").strong());
                        ui.add(egui::TextEdit::singleline(&mut active_team.description).desired_width(450.0));
                    });
                });

            ui.add_space(8.0);

            // Main 2-Column Split: Member List / Flow (Left) vs Comprehensive Member Editor (Right)
            ui.columns(2, |cols| {
                // COLUMN 1: Member Roster & Visual Topology
                cols[0].vertical(|ui: &mut egui::Ui| {
                    ui.horizontal(|ui: &mut egui::Ui| {
                        ui.heading(RichText::new("Team Members & Topology").small().strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                            if ui.button("Add Member").clicked() {
                                let active_team = &mut self.expert_teams[self.active_team_index];
                                let m_id = format!("member_{}", active_team.members.len() + 1);
                                active_team.members.push(ExpertMember::new(
                                    &m_id,
                                    "New Specialist",
                                    "Domain Specialist",
                                    AiProvider::OpenRouter,
                                    "anthropic/claude-3.5-sonnet",
                                    vec!["system_tools"],
                                    vec!["src/"],
                                    "Focus on domain-specific module changes.",
                                ));
                            }
                        });
                    });

                    ui.separator();

                    let active_team = &self.expert_teams[self.active_team_index];
                    let selected_id = self.selected_member_id.clone().unwrap_or_else(|| {
                        active_team.members.first().map(|m| m.id.clone()).unwrap_or_default()
                    });

                    egui::ScrollArea::vertical().id_salt("team_members_scroll").max_height(400.0).show(ui, |ui: &mut egui::Ui| {
                        for (m_idx, member) in active_team.members.iter().enumerate() {
                            let is_selected = member.id == selected_id;
                            let frame_color = if is_selected {
                                Color32::from_rgb(45, 60, 95)
                            } else {
                                Color32::from_rgb(30, 33, 42)
                            };

                            let frame_res = egui::Frame::group(ui.style())
                                .fill(frame_color)
                                .inner_margin(8.0)
                                .show(ui, |ui: &mut egui::Ui| {
                                    ui.horizontal(|ui: &mut egui::Ui| {
                                        ui.label(RichText::new(format!("#{} {}", m_idx + 1, member.name)).strong().color(Color32::WHITE));
                                        ui.label(RichText::new(format!("({})", member.role)).italics().color(Color32::LIGHT_BLUE));
                                    });
                                    ui.horizontal(|ui: &mut egui::Ui| {
                                        ui.label(RichText::new(format!("{}", member.provider.label())).small().color(Color32::GOLD));
                                        ui.label(RichText::new(format!("{}", member.model_id)).small().color(Color32::KHAKI));
                                    });
                                    ui.horizontal(|ui: &mut egui::Ui| {
                                        ui.label(RichText::new("Scopes:").small().strong());
                                        for scope in member.scope_patterns.iter().take(3) {
                                            ui.label(RichText::new(format!("[{}]", scope)).small().color(Color32::LIGHT_GREEN));
                                        }
                                    });
                                });

                            if frame_res.response.interact(Sense::click()).clicked() {
                                self.selected_member_id = Some(member.id.clone());
                            }
                            ui.add_space(4.0);
                        }
                    });

                    ui.add_space(10.0);

                    // Topology Diagram Box
                    ui.label(RichText::new("Delegation Topology").small().strong());
                    egui::Frame::canvas(ui.style())
                        .fill(Color32::from_rgb(18, 20, 26))
                        .inner_margin(10.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            let (rect, _response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 140.0), Sense::hover());
                            let painter = ui.painter_at(rect);

                            let member_count = active_team.members.len().max(1);
                            let step_x = rect.width() / (member_count as f32);

                            for (i, m) in active_team.members.iter().enumerate() {
                                let center_x = rect.min.x + (i as f32 + 0.5) * step_x;
                                let center_y = rect.min.y + 70.0;
                                let node_center = egui::pos2(center_x, center_y);

                                // Connect edges to next member
                                if i + 1 < member_count {
                                    let next_center = egui::pos2(rect.min.x + (i as f32 + 1.5) * step_x, center_y);
                                    painter.line_segment([node_center, next_center], Stroke::new(2.0, Color32::from_rgb(70, 100, 160)));
                                }

                                let node_color = if Some(&m.id) == self.selected_member_id.as_ref() {
                                    Color32::from_rgb(90, 140, 240)
                                } else {
                                    Color32::from_rgb(45, 55, 75)
                                };

                                painter.circle_filled(node_center, 22.0, node_color);
                                painter.circle_stroke(node_center, 22.0, Stroke::new(1.5, Color32::WHITE));
                                painter.text(
                                    node_center,
                                    egui::Align2::CENTER_CENTER,
                                    format!("E{}", i + 1),
                                    egui::FontId::proportional(12.0),
                                    Color32::WHITE,
                                );
                            }
                        });
                });

                // COLUMN 2: Comprehensive Member Editor Menu
                cols[1].vertical(|ui: &mut egui::Ui| {
                    ui.heading(RichText::new("Member Configuration").small().strong());
                    ui.separator();

                    let active_team = &mut self.expert_teams[self.active_team_index];
                    let sel_id = self.selected_member_id.clone().unwrap_or_else(|| {
                        active_team.members.first().map(|m| m.id.clone()).unwrap_or_default()
                    });

                    if let Some(m_idx) = active_team.members.iter().position(|m| m.id == sel_id) {
                        let can_remove = active_team.members.len() > 1;
                        let mut remove_requested = false;
                        let member = &mut active_team.members[m_idx];
                        let member_id_display = member.id.clone();

                        egui::ScrollArea::vertical().id_salt("member_editor_scroll").show(ui, |ui: &mut egui::Ui| {
                            ui.horizontal(|ui: &mut egui::Ui| {
                                ui.label(RichText::new("Member ID:").strong());
                                ui.label(RichText::new(&member_id_display).monospace().color(Color32::GRAY));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                    if can_remove && ui.button(RichText::new("Remove").color(Color32::RED)).clicked() {
                                        remove_requested = true;
                                    }
                                });
                            });

                            ui.add_space(6.0);

                            ui.horizontal(|ui: &mut egui::Ui| {
                                ui.label(RichText::new("Member Name:").strong());
                                ui.add(egui::TextEdit::singleline(&mut member.name).desired_width(180.0));
                                ui.add_space(10.0);
                                ui.label(RichText::new("Role Title:").strong());
                                ui.add(egui::TextEdit::singleline(&mut member.role).desired_width(180.0));
                            });

                            ui.add_space(8.0);

                            // Provider & Model
                            ui.horizontal(|ui: &mut egui::Ui| {
                                ui.label(RichText::new("AI Provider:").strong());
                                egui::ComboBox::from_id_salt(format!("prov_cb_{}", member_id_display))
                                    .selected_text(member.provider.label())
                                    .show_ui(ui, |ui: &mut egui::Ui| {
                                        for p in [
                                            AiProvider::CloudflareWorkersAi,
                                            AiProvider::OpenRouter,
                                            AiProvider::OpenAI,
                                            AiProvider::Anthropic,
                                            AiProvider::GoogleVertex,
                                            AiProvider::AzureOpenAi,
                                            AiProvider::LocalOllama,
                                        ] {
                                            ui.selectable_value(&mut member.provider, p, p.label());
                                        }
                                    });

                                ui.add_space(10.0);
                                ui.label(RichText::new("Model ID:").strong());
                                ui.add(egui::TextEdit::singleline(&mut member.model_id).desired_width(200.0));
                            });

                            ui.add_space(10.0);

                            // Domain Scope Patterns
                            ui.label(RichText::new("Domain Scopes & File Patterns (comma separated):").strong());
                            let mut scopes_str = member.scope_patterns.join(", ");
                            if ui.add(egui::TextEdit::singleline(&mut scopes_str).desired_width(420.0)).changed() {
                                member.scope_patterns = scopes_str
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }

                            ui.add_space(10.0);

                            // Skill Capabilities Checklist
                            ui.label(RichText::new("Assigned Skill Capabilities:").strong());
                            ui.horizontal_wrapped(|ui: &mut egui::Ui| {
                                for skill_name in ["system_tools", "android-cli", "chembl-database", "pymol", "literature-search-arxiv", "quickgo-database"] {
                                    let mut has_skill = member.skills.iter().any(|s| s == skill_name);
                                    if ui.checkbox(&mut has_skill, skill_name).changed() {
                                        if has_skill {
                                            if !member.skills.contains(&skill_name.to_string()) {
                                                member.skills.push(skill_name.to_string());
                                            }
                                        } else {
                                            member.skills.retain(|s| s != skill_name);
                                        }
                                    }
                                }
                            });

                            ui.add_space(10.0);

                            // System Prompt & Workflow Instructions
                            ui.label(RichText::new("System Prompt & Workflow Instructions:").strong());
                            ui.add(
                                egui::TextEdit::multiline(&mut member.workflow_instructions)
                                    .desired_width(450.0)
                                    .desired_rows(6)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });

                        if remove_requested {
                            active_team.members.remove(m_idx);
                            self.selected_member_id = None;
                        }
                    } else {
                        ui.label("Select a member from the left panel to configure.");
                    }
                });
            });
        });
    }
}
