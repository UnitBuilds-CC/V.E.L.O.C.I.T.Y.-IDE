//! Hot exit: unsaved work survives a restart.
//!
//! Every competitor (VS Code, Cursor, and friends) keeps dirty buffers across
//! application exits — closing the window with unsaved edits must never
//! silently lose them. This module owns the on-disk shape: a small JSON
//! session under `.velocity/` written from `on_exit` and consumed once at the
//! next start. The capture/restore policy (what counts as worth keeping,
//! deduplication by path, expiry) lives here as pure functions so it is all
//! unit-testable without an egui harness.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bump when the session shape changes so old files fail closed on read.
pub const SESSION_VERSION: u32 = 1;
/// Sessions older than this are dropped rather than restored: a month-old
/// scratch buffer is detritus, not the user's work.
pub const RETENTION_DAYS: i64 = 30;

const SESSION_FILE_NAME: &str = "hot-exit.json";

/// One unsaved document: its path (`None` for never-saved scratch buffers)
/// and the exact in-memory content at exit time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HotExitFile {
    pub path: Option<PathBuf>,
    pub content: String,
}

/// The full captured session. `active_index` points into `files` so the
/// restored window can re-focus the tab the user was actually editing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HotExitSession {
    pub version: u32,
    pub saved_at_unix: i64,
    pub files: Vec<HotExitFile>,
    pub active_index: Option<usize>,
}

/// Where the session lives for a workspace: `.velocity/hot-exit.json`.
pub fn session_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".velocity").join(SESSION_FILE_NAME)
}

/// Assemble a session from captured files. Files sharing a path collapse to
/// their first occurrence (two dirty tabs on one buffer must not restore
/// twice); untitled scratch buffers are always distinct and all kept. An
/// `active_index` whose file was dropped by dedup falls back to `None`.
pub fn build_session(
    files: Vec<HotExitFile>,
    active_index: Option<usize>,
    saved_at_unix: i64,
) -> HotExitSession {
    let mut seen: Vec<Option<&Path>> = Vec::new();
    let mut kept = Vec::new();
    let mut new_active: Option<usize> = None;
    for (i, file) in files.iter().enumerate() {
        let p = file.path.as_deref();
        if p.is_some() && seen.contains(&p) {
            if active_index == Some(i) {
                new_active = None;
            }
            continue;
        }
        seen.push(p);
        if active_index == Some(i) {
            new_active = Some(kept.len());
        }
        kept.push(file.clone());
    }
    HotExitSession {
        version: SESSION_VERSION,
        saved_at_unix,
        files: kept,
        active_index: if active_index.is_some() {
            new_active
        } else {
            None
        },
    }
}

/// True when the session predates the retention window and should be dropped.
pub fn is_expired(session: &HotExitSession, now_unix: i64, retention_days: i64) -> bool {
    now_unix.saturating_sub(session.saved_at_unix) > retention_days * 86_400
}

pub fn to_json(session: &HotExitSession) -> String {
    // Serialization of these types cannot fail; the write path surfaces any
    // io error instead.
    serde_json::to_string_pretty(session).unwrap_or_default()
}

pub fn from_json(raw: &str) -> Option<HotExitSession> {
    // Tolerate a UTF-8 BOM: PowerShell's `Set-Content -Encoding UTF8` and many
    // editors write one, and rejecting a BOM'd file would silently discard the
    // user's unsaved work it protects.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let session: HotExitSession = serde_json::from_str(raw).ok()?;
    if session.version != SESSION_VERSION {
        return None;
    }
    Some(session)
}

/// Read the session file; `None` when absent or unreadable/stale-shaped.
pub fn read_session(path: &Path) -> Option<HotExitSession> {
    let raw = std::fs::read_to_string(path).ok()?;
    from_json(&raw)
}

/// Write the session, creating `.velocity/` as needed.
pub fn write_session(path: &Path, session: &HotExitSession) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, to_json(session))
}

/// Remove the session file once consumed (or when nothing was dirty).
pub fn clear_session(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: Option<&str>, content: &str) -> HotExitFile {
        HotExitFile {
            path: path.map(PathBuf::from),
            content: content.to_string(),
        }
    }

    #[test]
    fn session_round_trips_through_json() {
        let s = build_session(
            vec![file(Some("a.rs"), "dirty a"), file(None, "scratch")],
            Some(1),
            1_700_000_000,
        );
        let loaded = from_json(&to_json(&s)).expect("round trip");
        assert_eq!(loaded, s);
        assert_eq!(loaded.active_index, Some(1));
    }

    #[test]
    fn build_session_dedupes_by_path_keeping_the_first() {
        let s = build_session(
            vec![
                file(Some("a.rs"), "first"),
                file(Some("a.rs"), "second"),
                file(None, "scratch one"),
                file(None, "scratch two"),
            ],
            None,
            0,
        );
        // Same path collapses; each untitled buffer is its own document.
        assert_eq!(
            s.files,
            vec![
                file(Some("a.rs"), "first"),
                file(None, "scratch one"),
                file(None, "scratch two")
            ]
        );
    }

    #[test]
    fn active_index_follows_dedup_or_falls_back_to_none() {
        // Active tab is the dropped duplicate → no active marker afterwards.
        let s = build_session(
            vec![file(Some("a.rs"), "first"), file(Some("a.rs"), "dup")],
            Some(1),
            0,
        );
        assert_eq!(s.active_index, None);
        // Active tab survives → its index is remapped through the collapse.
        let s = build_session(
            vec![
                file(Some("a.rs"), "one"),
                file(Some("a.rs"), "dup"),
                file(Some("b.rs"), "two"),
            ],
            Some(2),
            0,
        );
        assert_eq!(s.active_index, Some(1));
    }

    #[test]
    fn expiry_is_strict_and_version_guards_reads() {
        let mut s = build_session(vec![file(Some("a.rs"), "x")], None, 1_000_000);
        assert!(!is_expired(&s, 1_000_000 + 29 * 86_400, RETENTION_DAYS));
        assert!(is_expired(&s, 1_000_000 + 31 * 86_400, RETENTION_DAYS));
        s.version = SESSION_VERSION + 99;
        assert!(from_json(&to_json(&s)).is_none());
    }

    #[test]
    fn from_json_tolerates_a_utf8_bom() {
        let s = build_session(vec![file(Some("a.rs"), "keep me")], Some(0), 3);
        let bomned = format!("\u{feff}{}", to_json(&s));
        assert_eq!(from_json(&bomned).expect("BOM must not break the read"), s);
    }

    #[test]
    fn read_write_clear_round_trip_on_disk() {
        let dir =
            std::env::temp_dir().join(format!("velocity_hot_exit_{}_{}", std::process::id(), "rw"));
        let path = session_path(&dir);
        let s = build_session(vec![file(Some("a.rs"), "keep me")], Some(0), 7);
        write_session(&path, &s).expect("write");
        assert_eq!(read_session(&path).expect("read"), s);
        clear_session(&path).expect("clear");
        assert!(read_session(&path).is_none());
        // Clearing a missing file is not an error (idempotent consumption).
        assert!(clear_session(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
