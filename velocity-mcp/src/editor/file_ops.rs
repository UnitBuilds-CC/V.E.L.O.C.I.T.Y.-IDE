//! On-disk file/folder operations backing the explorer context menu.
//!
//! Kept pure (paths in, `Result` out) so the validation and collision rules
//! are unit-tested without any UI. The GUI layer ([`crate::editor::app`])
//! owns dialogs and tree refresh; it only ever calls these functions.

use std::path::{Path, PathBuf};

/// Actions the explorer context menu can request of the host app. Produced by
/// the (UI-only) menu rows and consumed centrally so menu rendering never
/// touches the filesystem itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeAction {
    /// Put the absolute path on the clipboard.
    CopyPath(PathBuf),
    /// Prompt for a new file name inside this directory.
    NewFile(PathBuf),
    /// Prompt for a new folder name inside this directory.
    NewFolder(PathBuf),
    /// Prompt for a new name for this entry.
    Rename(PathBuf),
    /// Delete this entry (after an explicit confirmation).
    Delete(PathBuf),
}

/// Which operation the pending name-entry dialog performs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileEntryMode {
    /// Create a file named by the dialog text inside `dir`.
    NewFile { dir: PathBuf },
    /// Create a folder named by the dialog text inside `dir`.
    NewFolder { dir: PathBuf },
    /// Rename `path` to the dialog text (name only, stays in its parent).
    Rename { path: PathBuf },
}

/// A pending single-line name prompt (new file / new folder / rename).
#[derive(Clone, Debug)]
pub struct FileEntryDialog {
    pub mode: FileEntryMode,
    /// Editable text: empty for creates, current name for rename.
    pub value: String,
}

impl FileEntryDialog {
    /// Build the dialog for a tree action, or `None` for actions that need no
    /// prompt (copy, delete-confirmation is handled separately).
    pub fn for_action(action: &TreeAction) -> Option<Self> {
        match action {
            TreeAction::NewFile(dir) => Some(Self {
                mode: FileEntryMode::NewFile { dir: dir.clone() },
                value: String::new(),
            }),
            TreeAction::NewFolder(dir) => Some(Self {
                mode: FileEntryMode::NewFolder { dir: dir.clone() },
                value: String::new(),
            }),
            TreeAction::Rename(path) => Some(Self {
                mode: FileEntryMode::Rename { path: path.clone() },
                value: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            }),
            _ => None,
        }
    }

    /// Dialog title for its mode.
    pub fn title(&self) -> &'static str {
        match &self.mode {
            FileEntryMode::NewFile { .. } => "New File",
            FileEntryMode::NewFolder { .. } => "New Folder",
            FileEntryMode::Rename { .. } => "Rename",
        }
    }
}

/// Validate a single path-segment name a user typed into a dialog. Rejects
/// empties, embedded separators (no sneaking paths through a "name"), and the
/// Windows-forbidden characters; returns the trimmed name when acceptable.
pub fn validate_entry_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name must not be empty".into());
    }
    if name.contains(['/', '\\']) {
        return Err("Name must not contain path separators".into());
    }
    if name.contains([':', '*', '?', '"', '<', '>', '|']) {
        return Err("Name contains characters forbidden by Windows".into());
    }
    if name == "." || name == ".." || name.chars().any(|c| c.is_control()) {
        return Err("Name is not a valid file or folder name".into());
    }
    Ok(name)
}

/// Create an empty file `dir/name`, returning its path. Fails when the name
/// is invalid, the parent is missing, or the entry already exists (never
/// clobbers).
pub fn create_file_on_disk(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = validate_entry_name(name)?;
    if !dir.is_dir() {
        return Err(format!("Target folder does not exist: {}", dir.display()));
    }
    let path = dir.join(name);
    if path.exists() {
        return Err(format!("{} already exists", name));
    }
    std::fs::File::create(&path).map_err(|e| format!("Could not create file: {e}"))?;
    Ok(path)
}

/// Create a folder `dir/name`. Fails on invalid names and collisions.
pub fn create_folder_on_disk(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = validate_entry_name(name)?;
    if !dir.is_dir() {
        return Err(format!("Target folder does not exist: {}", dir.display()));
    }
    let path = dir.join(name);
    if path.exists() {
        return Err(format!("{} already exists", name));
    }
    std::fs::create_dir(&path).map_err(|e| format!("Could not create folder: {e}"))?;
    Ok(path)
}

/// Rename `path` to `new_name` within its current parent, returning the new
/// path. Refuses separators in the new name, a missing source, and collisions
/// (including a case-only rename colliding with itself being a no-op).
pub fn rename_on_disk(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_name = validate_entry_name(new_name)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Cannot rename: entry has no parent folder".to_string())?;
    let target = parent.join(new_name);
    if target == path {
        return Err("The entry already has that name".into());
    }
    if target.exists() {
        return Err(format!("{} already exists", new_name));
    }
    std::fs::rename(path, &target).map_err(|e| format!("Could not rename: {e}"))?;
    Ok(target)
}

/// Delete a file, or a folder recursively. `protected` (the workspace root)
/// may never be deleted, nor anything that is not inside it — this keeps a
/// buggy menu path from wiping directories outside the project.
pub fn delete_on_disk(path: &Path, workspace_root: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("No longer exists: {}", path.display()));
    }
    if path == workspace_root {
        return Err("Cannot delete the workspace root".into());
    }
    // Everything deletable must live *under* the workspace root. (`..`
    // resolution differs per platform, so normalize by component walk.)
    let normalized = normalize(path);
    let root_normalized = normalize(workspace_root);
    if !normalized.starts_with(&root_normalized) {
        return Err(format!(
            "Refusing to delete outside the workspace: {}",
            path.display()
        ));
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| format!("Could not delete folder: {e}"))
    } else {
        std::fs::remove_file(path).map_err(|e| format!("Could not delete file: {e}"))
    }
}

/// Best-effort lexical normalization (resolves `.` and `..` segments) so the
/// containment check above isn't fooled by decorative path components.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when two paths name the same file for the editor's purposes. The same
/// file reaches the tab layer in different textual forms: the GUI bridge
/// canonicalizes to the Windows verbatim form (`\\?\C:\dir\file`), the file
/// tree spells paths however the workspace root was spelled (drive-letter case
/// included), and a hand-written hot-exit session may use any of these. Tab
/// dedupe and hot-exit overlay matching must compare identity, not spelling.
pub fn same_editor_path(a: &Path, b: &Path) -> bool {
    fn spoken(p: &Path) -> String {
        let s = p.to_string_lossy();
        // Windows accepts both separators, so `src\main.rs` and `src/main.rs`
        // name one file; go-to-symbol paths arrive slash-forward from the
        // merge layer while the tree spells them with backslashes.
        let s = s.replace('/', "\\");
        let s = s
            .strip_prefix(r"\\?\")
            .or_else(|| s.strip_prefix(r"\\.\"))
            .unwrap_or(&s);
        // Trailing separators decorate the same directory (`c:\ws\` == `c:\ws`).
        s.trim_end_matches('\\').to_lowercase()
    }
    if cfg!(windows) {
        spoken(a) == spoken(b)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn same_editor_path_matches_verbatim_case_and_trailing_spelling() {
        // Windows canonicalization (bridge OpenFile) vs workspace-root spelling.
        assert!(same_editor_path(
            Path::new(r"\\?\C:\Users\dev\proj\a.rs"),
            Path::new(r"c:\users\DEV\Proj\A.RS")
        ));
        // Trailing separator on a directory.
        assert!(same_editor_path(Path::new(r"c:\ws"), Path::new(r"C:\ws\")));
        // Mixed separators name one file on Windows (v50 live probe: a symbol
        // jump re-opened `src/main.rs` beside the tree's `src\main.rs` tab).
        assert!(same_editor_path(
            Path::new(r"c:\ws\src\main.rs"),
            Path::new("c:/ws/src/main.rs")
        ));
        assert!(same_editor_path(
            Path::new(r"\\?\c:\ws\src\main.rs"),
            Path::new("c:/ws/src/main.rs")
        ));
        // Different files stay different.
        assert!(!same_editor_path(
            Path::new(r"c:\ws\a.rs"),
            Path::new(r"c:\ws\b.rs")
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn same_editor_path_is_exact_off_windows() {
        assert!(same_editor_path(
            Path::new("/ws/a.rs"),
            Path::new("/ws/a.rs")
        ));
        assert!(!same_editor_path(
            Path::new("/ws/a.rs"),
            Path::new("/WS/A.RS")
        ));
    }

    /// Fresh per-test scratch directory under the system temp folder.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "velocity_file_ops_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entry_name_validation() {
        assert_eq!(validate_entry_name("  main.rs  "), Ok("main.rs"));
        assert!(validate_entry_name("").is_err());
        assert!(validate_entry_name("   ").is_err());
        assert!(validate_entry_name("a/b").is_err());
        assert!(validate_entry_name("a\\b").is_err());
        assert!(validate_entry_name("..").is_err());
        assert!(validate_entry_name("con:port").is_err());
        assert!(validate_entry_name("bad*?name").is_err());
        assert!(validate_entry_name("tab\tname").is_err());
    }

    #[test]
    fn create_file_succeeds_then_refuses_clobber() {
        let dir = scratch("create_file");
        let p = create_file_on_disk(&dir, "notes.md").unwrap();
        assert!(p.is_file());
        // Second create with the same name errors and leaves the file alone.
        std::fs::write(&p, "keep").unwrap();
        let err = create_file_on_disk(&dir, "notes.md").unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_folder_and_invalid_names_never_touch_disk() {
        let dir = scratch("create_dir");
        assert!(create_folder_on_disk(&dir, "sub").unwrap().is_dir());
        assert!(create_folder_on_disk(&dir, "sub").is_err());
        assert!(create_file_on_disk(&dir, "../escape").is_err());
        assert!(!dir.parent().unwrap().join("escape").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_moves_file_and_rejects_collisions() {
        let dir = scratch("rename");
        let a = dir.join("a.txt");
        std::fs::write(&a, "x").unwrap();
        let b = dir.join("b.txt");
        std::fs::write(&b, "y").unwrap();
        // Colliding rename errors and keeps both files.
        assert!(rename_on_disk(&a, "b.txt").is_err());
        // Same-name rename is a no-op error, not a self-delete.
        assert!(rename_on_disk(&a, "a.txt").is_err());
        assert!(a.is_file() && b.is_file());
        let moved = rename_on_disk(&a, "c.txt").unwrap();
        assert!(!a.exists() && moved.is_file());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_files_dirs_and_protects_root() {
        let dir = scratch("delete");
        let f = dir.join("gone.txt");
        std::fs::write(&f, "1").unwrap();
        delete_on_disk(&f, &dir).unwrap();
        assert!(!f.exists());
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("deep.txt"), "1").unwrap();
        delete_on_disk(&sub, &dir).unwrap();
        assert!(!sub.exists());
        // The workspace root itself can never be deleted.
        assert!(delete_on_disk(&dir, &dir).is_err());
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_refuses_paths_outside_workspace() {
        let parent = scratch("outside");
        let ws = parent.join("ws");
        std::fs::create_dir(&ws).unwrap();
        let outside = parent.join("victim.txt");
        std::fs::write(&outside, "precious").unwrap();
        assert!(delete_on_disk(&outside, &ws).is_err());
        assert!(outside.is_file(), "non-member path untouched");
        // Decorative `ws/../victim.txt` resolves to the outside path and is
        // still refused.
        assert!(delete_on_disk(&ws.join("..").join("victim.txt"), &ws).is_err());
        assert!(outside.is_file());
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn dialogs_prefill_rename_and_skip_non_prompt_actions() {
        let d =
            FileEntryDialog::for_action(&TreeAction::Rename(PathBuf::from("C:/proj/src/main.rs")))
                .unwrap();
        assert_eq!(d.value, "main.rs");
        assert_eq!(d.title(), "Rename");
        let n =
            FileEntryDialog::for_action(&TreeAction::NewFile(PathBuf::from("C:/proj"))).unwrap();
        assert_eq!(n.value, "");
        assert_eq!(n.title(), "New File");
        assert!(FileEntryDialog::for_action(&TreeAction::Delete(PathBuf::from("x"))).is_none());
        assert!(FileEntryDialog::for_action(&TreeAction::CopyPath(PathBuf::from("x"))).is_none());
    }
}
