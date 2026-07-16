use eframe::egui;
use eframe::egui::{Color32, FontId, Response, Stroke, TextEdit, TextFormat};
use syntect::easy::HighlightLines;
use syntect::highlighting::{self, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub struct CodeEditor {
    id: egui::Id,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Default for CodeEditor {
    fn default() -> Self {
        Self::new("code_editor")
    }
}

impl CodeEditor {
    pub fn new(id_source: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id: egui::Id::new(id_source),
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String) -> Response {
        let theme = self.theme_set.themes.get("base16-ocean.dark").unwrap_or(&self.theme_set.themes["InspiredGitHub"]);
        let syntax = self
            .syntax_set
            .find_syntax_by_extension("rs")
            .cloned()
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text().clone());

        let ss = self.syntax_set.clone();
        let mut h = HighlightLines::new(&syntax, theme);
        let mut layouter = |ui: &egui::Ui, string: &dyn egui::TextBuffer, wrap_width: f32| {
            let string_str = string.as_str();
            let mut layout_job = egui::text::LayoutJob::default();
            for line in LinesWithEndings::from(string_str) {
                let line_without_nl = if line.ends_with('\n') {
                    &line[..line.len() - 1]
                } else {
                    line
                };
                let ranges = h.highlight_line(line_without_nl, &ss).unwrap_or_default();
                for (style, word) in ranges {
                    let color = syntect_color_to_egui(style.foreground);
                    let format = TextFormat {
                        font_id: FontId::monospace(13.0),
                        color,
                        ..Default::default()
                     };
                    layout_job.append(word, 0.0, format);
                }
                if line.ends_with('\n') {
                    layout_job.append("\n", 0.0, Default::default());
                }
            }
            layout_job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(layout_job))
        };

        TextEdit::multiline(text)
            .id(self.id)
            .code_editor()
            .desired_width(f32::INFINITY)
            .layouter(&mut layouter)
            .show(ui)
            .response
            .response
    }
}

fn syntect_color_to_egui(c: highlighting::Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b).linear_multiply(c.a as f32 / 255.0)
}

/// Render a gutter with line numbers next to a code text edit.
pub fn code_block_with_gutter(ui: &mut egui::Ui, text: &mut String) -> Response {
    let total_rows = text.lines().count().max(1);
    let line_numbers: String = (1..=total_rows).map(|i| format!("{i}\n")).collect();

    ui.horizontal_top(|ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(line_numbers).monospace().size(13.0))
                .selectable(false),
        );
        let mut editor = CodeEditor::default();
        editor.show(ui, text)
    })
    .inner
}
