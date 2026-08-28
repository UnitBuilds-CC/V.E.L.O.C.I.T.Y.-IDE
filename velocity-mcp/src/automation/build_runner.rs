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
    workspace_root
        .join(".velocity")
        .join("build_diagnostics.json")
}

pub fn diagnostics_nda_path(workspace_root: &std::path::Path) -> PathBuf {
    workspace_root
        .join(".velocity")
        .join("build_diagnostics.nda")
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
    if WATCHER_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
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
    } else if workspace_root
        .join("velocity-mcp")
        .join("Cargo.toml")
        .exists()
    {
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
        format!(
            "cargo check FAILED ({} errors, {} warnings)",
            diag.errors.len(),
            diag.warnings.len()
        )
    };
    diag
}

pub fn write_diagnostics(
    workspace_root: &std::path::Path,
    diag: &BuildDiagnostics,
) -> std::io::Result<()> {
    if let Some(parent) = diagnostics_nda_path(workspace_root).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        diagnostics_nda_path(workspace_root),
        serialize_diagnostics_nda(diag),
    )?;
    std::fs::write(
        diagnostics_path(workspace_root),
        serde_json::to_string_pretty(diag)?,
    )
}

fn serialize_diagnostics_nda(diag: &BuildDiagnostics) -> String {
    let mut lines = vec![
        "build-diagnostics version 2".to_string(),
        format!("timestamp_ms {}", diag.timestamp_ms),
        format!("success {}", diag.success),
        format!("summary {}", encode_nda_text(&diag.summary)),
        format!("error_count {}", diag.errors.len()),
        format!("warning_count {}", diag.warnings.len()),
    ];
    for (idx, error) in diag.errors.iter().enumerate() {
        lines.push(format!("issue\terror\t{}\t{}", idx, encode_nda_text(error)));
    }
    for (idx, warning) in diag.warnings.iter().enumerate() {
        lines.push(format!(
            "issue\twarning\t{}\t{}",
            idx,
            encode_nda_text(warning)
        ));
    }
    lines.join("\n") + "\n"
}

fn parse_diagnostics_nda(raw: &str) -> Option<BuildDiagnostics> {
    let mut diag = BuildDiagnostics::default();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty()
            || line == "build-diagnostics version 1"
            || line == "build-diagnostics version 2"
        {
            continue;
        }
        if let Some(rest) = line.strip_prefix("issue\t") {
            let parts: Vec<&str> = rest.split('\t').collect();
            if parts.len() != 3 {
                return None;
            }
            let kind = parts[0];
            let _index: usize = parts[1].parse().ok()?;
            let message = decode_nda_text(parts[2]);
            match kind {
                "error" => diag.errors.push(message),
                "warning" => diag.warnings.push(message),
                _ => {}
            }
            continue;
        }
        let (key, value) = line.split_once(' ')?;
        match key {
            "timestamp_ms" => diag.timestamp_ms = value.parse().ok()?,
            "success" => diag.success = value.parse().ok()?,
            "summary" => diag.summary = decode_nda_text(value),
            "error" => diag.errors.push(decode_nda_text(value)),
            "warning" => diag.warnings.push(decode_nda_text(value)),
            "error_count" | "warning_count" => {}
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
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn decode_nda_text(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
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

        assert!(nda.starts_with("build-diagnostics version 2\n"));
        assert!(nda.contains("summary cargo check FAILED (1 errors, 1 warnings)"));
        assert!(nda.contains("error_count 1"));
        assert!(nda.contains("warning_count 1"));
        assert!(nda.contains("issue\terror\t0\terror: failure"));
        assert!(nda.contains("issue\twarning\t0\twarning: caution"));
        assert!(json.contains("\"summary\": \"cargo check FAILED (1 errors, 1 warnings)\""));
    }

    #[test]
    fn reads_nda_build_diagnostics_before_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
        std::fs::write(
            diagnostics_nda_path(tmp.path()),
            "build-diagnostics version 2\ntimestamp_ms 7\nsuccess true\nsummary nda summary\nerror_count 1\nwarning_count 1\nissue\terror\t0\tfirst\\tproblem\nissue\twarning\t0\tnda warning\n",
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
        assert_eq!(diag.errors, vec!["first\tproblem".to_string()]);
        assert_eq!(diag.warnings, vec!["nda warning".to_string()]);
    }

    #[test]
    fn reads_legacy_v1_build_diagnostics_nda() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
        std::fs::write(
            diagnostics_nda_path(tmp.path()),
            "build-diagnostics version 1\ntimestamp_ms 7\nsuccess false\nsummary legacy summary\nerror legacy error\nwarning legacy warning\n",
        )
        .unwrap();

        let diag = read_latest_diagnostics(tmp.path());
        assert_eq!(diag.timestamp_ms, 7);
        assert!(!diag.success);
        assert_eq!(diag.summary, "legacy summary");
        assert_eq!(diag.errors, vec!["legacy error".to_string()]);
        assert_eq!(diag.warnings, vec!["legacy warning".to_string()]);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let original = "hello\tworld\nnew line\r\\backslash";
        let encoded = encode_nda_text(original);
        let decoded = decode_nda_text(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn encode_escapes_special_chars() {
        assert_eq!(encode_nda_text("a\tb"), "a\\tb");
        assert_eq!(encode_nda_text("a\nb"), "a\\nb");
        assert_eq!(encode_nda_text("a\\b"), "a\\\\b");
    }

    #[test]
    fn decode_handles_unknown_escape() {
        // Unknown escape sequences should preserve the character
        assert_eq!(decode_nda_text("a\\xb"), "axb");
        assert_eq!(decode_nda_text("\\"), "\\"); // trailing backslash
    }

    #[test]
    fn parse_nda_empty_returns_none() {
        assert!(parse_diagnostics_nda("").is_none());
    }

    #[test]
    fn parse_nda_missing_summary_returns_none() {
        let raw = "build-diagnostics version 2\ntimestamp_ms 100\nsuccess true\n";
        assert!(parse_diagnostics_nda(raw).is_none()); // no summary
    }

    #[test]
    fn parse_nda_v2_with_issues() {
        let raw = "build-diagnostics version 2\ntimestamp_ms 50\nsuccess false\nsummary test summary\nerror_count 1\nwarning_count 1\nissue\terror\t0\tfirst error\nissue\twarning\t0\tfirst warning\n";
        let diag = parse_diagnostics_nda(raw).unwrap();
        assert_eq!(diag.timestamp_ms, 50);
        assert!(!diag.success);
        assert_eq!(diag.summary, "test summary");
        assert_eq!(diag.errors, vec!["first error".to_string()]);
        assert_eq!(diag.warnings, vec!["first warning".to_string()]);
    }

    #[test]
    fn diagnostics_paths_are_correct() {
        let root = std::path::Path::new("/tmp/test");
        assert_eq!(
            diagnostics_path(root),
            std::path::PathBuf::from("/tmp/test/.velocity/build_diagnostics.json")
        );
        assert_eq!(
            diagnostics_nda_path(root),
            std::path::PathBuf::from("/tmp/test/.velocity/build_diagnostics.nda")
        );
    }

    #[test]
    fn read_diagnostics_missing_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let diag = read_latest_diagnostics(tmp.path());
        assert_eq!(diag.summary, "No diagnostics available");
        assert!(!diag.success);
        assert!(diag.errors.is_empty());
    }

    #[test]
    fn serialize_diagnostics_nda_format() {
        let diag = BuildDiagnostics {
            timestamp_ms: 100,
            success: true,
            errors: vec![],
            warnings: vec!["warn1".to_string()],
            summary: "all good".to_string(),
        };
        let serialized = serialize_diagnostics_nda(&diag);
        assert!(serialized.starts_with("build-diagnostics version 2\n"));
        assert!(serialized.contains("timestamp_ms 100"));
        assert!(serialized.contains("success true"));
        assert!(serialized.contains("warning_count 1"));
        assert!(serialized.contains("issue\twarning\t0\twarn1"));
    }
}
