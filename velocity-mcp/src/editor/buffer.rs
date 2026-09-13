use std::path::PathBuf;
use std::time::SystemTime;

// ─── Undo/Redo ───────────────────────────────────────────────────────────────

/// A single undoable edit operation.
#[derive(Debug, Clone)]
pub struct EditOp {
    /// Content snapshot *before* this edit.
    pub before: String,
    /// Cursor char-offset after the edit (so undo can restore position).
    pub cursor_after: usize,
}

/// Bounded undo/redo history. Keeps at most `capacity` undo entries.
#[derive(Debug, Clone)]
pub struct UndoStack {
    undo: Vec<EditOp>,
    redo: Vec<EditOp>,
    capacity: usize,
    /// Hash of content when last snapshot was pushed (coalesces rapid edits).
    last_push_hash: u64,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            capacity: 500,
            last_push_hash: 0,
        }
    }
}

impl UndoStack {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ..Default::default()
        }
    }

    /// Push a snapshot of the content *before* the edit that is about to happen.
    /// Coalesces if content hash hasn't changed (avoids duplicate entries from
    /// per-frame TextEdit re-renders).
    pub fn push(&mut self, content_before: &str, cursor_after: usize) {
        let h = fnv1a(content_before);
        if h == self.last_push_hash && !self.undo.is_empty() {
            return; // same content — skip duplicate
        }
        self.last_push_hash = h;
        self.redo.clear(); // new edit invalidates redo branch
        self.undo.push(EditOp {
            before: content_before.to_string(),
            cursor_after,
        });
        if self.undo.len() > self.capacity {
            self.undo.remove(0);
        }
    }

    /// Undo: returns the content to restore (and pushes current onto redo).
    pub fn undo(&mut self, current_content: &str, current_cursor: usize) -> Option<EditOp> {
        let op = self.undo.pop()?;
        self.redo.push(EditOp {
            before: current_content.to_string(),
            cursor_after: current_cursor,
        });
        Some(op)
    }

    /// Redo: returns the content to restore.
    pub fn redo(&mut self, current_content: &str, current_cursor: usize) -> Option<EditOp> {
        let op = self.redo.pop()?;
        self.undo.push(EditOp {
            before: current_content.to_string(),
            cursor_after: current_cursor,
        });
        Some(op)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// A simple in-memory document.
#[derive(Default, Debug, Clone)]
pub struct EditorBuffer {
    pub path: Option<PathBuf>,
    pub content: String,
    /// Snapshot of `content` as last saved/loaded from disk. Used to detect
    /// unsaved edits (`is_dirty`) without re-reading the file.
    pub saved_content: String,
    /// Modification time of the file the last time we read/wrote it, used to
    /// detect external changes on disk.
    pub disk_mtime: Option<SystemTime>,
    /// Per-line change markers vs `saved_content`, one entry per current line:
    /// 0 = unchanged, 1 = added, 2 = modified, 3 = deletion just above this line.
    /// Cached; recomputed only when `content` changes (see `refresh_diff_marks`).
    pub diff_marks: Vec<u8>,
    /// Hash of content when `diff_marks` was last computed (cache key).
    diff_marks_hash: u64,
    /// Undo/redo history for this buffer.
    pub undo_stack: UndoStack,
    /// Hash of content at last frame — detects when egui TextEdit mutated text.
    pub last_frame_hash: u64,
    /// Per-buffer find/replace overlay state.
    pub find_replace: crate::editor::find_replace::FindReplaceState,
    /// Per-buffer code folding state.
    pub fold_state: crate::editor::code_folding::FoldState,
    /// Detected indent style for this buffer.
    pub indent_style: crate::editor::auto_indent::IndentStyle,
    /// Breakpoints set in this buffer (line numbers, 1-based).
    pub breakpoints: Vec<usize>,
    /// Cached line index for fast line-based operations on large files.
    line_index: Option<LineIndex>,
    /// Line window for virtual scrolling in large files.
    pub line_window: Option<LineWindow>,
    /// Dirty flag set by mutation methods. Avoids hashing the entire buffer
    /// content every frame to detect changes — `pre_frame_snapshot()` checks
    /// this flag instead of calling `fnv1a(&self.content)`.
    content_dirty: bool,
    /// Monotonic generation counter. Incremented on every content mutation.
    /// Used by `LineIndex` to check staleness in O(1) instead of re-hashing.
    generation: u64,
    /// FNV-1a hash of `content` — maintained incrementally in `mark_mutated()`.
    content_hash: u64,
    /// FNV-1a hash of `saved_content` — updated in `mark_saved()` / `load_text()`.
    saved_content_hash: u64,
}

impl EditorBuffer {
    pub fn new(path: Option<PathBuf>, content: String) -> Self {
        let h = fnv1a(&content);
        let indent_style = crate::editor::auto_indent::IndentStyle::detect(&content);
        let line_count = content.lines().count().max(1);
        Self {
            path,
            saved_content: content.clone(),
            content,
            disk_mtime: None,
            diff_marks: Vec::new(),
            diff_marks_hash: 0,
            undo_stack: UndoStack::default(),
            last_frame_hash: h,
            find_replace: Default::default(),
            fold_state: Default::default(),
            indent_style,
            breakpoints: Vec::new(),
            line_index: None,
            line_window: if line_count > LARGE_FILE_THRESHOLD {
                Some(LineWindow::new(line_count, 50))
            } else {
                None
            },
            content_dirty: false,
            generation: 1,
            content_hash: h,
            saved_content_hash: h, // content == saved_content at construction
        }
    }

    /// Mark content as mutated. Bumps the generation counter (invalidates any
    /// cached `LineIndex`) and sets the dirty flag for `pre_frame_snapshot()`.
    fn mark_mutated(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.content_dirty = true;
        self.content_hash = fnv1a(&self.content);
    }

    /// Call once per frame *before* the TextEdit renders. If content changed
    /// since last frame (detected via the dirty flag, not a full content hash),
    /// push the previous state onto the undo stack.
    pub fn pre_frame_snapshot(&mut self) {
        if self.content_dirty {
            self.undo_stack.push(&self.content, 0);
            self.content_dirty = false;
        }
    }

    /// Call once per frame *after* the TextEdit renders to record the new hash.
    pub fn post_frame_snapshot(&mut self) {
        self.last_frame_hash = fnv1a(&self.content);
    }

    /// Perform undo: restores previous content. Returns cursor position hint.
    pub fn undo(&mut self) -> Option<usize> {
        let op = self.undo_stack.undo(&self.content, 0)?;
        self.content = op.before;
        self.mark_mutated();
        self.last_frame_hash = fnv1a(&self.content);
        Some(op.cursor_after)
    }

    /// Perform redo: restores next content. Returns cursor position hint.
    pub fn redo(&mut self) -> Option<usize> {
        let op = self.undo_stack.redo(&self.content, 0)?;
        self.content = op.before;
        self.mark_mutated();
        self.last_frame_hash = fnv1a(&self.content);
        Some(op.cursor_after)
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "untitled".to_string())
    }

    pub fn update_content(&mut self, content: String) {
        self.content = content;
        self.mark_mutated();
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn content_mut(&mut self) -> &mut String {
        self.mark_mutated();
        &mut self.content
    }

    /// Returns `&mut String` **without** marking the buffer dirty.
    ///
    /// Use this when handing a mutable reference to egui's `TextEdit` and then
    /// checking `response.changed()` to decide whether the content was actually
    /// modified.  The old `content_mut()` unconditionally called `mark_mutated()`
    /// which rebuilt the `LineIndex` and invalidated every downstream cache on
    /// every frame — even when the user was not typing.
    pub fn content_mut_lazy(&mut self) -> &mut String {
        &mut self.content
    }

    /// Explicitly mark the buffer as mutated (bump generation, set dirty flag).
    /// Call this after using `content_mut_lazy()` when you detect that the
    /// content actually changed (e.g. `response.changed()` from egui).
    pub fn mark_mutated_pub(&mut self) {
        self.mark_mutated();
    }

    /// Load text from disk (or an authoritative source); marks the buffer clean.
    pub fn load_text(&mut self, text: &str) {
        self.content = text.to_string();
        self.saved_content = self.content.clone();
        let h = fnv1a(&self.content);
        self.content_hash = h;
        self.saved_content_hash = h;
        self.mark_mutated();
    }

    /// True when the in-memory content differs from the last saved/loaded state.
    /// Uses O(1) hash comparison instead of O(n) string equality.
    pub fn is_dirty(&self) -> bool {
        self.content_hash != self.saved_content_hash
    }

    /// Mark the current content as the saved baseline (call after a successful write).
    pub fn mark_saved(&mut self) {
        self.saved_content = self.content.clone();
        self.saved_content_hash = self.content_hash;
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            std::fs::write(path, &self.content)?;
        }
        self.saved_content = self.content.clone();
        self.saved_content_hash = self.content_hash;
        Ok(())
    }

    /// Recompute `diff_marks` if `content` changed since the last call. Cheap on
    /// the hot path: an unchanged buffer only pays for a single content hash.
    pub fn refresh_diff_marks(&mut self) {
        let h = fnv1a(&self.content);
        if h == self.diff_marks_hash {
            return;
        }
        self.diff_marks = compute_line_diff(&self.saved_content, &self.content);
        self.diff_marks_hash = h;
    }

    /// Get or build the cached line index. Rebuilds only when content changes
    /// (checked via O(1) generation counter, not a full content hash).
    pub fn line_index(&mut self) -> &LineIndex {
        let needs_rebuild = match &self.line_index {
            None => true,
            Some(idx) => idx.generation != self.generation,
        };
        if needs_rebuild {
            self.line_index = Some(LineIndex::build(&self.content, self.generation));
        }
        self.line_index.as_ref().unwrap()
    }

    /// Get the total line count using the cached index.
    pub fn line_count(&mut self) -> usize {
        self.line_index().line_count()
    }

    /// Get a specific line's content using the cached index. O(1) after index build.
    pub fn get_line(&mut self, line: usize) -> Option<&str> {
        // Build index if needed (separate statement to end mutable borrow)
        let needs_rebuild = match &self.line_index {
            None => true,
            Some(idx) => idx.generation != self.generation,
        };
        if needs_rebuild {
            self.line_index = Some(LineIndex::build(&self.content, self.generation));
        }
        let content_len = self.content.len();
        let idx = self.line_index.as_ref().unwrap();
        idx.line_range(line, content_len)
            .map(|(start, end)| &self.content[start..end])
    }

    /// Convert byte offset to (line, col) using the cached index.
    pub fn byte_to_line_col(&mut self, offset: usize) -> (usize, usize) {
        self.line_index().byte_to_line_col(offset)
    }

    /// Convert (line, col) to byte offset using the cached index.
    pub fn line_col_to_byte(&mut self, line: usize, col: usize) -> usize {
        self.line_index().line_col_to_byte(line, col)
    }

    /// Ensure the line window is initialized and up to date.
    pub fn ensure_line_window(&mut self, viewport_height: usize) {
        let total = self.line_index().line_count();
        match &mut self.line_window {
            Some(win) => {
                win.set_total_lines(total);
                win.set_viewport_height(viewport_height);
            }
            None if total > LARGE_FILE_THRESHOLD => {
                self.line_window = Some(LineWindow::new(total, viewport_height));
            }
            None => {}
        }
    }

    /// Scroll the line window to make a specific line visible.
    pub fn scroll_to_line(&mut self, line: usize, viewport_height: usize) {
        self.ensure_line_window(viewport_height);
        if let Some(win) = &mut self.line_window {
            win.scroll_to(line);
        }
    }

    /// Get the visible line range for virtual scrolling.
    pub fn visible_lines(&mut self, viewport_height: usize) -> std::ops::Range<usize> {
        self.ensure_line_window(viewport_height);
        match &self.line_window {
            Some(win) => win.visible_range(),
            None => 0..self.line_index().line_count(),
        }
    }
}

/// FNV-1a hash of a string, used as a cheap change-detection key.
pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Classify each line of `current` relative to `saved` with an LCS line diff.
/// Returns one marker per current line (0=unchanged, 1=added, 2=modified,
/// 3=deletion just above this line). Falls back to a cheap prefix/suffix diff
/// for very large files to keep recomputation bounded.
fn compute_line_diff(saved: &str, current: &str) -> Vec<u8> {
    let old: Vec<&str> = saved.lines().collect();
    let new: Vec<&str> = current.lines().collect();
    let mut marks = vec![0u8; new.len()];
    if old.is_empty() {
        for m in marks.iter_mut() {
            *m = 1;
        }
        return marks;
    }
    let n = old.len();
    let m = new.len();
    // Bound the O(n*m) LCS: fall back to a cheap contiguous-change diff.
    if n > 4000 || m > 4000 {
        return cheap_line_diff(&old, &new);
    }
    let stride = m + 1;
    let mut lcs = vec![0u32; (n + 1) * stride];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let v = if old[i] == new[j] {
                lcs[(i + 1) * stride + (j + 1)] + 1
            } else {
                lcs[(i + 1) * stride + j].max(lcs[i * stride + (j + 1)])
            };
            lcs[i * stride + j] = v;
        }
    }
    let mut i = 0usize;
    let mut j = 0usize;
    let mut pending_deletes = 0u32;
    while i < n && j < m {
        if old[i] == new[j] {
            if pending_deletes > 0 {
                marks[j] = 3;
                pending_deletes = 0;
            }
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * stride + j] >= lcs[i * stride + (j + 1)] {
            pending_deletes += 1;
            i += 1;
        } else {
            marks[j] = if pending_deletes > 0 {
                pending_deletes -= 1;
                2
            } else {
                1
            };
            j += 1;
        }
    }
    while j < m {
        marks[j] = if pending_deletes > 0 {
            pending_deletes -= 1;
            2
        } else {
            1
        };
        j += 1;
    }
    if i < n && !marks.is_empty() {
        let last = marks.len() - 1;
        if marks[last] == 0 {
            marks[last] = 3;
        }
    }
    marks
}

/// Cheap fallback diff: mark the contiguous span between the common prefix and
/// common suffix as changed. Used for very large files.
fn cheap_line_diff(old: &[&str], new: &[&str]) -> Vec<u8> {
    let mut marks = vec![0u8; new.len()];
    let max_pre = old.len().min(new.len());
    let mut p = 0;
    while p < max_pre && old[p] == new[p] {
        p += 1;
    }
    let mut s = 0;
    while s < (old.len() - p)
        && s < (new.len() - p)
        && old[old.len() - 1 - s] == new[new.len() - 1 - s]
    {
        s += 1;
    }
    let old_had_mid = old.len().saturating_sub(p + s) > 0;
    let end = new.len().saturating_sub(s);
    for mark in marks.iter_mut().take(end).skip(p) {
        *mark = if old_had_mid { 2 } else { 1 };
    }
    marks
}

// ═══════════════════════════════════════════════════════════════════════════
// Large File Optimizations
// ═══════════════════════════════════════════════════════════════════════════

/// Threshold above which a buffer is considered "large" and gets special
/// treatment (line windowing, deferred syntax highlighting).
pub const LARGE_FILE_THRESHOLD: usize = 10_000; // lines

/// A cached line index for fast line-based operations on large files.
/// Instead of scanning the full content string each time, we cache
/// byte offsets of each line start.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each line start within the content.
    line_starts: Vec<u64>,
    /// Generation counter of the buffer when this index was built.
    /// Compared against `EditorBuffer::generation` for O(1) staleness check
    /// instead of re-hashing the entire content.
    pub generation: u64,
}

impl LineIndex {
    /// Build a line index from content. O(n) but only done once per edit.
    pub fn build(content: &str, generation: u64) -> Self {
        let mut starts = vec![0u64];
        for (i, b) in content.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u64);
            }
        }
        Self {
            line_starts: starts,
            generation,
        }
    }

    /// Total number of lines.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Get the byte range of a line (0-indexed).
    pub fn line_range(&self, line: usize, content_len: usize) -> Option<(usize, usize)> {
        if line >= self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line] as usize;
        let end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1] as usize
        } else {
            content_len
        };
        Some((start, end))
    }

    /// Get the content of a specific line without trailing newline.
    pub fn line_content<'a>(&self, line: usize, content: &'a str) -> Option<&'a str> {
        let (start, end) = self.line_range(line, content.len())?;
        let text = &content[start..end];
        Some(text.trim_end_matches('\n').trim_end_matches('\r'))
    }

    /// Convert a byte offset to (line, col) using binary search. O(log n).
    pub fn byte_to_line_col(&self, offset: usize) -> (usize, usize) {
        match self.line_starts.binary_search(&(offset as u64)) {
            Ok(line) => (line, 0),
            Err(line) => {
                let line = line.saturating_sub(1);
                let col = offset - self.line_starts[line] as usize;
                (line, col)
            }
        }
    }

    /// Convert (line, col) to byte offset. O(1).
    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        if line >= self.line_starts.len() {
            return self.line_starts.last().copied().unwrap_or(0) as usize;
        }
        self.line_starts[line] as usize + col
    }
}

/// A visible window of lines for virtual scrolling.
/// Only the lines in the viewport are rendered, dramatically reducing
/// the cost of displaying large files.
#[derive(Debug, Clone)]
pub struct LineWindow {
    pub first_visible: usize,
    pub visible_count: usize,
    total_lines: usize,
}

impl LineWindow {
    pub fn new(total_lines: usize, viewport_height: usize) -> Self {
        Self {
            first_visible: 0,
            visible_count: viewport_height.min(total_lines),
            total_lines,
        }
    }

    /// Scroll to make a specific line visible, keeping it centered if possible.
    pub fn scroll_to(&mut self, line: usize) {
        if line >= self.total_lines {
            return;
        }
        let half = self.visible_count / 2;
        self.first_visible = line.saturating_sub(half);
        if self.first_visible + self.visible_count > self.total_lines {
            self.first_visible = self.total_lines.saturating_sub(self.visible_count);
        }
    }

    /// Check if a line is within the visible window.
    pub fn is_visible(&self, line: usize) -> bool {
        line >= self.first_visible && line < self.first_visible + self.visible_count
    }

    /// Get the range of visible lines.
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        self.first_visible..(self.first_visible + self.visible_count).min(self.total_lines)
    }

    /// Update the viewport height (e.g., when the window is resized).
    pub fn set_viewport_height(&mut self, height: usize) {
        self.visible_count = height.min(self.total_lines);
    }

    /// Update total line count after content changes.
    pub fn set_total_lines(&mut self, count: usize) {
        self.total_lines = count;
        if self.first_visible + self.visible_count > count {
            self.first_visible = count.saturating_sub(self.visible_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_from_path() {
        let b = EditorBuffer::new(Some(PathBuf::from("src/main.rs")), String::new());
        assert_eq!(b.title(), "main.rs");
    }

    #[test]
    fn title_untitled() {
        let b = EditorBuffer::new(None, String::new());
        assert_eq!(b.title(), "untitled");
    }

    #[test]
    fn diff_marks_added_and_modified() {
        let marks = compute_line_diff("a\nb\nc", "a\nB\nc\nd");
        assert_eq!(marks, vec![0, 2, 0, 1]);
    }

    #[test]
    fn diff_marks_removed_above() {
        let marks = compute_line_diff("a\nb\nc", "a\nc");
        assert_eq!(marks, vec![0, 3]);
    }

    #[test]
    fn diff_marks_all_new_when_saved_empty() {
        let marks = compute_line_diff("", "x\ny");
        assert_eq!(marks, vec![1, 1]);
    }

    #[test]
    fn refresh_diff_marks_clean_buffer_has_no_changes() {
        let mut b = EditorBuffer::new(Some(PathBuf::from("f.rs")), "a\nb".to_string());
        b.refresh_diff_marks();
        assert!(b.diff_marks.iter().all(|m| *m == 0));
    }

    #[test]
    fn line_index_build_and_query() {
        let content = "hello\nworld\nfoo";
        let idx = LineIndex::build(content, 1);
        assert_eq!(idx.line_count(), 3);
        assert_eq!(idx.line_content(0, content), Some("hello"));
        assert_eq!(idx.line_content(1, content), Some("world"));
        assert_eq!(idx.line_content(2, content), Some("foo"));
        assert_eq!(idx.line_content(3, content), None);
    }

    #[test]
    fn line_index_byte_to_line_col() {
        let content = "ab\ncd\nef";
        let idx = LineIndex::build(content, 1);
        assert_eq!(idx.byte_to_line_col(0), (0, 0)); // 'a'
        assert_eq!(idx.byte_to_line_col(1), (0, 1)); // 'b'
        assert_eq!(idx.byte_to_line_col(3), (1, 0)); // 'c'
        assert_eq!(idx.byte_to_line_col(6), (2, 0)); // 'e'
    }

    #[test]
    fn line_index_line_col_to_byte() {
        let content = "ab\ncd\nef";
        let idx = LineIndex::build(content, 1);
        assert_eq!(idx.line_col_to_byte(0, 0), 0);
        assert_eq!(idx.line_col_to_byte(1, 0), 3);
        assert_eq!(idx.line_col_to_byte(2, 1), 7);
    }

    #[test]
    fn line_window_scroll() {
        let mut win = LineWindow::new(100, 10);
        assert_eq!(win.first_visible, 0);
        assert!(win.is_visible(0));
        assert!(win.is_visible(9));
        assert!(!win.is_visible(10));

        win.scroll_to(50);
        assert!(win.is_visible(50));
        assert!(!win.is_visible(0));
    }

    #[test]
    fn line_window_visible_range() {
        let mut win = LineWindow::new(100, 10);
        assert_eq!(win.visible_range(), 0..10);
        win.scroll_to(95);
        assert_eq!(win.visible_range(), 90..100);
    }
}
