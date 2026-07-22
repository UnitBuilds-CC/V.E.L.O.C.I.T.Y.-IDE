use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Style,
    TextStyle, Vec2, Visuals,
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

    pub fn operator() -> Self {
        Self {
            bg_primary: Color32::from_rgb(6, 12, 16),
            bg_secondary: Color32::from_rgb(12, 22, 28),
            bg_tertiary: Color32::from_rgb(17, 32, 41),
            text: Color32::from_rgb(223, 238, 242),
            text_muted: Color32::from_rgb(136, 168, 176),
            accent: Color32::from_rgb(45, 212, 191),
            border: Color32::from_rgb(29, 55, 65),
            success: Color32::from_rgb(74, 222, 128),
            warning: Color32::from_rgb(251, 191, 36),
            error: Color32::from_rgb(248, 113, 113),
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
    HighContrast,
}

impl ThemeVariant {
    pub const ALL: [Self; 4] = [
        Self::Midnight,
        Self::Daylight,
        Self::Operator,
        Self::HighContrast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Daylight => "Daylight",
            Self::Operator => "Operator",
            Self::HighContrast => "High Contrast",
        }
    }

    pub fn palette(self) -> IdePalette {
        match self {
            Self::Midnight => IdePalette::dark(),
            Self::Daylight => IdePalette::light(),
            Self::Operator => IdePalette::operator(),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn focus_label(self) -> &'static str {
        match self {
            Self::Coder => "Editing, diff review, and fast agent iteration",
            Self::AutomationOperator => "Live automation runs, evidence, and intervention",
            Self::MissionControl => "Fleet health, blockers, and approvals",
            Self::Accessibility => "Readable control surfaces with less visual strain",
        }
    }

    pub fn quick_tip(self) -> &'static str {
        match self {
            Self::Coder => "Keep editor, chat, search, and output close together so code and feedback stay in one loop.",
            Self::AutomationOperator => "Put runtime state ahead of raw logs so you can tell whether a run is healthy before reading details.",
            Self::MissionControl => "Start with the exception queue: blocked work, approvals, and failing tasks should surface before everything else.",
            Self::Accessibility => "Favor fewer competing panels, larger defaults, and one obvious action path per task.",
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
                theme: ThemeVariant::Midnight,
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
        FontId::new(13.0 * self.ui_scale, FontFamily::Proportional)
    }

    pub fn code_font_id(self) -> FontId {
        FontId::new(13.0 * self.code_scale, FontFamily::Monospace)
    }

    fn item_spacing(self) -> Vec2 {
        match self.density {
            Density::Compact => Vec2::new(4.0, 4.0),
            Density::Comfortable => Vec2::new(6.0, 6.0),
            Density::Spacious => Vec2::new(8.0, 8.0),
        }
    }

    fn button_padding(self) -> Vec2 {
        match self.density {
            Density::Compact => Vec2::new(6.0, 3.0),
            Density::Comfortable => Vec2::new(8.0, 4.0),
            Density::Spacious => Vec2::new(10.0, 5.0),
        }
    }

    fn window_margin(self) -> i8 {
        match self.density {
            Density::Compact => 8,
            Density::Comfortable => 10,
            Density::Spacious => 12,
        }
    }
}

pub fn setup_fonts(fonts: &mut FontDefinitions) -> FontId {
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
    style.spacing.item_spacing = appearance.item_spacing();
    style.spacing.button_padding = appearance.button_padding();
    style.spacing.window_margin = egui::Margin::same(appearance.window_margin());
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(18.0 * appearance.ui_scale, FontFamily::Proportional),
    );
    style.text_styles.insert(TextStyle::Body, appearance.ui_font_id());
    style.text_styles.insert(TextStyle::Button, appearance.ui_font_id());
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
