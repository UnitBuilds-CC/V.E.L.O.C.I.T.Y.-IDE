#![allow(dead_code)]
//! Minimap — provides a zoomed-out overview of the file for quick navigation.
//!
//! Renders a condensed view of the source with highlighted regions for the
//! viewport, search matches, diagnostics, and git changes.

use eframe::egui::{self, Color32, Rect, Sense, Vec2};

/// Minimap configuration.
#[derive(Debug, Clone, Copy)]
pub struct MinimapConfig {
    pub width: f32,
    pub char_height: f32,
    pub char_width: f32,
    pub visible: bool,
}

impl Default for MinimapConfig {
    fn default() -> Self {
        Self {
            width: 80.0,
            char_height: 2.0,
            char_width: 1.2,
            visible: true,
        }
    }
}

/// Highlights to show on the minimap.
#[derive(Debug, Clone)]
pub struct MinimapHighlight {
    pub line: usize,
    pub color: Color32,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = MinimapConfig::default();
        assert!(cfg.visible);
        assert_eq!(cfg.width, 80.0);
    }
}
