use crate::editor::theme::IdePalette;
use crate::usage::AccountUsageView;
use eframe::egui;

pub fn render_usage_panel(
    ui: &mut egui::Ui,
    accounts: &[AccountUsageView],
    date: &str,
    palette: IdePalette,
    on_refresh: impl FnOnce(),
) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                render_header(ui, accounts, date, on_refresh, palette);
                ui.separator();
                ui.add_space(4.0);

                if accounts.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(30.0);
                        ui.label(
                            egui::RichText::new("No Cloudflare accounts configured")
                                .color(palette.text_muted),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Add CF_ACCOUNT_N_ID and CF_ACCOUNT_N_TOKEN to .env",
                            )
                            .small()
                            .color(palette.text_muted),
                        );
                    });
                    return;
                }

                render_summary_cards(ui, accounts, palette);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                render_account_table(ui, accounts, palette);
            });
        });
}

fn render_header(
    ui: &mut egui::Ui,
    accounts: &[AccountUsageView],
    date: &str,
    on_refresh: impl FnOnce(),
    palette: IdePalette,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Account Usage")
                .strong()
                .size(16.0)
                .color(palette.text),
        );
        ui.label(
            egui::RichText::new(format!("UTC {date}"))
                .small()
                .color(palette.text_muted),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("↻ Refresh")
                .on_hover_text("Reload usage stats")
                .clicked()
            {
                on_refresh();
            }
            let available = accounts.iter().filter(|a| !a.exhausted).count();
            let total = accounts.len();
            let color = if available == 0 {
                palette.error
            } else if available < total {
                palette.warning
            } else {
                palette.success
            };
            ui.label(
                egui::RichText::new(format!("{available}/{total} available"))
                    .strong()
                    .color(color),
            );
        });
    });
}

fn render_summary_cards(ui: &mut egui::Ui, accounts: &[AccountUsageView], palette: IdePalette) {
    let total_requests: u32 = accounts.iter().map(|a| a.requests).sum();
    let total_remaining: u32 = accounts.iter().map(|a| a.remaining).sum();
    let total_limit: u32 = accounts.iter().map(|a| a.daily_limit).sum();
    let total_tokens_in: u64 = accounts.iter().map(|a| a.tokens_in).sum();
    let total_tokens_out: u64 = accounts.iter().map(|a| a.tokens_out).sum();

    ui.horizontal(|ui| {
        summary_card(ui, "Requests today", &total_requests.to_string(), palette);
        summary_card(
            ui,
            "Remaining",
            &format!("{total_remaining} / {total_limit}"),
            palette,
        );
        summary_card(ui, "Tokens in", &format_tokens(total_tokens_in), palette);
        summary_card(ui, "Tokens out", &format_tokens(total_tokens_out), palette);
    });
}

fn summary_card(ui: &mut egui::Ui, label: &str, value: &str, palette: IdePalette) {
    egui::Frame::new()
        .fill(palette.bg_tertiary)
        .stroke(egui::Stroke::new(1.0, palette.border))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(120.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).small().color(palette.text_muted));
                ui.label(egui::RichText::new(value).strong().size(15.0));
            });
        });
    ui.add_space(6.0);
}

fn render_account_table(ui: &mut egui::Ui, accounts: &[AccountUsageView], palette: IdePalette) {
    ui.label(
        egui::RichText::new("Per-account breakdown")
            .strong()
            .color(palette.text),
    );
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("usage_grid")
                .num_columns(6)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Account").strong());
                    ui.label(egui::RichText::new("Tier").strong());
                    ui.label(egui::RichText::new("Requests").strong());
                    ui.label(egui::RichText::new("Remaining").strong());
                    ui.label(egui::RichText::new("Tokens").strong());
                    ui.label(egui::RichText::new("Status").strong());
                    ui.end_row();

                    for acct in accounts {
                        let status_color = if acct.exhausted {
                            palette.error
                        } else if acct.remaining <= acct.daily_limit / 10 {
                            palette.warning
                        } else {
                            palette.success
                        };
                        let status = if acct.exhausted {
                            "Exhausted"
                        } else if acct.remaining == 0 {
                            "At limit"
                        } else {
                            "Active"
                        };

                        ui.label(&acct.label);
                        ui.label(&acct.tier);
                        ui.label(format!("{} / {}", acct.requests, acct.daily_limit));
                        ui.label(acct.remaining.to_string());
                        ui.label(format!(
                            "↓{} ↑{}",
                            format_tokens(acct.tokens_in),
                            format_tokens(acct.tokens_out)
                        ));

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(status).color(status_color).strong());
                        });
                        ui.end_row();

                        ui.horizontal(|ui| {
                            ui.add_space(2.0);
                            let fraction = if acct.daily_limit > 0 {
                                acct.requests as f32 / acct.daily_limit as f32
                            } else {
                                0.0
                            };
                            let bar_color = if acct.exhausted {
                                palette.error
                            } else if fraction > 0.85 {
                                palette.warning
                            } else {
                                palette.accent
                            };
                            let progress = egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                                .fill(bar_color)
                                .animate(acct.exhausted);
                            ui.add(progress);
                        });
                        ui.end_row();
                    }
                });
        });
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn render_usage_compact(
    ui: &mut egui::Ui,
    accounts: &[AccountUsageView],
    palette: IdePalette,
) -> bool {
    let total_remaining: u32 = accounts.iter().map(|a| a.remaining).sum();
    let total_limit: u32 = accounts.iter().map(|a| a.daily_limit).sum();
    let available = accounts.iter().filter(|a| !a.exhausted).count();
    let color = if available == 0 {
        palette.error
    } else if total_remaining < total_limit / 5 {
        palette.warning
    } else {
        palette.success
    };

    ui.label(
        egui::RichText::new(format!(
            "📊 {total_remaining}/{total_limit} req · {available} acct"
        ))
        .size(11.0)
        .color(color)
        .strong(),
    )
    .on_hover_text("Click to open Usage panel")
    .clicked()
}
