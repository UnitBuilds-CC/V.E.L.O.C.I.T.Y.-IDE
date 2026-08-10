//! V.E.L.O.C.I.T.Y. GUI — egui/eframe application.

use eframe::egui;
use std::sync::mpsc;

use super::api_client::{self, ChatMessage};
use super::config::{AppConfig, Provider, ProviderConfig};

/// Which screen the app is showing.
enum Screen {
    SetupWizard,
    Chat,
}

/// Messages from background thread to UI.
enum BackgroundMsg {
    ChatResponse(Result<String, String>),
}

/// The main application state.
struct VelocityApp {
    config: AppConfig,
    screen: Screen,
    rx: mpsc::Receiver<BackgroundMsg>,

    // Chat state
    messages: Vec<ChatMessage>,
    input_text: String,
    is_loading: bool,

    // Setup wizard state
    wizard_provider: usize,
    wizard_api_key: String,
    wizard_model: String,
    wizard_base_url: String,
    wizard_account_id: String,
    wizard_custom_name: String,
    wizard_error: Option<String>,
}

const PROVIDER_OPTIONS: &[&str] = &["OpenAI", "Anthropic", "Cloudflare Workers AI", "Custom"];

impl VelocityApp {
    fn new(config: AppConfig) -> Self {
        let screen = if config.is_configured() {
            Screen::Chat
        } else {
            Screen::SetupWizard
        };

        let (tx, rx) = mpsc::channel();

        Self {
            config,
            screen,
            rx,
            messages: vec![ChatMessage {
                role: "system".to_string(),
                content: "You are V.E.L.O.C.I.T.Y., a helpful AI coding assistant.".to_string(),
            }],
            input_text: String::new(),
            is_loading: false,
            wizard_provider: 0,
            wizard_api_key: String::new(),
            wizard_model: String::new(),
            wizard_base_url: String::new(),
            wizard_account_id: String::new(),
            wizard_custom_name: String::new(),
            wizard_error: None,
        }
    }

    fn default_model_for_provider(provider_idx: usize) -> &'static str {
        match provider_idx {
            0 => "gpt-4o-mini",
            1 => "claude-sonnet-4-20250514",
            2 => "@cf/moonshotai/kimi-k2.7-code",
            _ => "",
        }
    }

    /// Poll for background messages.
    fn poll_background(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                BackgroundMsg::ChatResponse(result) => {
                    self.is_loading = false;
                    match result {
                        Ok(response) => {
                            self.messages.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: response,
                            });
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: format!("Error: {}", e),
                            });
                        }
                    }
                }
            }
        }
    }

    // ─── Setup Wizard ──────────────────────────────────────────────────

    fn show_setup_wizard(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.heading("Welcome to V.E.L.O.C.I.T.Y.");
                ui.add_space(4.0);
                ui.label("Configure your AI provider to get started.");
                ui.add_space(16.0);
            });

            ui.horizontal(|ui| {
                ui.add_space(40.0);
                self.draw_wizard_form(ui);
            });
        });
    }

    fn draw_wizard_form(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(420.0);

        // Provider selection
        ui.label("AI Provider:");
        ui.add_space(4.0);
        egui::Grid::new("provider_grid")
            .num_columns(2)
            .show(ui, |ui| {
                for (i, name) in PROVIDER_OPTIONS.iter().enumerate() {
                    let selected = self.wizard_provider == i;
                    if ui.selectable_label(selected, *name).clicked() {
                        self.wizard_provider = i;
                        self.wizard_model = Self::default_model_for_provider(i).to_string();
                        if i != 2 {
                            self.wizard_account_id.clear();
                        }
                        if i != 3 {
                            self.wizard_base_url.clear();
                        }
                    }
                    if (i + 1) % 2 == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(12.0);

        // Custom provider fields
        if self.wizard_provider == 3 {
            ui.label("Provider Name:");
            ui.text_edit_singleline(&mut self.wizard_custom_name);
            ui.add_space(8.0);

            ui.label("Base URL:");
            ui.text_edit_singleline(&mut self.wizard_base_url);
            ui.add_space(8.0);
        }

        // API Key
        ui.label("API Key:");
        ui.add(
            egui::TextEdit::singleline(&mut self.wizard_api_key)
                .password(true)
                .hint_text("sk-... or bearer token"),
        );
        ui.add_space(8.0);

        // Model
        ui.label("Model:");
        if self.wizard_model.is_empty() {
            self.wizard_model = Self::default_model_for_provider(self.wizard_provider).to_string();
        }

        if self.wizard_provider < 3 {
            let presets: &[&str] = match self.wizard_provider {
                0 => &["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "o1", "o1-mini"],
                1 => &[
                    "claude-sonnet-4-20250514",
                    "claude-3-5-haiku-20241022",
                    "claude-3-opus-20240229",
                ],
                2 => &[
                    "@cf/moonshotai/kimi-k2.7-code",
                    "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
                ],
                _ => &[],
            };
            egui::ComboBox::from_id_salt("model_combo")
                .selected_text(if presets.contains(&self.wizard_model.as_str()) {
                    self.wizard_model.clone()
                } else {
                    format!("Custom: {}", self.wizard_model)
                })
                .show_ui(ui, |ui| {
                    for model in presets {
                        ui.selectable_value(&mut self.wizard_model, model.to_string(), *model);
                    }
                });
            ui.add_space(4.0);
        }
        ui.label("Model name (or type custom):");
        ui.text_edit_singleline(&mut self.wizard_model);
        ui.add_space(8.0);

        // Cloudflare Account ID
        if self.wizard_provider == 2 {
            ui.label("Cloudflare Account ID:");
            ui.text_edit_singleline(&mut self.wizard_account_id);
            ui.add_space(8.0);
        }

        // Optional base URL override
        if self.wizard_provider != 3 {
            ui.label("Base URL (optional override):");
            ui.text_edit_singleline(&mut self.wizard_base_url);
            ui.add_space(8.0);
        }

        // Error
        if let Some(ref err) = self.wizard_error {
            ui.colored_label(egui::Color32::from_rgb(255, 100, 100), err);
            ui.add_space(8.0);
        }

        ui.add_space(12.0);

        // Buttons
        ui.horizontal(|ui| {
            if ui
                .add_sized([140.0, 32.0], egui::Button::new("Save & Start"))
                .clicked()
            {
                self.save_wizard_config();
            }
            if !self.config.providers.is_empty() {
                if ui
                    .add_sized([80.0, 32.0], egui::Button::new("Cancel"))
                    .clicked()
                {
                    self.screen = Screen::Chat;
                }
            }
        });
    }

    fn save_wizard_config(&mut self) {
        if self.wizard_api_key.trim().is_empty() {
            self.wizard_error = Some("API Key is required.".to_string());
            return;
        }
        if self.wizard_model.trim().is_empty() {
            self.wizard_error = Some("Model name is required.".to_string());
            return;
        }
        if self.wizard_provider == 3 && self.wizard_custom_name.trim().is_empty() {
            self.wizard_error = Some("Provider name is required for custom providers.".to_string());
            return;
        }
        if self.wizard_provider == 3 && self.wizard_base_url.trim().is_empty() {
            self.wizard_error = Some("Base URL is required for custom providers.".to_string());
            return;
        }
        if self.wizard_provider == 2 && self.wizard_account_id.trim().is_empty() {
            self.wizard_error = Some("Account ID is required for Cloudflare.".to_string());
            return;
        }

        let provider = match self.wizard_provider {
            0 => Provider::OpenAI,
            1 => Provider::Anthropic,
            2 => Provider::Cloudflare,
            3 => Provider::Custom(self.wizard_custom_name.trim().to_string()),
            _ => Provider::OpenAI,
        };

        let base_url = if self.wizard_base_url.trim().is_empty() {
            None
        } else {
            Some(self.wizard_base_url.trim().to_string())
        };

        let account_id = if self.wizard_provider == 2 {
            Some(self.wizard_account_id.trim().to_string())
        } else {
            None
        };

        let provider_config = ProviderConfig {
            provider,
            api_key: self.wizard_api_key.trim().to_string(),
            base_url,
            model: self.wizard_model.trim().to_string(),
            account_id,
        };

        if let Some(idx) = self.config.active_provider {
            if idx < self.config.providers.len() {
                self.config.providers[idx] = provider_config;
            } else {
                self.config.providers.push(provider_config);
                self.config.active_provider = Some(self.config.providers.len() - 1);
            }
        } else {
            self.config.providers.push(provider_config);
            self.config.active_provider = Some(self.config.providers.len() - 1);
        }

        if let Err(e) = self.config.save() {
            self.wizard_error = Some(format!("Failed to save config: {}", e));
            return;
        }

        self.wizard_error = None;
        self.screen = Screen::Chat;
    }

    // ─── Chat View ─────────────────────────────────────────────────────

    fn show_chat(&mut self, ctx: &egui::Context) {
        // Top bar
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("V.E.L.O.C.I.T.Y.");
                ui.separator();
                if let Some(cfg) = self.config.active_provider_config() {
                    ui.label(format!("{} — {}", cfg.provider, cfg.model));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Settings").clicked() {
                        if let Some(cfg) = self.config.active_provider_config() {
                            self.wizard_api_key = cfg.api_key.clone();
                            self.wizard_model = cfg.model.clone();
                            self.wizard_base_url = cfg.base_url.clone().unwrap_or_default();
                            self.wizard_account_id = cfg.account_id.clone().unwrap_or_default();
                            self.wizard_provider = match &cfg.provider {
                                Provider::OpenAI => 0,
                                Provider::Anthropic => 1,
                                Provider::Cloudflare => 2,
                                Provider::Custom(name) => {
                                    self.wizard_custom_name = name.clone();
                                    3
                                }
                            };
                        }
                        self.wizard_error = None;
                        self.screen = Screen::SetupWizard;
                    }
                });
            });
        });

        // Input bar
        egui::TopBottomPanel::bottom("input_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let text_edit = ui.add(
                    egui::TextEdit::multiline(&mut self.input_text)
                        .hint_text("Type a message... (Enter to send)")
                        .desired_width(f32::INFINITY)
                        .desired_rows(2),
                );

                let enter_pressed = text_edit.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !ui.input(|i| i.modifiers.shift);

                let send_btn = ui.add_enabled(
                    !self.is_loading && !self.input_text.trim().is_empty(),
                    egui::Button::new("Send").min_size(egui::vec2(60.0, 40.0)),
                );

                if enter_pressed || send_btn.clicked() {
                    self.send_message(ctx);
                }
            });
        });

        // Messages
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for msg in self.messages.iter().skip(1) {
                        self.show_message_bubble(ui, msg);
                    }
                    if self.is_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Thinking...");
                        });
                    }
                });
        });
    }

    fn show_message_bubble(&self, ui: &mut egui::Ui, msg: &ChatMessage) {
        let is_user = msg.role == "user";
        let (bg, fg, label) = if is_user {
            (
                egui::Color32::from_rgb(50, 80, 140),
                egui::Color32::WHITE,
                "You",
            )
        } else {
            (
                egui::Color32::from_rgb(45, 45, 48),
                egui::Color32::from_rgb(220, 220, 220),
                "V.E.L.O.C.I.T.Y.",
            )
        };

        ui.add_space(4.0);
        egui::Frame::default()
            .fill(bg)
            .rounding(6.0)
            .inner_margin(8.0)
            .outer_margin(egui::Margin::symmetric(8.0, 2.0))
            .show(ui, |ui: &mut egui::Ui| {
                ui.label(egui::RichText::new(label).strong().color(fg));
                ui.add_space(2.0);
                ui.label(egui::RichText::new(&msg.content).color(fg));
            });
    }

    fn send_message(&mut self, ctx: &egui::Context) {
        let text = self.input_text.trim().to_string();
        if text.is_empty() || self.is_loading {
            return;
        }

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: text,
        });
        self.input_text.clear();
        self.is_loading = true;

        let config = self.config.clone();
        let messages = self.messages.clone();
        let (tx, rx) = mpsc::channel();

        // Swap receiver
        self.rx = rx;

        std::thread::spawn(move || {
            let result = api_client::chat_completion(&config, &messages);
            let msg = match result {
                Ok(r) => BackgroundMsg::ChatResponse(Ok(r)),
                Err(e) => BackgroundMsg::ChatResponse(Err(format!("{}", e))),
            };
            let _ = tx.send(msg);
        });
    }
}

impl eframe::App for VelocityApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background();

        match self.screen {
            Screen::SetupWizard => self.show_setup_wizard(ctx),
            Screen::Chat => self.show_chat(ctx),
        }

        // Keep repainting while loading
        if self.is_loading {
            ctx.request_repaint();
        }
    }
}

/// Launch the GUI application.
pub fn launch() -> anyhow::Result<()> {
    let config = AppConfig::load();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("V.E.L.O.C.I.T.Y. Cognitive IDE"),
        ..Default::default()
    };

    eframe::run_native(
        "V.E.L.O.C.I.T.Y.",
        options,
        Box::new(move |_cc| Ok(Box::new(VelocityApp::new(config)))),
    )
    .map_err(|e| anyhow::anyhow!("GUI failed: {}", e))
}
