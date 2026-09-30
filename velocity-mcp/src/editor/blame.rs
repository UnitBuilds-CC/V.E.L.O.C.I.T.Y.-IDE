//! Git blame inline annotation: pure helpers for formatting and caching blame
//! data surfaced in the editor status bar. Every competitor (VS Code + GitLens,
//! Cursor, Qoder) shows who last modified the current line and when; this module
//! provides the formatting logic decoupled from process-spawning.

use crate::editor::git_ui::BlameLine;

/// Convert a Unix timestamp to a compact relative date string.
/// `now` is also a Unix timestamp (injected for testability).
pub fn relative_date(ts: i64, now: i64) -> String {
    let diff = now - ts;
    if diff < 0 {
        return "future".to_string();
    }
    if diff < 60 {
        return "just now".to_string();
    }
    let mins = diff / 60;
    if mins < 60 {
        return format!("{mins} min ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours} hr ago");
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{days} day{} ago", if days == 1 { "" } else { "s" });
    }
    let months = days / 30;
    if months < 12 {
        return format!("{months} month{} ago", if months == 1 { "" } else { "s" });
    }
    let years = months / 12;
    format!("{years} year{} ago", if years == 1 { "" } else { "s" })
}

/// Format a blame annotation for display in the status bar.
/// Returns `None` if the date field cannot be parsed as a Unix timestamp.
pub fn format_blame_annotation(blame: &BlameLine, now: i64) -> Option<String> {
    let ts: i64 = blame.date.trim().parse().ok()?;
    let rel = relative_date(ts, now);
    // Truncate author to first name only for compactness.
    let author = blame.author.split_whitespace().next().unwrap_or("???");
    Some(format!(
        "{} \u{b7} {} \u{b7} {}",
        author, rel, blame.commit_hash
    ))
}

/// Safe 0-indexed lookup into a blame vec. Returns `None` for out-of-range.
pub fn blame_for_line(blames: &[BlameLine], line: usize) -> Option<&BlameLine> {
    blames.get(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_blame(hash: &str, author: &str, date: &str) -> BlameLine {
        BlameLine {
            commit_hash: hash.to_string(),
            author: author.to_string(),
            date: date.to_string(),
            line_content: String::new(),
        }
    }

    #[test]
    fn relative_date_just_now() {
        assert_eq!(relative_date(1000, 1030), "just now");
    }

    #[test]
    fn relative_date_minutes() {
        assert_eq!(relative_date(1000, 1000 + 120), "2 min ago");
    }

    #[test]
    fn relative_date_hours() {
        assert_eq!(relative_date(0, 7200), "2 hr ago");
    }

    #[test]
    fn relative_date_days() {
        assert_eq!(relative_date(0, 86400), "1 day ago");
        assert_eq!(relative_date(0, 86400 * 5), "5 days ago");
    }

    #[test]
    fn relative_date_months() {
        assert_eq!(relative_date(0, 86400 * 60), "2 months ago");
    }

    #[test]
    fn relative_date_years() {
        assert_eq!(relative_date(0, 86400 * 400), "1 year ago");
    }

    #[test]
    fn format_annotation_valid() {
        let blame = make_blame("a1b2c3d", "Alice Smith", "1000");
        let result = format_blame_annotation(&blame, 1000 + 86400 * 3).unwrap();
        assert_eq!(result, "Alice \u{b7} 3 days ago \u{b7} a1b2c3d");
    }

    #[test]
    fn format_annotation_bad_date() {
        let blame = make_blame("a1b2c3d", "Alice", "not_a_number");
        assert!(format_blame_annotation(&blame, 9999).is_none());
    }

    #[test]
    fn blame_for_line_in_bounds() {
        let blames = vec![make_blame("h1", "A", "1"), make_blame("h2", "B", "2")];
        assert_eq!(blame_for_line(&blames, 0).unwrap().commit_hash, "h1");
        assert_eq!(blame_for_line(&blames, 1).unwrap().commit_hash, "h2");
    }

    #[test]
    fn blame_for_line_out_of_bounds() {
        let blames = vec![make_blame("h1", "A", "1")];
        assert!(blame_for_line(&blames, 5).is_none());
        assert!(blame_for_line(&[], 0).is_none());
    }
}
