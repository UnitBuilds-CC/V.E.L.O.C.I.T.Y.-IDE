//! Code editor widget with line numbers and `syntect` syntax highlighting.
use egui::{Color32, Frame, Rect, TextEdit, Ui, Vec2};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::theme;

pub struct CodeEditor {
    syntax_name: String,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl CodeEditor {
    pub fn new(_buf_id: usize, file_path: Option<&std::path::Path>) -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let syntax_name = if let Some(path) = file_path {
            syntax_set.find_syntax_for_file(path)
                .ok()
                .flatten()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Plain Text".to_string())
        } else {
            "Plain Text".to_string()
        };
        Self { syntax_name, syntax_set, theme_set }
    }

    pub fn show(&mut self, ui: &mut Ui, content: &mut String) -> egui::Response {
        let available = ui.available_rect_before_wrap();
        let line_height = ui.fonts(|f| f.row_height(&theme::code_font_id(14.0)));
        let p = theme::IdePalette::default();

        // A simple single-line gutter width estimate: 2em per digit.
        let line_count = content.lines().count().max(1);
        let digits = line_count.to_string().len();
        let gutter_width = (digits as f32 * 2.0 * 7.0 + 16.0).max(40.0);

        Frame::none()
            .fill(p.bg)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_size(Vec2::new(available.width(), available.height()));

                    // ---- Gutter ----
                    let gutter_rect = Rect::from_min_size(
                        ui.cursor().min,
                        Vec2::new(gutter_width, available.height()),
                    );
                    ui.painter().rect_filled(gutter_rect, 0.0, p.bg_secondary);

                    for (idx, _line) in content.lines().enumerate() {
                        let line_no = idx + 1;
                        let y = gutter_rect.min.y + (idx as f32 * line_height);
                        let text_color = if (idx % 10 == 0) && idx > 0 {
                            p.line_number_active
                        } else {
                            p.line_number
                        };
                        ui.painter().text(
                            egui::Pos2::new(gutter_rect.max.x - 8.0, y),
                            egui::Align2::RIGHT_TOP,
                            format!("{}", line_no),
                            theme::code_font_id(12.0),
                            text_color,
                        );
                    }

                    // ---- Text editor ----
                    let editor_width = (available.width() - gutter_width - 12.0).max(100.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(editor_width, available.height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add(
                                TextEdit::multiline(content)
                                    .font(theme::code_font_id(14.0))
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(20)
                                    .margin(egui::Vec2::splat(8.0))
                                    .layouter(&mut |ui: &Ui, string: &str, wrap_width: f32| {
                                        self.highlight(ui, string, wrap_width)
                                    }),
                            )
                        },
                    )
                    .inner
                })
                .inner
            })
            .inner
    }

    fn highlight(&self, ui: &Ui, text: &str, wrap_width: f32) -> std::sync::Arc<egui::Galley> {
        let font_id = theme::code_font_id(14.0);
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap_width;

        let syntax = self
            .syntax_set
            .find_syntax_by_name(&self.syntax_name)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut h = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(text) {
            let line_without_end = line.trim_end_matches('\n').trim_end_matches('\r');
            let ranges = h.highlight_line(line_without_end, &self.syntax_set).unwrap_or_default();
            for (style, slice) in ranges {
                let color = syntect_color_to_egui(style.foreground);
                job.append(
                    slice,
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color,
                        ..Default::default()
                    },
                );
            }
            // Append the actual newline character so the next line starts on a new visual line.
            if line.ends_with('\n') {
                job.append("\n", 0.0, egui::TextFormat::default());
            }
        }

        ui.fonts(|f| f.layout_job(job))
    }
}

fn syntect_color_to_egui(c: syntect::highlighting::Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}
