//! Minimap — provides a zoomed-out overview of the file for quick navigation.
//!
//! Renders a condensed view of the source with highlighted regions for the
//! viewport, search matches, diagnostics, and git changes.
//!
//! The [`MinimapRenderer`] provides viewport-aware, cached rendering that only
//! re-processes lines when content actually changes and subsamples very large
//! files to keep rendering costs bounded.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, Rect, Sense, Vec2};

/// An RGB colour tuple used in minimap colour data.
pub type Rgb = (u8, u8, u8);

/// A syntax span: `(token_string, rgb_colour)`.
pub type SyntaxSpan = (String, Rgb);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Minimap configuration.
#[derive(Debug, Clone, Copy)]
pub struct MinimapConfig {
    /// Pixel width of the minimap panel.
    pub width: f32,
    /// Pixel height per character row (legacy, used by immediate-mode renderer).
    pub char_height: f32,
    /// Pixel width per character column.
    pub char_width: f32,
    /// Whether the minimap is visible at all.
    pub visible: bool,
    /// Whether to draw the viewport indicator overlay.
    pub show_viewport_indicator: bool,
    /// Pixel height allocated per source line in the cached renderer.
    pub line_height: f32,
    /// Files with more lines than this will be subsampled (every Nth line).
    pub max_lines_to_render: usize,
}

impl Default for MinimapConfig {
    fn default() -> Self {
        Self {
            width: 80.0,
            char_height: 2.0,
            char_width: 1.2,
            visible: true,
            show_viewport_indicator: true,
            line_height: 1.0,
            max_lines_to_render: 5000,
        }
    }
}

// ---------------------------------------------------------------------------
// Highlight (used by the immediate-mode renderer)
// ---------------------------------------------------------------------------

/// Highlights to show on the minimap.
#[derive(Debug, Clone)]
pub struct MinimapHighlight {
    pub line: usize,
    pub color: Color32,
}

// ---------------------------------------------------------------------------
// Cached / viewport-aware renderer
// ---------------------------------------------------------------------------

/// Per-line, per-character RGB colour data produced by the cached renderer.
#[derive(Debug, Clone, Default)]
pub struct MinimapRenderData {
    /// For each rendered line, a vec of `(r, g, b)` tuples — one per character
    /// column that was coloured.
    pub line_colors: Vec<Vec<Rgb>>,
    /// Total number of source lines in the document (may be larger than
    /// `line_colors.len()` when subsampling is active).
    pub total_lines: usize,
    /// First visible line in the editor viewport.
    pub viewport_start: usize,
    /// Last visible line in the editor viewport (inclusive).
    pub viewport_end: usize,
    /// Fraction of the file that is currently visible (0.0 – 1.0).
    pub viewport_fraction: f64,
}

/// Viewport-aware minimap renderer with content-hash caching.
///
/// The renderer stores a compact colour representation of the file and only
/// re-renders when the content hash changes.  For files exceeding
/// [`MinimapConfig::max_lines_to_render`] lines, every Nth line is rendered
/// (subsampling) so that rendering cost stays bounded regardless of file size.
#[derive(Debug, Clone)]
pub struct MinimapRenderer {
    /// Hash of the content that was last rendered.
    cache_hash: u64,
    /// Per-line, per-character RGB colours.
    line_colors: Vec<Vec<Rgb>>,
    /// Total source lines in the document.
    total_lines: usize,
    /// First visible line in the editor viewport.
    viewport_start: usize,
    /// Last visible line in the editor viewport (inclusive).
    viewport_end: usize,
    /// Minimap character width (columns).
    max_width: usize,
    /// Whether the renderer has been modified since the last render pass.
    dirty: bool,
    /// Cached render data returned to callers.
    render_data: MinimapRenderData,
}

impl MinimapRenderer {
    /// Create a new, empty renderer.
    pub fn new() -> Self {
        Self {
            cache_hash: 0,
            line_colors: Vec::new(),
            total_lines: 0,
            viewport_start: 0,
            viewport_end: 0,
            max_width: 80,
            dirty: true,
            render_data: MinimapRenderData::default(),
        }
    }

    /// Recompute the minimap colours if the content hash has changed.
    ///
    /// * `text` — full source text of the document.
    /// * `syntax_spans` — `(token, colour)` pairs produced by the syntax
    ///   highlighter.  Each entry maps an exact token string to an RGB colour.
    /// * `config` — current minimap configuration.
    pub fn update(&mut self, text: &str, syntax_spans: &[SyntaxSpan], config: &MinimapConfig) {
        let new_hash = Self::hash_content(text);
        if new_hash == self.cache_hash && !self.dirty {
            return; // content unchanged — skip work
        }
        self.cache_hash = new_hash;
        self.max_width = config.max_lines_to_render.clamp(80, 200);

        let all_lines: Vec<&str> = text.lines().collect();
        let total = all_lines.len();
        self.total_lines = total;

        // Decide subsampling stride.
        let stride = if total > config.max_lines_to_render {
            total.div_ceil(config.max_lines_to_render)
        } else {
            1
        };

        // Build colour data for the (possibly subsampled) lines.
        let mut line_colors: Vec<Vec<Rgb>> = Vec::with_capacity(total.div_ceil(stride));

        for (idx, line) in all_lines.iter().enumerate() {
            if idx % stride != 0 {
                continue;
            }
            let cols = Self::colorize_line(line, syntax_spans, self.max_width);
            line_colors.push(cols);
        }

        self.line_colors = line_colors;
        self.rebuild_render_data(config);
        self.dirty = false;
    }

    /// Update the visible viewport range.
    pub fn set_viewport(&mut self, start_line: usize, end_line: usize) {
        if self.viewport_start == start_line && self.viewport_end == end_line {
            return;
        }
        self.viewport_start = start_line;
        self.viewport_end = end_line;
        self.dirty = true;
    }

    /// Access the latest render data.
    pub fn render_data(&self) -> &MinimapRenderData {
        &self.render_data
    }

    /// Whether the renderer has unflushed changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    // -- internal helpers ---------------------------------------------------

    fn hash_content(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// Produce a per-character colour vec for a single source line.
    fn colorize_line(line: &str, syntax_spans: &[SyntaxSpan], max_width: usize) -> Vec<Rgb> {
        let chars: Vec<char> = line.chars().take(max_width).collect();
        let len = chars.len();
        // Default: muted grey for non-highlighted characters.
        let mut colours = vec![(120u8, 120u8, 120u8); len];

        for (token, colour) in syntax_spans {
            let mut search_start = 0;
            while let Some(pos) = line[search_start..].find(token.as_str()) {
                let abs = search_start + pos;
                let end = (abs + token.len()).min(len);
                for c in &mut colours[abs..end] {
                    *c = *colour;
                }
                search_start = abs + token.len();
                if search_start >= len {
                    break;
                }
            }
        }

        colours
    }

    fn rebuild_render_data(&mut self, config: &MinimapConfig) {
        let vp_frac = if self.total_lines == 0 {
            1.0
        } else {
            let visible = (self.viewport_end.saturating_sub(self.viewport_start) + 1) as f64;
            (visible / self.total_lines as f64).clamp(0.0, 1.0)
        };

        self.render_data = MinimapRenderData {
            line_colors: self.line_colors.clone(),
            total_lines: self.total_lines,
            viewport_start: self.viewport_start,
            viewport_end: self.viewport_end,
            viewport_fraction: vp_frac,
        };
        let _ = config; // reserved for future line_height scaling
    }
}

impl Default for MinimapRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Viewport indicator
// ---------------------------------------------------------------------------

/// Geometry of the viewport indicator overlay on the minimap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapViewportIndicator {
    /// Y offset (pixels) from the top of the minimap.
    pub start_y: f32,
    /// Height (pixels) of the indicator.
    pub height: f32,
    /// Total height (pixels) of the minimap.
    pub total_height: f32,
}

/// Compute the viewport indicator geometry.
///
/// * `total_lines` — total source lines in the document.
/// * `viewport_start` — first visible line.
/// * `viewport_end` — last visible line (inclusive).
/// * `total_height` — pixel height of the minimap area.
pub fn compute_viewport_indicator(
    total_lines: usize,
    viewport_start: usize,
    viewport_end: usize,
    total_height: f32,
) -> MinimapViewportIndicator {
    if total_lines == 0 {
        return MinimapViewportIndicator {
            start_y: 0.0,
            height: total_height,
            total_height,
        };
    }

    let line_h = total_height / total_lines as f32;
    let start_y = viewport_start as f32 * line_h;
    let end_y = (viewport_end + 1) as f32 * line_h;
    let height = (end_y - start_y).clamp(0.0, total_height);
    let start_y = start_y.clamp(0.0, total_height);

    MinimapViewportIndicator {
        start_y,
        height,
        total_height,
    }
}

// ---------------------------------------------------------------------------
// Immediate-mode renderer (original API, kept for backward compatibility)
// ---------------------------------------------------------------------------

/// Render a minimap for the given content.
pub fn render_minimap(
    ui: &mut egui::Ui,
    content: &str,
    config: MinimapConfig,
    viewport_start_line: usize,
    viewport_end_line: usize,
    highlights: &[MinimapHighlight],
    palette: &crate::editor::theme::IdePalette,
) -> Option<usize> {
    if !config.visible {
        return None;
    }

    let total_lines = content.lines().count().max(1);
    let minimap_height = total_lines as f32 * config.char_height;
    let available_height = ui.available_height();
    let scale = if minimap_height > available_height {
        available_height / minimap_height
    } else {
        1.0
    };

    let (response, painter) = ui.allocate_painter(
        Vec2::new(config.width, available_height.min(minimap_height * scale)),
        Sense::click(),
    );

    let rect = response.rect;

    // Background
    painter.rect_filled(rect, 0.0, palette.bg_tertiary);

    // Render lines as colored rectangles
    let line_height = config.char_height * scale;
    for (i, line) in content.lines().enumerate() {
        let y = rect.min.y + i as f32 * line_height;
        if y > rect.max.y {
            break;
        }

        let indent = line.len() - line.trim_start().len();
        let content_len = line.trim().len().min(60);

        if content_len > 0 {
            let x_start = rect.min.x + indent as f32 * config.char_width * scale;
            let x_end = x_start + content_len as f32 * config.char_width * scale;
            let line_rect = Rect::from_min_max(
                egui::pos2(x_start.min(rect.max.x), y),
                egui::pos2(x_end.min(rect.max.x), y + line_height),
            );
            painter.rect_filled(line_rect, 0.0, palette.text_muted.gamma_multiply(0.3));
        }
    }

    // Viewport indicator
    if config.show_viewport_indicator {
        let vp_start_y = rect.min.y + viewport_start_line as f32 * line_height;
        let vp_end_y = rect.min.y + viewport_end_line as f32 * line_height;
        let viewport_rect = Rect::from_min_max(
            egui::pos2(rect.min.x, vp_start_y.max(rect.min.y)),
            egui::pos2(rect.max.x, vp_end_y.min(rect.max.y)),
        );
        painter.rect_filled(viewport_rect, 0.0, palette.accent.gamma_multiply(0.12));
        painter.rect_stroke(
            viewport_rect,
            0.0,
            egui::Stroke::new(1.0, palette.accent.gamma_multiply(0.4)),
            egui::StrokeKind::Outside,
        );
    }

    // Highlights (search matches, errors, etc.)
    for hl in highlights {
        let y = rect.min.y + hl.line as f32 * line_height;
        if y > rect.max.y {
            continue;
        }
        let hl_rect = Rect::from_min_max(
            egui::pos2(rect.max.x - 3.0, y),
            egui::pos2(rect.max.x, y + line_height),
        );
        painter.rect_filled(hl_rect, 0.0, hl.color);
    }

    // Click to jump
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let relative_y = pos.y - rect.min.y;
            let target_line = (relative_y / line_height) as usize;
            return Some(target_line.min(total_lines.saturating_sub(1)));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn small_text() -> &'static str {
        "fn main() {\n    println!(\"hello\");\n}\n"
    }

    fn large_text(lines: usize) -> String {
        (0..lines).map(|i| format!("let x{i} = {i};\n")).collect()
    }

    fn default_spans() -> Vec<SyntaxSpan> {
        vec![
            ("fn".into(), (86, 156, 214)),
            ("let".into(), (86, 156, 214)),
        ]
    }

    // -- MinimapConfig defaults ---------------------------------------------

    #[test]
    fn config_defaults_are_correct() {
        let cfg = MinimapConfig::default();
        assert!(cfg.visible);
        assert!(cfg.show_viewport_indicator);
        assert_eq!(cfg.width, 80.0);
        assert_eq!(cfg.line_height, 1.0);
        assert_eq!(cfg.max_lines_to_render, 5000);
    }

    // -- Small file rendering -----------------------------------------------

    #[test]
    fn render_small_file_produces_correct_line_count() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update(small_text(), &default_spans(), &cfg);
        let data = r.render_data();
        // small_text has 3 non-empty lines (trailing newline doesn't add a 4th
        // because .lines() skips the trailing empty segment).
        assert_eq!(data.total_lines, 3);
        assert_eq!(data.line_colors.len(), 3);
    }

    #[test]
    fn render_small_file_keywords_colored() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update(small_text(), &default_spans(), &cfg);
        let data = r.render_data();
        // First line is "fn main() {" — first two chars should be keyword colour.
        let first_line = &data.line_colors[0];
        assert_eq!(first_line[0], (86, 156, 214)); // 'f'
        assert_eq!(first_line[1], (86, 156, 214)); // 'n'
    }

    // -- Subsampling for large files ----------------------------------------

    #[test]
    fn large_file_is_subsamped() {
        let mut r = MinimapRenderer::new();
        let mut cfg = MinimapConfig::default();
        cfg.max_lines_to_render = 100;
        let text = large_text(1000);
        r.update(&text, &[], &cfg);
        let data = r.render_data();
        assert_eq!(data.total_lines, 1000);
        // Should have rendered ~100 lines, not 1000.
        assert!(data.line_colors.len() <= 101);
        assert!(data.line_colors.len() >= 99);
    }

    #[test]
    fn subsampling_preserves_total_lines() {
        let mut r = MinimapRenderer::new();
        let mut cfg = MinimapConfig::default();
        cfg.max_lines_to_render = 50;
        let text = large_text(500);
        r.update(&text, &[], &cfg);
        assert_eq!(r.render_data().total_lines, 500);
    }

    // -- Viewport indicator -------------------------------------------------

    #[test]
    fn viewport_indicator_top_of_file() {
        let ind = compute_viewport_indicator(1000, 0, 49, 500.0);
        assert!((ind.start_y - 0.0).abs() < 0.01);
        assert!((ind.height - 25.0).abs() < 0.01); // 50/1000 * 500
        assert_eq!(ind.total_height, 500.0);
    }

    #[test]
    fn viewport_indicator_middle_of_file() {
        let ind = compute_viewport_indicator(1000, 250, 299, 500.0);
        let expected_y = 250.0 / 1000.0 * 500.0;
        assert!((ind.start_y - expected_y).abs() < 0.01);
        assert!((ind.height - 25.0).abs() < 0.01);
    }

    #[test]
    fn viewport_indicator_zero_lines() {
        let ind = compute_viewport_indicator(0, 0, 0, 200.0);
        assert_eq!(ind.start_y, 0.0);
        assert_eq!(ind.height, 200.0);
    }

    #[test]
    fn viewport_indicator_single_line_file() {
        let ind = compute_viewport_indicator(1, 0, 0, 100.0);
        assert!((ind.start_y - 0.0).abs() < 0.01);
        assert!((ind.height - 100.0).abs() < 0.01);
    }

    // -- Cache invalidation -------------------------------------------------

    #[test]
    fn cache_invalidated_on_content_change() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update("aaa\n", &[], &cfg);
        assert!(!r.is_dirty());
        r.update("bbb\n", &[], &cfg);
        // After update with new content the renderer should have processed it.
        assert_eq!(r.render_data().total_lines, 1);
        assert!(!r.is_dirty());
    }

    #[test]
    fn no_rerender_when_content_unchanged() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update("hello\nworld\n", &[], &cfg);
        let hash_before = r.cache_hash;
        r.update("hello\nworld\n", &[], &cfg);
        assert_eq!(r.cache_hash, hash_before);
    }

    // -- Empty file ---------------------------------------------------------

    #[test]
    fn empty_file_handling() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update("", &[], &cfg);
        let data = r.render_data();
        assert_eq!(data.total_lines, 0);
        assert!(data.line_colors.is_empty());
        assert!((data.viewport_fraction - 1.0).abs() < f64::EPSILON);
    }

    // -- set_viewport -------------------------------------------------------

    #[test]
    fn set_viewport_marks_dirty() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update(small_text(), &[], &cfg);
        assert!(!r.is_dirty());
        r.set_viewport(1, 2);
        assert!(r.is_dirty());
    }

    #[test]
    fn set_viewport_same_values_not_dirty() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update(small_text(), &[], &cfg);
        r.set_viewport(0, 0);
        assert!(!r.is_dirty());
    }

    // -- viewport_fraction --------------------------------------------------

    #[test]
    fn viewport_fraction_half_file() {
        let mut r = MinimapRenderer::new();
        let cfg = MinimapConfig::default();
        r.update(&large_text(100), &[], &cfg);
        r.set_viewport(0, 49);
        // rebuild needed because set_viewport marks dirty but doesn't rebuild
        r.update(&large_text(100), &[], &cfg);
        r.set_viewport(0, 49);
        // Manually rebuild to get updated fraction.
        r.rebuild_render_data(&cfg);
        let frac = r.render_data().viewport_fraction;
        assert!((frac - 0.5).abs() < 0.01);
    }

    // -- colorize_line ------------------------------------------------------

    #[test]
    fn colorize_line_highlights_all_occurrences() {
        let spans = vec![("ab".into(), (255, 0, 0))];
        let colours = MinimapRenderer::colorize_line("ab ab", &spans, 80);
        assert_eq!(colours[0], (255, 0, 0));
        assert_eq!(colours[1], (255, 0, 0));
        assert_eq!(colours[2], (120, 120, 120)); // space
        assert_eq!(colours[3], (255, 0, 0));
        assert_eq!(colours[4], (255, 0, 0));
    }

    #[test]
    fn colorize_line_respects_max_width() {
        let spans: Vec<SyntaxSpan> = vec![];
        let colours = MinimapRenderer::colorize_line("abcdefghij", &spans, 5);
        assert_eq!(colours.len(), 5);
    }

    // -- Default trait ------------------------------------------------------

    #[test]
    fn default_renderer_is_dirty() {
        let r = MinimapRenderer::default();
        assert!(r.is_dirty());
        assert_eq!(r.render_data().total_lines, 0);
    }
}
