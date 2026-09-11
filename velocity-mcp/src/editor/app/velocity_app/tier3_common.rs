//! Shared primitives for the Tier-3 IDE panels.
//!
//! Extracted from `tier3_panels.rs` so the section-header helper and the small
//! formatting utilities can be reused by each focused panel module without
//! duplicating them. Behaviour is unchanged.

use eframe::egui;
use egui::RichText;

use super::struct_def::VelocityApp;
use crate::editor::theme::{IdePalette, FONT_SMALL, ITEM_SPACING, SECTION_SPACING};

impl VelocityApp {
    // --- Section header shared by the Tier-3 panels ---
    pub(crate) fn tier3_header(
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        accent: egui::Color32,
        muted: egui::Color32,
    ) {
        ui.add_space(SECTION_SPACING);
        ui.horizontal(|ui| {
            ui.heading(RichText::new(title).strong().color(accent));
            ui.label(RichText::new(subtitle).small().color(muted));
        });
        ui.separator();
        ui.add_space(ITEM_SPACING);
    }
}

/// Primary action button: accent fill, on-accent label, and a taller hit target
/// so the single main action in a panel clearly outranks the rest (UX audit #8).
/// Returns the [`egui::Response`] so callers can chain `.clicked()` / hover text.
pub(crate) fn primary_button(
    ui: &mut egui::Ui,
    palette: IdePalette,
    text: impl Into<String>,
) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(text.into())
                .size(FONT_SMALL)
                .strong()
                .color(palette.text_on_accent),
        )
        .fill(palette.accent)
        .corner_radius(egui::CornerRadius::same(6))
        .min_size(egui::vec2(0.0, 28.0)),
    )
}

/// Two-run layout for an icon+label pair. The icon draws through the dedicated
/// `phosphor-icons` family: several Phosphor codepoints (ARROWS_CLOCKWISE,
/// EYE/EYE_SLASH, GIT_BRANCH, …) collide with Inter PUA stylistic alternates in
/// the shared proportional family and render as stray punctuation (a dot, "Ž"),
/// so mixed icon+text strings must not go through it as one run.
pub(crate) fn icon_label_job(
    icon: &str,
    label: &str,
    size: f32,
    color: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        icon,
        0.0,
        egui::TextFormat {
            font_id: crate::editor::theme::icon_font_id(size + 1.0),
            color,
            ..Default::default()
        },
    );
    job.append(
        &format!(" {label}"),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    job
}

/// [`primary_button`] variant taking a precomposed [`egui::text::LayoutJob`]
/// label (see [`icon_label_job`]) so icon+text buttons render correctly.
pub(crate) fn primary_button_job(
    ui: &mut egui::Ui,
    palette: IdePalette,
    job: egui::text::LayoutJob,
) -> egui::Response {
    ui.add(
        egui::Button::new(job)
            .fill(palette.accent)
            .corner_radius(egui::CornerRadius::same(6))
            .min_size(egui::vec2(0.0, 28.0)),
    )
}

/// Secondary action button: neutral surface with a hairline border, visually
/// recessive next to [`primary_button`]. Use for everything that is not the one
/// primary action of a panel.
pub(crate) fn secondary_button(
    ui: &mut egui::Ui,
    palette: IdePalette,
    text: impl Into<String>,
) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(text.into())
                .size(FONT_SMALL)
                .color(palette.text),
        )
        .fill(palette.bg_tertiary)
        .stroke(egui::Stroke::new(0.5, palette.border))
        .corner_radius(egui::CornerRadius::same(6))
        .min_size(egui::vec2(0.0, 24.0)),
    )
}

/// Format a duration in seconds as a compact human string (s/m/h/d).
pub(super) fn human_secs(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// Format a count with K/M suffixes for readability.
pub(super) fn format_count(n: impl Into<u64>) -> String {
    let n = n.into();
    if n < 1_000 {
        format!("{}", n)
    } else if n < 1_000_000 {
        let k = n as f64 / 1_000.0;
        if k < 10.0 {
            format!("{:.1}K", k)
        } else {
            format!("{:.0}K", k)
        }
    } else {
        let m = n as f64 / 1_000_000.0;
        if m < 10.0 {
            format!("{:.1}M", m)
        } else {
            format!("{:.0}M", m)
        }
    }
}
