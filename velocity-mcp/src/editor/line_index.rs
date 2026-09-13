//! Incremental line index for O(log n) line↔offset lookups.
//!
//! Maintains a sorted `Vec<usize>` of byte offsets where each line begins.
//! On edits only the affected range is rescanned — making single-line edits
//! O(log n) instead of O(n) full rebuilds.

/// Incremental line index mapping byte offsets ↔ line numbers.
///
/// `line_starts[i]` is the byte offset where line `i` begins.  Line 0 always
/// starts at byte 0 (even for empty content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offset of each line start (sorted, always begins with 0).
    line_starts: Vec<usize>,
    /// Total content length in bytes.
    content_len: usize,
}

impl LineIndex {
    /// Build an initial index by scanning `text` for newlines.  O(n).
    pub fn new(text: &str) -> Self {
        let mut starts = Vec::with_capacity(64.min(text.len() / 20 + 1));
        starts.push(0);
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self {
            line_starts: starts,
            content_len: text.len(),
        }
    }

    /// Incremental update after an edit.
    ///
    /// * `start`     – byte offset where the edit begins
    /// * `old_end`   – byte offset where the old text ended (exclusive)
    /// * `new_end`   – byte offset where the new text ends (exclusive);
    ///   equals `start + replacement.len()`
    /// * `replacement` – the new text that replaced `text[start..old_end]`
    ///
    /// Instead of rebuilding the entire index this:
    /// 1. Binary-searches for the affected line range.
    /// 2. Removes old line-starts within `[start ..= old_end]`.
    /// 3. Scans only `replacement` for new line-starts.
    /// 4. Shifts all line-starts after the edit by the delta.
    ///
    /// For single-line edits (the common case) this is O(log n).
    pub fn update(&mut self, start: usize, old_end: usize, new_end: usize, replacement: &str) {
        let delta = new_end as isize - old_end as isize;

        // Fast path: zero-length delta and no newlines in replacement → just
        // shift is unnecessary; but we still need to handle line-start
        // additions/removals inside the replacement, so no special fast path.

        // ── 1. Locate affected range ────────────────────────────────────
        // `before_end`: first index where line_starts[i] >= start
        let before_end = self.line_starts.partition_point(|&s| s < start);
        // `after_idx`:  first index where line_starts[i] > old_end
        let after_idx = self.line_starts.partition_point(|&s| s <= old_end);

        // Was `start` itself a line-start?  (byte before it is '\n' or start == 0)
        let start_is_ls =
            before_end < self.line_starts.len() && self.line_starts[before_end] == start;

        // ── 2. Scan replacement for new line-starts ─────────────────────
        let new_lines: Vec<usize> = replacement
            .bytes()
            .enumerate()
            .filter_map(|(i, b)| if b == b'\n' { Some(start + i + 1) } else { None })
            .collect();

        // ── 3. Build the new line_starts ────────────────────────────────
        let kept_before = before_end;
        let kept_start = if start_is_ls { 1 } else { 0 };
        let kept_after = self.line_starts.len() - after_idx;
        let cap = kept_before + kept_start + new_lines.len() + kept_after;
        let mut result = Vec::with_capacity(cap);

        // (a) Lines entirely before the edit — unchanged.
        result.extend_from_slice(&self.line_starts[..before_end]);

        // (b) The edit point itself, if it was a line boundary.
        if start_is_ls {
            result.push(start);
        }

        // (c) New line boundaries introduced by the replacement text.
        result.extend_from_slice(&new_lines);

        // (d) Lines after the edit — shifted by delta.
        for &ls in &self.line_starts[after_idx..] {
            result.push((ls as isize + delta) as usize);
        }

        self.line_starts = result;
        self.content_len = (self.content_len as isize + delta) as usize;
    }

    /// Map a byte offset to its 0-based line number via binary search.  O(log n).
    ///
    /// Returns `line_count() - 1` if `offset >= content_len`.
    pub fn line_of_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(line) => line.saturating_sub(1),
        }
    }

    /// Map a 0-based line number to its starting byte offset.  O(1).
    ///
    /// Returns `content_len` if `line >= line_count()`.
    pub fn offset_of_line(&self, line: usize) -> usize {
        self.line_starts
            .get(line)
            .copied()
            .unwrap_or(self.content_len)
    }

    /// Total number of lines.  O(1).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Whether the indexed content is empty (zero bytes).  O(1).
    pub fn is_empty(&self) -> bool {
        self.content_len == 0
    }

    /// Total content length in bytes.  O(1).
    pub fn content_len(&self) -> usize {
        self.content_len
    }

    /// Borrow the raw line-starts slice (useful for debugging / integration).
    pub fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────

    /// Naive reference implementation — rebuilds from scratch every time.
    fn naive_line_starts(text: &str) -> Vec<usize> {
        let mut s = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                s.push(i + 1);
            }
        }
        s
    }

    /// Apply an edit to a string and return the new string.
    fn apply_edit(text: &str, start: usize, old_end: usize, replacement: &str) -> String {
        let mut s = String::with_capacity(text.len() - (old_end - start) + replacement.len());
        s.push_str(&text[..start]);
        s.push_str(replacement);
        s.push_str(&text[old_end..]);
        s
    }

    // ── construction ────────────────────────────────────────────────────

    #[test]
    fn empty_text() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_starts(), &[0]);
        assert_eq!(idx.line_count(), 1);
        assert!(idx.is_empty());
        assert_eq!(idx.content_len(), 0);
    }

    #[test]
    fn single_line_no_newline() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_starts(), &[0]);
        assert_eq!(idx.line_count(), 1);
        assert!(!idx.is_empty());
    }

    #[test]
    fn single_line_with_trailing_newline() {
        let idx = LineIndex::new("hello\n");
        assert_eq!(idx.line_starts(), &[0, 6]);
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn multi_line() {
        let idx = LineIndex::new("abc\ndef\nghi");
        assert_eq!(idx.line_starts(), &[0, 4, 8]);
        assert_eq!(idx.line_count(), 3);
    }

    #[test]
    fn multi_line_trailing_newline() {
        let idx = LineIndex::new("abc\ndef\n");
        assert_eq!(idx.line_starts(), &[0, 4, 8]);
        assert_eq!(idx.line_count(), 3);
    }

    #[test]
    fn windows_line_endings() {
        // \r\n — only \n triggers a new line start
        let idx = LineIndex::new("abc\r\ndef\r\n");
        assert_eq!(idx.line_starts(), &[0, 5, 10]);
    }

    // ── line_of_offset / offset_of_line ─────────────────────────────────

    #[test]
    fn line_of_offset_basic() {
        let idx = LineIndex::new("abc\ndef\nghi");
        assert_eq!(idx.line_of_offset(0), 0);
        assert_eq!(idx.line_of_offset(3), 0);
        assert_eq!(idx.line_of_offset(4), 1);
        assert_eq!(idx.line_of_offset(7), 1);
        assert_eq!(idx.line_of_offset(8), 2);
        assert_eq!(idx.line_of_offset(10), 2); // past end → last line
    }

    #[test]
    fn offset_of_line_basic() {
        let idx = LineIndex::new("abc\ndef\nghi");
        assert_eq!(idx.offset_of_line(0), 0);
        assert_eq!(idx.offset_of_line(1), 4);
        assert_eq!(idx.offset_of_line(2), 8);
        assert_eq!(idx.offset_of_line(3), 11); // past end → content_len
    }

    #[test]
    fn roundtrip_offset_line() {
        let text = "hello\nworld\nfoo\nbar";
        let idx = LineIndex::new(text);
        for offset in 0..text.len() {
            let line = idx.line_of_offset(offset);
            assert!(idx.offset_of_line(line) <= offset);
        }
    }

    // ── incremental update ──────────────────────────────────────────────

    #[test]
    fn insert_char_in_single_line() {
        let text = "abcdef";
        let mut idx = LineIndex::new(text);
        // Insert 'X' at offset 3
        let new_text = apply_edit(text, 3, 3, "X");
        let new_end = 3 + 1;
        idx.update(3, 3, new_end, "X");
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn insert_newline() {
        let text = "abcdef";
        let mut idx = LineIndex::new(text);
        let repl = "\n";
        let new_text = apply_edit(text, 3, 3, repl);
        idx.update(3, 3, 3 + repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn insert_multiple_newlines() {
        let text = "abcdef";
        let mut idx = LineIndex::new(text);
        let repl = "\n\n\n";
        let new_text = apply_edit(text, 2, 2, repl);
        idx.update(2, 2, 2 + repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn delete_single_char() {
        let text = "abcdef";
        let mut idx = LineIndex::new(text);
        let new_text = apply_edit(text, 2, 3, "");
        idx.update(2, 3, 2, "");
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn delete_across_lines() {
        let text = "abc\ndef\nghi";
        let mut idx = LineIndex::new(text);
        // Delete "c\nd" (offsets 2..6)
        let new_text = apply_edit(text, 2, 6, "");
        idx.update(2, 6, 2, "");
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn delete_entire_line() {
        let text = "abc\ndef\nghi";
        let mut idx = LineIndex::new(text);
        // Delete "def\n" (offsets 4..8)
        let new_text = apply_edit(text, 4, 8, "");
        idx.update(4, 8, 4, "");
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn replace_spanning_multiple_lines() {
        let text = "aaa\nbbb\nccc\nddd\neee";
        let mut idx = LineIndex::new(text);
        // Replace "bbb\nccc\n" (offsets 4..12) with "XXX\nYYY\nZZZ"
        let repl = "XXX\nYYY\nZZZ";
        let new_text = apply_edit(text, 4, 12, repl);
        idx.update(4, 12, 4 + repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn replace_with_fewer_lines() {
        let text = "aaa\nbbb\nccc\nddd";
        let mut idx = LineIndex::new(text);
        // Replace "aaa\nbbb\nccc\n" with "X"
        let repl = "X";
        let new_text = apply_edit(text, 0, 12, repl);
        idx.update(0, 12, repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn replace_with_more_lines() {
        let text = "abc";
        let mut idx = LineIndex::new(text);
        let repl = "x\ny\nz\n";
        let new_text = apply_edit(text, 0, 3, repl);
        idx.update(0, 3, repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn edit_at_start_insert() {
        let text = "abc\ndef";
        let mut idx = LineIndex::new(text);
        let repl = "xy\n";
        let new_text = apply_edit(text, 0, 0, repl);
        idx.update(0, 0, repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn edit_at_end_append() {
        let text = "abc\ndef";
        let mut idx = LineIndex::new(text);
        let repl = "\nghi";
        let new_text = apply_edit(text, text.len(), text.len(), repl);
        idx.update(text.len(), text.len(), text.len() + repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.content_len(), new_text.len());
    }

    #[test]
    fn delete_newline_merges_lines() {
        let text = "abc\ndef";
        let mut idx = LineIndex::new(text);
        // Delete the '\n' at offset 3
        let new_text = apply_edit(text, 3, 4, "");
        idx.update(3, 4, 3, "");
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn replace_newline_with_text() {
        let text = "abc\ndef";
        let mut idx = LineIndex::new(text);
        // Replace '\n' with "XY"
        let repl = "XY";
        let new_text = apply_edit(text, 3, 4, repl);
        idx.update(3, 4, 3 + repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn replace_text_with_newline() {
        let text = "abcdef";
        let mut idx = LineIndex::new(text);
        // Replace "cd" with "\n"
        let repl = "\n";
        let new_text = apply_edit(text, 2, 4, repl);
        idx.update(2, 4, 2 + repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn clear_all_content() {
        let text = "abc\ndef\nghi";
        let mut idx = LineIndex::new(text);
        let new_text = apply_edit(text, 0, text.len(), "");
        idx.update(0, text.len(), 0, "");
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.line_count(), 1);
        assert!(idx.is_empty());
    }

    #[test]
    fn insert_into_empty() {
        let text = "";
        let mut idx = LineIndex::new(text);
        let repl = "hello\nworld";
        let new_text = apply_edit(text, 0, 0, repl);
        idx.update(0, 0, repl.len(), repl);
        assert_eq!(idx.line_starts(), &naive_line_starts(&new_text));
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn no_op_edit() {
        let text = "abc\ndef\nghi";
        let mut idx = LineIndex::new(text);
        let original_starts = idx.line_starts().to_vec();
        // Zero-length edit at offset 2 with empty replacement
        idx.update(2, 2, 2, "");
        assert_eq!(idx.line_starts(), &original_starts);
    }

    #[test]
    fn line_of_offset_after_edit() {
        let text = "abc\ndef\nghi";
        let mut idx = LineIndex::new(text);
        let repl = "XY\nZ";
        let _new_text = apply_edit(text, 4, 7, repl); // replace "def" with "XY\nZ"
        idx.update(4, 7, 4 + repl.len(), repl);
        // new_text = "abc\nXY\nZ\nghi"
        assert_eq!(idx.line_of_offset(0), 0); // 'a' → line 0
        assert_eq!(idx.line_of_offset(4), 1); // 'X' → line 1
        assert_eq!(idx.line_of_offset(7), 2); // 'Z' → line 2
        assert_eq!(idx.line_of_offset(9), 3); // 'g' → line 3
    }

    #[test]
    fn offset_of_line_after_edit() {
        let text = "abc\ndef\nghi";
        let mut idx = LineIndex::new(text);
        let repl = "XX\nYY\nZZ";
        let _new_text = apply_edit(text, 4, 7, repl);
        idx.update(4, 7, 4 + repl.len(), repl);
        // new_text = "abc\nXX\nYY\nZZ\nghi"
        assert_eq!(idx.offset_of_line(0), 0);
        assert_eq!(idx.offset_of_line(1), 4);
        assert_eq!(idx.offset_of_line(2), 7);
        assert_eq!(idx.offset_of_line(3), 10);
        assert_eq!(idx.offset_of_line(4), 13);
    }

    // ── fuzz-style: random edits vs naive ───────────────────────────────

    #[test]
    fn fuzz_random_edits() {
        // Deterministic pseudo-random via simple LCG so the test is reproducible.
        let mut seed: u64 = 0xDEAD_BEEF;
        let next_rand = |s: &mut u64| -> u64 {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *s >> 33
        };

        let mut text = String::from("hello\nworld\nfoo\nbar\nbaz");
        let mut idx = LineIndex::new(&text);

        for _ in 0..200 {
            let len = text.len();
            let start = (next_rand(&mut seed) as usize) % (len + 1);
            let end_max = len - start;
            let del_len = if end_max == 0 {
                0
            } else {
                (next_rand(&mut seed) as usize) % end_max
            };
            let old_end = start + del_len;

            // Generate a random replacement (0..6 chars from a small alphabet)
            let repl_len = (next_rand(&mut seed) as usize) % 7;
            let chars = b"abc\nXY";
            let repl: String = (0..repl_len)
                .map(|_| chars[(next_rand(&mut seed) as usize) % chars.len()] as char)
                .collect();

            // Apply to naive string
            text = apply_edit(&text, start, old_end, &repl);

            // Apply to index
            idx.update(start, old_end, start + repl.len(), &repl);

            // Compare
            let expected = naive_line_starts(&text);
            assert_eq!(
                idx.line_starts(),
                expected.as_slice(),
                "divergence after edit [{start}..{old_end}] => {repl:?}  \
                 text={text:?}"
            );
            assert_eq!(idx.content_len(), text.len());
        }
    }

    #[test]
    fn fuzz_many_single_char_inserts() {
        let mut text = String::from("abc");
        let mut idx = LineIndex::new(&text);

        for i in 0..100 {
            let pos = i % (text.len() + 1);
            let ch = if i % 5 == 0 { "\n" } else { "x" };
            text = apply_edit(&text, pos, pos, ch);
            idx.update(pos, pos, pos + ch.len(), ch);
            assert_eq!(idx.line_starts(), naive_line_starts(&text).as_slice());
        }
    }

    #[test]
    fn fuzz_delete_all_then_rebuild() {
        let mut text = String::from("line1\nline2\nline3\nline4\nline5");
        let mut idx = LineIndex::new(&text);

        // Delete one character at a time from random positions
        let mut seed: u64 = 42;
        let next_rand = |s: &mut u64| -> u64 {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *s >> 33
        };

        while !text.is_empty() {
            let pos = (next_rand(&mut seed) as usize) % text.len();
            text = apply_edit(&text, pos, pos + 1, "");
            idx.update(pos, pos + 1, pos, "");
            assert_eq!(idx.line_starts(), naive_line_starts(&text).as_slice());
        }
        assert_eq!(idx.line_count(), 1);
        assert!(idx.is_empty());

        // Now rebuild by inserting characters
        let rebuild = "new\ncontent\nhere";
        text = apply_edit(&text, 0, 0, rebuild);
        idx.update(0, 0, rebuild.len(), rebuild);
        assert_eq!(idx.line_starts(), naive_line_starts(&text).as_slice());
    }
}
