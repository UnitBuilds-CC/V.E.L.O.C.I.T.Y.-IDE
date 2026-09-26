//! Disk hygiene: build-artifact tracking and safe one-click cleanup.
//!
//! SSD bloat from build artifacts is the quiet failure mode of every IDE
//! session: one `cargo build` here, one `npm install` there, and the drive is
//! full before anyone noticed. Until now cleanup happened only because the
//! agent remembered to run `cargo clean`; this module makes the IDE itself
//! the housekeeper.
//!
//! Design rules (from the Disk Hygiene plan):
//! - Nothing is deleted that doesn't match a known artifact family *plus* its
//!   context manifest (a `target/` is only cargo's when a `Cargo.toml` sits
//!   beside it). Ambiguous matches are classified `Review` and reported only.
//! - Cleanup is never automatic: this module scans, tracks, nags, and deletes
//!   only when a human clicks or an approved tool call asks.
//! - `.velocity*`, top-level `memory/`, and `.git/` are never entered; symlink
//!   and junction (reparse) entries are never followed or deleted.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Free-space floor below which the IDE nags about reclaiming artifacts.
pub const DISK_PRESSURE_FLOOR_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// A single artifact tree bigger than this is worth a nag on its own.
pub const BIG_TREE_NAG_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// BFS from the workspace root stops descending at this depth, so nested
/// repos are covered without walking the entire disk on huge trees.
const MAX_WALK_DEPTH: usize = 6;
/// Growth below this is noise; provenance records skip it.
const GROWTH_DETAIL_FLOOR_BYTES: i64 = 64 * 1024;
/// Provenance history is capped so the manifest cannot grow unboundedly.
const MANIFEST_HISTORY_LIMIT: usize = 50;

// ─── Classification ─────────────────────────────────────────────────────────

/// Whether a matched tree may be cleaned, or is reported for human review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    /// Matches the artifact family *and* its context manifest.
    Safe,
    /// Name looks like an artifact but the marker is absent; report-only.
    Review,
}

/// A directory matched against the artifact table.
#[derive(Clone, Debug)]
pub struct Classification {
    pub kind: &'static str,
    pub safety: Safety,
    /// Why a `Review` classification was chosen (shown in UI and reports).
    pub reason: Option<String>,
}

impl Classification {
    fn safe(kind: &'static str) -> Self {
        Self {
            kind,
            safety: Safety::Safe,
            reason: None,
        }
    }
    fn gated(kind: &'static str, dir_name: &str, marker: &str, present: bool) -> Self {
        Self {
            kind,
            safety: if present { Safety::Safe } else { Safety::Review },
            reason: (!present).then(|| {
                format!(
                    "'{dir_name}' has no {marker} beside it — may be hand-written content, not build output"
                )
            }),
        }
    }
}

/// Match one directory name against the artifact table. `parent` is the
/// directory *containing* the candidate, used for the context-marker checks.
pub fn classify(dir_name: &str, parent: &Path) -> Option<Classification> {
    match dir_name {
        "node_modules" => return Some(Classification::safe("node-modules")),
        "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" => {
            return Some(Classification::safe("python-cache"))
        }
        "target" => {
            return Some(Classification::gated(
                "cargo-target",
                dir_name,
                "Cargo.toml",
                parent.join("Cargo.toml").is_file(),
            ))
        }
        ".next" | ".turbo" | "dist" | "build" | "out" => {
            return Some(Classification::gated(
                "js-build",
                dir_name,
                "package.json",
                parent.join("package.json").is_file(),
            ))
        }
        ".venv" | "venv" => {
            let present = parent.join("pyproject.toml").is_file()
                || parent.join("requirements.txt").is_file();
            return Some(Classification::gated(
                "venv",
                dir_name,
                "pyproject.toml or requirements.txt",
                present,
            ));
        }
        "Debug" | "obj" => {
            return Some(Classification::gated(
                "dotnet",
                dir_name,
                "a .sln or .csproj",
                has_sibling_with_extension(parent, &["sln", "csproj"]),
            ))
        }
        "cache" if parent.file_name().is_some_and(|n| n == ".wrangler") => {
            return Some(Classification::safe("wrangler-cache"))
        }
        _ => {}
    }
    if dir_name.ends_with(".egg-info") {
        return Some(Classification::safe("egg-info"));
    }
    None
}

fn has_sibling_with_extension(parent: &Path, extensions: &[&str]) -> bool {
    fs::read_dir(parent)
        .map(|rd| {
            rd.flatten().any(|e| {
                e.path().is_file()
                    && e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| extensions.iter().any(|g| x.eq_ignore_ascii_case(g)))
            })
        })
        .unwrap_or(false)
}

/// Directories the walker must never enter, whatever they contain.
fn is_protected_dir(name: &str, at_root: bool) -> bool {
    name.starts_with(".velocity") || name == ".git" || (at_root && name == "memory")
}

// ─── Report types ───────────────────────────────────────────────────────────

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn system_time_unix(t: SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// One artifact tree found by the scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactEntry {
    /// Workspace-relative path with `/` separators.
    pub relative_path: String,
    pub kind: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub last_modified_unix: Option<i64>,
    pub safety: Safety,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Result of a full workspace scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HygieneReport {
    pub scanned_at_unix: i64,
    pub entries: Vec<ArtifactEntry>,
    /// Sum of sizes for `Safe` entries only — what cleanup can free.
    pub total_reclaimable_bytes: u64,
    /// Sum of all entries including `Review` rows (reported, never cleaned).
    pub total_reported_bytes: u64,
    pub free_space_bytes: u64,
}

/// What [`clean`] did (or would do, when dry-run).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CleanResult {
    pub dry_run: bool,
    pub freed_bytes: u64,
    pub file_count: u64,
    pub removed: Vec<RemovedTree>,
    /// Human-readable refusals: path + why it was not touched.
    pub rejected: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemovedTree {
    pub path: String,
    pub size_bytes: u64,
    pub file_count: u64,
}

// ─── Scanning ───────────────────────────────────────────────────────────────

/// Walk one artifact tree, totalling bytes, file count and newest mtime.
/// Symlinks are counted neither way: never followed, never descended.
fn measure_tree(dir: &Path) -> (u64, u64, Option<i64>) {
    let mut stack = vec![dir.to_path_buf()];
    let mut size = 0u64;
    let mut files = 0u64;
    let mut newest: Option<i64> = None;
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(meta) = e.path().symlink_metadata() else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                size = size.saturating_add(meta.len());
                files += 1;
                if let Some(t) = meta.modified().ok().and_then(system_time_unix) {
                    newest = Some(newest.map_or(t, |n: i64| n.max(t)));
                }
            }
        }
    }
    (size, files, newest)
}

/// Breadth-first scan of `root` for artifact trees. Never enters protected
/// directories, never follows symlinks/junctions, and never descends past
/// [`MAX_WALK_DEPTH`] (matched trees are measured, not walked for matches).
pub fn scan(root: &Path) -> HygieneReport {
    let mut entries: Vec<ArtifactEntry> = Vec::new();
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((PathBuf::new(), 0));
    while let Some((rel, depth)) = queue.pop_front() {
        let abs = root.join(&rel);
        let Ok(rd) = fs::read_dir(&abs) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(meta) = e.path().symlink_metadata() else {
                continue;
            };
            // Only real directories are candidates; symlinks (and Windows
            // junctions, which std reports as symlinks) are skipped outright.
            if !meta.file_type().is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if is_protected_dir(&name, rel.as_os_str().is_empty()) {
                continue;
            }
            if let Some(cls) = classify(&name, &abs) {
                let (size, files, lm) = measure_tree(&e.path());
                entries.push(ArtifactEntry {
                    relative_path: join_posix(&rel, &name),
                    kind: cls.kind.to_string(),
                    size_bytes: size,
                    file_count: files,
                    last_modified_unix: lm,
                    safety: cls.safety,
                    reason: cls.reason,
                });
                continue;
            }
            if depth + 1 < MAX_WALK_DEPTH {
                queue.push_back((rel.join(&name), depth + 1));
            }
        }
    }
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let total_reclaimable_bytes = entries
        .iter()
        .filter(|e| e.safety == Safety::Safe)
        .map(|e| e.size_bytes)
        .sum();
    let total_reported_bytes = entries.iter().map(|e| e.size_bytes).sum();
    HygieneReport {
        scanned_at_unix: now_unix(),
        entries,
        total_reclaimable_bytes,
        total_reported_bytes,
        free_space_bytes: drive_free_space(root),
    }
}

/// Scan and merge the result into the persisted manifest in one step.
pub fn scan_and_record(root: &Path) -> HygieneReport {
    let report = scan(root);
    record_scan(root, &report);
    report
}

fn join_posix(rel: &Path, name: &str) -> String {
    let mut s = rel.to_string_lossy().replace('\\', "/");
    if !s.is_empty() {
        s.push('/');
    }
    s.push_str(name);
    s
}

// ─── Manifest ───────────────────────────────────────────────────────────────

/// Last-known state of one artifact tree, persisted between sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub kind: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub safety: Safety,
    pub last_modified_unix: Option<i64>,
    pub seen_at_unix: i64,
}

/// Which build command grew which tree — the disk cost sits next to the
/// action that caused it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrowthDelta {
    pub path: String,
    pub delta_bytes: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub at_unix: i64,
    pub command: String,
    pub growth_bytes: i64,
    pub details: Vec<GrowthDelta>,
}

/// `.velocity/disk_hygiene.json`: plain JSON, no secrets, same treatment as
/// `build_diagnostics.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HygieneManifest {
    pub last_scan_unix: Option<i64>,
    pub bytes_reclaimed_total: u64,
    pub entries: BTreeMap<String, ManifestEntry>,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRecord>,
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(".velocity").join("disk_hygiene.json")
}

/// Load the manifest; a missing or corrupt file reads as empty rather than
/// failing every caller.
pub fn load_manifest(root: &Path) -> HygieneManifest {
    fs::read_to_string(manifest_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_manifest(root: &Path, manifest: &HygieneManifest) -> std::io::Result<()> {
    let path = manifest_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(manifest).unwrap_or_default(),
    )
}

/// Replace the manifest's per-tree snapshot with this scan's results,
/// preserving reclaimed totals and provenance history.
pub fn record_scan(root: &Path, report: &HygieneReport) {
    let mut manifest = load_manifest(root);
    manifest.last_scan_unix = Some(report.scanned_at_unix);
    manifest.entries = report
        .entries
        .iter()
        .map(|e| {
            (
                e.relative_path.clone(),
                ManifestEntry {
                    kind: e.kind.clone(),
                    size_bytes: e.size_bytes,
                    file_count: e.file_count,
                    safety: e.safety,
                    last_modified_unix: e.last_modified_unix,
                    seen_at_unix: report.scanned_at_unix,
                },
            )
        })
        .collect();
    let _ = save_manifest(root, &manifest);
}

// ─── Cleaning ───────────────────────────────────────────────────────────────

/// Containment + sanity gate for one selected tree. Mirrors the
/// canonicalize-and-prefix discipline of `resolve_workspace_path` and adds
/// the symlink/junction refusal cleanup needs (we must not delete through a
/// link pointing outside the workspace).
fn contained_real_dir(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let given = Path::new(rel);
    if given.is_absolute() || matches!(given.components().next(), Some(Component::Prefix(_))) {
        return Err("absolute path refused; artifact paths are workspace-relative".into());
    }
    if given
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err("'..' in path refused".into());
    }
    let abs = root.join(given);
    let meta = fs::symlink_metadata(&abs).map_err(|e| format!("cannot stat: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err("symlink or junction refused; only real directories are cleaned".into());
    }
    if !meta.is_dir() {
        return Err("not a directory".into());
    }
    let root_canon = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve root: {e}"))?;
    let abs_canon = abs
        .canonicalize()
        .map_err(|e| format!("cannot resolve path: {e}"))?;
    if !abs_canon.starts_with(&root_canon) {
        return Err("resolves outside the workspace".into());
    }
    Ok(abs)
}

/// Delete the selected artifact trees (or all `Safe` ones when `None`).
/// Safe-only, always: nothing classified `Review`, unknown, or unresolvable
/// is ever touched — an override is deliberately not supported. Sizes are
/// re-measured at deletion time and classification is re-checked against the
/// live filesystem, so a stale report cannot smuggle a path through.
pub fn clean(root: &Path, selected: Option<&[String]>, dry_run: bool) -> CleanResult {
    let report = scan(root);
    let mut result = CleanResult {
        dry_run,
        ..Default::default()
    };

    let mut chosen: Vec<&ArtifactEntry> = Vec::new();
    match selected {
        None => chosen.extend(report.entries.iter().filter(|e| e.safety == Safety::Safe)),
        Some(paths) => {
            for p in paths {
                match report.entries.iter().find(|e| &e.relative_path == p) {
                    Some(e) if e.safety == Safety::Safe => chosen.push(e),
                    Some(e) => result.rejected.push(format!(
                        "'{p}' is classified review and cannot be cleaned: {}",
                        e.reason
                            .clone()
                            .unwrap_or_else(|| "not an auto-cleanable tree".into())
                    )),
                    None => result.rejected.push(format!(
                        "'{p}' is not a recognized build-artifact tree; refusing to delete"
                    )),
                }
            }
        }
    }

    let mut deleted_paths: Vec<String> = Vec::new();
    for entry in chosen {
        let rel = entry.relative_path.clone();
        let abs = match contained_real_dir(root, &rel) {
            Ok(p) => p,
            Err(msg) => {
                result.rejected.push(format!("'{rel}': {msg}"));
                continue;
            }
        };
        // Re-check the family against the live tree (the report may be older
        // than the last edit to the directory beside it).
        let parent = abs.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = abs
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match classify(&name, &parent) {
            Some(c) if c.safety == Safety::Safe => {}
            _ => {
                result
                    .rejected
                    .push(format!("'{rel}': no longer matches a safe artifact family"));
                continue;
            }
        }
        let (size, files, _) = measure_tree(&abs);
        if dry_run {
            result.freed_bytes = result.freed_bytes.saturating_add(size);
            result.file_count += files;
            result.removed.push(RemovedTree {
                path: rel,
                size_bytes: size,
                file_count: files,
            });
            continue;
        }
        match fs::remove_dir_all(&abs) {
            Ok(()) => {
                result.freed_bytes = result.freed_bytes.saturating_add(size);
                result.file_count += files;
                result.removed.push(RemovedTree {
                    path: rel.clone(),
                    size_bytes: size,
                    file_count: files,
                });
                deleted_paths.push(rel);
            }
            Err(e) => result
                .rejected
                .push(format!("'{rel}': deletion failed: {e}")),
        }
    }

    if !dry_run && result.freed_bytes > 0 {
        let mut manifest = load_manifest(root);
        manifest.bytes_reclaimed_total = manifest
            .bytes_reclaimed_total
            .saturating_add(result.freed_bytes);
        for p in &deleted_paths {
            manifest.entries.remove(p);
        }
        let _ = save_manifest(root, &manifest);
    }
    result
}

// ─── Provenance: which build grew what ──────────────────────────────────────

/// Classify a shell command as a known artifact-producing build. Returns the
/// family label used in provenance records, or None for commands that should
/// not trigger a growth scan.
pub fn classify_build_command(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();
    let toks: Vec<&str> = lower.split_whitespace().collect();
    let first = toks.first().copied().unwrap_or("");
    let has = |words: &[&str]| toks.iter().any(|t| words.contains(t));
    if matches!(first, "cargo" | "cargo.exe") && has(&["build", "check", "test", "clippy", "fix"]) {
        return Some("cargo");
    }
    if matches!(first, "npm" | "npx" | "pnpm" | "yarn" | "bun")
        && has(&["install", "i", "ci", "add", "build"])
    {
        return Some("js-package-manager");
    }
    if (matches!(first, "pip" | "pip3") && has(&["install"]))
        || (matches!(first, "python" | "python3") && lower.contains("setup.py"))
        || (first == "uv" && lower.contains("pip install"))
        || lower.contains("poetry install")
    {
        return Some("python");
    }
    if (first == "dotnet" && has(&["build", "restore", "publish", "test"]))
        || lower.contains("msbuild")
    {
        return Some("dotnet");
    }
    if first == "make" || (first == "cmake" && has(&["--build"])) || first == "ninja" {
        return Some("native-toolchain");
    }
    None
}

/// After a classified build command finished: quick scan, diff sizes against
/// the manifest, and persist the growth as provenance plus one event so the
/// Decision Trail shows the disk cost next to the action that caused it.
/// Returns a human summary when anything actually grew.
pub fn note_build_growth(root: &Path, command: &str) -> Option<String> {
    let family = classify_build_command(command)?;
    let report = scan(root);
    let before = load_manifest(root);
    let mut details: Vec<GrowthDelta> = Vec::new();
    let mut growth: i64 = 0;
    for e in &report.entries {
        let prev = before
            .entries
            .get(&e.relative_path)
            .map(|m| m.size_bytes as i64)
            .unwrap_or(0);
        let delta = e.size_bytes as i64 - prev;
        if delta > GROWTH_DETAIL_FLOOR_BYTES {
            details.push(GrowthDelta {
                path: e.relative_path.clone(),
                delta_bytes: delta,
            });
            growth += delta;
        }
    }
    // The snapshot is worth keeping even when nothing grew.
    record_scan(root, &report);
    if growth <= 0 {
        return None;
    }
    let mut manifest = load_manifest(root);
    manifest.provenance.push(ProvenanceRecord {
        at_unix: now_unix(),
        command: clip(command, 140),
        growth_bytes: growth,
        details: details.clone(),
    });
    if manifest.provenance.len() > MANIFEST_HISTORY_LIMIT {
        let excess = manifest.provenance.len() - MANIFEST_HISTORY_LIMIT;
        manifest.provenance.drain(..excess);
    }
    let _ = save_manifest(root, &manifest);

    let top: Vec<String> = details
        .iter()
        .take(3)
        .map(|d| format!("{} +{}", d.path, format_bytes(d.delta_bytes as u64)))
        .collect();
    let summary = format!(
        "Artifacts grew: {} (+{} total) after `{family}` command",
        top.join(", "),
        format_bytes(growth as u64)
    );
    let store = crate::registry::event_store::EventStore::open(root);
    let affected: Vec<String> = details.iter().map(|d| d.path.clone()).collect();
    if let Ok(seq) = store.record("disk_hygiene", &summary, None, None, None, affected) {
        let _ = store.mark_outcome(
            seq,
            crate::registry::event_store::EventOutcome::Success,
            None,
        );
    }
    let pressure = check_disk_pressure(root)
        .map(|p| format!(" {p}"))
        .unwrap_or_default();
    Some(format!("{summary}{pressure}"))
}

// ─── Low-disk sentinel ──────────────────────────────────────────────────────

/// Pure nag logic, split out so it is testable without filling a real drive.
pub fn pressure_message(
    free_bytes: u64,
    reclaimable_bytes: u64,
    largest_tree_bytes: u64,
) -> Option<String> {
    let low = free_bytes < DISK_PRESSURE_FLOOR_BYTES;
    let big = largest_tree_bytes > BIG_TREE_NAG_BYTES;
    if !low && !big {
        return None;
    }
    if reclaimable_bytes > 0 {
        Some(format!(
            "Disk: {} reclaimable — Ctrl+Shift+K to clean",
            format_bytes(reclaimable_bytes)
        ))
    } else {
        Some(format!(
            "Disk: only {} free — run 'Clean Build Artifacts…' to scan",
            format_bytes(free_bytes)
        ))
    }
}

/// Cheap pressure check: drive free space plus the *cached* manifest sizes,
/// no filesystem walk. Called at app startup and after classified builds.
pub fn check_disk_pressure(root: &Path) -> Option<String> {
    let manifest = load_manifest(root);
    let reclaimable = manifest
        .entries
        .values()
        .filter(|e| e.safety == Safety::Safe)
        .map(|e| e.size_bytes)
        .sum();
    let largest = manifest
        .entries
        .values()
        .map(|e| e.size_bytes)
        .max()
        .unwrap_or(0);
    pressure_message(drive_free_space(root), reclaimable, largest)
}

// ─── Formatting / platform helpers ──────────────────────────────────────────

/// Human binary size: "24.1 GiB", "912.3 MiB", "44.0 KiB", "512 B".
pub fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1 << 30;
    const MIB: u64 = 1 << 20;
    const KIB: u64 = 1 << 10;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Digit-grouped count for status lines: "9,881 files". Rust's format
/// strings have no `,` grouping (that's Python), so the separators are
/// inserted by hand.
pub fn format_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn clip(s: &str, max_chars: usize) -> String {
    let mut flat: String = s.chars().take(max_chars).collect();
    if flat.chars().count() < s.chars().count() {
        flat.push('…');
    }
    flat
}

/// Free bytes on the drive containing `path`. `std::fs::available_space` is
/// still unstable, so this goes straight to the platform call. Read-only.
pub fn drive_free_space(path: &Path) -> u64 {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free = 0u64;
        // SAFETY: `wide` is a valid null-terminated UTF-16 buffer and
        // `free` is a live u64 the API is allowed to write.
        unsafe {
            GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None)
                .map(|_| free)
                .unwrap_or(0)
        }
    }
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        let cpath = match std::ffi::CString::new(path.as_os_str().to_string_lossy().as_bytes()) {
            Ok(c) => c,
            Err(_) => return 0,
        };
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `cpath` outlives the call and `stat` is a live struct.
        if unsafe { libc::statvfs(cpath.as_ptr(), &mut stat) } == 0 {
            (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64)
        } else {
            0
        }
    }
    #[cfg(not(any(windows, all(unix, not(target_arch = "wasm32")))))]
    {
        let _ = path;
        0
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        (temp, root)
    }

    fn write_file(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![b'x'; len]).unwrap();
    }

    #[test]
    fn target_requires_a_cargo_marker_to_be_safe() {
        let (tmp, root) = fixture();
        write_file(&root.join("app/target/debug/build.o"), 2048);
        let review = scan(&root);
        let e = review
            .entries
            .iter()
            .find(|e| e.relative_path == "app/target")
            .expect("target dir should be matched");
        assert_eq!(e.safety, Safety::Review);
        assert!(e
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("Cargo.toml")));
        assert_eq!(
            review.total_reclaimable_bytes, 0,
            "review rows are not reclaimable"
        );

        fs::write(root.join("app/Cargo.toml"), "[package]\n").unwrap();
        let with = scan(&root);
        let e = with
            .entries
            .iter()
            .find(|e| e.relative_path == "app/target")
            .unwrap();
        assert_eq!(e.safety, Safety::Safe);
        assert_eq!(e.size_bytes, 2048);
        assert_eq!(e.file_count, 1);
        assert_eq!(with.total_reclaimable_bytes, 2048);
        drop(tmp);
    }

    #[test]
    fn unconditional_families_and_gates() {
        let (_tmp, root) = fixture();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join("src/__pycache__")).unwrap();
        fs::create_dir_all(root.join("mypkg.egg-info")).unwrap();
        fs::create_dir_all(root.join(".wrangler/cache")).unwrap();
        fs::create_dir_all(root.join("web/build")).unwrap();
        fs::create_dir_all(root.join("dotnet/obj")).unwrap();
        let report = scan(&root);
        let find = |p: &str| report.entries.iter().find(|e| e.relative_path == p);
        assert_eq!(find("node_modules").unwrap().safety, Safety::Safe);
        assert_eq!(find("src/__pycache__").unwrap().safety, Safety::Safe);
        assert_eq!(find("mypkg.egg-info").unwrap().safety, Safety::Safe);
        assert_eq!(find(".wrangler/cache").unwrap().safety, Safety::Safe);
        // `build` without package.json and `obj` without a project file stay
        // report-only.
        assert_eq!(find("web/build").unwrap().safety, Safety::Review);
        assert_eq!(find("dotnet/obj").unwrap().safety, Safety::Review);
        fs::write(root.join("dotnet/App.csproj"), "").unwrap();
        assert_eq!(
            classify("obj", &root.join("dotnet")).unwrap().safety,
            Safety::Safe
        );
    }

    #[test]
    fn scan_never_enters_protected_dirs() {
        let (_tmp, root) = fixture();
        for shielded in [
            ".velocity/target",
            ".velocity-cache/node_modules",
            "memory/build",
            ".git/node_modules",
        ] {
            fs::create_dir_all(root.join(shielded)).unwrap();
            fs::write(root.join(format!("{shielded}/payload.bin")), "x").unwrap();
        }
        fs::write(root.join("Cargo.toml"), "").unwrap();
        fs::write(root.join("package.json"), "").unwrap();
        let report = scan(&root);
        assert!(
            report.entries.is_empty(),
            "protected dirs leaked matches: {:?}",
            report.entries
        );
    }

    #[test]
    fn scan_respects_depth_cap() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        // depth cap 6: parents are read at depths 0..=5, so a tree whose
        // parent sits at depth 5 is still found, one deeper is not.
        let shallows = "a/b/c/d/e/target";
        let deep = "a1/b/c/d/e/f/target";
        for rel in [shallows, deep] {
            fs::create_dir_all(root.join(rel)).unwrap();
            fs::write(root.join(format!("{rel}/lib.o")), "x").unwrap();
        }
        fs::write(root.join("a/b/c/d/e/Cargo.toml"), "").unwrap();
        fs::write(root.join("a1/b/c/d/e/f/Cargo.toml"), "").unwrap();
        let report = scan(&root);
        let paths: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.relative_path.as_str())
            .collect();
        assert!(paths.contains(&"a/b/c/d/e/target"), "{paths:?}");
        assert!(!paths.contains(&"a1/b/c/d/e/f/target"), "{paths:?}");
    }

    #[test]
    fn clean_dry_run_reports_without_touching_files() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        write_file(&root.join("target/debug/big.o"), 4096);
        let result = clean(&root, None, true);
        assert!(result.dry_run);
        assert_eq!(result.freed_bytes, 4096);
        assert_eq!(result.removed.len(), 1);
        assert!(root.join("target").exists(), "dry run deleted something");
        assert!(result.rejected.is_empty(), "{:?}", result.rejected);
    }

    #[test]
    fn clean_removes_safe_trees_and_refuses_everything_else() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        write_file(&root.join("target/debug/big.o"), 2048);
        fs::create_dir_all(root.join("docs/build")).unwrap();
        write_file(&root.join("docs/build/handwritten.md"), 10);
        let paths = vec![
            "target".to_string(),
            "docs/build".to_string(),
            "../outside".to_string(),
            "never/scanned".to_string(),
        ];
        let result = clean(&root, Some(&paths), false);
        assert!(!root.join("target").exists(), "safe tree was not removed");
        assert!(root.join("docs/build").exists(), "review tree was deleted");
        assert!(root.join("Cargo.toml").exists());
        assert_eq!(result.freed_bytes, 2048);
        assert_eq!(result.rejected.len(), 3, "{:?}", result.rejected);
        assert!(result
            .rejected
            .iter()
            .any(|r| r.contains("docs/build") && r.contains("review")));
        assert!(result.rejected.iter().any(|r| r.contains("../outside")));
        assert!(result.rejected.iter().any(|r| r.contains("never/scanned")));
    }

    #[test]
    fn clean_updates_manifest_totals() {
        let (_tmp, root) = fixture();
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/Cargo.toml"), "").unwrap();
        write_file(&root.join("app/target/debug/x.o"), 5000);
        let report = scan_and_record(&root);
        let mut manifest = load_manifest(&root);
        manifest.bytes_reclaimed_total = 100;
        save_manifest(&root, &manifest).unwrap();

        let result = clean(&root, None, false);
        assert_eq!(result.freed_bytes, 5000);
        let manifest = load_manifest(&root);
        assert_eq!(manifest.bytes_reclaimed_total, 5100);
        assert!(
            !manifest.entries.contains_key("app/target"),
            "removed tree still tracked: {:?}",
            manifest.entries.keys()
        );
        assert_eq!(report.total_reclaimable_bytes, 5000);
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let (_tmp, root) = fixture();
        let mut manifest = HygieneManifest::default();
        manifest.last_scan_unix = Some(123);
        manifest.entries.insert(
            "target".into(),
            ManifestEntry {
                kind: "cargo-target".into(),
                size_bytes: 42,
                file_count: 1,
                safety: Safety::Safe,
                last_modified_unix: None,
                seen_at_unix: 123,
            },
        );
        manifest.provenance.push(ProvenanceRecord {
            at_unix: 124,
            command: "cargo build".into(),
            growth_bytes: 42,
            details: vec![GrowthDelta {
                path: "target".into(),
                delta_bytes: 42,
            }],
        });
        save_manifest(&root, &manifest).unwrap();
        let loaded = load_manifest(&root);
        assert_eq!(loaded.last_scan_unix, Some(123));
        assert_eq!(loaded.entries["target"].size_bytes, 42);
        assert_eq!(loaded.provenance.len(), 1);
        // Corrupt file degrades to empty instead of failing callers.
        fs::write(manifest_path(&root), "not json").unwrap();
        assert!(load_manifest(&root).entries.is_empty());
    }

    #[test]
    fn build_command_classifier() {
        assert_eq!(
            classify_build_command("cargo build --release"),
            Some("cargo")
        );
        assert_eq!(classify_build_command("cargo check"), Some("cargo"));
        assert_eq!(
            classify_build_command("npm install left-pad"),
            Some("js-package-manager")
        );
        assert_eq!(
            classify_build_command("npm run build"),
            Some("js-package-manager")
        );
        assert_eq!(
            classify_build_command("pip install -r x.txt"),
            Some("python")
        );
        assert_eq!(classify_build_command("dotnet build App"), Some("dotnet"));
        assert_eq!(classify_build_command("make -j8"), Some("native-toolchain"));
        assert_eq!(
            classify_build_command("cmake --build ."),
            Some("native-toolchain")
        );
        assert_eq!(classify_build_command("git status"), None);
        assert_eq!(classify_build_command("cargo fmt"), None);
        assert_eq!(classify_build_command("echo build stuff"), None);
    }

    #[test]
    fn note_build_growth_records_provenance_once() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        write_file(&root.join("target/debug/blob.o"), 200_000);

        let msg = note_build_growth(&root, "cargo build")
            .expect("first build against an empty manifest must report growth");
        assert!(msg.starts_with("Artifacts grew:"), "{msg}");
        assert!(msg.contains("target"));
        let manifest = load_manifest(&root);
        assert_eq!(manifest.provenance.len(), 1);
        assert_eq!(manifest.provenance[0].command, "cargo build");
        assert!(manifest.provenance[0].growth_bytes >= 200_000);
        // The event made it into the shared log the Decision Trail reads.
        let events = fs::read_to_string(root.join(".velocity/events/events.jsonl"))
            .expect("growth event missing");
        assert!(events.contains("Artifacts grew"));

        // Nothing changed: the second call records no new provenance.
        assert!(note_build_growth(&root, "cargo build").is_none());
        assert_eq!(load_manifest(&root).provenance.len(), 1);
    }

    #[test]
    fn non_build_commands_are_ignored_by_the_hook() {
        let (_tmp, root) = fixture();
        write_file(&root.join("target/whatever.o"), 1_000_000);
        assert!(note_build_growth(&root, "git push").is_none());
        assert!(load_manifest(&root).provenance.is_empty());
    }

    #[test]
    fn pressure_message_thresholds() {
        let gib = 1u64 << 30;
        // Comfortable drive, small trees: no nag.
        assert!(pressure_message(50 * gib, 4 * gib, 2 * gib).is_none());
        // Below the floor: nag with the reclaimable figure.
        let nag = pressure_message(4 * gib, 12 * gib, gib).unwrap();
        assert!(nag.contains("12.0 GiB reclaimable"), "{nag}");
        assert!(nag.contains("Ctrl+Shift+K"), "{nag}");
        // One enormous tree nags even on a healthy drive.
        assert!(pressure_message(50 * gib, 9 * gib, 9 * gib).is_some());
        // Low space but nothing tracked yet: points at the scan instead.
        let scan_hint = pressure_message(2 * gib, 0, 0).unwrap();
        assert!(scan_hint.contains("Clean Build Artifacts"), "{scan_hint}");
    }

    #[test]
    fn format_bytes_scales() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(3 * (1 << 20)), "3.0 MiB");
        assert_eq!(format_bytes((1u64 << 30) + (1u64 << 29)), "1.5 GiB");
    }

    #[test]
    fn format_count_groups_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1000), "1,000");
        assert_eq!(format_count(9881), "9,881");
        assert_eq!(format_count(1_000_000), "1,000,000");
    }

    #[test]
    fn drive_free_space_reports_a_live_number() {
        let (_tmp, root) = fixture();
        assert!(
            drive_free_space(&root) > 0,
            "free-space probe returned zero on a real drive"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_trees_are_never_matched_or_deleted() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let outside = _tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.txt"), "keep me").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("target")).unwrap();
        let report = scan(&root);
        assert!(
            report.entries.is_empty(),
            "symlink classified as an artifact: {:?}",
            report.entries
        );
        // Even a forced selection cannot delete through the link.
        let result = clean(&root, Some(&["target".to_string()]), false);
        assert!(outside.join("precious.txt").exists());
        assert_eq!(result.freed_bytes, 0);
        assert!(!result.rejected.is_empty());
    }

    /// Windows has its own link flavor: a junction needs no privilege to
    /// create and std reports it as a symlink, but the contract must hold
    /// there too — a `target` that is really a doorway outside the workspace
    /// is neither scanned nor deleted.
    #[cfg(windows)]
    #[test]
    fn junctions_are_never_matched_or_deleted() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let outside = _tmp.path().join("outside");
        fs::create_dir_all(outside.join("target")).unwrap();
        fs::write(outside.join("target/precious.o"), "keep me").unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("target"))
            .arg(outside.join("target"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !made {
            // Boxes that refuse junction creation are covered by the unix
            // symlink test above; the refusal path is the same code either way.
            return;
        }
        let report = scan(&root);
        assert!(
            report.entries.iter().all(|e| e.relative_path != "target"),
            "junction matched as an artifact: {:?}",
            report.entries
        );
        let result = clean(&root, Some(&["target".to_string()]), false);
        assert!(outside.join("target/precious.o").exists());
        assert_eq!(result.freed_bytes, 0);
        assert!(!result.rejected.is_empty());
    }

    #[test]
    fn clean_rechecks_the_context_marker_at_deletion_time() {
        // The report said Safe; then the Cargo.toml beside it vanished. The
        // live re-classification inside `clean` must catch that and refuse.
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        write_file(&root.join("target/debug/x.o"), 1024);
        assert_eq!(scan(&root).entries[0].safety, Safety::Safe);
        fs::remove_file(root.join("Cargo.toml")).unwrap();
        let result = clean(&root, Some(&["target".to_string()]), false);
        assert!(
            root.join("target").exists(),
            "deleted after its marker vanished"
        );
        assert_eq!(result.freed_bytes, 0);
        assert!(
            result
                .rejected
                .iter()
                .any(|r| r.contains("classified review")),
            "{:?}",
            result.rejected
        );
    }

    #[test]
    fn contained_real_dir_refuses_hostile_paths() {
        let (_tmp, root) = fixture();
        let abs = root.join("target").to_string_lossy().into_owned();
        let err = contained_real_dir(&root, &abs).unwrap_err();
        assert!(err.contains("absolute"), "{err}");
        assert!(contained_real_dir(&root, "../outside").is_err());
        assert!(contained_real_dir(&root, "target/../../outside").is_err());
        assert!(contained_real_dir(&root, "missing/dir").is_err());
        fs::create_dir_all(root.join("real")).unwrap();
        assert!(contained_real_dir(&root, "real").is_ok());
    }

    #[test]
    fn a_file_named_target_is_not_an_artifact() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        fs::write(root.join("target"), "I am a file, not a build directory").unwrap();
        let report = scan(&root);
        assert!(report.entries.is_empty(), "{:?}", report.entries);
    }

    #[test]
    fn matched_trees_are_measured_not_walked_for_more_matches() {
        // `node_modules` swallows everything below it: a `build/` inside a
        // dependency must not surface as its own (deletable) row.
        let (_tmp, root) = fixture();
        write_file(&root.join("node_modules/pkg/package.json"), 2);
        write_file(&root.join("node_modules/pkg/build/bundle.js"), 100);
        let report = scan(&root);
        assert_eq!(report.entries.len(), 1, "{:?}", report.entries);
        let e = &report.entries[0];
        assert_eq!(e.relative_path, "node_modules");
        assert_eq!(e.size_bytes, 102);
        assert_eq!(e.file_count, 2);
    }

    #[test]
    fn matching_is_case_sensitive_except_dotnet_markers() {
        // Conservative on purpose: tools write `target`, `node_modules`,
        // `dist` in exact case, so a hand-made `Target` folder is content,
        // not build output. The .NET marker check ignores case because
        // `MyApp.SLN` is ordinary on Windows.
        let (_tmp, root) = fixture();
        fs::create_dir_all(root.join("Target")).unwrap();
        fs::create_dir_all(root.join("NODE_MODULES")).unwrap();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let report = scan(&root);
        assert!(report.entries.is_empty(), "{:?}", report.entries);
        fs::create_dir_all(root.join("proj/obj")).unwrap();
        fs::write(root.join("proj/MyApp.SLN"), "").unwrap();
        assert_eq!(
            classify("obj", &root.join("proj")).unwrap().safety,
            Safety::Safe
        );
    }

    #[test]
    fn provenance_history_is_capped() {
        let (_tmp, root) = fixture();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        write_file(&root.join("target/seed.o"), 128 * 1024);
        for round in 0..(MANIFEST_HISTORY_LIMIT + 5) {
            write_file(&root.join(format!("target/f{round}.o")), 128 * 1024);
            assert!(
                note_build_growth(&root, "cargo build").is_some(),
                "round {round}"
            );
        }
        let manifest = load_manifest(&root);
        assert_eq!(manifest.provenance.len(), MANIFEST_HISTORY_LIMIT);
        // The oldest records were trimmed, newest kept.
        assert!(manifest.provenance.last().unwrap().growth_bytes > 0);
    }

    #[test]
    fn scan_of_a_missing_root_is_an_empty_report() {
        let report = scan(Path::new("Z:/definitely/not/a/real/tree"));
        assert!(report.entries.is_empty());
        assert_eq!(report.total_reclaimable_bytes, 0);
    }

    #[test]
    fn classifier_edges_hold_up() {
        assert_eq!(classify_build_command("CARGO BUILD"), Some("cargo"));
        assert_eq!(classify_build_command("cargo.exe build"), Some("cargo"));
        assert_eq!(
            classify_build_command("pnpm install"),
            Some("js-package-manager")
        );
        assert_eq!(
            classify_build_command("yarn add left-pad"),
            Some("js-package-manager")
        );
        assert_eq!(classify_build_command("poetry install"), Some("python"));
        assert_eq!(
            classify_build_command("ninja -C out"),
            Some("native-toolchain")
        );
        assert_eq!(classify_build_command("docker build ."), None);
        assert_eq!(classify_build_command("cargo"), None);
        assert_eq!(classify_build_command(""), None);
    }

    #[test]
    fn venv_and_wrangler_trees_clean_like_cargo_ones() {
        let (_tmp, root) = fixture();
        fs::write(root.join("requirements.txt"), "").unwrap();
        write_file(&root.join(".venv/lib/site.py"), 512);
        write_file(&root.join(".wrangler/cache/blob"), 256);
        let report = scan(&root);
        assert_eq!(report.total_reclaimable_bytes, 768, "{:?}", report.entries);
        let result = clean(&root, None, false);
        assert_eq!(result.freed_bytes, 768);
        assert!(!root.join(".venv").exists());
        assert!(!root.join(".wrangler/cache").exists());
        assert!(
            root.join(".wrangler").exists(),
            "only the cache tree is removed"
        );
    }

    #[test]
    fn format_bytes_boundaries() {
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1 << 10), "1.0 KiB");
        assert_eq!(format_bytes((1 << 30) - 1), "1024.0 MiB");
    }
}
