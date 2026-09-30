//! In-file Find & Replace (Ctrl+F / Ctrl+H).
//!
//! Provides incremental search with match highlighting, case sensitivity toggle,
//! regex support, and replace-one / replace-all operations.

use crate::editor::theme::IdePalette;
use eframe::egui;

/// State for the find/replace overlay within a single editor tab.
#[derive(Debug, Clone, Default)]
pub struct FindReplaceState {
    pub visible: bool,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub whole_word: bool,
    /// Indices (byte offsets) of all matches in the current buffer.
    pub matches: Vec<(usize, usize)>,
    /// Capture-group byte spans per regex match, parallel to `matches`
    /// (empty inner vecs in literal mode). Powers `$1`-style expansion in
    /// the replacement field.
    pub match_groups: Vec<Vec<Option<(usize, usize)>>>,
    /// Which match is currently focused (for F3 / Shift+F3 cycling).
    pub current_match: usize,
    /// Whether the replace field is shown (Ctrl+H vs Ctrl+F).
    pub replace_visible: bool,
    /// One-shot flag: just opened, should focus the query field.
    pub just_opened: bool,
}

impl FindReplaceState {
    pub fn open_find(&mut self) {
        self.visible = true;
        self.replace_visible = false;
        self.just_opened = true;
    }

    pub fn open_find_replace(&mut self) {
        self.visible = true;
        self.replace_visible = true;
        self.just_opened = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Recompute matches against `text`. Call when query or text changes.
    pub fn recompute_matches(&mut self, text: &str) {
        self.matches.clear();
        self.match_groups.clear();
        if self.query.is_empty() {
            return;
        }

        if self.use_regex {
            self.compute_regex_matches(text);
        } else {
            self.compute_literal_matches(text);
            // Keep the group vec parallel to `matches` so index math upstream
            // never has to special-case literal mode.
            self.match_groups = vec![Vec::new(); self.matches.len()];
        }

        // Clamp current_match
        if self.current_match >= self.matches.len() {
            self.current_match = 0;
        }
    }

    /// Compute matches using the built-in regex engine. On a syntax error the
    /// match list is simply left empty (the UI then shows "No results"),
    /// mirroring how an unmatched literal query behaves.
    fn compute_regex_matches(&mut self, text: &str) {
        let regex =
            match crate::editor::regex_engine::Regex::compile(&self.query, !self.case_sensitive) {
                Ok(r) => r,
                Err(_) => return,
            };
        for m in regex.find_all_caps(text) {
            if self.whole_word && !self.is_whole_word(text, m.start, m.end) {
                continue;
            }
            self.matches.push((m.start, m.end));
            self.match_groups.push(m.groups);
        }
    }

    /// Whole-word boundary test shared by literal and regex search.
    fn is_whole_word(&self, text: &str, start: usize, end: usize) -> bool {
        let before_ok = start == 0 || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
        let after_ok = end >= text.len() || !text.as_bytes()[end].is_ascii_alphanumeric();
        before_ok && after_ok
    }

    fn compute_literal_matches(&mut self, text: &str) {
        let query = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        let haystack = if self.case_sensitive {
            text.to_string()
        } else {
            text.to_lowercase()
        };

        let qlen = query.len();
        if qlen == 0 {
            return;
        }

        let mut start = 0;
        while let Some(pos) = haystack[start..].find(&query) {
            let abs_pos = start + pos;
            let end = abs_pos + qlen;

            if self.whole_word {
                let before_ok =
                    abs_pos == 0 || !text.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
                let after_ok = end >= text.len() || !text.as_bytes()[end].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    self.matches.push((abs_pos, end));
                }
            } else {
                self.matches.push((abs_pos, end));
            }
            start = abs_pos + 1;
            if start >= haystack.len() {
                break;
            }
        }
    }

    /// Move to next match.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    /// Move to previous match.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }

    /// The replacement text for match `idx`, with `$0`–`$99` / `${n}`
    /// capture-group references expanded in regex mode. Literal mode inserts
    /// the replacement verbatim (a `$1` there is just "$1").
    fn expanded_replacement(&self, text: &str, idx: usize) -> String {
        if !self.use_regex {
            return self.replacement.clone();
        }
        let Some(&(start, end)) = self.matches.get(idx) else {
            return self.replacement.clone();
        };
        let groups: Vec<Option<&str>> = self
            .match_groups
            .get(idx)
            .map(|g| g.iter().map(|o| o.map(|(a, b)| &text[a..b])).collect())
            .unwrap_or_default();
        crate::editor::search::expand_replacement(&self.replacement, &text[start..end], &groups)
    }

    /// Replace the current match. Returns the new text.
    pub fn replace_current(&mut self, text: &str) -> String {
        if self.matches.is_empty() {
            return text.to_string();
        }
        let (start, end) = self.matches[self.current_match];
        let replacement = self.expanded_replacement(text, self.current_match);
        let mut result = String::with_capacity(text.len());
        result.push_str(&text[..start]);
        result.push_str(&replacement);
        result.push_str(&text[end..]);
        result
    }

    /// Replace all matches. Returns the new text.
    pub fn replace_all(&self, text: &str) -> String {
        if self.matches.is_empty() {
            return text.to_string();
        }
        let mut result = String::with_capacity(text.len());
        let mut last_end = 0;
        for (idx, &(start, end)) in self.matches.iter().enumerate() {
            result.push_str(&text[last_end..start]);
            result.push_str(&self.expanded_replacement(text, idx));
            last_end = end;
        }
        result.push_str(&text[last_end..]);
        result
    }

    /// Render the find/replace bar UI. Returns actions to apply.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        palette: &crate::editor::theme::IdePalette,
    ) -> FindAction {
        let mut action = FindAction::None;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // Find field
            let find_response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(200.0)
                    .hint_text("Find..."),
            );
            if self.just_opened {
                find_response.request_focus();
                self.just_opened = false;
            }
            if find_response.changed() {
                action = FindAction::Recompute;
            }

            // Match count label
            let count = self.matches.len();
            let label = if count == 0 && !self.query.is_empty() {
                "No results".to_string()
            } else if count > 0 {
                format!("{}/{}", self.current_match + 1, count)
            } else {
                String::new()
            };
            if !label.is_empty() {
                ui.colored_label(palette.text_muted, &label);
            }

            // Nav buttons
            if ui
                .small_button("\u{25B2}")
                .on_hover_text("Previous (Shift+F3)")
                .clicked()
            {
                action = FindAction::Prev;
            }
            if ui
                .small_button("\u{25BC}")
                .on_hover_text("Next (F3)")
                .clicked()
            {
                action = FindAction::Next;
            }

            // Toggles
            let cs_btn = ui.selectable_label(self.case_sensitive, "Aa");
            if cs_btn.clicked() {
                self.case_sensitive = !self.case_sensitive;
                action = FindAction::Recompute;
            }
            let ww_btn = ui.selectable_label(self.whole_word, "W");
            if ww_btn.clicked() {
                self.whole_word = !self.whole_word;
                action = FindAction::Recompute;
            }

            // Close
            if ui
                .small_button(
                    egui::RichText::new(egui_phosphor::regular::X)
                        .font(crate::editor::theme::icon_font_id(11.0)),
                )
                .clicked()
            {
                self.close();
            }
        });

        // Replace row
        if self.replace_visible {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(
                    egui::TextEdit::singleline(&mut self.replacement)
                        .desired_width(200.0)
                        .hint_text("Replace..."),
                );
                if ui.small_button("Replace").clicked() {
                    action = FindAction::ReplaceCurrent;
                }
                if ui.small_button("Replace All").clicked() {
                    action = FindAction::ReplaceAll;
                }
            });
        }

        action
    }
}

/// Convert a byte offset (as stored in [`FindReplaceState::matches`]) to a char
/// index (as egui's `CCursor` expects): the number of chars that begin before
/// `byte`. For a real char boundary (every match offset is one, since matches
/// come from scanning the same string) this is the exact char index of the
/// character at `byte`. It never panics on a non-boundary `byte` and clamps
/// past the end, so it bridges find's byte-based offsets to the editor's
/// char-based caret without assuming ASCII-only content.
pub fn char_index_of_byte(content: &str, byte: usize) -> usize {
    content
        .char_indices()
        .take_while(|&(b, _)| b < byte)
        .count()
}

/// Actions the find/replace UI can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindAction {
    None,
    Recompute,
    Next,
    Prev,
    ReplaceCurrent,
    ReplaceAll,
}

/// Render the find/replace overlay bar. `content` is the buffer text; Replace
/// and Replace-All mutate it in place and the edit is picked up by the buffer's
/// per-frame dirty tracking.
pub fn render_find_replace(
    ui: &mut egui::Ui,
    state: &mut FindReplaceState,
    content: &mut String,
    palette: IdePalette,
) {
    egui::Frame::new()
        .fill(palette.bg_secondary)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("Find\u{2026}")
                        .desired_width(200.0),
                );
                if state.just_opened {
                    resp.request_focus();
                    state.just_opened = false;
                }
                if resp.changed() {
                    state.recompute_matches(content.as_str());
                }
                // Match count. Only surface a status once the user has typed a
                // query: an empty field is "nothing to search for yet", not "no
                // matches", so keep the label blank until a query exists.
                let match_text = if !state.matches.is_empty() {
                    format!("{}/{}", state.current_match + 1, state.matches.len())
                } else if state.query.is_empty() {
                    String::new()
                } else if state.use_regex
                    && crate::editor::regex_engine::Regex::compile(
                        &state.query,
                        !state.case_sensitive,
                    )
                    .is_err()
                {
                    "Bad regex".to_string()
                } else {
                    "No matches".to_string()
                };
                ui.label(match_text);

                if ui.small_button("\u{25b2}").clicked() {
                    state.prev_match();
                }
                if ui.small_button("\u{25bc}").clicked() {
                    state.next_match();
                }
                // Toggles
                let cs_label = if state.case_sensitive {
                    "Aa\u{2713}"
                } else {
                    "Aa"
                };
                if ui.small_button(cs_label).clicked() {
                    state.case_sensitive = !state.case_sensitive;
                    state.recompute_matches(content.as_str());
                }
                let re_label = if state.use_regex { ".*\u{2713}" } else { ".*" };
                if ui.small_button(re_label).clicked() {
                    state.use_regex = !state.use_regex;
                    state.recompute_matches(content.as_str());
                }
                if ui
                    .small_button(
                        egui::RichText::new(egui_phosphor::regular::X)
                            .font(crate::editor::theme::icon_font_id(11.0)),
                    )
                    .clicked()
                {
                    state.close();
                }
            });

            // Replace row
            if state.replace_visible {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.replacement)
                            .hint_text(if state.use_regex {
                                "Replace with\u{2026} ($1 groups)"
                            } else {
                                "Replace with\u{2026}"
                            })
                            .desired_width(200.0),
                    );
                    if ui.small_button("Replace").clicked() && !state.matches.is_empty() {
                        *content = state.replace_current(content.as_str());
                        state.recompute_matches(content.as_str());
                    }
                    if ui.small_button("All").clicked() && !state.matches.is_empty() {
                        *content = state.replace_all(content.as_str());
                        state.recompute_matches(content.as_str());
                    }
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_literal_case_insensitive() {
        let mut state = FindReplaceState::default();
        state.query = "hello".to_string();
        state.recompute_matches("Hello world hello HELLO");
        assert_eq!(state.matches.len(), 3);
    }

    #[test]
    fn find_literal_case_sensitive() {
        let mut state = FindReplaceState::default();
        state.query = "Hello".to_string();
        state.case_sensitive = true;
        state.recompute_matches("Hello world hello HELLO");
        assert_eq!(state.matches.len(), 1);
    }

    #[test]
    fn find_whole_word() {
        let mut state = FindReplaceState::default();
        state.query = "he".to_string();
        state.whole_word = true;
        state.recompute_matches("he hello the he");
        assert_eq!(state.matches.len(), 2); // "he" at start and end
    }

    #[test]
    fn replace_all() {
        let mut state = FindReplaceState::default();
        state.query = "foo".to_string();
        state.replacement = "bar".to_string();
        state.recompute_matches("foo baz foo");
        let result = state.replace_all("foo baz foo");
        assert_eq!(result, "bar baz bar");
    }

    #[test]
    fn replace_current() {
        let mut state = FindReplaceState::default();
        state.query = "x".to_string();
        state.replacement = "Y".to_string();
        state.recompute_matches("axbxc");
        state.current_match = 1;
        let result = state.replace_current("axbxc");
        assert_eq!(result, "axbYc");
    }

    #[test]
    fn regex_digit_matches() {
        let mut state = FindReplaceState::default();
        state.use_regex = true;
        state.query = r"\d+".to_string();
        state.recompute_matches("a12 b345 c");
        assert_eq!(state.matches.len(), 2);
    }

    #[test]
    fn regex_alternation_replace_all() {
        let mut state = FindReplaceState::default();
        state.use_regex = true;
        state.query = "cat|dog".to_string();
        state.replacement = "pet".to_string();
        state.recompute_matches("a cat and a dog");
        let result = state.replace_all("a cat and a dog");
        assert_eq!(result, "a pet and a pet");
    }

    #[test]
    fn regex_replace_expands_capture_groups() {
        let mut state = FindReplaceState::default();
        state.use_regex = true;
        state.query = r"(\w+) (\w+)".to_string();
        state.replacement = "$2 $1".to_string();
        state.recompute_matches("hi there");
        assert_eq!(state.matches.len(), 1);
        assert_eq!(state.replace_all("hi there"), "there hi");
        // Braced form and $0 (whole match) work too.
        state.replacement = "[${0}] -> ${1}".to_string();
        assert_eq!(state.replace_all("hi there"), "[hi there] -> hi");
        // Replace-one uses the same expansion path.
        state.replacement = "$2!".to_string();
        assert_eq!(state.replace_current("hi there"), "there!");
    }

    #[test]
    fn literal_replace_keeps_dollar_verbatim() {
        let mut state = FindReplaceState::default();
        state.query = "foo".to_string();
        state.replacement = "$1".to_string();
        state.recompute_matches("foo foo");
        // No regex mode: `$1` is literal text, not a group reference.
        assert_eq!(state.replace_all("foo foo"), "$1 $1");
    }

    #[test]
    fn regex_invalid_yields_no_matches() {
        let mut state = FindReplaceState::default();
        state.use_regex = true;
        state.query = "(unclosed".to_string();
        state.recompute_matches("unclosed text");
        assert!(state.matches.is_empty());
    }

    #[test]
    fn char_index_of_byte_is_char_based_not_byte_based() {
        // 'é' and 'ö' are 2 bytes but 1 char each.
        let text = "héllo wörld";
        // 'w' starts at byte 7 (h=1,é=2,l=1,l=1,o=1,space=1) but char index 6.
        let byte_of_w = text.find('w').unwrap();
        assert_eq!(byte_of_w, 7);
        assert_eq!(char_index_of_byte(text, byte_of_w), 6);
        assert_eq!(char_index_of_byte(text, 0), 0);
        // A char boundary maps to the exact char index; the end maps to the
        // total char count.
        assert_eq!(char_index_of_byte(text, text.len()), text.chars().count());
        // Past the end clamps to the total char count (never panics).
        assert_eq!(char_index_of_byte(text, 999), text.chars().count());
    }
}
