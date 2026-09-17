//! Shared PowerShell host invocation for the Windows-Automation subsystem.
//!
//! Every `wa_*` tool that reaches the Win32/UIA world does it by handing a
//! generated script to `powershell.exe`. Those calls used to be duplicated per
//! module and each one waited on `wait_with_output()` with no deadline, so a
//! PowerShell child that stalls — a UIA cross-process COM call against an
//! unresponsive window is enough — wedged the MCP server thread forever and
//! took the whole tool session down with it.
//!
//! This module owns the single bounded implementation, and it runs scripts via
//! `-File <temp.ps1>` rather than piping them into `-Command -`: Windows
//! PowerShell 5.1 consumes piped command input line by line, so every
//! multi-line statement these generators emit (`while`, `function`, hashtable
//! literals) was discarded while the process still exited 0 with no output.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Default budget for a generated PowerShell script. Spawning the host and
/// loading .NET assemblies costs ~1-2 s; UIA tree walks on a busy desktop
/// have been observed near 10 s. Anything past this is treated as stuck.
pub const DEFAULT_BUDGET: Duration = Duration::from_secs(45);

/// Extra time granted on top of a script's own internal deadline, so a script
/// that sleeps for its configured duration is never killed while healthy.
pub const SLACK: Duration = Duration::from_secs(20);

/// Preamble forcing UTF-8 on the script's output stream. Windows PowerShell 5.1
/// writes redirected stdout in the OEM console codepage, so every non-ASCII
/// result — window titles, element names, captured text — arrived as `?`
/// (bug #20). No generated script starts with `param()` or `#Requires`, so
/// prepending a statement is safe.
const UTF8_PREAMBLE: &str = "try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
                             $OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }\n";

/// Run a PowerShell script with the default budget.
pub fn run_ps_script(script: &str) -> Result<String, String> {
    run_ps_script_budget(script, DEFAULT_BUDGET)
}

/// Materialise the script as a uniquely named temp `.ps1`.
///
/// A UTF-8 BOM is written because Windows PowerShell 5.1 decodes BOM-less
/// scripts as ANSI, which would mangle any non-ASCII literal embedded in the
/// generated script (window titles, typed text, paths).
fn write_script_file(script: &str) -> Result<PathBuf, String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "velocity_wa_{}_{}_{}.ps1",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut bytes = Vec::with_capacity(script.len() + UTF8_PREAMBLE.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(UTF8_PREAMBLE.as_bytes());
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(&path, bytes)
        .map_err(|e| format!("write temp script {}: {e}", path.display()))?;
    Ok(path)
}

/// Run a PowerShell script, forcing a kill if it overruns `budget`.
///
/// Returns the trimmed stdout on success; on non-zero exit the stderr text is
/// surfaced, on silent-error exit a stderr complaint is surfaced, and on
/// deadline expiry the child (and its tree) is killed and the error says so,
/// so callers get an honest failure instead of a hang or empty "success".
pub fn run_ps_script_budget(script: &str, budget: Duration) -> Result<String, String> {
    run_ps_script_env(script, budget, &[])
}

/// Run a PowerShell script with per-call environment variables.
///
/// Several UIA generators take their parameters through `$env:` rather than
/// arguments. They used to spawn `powershell` themselves because this helper
/// had no way to pass them, which meant they also kept the pre-bug-#19
/// `-Command -` input mode (multi-line scripts were discarded statement by
/// statement) and an unbounded `wait_with_output()`. Routing them through here
/// gives them the `-File` execution and the deadline as well.
pub fn run_ps_script_env(
    script: &str,
    budget: Duration,
    envs: &[(&str, &str)],
) -> Result<String, String> {
    let script_path = write_script_file(script)?;
    let mut command = Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    let spawned = command.spawn();
    let child = match spawned {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return Err(format!("failed to spawn powershell: {e}"));
        }
    };

    match wait_with_budget(child, budget) {
        Ok(output) => {
            let _ = std::fs::remove_file(&script_path);
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !output.status.success() {
                return Err(format!(
                    "PowerShell error: {}",
                    if stderr.is_empty() {
                        format!("script exited with {}", output.status)
                    } else {
                        stderr
                    }
                ));
            }
            // Parse and type errors leave exit code 0 with the complaint only on
            // stderr, which previously read back as "succeeded, produced nothing".
            if stdout.is_empty() && !stderr.is_empty() {
                return Err(format!(
                    "PowerShell exited clean but printed nothing: {stderr}"
                ));
            }
            Ok(stdout)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            Err(e)
        }
    }
}

/// `wait_with_output` on a worker thread with a hard deadline. On expiry the
/// child process tree is killed, which releases the pipes so the worker
/// thread unwinds instead of leaking. A killed script always reports the
/// deadline — never a generic non-zero-exit error, which would hide it.
fn wait_with_budget(
    child: std::process::Child,
    budget: Duration,
) -> Result<std::process::Output, String> {
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(budget) {
        Ok(result) => result.map_err(|e| format!("wait: {e}")),
        Err(_) => {
            kill_tree(pid);
            // Drain whatever the killed child managed to write so the worker
            // thread and its pipes are released.
            let _ = rx.recv_timeout(Duration::from_secs(5));
            Err(format!(
                "PowerShell script exceeded its {}s budget and was killed (pid {pid})",
                budget.as_secs()
            ))
        }
    }
}

/// Force-terminate a child and everything it spawned, so a hung script cannot
/// keep the tool call alive behind us.
fn kill_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn runs_a_script_and_captures_stdout() {
        let out = run_ps_script("Write-Output 'ps-ok'").unwrap();
        assert!(out.contains("ps-ok"), "got: {out}");
    }

    #[test]
    fn surfaces_script_errors() {
        let err = run_ps_script("$ErrorActionPreference='Stop'; throw 'boom'").unwrap_err();
        assert!(err.contains("PowerShell error"), "got: {err}");
    }

    /// The regression this module exists for: a script that never finishes must
    /// fail on a deadline, not block the caller forever.
    #[test]
    fn kills_a_stalled_script_on_budget_expiry() {
        let start = std::time::Instant::now();
        let err = run_ps_script_budget(
            "Start-Sleep -Seconds 60; Write-Output late",
            DEFAULT_BUDGET_MIN,
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        assert!(
            err.contains("budget") || err.contains("did not exit"),
            "expected a budget-exceeded error, got: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "stalled script should be killed quickly, waited {elapsed:?}"
        );
    }

    /// A script whose own sleep is inside the budget must not be cut short.
    #[test]
    fn healthy_script_within_budget_completes() {
        let out = run_ps_script_budget(
            "Start-Sleep -Milliseconds 300; Write-Output done",
            DEFAULT_BUDGET,
        )
        .unwrap();
        assert!(out.contains("done"), "got: {out}");
    }

    /// The regression for bug #19: statements spanning several lines (`while`,
    /// `function`, hashtable literals) used to vanish when the script was piped
    /// through `-Command -`, yet the process still exited 0.
    #[test]
    fn multi_line_scripts_execute() {
        let script = "\
$h = @{
  a = 9
  b = 9
}
$i = 0
while ($i -lt 2) {
  $key = ('a','b')[$i]
  $h[$key] = $i
  $i++
}
ConvertTo-Json $h -Compress";
        let out = run_ps_script(script).unwrap();
        assert!(out.contains("\"a\":0"), "got: {out}");
        assert!(out.contains("\"b\":1"), "got: {out}");
    }

    /// Bug #18: a broken script must not report success because it exited 0.
    #[test]
    fn silent_parse_errors_are_surfaced() {
        let err = run_ps_script("Write-Output a\n{").unwrap_err();
        assert!(
            err.contains("printed nothing") || err.contains("PowerShell error"),
            "got: {err}"
        );
    }

    /// Non-ASCII literals must survive the round trip through the script file.
    #[test]
    fn unicode_literals_survive() {
        let out = run_ps_script("Write-Output 'éneś — 日本語'").unwrap();
        assert!(out.contains("日本語"), "got: {out}");
        assert!(out.contains("éneś"), "got: {out}");
    }

    /// Short budget used by the stall test so it does not slow the suite down.
    const DEFAULT_BUDGET_MIN: Duration = Duration::from_secs(2);
}

/// Source guards over the generated scripts. These run on every platform
/// because they read the script text rather than executing it.
#[cfg(test)]
mod source_guards {
    /// Every module that builds a PowerShell script passed to this runner.
    fn scripts() -> Vec<(&'static str, &'static str)> {
        vec![
            ("wa/events.rs", include_str!("./events.rs")),
            ("wa/browser_bridge.rs", include_str!("./browser_bridge.rs")),
            ("wa/triggers.rs", include_str!("./triggers.rs")),
            ("wa/file_dialog.rs", include_str!("./file_dialog.rs")),
            ("wa/advanced_input.rs", include_str!("./advanced_input.rs")),
            ("wa/screenshot.rs", include_str!("./screenshot.rs")),
            ("wa/notifications.rs", include_str!("./notifications.rs")),
            (
                "wa/virtual_desktop.rs",
                include_str!("./virtual_desktop.rs"),
            ),
            ("wa/multi_monitor.rs", include_str!("./multi_monitor.rs")),
            ("wa/registry.rs", include_str!("./registry.rs")),
            ("wa/ocr.rs", include_str!("./ocr.rs")),
            ("wa/clipboard.rs", include_str!("./clipboard.rs")),
            ("wa/process_mgmt.rs", include_str!("./process_mgmt.rs")),
            ("wa/window_mgmt.rs", include_str!("./window_mgmt.rs")),
            (
                "wa/windows/scripts.rs",
                include_str!("./windows/scripts.rs"),
            ),
        ]
    }

    /// Script-building code only: the test modules legitimately quote the
    /// broken idioms while asserting that they stay gone.
    fn production(source: &str) -> &str {
        match source.find("#[cfg(test") {
            Some(at) => &source[..at],
            None => source,
        }
    }

    /// The `fn` body containing `pos`, so a variable name reused by a
    /// neighbouring script builder cannot raise a false positive.
    fn enclosing_fn(code: &str, pos: usize) -> &str {
        let patterns = ["\nfn ", "\npub fn ", "\npub(crate) fn "];
        let start = patterns
            .iter()
            .filter_map(|pat| code[..pos].rfind(pat))
            .max()
            .unwrap_or(0);
        let body = &code[start..];
        let mut end = body.len();
        for pat in patterns {
            if let Some(rel) = body[1..].find(pat) {
                end = end.min(rel + 1);
            }
        }
        &body[..end]
    }

    /// `[Environment]::TickCount64` does not exist in Windows PowerShell 5.1,
    /// where it evaluates to `$null`. `null -lt $deadline` is always true, so
    /// every polling loop guarded by it spun forever until the budget killed
    /// it (bug #16). `[System.Diagnostics.Stopwatch]` is the portable monotonic
    /// clock, so no generated script may reach back for the broken API.
    #[test]
    fn generated_scripts_never_use_tickcount64() {
        for (name, source) in scripts() {
            assert!(
                !production(source).contains("TickCount64"),
                "{name} still uses [Environment]::TickCount64, which is absent on PowerShell 5.1"
            );
        }
    }

    /// Array-wrapping a `Generic.List` with `@(...)` throws "Argument types do
    /// not match" on Windows PowerShell 5.1 (bug #17) and empties the script's
    /// output, so those lists must be converted with `.ToArray()`.
    #[test]
    fn generated_scripts_never_array_wrap_generic_lists() {
        let marker = "New-Object System.Collections.Generic.List";
        for (name, source) in scripts() {
            let code = production(source);
            for (pos, _) in code.match_indices(marker) {
                let scope = enclosing_fn(code, pos);
                let head = &code[..pos];
                let line_start = head.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let decl = code[line_start..pos].trim_start();
                let Some(dollar) = decl.find('$') else {
                    continue;
                };
                let var: String = decl[dollar + 1..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if var.is_empty() {
                    continue;
                }
                let wrapped = format!("@(${var})");
                assert!(
                    !scope.contains(&wrapped),
                    "{name} array-wraps the generic list {wrapped}; use {var}.ToArray()"
                );
            }
        }
    }

    /// Every polled wait loop must also be bounded by a sleep, otherwise it
    /// busy-spins against a UIA tree and floods the COM apartment.
    #[test]
    fn polling_loops_sleep() {
        assert!(production(include_str!("./events.rs")).contains("Start-Sleep -Milliseconds 50"));
        assert!(production(include_str!("./browser_bridge.rs"))
            .contains("Start-Sleep -Milliseconds 200"));
    }
}
