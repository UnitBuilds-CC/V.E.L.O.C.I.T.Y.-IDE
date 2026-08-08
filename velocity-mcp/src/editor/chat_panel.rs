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
    pub provider: crate::agent::AiProvider,
    /// Files attached to the next chat turn (multimodal input).
    pub attachments: Vec<crate::editor::multimodal::Attachment>,
    /// Path entry for attaching a file to the next turn.
    pub attach_input: String,
}

impl Default for ChatPanelState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            agent_active: false,
            pending_approvals: Vec::new(),
            auto_approve: false,
            available_models: Vec::new(),
            selected_model: "@cf/moonshotai/kimi-k2.7-code".into(),
            thinking_enabled: false,
            thinking_supported: true,
            tools_supported: true,
            models_loading: false,
            show_thoughts: false,
            provider: crate::agent::AiProvider::CloudflareWorkersAi,
            attachments: Vec::new(),
            attach_input: String::new(),
        }
    }
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
        let need_new = self.agent_active
            || self
                .messages
                .last()
                .map(|m| m.role != ChatRole::Agent)
                .unwrap_or(true);

        if need_new {
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

    /// Compose the outgoing prompt for the next turn, folding in any attached
    /// files via the multimodal content assembler, then clear the attachments.
    pub fn compose_and_take_prompt(&mut self, text: &str) -> String {
        if self.attachments.is_empty() {
            return text.to_string();
        }
        let parts = crate::editor::multimodal::assemble_content_parts(
            &self.selected_model,
            text,
            &self.attachments,
        );
        self.attachments.clear();
        self.attach_input.clear();
        flatten_content_parts(&parts)
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
            self.messages.push(UiChatMessage {
                role: chat_role,
                content,
            });
        }
    }
}

pub fn render_chat_panel(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    agent_tx: &Sender<UiToAgentMessage>,
    palette: IdePalette,
) -> bool {
    let mut preferences_changed = false;
    egui::Frame::new()
        .fill(palette.bg_primary)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                preferences_changed |= render_header(ui, state, agent_tx, palette);
                ui.add_space(4.0);
                ui.separator();
                render_messages(ui, state, palette);
                render_pending_approvals(ui, state, agent_tx, palette);
                ui.add_space(6.0);
                render_input(ui, state, agent_tx, palette);
            });
        });
    preferences_changed
}

fn render_header(
    ui: &mut egui::Ui,
    state: &mut ChatPanelState,
    agent_tx: &Sender<UiToAgentMessage>,
    palette: IdePalette,
) -> bool {
    let mut preferences_changed = false;

    // Row 1: title + status indicator
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Chat")
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

        // Right-aligned secondary actions (compact, less prominent)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(egui::RichText::new("Clear").size(10.0))
                .on_hover_text("Clear chat & reset context")
                .clicked()
            {
                state.messages.clear();
                let _ = agent_tx.send(UiToAgentMessage::ClearHistory);
            }
            if state.agent_active
                && ui
                    .small_button(egui::RichText::new("Interrupt").size(10.0))
                    .on_hover_text("Interrupt current agent task")
                    .clicked()
            {
                let _ = agent_tx.send(UiToAgentMessage::CancelTask);
            }
        });
    });

    // Row 2: toggle options (visually separated, less cluttered)
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let thoughts_label = if state.show_thoughts { "Show reasoning: on" } else { "Show reasoning: off" };
        if ui
            .add(egui::Button::new(egui::RichText::new(thoughts_label).size(9.0))
                .frame(false))
            .clicked()
        {
            state.show_thoughts = !state.show_thoughts;
            preferences_changed = true;
        }
    });

    preferences_changed
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
                    ui.add_space(48.0);
                    ui.label(
                        egui::RichText::new("◇")
                            .size(30.0)
                            .color(palette.accent.gamma_multiply(0.7)),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Start a conversation with the agent")
                            .size(14.0)
                            .color(palette.text_muted)
                            .italics(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Enter to send · Shift+Enter for newline · Ctrl+L to focus",
                        )
                        .small()
                        .color(palette.text_muted.gamma_multiply(0.8)),
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

fn sanitize_display_text(s: &str) -> String {
    let mut out = s.to_string();
    let tags = [
        "</tool_call>",
        "<tool_call>",
        "</function>",
        "<function>",
        "</parameter>",
        "<parameter>",
    ];
    for tag in &tags {
        out = out.replace(&format!("{}\r\n", tag), "");
        out = out.replace(&format!("{}\n", tag), "");
        out = out.replace(tag, "");
    }
    let mut result = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            let mut is_tag_structure = false;
            while j < chars.len() && chars[j] != '>' {
                let c = chars[j];
                if c.is_alphabetic()
                    || c == '/'
                    || c == '='
                    || c == '_'
                    || c == '-'
                    || c.is_ascii_digit()
                    || c == '\"'
                    || c == '\''
                    || c == '.'
                {
                    is_tag_structure = true;
                } else {
                    is_tag_structure = false;
                    break;
                }
                j += 1;
            }
            if is_tag_structure && j < chars.len() && chars[j] == '>' {
                i = j + 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn render_markdown(ui: &mut egui::Ui, text: &str, palette: IdePalette) {
    let mut in_code_block = false;
    let mut code_accumulator = String::new();
    let mut code_language = String::new();
    let mut current_list_number = 0;

    for line in text.lines() {
        if line.starts_with("```") {
            current_list_number = 0;
            if in_code_block {
                let mut code = code_accumulator.trim_end().to_string();
                let line_count = code.lines().count();
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(egui::Stroke::new(1.0, palette.border))
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width());
                        // Header row: language label + copy button
                        ui.horizontal(|ui| {
                            if !code_language.is_empty() {
                                ui.label(
                                    egui::RichText::new(&code_language)
                                        .small()
                                        .color(palette.text_muted),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(format!("{} lines", line_count))
                                        .small()
                                        .color(palette.text_muted),
                                );
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("📋 Copy").clicked() {
                                    ui.ctx().copy_text(code.clone());
                                }
                            });
                        });
                        ui.add_space(4.0);
                        // Use syntax-highlighted code editor for blocks with 3+ lines
                        if line_count >= 3 {
                            crate::editor::code_editor::code_block_with_gutter(ui, &mut code);
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut code)
                                        .font(egui::FontId::monospace(12.5))
                                        .code_editor()
                                        .desired_width(f32::INFINITY)
                                        .text_color(palette.text)
                                        .interactive(false),
                                );
                            });
                        }
                    });
                code_accumulator.clear();
                code_language.clear();
                in_code_block = false;
            } else {
                // Extract language from opening fence (e.g. ```rust)
                code_language = line.trim_start_matches('`').trim().to_string();
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_accumulator.push_str(line);
            code_accumulator.push('\n');
            continue;
        }

        if line.starts_with("# ") {
            current_list_number = 0;
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&line[2..])
                    .size(17.0)
                    .strong()
                    .color(palette.accent),
            );
            ui.add_space(4.0);
        } else if line.starts_with("## ") {
            current_list_number = 0;
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(&line[3..])
                    .size(15.5)
                    .strong()
                    .color(palette.text),
            );
            ui.add_space(3.0);
        } else if line.starts_with("### ") {
            current_list_number = 0;
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&line[4..])
                    .size(14.0)
                    .strong()
                    .color(palette.text),
            );
            ui.add_space(2.0);
        } else if line.starts_with("- ") || line.starts_with("* ") {
            current_list_number = 0;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(" • ").color(palette.accent).strong());
                render_inline_markdown(ui, &line[2..], palette);
            });
        } else {
            let mut is_num = false;
            let mut stripped = line;
            if let Some(first_dot) = line.find(". ") {
                let prefix = &line[..first_dot];
                if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                    is_num = true;
                    stripped = &line[first_dot + 2..];
                }
            }
            if is_num {
                current_list_number += 1;
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(" {}. ", current_list_number))
                            .color(palette.accent)
                            .strong(),
                    );
                    render_inline_markdown(ui, stripped, palette);
                });
            } else if !line.trim().is_empty() {
                current_list_number = 0;
                render_inline_markdown(ui, line, palette);
            } else {
                ui.add_space(4.0);
            }
        }
    }
}

fn render_inline_markdown(ui: &mut egui::Ui, text: &str, palette: IdePalette) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        let mut parts = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
                if !current.is_empty() {
                    parts.push((current.clone(), false, false, false));
                    current.clear();
                }
                i += 2;
                let mut bold_text = String::new();
                while i < chars.len()
                    && !(i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*')
                {
                    bold_text.push(chars[i]);
                    i += 1;
                }
                parts.push((bold_text, true, false, false));
                if i + 1 < chars.len() {
                    i += 2;
                } else {
                    i = chars.len();
                }
            } else if chars[i] == '`' {
                if !current.is_empty() {
                    parts.push((current.clone(), false, false, false));
                    current.clear();
                }
                i += 1;
                let mut code_text = String::new();
                while i < chars.len() && chars[i] != '`' {
                    code_text.push(chars[i]);
                    i += 1;
                }
                parts.push((code_text, false, false, true));
                if i < chars.len() {
                    i += 1;
                }
            } else if chars[i] == '*' {
                if !current.is_empty() {
                    parts.push((current.clone(), false, false, false));
                    current.clear();
                }
                i += 1;
                let mut italic_text = String::new();
                while i < chars.len() && chars[i] != '*' {
                    italic_text.push(chars[i]);
                    i += 1;
                }
                parts.push((italic_text, false, true, false));
                if i < chars.len() {
                    i += 1;
                }
            } else {
                current.push(chars[i]);
                i += 1;
            }
        }

        if !current.is_empty() {
            parts.push((current, false, false, false));
        }

        for (content, is_bold, is_italic, is_code) in parts {
            // Comfortable line-height keeps wrapped paragraphs legible.
            let mut rt = egui::RichText::new(content).size(14.0).line_height(Some(21.0));
            if is_bold {
                rt = rt.strong().color(palette.text);
            } else if is_italic {
                rt = rt.italics().color(palette.text);
            } else if is_code {
                rt = rt
                    .monospace()
                    .size(13.0)
                    .color(palette.accent)
                    .background_color(palette.bg_secondary);
            } else {
                rt = rt.color(palette.text);
            }
            ui.label(rt);
        }
    });
}

fn render_message_bubble(ui: &mut egui::Ui, msg: &UiChatMessage, palette: IdePalette) {
    let (role_label, bg, border, align, accent) = match msg.role {
        ChatRole::User => (
            "You",
            palette.accent.gamma_multiply(0.16),
            palette.accent.gamma_multiply(0.45),
            egui::Align::RIGHT,
            palette.accent,
        ),
        ChatRole::Agent => (
            "Agent",
            palette.bg_secondary,
            palette.border.gamma_multiply(0.6),
            egui::Align::LEFT,
            palette.accent,
        ),
        ChatRole::Thought => (
            "Reasoning",
            palette.warning.gamma_multiply(0.10),
            palette.warning.gamma_multiply(0.35),
            egui::Align::LEFT,
            palette.warning,
        ),
    };

    ui.with_layout(egui::Layout::top_down(align), |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(role_label)
                    .small()
                    .strong()
                    .color(accent),
            );
            if ui
                .small_button("Copy")
                .on_hover_text("Copy message text to clipboard")
                .clicked()
            {
                let raw_text = msg.content.trim();
                let text = sanitize_display_text(raw_text);
                ui.ctx().copy_text(text);
            }
        });
        egui::Frame::new()
            .fill(bg)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() * 0.82);
                let raw_text = msg.content.trim();
                let text = sanitize_display_text(raw_text);
                if msg.role == ChatRole::Thought {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&text)
                                .size(12.5)
                                .line_height(Some(19.0))
                                .color(palette.text_muted)
                                .italics(),
                        )
                        .selectable(true),
                    );
                } else {
                    render_markdown(ui, &text, palette);
                }
            });
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
        .fill(palette.bg_primary)
        .stroke(egui::Stroke::new(1.0, palette.warning))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.colored_label(palette.warning, "Pending Tool Approvals");
            let pending = state.pending_approvals.clone();
            for (id, tool_name, arguments) in pending {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("Tool: {tool_name}"))
                                .strong()
                                .color(palette.accent),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Decline").clicked() {
                                let _ = agent_tx.send(UiToAgentMessage::RejectTool {
                                    id: id.clone(),
                                });
                                state.pending_approvals.retain(|(p_id, _, _)| p_id != &id);
                            }
                            if ui.button("Approve").clicked() {
                                let _ = agent_tx.send(UiToAgentMessage::ApproveTool {
                                    id: id.clone(),
                                    arguments: arguments.clone(),
                                });
                                state.pending_approvals.retain(|(p_id, _, _)| p_id != &id);
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
    egui::Frame::new()
        .fill(palette.bg_secondary)
        .stroke(egui::Stroke::new(1.0, palette.border))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut state.input)
                        .desired_width(f32::INFINITY)
                        .desired_rows(2)
                        .hint_text("Message… (Enter to send)"),
                );

                let mut submit = false;
                if response.has_focus() {
                    let (enter_pressed, shift_pressed) = ui.input(|i| (
                        i.key_pressed(egui::Key::Enter),
                        i.modifiers.shift,
                    ));
                    if enter_pressed && !shift_pressed {
                        ui.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                        });
                        submit = true;
                    }
                }

                ui.add_space(6.0);

                // Attachment row (multimodal input).
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.attach_input)
                            .hint_text("attach file path…")
                            .desired_width(ui.available_width() - 64.0),
                    );
                    if ui.small_button("📎 Attach").clicked() {
                        let path = state.attach_input.trim().to_string();
                        if !path.is_empty() {
                            match crate::editor::multimodal::Attachment::load(&path) {
                                Ok(att) => {
                                    state.attachments.push(att);
                                    state.attach_input.clear();
                                }
                                Err(_) => state.attach_input = format!("⚠ cannot read: {path}"),
                            }
                        }
                    }
                });
                if !state.attachments.is_empty() {
                    let mut remove: Option<usize> = None;
                    ui.horizontal_wrapped(|ui| {
                        for (i, att) in state.attachments.iter().enumerate() {
                            let name = att
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file");
                            if ui
                                .small_button(format!("{} {} ✖", att.kind.label(), name))
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        }
                    });
                    if let Some(i) = remove {
                        state.attachments.remove(i);
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.auto_approve, "Auto");
                    ui.add_space(4.0);

                    // Floating Provider selector dropdown
                    let mut provider_changed = false;
                    egui::ComboBox::from_id_salt("floating_agent_provider")
                        .selected_text(state.provider.label())
                        .show_ui(ui, |ui| {
                            for provider in [
                                crate::agent::AiProvider::CloudflareWorkersAi,
                                crate::agent::AiProvider::OpenRouter,
                                crate::agent::AiProvider::OpenAI,
                                crate::agent::AiProvider::Anthropic,
                                crate::agent::AiProvider::GoogleVertex,
                                crate::agent::AiProvider::AzureOpenAi,
                                crate::agent::AiProvider::LocalOllama,
                                crate::agent::AiProvider::Deepseek,
                                crate::agent::AiProvider::AlibabaQwen,
                                crate::agent::AiProvider::Groq,
                                crate::agent::AiProvider::Mistral,
                                crate::agent::AiProvider::TogetherAi,
                                crate::agent::AiProvider::FireworksAi,
                                crate::agent::AiProvider::Perplexity,
                                crate::agent::AiProvider::Cerebras,
                                crate::agent::AiProvider::AwsBedrock,
                            ] {
                                if ui.selectable_value(&mut state.provider, provider, provider.label()).clicked() {
                                    provider_changed = true;
                                }
                            }
                        });
                    if provider_changed {
                        let _ = agent_tx.send(UiToAgentMessage::SetProvider(state.provider));
                        let _ = agent_tx.send(UiToAgentMessage::RefreshModels);
                    }

                    ui.add_space(4.0);

                    // Floating model pill selector
                    let mut model_changed = false;
                    egui::ComboBox::from_id_salt("floating_agent_model")
                        .selected_text(truncate_model_label(&state.selected_model, 22))
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let send_btn = egui::Button::new(egui::RichText::new("➔").strong().color(egui::Color32::WHITE))
                            .corner_radius(egui::CornerRadius::same(12))
                            .fill(palette.accent)
                            .min_size(egui::vec2(28.0, 28.0));

                        if ui.add(send_btn).clicked() || submit {
                            let text = state.input.trim().to_string();
                            state.input.clear();
                            if !text.is_empty() || !state.attachments.is_empty() {
                                let prompt = state.compose_and_take_prompt(&text);
                                state.push_user(text);
                                let _ = agent_tx.send(UiToAgentMessage::UserPrompt(prompt));
                            }
                        }
                    });
                });
            });
        });
}

fn truncate_model_label(model: &str, max: usize) -> String {
    if model.len() <= max {
        model.to_string()
    } else {
        format!("…{}", &model[model.len().saturating_sub(max - 1)..])
    }
}

/// Flatten OpenAI-style content parts (from the multimodal assembler) into a
/// single prompt string: text parts verbatim, image parts as a labeled data
/// URL line so the transport (which carries a plain string) still delivers the
/// image payload.
fn flatten_content_parts(parts: &serde_json::Value) -> String {
    let Some(arr) = parts.as_array() else {
        return parts.as_str().unwrap_or_default().to_string();
    };
    let mut out = String::new();
    for part in arr {
        match part["type"].as_str() {
            Some("text") => {
                if let Some(t) = part["text"].as_str() {
                    out.push_str(t);
                }
            }
            Some("image_url") => {
                if let Some(url) = part["image_url"]["url"].as_str() {
                    out.push_str("\n\n[image] ");
                    out.push_str(url);
                }
            }
            _ => {}
        }
    }
    out
}
