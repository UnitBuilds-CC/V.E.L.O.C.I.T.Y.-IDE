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

pub fn read_latest_diagnostics(workspace_root: &std::path::Path) -> BuildDiagnostics {
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

    let output = match Command::new("cargo")
        .arg("check")
        .arg("--workspace")
        .current_dir(workspace_root)
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
    std::fs::write(diagnostics_path(workspace_root), serde_json::to_string_pretty(diag)?)
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
