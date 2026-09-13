//! E2E test harness for orchestrating integration scenarios.
//!
//! Provides [`TestHarness`] for setting up isolated workspaces, running
//! predefined or custom [`E2EScenario`]s, and collecting structured
//! [`TestOutcome`] results with per-step timing.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Core data types
// ---------------------------------------------------------------------------

/// Outcome of a single executed step inside a scenario.
#[derive(Debug, Clone)]
pub struct TestStep {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
}

/// Outcome of a full scenario run.
#[derive(Debug, Clone)]
pub struct TestOutcome {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub steps: Vec<TestStep>,
}

/// A single declarative step inside an [`E2EScenario`].
#[derive(Debug, Clone)]
pub enum ScenarioStep {
    CreateFile { path: String, content: String },
    OpenFile { path: String },
    EditFile { path: String, line: usize, content: String },
    RunAgent { prompt: String },
    VerifyFile { path: String, contains: String },
    DeleteFile { path: String },
    WaitFor { condition: String, timeout_ms: u64 },
}

/// A named, descriptive collection of [`ScenarioStep`]s.
#[derive(Debug, Clone)]
pub struct E2EScenario {
    pub name: String,
    pub description: String,
    pub steps: Vec<ScenarioStep>,
}

/// Aggregate summary produced by [`TestHarness::summary`].
#[derive(Debug, Clone)]
pub struct HarnessSummary {
    pub total_scenarios: usize,
    pub passed: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
    pub step_count: usize,
}

// ---------------------------------------------------------------------------
// TestHarness
// ---------------------------------------------------------------------------

/// Orchestrates E2E test scenarios inside an isolated workspace directory.
pub struct TestHarness {
    pub workspace_root: PathBuf,
    pub test_results: Vec<TestOutcome>,
    pub setup_complete: bool,
}

impl TestHarness {
    /// Create a new harness rooted at `workspace_root`.
    ///
    /// The directory is **not** created until [`setup`](Self::setup) is called.
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            test_results: Vec::new(),
            setup_complete: false,
        }
    }

    /// Prepare the test workspace directory.
    pub fn setup(&mut self) -> Result<(), String> {
        if self.setup_complete {
            return Ok(());
        }
        fs::create_dir_all(&self.workspace_root)
            .map_err(|e| format!("failed to create workspace root: {}", e))?;
        self.setup_complete = true;
        Ok(())
    }

    /// Run a full scenario, recording a [`TestOutcome`].
    pub fn run_scenario(&mut self, scenario: &E2EScenario) -> TestOutcome {
        let scenario_start = Instant::now();
        let mut steps = Vec::new();
        let mut first_error: Option<String> = None;

        for step in &scenario.steps {
            let step_start = Instant::now();
            let step_name = Self::step_display_name(step);
            let result = self.execute_step(step);
            let elapsed = step_start.elapsed().as_millis() as u64;

            match result {
                Ok(()) => {
                    steps.push(TestStep {
                        name: step_name,
                        passed: true,
                        duration_ms: elapsed,
                    });
                }
                Err(e) => {
                    steps.push(TestStep {
                        name: step_name,
                        passed: false,
                        duration_ms: elapsed,
                    });
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                    // Stop executing further steps on first failure.
                    break;
                }
            }
        }

        let total_ms = scenario_start.elapsed().as_millis() as u64;
        let passed = first_error.is_none();
        let outcome = TestOutcome {
            name: scenario.name.clone(),
            passed,
            duration_ms: total_ms,
            error: first_error,
            steps,
        };
        self.test_results.push(outcome.clone());
        outcome
    }

    /// Execute a single [`ScenarioStep`] against the workspace.
    pub fn execute_step(&self, step: &ScenarioStep) -> Result<(), String> {
        match step {
            ScenarioStep::CreateFile { path, content } => {
                let full = self.workspace_root.join(path);
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("create dirs for {}: {}", path, e))?;
                }
                fs::write(&full, content)
                    .map_err(|e| format!("write {}: {}", path, e))
            }
            ScenarioStep::OpenFile { path } => {
                let full = self.workspace_root.join(path);
                if !full.exists() {
                    return Err(format!("open {}: file does not exist", path));
                }
                // Reading validates that the file is accessible.
                fs::read_to_string(&full)
                    .map_err(|e| format!("read {}: {}", path, e))?;
                Ok(())
            }
            ScenarioStep::EditFile { path, line, content } => {
                let full = self.workspace_root.join(path);
                let mut text = fs::read_to_string(&full)
                    .map_err(|e| format!("read for edit {}: {}", path, e))?;
                let mut lines: Vec<&str> = text.lines().collect();
                // Ensure enough lines exist (pad with empty lines).
                while lines.len() <= *line {
                    lines.push("");
                }
                lines[*line] = content;
                text = lines.join("\n");
                // Preserve trailing newline if original had one.
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                fs::write(&full, text)
                    .map_err(|e| format!("write edited {}: {}", path, e))
            }
            ScenarioStep::RunAgent { prompt } => {
                // In the harness layer we record the prompt but do not spawn a
                // real agent process.  Integration tests that need a live agent
                // should wrap this step.  Here we simply validate the prompt is
                // non-empty so mis-configured scenarios fail fast.
                if prompt.trim().is_empty() {
                    return Err("RunAgent: prompt is empty".into());
                }
                Ok(())
            }
            ScenarioStep::VerifyFile { path, contains } => {
                let full = self.workspace_root.join(path);
                let text = fs::read_to_string(&full)
                    .map_err(|e| format!("read for verify {}: {}", path, e))?;
                if !text.contains(contains.as_str()) {
                    return Err(format!(
                        "verify {}: expected content {:?} not found",
                        path, contains
                    ));
                }
                Ok(())
            }
            ScenarioStep::DeleteFile { path } => {
                let full = self.workspace_root.join(path);
                if full.exists() {
                    fs::remove_file(&full)
                        .map_err(|e| format!("delete {}: {}", path, e))?;
                }
                Ok(())
            }
            ScenarioStep::WaitFor {
                condition,
                timeout_ms: _,
            } => {
                // The harness does not implement a real polling loop; integration
                // tests may override this.  We accept any non-empty condition.
                if condition.trim().is_empty() {
                    return Err("WaitFor: condition string is empty".into());
                }
                Ok(())
            }
        }
    }

    /// Remove the workspace directory and all its contents.
    pub fn cleanup(&mut self) {
        if self.workspace_root.exists() {
            let _ = fs::remove_dir_all(&self.workspace_root);
        }
        self.setup_complete = false;
    }

    /// Produce a [`HarnessSummary`] from all recorded outcomes.
    pub fn summary(&self) -> HarnessSummary {
        let total = self.test_results.len();
        let passed = self.test_results.iter().filter(|r| r.passed).count();
        let failed = total - passed;
        let total_ms: u64 = self.test_results.iter().map(|r| r.duration_ms).sum();
        let steps: usize = self.test_results.iter().map(|r| r.steps.len()).sum();
        HarnessSummary {
            total_scenarios: total,
            passed,
            failed,
            total_duration_ms: total_ms,
            step_count: steps,
        }
    }

    // -- internal helpers ---------------------------------------------------

    fn step_display_name(step: &ScenarioStep) -> String {
        match step {
            ScenarioStep::CreateFile { path, .. } => format!("CreateFile({})", path),
            ScenarioStep::OpenFile { path } => format!("OpenFile({})", path),
            ScenarioStep::EditFile { path, line, .. } => {
                format!("EditFile({}, line {})", path, line)
            }
            ScenarioStep::RunAgent { prompt } => {
                let preview: String = prompt.chars().take(30).collect();
                format!("RunAgent({:?}..)", preview)
            }
            ScenarioStep::VerifyFile { path, .. } => format!("VerifyFile({})", path),
            ScenarioStep::DeleteFile { path } => format!("DeleteFile({})", path),
            ScenarioStep::WaitFor { condition, .. } => {
                format!("WaitFor({:?})", condition)
            }
        }
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ---------------------------------------------------------------------------
// Predefined scenarios
// ---------------------------------------------------------------------------

/// Create a file, edit a line, verify content, then delete it.
pub fn file_lifecycle_scenario() -> E2EScenario {
    E2EScenario {
        name: "file_lifecycle".into(),
        description: "Create, edit, verify, and delete a single file".into(),
        steps: vec![
            ScenarioStep::CreateFile {
                path: "hello.txt".into(),
                content: "line zero\nline one\nline two\n".into(),
            },
            ScenarioStep::OpenFile {
                path: "hello.txt".into(),
            },
            ScenarioStep::EditFile {
                path: "hello.txt".into(),
                line: 1,
                content: "line ONE (edited)".into(),
            },
            ScenarioStep::VerifyFile {
                path: "hello.txt".into(),
                contains: "line ONE (edited)".into(),
            },
            ScenarioStep::DeleteFile {
                path: "hello.txt".into(),
            },
        ],
    }
}

/// Run a basic agent prompt and validate it is accepted.
pub fn agent_basic_scenario() -> E2EScenario {
    E2EScenario {
        name: "agent_basic".into(),
        description: "Send a simple prompt to the agent and validate acceptance".into(),
        steps: vec![
            ScenarioStep::CreateFile {
                path: "input.txt".into(),
                content: "Hello, agent!\n".into(),
            },
            ScenarioStep::RunAgent {
                prompt: "Read input.txt and summarise it.".into(),
            },
            ScenarioStep::VerifyFile {
                path: "input.txt".into(),
                contains: "Hello".into(),
            },
        ],
    }
}

/// Work with multiple files in nested directories.
pub fn multi_file_scenario() -> E2EScenario {
    E2EScenario {
        name: "multi_file".into(),
        description: "Create and verify files across nested directories".into(),
        steps: vec![
            ScenarioStep::CreateFile {
                path: "src/main.rs".into(),
                content: "fn main() {\n    println!(\"hello\");\n}\n".into(),
            },
            ScenarioStep::CreateFile {
                path: "src/lib.rs".into(),
                content: "pub fn add(a: i32, b: i32) -> i32 { a + b }\n".into(),
            },
            ScenarioStep::CreateFile {
                path: "Cargo.toml".into(),
                content: "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n".into(),
            },
            ScenarioStep::OpenFile {
                path: "src/main.rs".into(),
            },
            ScenarioStep::VerifyFile {
                path: "src/lib.rs".into(),
                contains: "fn add".into(),
            },
            ScenarioStep::EditFile {
                path: "src/main.rs".into(),
                line: 1,
                content: "    println!(\"world\");".into(),
            },
            ScenarioStep::VerifyFile {
                path: "src/main.rs".into(),
                contains: "world".into(),
            },
            ScenarioStep::DeleteFile {
                path: "src/main.rs".into(),
            },
            ScenarioStep::DeleteFile {
                path: "src/lib.rs".into(),
            },
            ScenarioStep::DeleteFile {
                path: "Cargo.toml".into(),
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: build a harness inside a fresh temp directory.
    fn harness() -> (TestHarness, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        let h = TestHarness::new(tmp.path().to_path_buf());
        (h, tmp)
    }

    // ---- construction / setup ---------------------------------------------

    #[test]
    fn new_harness_is_not_setup() {
        let (h, _tmp) = harness();
        assert!(!h.setup_complete);
        assert!(h.test_results.is_empty());
    }

    #[test]
    fn setup_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sub").join("dir");
        let mut h = TestHarness::new(root.clone());
        assert!(!root.exists());
        h.setup().unwrap();
        assert!(root.exists());
        assert!(h.setup_complete);
    }

    #[test]
    fn setup_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let mut h = TestHarness::new(tmp.path().to_path_buf());
        h.setup().unwrap();
        h.setup().unwrap(); // must not error
        assert!(h.setup_complete);
    }

    // ---- execute_step: CreateFile / OpenFile / VerifyFile -----------------

    #[test]
    fn create_and_open_file() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::CreateFile {
            path: "a.txt".into(),
            content: "hello".into(),
        })
        .unwrap();
        h.execute_step(&ScenarioStep::OpenFile {
            path: "a.txt".into(),
        })
        .unwrap();
    }

    #[test]
    fn open_nonexistent_file_errors() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        let err = h
            .execute_step(&ScenarioStep::OpenFile {
                path: "nope.txt".into(),
            })
            .unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn verify_file_content_match() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::CreateFile {
            path: "v.txt".into(),
            content: "alpha beta".into(),
        })
        .unwrap();
        h.execute_step(&ScenarioStep::VerifyFile {
            path: "v.txt".into(),
            contains: "beta".into(),
        })
        .unwrap();
    }

    #[test]
    fn verify_file_content_mismatch_errors() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::CreateFile {
            path: "v.txt".into(),
            content: "alpha".into(),
        })
        .unwrap();
        let err = h
            .execute_step(&ScenarioStep::VerifyFile {
                path: "v.txt".into(),
                contains: "gamma".into(),
            })
            .unwrap_err();
        assert!(err.contains("not found"));
    }

    // ---- EditFile ---------------------------------------------------------

    #[test]
    fn edit_file_replaces_line() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::CreateFile {
            path: "e.txt".into(),
            content: "aaa\nbbb\nccc\n".into(),
        })
        .unwrap();
        h.execute_step(&ScenarioStep::EditFile {
            path: "e.txt".into(),
            line: 1,
            content: "BBB".into(),
        })
        .unwrap();
        h.execute_step(&ScenarioStep::VerifyFile {
            path: "e.txt".into(),
            contains: "BBB".into(),
        })
        .unwrap();
        // Original neighbours must still be present.
        h.execute_step(&ScenarioStep::VerifyFile {
            path: "e.txt".into(),
            contains: "aaa".into(),
        })
        .unwrap();
    }

    #[test]
    fn edit_file_pads_short_files() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::CreateFile {
            path: "short.txt".into(),
            content: "only\n".into(),
        })
        .unwrap();
        // Edit line 5 — well past the end.
        h.execute_step(&ScenarioStep::EditFile {
            path: "short.txt".into(),
            line: 5,
            content: "padded".into(),
        })
        .unwrap();
        h.execute_step(&ScenarioStep::VerifyFile {
            path: "short.txt".into(),
            contains: "padded".into(),
        })
        .unwrap();
    }

    // ---- DeleteFile -------------------------------------------------------

    #[test]
    fn delete_file_removes_it() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::CreateFile {
            path: "del.txt".into(),
            content: "gone".into(),
        })
        .unwrap();
        h.execute_step(&ScenarioStep::DeleteFile {
            path: "del.txt".into(),
        })
        .unwrap();
        assert!(h
            .execute_step(&ScenarioStep::OpenFile {
                path: "del.txt".into(),
            })
            .is_err());
    }

    #[test]
    fn delete_nonexistent_file_is_ok() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.execute_step(&ScenarioStep::DeleteFile {
            path: "ghost.txt".into(),
        })
        .unwrap();
    }

    // ---- RunAgent / WaitFor -----------------------------------------------

    #[test]
    fn run_agent_rejects_empty_prompt() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        let err = h
            .execute_step(&ScenarioStep::RunAgent {
                prompt: "   ".into(),
            })
            .unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn wait_for_rejects_empty_condition() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        let err = h
            .execute_step(&ScenarioStep::WaitFor {
                condition: String::new(),
                timeout_ms: 100,
            })
            .unwrap_err();
        assert!(err.contains("empty"));
    }

    // ---- run_scenario / summary -------------------------------------------

    #[test]
    fn run_scenario_all_pass() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        let scenario = file_lifecycle_scenario();
        let outcome = h.run_scenario(&scenario);
        assert!(outcome.passed, "error: {:?}", outcome.error);
        assert_eq!(outcome.steps.len(), scenario.steps.len());
        assert!(outcome.error.is_none());
    }

    #[test]
    fn run_scenario_stops_on_first_failure() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        let scenario = E2EScenario {
            name: "fail_fast".into(),
            description: String::new(),
            steps: vec![
                ScenarioStep::OpenFile {
                    path: "missing.txt".into(),
                },
                ScenarioStep::CreateFile {
                    path: "never.txt".into(),
                    content: "x".into(),
                },
            ],
        };
        let outcome = h.run_scenario(&scenario);
        assert!(!outcome.passed);
        // Only the first step should have been recorded.
        assert_eq!(outcome.steps.len(), 1);
    }

    #[test]
    fn summary_aggregates_results() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        h.run_scenario(&file_lifecycle_scenario());
        h.run_scenario(&agent_basic_scenario());
        // Inject a failing scenario.
        h.run_scenario(&E2EScenario {
            name: "boom".into(),
            description: String::new(),
            steps: vec![ScenarioStep::OpenFile {
                path: "nope.txt".into(),
            }],
        });

        let s = h.summary();
        assert_eq!(s.total_scenarios, 3);
        assert_eq!(s.passed, 2);
        assert_eq!(s.failed, 1);
        assert!(s.step_count > 0);
    }

    #[test]
    fn summary_empty_harness() {
        let (h, _tmp) = harness();
        let s = h.summary();
        assert_eq!(s.total_scenarios, 0);
        assert_eq!(s.passed, 0);
        assert_eq!(s.failed, 0);
        assert_eq!(s.total_duration_ms, 0);
        assert_eq!(s.step_count, 0);
    }

    // ---- predefined scenarios ---------------------------------------------

    #[test]
    fn multi_file_scenario_runs_cleanly() {
        let (mut h, _tmp) = harness();
        h.setup().unwrap();
        let outcome = h.run_scenario(&multi_file_scenario());
        assert!(outcome.passed, "error: {:?}", outcome.error);
    }

    // ---- cleanup ----------------------------------------------------------

    #[test]
    fn cleanup_removes_workspace() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("cleanme");
        let mut h = TestHarness::new(root.clone());
        h.setup().unwrap();
        assert!(root.exists());
        h.cleanup();
        assert!(!root.exists());
        assert!(!h.setup_complete);
    }

    // ---- step_display_name ------------------------------------------------

    #[test]
    fn step_display_name_covers_variants() {
        let name = TestHarness::step_display_name(&ScenarioStep::CreateFile {
            path: "x".into(),
            content: String::new(),
        });
        assert!(name.contains("CreateFile"));

        let name = TestHarness::step_display_name(&ScenarioStep::WaitFor {
            condition: "ready".into(),
            timeout_ms: 0,
        });
        assert!(name.contains("WaitFor"));
    }
}
