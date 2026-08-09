#![allow(dead_code)]
//! Deploy Pipeline Integration: manages build → test → deploy stages with
//! status tracking, artifact management, and rollback capability.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Maximum time a single pipeline command may run before it is killed.
/// Prevents a hung build/test/deploy from blocking the pipeline indefinitely.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

/// Pipeline execution stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    Build,
    Test,
    Package,
    Deploy,
}

impl PipelineStage {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Test => "Test",
            Self::Package => "Package",
            Self::Deploy => "Deploy",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Build => "⚙",
            Self::Test => "✓",
            Self::Package => "📦",
            Self::Deploy => "▲",
        }
    }

    pub fn all() -> &'static [PipelineStage] {
        &[Self::Build, Self::Test, Self::Package, Self::Deploy]
    }
}

/// Status of a pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageStatus {
    Pending,
    Running,
    Passed,
    Failed(String),
    Skipped,
}

impl StageStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Passed | Self::Failed(_) | Self::Skipped)
    }
}

/// A single stage result in the pipeline.
#[derive(Debug, Clone)]
pub struct StageResult {
    pub stage: PipelineStage,
    pub status: StageStatus,
    pub started_at: Option<Instant>,
    pub duration_ms: Option<u64>,
    pub output: String,
    pub artifacts: Vec<String>,
}

impl StageResult {
    pub fn new(stage: PipelineStage) -> Self {
        Self {
            stage,
            status: StageStatus::Pending,
            started_at: None,
            duration_ms: None,
            output: String::new(),
            artifacts: Vec::new(),
        }
    }
}

/// Pipeline configuration defining what commands to run at each stage.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub name: String,
    pub build_command: String,
    pub test_command: String,
    pub package_command: Option<String>,
    pub deploy_command: Option<String>,
    pub deploy_target: String,
    pub auto_deploy_on_success: bool,
    pub rollback_enabled: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            name: "Default Pipeline".to_string(),
            build_command: "cargo build --release".to_string(),
            test_command: "cargo test".to_string(),
            package_command: None,
            deploy_command: None,
            deploy_target: "production".to_string(),
            auto_deploy_on_success: false,
            rollback_enabled: true,
        }
    }
}

impl PipelineConfig {
    /// Create a Rust-specific pipeline.
    pub fn rust_pipeline() -> Self {
        Self {
            name: "Rust Release".to_string(),
            build_command: "cargo build --release".to_string(),
            test_command: "cargo test".to_string(),
            package_command: Some("cargo package".to_string()),
            deploy_command: None,
            deploy_target: "crates.io".to_string(),
            auto_deploy_on_success: false,
            rollback_enabled: true,
        }
    }

    /// Create a Node.js pipeline.
    pub fn node_pipeline() -> Self {
        Self {
            name: "Node.js Deploy".to_string(),
            build_command: "npm run build".to_string(),
            test_command: "npm test".to_string(),
            package_command: None,
            deploy_command: Some("npm run deploy".to_string()),
            deploy_target: "vercel".to_string(),
            auto_deploy_on_success: false,
            rollback_enabled: true,
        }
    }
}

/// A deployment record for rollback tracking.
#[derive(Debug, Clone)]
pub struct DeploymentRecord {
    pub id: u64,
    pub timestamp: Instant,
    pub target: String,
    pub version: String,
    pub status: StageStatus,
    pub can_rollback: bool,
}

/// The pipeline manager orchestrating build/test/deploy.
#[derive(Debug)]
pub struct PipelineManager {
    pub config: PipelineConfig,
    pub stages: Vec<StageResult>,
    pub current_stage: Option<PipelineStage>,
    pub history: VecDeque<Vec<StageResult>>,
    pub deployments: Vec<DeploymentRecord>,
    pub workspace_root: PathBuf,
    next_deploy_id: u64,
}

impl PipelineManager {
    pub fn new(workspace_root: PathBuf, config: PipelineConfig) -> Self {
        Self {
            config,
            stages: PipelineStage::all()
                .iter()
                .map(|s| StageResult::new(*s))
                .collect(),
            current_stage: None,
            history: VecDeque::with_capacity(10),
            deployments: Vec::new(),
            workspace_root,
            next_deploy_id: 1,
        }
    }

    /// Create a pipeline manager from a workspace root, auto-detecting build config.
    pub fn from_workspace(workspace_root: &Path) -> Self {
        let config = if workspace_root.join("Cargo.toml").exists() {
            PipelineConfig {
                name: "Rust Pipeline".into(),
                build_command: "cargo build --release".into(),
                test_command: "cargo test".into(),
                package_command: None,
                deploy_command: None,
                deploy_target: "local".into(),
                auto_deploy_on_success: false,
                rollback_enabled: true,
            }
        } else if workspace_root.join("package.json").exists() {
            PipelineConfig {
                name: "Node.js Pipeline".into(),
                build_command: "npm run build".into(),
                test_command: "npm test".into(),
                package_command: Some("npm pack".into()),
                deploy_command: None,
                deploy_target: "npm".into(),
                auto_deploy_on_success: false,
                rollback_enabled: false,
            }
        } else {
            PipelineConfig::default()
        };
        Self::new(workspace_root.to_path_buf(), config)
    }

    /// Trigger a full pipeline run (convenience wrapper around start()).
    pub fn trigger_run(&mut self) {
        let _ = self.start();
    }

    /// Start the pipeline from the build stage.
    pub fn start(&mut self) -> Result<(), String> {
        self.stages = PipelineStage::all()
            .iter()
            .map(|s| StageResult::new(*s))
            .collect();
        self.current_stage = Some(PipelineStage::Build);
        self.run_current_stage()
    }

    /// Run the current pipeline stage.
    fn run_current_stage(&mut self) -> Result<(), String> {
        let Some(stage) = self.current_stage else {
            return Ok(());
        };

        let command = match stage {
            PipelineStage::Build => self.config.build_command.clone(),
            PipelineStage::Test => self.config.test_command.clone(),
            PipelineStage::Package => {
                if let Some(ref cmd) = self.config.package_command {
                    cmd.clone()
                } else {
                    self.mark_stage(stage, StageStatus::Skipped);
                    self.advance_stage();
                    return Ok(());
                }
            }
            PipelineStage::Deploy => {
                if let Some(ref cmd) = self.config.deploy_command {
                    cmd.clone()
                } else {
                    self.mark_stage(stage, StageStatus::Skipped);
                    return Ok(());
                }
            }
        };

        self.mark_stage(stage, StageStatus::Running);
        let result = run_command(&command, &self.workspace_root);

        match result {
            Ok(output) => {
                if let Some(sr) = self.stages.iter_mut().find(|s| s.stage == stage) {
                    sr.output = output;
                    sr.artifacts = detect_artifacts(stage, &self.workspace_root);
                }
                self.mark_stage(stage, StageStatus::Passed);
                self.advance_stage();
                Ok(())
            }
            Err(err) => {
                if let Some(sr) = self.stages.iter_mut().find(|s| s.stage == stage) {
                    sr.output = err.clone();
                }
                self.mark_stage(stage, StageStatus::Failed(err.clone()));
                Err(err)
            }
        }
    }

    /// Advance to the next pipeline stage.
    fn advance_stage(&mut self) {
        let next = match self.current_stage {
            Some(PipelineStage::Build) => Some(PipelineStage::Test),
            Some(PipelineStage::Test) => Some(PipelineStage::Package),
            Some(PipelineStage::Package) => {
                if self.config.auto_deploy_on_success {
                    Some(PipelineStage::Deploy)
                } else {
                    None
                }
            }
            Some(PipelineStage::Deploy) => None,
            None => None,
        };
        self.current_stage = next;
        if next.is_some() {
            let _ = self.run_current_stage();
        } else {
            // Pipeline complete — archive to history
            if self.history.len() >= 10 {
                self.history.pop_front();
            }
            self.history.push_back(self.stages.clone());
        }
    }

    fn mark_stage(&mut self, stage: PipelineStage, status: StageStatus) {
        if let Some(sr) = self.stages.iter_mut().find(|s| s.stage == stage) {
            if status == StageStatus::Running {
                sr.started_at = Some(Instant::now());
            } else if let Some(started) = sr.started_at {
                sr.duration_ms = Some(started.elapsed().as_millis() as u64);
            }
            sr.status = status;
        }
    }

    /// Trigger deploy stage manually.
    pub fn deploy(&mut self) -> Result<(), String> {
        // Check that build+test passed
        let all_prior_passed = self
            .stages
            .iter()
            .filter(|s| s.stage != PipelineStage::Deploy)
            .all(|s| matches!(s.status, StageStatus::Passed | StageStatus::Skipped));

        if !all_prior_passed {
            return Err("Cannot deploy: prior stages have not passed".into());
        }

        self.current_stage = Some(PipelineStage::Deploy);
        self.run_current_stage()?;

        // Record deployment
        self.deployments.push(DeploymentRecord {
            id: self.next_deploy_id,
            timestamp: Instant::now(),
            target: self.config.deploy_target.clone(),
            version: format!("v0.{}", self.next_deploy_id),
            status: StageStatus::Passed,
            can_rollback: self.config.rollback_enabled,
        });
        self.next_deploy_id += 1;
        Ok(())
    }

    /// Rollback to the previous deployment.
    pub fn rollback(&mut self) -> Result<(), String> {
        if self.deployments.len() < 2 {
            return Err("No previous deployment to rollback to".into());
        }
        let prev = &self.deployments[self.deployments.len() - 2];
        if !prev.can_rollback {
            return Err("Previous deployment does not support rollback".into());
        }
        // Mark current as rolled back
        if let Some(current) = self.deployments.last_mut() {
            current.status = StageStatus::Failed("Rolled back".into());
        }
        Ok(())
    }

    /// Get overall pipeline status label.
    pub fn status_label(&self) -> &str {
        if self
            .stages
            .iter()
            .all(|s| matches!(s.status, StageStatus::Passed | StageStatus::Skipped))
        {
            "All Passed"
        } else if self
            .stages
            .iter()
            .any(|s| matches!(s.status, StageStatus::Failed(_)))
        {
            "Failed"
        } else if self.stages.iter().any(|s| s.status == StageStatus::Running) {
            "Running"
        } else {
            "Pending"
        }
    }

    /// Duration string for the entire pipeline.
    pub fn total_duration_ms(&self) -> u64 {
        self.stages.iter().filter_map(|s| s.duration_ms).sum()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // T3b: Git push + CI trigger wiring
    // ═══════════════════════════════════════════════════════════════════════

    /// Git push: stage all changes, commit with message, and push to remote.
    /// Returns the push output on success. Arguments are passed as discrete
    /// argv entries, so commit messages containing spaces or quotes are safe.
    pub fn git_push(
        &self,
        commit_message: &str,
        remote: &str,
        branch: &str,
    ) -> Result<String, String> {
        // Stage all changes
        run_command_args(&["git", "add", "-A"], &self.workspace_root)?;

        // Commit (message passed as a single argv entry — no shell quoting needed)
        let commit_out = run_command_args(
            &["git", "commit", "-m", commit_message],
            &self.workspace_root,
        )?;

        // Push to remote
        let push_out = run_command_args(&["git", "push", remote, branch], &self.workspace_root)?;

        Ok(format!("{}\n{}", commit_out, push_out))
    }

    /// Trigger a CI pipeline via GitHub Actions workflow_dispatch.
    /// Requires `gh` CLI to be available.
    pub fn trigger_ci_github(&self, workflow: &str, ref_branch: &str) -> Result<String, String> {
        run_command_args(
            &["gh", "workflow", "run", workflow, "--ref", ref_branch],
            &self.workspace_root,
        )
    }

    /// Trigger a CI pipeline via a generic webhook URL (e.g., GitLab, Jenkins).
    /// Uses `curl` to POST to the webhook. The JSON payload is passed as a
    /// single argv entry so embedded quotes/spaces are preserved verbatim.
    pub fn trigger_ci_webhook(&self, webhook_url: &str, payload: &str) -> Result<String, String> {
        run_command_args(
            &[
                "curl",
                "-s",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
                "-d",
                payload,
                webhook_url,
            ],
            &self.workspace_root,
        )
    }

    /// Full deploy flow: build → test → git push → trigger CI.
    /// This is the one-click "ship it" pipeline.
    pub fn ship_it(
        &mut self,
        commit_message: &str,
        remote: &str,
        branch: &str,
        ci_workflow: Option<&str>,
    ) -> Result<String, String> {
        // Run build + test first
        self.start()?;

        // Check all stages passed
        let all_passed = self
            .stages
            .iter()
            .all(|s| matches!(s.status, StageStatus::Passed | StageStatus::Skipped));
        if !all_passed {
            return Err("Pipeline stages did not pass — aborting ship".into());
        }

        // Git push
        let push_out = self.git_push(commit_message, remote, branch)?;

        // Trigger CI if configured
        let ci_out = if let Some(workflow) = ci_workflow {
            match self.trigger_ci_github(workflow, branch) {
                Ok(out) => format!("\nCI triggered: {}", out),
                Err(e) => format!("\nCI trigger failed (non-fatal): {}", e),
            }
        } else {
            String::new()
        };

        Ok(format!("Shipped successfully!\n{}{}", push_out, ci_out))
    }
}

/// Run a shell command (parsed with [`parse_shell_words`]) and capture output.
fn run_command(command: &str, cwd: &Path) -> Result<String, String> {
    let args = parse_shell_words(command);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_command_args(&arg_refs, cwd)
}

/// Split a command line into arguments, honoring single and double quotes so
/// that arguments containing spaces survive (e.g. `commit -m "fix: a b"`).
fn parse_shell_words(command: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut has_token = false;
    for c in command.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                has_token = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_token = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_token {
                    args.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        args.push(current);
    }
    args
}

/// Run a command expressed as discrete argv entries (no shell re-splitting),
/// guarded by [`COMMAND_TIMEOUT`]. Captures combined stdout/stderr.
fn run_command_args(args: &[&str], cwd: &Path) -> Result<String, String> {
    run_command_args_with_timeout(args, cwd, COMMAND_TIMEOUT)
}

/// Run a command with an explicit timeout. The child process is polled via
/// `try_wait` so a hung command is killed once `timeout` elapses rather than
/// blocking the pipeline indefinitely.
fn run_command_args_with_timeout(
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| "Empty command".to_string())?;

    let mut child = Command::new(program)
        .args(rest)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to execute {}: {}", program, e))?;

    let deadline = Instant::now() + timeout;
    let mut exit_status: Option<std::process::ExitStatus> = None;
    let exited = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_status = Some(status);
                break true;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("Failed to poll {}: {}", program, e)),
        }
    };

    if !exited {
        return Err(format!(
            "Command {} timed out after {}s and was killed",
            program,
            timeout.as_secs()
        ));
    }

    let stdout = child
        .stdout
        .take()
        .map(|mut r| {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut r, &mut s);
            s
        })
        .unwrap_or_default();
    let stderr = child
        .stderr
        .take()
        .map(|mut r| {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut r, &mut s);
            s
        })
        .unwrap_or_default();
    let status =
        exit_status.ok_or_else(|| format!("Command {} exited without a status", program))?;

    finish_output(stdout, stderr, status)
}

/// Interpret a completed command's output as success or a descriptive error.
fn finish_output(
    stdout: String,
    stderr: String,
    status: std::process::ExitStatus,
) -> Result<String, String> {
    if status.success() {
        Ok(format!("{}{}", stdout, stderr))
    } else {
        Err(format!(
            "Exit code {}: {}{}",
            status.code().unwrap_or(-1),
            stdout,
            stderr
        ))
    }
}

/// Detect build artifacts based on the stage and workspace.
fn detect_artifacts(stage: PipelineStage, workspace_root: &Path) -> Vec<String> {
    let mut artifacts = Vec::new();
    match stage {
        PipelineStage::Build => {
            let release_dir = workspace_root.join("target").join("release");
            if release_dir.exists() {
                artifacts.push(release_dir.display().to_string());
            }
        }
        PipelineStage::Package => {
            let package_dir = workspace_root.join("target").join("package");
            if package_dir.exists() {
                artifacts.push(package_dir.display().to_string());
            }
        }
        _ => {}
    }
    artifacts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn pipeline_config_defaults() {
        let config = PipelineConfig::default();
        assert!(config.build_command.contains("cargo build"));
        assert!(config.test_command.contains("cargo test"));
        assert!(!config.auto_deploy_on_success);
    }

    #[test]
    fn stage_ordering() {
        let stages = PipelineStage::all();
        assert_eq!(stages[0], PipelineStage::Build);
        assert_eq!(stages[1], PipelineStage::Test);
        assert_eq!(stages[2], PipelineStage::Package);
        assert_eq!(stages[3], PipelineStage::Deploy);
    }

    #[test]
    fn stage_result_initial_state() {
        let result = StageResult::new(PipelineStage::Build);
        assert_eq!(result.status, StageStatus::Pending);
        assert!(result.output.is_empty());
    }

    #[test]
    fn rollback_requires_history() {
        let mut pm = PipelineManager::new(PathBuf::from("/tmp"), PipelineConfig::default());
        let result = pm.rollback();
        assert!(result.is_err());
    }

    #[test]
    fn deploy_requires_prior_stages() {
        let mut pm = PipelineManager::new(
            PathBuf::from("/tmp"),
            PipelineConfig {
                deploy_command: Some("echo deploy".into()),
                ..Default::default()
            },
        );
        let result = pm.deploy();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("prior stages"));
    }

    #[test]
    fn status_label_pending() {
        let pm = PipelineManager::new(PathBuf::from("/tmp"), PipelineConfig::default());
        assert_eq!(pm.status_label(), "Pending");
    }

    #[test]
    fn parse_shell_words_splits_on_whitespace() {
        assert_eq!(
            parse_shell_words("cargo build --release"),
            ["cargo", "build", "--release"]
        );
    }

    #[test]
    fn parse_shell_words_preserves_double_quoted_spaces() {
        assert_eq!(
            parse_shell_words(r#"git commit -m "fix: a b""#),
            ["git", "commit", "-m", "fix: a b"]
        );
    }

    #[test]
    fn parse_shell_words_preserves_single_quoted_spaces() {
        assert_eq!(
            parse_shell_words("echo 'hello world'"),
            ["echo", "hello world"]
        );
    }

    #[test]
    fn parse_shell_words_empty_and_blank() {
        assert!(parse_shell_words("").is_empty());
        assert!(parse_shell_words("   ").is_empty());
    }

    #[test]
    fn run_command_args_empty_is_error() {
        let temp = tempfile::tempdir().unwrap();
        assert!(run_command_args(&[], temp.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn run_command_args_captures_output() {
        let temp = tempfile::tempdir().unwrap();
        let out = run_command_args(&["cmd", "/C", "echo hello"], temp.path()).unwrap();
        assert!(out.contains("hello"));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_args_reports_failure() {
        let temp = tempfile::tempdir().unwrap();
        let err = run_command_args(&["cmd", "/C", "exit 3"], temp.path()).unwrap_err();
        assert!(err.contains("Exit code 3"));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_args_missing_program_is_error() {
        let temp = tempfile::tempdir().unwrap();
        let err = run_command_args(&["definitely_not_a_real_cmd_xyz"], temp.path()).unwrap_err();
        assert!(err.contains("Failed to execute"));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_parses_quoted_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let out = run_command(r#"cmd /C "echo hello world""#, temp.path()).unwrap();
        assert!(out.contains("hello world"));
    }

    #[cfg(windows)]
    #[test]
    fn run_command_args_times_out_hung_process() {
        let temp = tempfile::tempdir().unwrap();
        // `ping -n 30` runs ~30s; a 100ms budget must trip the timeout guard.
        let err = run_command_args_with_timeout(
            &["ping", "-n", "30", "127.0.0.1"],
            temp.path(),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(err.contains("timed out"));
    }
}
