use crate::editor::bracket_match::find_matching_bracket;
use crate::editor::theme::AppearanceSettings;
use eframe::egui;
use eframe::egui::{Color32, Response, TextEdit, TextFormat};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use syntect::easy::HighlightLines;
use syntect::highlighting::{self, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Reusable syntax + layout caches for one editor buffer, keyed by its stable
/// per-buffer [`egui::Id`]. These live in a global store because a fresh
/// [`CodeEditor`] is constructed on every frame — without persistence the whole
/// file would be re-tokenized by syntect and re-shaped by egui each frame.
#[derive(Default)]
struct EditorCaches {
    /// Content hash the spans were tokenized for.
    syntax_hash: u64,
    /// File extension the spans were tokenized for.
    syntax_ext: String,
    /// Highlighted spans: `(text, foreground_color)` per span.
    spans: Vec<(String, highlighting::Color)>,
    /// Laid-out galley for the current spans (avoids re-shaping every frame).
    galley: Option<Arc<egui::Galley>>,
    /// Wrap width `galley` was built for.
    galley_wrap: f32,
    /// Font hash `galley` was built for.
    galley_font: u64,
    /// Hash of the semantic-token overlay the spans were coloured for, so the
    /// cache busts when the language server's token stream arrives/changes even
    /// though the document text is unchanged.
    sem_hash: u64,
    /// Real 0-based inclusive viewport line range (first, last) observed during
    /// the *previous* frame's layout, derived from the ScrollArea's actual scroll
    /// offset. The gutter reads this to decide which line numbers to build, so
    /// the top of the viewport is never left blank (the historical bug where an
    /// absolute-window estimate dropped the first few lines' numbers).
    viewport: Option<(usize, usize)>,
}

static EDITOR_CACHES: LazyLock<Mutex<HashMap<egui::Id, EditorCaches>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Max distinct buffers to keep cached before dropping them all (bounds memory;
/// galleys for large files are costly to hold). Re-warming costs one tokenize.
const MAX_EDITOR_CACHES: usize = 8;

/// Hash a [`egui::FontId`] (family + size) so a theme/font change busts the galley cache.
fn font_id_hash(f: &egui::FontId) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in f.size.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for b in format!("{:?}", f.family).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// FNV-1a hash of a buffer's full text (much cheaper than re-tokenizing).
/// Public so the render loop can key the semantic-token cache to the exact
/// content hash the editor uses, guaranteeing an overlay is only applied to
/// the content it was computed for.
pub fn content_hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Compare two wrap widths treating infinities as equal and finite values as
/// equal within a sub-pixel tolerance.
fn same_wrap(a: f32, b: f32) -> bool {
    a == b || (a.is_finite() && b.is_finite() && (a - b).abs() < 0.5)
}

/// 1-based inclusive gutter line range to build, given the real 0-based
/// inclusive viewport `(first, last)` observed on the previous frame and the
/// document's total row count. The range is padded by one viewport above and
/// below so fast scrolling never outruns it; the result is clamped to
/// `1..=total_rows`. Returns `None` when the viewport is not yet known (the
/// very first frame), in which case the caller builds every line.
///
/// This is the whole fix for the "top lines have no number" bug: the range is
/// derived from the actual scroll offset, so a file at scroll-top always
/// includes line 1 rather than skipping the first few lines' worth of padding.
fn gutter_range_from_viewport(
    viewport: Option<(usize, usize)>,
    total_rows: usize,
) -> Option<(usize, usize)> {
    let (first0, last0) = viewport?;
    let span = last0.saturating_sub(first0) + 1;
    // 0-based first → 1-based line is first0 + 1; pad one viewport above.
    let start = (first0 + 1).saturating_sub(span).max(1);
    let end = (last0 + 1) + span;
    Some((start, end.min(total_rows)))
}

/// FNV-1a over a byte slice (cache-key hashing, editor-wide convention).
fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn mix_u64(h: &mut u64, v: u64) {
    for b in v.to_le_bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

/// Fold a semantic-token list into a stable hash used to bust the span cache
/// when the language server's token stream arrives or changes (even though the
/// document text is byte-for-byte identical).
fn semantic_tokens_hash(tokens: &[crate::editor::lsp_client::LspSemanticToken]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for t in tokens {
        mix_u64(&mut h, t.line as u64);
        mix_u64(&mut h, t.start_char as u64);
        mix_u64(&mut h, t.length as u64);
        mix_u64(&mut h, fnv1a_bytes(t.token_type.as_bytes()));
        mix_u64(&mut h, t.modifiers.len() as u64);
    }
    mix_u64(&mut h, tokens.len() as u64);
    h
}

fn color_eq(a: highlighting::Color, b: highlighting::Color) -> bool {
    a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a
}

/// Map an LSP semantic token to a base16-ocean-compatible foreground colour.
/// Returns `None` for types we intentionally leave to syntect, so ordinary
/// keywords/strings keep the theme's own palette.
fn semantic_token_color(
    tok: &crate::editor::lsp_client::LspSemanticToken,
) -> Option<highlighting::Color> {
    let c = |r, g, b| highlighting::Color { r, g, b, a: 255 };
    // A modifier outranks the type: deprecated members read as muted.
    if tok.modifiers.iter().any(|m| m == "deprecated") {
        return Some(c(0x77, 0x7c, 0x7d));
    }
    match tok.token_type.as_str() {
        "namespace" | "type" | "class" | "struct" | "enum" | "interface" | "typeParameter" => {
            Some(c(0xa3, 0xbe, 0x8c))
        }
        "enumMember" | "macro" | "event" => Some(c(0xd0, 0x87, 0x71)),
        "function" | "method" => Some(c(0x8f, 0xbc, 0xbb)),
        "property" | "variable" | "parameter" => Some(c(0xbf, 0x61, 0x6a)),
        "decorator" => Some(c(0xf2, 0x77, 0x7a)),
        _ => None,
    }
}

/// Override syntect span colours on one line with the LSP semantic tokens that
/// cover it, splitting spans at token boundaries. `line_text` excludes the
/// newline; `spans` concatenate back to exactly `line_text`. Token columns are
/// in the server's encoding (UTF-16 for most servers) — an acceptable
/// approximation for the predominantly-ASCII identifiers we recolour.
fn apply_semantic_line(
    line_text: &str,
    spans: Vec<(String, highlighting::Color)>,
    tokens: &[&crate::editor::lsp_client::LspSemanticToken],
) -> Vec<(String, highlighting::Color)> {
    let chars: Vec<char> = line_text.chars().collect();
    if chars.is_empty() {
        return spans;
    }
    let mut over: Vec<Option<highlighting::Color>> = vec![None; chars.len()];
    for t in tokens {
        if let Some(col) = semantic_token_color(t) {
            let start = t.start_char.min(chars.len());
            let end = (t.start_char + t.length).min(chars.len());
            for slot in over.iter_mut().take(end).skip(start) {
                *slot = Some(col);
            }
        }
    }
    if over.iter().all(|o| o.is_none()) {
        return spans;
    }
    let mut out: Vec<(String, highlighting::Color)> = Vec::new();
    let mut col = 0usize; // absolute char index of the current span's start
    for (txt, base) in spans {
        let sc: Vec<char> = txt.chars().collect();
        let mut i = 0usize;
        while i < sc.len() {
            let cur = over.get(col + i).copied().flatten().unwrap_or(base);
            let mut j = i + 1;
            while j < sc.len() {
                let nx = over.get(col + j).copied().flatten().unwrap_or(base);
                if !color_eq(nx, cur) {
                    break;
                }
                j += 1;
            }
            out.push((sc[i..j].iter().collect(), cur));
            i = j;
        }
        col += sc.len();
    }
    out
}

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
    /// LSP semantic tokens for this document (empty when unavailable). When
    /// present, each token's range overrides the syntect foreground colour so
    /// the editor reflects the language server's semantic view (types,
    /// parameters, deprecated members, …) rather than regex highlighting alone.
    pub semantic_tokens: Vec<crate::editor::lsp_client::LspSemanticToken>,
}

pub struct CodeEditor {
    id: egui::Id,
    /// 0-based inclusive viewport line range (first, last) observed during the
    /// last [`Self::show_enhanced`] call, derived from the editor's real scroll
    /// offset. The minimap reads this so its green indicator tracks scrolling
    /// instead of a hardcoded range.
    pub last_viewport_lines: Option<(usize, usize)>,
    /// Set during [`Self::show_enhanced`] when the user Ctrl/Cmd+clicks the
    /// text: the 0-based **char** offset resolved through the laid-out galley.
    /// The app shell reads it after drawing to aim the caret and run
    /// go-to-definition there (the mouse form of F12, like every major editor).
    pub goto_request: Option<usize>,
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
            last_viewport_lines: None,
            goto_request: None,
        }
    }

    /// The egui `Id` under which the *main* code editor stores its
    /// `TextEditState` (the live caret and egui's built-in undo history).
    ///
    /// `CodeEditor::new(buffer_id)` salts the id per open buffer (so each file
    /// keeps independent scroll/caret/undo state), which means any code that
    /// reads or writes the caret from *outside* the widget -- status-bar cursor
    /// tracking, inline completion, whole-line edits, LSP position resolution --
    /// must resolve the id the same way. Targeting a literal such as
    /// `"code_editor"` addresses a different, empty state and silently pins the
    /// caret to line 0, so this is the single source of truth for that id.
    pub fn textedit_id<K: std::hash::Hash + std::fmt::Debug + Clone>(buffer_id: &K) -> egui::Id {
        egui::Id::new(buffer_id.clone())
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

        // ── Persistent per-buffer caches ───────────────────────────────────
        // A fresh CodeEditor is constructed every frame, so the syntax spans
        // and the laid-out galley live in a global store keyed by the (stable,
        // per-buffer) editor id. Without this the whole file is re-tokenized by
        // syntect AND re-shaped by egui every frame — O(file_size) per frame,
        // which is what makes large files hover around 200ms/refresh.
        let text_hash = content_hash(text);
        let font_hash = font_id_hash(&code_font);
        let sem_hash = semantic_tokens_hash(&options.semantic_tokens);

        let mut cache_guard = EDITOR_CACHES.lock().unwrap_or_else(|e| e.into_inner());
        if cache_guard.len() > MAX_EDITOR_CACHES {
            cache_guard.clear();
        }
        let cache = cache_guard.entry(self.id).or_default();
        // Copy the previous frame's real viewport now, before `layouter` takes a
        // mutable borrow of `cache` below. The gutter is built from this.
        let prev_viewport = cache.viewport;

        // Re-tokenize when the content, the language, OR the semantic-token
        // overlay changed (the token stream arrives asynchronously after the
        // text, so a text-only key would freeze syntect-only colours).
        if cache.syntax_hash != text_hash
            || cache.syntax_ext != extension
            || cache.sem_hash != sem_hash
        {
            let ss = &*SYNTAX_SET;
            let mut hl = HighlightLines::new(&syntax, theme);
            let mut spans = Vec::new();
            // Group LSP tokens by 0-based line for per-line colour override.
            let mut sem_by_line: HashMap<usize, Vec<&crate::editor::lsp_client::LspSemanticToken>> =
                HashMap::new();
            for t in &options.semantic_tokens {
                sem_by_line.entry(t.line).or_default().push(t);
            }
            for (li, line) in LinesWithEndings::from(text.as_str()).enumerate() {
                let line_without_nl = if line.ends_with('\n') {
                    &line[..line.len() - 1]
                } else {
                    line
                };
                let line_spans = hl.highlight_line(line_without_nl, ss).unwrap_or_default();
                let mut line_out: Vec<(String, highlighting::Color)> = line_spans
                    .into_iter()
                    .map(|(style, word)| (word.to_string(), style.foreground))
                    .collect();
                if !sem_by_line.is_empty() {
                    if let Some(toks) = sem_by_line.get(&li) {
                        line_out = apply_semantic_line(line_without_nl, line_out, toks);
                    }
                }
                spans.extend(line_out);
                if line.ends_with('\n') {
                    spans.push((
                        "\n".to_string(),
                        highlighting::Color {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 0,
                        },
                    ));
                }
            }
            cache.syntax_hash = text_hash;
            cache.syntax_ext = extension.to_string();
            cache.sem_hash = sem_hash;
            cache.spans = spans;
            cache.galley = None; // content changed → force a reshape
        }

        let mut layouter = |ui: &egui::Ui, _string: &dyn egui::TextBuffer, wrap_width: f32| {
            // Return the cached galley when text, wrap width and font all match
            // — this is the path taken on every idle frame (the big perf win).
            if let Some(g) = cache.galley.as_ref() {
                if same_wrap(cache.galley_wrap, wrap_width) && cache.galley_font == font_hash {
                    return g.clone();
                }
            }
            let mut layout_job = egui::text::LayoutJob::default();
            for (word, color) in cache.spans.iter() {
                if word == "\n" {
                    layout_job.append("\n", 0.0, Default::default());
                    continue;
                }
                let egui_color = syntect_color_to_egui(*color);
                let format = TextFormat {
                    font_id: code_font.clone(),
                    color: egui_color,
                    ..Default::default()
                };
                layout_job.append(word.as_str(), 0.0, format);
            }
            layout_job.wrap.max_width = wrap_width;
            let g = ui.fonts_mut(|f| f.layout_job(layout_job));
            cache.galley = Some(g.clone());
            cache.galley_wrap = wrap_width;
            cache.galley_font = font_hash;
            g
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

        // Viewport-only gutter rendering: build numbers only for the lines that
        // were actually on screen last frame (plus one viewport of margin), so a
        // 10k-line file costs ~150 rows of text per frame, not 10k. Derived from
        // the real scroll offset rather than the editor's absolute window
        // position, which is what previously left the top few lines unnumbered.
        let (gutter_start, gutter_end) = match gutter_range_from_viewport(prev_viewport, total_rows)
        {
            Some((start, end)) => (start, end),
            None => (1, total_rows), // First frame before the viewport is known: render all
        };

        // Add invisible padding for lines above the viewport so the gutter
        // aligns correctly with the code area in the ScrollArea.
        if gutter_start > 1 {
            // Each line is one "\n" in the layout job
            let padding: String = "\n".repeat(gutter_start - 1);
            gutter_job.append(
                &padding,
                0.0,
                egui::TextFormat {
                    font_id: code_font.clone(),
                    color: Color32::TRANSPARENT,
                    ..Default::default()
                },
            );
        }

        for i in gutter_start..=gutter_end {
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

        // Wrap the entire editor and gutter in a ScrollArea so they scroll together vertically.
        //
        // The scroll id is salted with this editor's id so each open buffer keeps
        // its OWN scroll offset — otherwise every tab shares one ScrollArea state
        // and the viewport indicator appears to "jump" / desync when switching files.
        let line_h = (code_font.size * 1.4).max(1.0);
        let mut scroll_area = egui::ScrollArea::vertical().id_salt(self.id);
        // A pending navigation line (go-to-line, minimap click) scrolls the
        // viewport to that line for this frame; the offset then persists because
        // egui writes it back to the scroll state.
        if let Some(tl) = pending_line {
            scroll_area = scroll_area.vertical_scroll_offset(tl.saturating_sub(1) as f32 * line_h);
        }
        let scroll_output = scroll_area.show(ui, |ui: &mut egui::Ui| {
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

                // `show` (not `ui.add`) so the click→char resolution below can
                // reuse the *same* laid-out galley and placement egui just used;
                // the returned Response is exactly what `ui.add` would give.
                let output = text_edit.show(ui);
                let response = output.response.response;
                // Ctrl/Cmd + left-click over the text: resolve the click through
                // the galley exactly like egui's own caret placement
                // (`pointer - inner_rect.min + text_offset + galley.rect.left()`,
                // which for a multiline editor with `text_offset` folded into
                // `galley_pos` reduces to the expression below), and hand the
                // char offset to the app shell.
                let ctrl_click = ui.input(|i| {
                    i.modifiers.command && i.pointer.button_pressed(egui::PointerButton::Primary)
                });
                if ctrl_click && response.hovered() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let rel =
                            pos - output.galley_pos + egui::vec2(output.galley.rect.left(), 0.0);
                        self.goto_request = Some(output.galley.cursor_from_pos(rel).index.0);
                    }
                }
                response
            })
            .inner
        });

        // Record the real viewport for the minimap indicator: derive first
        // visible line from the scroll offset and how many lines fit in view.
        {
            let offset_y = scroll_output.state.offset.y;
            let visible_h = scroll_output.inner_rect.height();
            let first = (offset_y / line_h).floor().max(0.0) as usize;
            let count = (visible_h / line_h).ceil().max(1.0) as usize;
            self.last_viewport_lines = Some((first, first + count.saturating_sub(1)));
            // Persist for next frame's gutter build (see `prev_viewport` above).
            cache.viewport = self.last_viewport_lines;
        }

        // Layout is done; release the global cache lock before the rest of the frame.
        drop(cache_guard);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_wrap_treats_infinities_and_near_values_as_equal() {
        // Non-wrapping code editor passes INFINITY: must compare equal so the
        // galley cache hits every idle frame instead of re-shaping.
        assert!(same_wrap(f32::INFINITY, f32::INFINITY));
        // Sub-pixel drift on finite widths should still hit the cache.
        assert!(same_wrap(400.0, 400.3));
        // Genuinely different widths must bust the cache.
        assert!(!same_wrap(400.0, 520.0));
        assert!(!same_wrap(f32::INFINITY, 400.0));
    }

    #[test]
    fn textedit_id_matches_the_widget_and_isolates_buffers() {
        // The id a caller uses to read/write the live caret must equal the id
        // the editor widget itself stores its state under (render.rs builds the
        // editor as `CodeEditor::new(buffer_id)`), or the caret mis-tracks.
        let buffer_id: u64 = 7;
        let editor = CodeEditor::new(buffer_id);
        assert_eq!(
            editor.id,
            CodeEditor::textedit_id(&buffer_id),
            "external id resolution must match the widget's own id"
        );
        // Per-buffer isolation: two open files never share caret/undo state.
        assert_ne!(
            CodeEditor::textedit_id(&7u64),
            CodeEditor::textedit_id(&8u64)
        );
        // Guards against the historical bug of targeting a fixed literal id.
        assert_ne!(
            CodeEditor::textedit_id(&buffer_id),
            egui::Id::new("code_editor")
        );
    }

    fn tok(
        line: usize,
        start: usize,
        len: usize,
        ty: &str,
    ) -> crate::editor::lsp_client::LspSemanticToken {
        crate::editor::lsp_client::LspSemanticToken {
            line,
            start_char: start,
            length: len,
            token_type: ty.to_string(),
            modifiers: Vec::new(),
        }
    }

    #[test]
    fn semantic_token_color_maps_types_and_leaves_unknowns() {
        assert!(semantic_token_color(&tok(0, 0, 3, "function")).is_some());
        assert!(
            semantic_token_color(&tok(0, 0, 3, "operator")).is_none(),
            "defer to syntect"
        );
        // A deprecated modifier mutes regardless of the base type.
        let mut t = tok(0, 0, 3, "function");
        t.modifiers = vec!["deprecated".to_string()];
        assert_eq!(semantic_token_color(&t).unwrap().r, 0x77);
    }

    #[test]
    fn apply_semantic_line_recolors_and_preserves_text() {
        let base = highlighting::Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        };
        // One syntect span covering the whole line; recolor chars 2..3 ("b").
        let spans = vec![("a b".to_string(), base)];
        let tokens = [&tok(0, 2, 1, "function")];
        let out = apply_semantic_line("a b", spans, &tokens);
        // Text is preserved exactly across the split.
        let joined: String = out.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "a b");
        // "a " keeps the base colour; "b" takes the function colour.
        assert_eq!(out[0].1, base);
        assert_ne!(out.last().unwrap().1, base);
        assert_eq!(out.last().unwrap().0, "b");
    }

    #[test]
    fn apply_semantic_line_noop_without_covered_tokens() {
        let base = highlighting::Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        };
        let spans = vec![("abc".to_string(), base)];
        // Token type maps to None → no override, spans returned unchanged.
        let tokens = [&tok(0, 0, 3, "operator")];
        let out = apply_semantic_line("abc", spans, &tokens);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], ("abc".to_string(), base));
    }

    #[test]
    fn semantic_tokens_hash_is_stable_and_sensitive() {
        let a = vec![tok(0, 0, 3, "function"), tok(1, 4, 2, "variable")];
        let b = vec![tok(0, 0, 3, "function"), tok(1, 4, 2, "variable")];
        let c = vec![tok(0, 0, 3, "function"), tok(1, 4, 2, "property")];
        assert_eq!(semantic_tokens_hash(&a), semantic_tokens_hash(&b));
        assert_ne!(
            semantic_tokens_hash(&a),
            semantic_tokens_hash(&c),
            "type change busts cache"
        );
        assert_ne!(semantic_tokens_hash(&a), semantic_tokens_hash(&[]));
    }

    #[test]
    fn content_hash_is_deterministic_and_distinct() {
        assert_eq!(content_hash("hello world"), content_hash("hello world"));
        assert_ne!(content_hash("hello world"), content_hash("hello worlD"));
    }

    #[test]
    fn font_id_hash_distinguishes_size_and_family_but_is_stable() {
        let a = egui::FontId::proportional(13.0);
        let b = egui::FontId::proportional(13.0);
        let c = egui::FontId::proportional(14.0);
        let d = egui::FontId::monospace(13.0);
        assert_eq!(font_id_hash(&a), font_id_hash(&b), "same font → same hash");
        assert_ne!(
            font_id_hash(&a),
            font_id_hash(&c),
            "size change busts cache"
        );
        assert_ne!(
            font_id_hash(&a),
            font_id_hash(&d),
            "family change busts cache"
        );
    }

    #[test]
    fn gutter_range_at_scroll_top_includes_line_one() {
        // The regression: a file at scroll-top (real viewport starts at line 1)
        // must number line 1, never skip the first few lines. 0-based (0,49).
        let (start, end) = gutter_range_from_viewport(Some((0, 49)), 801).unwrap();
        assert_eq!(start, 1, "top of viewport must be numbered");
        assert!(end >= 50, "must cover the visible rows");
    }

    #[test]
    fn gutter_range_pads_around_a_scrolled_viewport() {
        // Scrolled so lines 101..150 (0-based 100..149) are visible: pad one
        // viewport above/below → start well before 101, end well after 150.
        let (start, end) = gutter_range_from_viewport(Some((100, 149)), 5000).unwrap();
        assert_eq!(start, 51, "one viewport (50) of headroom above line 101");
        assert_eq!(end, 200, "one viewport of lookahead below line 150");
    }

    #[test]
    fn gutter_range_clamps_to_total_rows() {
        // Short file: end must not exceed the real line count.
        let (start, end) = gutter_range_from_viewport(Some((0, 49)), 10).unwrap();
        assert_eq!(start, 1);
        assert_eq!(end, 10, "clamped to the last line");
    }

    #[test]
    fn gutter_range_unknown_viewport_builds_nothing() {
        // First frame: caller falls back to building every line.
        assert_eq!(gutter_range_from_viewport(None, 801), None);
    }
}
