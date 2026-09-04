//! Shared primitives for the Tier-3 IDE panels.
//!
//! Extracted from `tier3_panels.rs` so the section-header helper and the small
//! formatting utilities can be reused by each focused panel module without
//! duplicating them. Behaviour is unchanged.

use eframe::egui;
use egui::RichText;

use super::struct_def::VelocityApp;
use crate::editor::theme::{ITEM_SPACING, SECTION_SPACING};

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
