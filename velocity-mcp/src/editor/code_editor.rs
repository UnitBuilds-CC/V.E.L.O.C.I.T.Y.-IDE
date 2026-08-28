use crate::editor::bracket_match::find_matching_bracket;
use crate::editor::theme::AppearanceSettings;
use eframe::egui;
use eframe::egui::{Color32, Response, TextEdit, TextFormat};
use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{self, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

/// Editor rendering options for enhanced features.
#[derive(Default)]
pub struct EditorOptions {
    /// Cursor byte offset for bracket matching.
    pub cursor_offset: usize,
    /// Diagnostics to render as squiggles (line, severity: 1=error, 2=warning).
    pub diagnostic_lines: Vec<(usize, u8)>,
    /// Breakpoint lines (1-based).
    pub breakpoints: Vec<usize>,
    /// Code fold state reference.
    pub collapsed_lines: Vec<usize>,
    /// Whether word wrap is enabled.
    pub word_wrap: bool,
}

pub struct CodeEditor {
    id: egui::Id,
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
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        text: &mut String,
        path: Option<&std::path::Path>,
        pending_line: Option<usize>,
        active_locks: &[crate::automation::mediator::EditLock],
        appearance: AppearanceSettings,
        diff_marks: &[u8],
    ) -> Response {
        self.show_enhanced(
            ui,
            text,
            path,
            pending_line,
            active_locks,
            appearance,
            diff_marks,
            &EditorOptions::default(),
        )
    }

    /// Enhanced show with bracket matching, folding, breakpoints, diagnostics.
    pub fn show_enhanced(
        &mut self,
        ui: &mut egui::Ui,
        text: &mut String,
        path: Option<&std::path::Path>,
        pending_line: Option<usize>,
        active_locks: &[crate::automation::mediator::EditLock],
        appearance: AppearanceSettings,
        diff_marks: &[u8],
        options: &EditorOptions,
    ) -> Response {
        let extension = path
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt");
        let palette = appearance.palette();
        let code_font = appearance.code_font_id();

        let theme = THEME_SET
            .themes
            .get("base16-ocean.dark")
            .unwrap_or(&THEME_SET.themes["InspiredGitHub"]);
        let syntax = SYNTAX_SET
            .find_syntax_by_extension(extension)
            .cloned()
            .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text().clone());

        let ss = &*SYNTAX_SET;
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
                let ranges = h.highlight_line(line_without_nl, ss).unwrap_or_default();
                for (style, word) in ranges {
                    let color = syntect_color_to_egui(style.foreground);
                    let format = TextFormat {
                        font_id: code_font.clone(),
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

        let is_line_locked = |line_idx: usize| -> bool {
            for lock in active_locks {
                let (start, end) = lock.line_range;
                if line_idx >= start && line_idx <= end {
                    return true;
                }
            }
            false
        };

        let total_rows = text.lines().count().max(1);
        let bracket_match = find_matching_bracket(text, options.cursor_offset);
        let mut gutter_job = egui::text::LayoutJob::default();
        for i in 1..=total_rows {
            let is_locked = is_line_locked(i);
            let is_collapsed = options.collapsed_lines.contains(&(i - 1));
            let has_breakpoint = options.breakpoints.contains(&i);
            let has_diagnostic = options.diagnostic_lines.iter().find(|(l, _)| *l == i);

            // Breakpoint margin (red dot or empty)
            let bp_glyph = if has_breakpoint { "\u{25cf}" } else { " " };
            let bp_color = if has_breakpoint {
                palette.error
            } else {
                palette.text_muted
            };
            gutter_job.append(
                bp_glyph,
                0.0,
                egui::TextFormat {
                    font_id: code_font.clone(),
                    color: bp_color,
                    ..Default::default()
                },
            );

            // Fold toggle
            let fold_glyph = if is_collapsed { "\u{25b6}" } else { " " };
            gutter_job.append(
                fold_glyph,
                0.0,
                egui::TextFormat {
                    font_id: code_font.clone(),
                    color: palette.text_muted,
                    ..Default::default()
                },
            );

            // Change marker vs. the on-disk baseline (added/modified/removed).
            let mark = diff_marks.get(i - 1).copied().unwrap_or(0);
            let (glyph, glyph_color) = match mark {
                1 => ("\u{258e}", palette.success),
                2 => ("\u{258e}", palette.accent),
                3 => ("\u{2594}", palette.error),
                _ => {
                    // Diagnostic marker in gutter if no diff mark
                    if let Some((_, severity)) = has_diagnostic {
                        match severity {
                            1 => ("\u{25cf}", palette.error),
                            2 => ("\u{25cf}", palette.warning),
                            _ => (" ", palette.text_muted),
                        }
                    } else {
                        (" ", palette.text_muted)
                    }
                }
            };
            gutter_job.append(
                glyph,
                0.0,
                egui::TextFormat {
                    font_id: code_font.clone(),
                    color: glyph_color,
                    ..Default::default()
                },
            );
            let num_color = if is_locked {
                palette.warning
            } else {
                palette.text_muted
            };
            let line_num_str = if is_locked {
                format!("L{: >3}\n", i)
            } else {
                format!("{: >3}\n", i)
            };
            gutter_job.append(
                &line_num_str,
                0.0,
                egui::TextFormat {
                    font_id: code_font.clone(),
                    color: num_color,
                    ..Default::default()
                },
            );
        }

        // Wrap the entire editor and gutter in a ScrollArea so they scroll together vertically
        let scroll_output = egui::ScrollArea::vertical().show(ui, |ui: &mut egui::Ui| {
            ui.horizontal_top(|ui: &mut egui::Ui| {
                // Line number gutter
                ui.add(egui::Label::new(gutter_job).selectable(false));

                // Vertical divider line
                ui.add(egui::Separator::default().vertical());

                // Code Editor TextEdit
                let text_edit = TextEdit::multiline(text)
                    .id(self.id)
                    .code_editor()
                    .desired_width(if options.word_wrap {
                        // When word wrap is on, let the editor fill available width
                        // so egui can break lines at the container boundary.
                        ui.available_width()
                    } else {
                        f32::INFINITY
                    })
                    .layouter(&mut layouter);

                ui.add(text_edit)
            })
            .inner
        });

        let response = scroll_output.inner;

        // Bracket match highlight hint (shown as a subtle bar below editor)
        if let Some(bm) = bracket_match {
            let open_line = text[..bm.open_offset].lines().count();
            let close_line = text[..bm.close_offset].lines().count();
            if open_line != close_line {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        palette.accent.gamma_multiply(0.7),
                        format!(
                            "Bracket match: line {} \u{2194} line {}",
                            open_line, close_line
                        ),
                    );
                });
            }
        }

        if let Some(target_line) = pending_line {
            let mut char_idx = 0;
            for (idx, line_str) in text.lines().enumerate() {
                if idx + 1 == target_line {
                    break;
                }
                char_idx += line_str.chars().count() + 1; // +1 for '\n'
            }

            let mut state = egui::widgets::text_edit::TextEditState::default();
            let ccursor = egui::text::CCursor::new(char_idx);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
            state.store(ui.ctx(), response.id);
            response.request_focus();
        }

        response
    }
}

fn syntect_color_to_egui(c: highlighting::Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b).linear_multiply(c.a as f32 / 255.0)
}

/// Render a gutter with line numbers next to a code text edit.
#[allow(dead_code)]
pub fn code_block_with_gutter(ui: &mut egui::Ui, text: &mut String) -> Response {
    let mut editor = CodeEditor::default();
    editor.show(
        ui,
        text,
        None,
        None,
        &[],
        AppearanceSettings::default(),
        &[],
    )
}
