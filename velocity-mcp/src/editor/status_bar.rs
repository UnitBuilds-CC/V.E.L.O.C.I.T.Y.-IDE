use crate::editor::theme::IdePalette;
use eframe::egui::{self, Panel, Ui};

/// Actions triggered by clicking status bar elements.
#[derive(Default)]
pub struct StatusBarActions {
    pub clicked_mode: bool,
    pub clicked_build: bool,
    pub clicked_position: bool,
    pub clicked_provider: bool,
    pub clicked_command_palette: bool,
}

pub struct StatusBar;

impl StatusBar {
    pub fn show(
        ui: &mut Ui,
        palette: IdePalette,
        branch: Option<&str>,
        position: Option<(usize, usize)>,
        build_ok: bool,
        status: &str,
        mode: &str,
        provider_label: &str,
        model_label: &str,
    ) -> StatusBarActions {
        let mut actions = StatusBarActions::default();

        Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(egui::Stroke::new(0.0, palette.border)),
            )
            .show(ui, |ui: &mut egui::Ui| {
                // Accent top border — 1px line across the full width
                {
                    let rect = ui.available_rect_before_wrap();
                    let top_line = egui::Rect::from_min_size(
                        egui::pos2(rect.min.x, rect.min.y),
                        egui::vec2(rect.width(), 1.0),
                    );
                    ui.painter()
                        .rect_filled(top_line, 0, palette.accent.gamma_multiply(0.4));
                }

                ui.add_space(3.0);
                ui.horizontal(|ui: &mut egui::Ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    // Mode badge — accent pill
                    {
                        let mode_pill = egui::Frame::new()
                            .fill(palette.accent.gamma_multiply(0.12))
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(6, 1));
                        let mode_response = mode_pill
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(mode)
                                        .size(11.0)
                                        .strong()
                                        .color(palette.accent),
                                )
                            })
                            .inner;
                        if mode_response.clicked() {
                            actions.clicked_mode = true;
                        }
                        if mode_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        mode_response.on_hover_text("Switch workspace mode");
                    }

                    ui.add_space(4.0);

                    // Build indicator — subtle pill
                    {
                        let (icon, color) = if build_ok {
                            (egui_phosphor::regular::CHECK, palette.success)
                        } else {
                            (egui_phosphor::regular::X, palette.error)
                        };
                        let build_bg = if build_ok {
                            palette.success.gamma_multiply(0.10)
                        } else {
                            palette.error.gamma_multiply(0.10)
                        };
                        let build_pill = egui::Frame::new()
                            .fill(build_bg)
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(5, 1));
                        let build_response = build_pill
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{} build", icon))
                                        .color(color)
                                        .size(11.0),
                                )
                            })
                            .inner;
                        if build_response.clicked() {
                            actions.clicked_build = true;
                        }
                        if build_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        build_response.on_hover_text("View diagnostics");
                    }

                    if let Some(b) = branch {
                        ui.add_space(4.0);
                        // Icon and label are separate runs: GIT_BRANCH collides with an
                        // Inter PUA glyph, so it must draw through the Phosphor family
                        // (otherwise it renders as an accented "å").
                        ui.label(
                            egui::RichText::new(egui_phosphor::regular::GIT_BRANCH)
                                .font(crate::editor::theme::icon_font_id(11.0))
                                .color(palette.text_muted),
                        );
                        ui.label(egui::RichText::new(b).size(11.0).color(palette.text_muted));
                    }

                    if let Some((line, col)) = position {
                        ui.add_space(4.0);
                        let pos_response = ui.label(
                            egui::RichText::new(format!("Ln {}, Col {}", line + 1, col + 1))
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                        if pos_response.clicked() {
                            actions.clicked_position = true;
                        }
                        if pos_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        pos_response.on_hover_text("Go to line");
                    }

                    // ── Right group: provider/model + command palette, laid out
                    // right-to-left inside a reserved child that spans only the space
                    // *after* the left group (mode/build/branch/position). Clamping the
                    // child's left edge to the left group's end means a narrow window
                    // clips the right pills instead of overprinting them onto "Code" /
                    // "✓ build" (the footer overlap bug).
                    let row_rect = ui.max_rect();
                    let left_end = ui.cursor().min.x;
                    let mut right = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_max(
                                egui::pos2(left_end, row_rect.min.y),
                                row_rect.max,
                            ))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    right.spacing_mut().item_spacing.x = 4.0;

                    // Provider / model pill. Registry tags like "@cf/" identify
                    // the host, not the model, so they are dropped for display
                    // and the remaining name fits the pill in full. Truly long
                    // names keep their distinguishing tail (char-safe slicing);
                    // the complete id stays one hover away.
                    let model_display = match model_label
                        .strip_prefix('@')
                        .and_then(|rest| rest.split_once('/'))
                    {
                        Some((_tag, tail)) if tail.contains('/') => tail,
                        _ => model_label,
                    };
                    // Count chars without allocating a Vec — char::count is O(n)
                    // but avoids the heap allocation of .collect::<Vec<char>>().
                    let char_count = model_display.chars().count();
                    let model_short = if char_count > 32 {
                        // Take the last 31 chars (char-safe via skip).
                        let skip = char_count - 31;
                        let tail: String = model_display.chars().skip(skip).collect();
                        format!("\u{2026}{tail}")
                    } else {
                        model_display.to_string()
                    };
                    let provider_pill = egui::Frame::new()
                        .fill(palette.bg_tertiary)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(5, 1));
                    let provider_response = provider_pill
                        .show(&mut right, |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} / {}",
                                    provider_label, model_short
                                ))
                                .monospace()
                                .size(10.0)
                                .color(palette.text_muted),
                            )
                        })
                        .inner;
                    if provider_response.clicked() {
                        actions.clicked_provider = true;
                    }
                    if provider_response.hovered() {
                        right.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    provider_response.on_hover_text(format!(
                        "{provider_label} / {model_label}\nClick to open settings"
                    ));

                    // Command palette affordance — a clickable pill so the palette
                    // is discoverable without memorizing the shortcut (UX audit #6).
                    right.add_space(4.0);
                    {
                        let cmd_pill = egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(6, 1));
                        let cmd_response = cmd_pill
                            .show(&mut right, |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}  Ctrl+P",
                                        egui_phosphor::regular::COMMAND
                                    ))
                                    .size(10.0)
                                    .color(palette.text_muted),
                                )
                            })
                            .inner;
                        if cmd_response.clicked() {
                            actions.clicked_command_palette = true;
                        }
                        if cmd_response.hovered() {
                            right.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        cmd_response.on_hover_text("Open command palette");
                    }
                    let right_left_edge = right.cursor().max.x;
                    drop(right);

                    // ── Middle: the free-form status message, clipped to the gap
                    // between the left and right groups so a long message can no longer
                    // run under the mode/build pills (the status-bar overlap bug). ─
                    if !status.is_empty() {
                        let x = ui.cursor().min.x + 8.0;
                        let avail = (right_left_edge - x - 8.0).max(0.0);
                        if avail > 24.0 {
                            // Use a child UI with exact width to contain the label
                            let mut status_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(egui::Rect::from_min_size(
                                        egui::pos2(x, row_rect.min.y),
                                        egui::vec2(avail, row_rect.height()),
                                    ))
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            );
                            let status_response = status_ui.add(
                                egui::Label::new(
                                    egui::RichText::new(status)
                                        .size(11.0)
                                        .color(palette.text_muted),
                                )
                                .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                            // Show full text on hover (UX polish for truncated messages)
                            status_response.on_hover_text(status);
                        }
                    }
                });
                ui.add_space(3.0);
            });

        actions
    }
}
