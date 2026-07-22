use super::super::TaskId;
use super::types::ScopedPaths;
use crate::automation::mediator::MediatorArena;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use velocity_ide::site_map::SiteMap;

pub fn acquire_scope_locks(
    workspace_root: &Path,
    scope: &[String],
    mediator: &Arc<MediatorArena>,
    site_map: &SiteMap,
    task_id: TaskId,
) -> Result<Vec<PathBuf>, String> {
    let agent_id = format!("task-{}", task_id.0);
    let mut locked_scopes: Vec<PathBuf> = Vec::new();
    for entry in scope {
        let rel = PathBuf::from(entry);
        let abs = if rel.is_absolute() {
            rel
        } else {
            workspace_root.join(&rel)
        };
        if let Err(conflict) =
            mediator.acquire_lock(abs.clone(), (1, usize::MAX / 4), agent_id.clone(), site_map)
        {
            for locked in &locked_scopes {
                mediator.release_lock(locked, &agent_id);
            }
            return Err(mediator.resolve_conflict(&conflict));
        }
        locked_scopes.push(abs);
    }
    Ok(locked_scopes)
}

pub fn collect_scoped_paths(workspace_root: &Path, scope: &[String]) -> ScopedPaths {
    let scope_roots = scope
        .iter()
        .map(PathBuf::from)
        .map(|rel_path| {
            if rel_path.is_absolute() {
                rel_path
            } else {
                workspace_root.join(rel_path)
            }
        })
        .collect::<Vec<_>>();
    let explicit_files = scope_roots
        .iter()
        .filter(|path| !path.is_dir())
        .cloned()
        .collect::<Vec<_>>();
    ScopedPaths {
        explicit_files,
        scope_roots,
    }
}

pub fn snapshot_scope(
    scoped_paths: &ScopedPaths,
    workspace_root: &Path,
    snapshot_root: &Path,
) -> Result<HashMap<PathBuf, Option<Vec<u8>>>, String> {
    let mut before_contents = HashMap::new();
    for abs_path in collect_candidate_files(scoped_paths)? {
        let rel_path = abs_path.strip_prefix(workspace_root).unwrap_or(&abs_path);
        let dest = snapshot_root.join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("create snapshot parent: {err}"))?;
        }
        let bytes = read_scoped_file(&abs_path)
            .map_err(|err| format!("snapshot file {}: {err}", abs_path.display()))?;
        if let Some(bytes) = &bytes {
            fs::write(&dest, bytes)
                .map_err(|err| format!("write snapshot {}: {err}", dest.display()))?;
        }
        before_contents.insert(abs_path, bytes);
    }
    Ok(before_contents)
}

pub fn detect_scoped_changes(
    scoped_paths: &ScopedPaths,
    before_contents: &HashMap<PathBuf, Option<Vec<u8>>>,
    workspace_root: &Path,
) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let mut changed = Vec::new();
    let mut created = Vec::new();
    let mut deleted = Vec::new();
    let mut candidate_paths = before_contents.keys().cloned().collect::<BTreeSet<_>>();
    for abs_path in collect_candidate_files(scoped_paths)? {
        candidate_paths.insert(abs_path);
    }
    for abs_path in candidate_paths {
        let before = before_contents.get(&abs_path).cloned().flatten();
        let after = read_scoped_file(&abs_path)
            .map_err(|err| format!("read post-run file {}: {err}", abs_path.display()))?;
        let rel = abs_path
            .strip_prefix(workspace_root)
            .unwrap_or(&abs_path)
            .display()
            .to_string();
        match (before, after) {
            (Some(before_bytes), Some(after_bytes)) if before_bytes != after_bytes => {
                changed.push(rel)
            }
            (None, Some(_)) => created.push(rel),
            (Some(_), None) => deleted.push(rel),
            _ => {}
        }
    }
    Ok((changed, created, deleted))
}

pub fn collect_candidate_files(scoped_paths: &ScopedPaths) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for path in &scoped_paths.explicit_files {
        push_unique_path(&mut files, path.clone());
    }
    for root in &scoped_paths.scope_roots {
        collect_existing_files(root, &mut files)?;
    }
    Ok(files)
}

pub fn collect_existing_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        push_unique_path(files, path.to_path_buf());
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }
    for entry in
        fs::read_dir(path).map_err(|err| format!("read scope dir {}: {err}", path.display()))?
    {
        let entry =
            entry.map_err(|err| format!("read scope dir entry {}: {err}", path.display()))?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_existing_files(&entry_path, files)?;
        } else if entry_path.is_file() {
            push_unique_path(files, entry_path);
        }
    }
    Ok(())
}

pub fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

pub fn read_scoped_file(path: &Path) -> Result<Option<Vec<u8>>, std::io::Error> {
    if path.is_dir() {
        return Ok(None);
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn detect_out_of_scope_created_files(
    scoped_paths: &ScopedPaths,
    before_workspace_files: &BTreeSet<PathBuf>,
    workspace_root: &Path,
) -> Result<Vec<String>, String> {
    let after_workspace_files = collect_workspace_files(workspace_root)?;
    let mut created = after_workspace_files
        .difference(before_workspace_files)
        .filter(|path| !is_path_within_scope(path, scoped_paths, workspace_root))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    created.sort();
    Ok(created)
}

pub fn collect_workspace_files(workspace_root: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let mut files = BTreeSet::new();
    collect_workspace_files_recursive(workspace_root, workspace_root, &mut files)?;
    Ok(files)
}

fn collect_workspace_files_recursive(
    workspace_root: &Path,
    current: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)
        .map_err(|err| format!("read workspace dir {}: {err}", current.display()))?
    {
        let entry = entry
            .map_err(|err| format!("read workspace dir entry {}: {err}", current.display()))?;
        let entry_path = entry.path();
        let rel_path = entry_path
            .strip_prefix(workspace_root)
            .unwrap_or(&entry_path)
            .to_path_buf();
        if entry_path.is_dir() {
            if should_skip_workspace_dir(&rel_path) {
                continue;
            }
            collect_workspace_files_recursive(workspace_root, &entry_path, files)?;
        } else if entry_path.is_file() && !should_skip_workspace_file(&rel_path) {
            files.insert(rel_path);
        }
    }
    Ok(())
}

fn should_skip_workspace_dir(rel_path: &Path) -> bool {
    rel_path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | ".velocity" | "target" | "node_modules" | "archive")
        )
    })
}

fn should_skip_workspace_file(rel_path: &Path) -> bool {
    rel_path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | ".velocity" | "target" | "node_modules" | "archive")
        )
    })
}

fn is_path_within_scope(
    rel_path: &Path,
    scoped_paths: &ScopedPaths,
    workspace_root: &Path,
) -> bool {
    scoped_paths.scope_roots.iter().any(|root| {
        let root_rel = root.strip_prefix(workspace_root).unwrap_or(root);
        rel_path == root_rel || rel_path.starts_with(root_rel)
    })
}
