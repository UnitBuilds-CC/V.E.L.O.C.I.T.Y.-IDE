use crate::agent::{ModelInfo, UiToAgentMessage};
use crate::editor::theme::IdePalette;
use crossbeam_channel::Sender;
use eframe::egui;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Agent,
    Thought,
}

#[derive(Clone, Debug)]
pub struct UiChatMessage {
    pub role: ChatRole,
    pub content: String,
}

pub struct ChatPanelState {
    pub messages: Vec<UiChatMessage>,
    pub input: String,
    pub agent_active: bool,
    pub pending_approvals: Vec<(String, String, serde_json::Value)>,
    pub auto_approve: bool,
    pub available_models: Vec<ModelInfo>,
    pub selected_model: String,
    pub thinking_enabled: bool,
    pub thinking_supported: bool,
    pub tools_supported: bool,
    pub models_loading: bool,
    pub show_thoughts: bool,
}

impl ChatPanelState {
    pub fn push_user(&mut self, text: String) {
        self.messages.push(UiChatMessage {
            role: ChatRole::User,
            content: text,
        });
        self.agent_active = true;
    }

    pub fn append_agent_token(&mut self, token: &str) {
        if self.agent_active {
            self.messages.push(UiChatMessage {
                role: ChatRole::Agent,
                content: String::new(),
            });
            self.agent_active = false;
        }
        if let Some(last) = self.messages.last_mut() {
            if last.role == ChatRole::Agent {
                last.content.push_str(token);
            }
        }
    }

    pub fn append_thought_token(&mut self, token: &str) {
        if let Some(last) = self.messages.last() {
            if last.role == ChatRole::Thought {
                if let Some(last_mut) = self.messages.last_mut() {
                    last_mut.content.push_str(token);
                    return;
                }
            }
        }
        self.messages.push(UiChatMessage {
            role: ChatRole::Thought,
            content: token.to_string(),
        });
    }

    pub fn restore_history(&mut self, entries: Vec<(String, String)>) {
        self.messages.clear();
        for (role, content) in entries {
            let chat_role = match role.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Agent,
                _ => continue,
            };
            if content.trim().is_empty() {
                continue;
            }
            self.messages.push(UiChatMessage { role: chat_role, content });
        }
    }
}

pub fn render_chat_panel(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    agent_tx: &Sender<UiToAgentMessage>,
) {
    let palette = IdePalette::dark();

    egui::Frame::new()
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                render_header(ui, state, palette);
                ui.add_space(4.0);
                render_model_bar(ui, state, agent_tx, palette);
                ui.separator();
                render_messages(ui, state, palette);
                ui.separator();
                render_tool_controls(ui, state, agent_tx, palette);
                render_pending_approvals(ui, state, agent_tx, palette);
                ui.separator();
                render_input(ui, state, agent_tx, palette);
            });
        });
}

fn render_header(ui: &mut egui::Ui, state: &mut ChatPanelState, palette: IdePalette) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Agent Chat")
                .strong()
                .size(16.0)
                .color(palette.text),
        );
        ui.add_space(8.0);

        let (label, color) = if state.agent_active {
            ("● Working", palette.warning)
        } else {
            ("● Ready", palette.success)
        };
        ui.label(egui::RichText::new(label).size(12.0).color(color));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(if state.show_thoughts {
                    "Hide thoughts"
                } else {
                    "Show thoughts"
                })
                .on_hover_text("Toggle reasoning/thought tokens")
                .clicked()
            {
                state.show_thoughts = !state.show_thoughts;
            }
            if ui
                .small_button("Clear")
                .on_hover_text("Clear chat display")
                .clicked()
            {
                state.messages.clear();
            }
        });
    });
}

fn render_model_bar(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    agent_tx: &Sender<UiToAgentMessage>,
    palette: IdePalette,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Model").small().color(palette.text_muted));
        let mut model_changed = false;
        egui::ComboBox::from_id_salt("agent_model")
            .selected_text(truncate_model_label(&state.selected_model, 28))
            .width(260.0)
            .show_ui(ui, |ui| {
                for model in state.available_models.clone() {
                    model_changed |= ui
                        .selectable_value(&mut state.selected_model, model.id.clone(), model.label)
                        .changed();
                }
            });
        if model_changed {
            let _ = agent_tx.send(UiToAgentMessage::SetModel(state.selected_model.clone()));
        }

        if ui
            .button(if state.models_loading {
                "Loading…"
            } else {
                "↻ Models"
            })
            .clicked()
            && !state.models_loading
        {
            state.models_loading = true;
            let _ = agent_tx.send(UiToAgentMessage::RefreshModels);
        }

        let thinking_changed = ui
            .add_enabled(
                state.thinking_supported,
                egui::Checkbox::new(&mut state.thinking_enabled, "Thinking"),
            )
            .changed();
        if thinking_changed {
            let _ = agent_tx.send(UiToAgentMessage::SetThinking(state.thinking_enabled));
        }
        if !state.thinking_supported {
            ui.label(
                egui::RichText::new("unsupported")
                    .small()
                    .color(palette.text_muted),
            );
        }
    });
}

fn render_messages(ui: &mut egui::Ui, state: &ChatPanelState, palette: IdePalette) {
    let scroll_height = ui.available_height() - 160.0;
    egui::ScrollArea::vertical()
        .max_height(scroll_height.max(120.0))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;

            if state.messages.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        egui::RichText::new("Start a conversation with the agent")
                            .color(palette.text_muted)
                            .italics(),
                    );
                    ui.label(
                        egui::RichText::new("Enter to send · Shift+Enter for newline · Ctrl+L to focus")
                            .small()
                            .color(palette.text_muted),
                    );
                });
                return;
            }

            for msg in &state.messages {
                if msg.role == ChatRole::Thought && !state.show_thoughts {
                    continue;
                }
                render_message_bubble(ui, msg, palette);
            }

            if state.agent_active {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.spinner();
                    ui.label(
                        egui::RichText::new("Agent is thinking…")
                            .small()
                            .color(palette.text_muted),
                    );
                });
            }
        });
}

fn render_message_bubble(ui: &mut egui::Ui, msg: &UiChatMessage, palette: IdePalette) {
    let (role_label, bg, border, align, accent) = match msg.role {
        ChatRole::User => (
            "You",
            egui::Color32::from_rgb(45, 25, 78),
            palette.accent,
            egui::Align::RIGHT,
            palette.accent,
        ),
        ChatRole::Agent => (
            "Agent",
            egui::Color32::from_rgb(20, 22, 34),
            palette.border,
            egui::Align::LEFT,
            egui::Color32::from_rgb(34, 211, 238),
        ),
        ChatRole::Thought => (
            "Reasoning",
            egui::Color32::from_rgb(28, 24, 18),
            egui::Color32::from_rgb(120, 90, 40),
            egui::Align::LEFT,
            palette.warning,
        ),
    };

    ui.with_layout(egui::Layout::top_down(align), |ui| {
        ui.label(
            egui::RichText::new(role_label)
                .small()
                .strong()
                .color(accent),
        );
        egui::Frame::new()
            .fill(bg)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() * 0.82);
                let text = msg.content.trim();
                if msg.role == ChatRole::Thought {
                    ui.label(
                        egui::RichText::new(text)
                            .size(12.0)
                            .color(palette.text_muted)
                            .italics(),
                    );
                } else {
                    ui.label(egui::RichText::new(text).size(13.5));
                }
            });
    });
}

fn render_tool_controls(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    _agent_tx: &Sender<UiToAgentMessage>,
    palette: IdePalette,
) {
    ui.horizontal(|ui| {
        // Always allow auto-approve — inline tool calling works for all models
        // regardless of whether the model supports native OpenAI tool_calls.
        ui.checkbox(&mut state.auto_approve, "Auto-approve tools");
        if !state.tools_supported {
            ui.label(
                egui::RichText::new("(inline mode)")
                    .small()
                    .color(palette.text_muted),
            );
        }
    });
}

fn render_pending_approvals(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    agent_tx: &Sender<UiToAgentMessage>,
    palette: IdePalette,
) {
    if state.pending_approvals.is_empty() {
        return;
    }

    egui::Frame::new()
        .fill(egui::Color32::from_rgb(12, 10, 20))
        .stroke(egui::Stroke::new(1.0, palette.warning))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.colored_label(palette.warning, "⚠ Pending Tool Approvals");
            let pending = state.pending_approvals.clone();
            for (id, tool_name, arguments) in pending {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("Tool: {tool_name}"))
                                .strong()
                                .color(egui::Color32::LIGHT_BLUE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Decline").clicked() {
                                let _ = agent_tx.send(UiToAgentMessage::RejectTool {
                                    id: id.clone(),
                                    tool_name: tool_name.clone(),
                                });
                                state
                                    .pending_approvals
                                    .retain(|(p_id, _, _)| p_id != &id);
                            }
                            if ui.button("Approve").clicked() {
                                let _ = agent_tx.send(UiToAgentMessage::ApproveTool {
                                    id: id.clone(),
                                    tool_name: tool_name.clone(),
                                    arguments: arguments.clone(),
                                });
                                state
                                    .pending_approvals
                                    .retain(|(p_id, _, _)| p_id != &id);
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(format!("Arguments: {arguments}"))
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });
            }
        });
}

fn render_input(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    agent_tx: &Sender<UiToAgentMessage>,
    palette: IdePalette,
) {
    ui.horizontal(|ui| {
        let input_width = ui.available_width() - 90.0;
        let response = ui.add(
            egui::TextEdit::multiline(&mut state.input)
                .desired_width(input_width)
                .desired_rows(3)
                .hint_text("Type instructions for the agent… (Enter to send, Shift+Enter for newline)"),
        );

        let enter_send = ui.input(|i| {
            i.key_pressed(egui::Key::Enter) && !i.modifiers.shift && response.has_focus()
        });

        let send_clicked = ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Send")
                        .color(palette.text)
                        .strong(),
                )
                .fill(palette.accent.gamma_multiply(0.35))
                .stroke(egui::Stroke::new(1.0, palette.accent)),
            )
            .clicked();

        if send_clicked || enter_send {
            let text = state.input.trim().to_string();
            if !text.is_empty() {
                state.push_user(text);
                state.input.clear();
                let _ = agent_tx.send(UiToAgentMessage::UserPrompt(
                    state.messages.last().map(|m| m.content.clone()).unwrap_or_default(),
                ));
            }
        }
    });
}

fn truncate_model_label(model: &str, max: usize) -> String {
    if model.len() <= max {
        model.to_string()
    } else {
        format!("…{}", &model[model.len().saturating_sub(max - 1)..])
    }
}
