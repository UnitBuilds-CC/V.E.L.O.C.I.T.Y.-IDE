//! Whole-line editing operations (duplicate, delete, move up/down).
//!
//! These are the staple editor actions every competitor ships (VS Code,
//! Cursor, Claude Code) and which the default keymap already advertises
//! (`edit.duplicate_line`, `edit.delete_line`, `edit.move_line_up`,
//! `edit.move_line_down`). The transforms are modelled as pure functions over
//! the document text and the caret's *char* offset so they are exhaustively
//! unit-testable without an egui context. egui's `CCursor` indexes by `char`,
//! so char offsets (not bytes) are the currency throughout — this keeps the
//! behaviour correct for text containing multi-byte or combining characters.

/// A whole-line operation applied at the caret's line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOp {
    /// Copy the caret's line below itself (caret lands on the copy).
    Duplicate,
    /// Remove the caret's line entirely.
    Delete,
    /// Swap the caret's line with the one above it.
    MoveUp,
    /// Swap the caret's line with the one below it.
    MoveDown,
}

/// Split points for a caret char offset: the line index and the column (chars
/// from the start of that line) into `lines` (which came from
/// `content.split('\n')`). A column past the line's end clamps to the end; an
/// offset at/after EOF clamps to the last line.
fn line_col_of(lines: &[&str], cursor: usize) -> (usize, usize) {
    let mut acc = 0usize; // char offset where the current line starts
    for (idx, line) in lines.iter().enumerate() {
        let len = line.chars().count();
        if cursor < acc + len + 1 || idx + 1 == lines.len() {
            let col = cursor.saturating_sub(acc).min(len);
            return (idx, col);
        }
        acc += len + 1; // +1 for the '\n' separator
    }
    (0, 0)
}

/// Char offset of the start of `line_idx` (plus `col`), given `lines`.
fn offset_of(lines: &[&str], line_idx: usize, col: usize) -> usize {
    let mut acc = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        if idx == line_idx {
            let len = line.chars().count();
            return acc + col.min(len);
        }
        acc += line.chars().count() + 1;
    }
    acc
}

/// Rebuild the document from line segments. `split('\n')` keeps a trailing
/// empty segment when the text ends in a newline, so a plain `join("\n")`
/// reproduces the original bytes exactly (including any final newline).
fn rebuild(lines: &[&str]) -> String {
    lines.join("\n")
}

/// 0-based char offset of a 0-based `(line, col)` in `content`, clamped to the
/// line's end. Used to convert the caret the app tracks (line/column) into the
/// char offset the pure ops operate on.
pub fn offset_of_line_col(content: &str, line: usize, col: usize) -> usize {
    let lines: Vec<&str> = content.split('\n').collect();
    let line = line.min(lines.len().saturating_sub(1));
    offset_of(&lines, line, col)
}

/// 1-based line number containing the char `offset`, matching the 1-based
/// convention the editor's `pending_cursor_line` uses.
pub fn line_number_of_offset(content: &str, offset: usize) -> usize {
    let lines: Vec<&str> = content.split('\n').collect();
    line_col_of(&lines, offset).0 + 1
}

/// 0-based `(line, col)` of a 0-based **char** offset in `content` — the
/// inverse of [`offset_of_line_col`]. Used to turn a click position resolved
/// through the editor's laid-out galley into the line/column LSP position
/// requests expect. Clamps the same way: an offset at/after EOF lands on the
/// last line.
pub fn line_col_of_offset(content: &str, offset: usize) -> (usize, usize) {
    let lines: Vec<&str> = content.split('\n').collect();
    line_col_of(&lines, offset)
}

/// The 0-based, inclusive `(first, last)` line indices touched by the
/// char-offset range `[start, end]` (order-insensitive). Used to turn an
/// egui selection (a pair of char offsets) into the block of lines an
/// indent/dedent operation should rewrite.
pub fn line_range_of(content: &str, start: usize, end: usize) -> (usize, usize) {
    let lines: Vec<&str> = content.split('\n').collect();
    let (a, _) = line_col_of(&lines, start.min(end));
    let (b, _) = line_col_of(&lines, start.max(end));
    (a, b)
}

/// The next line index whose diff mark is non-zero — i.e. a line that
/// differs from the buffer's last-saved baseline (see
/// [`crate::editor::buffer::EditorBuffer::refresh_diff_marks`]). Scans
/// forward (or backward when `forward` is false) starting strictly after
/// `from_line` and wraps around the ends, so repeated presses cycle through
/// every change. `from_line` itself is a legal target once the scan comes
/// back around to it; `None` means nothing is marked (clean buffer).
pub fn next_changed_line(marks: &[u8], from_line: usize, forward: bool) -> Option<usize> {
    let n = marks.len();
    if n == 0 {
        return None;
    }
    let start = from_line % n;
    for step in 1..=n {
        let idx = if forward {
            (start + step) % n
        } else {
            (start + n - step) % n
        };
        if marks[idx] != 0 {
            return Some(idx);
        }
    }
    None
}

/// Apply `op` to `content` with the caret at char offset `cursor`, returning
/// the new text and the new caret char offset. Returns `None` when the
/// operation is a no-op in the current position (moving up from the first
/// line, moving down from the last, deleting an already-empty document) so the
/// caller can leave the buffer untouched rather than rewrite it identically.
pub fn apply_line_op(content: &str, op: LineOp, cursor: usize) -> Option<(String, usize)> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    let (li, col) = line_col_of(&lines, cursor);
    match op {
        LineOp::Duplicate => {
            // Insert a copy of the caret line directly after it; caret follows
            // onto the copy at the same column. Duplicating the phantom last
            // line of a trailing-newline document is a no-op.
            if li + 1 >= lines.len() && lines[li].is_empty() {
                return None;
            }
            let clone = lines[li];
            lines.insert(li + 1, clone);
            Some((rebuild(&lines), offset_of(&lines, li + 1, col)))
        }
        LineOp::Delete => {
            // Remove the caret line. A document that is a single empty line
            // has nothing to delete.
            if lines.len() == 1 {
                return None;
            }
            // When deleting the last (phantom) line of a trailing-newline doc,
            // drop the newline that ends the previous line instead.
            let target = if li < lines.len() {
                li
            } else {
                lines.len() - 1
            };
            lines.remove(target);
            let new_line = target.min(lines.len() - 1);
            Some((rebuild(&lines), offset_of(&lines, new_line, col)))
        }
        LineOp::MoveUp => {
            if li == 0 {
                return None;
            }
            lines.swap(li, li - 1);
            Some((rebuild(&lines), offset_of(&lines, li - 1, col)))
        }
        LineOp::MoveDown => {
            // The final phantom segment (from a trailing newline) is not a real
            // line; moving down into it would drop a newline, so bound by it.
            let last_move = lines.len().saturating_sub(1);
            if li + 1 >= lines.len() || (li + 1 == last_move && lines[last_move].is_empty()) {
                return None;
            }
            lines.swap(li, li + 1);
            Some((rebuild(&lines), offset_of(&lines, li + 1, col)))
        }
    }
}

/// The line-comment token for a file extension, or `None` when we do not know
/// a *line* comment for it. Block-comment-only syntaxes (HTML, CSS, JS/TS
/// doc-comments) and unknown types deliberately return `None` rather than emit
/// a token that would corrupt the file — the caller treats `None` as a no-op.
pub fn line_comment_token(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "cs" | "java" | "js" | "jsx"
        | "mjs" | "cjs" | "ts" | "tsx" | "go" | "swift" | "kt" | "kts" | "php" | "dart"
        | "scala" | "groovy" => Some("//"),
        "py" | "rb" | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" | "conf" | "ini" | "r"
        | "pl" | "pm" | "ex" | "exs" => Some("#"),
        "sql" | "lua" => Some("--"),
        _ => None,
    }
}

/// Toggle a line comment (`//`, `#`, …, chosen by the caller via
/// [`line_comment_token`]) across the char-offset range `[start, end]`; a
/// zero-length range (caret) targets that single line. If *every* non-blank
/// line in the affected block already begins with `token` (after its leading
/// whitespace) the token — plus one optional following space — is stripped;
/// otherwise the token is inserted after each non-blank line's leading
/// whitespace so indentation is preserved. Blank lines are never annotated and
/// never influence the comment/uncomment decision, so a mixed block is treated
/// as "comment everything" (matching VS Code / Cursor). Returns the new text
/// and the new selection `[sel_start, sel_end]` (char offsets) spanning the
/// affected block, or `None` when there is nothing to change (empty document or
/// an all-blank block).
pub fn toggle_line_comment(
    content: &str,
    token: &str,
    start: usize,
    end: usize,
) -> Option<(String, usize, usize)> {
    let lines: Vec<&str> = content.split('\n').collect();
    if lines.is_empty() {
        return None;
    }
    let (first, _) = line_col_of(&lines, start.min(end));
    let (last, _) = line_col_of(&lines, start.max(end));

    // Decide comment-vs-uncomment from the non-blank lines in the block.
    let mut all_commented = true;
    let mut any_nonblank = false;
    for idx in first..=last {
        let trimmed = lines[idx].trim_start();
        if trimmed.is_empty() {
            continue; // blank lines don't influence the decision
        }
        any_nonblank = true;
        if !trimmed.starts_with(token) {
            all_commented = false;
            break;
        }
    }
    if !any_nonblank {
        return None; // the whole block is blank: nothing to do
    }

    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    for idx in first..=last {
        // Own the line so we can reassign `new_lines[idx]` without a borrow
        // conflict. Leading whitespace and `token` are ASCII, so slicing the
        // original bytes at `indent`/`token.len()` lands on char boundaries.
        let line = new_lines[idx].clone();
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        if rest.is_empty() {
            continue; // leave blank lines untouched
        }
        if all_commented {
            let after = &rest[token.len()..];
            new_lines[idx] = format!(
                "{}{}",
                &line[..indent],
                after.strip_prefix(' ').unwrap_or(after)
            );
        } else {
            new_lines[idx] = format!("{}{} {}", &line[..indent], token, rest);
        }
    }

    let joined: Vec<&str> = new_lines.iter().map(|s| s.as_str()).collect();
    let out = rebuild(&joined);
    let last_len = new_lines[last].chars().count();
    let sel_start = offset_of(&joined, first, 0);
    let sel_end = offset_of(&joined, last, last_len);
    Some((out, sel_start, sel_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn char_off(s: &str, needle: &str) -> usize {
        s.find(needle).unwrap_or(usize::MAX)
    }

    #[test]
    fn duplicate_copies_line_below_and_moves_caret_onto_copy() {
        let text = "aa\nbb\ncc";
        // Caret somewhere on "bb" (offset 3 = start of "bb").
        let (out, cur) = apply_line_op(text, LineOp::Duplicate, 4).unwrap();
        assert_eq!(out, "aa\nbb\nbb\ncc");
        // Caret now on the second "bb" (offset of its 'a' char + 1).
        assert_eq!(cur, char_off(&out, "bb\ncc") + 1);
    }

    #[test]
    fn duplicate_preserves_trailing_newline_document() {
        let text = "aa\nbb\n"; // ends with newline → split → ["aa","bb",""]
                               // Caret on "aa".
        let (out, _cur) = apply_line_op(text, LineOp::Duplicate, 1).unwrap();
        assert_eq!(out, "aa\naa\nbb\n");
    }

    #[test]
    fn duplicate_on_phantom_last_line_is_a_noop() {
        // Caret past the final newline (on the phantom empty line).
        assert!(apply_line_op("aa\nbb\n", LineOp::Duplicate, 6).is_none());
    }

    #[test]
    fn delete_removes_whole_line() {
        let text = "aa\nbb\ncc";
        let (out, cur) = apply_line_op(text, LineOp::Delete, 4).unwrap(); // on "bb" col 1
        assert_eq!(out, "aa\ncc");
        // The caret keeps its column onto the line that shifted up: "cc" col 1
        // == char offset 3 (start of "cc") + 1.
        assert_eq!(cur, 4);
    }

    #[test]
    fn delete_last_line_of_trailing_newline_drops_its_newline() {
        // "aa\nbb\n" → caret on the phantom last line (offset 6, at EOF);
        // deleting it removes the empty trailing segment, yielding "aa\nbb".
        let (out, _cur) = apply_line_op("aa\nbb\n", LineOp::Delete, 6).unwrap();
        assert_eq!(out, "aa\nbb");
    }

    #[test]
    fn delete_single_empty_document_is_a_noop() {
        assert!(apply_line_op("", LineOp::Delete, 0).is_none());
        assert!(apply_line_op("only", LineOp::Delete, 2).is_none());
    }

    #[test]
    fn move_up_swaps_with_previous_line() {
        let text = "aa\nbb\ncc";
        let (out, cur) = apply_line_op(text, LineOp::MoveUp, 4).unwrap(); // "bb" col 1 up
        assert_eq!(out, "bb\naa\ncc");
        // Caret rides the moved line to the top, preserving its column (1).
        assert_eq!(cur, 1);
    }

    #[test]
    fn move_up_on_first_line_is_a_noop() {
        assert!(apply_line_op("aa\nbb", LineOp::MoveUp, 1).is_none());
    }

    #[test]
    fn move_down_swaps_with_next_line() {
        let text = "aa\nbb\ncc";
        let (out, cur) = apply_line_op(text, LineOp::MoveDown, 1).unwrap(); // "aa" col 1 down
        assert_eq!(out, "bb\naa\ncc");
        // Moved "aa" now starts at offset 3; the caret keeps column 1.
        assert_eq!(cur, 4);
    }

    #[test]
    fn move_down_on_last_real_line_is_a_noop() {
        assert!(apply_line_op("aa\nbb", LineOp::MoveDown, 3).is_none());
        // Trailing-newline document: "cc" is the last real line.
        assert!(apply_line_op("aa\nbb\n", LineOp::MoveDown, 4).is_none());
    }

    #[test]
    fn move_preserves_column_within_moved_line() {
        // Moving a line under/over another preserves the caret's column, which
        // is measured in chars, so the offset reflects the new prefix length.
        let text = "long line here\nx";
        // Caret at column 5 of "long line here" (offset 5).
        let (out, cur) = apply_line_op(text, LineOp::MoveDown, 5).unwrap();
        assert_eq!(out, "x\nlong line here");
        // "x\n" is 2 chars, so the moved line's column 5 lands at offset 7.
        assert_eq!(cur, 7);
    }

    #[test]
    fn operations_are_char_based_not_byte_based() {
        // Multi-byte chars: the caret column is measured in chars, so the
        // duplicated line's caret offset must reflect char count of the prefix.
        let text = "héllo\nwörld"; // 'é','ö' are 2 bytes but 1 char each
        let caret = 1; // within "héllo", after 'h'
        let (out, cur) = apply_line_op(text, LineOp::Duplicate, caret).unwrap();
        assert_eq!(out, "héllo\nhéllo\nwörld");
        // Caret on the copy at char column 1 → char offset = len("héllo\n")+1 = 7.
        assert_eq!(cur, 7);
    }

    #[test]
    fn offset_helpers_round_trip_through_line_col() {
        let text = "aa\nbbbb\ncc";
        // Line 1 ("bbbb"), col 2 → char offset = 3 (past "aa\n") + 2 = 5.
        assert_eq!(offset_of_line_col(text, 1, 2), 5);
        assert_eq!(line_number_of_offset(text, 5), 2);
        // Column past a line's end clamps to the line's end.
        assert_eq!(offset_of_line_col(text, 0, 99), 2); // end of "aa"
                                                        // Line index past EOF clamps to the last line ("cc" starts at offset 8).
        assert_eq!(offset_of_line_col(text, 99, 0), 8); // start of "cc"
    }

    #[test]
    fn line_col_of_offset_inverts_offset_of_line_col() {
        let text = "aa\nbbbb\ncc";
        assert_eq!(line_col_of_offset(text, 5), (1, 2));
        assert_eq!(line_col_of_offset(text, 0), (0, 0));
        assert_eq!(line_col_of_offset(text, 3), (1, 0));
        // Round trip: every (line, col) comes back from its own offset.
        for (line, col) in [(0usize, 0usize), (0, 2), (1, 1), (1, 4), (2, 2)] {
            let off = offset_of_line_col(text, line, col);
            assert_eq!(line_col_of_offset(text, off), (line, col));
        }
        // An offset at/after EOF clamps to the last line.
        assert_eq!(line_col_of_offset(text, 99), (2, 2));
    }

    // ── toggle_line_comment ────────────────────────────────────────────────

    #[test]
    fn comment_token_map_covers_common_line_comment_languages() {
        assert_eq!(line_comment_token("rs"), Some("//"));
        assert_eq!(line_comment_token("cpp"), Some("//"));
        assert_eq!(line_comment_token("py"), Some("#"));
        assert_eq!(line_comment_token("toml"), Some("#"));
        assert_eq!(line_comment_token("sql"), Some("--"));
        // Block-comment-only / unknown types yield no line token.
        assert_eq!(line_comment_token("html"), None);
        assert_eq!(line_comment_token("css"), None);
        assert_eq!(line_comment_token("txt"), None);
    }

    #[test]
    fn comment_caret_line_inserts_token_after_indent() {
        let text = "    let x = 1;\n    let y = 2;";
        // Caret on the second line (offset within it), zero-length range.
        let caret = 16; // inside "    let y = 2;"
        let (out, s, e) = toggle_line_comment(text, "//", caret, caret).unwrap();
        assert_eq!(out, "    let x = 1;\n    // let y = 2;");
        // Selection spans the commented line from its start to its end.
        assert_eq!(s, 15);
        assert_eq!(e, s + "    // let y = 2;".chars().count());
    }

    #[test]
    fn uncomment_strips_token_and_one_space_preserving_indent() {
        let text = "    // let x = 1;";
        let (out, ..) = toggle_line_comment(text, "//", 4, 4).unwrap();
        assert_eq!(out, "    let x = 1;");
    }

    #[test]
    fn uncomment_without_space_after_token_strips_only_the_token() {
        let text = "//let x = 1;";
        let (out, ..) = toggle_line_comment(text, "//", 0, 0).unwrap();
        assert_eq!(out, "let x = 1;");
    }

    #[test]
    fn comment_spans_a_multiline_selection() {
        let text = "aa\nbb\ncc";
        let end = text.chars().count();
        let (out, s, e) = toggle_line_comment(text, "//", 0, end).unwrap();
        assert_eq!(out, "// aa\n// bb\n// cc");
        assert_eq!(s, 0);
        assert_eq!(e, out.chars().count());
    }

    #[test]
    fn already_commented_block_is_uncommented() {
        let text = "// aa\n// bb";
        let end = text.chars().count();
        let (out, ..) = toggle_line_comment(text, "//", 0, end).unwrap();
        assert_eq!(out, "aa\nbb");
    }

    #[test]
    fn blank_lines_are_skipped_and_do_not_change_the_decision() {
        let text = "aa\n\ncc";
        let (out, ..) = toggle_line_comment(text, "//", 0, 5).unwrap();
        assert_eq!(out, "// aa\n\n// cc");
    }

    #[test]
    fn mixed_block_comments_every_nonblank_line() {
        // One line already commented, one not → not "all commented", so the
        // whole block is (re-)commented, matching editor behaviour.
        let text = "// aa\nbb";
        let end = text.chars().count();
        let (out, ..) = toggle_line_comment(text, "//", 0, end).unwrap();
        assert_eq!(out, "// // aa\n// bb");
    }

    #[test]
    fn all_blank_or_empty_document_is_a_noop() {
        assert!(toggle_line_comment("", "//", 0, 0).is_none());
        assert!(toggle_line_comment("\n  \n\t", "//", 0, 5).is_none());
    }

    #[test]
    fn comment_is_reversible_and_char_based_for_unicode() {
        // Multi-byte content: toggling on then off returns the original text.
        let text = "héllo wörld";
        let end = text.chars().count();
        let (commented, ..) = toggle_line_comment(text, "//", 0, end).unwrap();
        assert_eq!(commented, "// héllo wörld");
        let (back, ..) =
            toggle_line_comment(&commented, "//", 0, commented.chars().count()).unwrap();
        assert_eq!(back, text);
    }

    #[test]
    fn line_range_of_maps_char_offsets_to_line_indices() {
        let text = "aa\nbb\ncc";
        // Caret (zero-length range) on the middle line → that single line.
        assert_eq!(line_range_of(text, 4, 4), (1, 1));
        // A selection from inside line 0 to inside line 2 touches lines 0..2.
        assert_eq!(line_range_of(text, 1, 6), (0, 2));
        // Order-insensitive: reversed endpoints give the same span.
        assert_eq!(line_range_of(text, 6, 1), (0, 2));
    }
}

#[cfg(test)]
mod next_changed_line_tests {
    use super::*;

    #[test]
    fn forward_scan_hits_the_next_marked_line() {
        let marks = [0u8, 1, 0, 2, 0];
        assert_eq!(next_changed_line(&marks, 0, true), Some(1));
        assert_eq!(next_changed_line(&marks, 2, true), Some(3));
        // The start line is skipped even when itself marked: scanning is
        // strictly *after* `from_line`.
        assert_eq!(next_changed_line(&marks, 1, true), Some(3));
    }

    #[test]
    fn forward_scan_wraps_to_the_first_change() {
        let marks = [0u8, 1, 0, 2, 0];
        assert_eq!(next_changed_line(&marks, 4, true), Some(1));
        assert_eq!(next_changed_line(&marks, 3, true), Some(1));
    }

    #[test]
    fn backward_scan_wraps_and_skips_the_start_line() {
        let marks = [0u8, 1, 0, 2, 0];
        assert_eq!(next_changed_line(&marks, 3, false), Some(1));
        assert_eq!(next_changed_line(&marks, 1, false), Some(3)); // wraps
        assert_eq!(next_changed_line(&marks, 2, false), Some(1));
    }

    #[test]
    fn marked_start_line_is_reachable_via_wrap() {
        // Only line 0 is marked: the scan skips it at first, wraps around the
        // whole buffer, and lands back on it.
        let marks = [1u8, 0, 0];
        assert_eq!(next_changed_line(&marks, 0, true), Some(0));
        assert_eq!(next_changed_line(&marks, 2, true), Some(0));
        assert_eq!(next_changed_line(&marks, 0, false), Some(0));
    }

    #[test]
    fn clean_and_empty_marks_yield_none() {
        assert_eq!(next_changed_line(&[0u8, 0, 0], 0, true), None);
        assert_eq!(next_changed_line(&[0u8, 0, 0], 1, false), None);
        assert_eq!(next_changed_line(&[], 0, true), None);
    }

    #[test]
    fn out_of_range_start_normalizes_into_the_scan() {
        let marks = [0u8, 0, 1];
        // Buffer shrank under a stale caret line: 100 % 3 = 1, so the forward
        // scan starts after line 1 and lands on the marked line 2.
        assert_eq!(next_changed_line(&marks, 100, true), Some(2));
        // Backward from the same normalized start wraps to line 2 as well.
        assert_eq!(next_changed_line(&marks, 100, false), Some(2));
    }
}
