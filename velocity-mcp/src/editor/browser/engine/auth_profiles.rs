use super::*;
use std::fs;
use std::path::{Path, PathBuf};

fn resolve_auth_profile_source(
    workspace_root: &Path,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
) -> Result<(String, BrowserSessionState), String> {
    let source_kind = if source_checkpoint_name.is_some() {
        "checkpoint".to_string()
    } else {
        "session".to_string()
    };
    let source = if let Some(checkpoint_name) = source_checkpoint_name {
        let checkpoint =
            read_session_checkpoint(workspace_root, source_session_id, checkpoint_name)?;
        checkpoint.session
    } else {
        load_session_state(workspace_root, source_session_id)?
    };
    Ok((source_kind, source))
}

fn build_auth_profile_from_source(
    workspace_root: &Path,
    source_kind: &str,
    profile_name: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    source: BrowserSessionState,
    sitemap_path: &Path,
) -> BrowserAuthProfile {
    let cookies = filter_auth_cookies(&source.cookies);
    let local_storage = filter_csrf_storage(&source.local_storage);
    let session_storage = filter_csrf_storage(&source.session_storage);
    let snapshot = match source.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot: &BrowserPageSnapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics =
        build_auth_diagnostics_report(workspace_root, source.clone(), snapshot, snapshot_json_path);
    BrowserAuthProfile {
        name: profile_name.to_string(),
        source_kind: source_kind.to_string(),
        source_session_id: source_session_id.to_string(),
        source_checkpoint_name: source_checkpoint_name.map(str::to_string),
        current_url: source.current_url,
        cookies,
        local_storage,
        session_storage,
        auth_diagnostics,
    }
}

fn write_auth_profile(
    workspace_root: &Path,
    profile: &BrowserAuthProfile,
) -> Result<PathBuf, String> {
    let path = browser_auth_profile_json_path(workspace_root, &profile.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create auth profile dir: {err}"))?;
    }
    let json = serde_json::to_vec_pretty(profile)
        .map_err(|err| format!("serialise auth profile: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write auth profile: {err}"))?;
    Ok(path)
}

pub fn load_auth_profile(
    workspace_root: &Path,
    profile_name: &str,
) -> Result<BrowserAuthProfile, String> {
    let path = browser_auth_profile_json_path(workspace_root, profile_name);
    let raw = fs::read(&path).map_err(|err| format!("read auth profile: {err}"))?;
    serde_json::from_slice(&raw).map_err(|err| format!("parse auth profile: {err}"))
}

pub fn save_auth_profile_report(
    workspace_root: &Path,
    profile_name: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserAuthProfileSaveReport, String> {
    let (source_kind, source) =
        resolve_auth_profile_source(workspace_root, source_session_id, source_checkpoint_name)?;
    let profile = build_auth_profile_from_source(
        workspace_root,
        &source_kind,
        profile_name,
        source_session_id,
        source_checkpoint_name,
        source,
        sitemap_path,
    );
    let path = write_auth_profile(workspace_root, &profile)?;
    let mut summary = summarize_auth_profile(profile);
    summary.json_path = Some(path.display().to_string());
    Ok(BrowserAuthProfileSaveReport {
        profile: summary,
        profile_json_path: path.display().to_string(),
    })
}

pub fn read_auth_profile_report(
    workspace_root: &Path,
    profile_name: &str,
) -> Result<BrowserAuthProfileReadReport, String> {
    let profile = load_auth_profile(workspace_root, profile_name)?;
    Ok(BrowserAuthProfileReadReport {
        profile: summarize_auth_profile(profile),
        profile_json_path: browser_auth_profile_json_path(workspace_root, profile_name)
            .display()
            .to_string(),
    })
}

pub fn list_auth_profiles(
    workspace_root: &Path,
    profile_name_contains: Option<&str>,
    source_session_id_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserAuthProfileSummary>, String> {
    let dir = workspace_root
        .join(".velocity")
        .join("browser-auth-profiles");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read auth profile dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read auth profile dir entry: {err}")),
        };
        let path = entry.path();
        if path
            .extension()
            .and_then(|ext: &std::ffi::OsStr| ext.to_str())
            != Some("json")
        {
            continue;
        }
        if path
            .file_name()
            .and_then(|name: &std::ffi::OsStr| name.to_str())
            .map(|name: &str| !name.ends_with(".auth.json"))
            .unwrap_or(true)
        {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read auth profile: {err}"))?;
        let profile: BrowserAuthProfile =
            serde_json::from_slice(&raw).map_err(|err| format!("parse auth profile: {err}"))?;
        let mut summary = summarize_auth_profile(profile);
        summary.json_path = Some(path.display().to_string());
        if profile_name_contains
            .map(|needle| contains_case_insensitive(&summary.name, needle))
            .unwrap_or(true)
            && source_session_id_contains
                .map(|needle| contains_case_insensitive(&summary.source_session_id, needle))
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.name.cmp(&right.name)
    });
    Ok(items)
}

pub fn apply_auth_profile_report(
    workspace_root: &Path,
    profile_name: &str,
    target_session_id: &str,
    sitemap_path: &Path,
) -> Result<BrowserAuthProfileApplyReport, String> {
    let profile = load_auth_profile(workspace_root, profile_name)?;
    let profile_json_path = browser_auth_profile_json_path(workspace_root, profile_name)
        .display()
        .to_string();
    let mut target = load_session_state(workspace_root, target_session_id)?;

    for cookie in profile.cookies.iter().cloned() {
        merge_cookie(&mut target.cookies, cookie);
    }
    apply_storage_updates(&mut target.local_storage, &profile.local_storage);
    apply_storage_updates(&mut target.session_storage, &profile.session_storage);

    let session_path = save_session_state(workspace_root, &target)?;
    let snapshot = match target.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot: &BrowserPageSnapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics =
        build_auth_diagnostics_report(workspace_root, target.clone(), snapshot, snapshot_json_path);

    Ok(BrowserAuthProfileApplyReport {
        profile_name: profile.name,
        target_session: summarize_session(target),
        copied_cookie_count: profile.cookies.len(),
        copied_cookie_names: summarize_cookie_names(&profile.cookies),
        copied_local_storage_count: profile.local_storage.len(),
        copied_local_storage_keys: summarize_sorted_keys(&profile.local_storage),
        copied_session_storage_count: profile.session_storage.len(),
        copied_session_storage_keys: summarize_sorted_keys(&profile.session_storage),
        session_json_path: session_path.display().to_string(),
        profile_json_path,
        auth_diagnostics,
    })
}

pub fn reseed_auth_state_report(
    workspace_root: &Path,
    target_session_id: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    sitemap_path: &Path,
) -> Result<BrowserAuthReseedReport, String> {
    let (source_kind, source) =
        resolve_auth_profile_source(workspace_root, source_session_id, source_checkpoint_name)?;
    let copied_cookies = filter_auth_cookies(&source.cookies);
    let copied_local_storage = filter_csrf_storage(&source.local_storage);
    let copied_session_storage = filter_csrf_storage(&source.session_storage);
    let mut target = load_session_state(workspace_root, target_session_id)?;

    for cookie in copied_cookies.iter().cloned() {
        merge_cookie(&mut target.cookies, cookie);
    }
    apply_storage_updates(&mut target.local_storage, &copied_local_storage);
    apply_storage_updates(&mut target.session_storage, &copied_session_storage);

    let session_path = save_session_state(workspace_root, &target)?;
    let snapshot = match target.current_url.as_deref() {
        Some(url) => load_snapshot_json(url, sitemap_path).ok(),
        None => None,
    };
    let snapshot_json_path = snapshot.as_ref().map(|snapshot: &BrowserPageSnapshot| {
        browser_snapshot_path(&snapshot.url, sitemap_path)
            .display()
            .to_string()
    });
    let auth_diagnostics =
        build_auth_diagnostics_report(workspace_root, target.clone(), snapshot, snapshot_json_path);

    Ok(BrowserAuthReseedReport {
        target_session: summarize_session(target),
        source_kind,
        source_session_id: source_session_id.to_string(),
        source_checkpoint_name: source_checkpoint_name.map(str::to_string),
        copied_cookie_count: copied_cookies.len(),
        copied_cookie_names: summarize_cookie_names(&copied_cookies),
        copied_local_storage_count: copied_local_storage.len(),
        copied_local_storage_keys: summarize_sorted_keys(&copied_local_storage),
        copied_session_storage_count: copied_session_storage.len(),
        copied_session_storage_keys: summarize_sorted_keys(&copied_session_storage),
        session_json_path: session_path.display().to_string(),
        auth_diagnostics,
    })
}

pub fn reseed_runtime_auth_state_report(
    workspace_root: &Path,
    target_session_id: &str,
    source_session_id: &str,
    source_checkpoint_name: Option<&str>,
    sitemap_path: &Path,
    wait_timeout_ms: Option<u64>,
) -> Result<RuntimeAuthReseedReport, String> {
    let (source_kind, source) =
        resolve_auth_profile_source(workspace_root, source_session_id, source_checkpoint_name)?;
    let copied_cookies = filter_auth_cookies(&source.cookies);
    let copied_runtime_cookies = auth_runtime_cookies_for_source(&source);
    let copied_local_storage = filter_csrf_storage(&source.local_storage);
    let copied_session_storage = filter_csrf_storage(&source.session_storage);
    let mut target = load_runtime_session_state(workspace_root, target_session_id)?;
    let request_body = serde_json::json!({
        "url": target.current_url,
        "cookies": copied_runtime_cookies,
        "localStorage": copied_local_storage,
        "sessionStorage": copied_session_storage,
        "waitTimeoutMs": wait_timeout_ms.unwrap_or(1_000),
    });
    let value = runtime_api_request(
        "POST",
        &format!(
            "{}/api/runtime/session/{}/state",
            target.api_base, target.runtime_session_id
        ),
        Some(&request_body),
    )?;
    let warnings = parse_runtime_string_list(value.get("warnings"));

    for cookie in auth_runtime_cookies_for_source(&source) {
        merge_runtime_cookie(&mut target.cookies, cookie);
    }
    apply_storage_updates(&mut target.local_storage, &copied_local_storage);
    apply_storage_updates(&mut target.session_storage, &copied_session_storage);

    let session_path = save_runtime_session_state(workspace_root, &target)?;
    let auth_diagnostics =
        build_runtime_auth_diagnostics_report(workspace_root, &target, sitemap_path);

    Ok(RuntimeAuthReseedReport {
        target_runtime_session: target,
        source_kind,
        source_session_id: source_session_id.to_string(),
        source_checkpoint_name: source_checkpoint_name.map(str::to_string),
        copied_cookie_count: copied_cookies.len(),
        copied_cookie_names: summarize_cookie_names(&copied_cookies),
        copied_local_storage_count: copied_local_storage.len(),
        copied_local_storage_keys: summarize_sorted_keys(&copied_local_storage),
        copied_session_storage_count: copied_session_storage.len(),
        copied_session_storage_keys: summarize_sorted_keys(&copied_session_storage),
        session_json_path: session_path.display().to_string(),
        auth_diagnostics,
        warning_count: warnings.len(),
        warnings,
    })
}
