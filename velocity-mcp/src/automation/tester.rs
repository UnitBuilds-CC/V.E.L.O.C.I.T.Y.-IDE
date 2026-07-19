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

pub fn run_jit_tests_in_sandbox(
    _workspace_root: &PathBuf,
    test_file: &PathBuf,
) -> Result<TestReport, String> {
    if !test_file.exists() {
        return Err(format!("Test file not found: {:?}", test_file));
    }

    Ok(TestReport {
        success: false,
        stdout: String::new(),
        stderr: String::new(),
        summary: format!(
            "JIT sandbox test execution is not implemented for {:?}; refusing to report fake success",
            test_file.file_name().unwrap_or_default()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_sandbox_runner_reports_unsupported_instead_of_fake_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("sample_test.rs");
        std::fs::write(&test_file, "fn test_one() {}\nfn test_two() {}").unwrap();

        let report = run_jit_tests_in_sandbox(&temp_dir.path().to_path_buf(), &test_file).unwrap();
        assert!(!report.success);
        assert!(report.summary.contains("not implemented"));
        assert!(report.summary.contains("refusing to report fake success"));
    }
}
