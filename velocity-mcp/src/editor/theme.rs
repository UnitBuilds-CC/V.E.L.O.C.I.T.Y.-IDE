use egui::{
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
    pub text_disabled: Color32,
    pub accent: Color32,
    pub info: Color32,
    pub border: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub surface_hover: Color32,
    pub text_on_accent: Color32,
}

/// Consistent corner radius for sub-panel cards across the IDE.
pub const CARD_RADIUS: f32 = 6.0;
/// Consistent inner padding for sub-panel cards.
pub const CARD_INNER_MARGIN: f32 = 8.0;
/// Spacing between cards and major sections.
pub const SECTION_SPACING: f32 = 8.0;
/// Small spacing for intra-card elements.
pub const ITEM_SPACING: f32 = 4.0;

/// Semantic font sizes for the IDE UI. Use these instead of inline `.size()`.
pub const FONT_CAPTION: f32 = 10.0;
pub const FONT_SMALL: f32 = 11.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_HEADING: f32 = 15.0;

impl IdePalette {
    pub fn dark() -> Self {
        // Modern AI-first IDE: deep neutral backgrounds with vivid green accent.
        Self {
            bg_primary: Color32::from_rgb(15, 15, 18),
            bg_secondary: Color32::from_rgb(22, 22, 26),
            bg_tertiary: Color32::from_rgb(32, 32, 38),
            text: Color32::from_rgb(235, 235, 240),
            text_muted: Color32::from_rgb(130, 130, 148),
            text_disabled: Color32::from_rgb(80, 80, 95),
            accent: Color32::from_rgb(34, 197, 94),
            info: Color32::from_rgb(59, 130, 246),
            border: Color32::from_rgb(42, 42, 50),
            success: Color32::from_rgb(34, 197, 94),
            warning: Color32::from_rgb(234, 179, 8),
            error: Color32::from_rgb(239, 68, 68),
            surface_hover: Color32::from_rgb(34, 197, 94).gamma_multiply(0.12),
            text_on_accent: Color32::WHITE,
        }
    }

    pub fn light() -> Self {
        // Clean modern light theme with blue accent and neutral warm greys.
        Self {
            bg_primary: Color32::from_rgb(255, 255, 255),
            bg_secondary: Color32::from_rgb(248, 248, 250),
            bg_tertiary: Color32::from_rgb(237, 237, 242),
            text: Color32::from_rgb(32, 32, 38),
            // Darkened from (115,115,130) to clear WCAG AA (~5.1:1) for muted body
            // text on the panel fill (bg_secondary), not just the white bg_primary.
            text_muted: Color32::from_rgb(105, 105, 120),
            text_disabled: Color32::from_rgb(180, 180, 190),
            accent: Color32::from_rgb(59, 130, 246),
            info: Color32::from_rgb(59, 130, 246),
            border: Color32::from_rgb(220, 220, 228),
            success: Color32::from_rgb(22, 163, 74),
            warning: Color32::from_rgb(146, 100, 0),
            error: Color32::from_rgb(210, 55, 45),
            surface_hover: Color32::from_rgb(59, 130, 246).gamma_multiply(0.08),
            text_on_accent: Color32::WHITE,
        }
    }

    pub fn operator() -> Self {
        // Cyberpunk terminal: deep teal-black with vivid cyan accent.
        Self {
            bg_primary: Color32::from_rgb(10, 16, 20),
            bg_secondary: Color32::from_rgb(16, 24, 28),
            bg_tertiary: Color32::from_rgb(24, 36, 42),
            text: Color32::from_rgb(220, 236, 240),
            text_muted: Color32::from_rgb(120, 152, 162),
            text_disabled: Color32::from_rgb(70, 90, 100),
            accent: Color32::from_rgb(6, 182, 212),
            info: Color32::from_rgb(56, 189, 248),
            border: Color32::from_rgb(35, 52, 60),
            success: Color32::from_rgb(34, 197, 94),
            warning: Color32::from_rgb(234, 179, 8),
            error: Color32::from_rgb(239, 68, 68),
            surface_hover: Color32::from_rgb(6, 182, 212).gamma_multiply(0.12),
            text_on_accent: Color32::WHITE,
        }
    }

    pub fn mission() -> Self {
        // Command-deck indigo with warm amber accent for mission control.
        // Accent is indigo (distinct from amber warning) for clear visual separation.
        Self {
            bg_primary: Color32::from_rgb(14, 16, 28),
            bg_secondary: Color32::from_rgb(20, 23, 38),
            bg_tertiary: Color32::from_rgb(28, 32, 50),
            text: Color32::from_rgb(228, 230, 245),
            text_muted: Color32::from_rgb(128, 136, 168),
            text_disabled: Color32::from_rgb(75, 80, 105),
            accent: Color32::from_rgb(99, 102, 241),
            info: Color32::from_rgb(56, 189, 248),
            border: Color32::from_rgb(40, 44, 66),
            success: Color32::from_rgb(34, 197, 94),
            warning: Color32::from_rgb(245, 158, 11),
            error: Color32::from_rgb(239, 68, 68),
            surface_hover: Color32::from_rgb(99, 102, 241).gamma_multiply(0.12),
            text_on_accent: Color32::WHITE,
        }
    }

    pub fn high_contrast() -> Self {
        // Maximum contrast: pure black background with vivid accent colors.
        Self {
            bg_primary: Color32::from_rgb(0, 0, 0),
            bg_secondary: Color32::from_rgb(8, 8, 8),
            bg_tertiary: Color32::from_rgb(20, 20, 20),
            text: Color32::from_rgb(255, 255, 255),
            text_muted: Color32::from_rgb(180, 180, 180),
            text_disabled: Color32::from_rgb(100, 100, 100),
            accent: Color32::from_rgb(56, 189, 248),
            info: Color32::from_rgb(56, 189, 248),
            border: Color32::from_rgb(100, 100, 100),
            success: Color32::from_rgb(74, 222, 128),
            warning: Color32::from_rgb(250, 204, 21),
            error: Color32::from_rgb(248, 113, 113),
            surface_hover: Color32::from_rgb(56, 189, 248).gamma_multiply(0.15),
            text_on_accent: Color32::BLACK,
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
            // Product-level terms describe the job a person is doing; the
            // underlying profile variants remain stable for saved preferences.
            Self::Coder => "Build",
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

    /// Distinct Phosphor icon per mode — reinforces the "night and day"
    /// identity in the toolbar pills and the status-bar badge. Rendered via the
    /// embedded Phosphor icon font registered in `setup_fonts`.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Coder => egui_phosphor::regular::CODE,
            Self::AutomationOperator => egui_phosphor::regular::ROBOT,
            Self::MissionControl => egui_phosphor::regular::GAUGE,
            Self::Accessibility => egui_phosphor::regular::EYE,
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
    /// Fold the OS light/dark preference into [`Self::resolved`]. A workspace
    /// that has never stored a preference opts in (so a light-mode desktop does
    /// not open into Midnight); choosing a theme by hand opts back out.
    #[serde(default)]
    pub follow_system_theme: bool,
    /// `theme` is a choice the person made rather than the mode preset's
    /// signature palette. Work modes are documented to bring their own colors
    /// ("operator colors", "higher contrast"), so a preset may only recolour a
    /// theme nobody picked — otherwise switching modes silently throws away a
    /// light theme and persists the loss.
    #[serde(default)]
    pub theme_chosen: bool,
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
                // Match Accessibility scale for consistent readability across panels
                ui_scale: 1.15,
                code_scale: 1.12,
                follow_system_theme: false,
                theme_chosen: false,
            },
            WorkspaceProfile::AutomationOperator => Self {
                profile,
                theme: ThemeVariant::Operator,
                density: Density::Compact,
                // Larger UI for better legibility like Accessibility preset
                ui_scale: 1.15,
                code_scale: 1.12,
                follow_system_theme: false,
                theme_chosen: false,
            },
            WorkspaceProfile::MissionControl => Self {
                profile,
                theme: ThemeVariant::Mission,
                density: Density::Spacious,
                // Match Accessibility preset scale for consistency
                ui_scale: 1.15,
                code_scale: 1.12,
                follow_system_theme: false,
                theme_chosen: false,
            },
            WorkspaceProfile::Accessibility => Self {
                profile,
                theme: ThemeVariant::HighContrast,
                density: Density::Spacious,
                ui_scale: 1.15,
                code_scale: 1.12,
                follow_system_theme: false,
                theme_chosen: false,
            },
        }
    }

    /// Apply a work mode's density, scale and palette.
    ///
    /// A palette nobody chose still follows the mode, because the mode
    /// descriptions promise its colors. A palette the user picked does not: it
    /// used to be reassigned wholesale here, so switching modes threw away e.g.
    /// a hand-picked Daylight *and persisted that overwrite*, which is what made
    /// light themes look like they "don't apply".
    ///
    /// Switching modes also stops following the desktop. The modes are sold by
    /// their palettes — Accessibility *is* higher contrast — so a followed system
    /// preference would quietly keep painting that mode's own promise away. The
    /// rule throughout is that the last deliberate appearance action wins.
    pub fn apply_profile(&mut self, profile: WorkspaceProfile) {
        let preset = Self::preset(profile);
        self.profile = profile;
        self.density = preset.density;
        self.ui_scale = preset.ui_scale;
        self.code_scale = preset.code_scale;
        self.follow_system_theme = false;
        if !self.theme_chosen {
            self.theme = preset.theme;
        }
    }

    /// Throw away every choice for the current mode and take its full preset,
    /// signature theme included. This is the one place a saved theme is allowed
    /// to be replaced, because the user asked for a reset.
    pub fn reset_to_profile_preset(&mut self) {
        let profile = self.profile;
        *self = Self::preset(profile);
    }

    /// Choose a palette by hand. An explicit choice outranks both the mode
    /// preset and the system preference from here on.
    pub fn set_theme(&mut self, theme: ThemeVariant) {
        self.theme = theme;
        self.theme_chosen = true;
        self.follow_system_theme = false;
    }

    /// The appearance to paint with, given the OS light/dark preference. Only
    /// `theme` can change here, and only while following the system with a
    /// preference actually read — an unreadable preference keeps the saved
    /// theme rather than guessing at a palette.
    pub fn resolved(self, os_prefers_dark: Option<bool>) -> Self {
        if !self.follow_system_theme {
            return self;
        }
        let mut resolved = self;
        if let Some(prefers_dark) = os_prefers_dark {
            resolved.theme = if prefers_dark {
                ThemeVariant::Midnight
            } else {
                ThemeVariant::Daylight
            };
        }
        resolved
    }

    pub fn palette(self) -> IdePalette {
        self.theme.palette()
    }

    pub fn ui_font_id(self) -> FontId {
        // Base size for default-styled widgets (text inputs, buttons, chips). Kept
        // compact so these match the hand-sized 9-14px sidebar labels instead of
        // towering over them; an 18.4px base made controls ~2x the surrounding text
        // and wide enough to clip at the sidebar edge.
        FontId::new(13.0 * self.ui_scale, FontFamily::Proportional)
    }

    pub fn code_font_id(self) -> FontId {
        // Increase base code font size by 2pt; prefer monospace bold if available at runtime.
        FontId::new(16.0 * self.code_scale, FontFamily::Monospace)
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
    // Embed the bundled Inter (UI) and JetBrains Mono (code) faces at compile time
    // for deterministic, cross-platform rendering. `include_bytes!` resolves relative
    // to this file (velocity-mcp/src/editor), so `../../../assets` is the repo-root
    // assets directory. egui's default emoji/symbol fonts stay as trailing fallbacks.
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/static/Inter_18pt-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jbmono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/fonts/ttf/JetBrainsMono-Regular.ttf"
        ))),
    );

    // Inter is the primary proportional (UI) face; JetBrains Mono the primary
    // monospace (code) face. Inserted at index 0 so they win over egui defaults.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jbmono".into());

    // Phosphor icons: registered as a proportional fallback so icon codepoints
    // (egui_phosphor::regular::*) can render inline with text. It MUST stay *behind*
    // Inter in the shared Proportional chain: egui picks the first family font that
    // has a glyph for each character, and Phosphor claims the lowercase ASCII
    // codepoints (mapping them to blank glyphs), so hoisting it to the front made
    // every lowercase letter in the UI disappear. Inter, conversely, carries a few
    // PUA stylistic-alternate glyphs that collide with some Phosphor icon codepoints
    // (FOLDER E24A, GEAR E270, GIT_BRANCH E278, BOOK_OPEN E0E6, …), so those inline
    // icons can render as accented Latin. Widgets that draw a *pure* icon glyph
    // (activity bar, etc.) use the dedicated `phosphor-icons` family below, where
    // Phosphor is primary, so they resolve correctly without disturbing normal text.
    egui_phosphor::add_to_fonts(fonts, egui_phosphor::Variant::Regular);
    fonts
        .families
        .entry(FontFamily::Name("phosphor-icons".into()))
        .or_default()
        .push("phosphor".into());

    // Base code font id (bumped +2pt for readability); AppearanceSettings applies
    // the user's ui_scale/code_scale on top of this.
    FontId::new(16.0, FontFamily::Monospace)
}

/// A [`FontId`] that renders Phosphor icon glyphs directly through the dedicated
/// `phosphor-icons` family registered in [`setup_fonts`], bypassing Inter's
/// colliding PUA stylistic-alternate glyphs (which otherwise turn FOLDER/GEAR/
/// GIT_BRANCH/BOOK_OPEN into accented Latin letters). Use ONLY for strings holding a
/// *single* icon glyph — mixed icon+text must keep the normal proportional family so
/// the text still renders (the icon family carries no usable Latin coverage).
pub fn icon_font_id(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("phosphor-icons".into()))
}

/// The OS light/dark preference, or `None` where the platform does not expose
/// one cheaply. Windows keeps this under a single registry value; reading it
/// directly (rather than through a shell) is what makes it affordable to poll
/// from the render loop while a workspace follows the system.
///
/// `VELOCITY_OS_THEME` (`light`, `dark`, `off`) answers before the registry is
/// consulted, so a headless frame server or a CI box with no theme service —
/// and anyone checking that the follow path actually repaints — can state the
/// preference instead of reconfiguring the desktop. An unrecognised value falls
/// through to the real read.
pub fn os_prefers_dark() -> Option<bool> {
    match std::env::var("VELOCITY_OS_THEME")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "light" => return Some(false),
        "dark" => return Some(true),
        "off" => return None,
        _ => {}
    }
    #[cfg(windows)]
    {
        use windows::core::PWSTR;
        use windows::Win32::System::Registry::{
            RegGetValueW, HKEY_CURRENT_USER, REG_VALUE_TYPE, RRF_RT_DWORD,
        };

        let to_utf16 =
            |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
        let mut subkey = to_utf16(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let mut value_name = to_utf16("AppsUseLightTheme");

        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut kind = REG_VALUE_TYPE(0);
        // SAFETY: `RegGetValueW` reads into `value`, `kind` and `size`, all of
        // which are live, correctly sized objects for the whole call; the two
        // UTF-16 buffers are null-terminated and kept alive by this scope, and
        // they are only borrowed as `PWSTR` because the API treats them as
        // read-only inputs. Nothing else mutates them while the pointers are
        // in flight.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PWSTR(subkey.as_mut_ptr()),
                PWSTR(value_name.as_mut_ptr()),
                RRF_RT_DWORD,
                Some(&mut kind),
                Some(std::ptr::addr_of_mut!(value).cast()),
                Some(&mut size),
            )
        };
        // REG_DWORD is 4; anything else (a hand-edited key, a missing value)
        // leaves `value` unset, so report "unknown" instead of guessing.
        if result.0 == 0 && kind.0 == 4 && size == 4 {
            // `AppsUseLightTheme` is 1 for a light desktop and 0 for a dark one.
            Some(value == 0)
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
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

    // Disabled: egui renders disabled widgets by painting them at
    // `disabled_alpha` opacity. The stock 0.35 left idle actions like
    // "Generate Detailed Page" barely distinguishable from the panel
    // background; raising it keeps them discoverable while still reading
    // clearly dimmer than their enabled neighbors.
    visuals.disabled_alpha = 0.6;

    // Inactive: transparent-ish buttons that only reveal fill on hover.
    visuals.widgets.inactive.bg_fill = palette.bg_tertiary;
    visuals.widgets.inactive.weak_bg_fill = palette.bg_secondary;
    // Match the *width* of the hovered/active/open strokes (all 1px) so a framed
    // widget allocates the same size in every state. egui derives a button's inner
    // margin as `button_padding + expansion - bg_stroke.width` (widget_style.rs),
    // so an inactive stroke of width 0 against a hovered stroke of width 1 shrank
    // every button/`selectable_label` by 2px on hover — the sidebar rows (files,
    // bookmarks, favorites) and the nested sub-tabs jittered as the pointer moved.
    // A transparent 1px stroke keeps the border invisible until hover while holding
    // the layout stable.
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::TRANSPARENT);
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

    // Compact, coherent type scale. Default-styled widgets (inputs, buttons, chips)
    // draw with Body/Button, while most sidebar labels are hand-sized at 9-14px; the
    // old 20/16/13pt bases (~23/18.4/15px at ui_scale 1.15) made controls tower over
    // those labels and grow wide enough to clip at the sidebar edge.
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(17.0 * appearance.ui_scale, FontFamily::Proportional),
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

/// Default code font ID (monospace, scaled by the default `code_scale`).
/// Convenience wrapper for rendering code that should use the theme's code font
/// without needing an [`AppearanceSettings`] instance.
pub fn code_font_id() -> FontId {
    AppearanceSettings::default().code_font_id()
}

/// Default UI font ID (proportional, scaled by the default `ui_scale`).
/// Convenience wrapper for rendering UI text that should use the theme's body
/// font without needing an [`AppearanceSettings`] instance.
pub fn ui_font_id() -> FontId {
    AppearanceSettings::default().ui_font_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The way a person picks a palette in Settings.
    fn chosen(theme: ThemeVariant) -> AppearanceSettings {
        let mut appearance = AppearanceSettings::default();
        appearance.set_theme(theme);
        appearance
    }

    #[test]
    fn a_chosen_theme_survives_a_mode_switch() {
        let mut appearance = chosen(ThemeVariant::Daylight);
        appearance.apply_profile(WorkspaceProfile::Accessibility);

        assert_eq!(appearance.profile, WorkspaceProfile::Accessibility);
        assert_eq!(appearance.theme, ThemeVariant::Daylight);
        // The mode still brings the density and type it advertises.
        assert_eq!(appearance.density, Density::Spacious);
        assert_eq!(appearance.ui_scale, 1.15);
    }

    #[test]
    fn an_unchosen_theme_still_follows_the_mode_palette() {
        // Modes are documented to carry their own colors, so a workspace that
        // never picked a palette is recoloured when it switches.
        let mut appearance = AppearanceSettings::default();
        appearance.apply_profile(WorkspaceProfile::AutomationOperator);
        assert_eq!(appearance.theme, ThemeVariant::Operator);

        appearance.apply_profile(WorkspaceProfile::MissionControl);
        assert_eq!(appearance.theme, ThemeVariant::Mission);
    }

    #[test]
    fn a_mode_switch_stops_following_the_desktop() {
        // The Accessibility mode advertises higher contrast. A workspace that was
        // following a light desktop had to stop doing that the moment it asked
        // for this mode, or the mode would keep promising a palette it never
        // painted.
        let mut appearance = AppearanceSettings::default();
        appearance.follow_system_theme = true;
        appearance.apply_profile(WorkspaceProfile::Accessibility);

        assert!(!appearance.follow_system_theme);
        assert_eq!(
            appearance.resolved(Some(false)).theme,
            ThemeVariant::HighContrast
        );
    }

    #[test]
    fn resetting_defaults_gives_the_mode_its_signature_palette_back() {
        let mut appearance = chosen(ThemeVariant::Daylight);
        appearance.apply_profile(WorkspaceProfile::Accessibility);
        appearance.reset_to_profile_preset();

        assert_eq!(appearance.theme, ThemeVariant::HighContrast);
        assert!(!appearance.theme_chosen);
        assert!(!appearance.follow_system_theme);
    }

    #[test]
    fn following_the_system_paints_light_on_a_light_desktop() {
        let mut appearance = AppearanceSettings::default();
        appearance.follow_system_theme = true;
        let resolved = appearance.resolved(Some(false));

        assert_eq!(resolved.theme, ThemeVariant::Daylight);
        assert!(!resolved.theme.is_dark());
        assert_eq!(
            resolved.palette().bg_primary,
            Color32::from_rgb(255, 255, 255)
        );
    }

    #[test]
    fn following_the_system_paints_dark_on_a_dark_desktop() {
        let mut appearance = AppearanceSettings::default();
        appearance.follow_system_theme = true;
        assert_eq!(
            appearance.resolved(Some(true)).theme,
            ThemeVariant::Midnight
        );
    }

    #[test]
    fn following_the_system_keeps_the_saved_theme_when_the_preference_is_unreadable() {
        let mut appearance = AppearanceSettings::default();
        appearance.follow_system_theme = true;
        appearance.theme = ThemeVariant::Mission;
        assert_eq!(appearance.resolved(None).theme, ThemeVariant::Mission);
    }

    #[test]
    fn a_theme_picked_by_hand_is_not_overridden_by_the_system() {
        let appearance = chosen(ThemeVariant::Midnight);
        assert!(!appearance.follow_system_theme);
        assert_eq!(
            appearance.resolved(Some(false)).theme,
            ThemeVariant::Midnight
        );
    }

    #[test]
    fn preferences_saved_before_the_follow_option_still_load() {
        // The two new fields arrived without a container default, so an older
        // workspace-preferences.json must still deserialize; if it did not, the
        // whole restore would silently fall through to shipped defaults.
        let stored = r#"{
            "profile": "MissionControl",
            "theme": "Daylight",
            "density": "Spacious",
            "ui_scale": 1.15,
            "code_scale": 1.12
        }"#;
        let appearance: AppearanceSettings =
            serde_json::from_str(stored).expect("pre-existing preferences must parse");

        assert_eq!(appearance.theme, ThemeVariant::Daylight);
        assert!(!appearance.follow_system_theme);
        assert!(!appearance.theme_chosen);

        let round_tripped: AppearanceSettings =
            serde_json::from_str(&serde_json::to_string(&appearance).unwrap()).unwrap();
        assert_eq!(round_tripped, appearance);
    }

    #[test]
    fn every_variant_resolves_to_a_paintable_palette() {
        for variant in ThemeVariant::ALL {
            let palette = variant.palette();
            assert_ne!(
                palette.bg_primary,
                palette.text,
                "{} is unreadable",
                variant.label()
            );
            assert_eq!(
                variant.is_dark(),
                !matches!(variant, ThemeVariant::Daylight)
            );
        }
    }

    #[test]
    fn a_declared_os_theme_answers_before_the_registry() {
        // Without this seam the light half of the follow path can only be proven
        // on a machine already running a light desktop — and a declaration that
        // lost to the real registry would leave `resolved` never seeing a light
        // signal at all. `off` is the third state, not a fourth theme: it is the
        // "no preference readable" answer the fallback is written for.
        let previous = std::env::var("VELOCITY_OS_THEME").ok();
        for (declared, expected) in [("light", Some(false)), ("DARK", Some(true)), ("off", None)] {
            std::env::set_var("VELOCITY_OS_THEME", declared);
            assert_eq!(os_prefers_dark(), expected, "as {declared}");
        }
        match previous {
            Some(value) => std::env::set_var("VELOCITY_OS_THEME", value),
            None => std::env::remove_var("VELOCITY_OS_THEME"),
        }
    }
}
