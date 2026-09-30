//! Unified-diff parsing for the SCM diff viewer.
//!
//! Turns the text `git diff` produces (and whole-file new/deleted cases) into
//! classified lines the panel can colour. The parsing and line-building here is
//! pure: no git, no filesystem, no egui. That is what makes the viewer's
//! correctness testable without a repository, mirroring how [`crate::editor::
//! git_ui`] keeps its status parsing honest.

use std::path::Path;

/// How a rendered diff line should be coloured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffLineKind {
    /// Unchanged context line shared by both sides.
    Context,
    /// A line present only in the new side.
    Add,
    /// A line present only in the old side.
    Delete,
    /// A `@@ -a,b +c,d @@` range header.
    HunkHeader,
}

/// One classified line of a diff, with the new-side line number when the line
/// actually exists on that side (context and added lines; deletions and hunk
/// headers carry `None`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub line_no: Option<usize>,
}

/// Parse the body of a `git diff` for a single file into coloured lines.
///
/// The `diff --git`, `index`, `---`/`+++` and mode headers are dropped: the
/// panel already shows which file is selected, so repeating it adds noise. The
/// `@@` ranges survive as [`DiffLineKind::HunkHeader`] separators, and every
/// body line is classified by its leading `+`, `-` or (context) marker.
pub fn parse_unified_diff(diff: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let mut new_line = 0usize;
    for raw in diff.lines() {
        if raw.starts_with("diff --git")
            || raw.starts_with("index ")
            || raw.starts_with("--- ")
            || raw.starts_with("+++ ")
            || raw.starts_with("\\ ")
            || raw.starts_with("new file mode")
            || raw.starts_with("deleted file mode")
            || raw.starts_with("old mode")
            || raw.starts_with("new mode")
            || raw.starts_with("rename ")
            || raw.starts_with("similarity ")
            || raw.starts_with("copy ")
        {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("@@") {
            new_line = parse_hunk_new_start(rest);
            out.push(DiffLine {
                kind: DiffLineKind::HunkHeader,
                text: raw.to_string(),
                line_no: None,
            });
        } else if let Some(text) = raw.strip_prefix('+') {
            out.push(DiffLine {
                kind: DiffLineKind::Add,
                text: text.to_string(),
                line_no: Some(new_line),
            });
            new_line += 1;
        } else if let Some(text) = raw.strip_prefix('-') {
            out.push(DiffLine {
                kind: DiffLineKind::Delete,
                text: text.to_string(),
                line_no: None,
            });
        } else {
            // A context line: git prefixes it with a single space. Strip one
            // leading space if present, but keep the number advancing either
            // way because the line exists on both sides.
            let text = raw.strip_prefix(' ').unwrap_or(raw).to_string();
            out.push(DiffLine {
                kind: DiffLineKind::Context,
                text,
                line_no: Some(new_line),
            });
            new_line += 1;
        }
    }
    out
}

/// Read the `+c` new-side start line out of a hunk header's tail, i.e. the `c`
/// in `@@ -a,b +c,d @@`. Defaults to 1 when the range is malformed, so a
/// hand-edited or truncated diff still numbers lines sensibly.
fn parse_hunk_new_start(after_at_prefix: &str) -> usize {
    if let Some(plus) = after_at_prefix.find('+') {
        let digits: String = after_at_prefix[plus + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse::<usize>() {
            return n;
        }
    }
    1
}

/// Build a whole-file diff for a new or deleted file, where `git diff` prints
/// no body (untracked) or the caller simply has one side. Every line becomes an
/// [`DiffLineKind::Add`] (new file) or [`DiffLineKind::Delete`] (removed file).
pub fn diff_from_sides(new_text: Option<&str>, old_text: Option<&str>) -> Vec<DiffLine> {
    let mut out = Vec::new();
    if let Some(text) = new_text {
        for (i, l) in text.lines().enumerate() {
            out.push(DiffLine {
                kind: DiffLineKind::Add,
                text: l.to_string(),
                line_no: Some(i + 1),
            });
        }
    }
    if let Some(text) = old_text {
        for l in text.lines() {
            out.push(DiffLine {
                kind: DiffLineKind::Delete,
                text: l.to_string(),
                line_no: None,
            });
        }
    }
    out
}

/// The file name to show in the diff header, preferring the workspace-relative
/// form. Falls back to the full path when it isn't under `workspace_root`.
pub fn diff_header_label(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hunk_with_add_delete_and_context() {
        let diff = concat!(
            "diff --git a/f.txt b/f.txt\n",
            "index 111..222 100644\n",
            "--- a/f.txt\n",
            "+++ b/f.txt\n",
            "@@ -1,3 +1,4 @@\n",
            " line1\n",
            "-line2\n",
            "+line2x\n",
            "+line2y\n",
            " line3\n",
        );
        let lines = parse_unified_diff(diff);
        let kinds: Vec<_> = lines.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::HunkHeader,
                DiffLineKind::Context,
                DiffLineKind::Delete,
                DiffLineKind::Add,
                DiffLineKind::Add,
                DiffLineKind::Context,
            ]
        );
        // Header lines are dropped; the context line after the hunk is line 1.
        assert_eq!(lines[1].text, "line1");
        assert_eq!(lines[1].line_no, Some(1));
        // Deletions carry no new-side number; additions advance it.
        assert_eq!(lines[2].line_no, None);
        assert_eq!(lines[3].line_no, Some(2));
        assert_eq!(lines[4].line_no, Some(3));
        assert_eq!(lines[5].line_no, Some(4));
    }

    #[test]
    fn new_file_body_is_all_additions() {
        let diff = concat!(
            "diff --git a/n.txt b/n.txt\n",
            "new file mode 100644\n",
            "index 000..abc\n",
            "--- /dev/null\n",
            "+++ b/n.txt\n",
            "@@ -0,0 +1,2 @@\n",
            "+hello\n",
            "+world\n",
        );
        let lines = parse_unified_diff(diff);
        assert_eq!(
            lines.iter().map(|l| l.kind).collect::<Vec<_>>(),
            vec![
                DiffLineKind::HunkHeader,
                DiffLineKind::Add,
                DiffLineKind::Add
            ]
        );
        assert_eq!(lines[1].text, "hello");
        assert_eq!(lines[1].line_no, Some(1));
        assert_eq!(lines[2].line_no, Some(2));
    }

    #[test]
    fn malformed_hunk_defaults_to_line_one() {
        // No `+c` range: the first context line still numbers from 1.
        let lines = parse_unified_diff("@@ garbage @@\n kept\n");
        assert_eq!(lines[0].kind, DiffLineKind::HunkHeader);
        assert_eq!(lines[1].line_no, Some(1));
    }

    #[test]
    fn diff_from_sides_covers_new_and_deleted_files() {
        let added = diff_from_sides(Some("a\nb\n"), None);
        assert_eq!(added.len(), 2);
        assert!(added.iter().all(|l| l.kind == DiffLineKind::Add));
        assert_eq!(added[1].line_no, Some(2));

        let removed = diff_from_sides(None, Some("x\ny\nz\n"));
        assert_eq!(removed.len(), 3);
        assert!(removed.iter().all(|l| l.kind == DiffLineKind::Delete));
        assert!(removed.iter().all(|l| l.line_no.is_none()));
    }

    #[test]
    fn header_label_prefers_relative_path() {
        let root = Path::new("/work/proj");
        assert_eq!(
            diff_header_label(root, &root.join("src/main.rs")),
            "src/main.rs"
        );
        // A path outside the workspace keeps its full form rather than panicking.
        assert_eq!(
            diff_header_label(root, Path::new("/elsewhere/f.rs")),
            "/elsewhere/f.rs"
        );
    }
}
