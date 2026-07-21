use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Style, Vec2,
    Visuals,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct IdePalette {
    pub bg_primary: Color32,
    pub bg_secondary: Color32,
    pub bg_tertiary: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub border: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
}

impl IdePalette {
    pub fn dark() -> Self {
        Self {
            bg_primary: Color32::from_rgb(8, 9, 14),
            bg_secondary: Color32::from_rgb(17, 18, 26),
            bg_tertiary: Color32::from_rgb(25, 27, 39),
            text: Color32::from_rgb(226, 227, 243),
            text_muted: Color32::from_rgb(125, 131, 166),
            accent: Color32::from_rgb(168, 85, 247),
            border: Color32::from_rgb(33, 36, 51),
            success: Color32::from_rgb(74, 222, 128),
            warning: Color32::from_rgb(250, 204, 21),
            error: Color32::from_rgb(248, 113, 113),
        }
    }

    pub fn light() -> Self {
        Self {
            bg_primary: Color32::from_rgb(250, 250, 252),
            bg_secondary: Color32::from_rgb(241, 241, 245),
            bg_tertiary: Color32::from_rgb(231, 231, 237),
            text: Color32::from_rgb(28, 28, 34),
            text_muted: Color32::from_rgb(100, 100, 115),
            accent: Color32::from_rgb(121, 40, 202),
            border: Color32::from_rgb(215, 215, 225),
            success: Color32::from_rgb(22, 163, 74),
            warning: Color32::from_rgb(202, 138, 4),
            error: Color32::from_rgb(220, 38, 38),
        }
    }
}

pub fn setup_fonts(fonts: &mut FontDefinitions) -> FontId {
    // Try to load a bundled coding font; fall back to system monospace if unavailable.
    let data = include_font();
    if let Some(data) = data {
        fonts
            .font_data
            .insert("code".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "code".into());
    }
    FontId::new(13.0, FontFamily::Monospace)
}

fn include_font() -> Option<Vec<u8>> {
    // Cargo will error if this path is absent, so we only include a placeholder
    // when a known bundled font exists. The function below is conditionally compiled
    // by an environment-driven cfg that defaults to off. In practice we rely on system
    // fonts to avoid missing asset errors.
    None
}

pub fn apply_theme(ctx: &egui::Context, palette: IdePalette) {
    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(palette.text);
    visuals.panel_fill = palette.bg_secondary;
    visuals.window_fill = palette.bg_primary;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.25);
    visuals.selection.stroke.color = palette.text;
    visuals.selection.stroke.width = 1.0;
    visuals.window_stroke.color = palette.border;
    visuals.window_stroke.width = 1.0;
    visuals.hyperlink_color = palette.accent;
    visuals.faint_bg_color = palette.bg_secondary;
    visuals.extreme_bg_color = palette.bg_primary;
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.widgets.noninteractive.bg_fill = palette.bg_tertiary;
    visuals.widgets.noninteractive.fg_stroke.color = palette.text_muted;
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
    visuals.widgets.inactive.bg_fill = palette.bg_secondary;
    visuals.widgets.inactive.fg_stroke.color = palette.text;
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.bg_fill = palette.bg_tertiary;
    visuals.widgets.active.fg_stroke.color = palette.text;
    visuals.widgets.active.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.bg_fill = palette.accent.gamma_multiply(0.15);
    visuals.widgets.hovered.fg_stroke.color = palette.text;
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.open.bg_fill = palette.bg_tertiary;
    visuals.widgets.open.fg_stroke.color = palette.text;
    visuals.widgets.open.corner_radius = CornerRadius::same(6);

    let mut style = Style::default();
    style.visuals = visuals;
    style.spacing.item_spacing = Vec2::splat(6.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.window_margin = egui::Margin::same(10);

    ctx.set_global_style(style);
}

pub fn code_font_id() -> FontId {
    FontId::new(13.0, FontFamily::Monospace)
}

pub fn ui_font_id() -> FontId {
    FontId::new(13.0, FontFamily::Proportional)
}
