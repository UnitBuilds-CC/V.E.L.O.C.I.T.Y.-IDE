use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TestReport {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub summary: String,
}

pub fn run_tests_on_demand(workspace_root: &PathBuf, package: Option<&str>) -> TestReport {
    let mut cmd = Command::new("cargo");
    cmd.arg("test").current_dir(workspace_root);
    if let Some(pkg) = package {
        cmd.arg("-p").arg(pkg);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return TestReport {
                success: false,
                stdout: String::new(),
                stderr: e.to_string(),
                summary: format!("cargo test failed to run: {}", e),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let summary = if output.status.success() {
        "cargo test passed".into()
    } else {
        format!("cargo test failed ({}", output.status)
    };

    TestReport {
        success: output.status.success(),
        stdout,
        stderr,
        summary,
    }
}
