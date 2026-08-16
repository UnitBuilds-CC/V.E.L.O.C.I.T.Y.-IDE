use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Style, TextStyle,
    Vec2, Visuals,
};
use serde::{Deserialize, Serialize};
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
        // Warm, softened near-black with a faint plum tint for a cozy feel.
        Self {
            bg_primary: Color32::from_rgb(22, 22, 28),
            bg_secondary: Color32::from_rgb(29, 29, 36),
            bg_tertiary: Color32::from_rgb(38, 38, 47),
            text: Color32::from_rgb(228, 226, 234),
            text_muted: Color32::from_rgb(142, 142, 160),
            accent: Color32::from_rgb(183, 156, 255),
            border: Color32::from_rgb(47, 47, 58),
            success: Color32::from_rgb(126, 211, 155),
            warning: Color32::from_rgb(240, 200, 120),
            error: Color32::from_rgb(240, 140, 140),
        }
    }

    pub fn light() -> Self {
        // Warm paper tones instead of cold grey for a softer, cozy daylight look.
        Self {
            bg_primary: Color32::from_rgb(250, 248, 244),
            bg_secondary: Color32::from_rgb(243, 240, 233),
            bg_tertiary: Color32::from_rgb(234, 230, 221),
            text: Color32::from_rgb(48, 44, 40),
            text_muted: Color32::from_rgb(122, 114, 106),
            accent: Color32::from_rgb(140, 92, 204),
            border: Color32::from_rgb(223, 217, 207),
            success: Color32::from_rgb(56, 150, 92),
            warning: Color32::from_rgb(190, 132, 32),
            error: Color32::from_rgb(202, 74, 62),
        }
    }

    pub fn operator() -> Self {
        // Softened teal-on-slate: still focused, but easier on the eyes.
        Self {
            bg_primary: Color32::from_rgb(14, 20, 23),
            bg_secondary: Color32::from_rgb(20, 28, 32),
            bg_tertiary: Color32::from_rgb(28, 39, 44),
            text: Color32::from_rgb(218, 232, 234),
            text_muted: Color32::from_rgb(142, 168, 173),
            accent: Color32::from_rgb(94, 214, 197),
            border: Color32::from_rgb(40, 59, 65),
            success: Color32::from_rgb(126, 211, 155),
            warning: Color32::from_rgb(240, 200, 120),
            error: Color32::from_rgb(240, 140, 140),
        }
    }

    pub fn mission() -> Self {
        // Deep command-deck indigo with a mission-control amber accent —
        // deliberately distinct from Coder's plum Midnight.
        Self {
            bg_primary: Color32::from_rgb(18, 20, 32),
            bg_secondary: Color32::from_rgb(24, 27, 42),
            bg_tertiary: Color32::from_rgb(33, 37, 56),
            text: Color32::from_rgb(224, 227, 240),
            text_muted: Color32::from_rgb(140, 148, 176),
            accent: Color32::from_rgb(255, 179, 71),
            border: Color32::from_rgb(44, 49, 72),
            success: Color32::from_rgb(126, 211, 155),
            warning: Color32::from_rgb(240, 200, 120),
            error: Color32::from_rgb(240, 140, 140),
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            bg_primary: Color32::from_rgb(0, 0, 0),
            bg_secondary: Color32::from_rgb(10, 10, 10),
            bg_tertiary: Color32::from_rgb(24, 24, 24),
            text: Color32::from_rgb(245, 245, 245),
            text_muted: Color32::from_rgb(196, 196, 196),
            accent: Color32::from_rgb(96, 165, 250),
            border: Color32::from_rgb(96, 96, 96),
            success: Color32::from_rgb(74, 222, 128),
            warning: Color32::from_rgb(250, 204, 21),
            error: Color32::from_rgb(248, 113, 113),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeVariant {
    Midnight,
    Daylight,
    Operator,
    Mission,
    HighContrast,
}

impl ThemeVariant {
    pub const ALL: [Self; 5] = [
        Self::Midnight,
        Self::Daylight,
        Self::Operator,
        Self::Mission,
        Self::HighContrast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Daylight => "Daylight",
            Self::Operator => "Operator",
            Self::Mission => "Mission",
            Self::HighContrast => "High Contrast",
        }
    }

    pub fn palette(self) -> IdePalette {
        match self {
            Self::Midnight => IdePalette::dark(),
            Self::Daylight => IdePalette::light(),
            Self::Operator => IdePalette::operator(),
            Self::Mission => IdePalette::mission(),
            Self::HighContrast => IdePalette::high_contrast(),
        }
    }

    pub fn is_dark(self) -> bool {
        !matches!(self, Self::Daylight)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Density {
    Compact,
    Comfortable,
    Spacious,
}

impl Density {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Comfortable, Self::Spacious];

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
            Self::Spacious => "Spacious",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkspaceProfile {
    Coder,
    AutomationOperator,
    MissionControl,
    Accessibility,
}

impl WorkspaceProfile {
    pub const ALL: [Self; 4] = [
        Self::Coder,
        Self::AutomationOperator,
        Self::MissionControl,
        Self::Accessibility,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Coder => "Coder",
            Self::AutomationOperator => "Automation Operator",
            Self::MissionControl => "Mission Control",
            Self::Accessibility => "Accessibility",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Coder => "Code",
            Self::AutomationOperator => "Automate",
            Self::MissionControl => "Mission",
            Self::Accessibility => "Access",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Coder => "Balanced editor-first default for coding and agent review.",
            Self::AutomationOperator => {
                "Higher density and operator colors for browser and desktop automation flows."
            }
            Self::MissionControl => {
                "Readable supervisory preset for monitoring multiple agents and interventions."
            }
            Self::Accessibility => {
                "Higher contrast and larger type for long sessions and lower-vision setups."
            }
        }
    }

    /// Keyboard shortcut that jumps straight to this mode, surfaced in hovers
    /// so the switcher is discoverable without opening the command palette.
    pub fn shortcut_hint(self) -> &'static str {
        match self {
            Self::Coder => "Ctrl+1",
            Self::AutomationOperator => "Ctrl+2",
            Self::MissionControl => "Ctrl+3",
            Self::Accessibility => "Ctrl+4",
        }
    }

    /// Distinct geometric glyph per mode — reinforces the "night and day"
    /// identity in the toolbar pills and the status-bar badge. Restricted to
    /// shapes already known to render in the bundled fonts.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Coder => "\u{25e7}",
            Self::AutomationOperator => "\u{25b6}",
            Self::MissionControl => "\u{25c7}",
            Self::Accessibility => "\u{25cc}",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub profile: WorkspaceProfile,
    pub theme: ThemeVariant,
    pub density: Density,
    pub ui_scale: f32,
    pub code_scale: f32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self::preset(WorkspaceProfile::Coder)
    }
}

impl AppearanceSettings {
    pub fn preset(profile: WorkspaceProfile) -> Self {
        match profile {
            WorkspaceProfile::Coder => Self {
                profile,
                theme: ThemeVariant::Midnight,
                density: Density::Comfortable,
                ui_scale: 1.0,
                code_scale: 1.0,
            },
            WorkspaceProfile::AutomationOperator => Self {
                profile,
                theme: ThemeVariant::Operator,
                density: Density::Compact,
                ui_scale: 0.98,
                code_scale: 0.96,
            },
            WorkspaceProfile::MissionControl => Self {
                profile,
                theme: ThemeVariant::Mission,
                density: Density::Spacious,
                ui_scale: 1.05,
                code_scale: 1.0,
            },
            WorkspaceProfile::Accessibility => Self {
                profile,
                theme: ThemeVariant::HighContrast,
                density: Density::Spacious,
                ui_scale: 1.15,
                code_scale: 1.12,
            },
        }
    }

    pub fn apply_profile(&mut self, profile: WorkspaceProfile) {
        *self = Self::preset(profile);
    }

    pub fn palette(self) -> IdePalette {
        self.theme.palette()
    }

    pub fn ui_font_id(self) -> FontId {
        FontId::new(14.0 * self.ui_scale, FontFamily::Proportional)
    }

    pub fn code_font_id(self) -> FontId {
        FontId::new(14.0 * self.code_scale, FontFamily::Monospace)
    }

    fn item_spacing(self) -> Vec2 {
        match self.density {
            Density::Compact => Vec2::new(6.0, 5.0),
            Density::Comfortable => Vec2::new(8.0, 7.0),
            Density::Spacious => Vec2::new(10.0, 9.0),
        }
    }

    fn button_padding(self) -> Vec2 {
        match self.density {
            Density::Compact => Vec2::new(8.0, 5.0),
            Density::Comfortable => Vec2::new(11.0, 6.0),
            Density::Spacious => Vec2::new(13.0, 8.0),
        }
    }

    fn window_margin(self) -> i8 {
        match self.density {
            Density::Compact => 10,
            Density::Comfortable => 13,
            Density::Spacious => 16,
        }
    }
}

pub fn setup_fonts(fonts: &mut FontDefinitions) -> FontId {
    // Prefer an embedded bundled font when available for consistent rendering.
    if let Some(data) = include_font() {
        fonts
            .font_data
            .insert("code".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "code".into());
        return FontId::new(14.0, FontFamily::Monospace);
    }

    // At runtime, attempt to load common system fonts on Windows so glyph coverage
    // (symbols, emoji, UI glyphs) is available even when no embedded font is shipped.
    #[cfg(target_os = "windows")]
    {
        let candidates: &[(&str, &str)] = &[
            ("consola", r"C:\\Windows\\Fonts\\consola.ttf"),
            ("segoe_ui_symbol", r"C:\\Windows\\Fonts\\seguisym.ttf"),
            ("segoe_ui", r"C:\\Windows\\Fonts\\segoeui.ttf"),
            ("segoe_ui_emoji", r"C:\\Windows\\Fonts\\SegoeUIEmoji.ttf"),
        ];
        for (name, path) in candidates.iter() {
            if std::path::Path::new(path).exists() {
                if let Ok(data) = std::fs::read(path) {
                    // insert under a stable key and prefer it for monospace/proportional families
                    fonts.font_data.insert((*name).to_string(), Arc::new(FontData::from_owned(data)));
                    // Prefer Consolas or the found monospace as the monospace first family entry
                    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, (*name).to_string());
                    // Also add symbol font to proportional family to cover UI glyphs.
                    fonts.families.entry(FontFamily::Proportional).or_default().push((*name).to_string());
                }
            }
        }
    }

    // Default to a slightly larger monospace font id for readability.
    FontId::new(14.0, FontFamily::Monospace)
}

fn include_font() -> Option<Vec<u8>> {
    None
}

pub fn apply_theme(ctx: &egui::Context, appearance: AppearanceSettings) {
    let palette = appearance.palette();
    let mut visuals = if appearance.theme.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    visuals.dark_mode = appearance.theme.is_dark();
    visuals.override_text_color = Some(palette.text);
    visuals.panel_fill = palette.bg_secondary;
    visuals.window_fill = palette.bg_primary;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.22);
    visuals.selection.stroke.color = palette.accent;
    visuals.selection.stroke.width = 1.0;
    visuals.window_stroke.color = palette.border;
    visuals.window_stroke.width = 1.0;
    visuals.hyperlink_color = palette.accent;
    visuals.faint_bg_color = palette.bg_secondary;
    visuals.extreme_bg_color = palette.bg_primary;

    // Rounder corners everywhere for a softer, cozier silhouette.
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    let widget_radius = CornerRadius::same(8);

    // Soft, diffuse shadows instead of hard edges on floating surfaces.
    let shadow_color = Color32::from_black_alpha(if appearance.theme.is_dark() { 90 } else { 40 });
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: shadow_color,
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: shadow_color,
    };

    // Declutter: no indent guide lines in trees, filled slider trails.
    visuals.indent_has_left_vline = false;
    visuals.slider_trailing_fill = true;

    // Noninteractive surfaces (labels, separators) — keep separators subtle.
    visuals.widgets.noninteractive.bg_fill = palette.bg_secondary;
    visuals.widgets.noninteractive.weak_bg_fill = palette.bg_secondary;
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, palette.border.gamma_multiply(0.5));
    visuals.widgets.noninteractive.fg_stroke.color = palette.text_muted;
    visuals.widgets.noninteractive.corner_radius = widget_radius;

    // Inactive: transparent-ish buttons that only reveal fill on hover.
    visuals.widgets.inactive.bg_fill = palette.bg_tertiary;
    visuals.widgets.inactive.weak_bg_fill = palette.bg_secondary;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.fg_stroke.color = palette.text;
    visuals.widgets.inactive.corner_radius = widget_radius;

    // Hover: gentle accent wash.
    visuals.widgets.hovered.bg_fill = palette.accent.gamma_multiply(0.16);
    visuals.widgets.hovered.weak_bg_fill = palette.accent.gamma_multiply(0.12);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, palette.accent.gamma_multiply(0.35));
    visuals.widgets.hovered.fg_stroke.color = palette.text;
    visuals.widgets.hovered.corner_radius = widget_radius;

    // Active/pressed: slightly stronger accent.
    visuals.widgets.active.bg_fill = palette.accent.gamma_multiply(0.24);
    visuals.widgets.active.weak_bg_fill = palette.accent.gamma_multiply(0.20);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, palette.accent.gamma_multiply(0.5));
    visuals.widgets.active.fg_stroke.color = palette.text;
    visuals.widgets.active.corner_radius = widget_radius;

    // Open (combo/menu): match tertiary surface.
    visuals.widgets.open.bg_fill = palette.bg_tertiary;
    visuals.widgets.open.weak_bg_fill = palette.bg_tertiary;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, palette.border);
    visuals.widgets.open.fg_stroke.color = palette.text;
    visuals.widgets.open.corner_radius = widget_radius;

    let mut style = Style::default();
    style.visuals = visuals;
    style.spacing.item_spacing = appearance.item_spacing();
    style.spacing.button_padding = appearance.button_padding();
    style.spacing.window_margin = egui::Margin::same(appearance.window_margin());
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.indent = 16.0;

    // Slim, unobtrusive scrollbars that only widen on hover.
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_inner_margin = 2.0;

    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(18.0 * appearance.ui_scale, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, appearance.ui_font_id());
    style
        .text_styles
        .insert(TextStyle::Button, appearance.ui_font_id());
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0 * appearance.ui_scale, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Monospace, appearance.code_font_id());

    ctx.set_global_style(style);
}

#[allow(dead_code)]
pub fn code_font_id() -> FontId {
    AppearanceSettings::default().code_font_id()
}

#[allow(dead_code)]
pub fn ui_font_id() -> FontId {
    AppearanceSettings::default().ui_font_id()
}
