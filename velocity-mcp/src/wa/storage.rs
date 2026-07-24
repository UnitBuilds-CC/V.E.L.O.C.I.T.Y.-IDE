use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::wa::model::{
    WaListSortDirection, WaNode, WaRunArtifactReport, WaRunListEntry, WaScript,
    WaScriptReadReport, WaScriptRunReport, WaScriptSaveReport, WaSession,
    WaSessionCreateReport, WaSessionListEntry, WaSessionReadReport, WaSnapshot,
    WaSnapshotListEntry, WaSnapshotReadReport, WaSnapshotSaveReport,
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn slugify(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "wa-artifact".to_string()
    } else {
        out
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn workspace_root_of(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|value| value.to_str()) == Some(".velocity") {
            return ancestor.parent().map(Path::to_path_buf);
        }
    }
    None
}

/// Read a `.nda` artifact, transparently decrypting an AES-256-GCM envelope when
/// present and passing legacy plaintext through unchanged.
fn read_nda_text(path: &Path) -> Result<String, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let plain = match workspace_root_of(path) {
        Some(root) => crate::agent::crypto::open(&root, b"wa", &bytes),
        None => bytes,
    };
    Ok(String::from_utf8_lossy(&plain).into_owned())
}

/// Write a `.nda` artifact sealed with AES-256-GCM, falling back to plaintext
/// only if key material is unavailable (so an artifact is never lost).
fn write_nda_text(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    let bytes = match workspace_root_of(path) {
        Some(root) => crate::agent::crypto::seal(&root, b"wa", text.as_bytes())
            .unwrap_or_else(|| text.as_bytes().to_vec()),
        None => text.as_bytes().to_vec(),
    };
    fs::write(path, bytes)?;
    Ok(())
}

fn ensure_velocity_dir(root: &Path, child: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = root.join(".velocity").join(child);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn session_json_legacy_path(root: &Path, session_id: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-sessions")?.join(format!("{}.json", slugify(session_id))))
}

fn session_nda_path(root: &Path, session_id: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-sessions")?.join(format!("{}.nda", slugify(session_id))))
}

fn snapshot_stem(session_id: &str, snapshot_name: &str) -> String {
    format!("{}--{}", slugify(session_id), slugify(snapshot_name))
}

fn snapshot_json_legacy_path(root: &Path, session_id: &str, snapshot_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-snapshots")?
        .join(format!("{}.json", snapshot_stem(session_id, snapshot_name))))
}

fn snapshot_nda_path(root: &Path, session_id: &str, snapshot_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-snapshots")?
        .join(format!("{}.nda", snapshot_stem(session_id, snapshot_name))))
}

#[allow(dead_code)]
fn script_json_legacy_path(root: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-scripts")?.join(format!("{}.wa.json", slugify(name))))
}

fn script_nda_path(root: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-scripts")?.join(format!("{}.wa.nda", slugify(name))))
}

fn run_nda_path(root: &Path, run_id: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ensure_velocity_dir(root, "wa-runs")?.join(format!("{}.wa-run.nda", slugify(run_id))))
}

fn script_nda_path_from_read_path(path: &Path) -> PathBuf {
    let as_text = path.to_string_lossy();
    if let Some(prefix) = as_text.strip_suffix(".wa.json") {
        PathBuf::from(format!("{prefix}.wa.nda"))
    } else {
        path.to_path_buf()
    }
}

fn count_session_snapshots(root: &Path, session_id: &str) -> Result<u32, Box<dyn Error>> {
    let dir = ensure_velocity_dir(root, "wa-snapshots")?;
    let prefix = format!("{}--", slugify(session_id));
    let mut stems = std::collections::BTreeSet::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let extension = path.extension().and_then(|value| value.to_str());
        if extension != Some("nda") && extension != Some("json") {
            continue;
        }
        let stem = match path.file_stem().and_then(|value| value.to_str()) {
            Some(value) => value,
            None => continue,
        };
        if stem.starts_with(&prefix) {
            stems.insert(stem.to_string());
        }
    }
    Ok(stems.len() as u32)
}

pub fn parse_list_sort_direction(value: Option<&str>) -> Result<WaListSortDirection, String> {
    match value {
        None => Ok(WaListSortDirection::Asc),
        Some(direction) if direction.eq_ignore_ascii_case("asc") => Ok(WaListSortDirection::Asc),
        Some(direction) if direction.eq_ignore_ascii_case("desc") => Ok(WaListSortDirection::Desc),
        Some(direction) => Err(format!(
            "invalid sort direction '{direction}', expected 'asc' or 'desc'"
        )),
    }
}

pub fn load_session(root: &Path, session_id: &str) -> Result<WaSession, Box<dyn Error>> {
    let nda_path = session_nda_path(root, session_id)?;
    if nda_path.exists() {
        return crate::wa::nda::deserialize_session_nda(&read_nda_text(&nda_path)?);
    }
    let legacy_path = session_json_legacy_path(root, session_id)?;
    let content = fs::read_to_string(legacy_path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn load_snapshot(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
) -> Result<WaSnapshot, Box<dyn Error>> {
    let nda_path = snapshot_nda_path(root, session_id, snapshot_name)?;
    if nda_path.exists() {
        return crate::wa::nda::deserialize_snapshot_nda(&read_nda_text(&nda_path)?);
    }
    let legacy_path = snapshot_json_legacy_path(root, session_id, snapshot_name)?;
    let content = fs::read_to_string(legacy_path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn load_script(path: &Path) -> Result<WaScript, Box<dyn Error>> {
    let nda_path = script_nda_path_from_read_path(path);
    if nda_path.exists() {
        return crate::wa::nda::deserialize_script_nda(&read_nda_text(&nda_path)?);
    }
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn create_session_report(
    root: &Path,
    session_id: &str,
) -> Result<WaSessionCreateReport, Box<dyn Error>> {
    let timestamp = now_ms();
    let session = WaSession {
        id: session_id.to_string(),
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
        latest_snapshot_name: None,
        latest_snapshot_nda_path: None,
        snapshot_count: 0,
    };
    let nda_path = session_nda_path(root, session_id)?;
    write_nda_text(&nda_path, &crate::wa::nda::serialize_session_nda(&session))?;
    Ok(WaSessionCreateReport {
        session,
        session_nda_path: relative_path(root, &nda_path),
    })
}

pub fn get_session_report(
    root: &Path,
    session_id: &str,
) -> Result<WaSessionReadReport, Box<dyn Error>> {
    let session = load_session(root, session_id)?;
    let nda_path = session_nda_path(root, session_id)?;
    Ok(WaSessionReadReport {
        session,
        session_nda_path: relative_path(root, &nda_path),
    })
}

pub fn list_sessions(
    root: &Path,
    session_id_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: WaListSortDirection,
) -> Result<Vec<WaSessionListEntry>, Box<dyn Error>> {
    let dir = ensure_velocity_dir(root, "wa-sessions")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("nda") {
            continue;
        }
        let session = crate::wa::nda::deserialize_session_nda(&read_nda_text(&path)?)?;
        if let Some(filter) = session_id_contains {
            if !session.id.to_ascii_lowercase().contains(&filter.to_ascii_lowercase()) {
                continue;
            }
        }
        entries.push(WaSessionListEntry {
            id: session.id.clone(),
            snapshot_count: session.snapshot_count,
            latest_snapshot_name: session.latest_snapshot_name.clone(),
            updated_at_ms: session.updated_at_ms,
            session_nda_path: relative_path(root, &path),
        });
    }
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    if matches!(sort_direction, WaListSortDirection::Desc) {
        entries.reverse();
    }
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    Ok(entries)
}

pub fn save_snapshot_report(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
    url: &str,
    title: &str,
    focus_node_id: Option<&str>,
    nodes: Vec<WaNode>,
) -> Result<WaSnapshotSaveReport, Box<dyn Error>> {
    let mut session = load_session(root, session_id)?;
    let snapshot = WaSnapshot {
        session_id: session_id.to_string(),
        snapshot_name: snapshot_name.to_string(),
        created_at_ms: now_ms(),
        url: url.to_string(),
        title: title.to_string(),
        focus_node_id: focus_node_id.map(|value| value.to_string()),
        nodes,
    };
    let snapshot_nda = snapshot_nda_path(root, session_id, snapshot_name)?;
    write_nda_text(&snapshot_nda, &crate::wa::nda::serialize_snapshot_nda(&snapshot))?;

    session.updated_at_ms = now_ms();
    session.latest_snapshot_name = Some(snapshot_name.to_string());
    session.latest_snapshot_nda_path = Some(relative_path(root, &snapshot_nda));
    session.snapshot_count = count_session_snapshots(root, session_id)?;
    let session_nda = session_nda_path(root, session_id)?;
    write_nda_text(&session_nda, &crate::wa::nda::serialize_session_nda(&session))?;

    Ok(WaSnapshotSaveReport {
        snapshot,
        snapshot_nda_path: relative_path(root, &snapshot_nda),
        session_nda_path: relative_path(root, &session_nda),
    })
}

pub fn read_snapshot_report(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
) -> Result<WaSnapshotReadReport, Box<dyn Error>> {
    let snapshot = load_snapshot(root, session_id, snapshot_name)?;
    let nda_path = snapshot_nda_path(root, session_id, snapshot_name)?;
    Ok(WaSnapshotReadReport {
        snapshot,
        snapshot_nda_path: relative_path(root, &nda_path),
    })
}

pub fn list_snapshots(
    root: &Path,
    session_id: Option<&str>,
    snapshot_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: WaListSortDirection,
) -> Result<Vec<WaSnapshotListEntry>, Box<dyn Error>> {
    let dir = ensure_velocity_dir(root, "wa-snapshots")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("nda") {
            continue;
        }
        let snapshot = crate::wa::nda::deserialize_snapshot_nda(&read_nda_text(&path)?)?;
        if let Some(expected_session_id) = session_id {
            if snapshot.session_id != expected_session_id {
                continue;
            }
        }
        if let Some(filter) = snapshot_name_contains {
            if !snapshot
                .snapshot_name
                .to_ascii_lowercase()
                .contains(&filter.to_ascii_lowercase())
            {
                continue;
            }
        }
        entries.push(WaSnapshotListEntry {
            session_id: snapshot.session_id.clone(),
            snapshot_name: snapshot.snapshot_name.clone(),
            url: snapshot.url.clone(),
            title: snapshot.title.clone(),
            node_count: snapshot.nodes.len(),
            created_at_ms: snapshot.created_at_ms,
            snapshot_nda_path: relative_path(root, &path),
        });
    }
    entries.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then(left.snapshot_name.cmp(&right.snapshot_name))
    });
    if matches!(sort_direction, WaListSortDirection::Desc) {
        entries.reverse();
    }
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    Ok(entries)
}

pub fn save_script_report(
    root: &Path,
    name: &str,
    start_url: Option<&str>,
    steps: Vec<crate::wa::model::WaScriptStep>,
) -> Result<WaScriptSaveReport, Box<dyn Error>> {
    let script = WaScript {
        name: name.to_string(),
        created_at_ms: now_ms(),
        start_url: start_url.map(|value| value.to_string()),
        steps,
    };
    let nda_path = script_nda_path(root, name)?;
    write_nda_text(&nda_path, &crate::wa::nda::serialize_script_nda(&script))?;
    Ok(WaScriptSaveReport {
        script,
        relative_file_path: relative_path(root, &nda_path),
        nda_path: relative_path(root, &nda_path),
    })
}

pub fn read_script_report(root: &Path, path: &Path) -> Result<WaScriptReadReport, Box<dyn Error>> {
    let script = load_script(path)?;
    let nda_path = script_nda_path_from_read_path(path);
    Ok(WaScriptReadReport {
        script,
        relative_file_path: relative_path(root, &nda_path),
        nda_path: relative_path(root, &nda_path),
    })
}

pub fn list_scripts(
    root: &Path,
    script_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: WaListSortDirection,
) -> Result<Vec<WaScriptReadReport>, Box<dyn Error>> {
    let dir = ensure_velocity_dir(root, "wa-scripts")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".wa.nda") {
            continue;
        }
        let script = crate::wa::nda::deserialize_script_nda(&read_nda_text(&path)?)?;
        if let Some(filter) = script_name_contains {
            if !script.name.to_ascii_lowercase().contains(&filter.to_ascii_lowercase()) {
                continue;
            }
        }
        entries.push(WaScriptReadReport {
            script,
            relative_file_path: relative_path(root, &path),
            nda_path: relative_path(root, &path),
        });
    }
    entries.sort_by(|left, right| left.script.name.cmp(&right.script.name));
    if matches!(sort_direction, WaListSortDirection::Desc) {
        entries.reverse();
    }
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    Ok(entries)
}

pub fn save_run_report(
    root: &Path,
    report: &WaScriptRunReport,
) -> Result<WaRunArtifactReport, Box<dyn Error>> {
    let nda_path = run_nda_path(root, &report.run_id)?;
    write_nda_text(&nda_path, &crate::wa::nda::serialize_run_nda(report))?;
    Ok(WaRunArtifactReport {
        run: report.clone(),
        relative_file_path: relative_path(root, &nda_path),
        nda_path: relative_path(root, &nda_path),
    })
}

pub fn read_run_report(root: &Path, path: &Path) -> Result<WaRunArtifactReport, Box<dyn Error>> {
    let run = crate::wa::nda::deserialize_run_nda(&read_nda_text(path)?)?;
    Ok(WaRunArtifactReport {
        run,
        relative_file_path: relative_path(root, path),
        nda_path: relative_path(root, path),
    })
}

pub fn list_runs(
    root: &Path,
    session_id: Option<&str>,
    script_name_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: WaListSortDirection,
) -> Result<Vec<WaRunListEntry>, Box<dyn Error>> {
    let dir = ensure_velocity_dir(root, "wa-runs")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".wa-run.nda") {
            continue;
        }
        let run = crate::wa::nda::deserialize_run_nda(&read_nda_text(&path)?)?;
        if let Some(expected_session_id) = session_id {
            if run.session_id != expected_session_id {
                continue;
            }
        }
        if let Some(filter) = script_name_contains {
            if !run.script_name.to_ascii_lowercase().contains(&filter.to_ascii_lowercase()) {
                continue;
            }
        }
        entries.push(WaRunListEntry {
            run_id: run.run_id.clone(),
            session_id: run.session_id.clone(),
            snapshot_name: run.snapshot_name.clone(),
            script_name: run.script_name.clone(),
            start_step_index: run.start_step_index,
            step_count: run.step_count,
            completed_step_count: run.completed_step_count,
            verified_step_count: run.verified_step_count,
            succeeded: run.succeeded,
            stopped_at_step_index: run.stopped_at_step_index,
            created_at_ms: run.created_at_ms,
            nda_path: relative_path(root, &path),
        });
    }
    entries.sort_by(|left, right| {
        left.created_at_ms
            .cmp(&right.created_at_ms)
            .then(left.run_id.cmp(&right.run_id))
    });
    if matches!(sort_direction, WaListSortDirection::Desc) {
        entries.reverse();
    }
    if let Some(limit) = limit {
        entries.truncate(limit);
    }
    Ok(entries)
}
