//! Custom theme, fonts, and visual tokens for the V.E.L.O.C.I.T.Y. IDE.
use egui::{FontDefinitions, FontFamily, FontId, Stroke, Style, Vec2, Visuals};

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x58, 0xA6, 0xFF);
pub const BG: egui::Color32 = egui::Color32::from_rgb(0x0D, 0x11, 0x17);
pub const BG_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x16, 0x1B, 0x22);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x21, 0x27, 0x30);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xE6, 0xED, 0xF3);
pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x8B, 0x94, 0x9F);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x30, 0x36, 0x3D);
pub const LINE_NUMBER: egui::Color32 = egui::Color32::from_rgb(0x48, 0x4F, 0x58);
pub const LINE_NUMBER_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0xB0, 0xB8, 0xC0);

pub const DEFAULT: egui::Color32 = TEXT;
pub const COMMENT: egui::Color32 = egui::Color32::from_rgb(0x6A, 0x73, 0x7E);
pub const KEYWORD: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x7B, 0x72);
pub const STRING: egui::Color32 = egui::Color32::from_rgb(0xA5, 0xD6, 0xFF);
pub const TYPE: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xD6, 0x6B);
pub const FUNCTION: egui::Color32 = egui::Color32::from_rgb(0xD2, 0xA8, 0xFF);
pub const NUMBER: egui::Color32 = egui::Color32::from_rgb(0x79, 0xC0, 0xFF);
pub const OPERATOR: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xAB, 0x70);
pub const LIFETIME: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xAB, 0x70);
pub const VARIABLE: egui::Color32 = TEXT;
pub const MACRO: egui::Color32 = egui::Color32::from_rgb(0x79, 0xC0, 0xFF);

#[derive(Clone, Copy)]
pub struct IdePalette {
    pub accent: egui::Color32,
    pub bg: egui::Color32,
    pub bg_secondary: egui::Color32,
    pub surface: egui::Color32,
    pub text: egui::Color32,
    pub text_muted: egui::Color32,
    pub border: egui::Color32,
    pub line_number: egui::Color32,
    pub line_number_active: egui::Color32,
}

impl Default for IdePalette {
    fn default() -> Self {
        Self {
            accent: ACCENT,
            bg: BG,
            bg_secondary: BG_SECONDARY,
            surface: SURFACE,
            text: TEXT,
            text_muted: TEXT_MUTED,
            border: BORDER,
            line_number: LINE_NUMBER,
            line_number_active: LINE_NUMBER_ACTIVE,
        }
    }
}

pub fn setup_custom_style(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // Alias our custom family names to the built-in fonts so the code still
    // works if the user has not placed any .ttf files in assets/fonts yet.
    fonts
        .families
        .insert(FontFamily::Name("code".into()), fonts.families[&FontFamily::Monospace].clone());
    fonts
        .families
        .insert(FontFamily::Name("ui".into()), fonts.families[&FontFamily::Proportional].clone());

    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();
    let p = IdePalette::default();
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.noninteractive_bg_fill = p.bg_secondary;
    visuals.extreme_bg_color = p.bg;
    visuals.selection.bg_fill = p.accent.linear_multiply(0.25);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);

    let mut style = Style::default();
    style.visuals = visuals;
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    ctx.set_style(style);
}

pub fn code_font_id(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("code".into()))
}

pub fn ui_font_id(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("ui".into()))
}
