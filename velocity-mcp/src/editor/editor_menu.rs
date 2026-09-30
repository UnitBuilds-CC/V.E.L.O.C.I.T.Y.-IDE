//! Editor right-click context menu: the action vocabulary plus the pure
//! text operations (char-offset splice / slice) the clipboard actions run
//! through. Keeping the multi-byte arithmetic here, isolated from egui, lets
//! the exact behaviour the handlers depend on be unit-tested directly.

use std::path::PathBuf;

/// One request collected from the editor's context menu during rendering and
/// executed by [`crate::editor::app::velocity_app::VelocityApp::run_editor_menu_action`]
/// once the buffer borrow that drew the menu is gone (the same collect-then-run
/// shape the explorer tree menu uses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorMenuAction {
    /// Copy the selection to the clipboard, then delete it from the buffer.
    Cut,
    /// Copy the selection to the clipboard, leaving the buffer untouched.
    Copy,
    /// Insert the system clipboard at the caret/selection (read at run time).
    Paste,
    /// Select the whole buffer.
    SelectAll,
    ToggleComment,
    FormatDocument,
    CodeActions,
    GoToDefinition,
    /// Open the semantic-rename overlay at the recorded cursor position.
    Rename,
    /// Put this file's absolute path on the clipboard.
    CopyPath(PathBuf),
}

/// Normalize a caret pair of char offsets: ordered and clamped into `len`.
pub fn normalize_range(a: usize, b: usize, len: usize) -> (usize, usize) {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    (start.min(len), end.min(len))
}

/// The characters in `[start, end)` of `content`'s *char* offsets (not bytes).
/// Range endpoints are normalized, so multi-byte text slices correctly.
pub fn slice_chars(content: &str, start: usize, end: usize) -> String {
    let (s, e) = normalize_range(start, end, content.chars().count());
    content.chars().skip(s).take(e - s).collect()
}

/// Replace the char range `[start, end)` with `text`, returning the new
/// content plus the resulting caret range that selects the inserted text
/// (collapsed to one point when `text` is empty).
pub fn splice(content: &str, start: usize, end: usize, text: &str) -> (String, usize, usize) {
    let (s, e) = normalize_range(start, end, content.chars().count());
    let mut out = String::with_capacity(content.len() + text.len());
    out.extend(content.chars().take(s));
    let insert_start = s;
    out.push_str(text);
    let insert_end = insert_start + text.chars().count();
    out.extend(content.chars().skip(e));
    (out, insert_start, insert_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_range_orders_and_clamps() {
        assert_eq!(normalize_range(3, 7, 100), (3, 7));
        assert_eq!(normalize_range(7, 3, 100), (3, 7));
        assert_eq!(normalize_range(5, 90, 40), (5, 40));
        assert_eq!(normalize_range(90, 50, 10), (10, 10));
    }

    #[test]
    fn slice_counts_characters_not_bytes() {
        // 'é' and '中' are multi-byte in UTF-8 but one char each.
        let text = "héllo 中";
        assert_eq!(slice_chars(text, 1, 2), "é");
        assert_eq!(slice_chars(text, 6, 7), "中");
        // Reversed endpoints still yield the ordered slice.
        assert_eq!(slice_chars(text, 5, 2), "llo");
    }

    #[test]
    fn splice_delete_returns_collapsed_caret() {
        let (out, s, e) = splice("hello world", 0, 6, "");
        assert_eq!(out, "world");
        assert_eq!((s, e), (0, 0));
    }

    #[test]
    fn splice_insert_reports_the_inserted_char_range() {
        let (out, s, e) = splice("ab", 1, 1, "中x");
        assert_eq!(out, "a中xb");
        assert_eq!((s, e), (1, 3));
    }

    #[test]
    fn splice_replaces_selection_with_multi_byte_text() {
        let (out, s, e) = splice("one two three", 4, 7, "ünicode");
        assert_eq!(out, "one ünicode three");
        assert_eq!(slice_chars(&out, s, e), "ünicode");
    }
}
