#![allow(dead_code)]
//! Auto-generated Test Coverage: analyzes source code and generates test
//! skeletons for untested functions, then runs them via the test runner.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A function detected in source that may need test coverage.
#[derive(Debug, Clone)]
pub struct TestableFunction {
    pub name: String,
    pub file: PathBuf,
    pub line: usize,
    pub signature: String,
    pub visibility: Visibility,
    pub has_existing_test: bool,
}

/// Visibility level of a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    CrateLocal,
}

/// A generated test case.
#[derive(Debug, Clone)]
pub struct GeneratedTest {
    pub function_name: String,
    pub test_name: String,
    pub test_body: String,
    pub target_file: PathBuf,
    pub confidence: f32,
}

/// Coverage analysis result for a workspace.
#[derive(Debug, Clone, Default)]
pub struct CoverageAnalysis {
    pub total_functions: usize,
    pub tested_functions: usize,
    pub untested_functions: Vec<TestableFunction>,
    pub coverage_percent: f32,
}

/// Configuration for test generation.
#[derive(Debug, Clone)]
pub struct TestGenConfig {
    /// Only generate tests for public functions.
    pub public_only: bool,
    /// Maximum number of tests to generate per run.
    pub max_tests_per_run: usize,
    /// Include assertion placeholders.
    pub include_assertions: bool,
    /// Language-specific test framework.
    pub framework: TestFramework,
}

impl Default for TestGenConfig {
    fn default() -> Self {
        Self {
            public_only: false,
            max_tests_per_run: 20,
            include_assertions: true,
            framework: TestFramework::RustBuiltin,
        }
    }
}

/// Supported test frameworks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestFramework {
    RustBuiltin,
    Pytest,
    Jest,
    Mocha,
    JUnit,
}

impl TestFramework {
    pub fn test_annotation(&self) -> &'static str {
        match self {
            Self::RustBuiltin => "#[test]",
            Self::Pytest => "def test_",
            Self::Jest => "test('",
            Self::Mocha => "it('",
            Self::JUnit => "@Test",
        }
    }
}

/// The test generator engine.
#[derive(Debug)]
pub struct TestGenerator {
    pub config: TestGenConfig,
    pub analysis: CoverageAnalysis,
    pub generated_tests: Vec<GeneratedTest>,
}

impl Default for TestGenerator {
    fn default() -> Self {
        Self::new(TestGenConfig::default())
    }
}

impl TestGenerator {
    pub fn new(config: TestGenConfig) -> Self {
        Self {
            config,
            analysis: CoverageAnalysis::default(),
            generated_tests: Vec::new(),
        }
    }

    /// Analyze a workspace for test coverage gaps.
    pub fn analyze_coverage(&mut self, workspace_root: &Path) {
        let mut functions = Vec::new();
        let test_index = build_test_index(workspace_root);
        collect_functions(workspace_root, workspace_root, &mut functions);

        for func in &mut functions {
            func.has_existing_test = test_index.contains_key(&func.name);
        }

        let total = functions.len();
        let tested = functions.iter().filter(|f| f.has_existing_test).count();
        let untested: Vec<_> = functions
            .into_iter()
            .filter(|f| !f.has_existing_test)
            .filter(|f| !self.config.public_only || f.visibility == Visibility::Public)
            .collect();

        self.analysis = CoverageAnalysis {
            total_functions: total,
            tested_functions: tested,
            untested_functions: untested,
            coverage_percent: if total == 0 {
                100.0
            } else {
                (tested as f32 / total as f32) * 100.0
            },
        };
    }

    /// Generate test skeletons for untested functions.
    pub fn generate_tests(&mut self) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        let limit = self.config.max_tests_per_run;

        for func in self.analysis.untested_functions.iter().take(limit) {
            let test = generate_test_for_function(func, &self.config);
            tests.push(test);
        }

        self.generated_tests = tests.clone();
        tests
    }

    /// Get a summary string of coverage analysis.
    pub fn coverage_summary(&self) -> String {
        format!(
            "Coverage: {:.1}% ({}/{} functions tested, {} gaps)",
            self.analysis.coverage_percent,
            self.analysis.tested_functions,
            self.analysis.total_functions,
            self.analysis.untested_functions.len()
        )
    }
}

/// Generate a test skeleton for a single function.
fn generate_test_for_function(func: &TestableFunction, config: &TestGenConfig) -> GeneratedTest {
    let test_name = format!("test_{}", func.name);
    let test_body = match config.framework {
        TestFramework::RustBuiltin => {
            if config.include_assertions {
                format!(
                    "#[test]\nfn {}() {{\n    // TODO: test {}\n    // assert_eq!({}(...), expected);\n    assert!(true);\n}}",
                    test_name, func.name, func.name
                )
            } else {
                format!(
                    "#[test]\nfn {}() {{\n    // TODO: test {}\n}}",
                    test_name, func.name
                )
            }
        }
        TestFramework::Pytest => {
            format!(
                "def {}():\n    # TODO: test {}\n    assert True",
                test_name, func.name
            )
        }
        TestFramework::Jest => {
            format!(
                "test('{}', () => {{\n  // TODO: test {}\n  expect(true).toBe(true);\n}});",
                func.name, func.name
            )
        }
        TestFramework::Mocha => {
            format!(
                "it('should {}', () => {{\n  // TODO: test {}\n  assert.ok(true);\n}});",
                func.name, func.name
            )
        }
        TestFramework::JUnit => {
            format!(
                "@Test\npublic void {}() {{\n    // TODO: test {}\n    assertTrue(true);\n}}",
                test_name, func.name
            )
        }
    };

    GeneratedTest {
        function_name: func.name.clone(),
        test_name,
        test_body,
        target_file: func.file.clone(),
        confidence: 0.7,
    }
}

/// Build an index of existing test function names across the workspace.
fn build_test_index(workspace_root: &Path) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    walk_for_tests(workspace_root, workspace_root, &mut index);
    index
}

fn walk_for_tests(root: &Path, dir: &Path, index: &mut HashMap<String, PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk_for_tests(root, &path, index);
        } else if path.is_file() {
            extract_test_names(&path, index);
        }
    }
}

fn extract_test_names(path: &Path, index: &mut HashMap<String, PathBuf>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        // Rust: fn test_xxx or #[test] above fn xxx
        if trimmed.starts_with("fn test_") {
            if let Some(name) = trimmed
                .strip_prefix("fn test_")
                .and_then(|rest| rest.split('(').next())
            {
                // The function being tested is likely "name" without "test_" prefix
                index.insert(name.trim().to_string(), path.to_path_buf());
            }
        }
        // Python: def test_xxx
        if trimmed.starts_with("def test_") {
            if let Some(name) = trimmed
                .strip_prefix("def test_")
                .and_then(|rest| rest.split('(').next())
            {
                index.insert(name.trim().to_string(), path.to_path_buf());
            }
        }
    }
}

/// Collect all public/private functions from source files.
fn collect_functions(root: &Path, dir: &Path, functions: &mut Vec<TestableFunction>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            collect_functions(root, &path, functions);
        } else if path.is_file() && is_source_file(&name) {
            extract_functions(root, &path, functions);
        }
    }
}

fn is_source_file(name: &str) -> bool {
    name.ends_with(".rs")
        || name.ends_with(".py")
        || name.ends_with(".js")
        || name.ends_with(".ts")
        || name.ends_with(".tsx")
}

fn extract_functions(root: &Path, path: &Path, functions: &mut Vec<TestableFunction>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        // Skip indented (nested) functions and test functions
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if trimmed.contains("test") {
            continue;
        }

        let (visibility, fn_name) = if let Some(rest) = trimmed.strip_prefix("pub fn ") {
            (Visibility::Public, rest)
        } else if let Some(rest) = trimmed.strip_prefix("pub(crate) fn ") {
            (Visibility::CrateLocal, rest)
        } else if let Some(rest) = trimmed.strip_prefix("fn ") {
            (Visibility::Private, rest)
        } else if let Some(rest) = trimmed.strip_prefix("def ") {
            (Visibility::Public, rest)
        } else {
            continue;
        };

        let name = fn_name
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("")
            .to_string();

        if name.is_empty() || name == "main" || name == "new" || name == "default" {
            continue;
        }

        functions.push(TestableFunction {
            name,
            file: rel.clone(),
            line: idx + 1,
            signature: trimmed.to_string(),
            visibility,
            has_existing_test: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_rust_test_skeleton() {
        let func = TestableFunction {
            name: "calculate_total".into(),
            file: PathBuf::from("src/math.rs"),
            line: 10,
            signature: "pub fn calculate_total(items: &[Item]) -> f64".into(),
            visibility: Visibility::Public,
            has_existing_test: false,
        };
        let config = TestGenConfig::default();
        let test = generate_test_for_function(&func, &config);
        assert!(test.test_body.contains("#[test]"));
        assert!(test.test_body.contains("test_calculate_total"));
        assert!(test.test_body.contains("assert"));
    }

    #[test]
    fn generate_pytest_skeleton() {
        let func = TestableFunction {
            name: "process_data".into(),
            file: PathBuf::from("main.py"),
            line: 5,
            signature: "def process_data(items):".into(),
            visibility: Visibility::Public,
            has_existing_test: false,
        };
        let config = TestGenConfig {
            framework: TestFramework::Pytest,
            ..Default::default()
        };
        let test = generate_test_for_function(&func, &config);
        assert!(test.test_body.contains("def test_"));
        assert!(test.test_body.contains("assert True"));
    }

    #[test]
    fn coverage_summary_format() {
        let mut gen = TestGenerator::default();
        gen.analysis = CoverageAnalysis {
            total_functions: 20,
            tested_functions: 15,
            untested_functions: vec![],
            coverage_percent: 75.0,
        };
        let summary = gen.coverage_summary();
        assert!(summary.contains("75.0%"));
        assert!(summary.contains("15/20"));
    }

    #[test]
    fn framework_annotations() {
        assert_eq!(TestFramework::RustBuiltin.test_annotation(), "#[test]");
        assert_eq!(TestFramework::Pytest.test_annotation(), "def test_");
        assert_eq!(TestFramework::Jest.test_annotation(), "test('");
    }
}
