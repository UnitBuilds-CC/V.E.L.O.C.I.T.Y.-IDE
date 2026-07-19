use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BuildDiagnostics {
    pub timestamp_ms: u64,
    pub success: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub summary: String,
}

pub fn diagnostics_path(workspace_root: &std::path::Path) -> PathBuf {
    workspace_root.join(".velocity").join("build_diagnostics.json")
}

pub fn diagnostics_nda_path(workspace_root: &std::path::Path) -> PathBuf {
    workspace_root.join(".velocity").join("build_diagnostics.nda")
}

pub fn read_latest_diagnostics(workspace_root: &std::path::Path) -> BuildDiagnostics {
    let nda_path = diagnostics_nda_path(workspace_root);
    if let Ok(raw) = std::fs::read_to_string(&nda_path) {
        if let Some(d) = parse_diagnostics_nda(&raw) {
            return d;
        }
    }
    let path = diagnostics_path(workspace_root);
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(d) = serde_json::from_slice::<BuildDiagnostics>(&bytes) {
            return d;
        }
    }
    BuildDiagnostics {
        summary: "No diagnostics available".into(),
        ..Default::default()
    }
}

static WATCHER_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn spawn_build_watcher(workspace_root: PathBuf, interval_secs: u64) {
    if WATCHER_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }

    if let Some(parent) = diagnostics_path(&workspace_root).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    thread::spawn(move || loop {
        let diag = run_cargo_check(&workspace_root);
        let _ = write_diagnostics(&workspace_root, &diag);
        thread::sleep(Duration::from_secs(interval_secs));
    });
}

pub fn run_cargo_check(workspace_root: &std::path::Path) -> BuildDiagnostics {
    let mut diag = BuildDiagnostics {
        timestamp_ms: now_ms(),
        ..Default::default()
    };

    let cargo_dir = if workspace_root.join("Cargo.toml").exists() {
        workspace_root.to_path_buf()
    } else if workspace_root.join("velocity-mcp").join("Cargo.toml").exists() {
        workspace_root.join("velocity-mcp")
    } else {
        let mut found = workspace_root.to_path_buf();
        if let Ok(entries) = std::fs::read_dir(workspace_root) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let path = entry.path();
                    if path.join("Cargo.toml").exists() {
                        found = path;
                        break;
                    }
                }
            }
        }
        found
    };

    let output = match Command::new("cargo")
        .arg("check")
        .arg("--workspace")
        .current_dir(&cargo_dir)
        .env("RUSTFLAGS", "--allow=warnings")
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            diag.success = false;
            diag.summary = format!("cargo check failed to run: {}", e);
            diag.errors.push(diag.summary.clone());
            return diag;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    for line in combined.lines() {
        if line.starts_with("error[") || line.starts_with("error:") {
            diag.errors.push(line.to_string());
        } else if line.starts_with("warning:") || line.starts_with("warning[") {
            diag.warnings.push(line.to_string());
        }
    }

    diag.success = output.status.success() && diag.errors.is_empty();
    diag.summary = if diag.success {
        format!("cargo check OK ({} warnings)", diag.warnings.len())
    } else {
        format!("cargo check FAILED ({} errors, {} warnings)", diag.errors.len(), diag.warnings.len())
    };
    diag
}

pub fn write_diagnostics(workspace_root: &std::path::Path, diag: &BuildDiagnostics) -> std::io::Result<()> {
    if let Some(parent) = diagnostics_nda_path(workspace_root).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(diagnostics_nda_path(workspace_root), serialize_diagnostics_nda(diag))?;
    std::fs::write(diagnostics_path(workspace_root), serde_json::to_string_pretty(diag)?)
}

fn serialize_diagnostics_nda(diag: &BuildDiagnostics) -> String {
    let mut lines = vec![
        "build-diagnostics version 1".to_string(),
        format!("timestamp_ms {}", diag.timestamp_ms),
        format!("success {}", diag.success),
        format!("summary {}", encode_nda_text(&diag.summary)),
    ];
    for error in &diag.errors {
        lines.push(format!("error {}", encode_nda_text(error)));
    }
    for warning in &diag.warnings {
        lines.push(format!("warning {}", encode_nda_text(warning)));
    }
    lines.join("\n") + "\n"
}

fn parse_diagnostics_nda(raw: &str) -> Option<BuildDiagnostics> {
    let mut diag = BuildDiagnostics::default();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line == "build-diagnostics version 1" {
            continue;
        }
        let (key, value) = line.split_once(' ')?;
        match key {
            "timestamp_ms" => diag.timestamp_ms = value.parse().ok()?,
            "success" => diag.success = value.parse().ok()?,
            "summary" => diag.summary = decode_nda_text(value),
            "error" => diag.errors.push(decode_nda_text(value)),
            "warning" => diag.warnings.push(decode_nda_text(value)),
            _ => {}
        }
    }
    if diag.summary.is_empty() {
        None
    } else {
        Some(diag)
    }
}

fn encode_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn decode_nda_text(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run_self_check() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let diag = run_cargo_check(&workspace_root);
    let _ = write_diagnostics(&workspace_root, &diag);
    if diag.success {
        println!("{}", diag.summary);
    } else {
        eprintln!("{}", diag.summary);
        for e in &diag.errors {
            eprintln!("{}", e);
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nda_build_diagnostics() {
        let tmp = tempfile::tempdir().unwrap();
        let diag = BuildDiagnostics {
            timestamp_ms: 123,
            success: false,
            errors: vec!["error: failure".to_string()],
            warnings: vec!["warning: caution".to_string()],
            summary: "cargo check FAILED (1 errors, 1 warnings)".to_string(),
        };

        write_diagnostics(tmp.path(), &diag).unwrap();
        let nda = std::fs::read_to_string(diagnostics_nda_path(tmp.path())).unwrap();
        let json = std::fs::read_to_string(diagnostics_path(tmp.path())).unwrap();

        assert!(nda.starts_with("build-diagnostics version 1\n"));
        assert!(nda.contains("summary cargo check FAILED (1 errors, 1 warnings)"));
        assert!(json.contains("\"summary\": \"cargo check FAILED (1 errors, 1 warnings)\""));
    }

    #[test]
    fn reads_nda_build_diagnostics_before_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
        std::fs::write(
            diagnostics_nda_path(tmp.path()),
            "build-diagnostics version 1\ntimestamp_ms 7\nsuccess true\nsummary nda summary\nwarning nda warning\n",
        )
        .unwrap();
        std::fs::write(
            diagnostics_path(tmp.path()),
            "{\"timestamp_ms\":999,\"success\":false,\"errors\":[],\"warnings\":[],\"summary\":\"json summary\"}",
        )
        .unwrap();

        let diag = read_latest_diagnostics(tmp.path());
        assert_eq!(diag.timestamp_ms, 7);
        assert!(diag.success);
        assert_eq!(diag.summary, "nda summary");
        assert_eq!(diag.warnings, vec!["nda warning".to_string()]);
    }
}
