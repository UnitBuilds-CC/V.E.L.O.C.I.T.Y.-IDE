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

    /// T3c: Ingest symbols from an LSP documentSymbol response.
    /// Parses the hierarchical symbol tree and extracts function entries
    /// as TestableFunction items for more accurate test generation.
    pub fn ingest_lsp_symbols(&mut self, file: &Path, symbols_json: &serde_json::Value) {
        let mut functions = Vec::new();
        parse_lsp_symbols_recursive(file, symbols_json, &mut functions);

        // Merge with existing analysis (LSP data takes priority)
        let test_index: HashMap<String, bool> = self.analysis.untested_functions.iter()
            .map(|f| (f.name.clone(), f.has_existing_test))
            .collect();

        for func in &mut functions {
            func.has_existing_test = test_index.contains_key(&func.name);
        }

        let total = functions.len();
        let tested = functions.iter().filter(|f| f.has_existing_test).count();
        let untested: Vec<_> = functions.into_iter()
            .filter(|f| !f.has_existing_test)
            .filter(|f| !self.config.public_only || f.visibility == Visibility::Public)
            .collect();

        self.analysis = CoverageAnalysis {
            total_functions: total,
            tested_functions: tested,
            untested_functions: untested,
            coverage_percent: if total == 0 { 100.0 } else { (tested as f32 / total as f32) * 100.0 },
        };
    }
}

/// Recursively parse LSP documentSymbol response into TestableFunction entries.
/// LSP SymbolKind: 12 = Function, 6 = Method, 9 = Constructor
fn parse_lsp_symbols_recursive(file: &Path, value: &serde_json::Value, out: &mut Vec<TestableFunction>) {
    if let Some(arr) = value.as_array() {
        for sym in arr {
            let kind = sym["kind"].as_u64().unwrap_or(0);
            // Function (12), Method (6), Constructor (9)
            if kind == 12 || kind == 6 || kind == 9 {
                let name = sym["name"].as_str().unwrap_or("").to_string();
                let line = sym["location"]["range"]["start"]["line"].as_u64()
                    .or_else(|| sym["range"]["start"]["line"].as_u64())
                    .unwrap_or(0) as usize;
                let detail = sym["detail"].as_str().unwrap_or("").to_string();
                let visibility = if name.starts_with("pub ") || detail.contains("pub") {
                    Visibility::Public
                } else {
                    Visibility::Private
                };
                if !name.is_empty() {
                    out.push(TestableFunction {
                        name: name.clone(),
                        file: file.to_path_buf(),
                        line,
                        signature: detail,
                        visibility,
                        has_existing_test: false,
                    });
                }
            }
            // Recurse into children
            if let Some(children) = sym.get("children") {
                parse_lsp_symbols_recursive(file, children, out);
            }
        }
    }
}

/// Generate a test skeleton for a single function.
fn generate_test_for_function(func: &TestableFunction, config: &TestGenConfig) -> GeneratedTest {
    let test_name = format!("test_{}", func.name);
    let (setup, assertion, confidence) = analyze_function_signature(func, config.framework);
    let test_body = match config.framework {
        TestFramework::RustBuiltin => {
            if config.include_assertions {
                format!(
                    "#[test]\nfn {}() {{\n{}    let result = {}(\n{}    );\n{}\n}}",
                    test_name, setup, func.name,
                    generate_default_args(&func.signature, TestFramework::RustBuiltin),
                    assertion
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
                "def {}():\n{}    result = {}(\n{}    )\n{}",
                test_name, setup, func.name,
                generate_default_args(&func.signature, TestFramework::Pytest),
                assertion
            )
        }
        TestFramework::Jest => {
            format!(
                "test('{}', () => {{\n{}  const result = {}(\n{}  );\n{}\n}});",
                func.name, setup, func.name,
                generate_default_args(&func.signature, TestFramework::Jest),
                assertion
            )
        }
        TestFramework::Mocha => {
            format!(
                "it('should {}', () => {{\n{}  const result = {}(\n{}  );\n{}\n}});",
                func.name, setup, func.name,
                generate_default_args(&func.signature, TestFramework::Mocha),
                assertion
            )
        }
        TestFramework::JUnit => {
            format!(
                "@Test\npublic void {}() {{\n{}    var result = {}(\n{}    );\n{}\n}}",
                test_name, setup, func.name,
                generate_default_args(&func.signature, TestFramework::JUnit),
                assertion
            )
        }
    };

    GeneratedTest {
        function_name: func.name.clone(),
        test_name,
        test_body,
        target_file: func.file.clone(),
        confidence,
    }
}

/// Analyze a function signature to generate appropriate setup and assertion code.
///
/// The emitted assertion is written in the target framework's language so the
/// generated skeleton is syntactically valid (e.g. Python `assert` for Pytest,
/// `expect(...)` for Jest, `assert.ok(...)` for Mocha, JUnit assertions, and
/// Rust `assert!`/`assert_eq!` for the built-in framework).
fn analyze_function_signature(
    func: &TestableFunction,
    framework: TestFramework,
) -> (String, String, f32) {
    let sig = &func.signature;
    let has_return = sig.contains("->") || sig.contains(": ");
    let returns_bool = sig.contains("bool") || sig.contains("Boolean");
    let returns_option = sig.contains("Option") || sig.contains("?");
    let returns_result = sig.contains("Result") || sig.contains("throws");
    let returns_vec = sig.contains("Vec") || sig.contains("[]") || sig.contains("List");
    let returns_string = sig.contains("String") || sig.contains("str");
    let returns_number = sig.contains("f64") || sig.contains("f32") || sig.contains("i32")
        || sig.contains("u32") || sig.contains("usize") || sig.contains("int") || sig.contains("float");

    let is_bool_predicate = {
        let n = func.name.to_lowercase();
        n.starts_with("is_") || n.starts_with("has_") || n.starts_with("can_") || n.starts_with("should_")
    };

    let kind = if returns_bool {
        ReturnKind::Bool
    } else if returns_option {
        ReturnKind::Option
    } else if returns_result {
        ReturnKind::Result
    } else if returns_vec {
        ReturnKind::Collection
    } else if returns_string {
        ReturnKind::StringLike
    } else if returns_number {
        ReturnKind::Number
    } else if has_return {
        ReturnKind::Other
    } else {
        ReturnKind::Void
    };

    let setup = String::new();
    let assertion = framework_assertion(framework, kind, is_bool_predicate);
    let confidence = if has_return { 0.8 } else { 0.5 };
    (setup, assertion, confidence)
}

/// Classification of a function's return value used to pick an assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnKind {
    Bool,
    Option,
    Result,
    Collection,
    StringLike,
    Number,
    Other,
    Void,
}

/// Produce a language-correct assertion line for the given framework and return kind.
fn framework_assertion(
    framework: TestFramework,
    kind: ReturnKind,
    is_bool_predicate: bool,
) -> String {
    match framework {
        TestFramework::RustBuiltin => match kind {
            ReturnKind::Bool if is_bool_predicate =>
                "    assert!(result, \"Expected {} to return true\");\n".to_string(),
            ReturnKind::Bool =>
                "    // Assert expected boolean result\n    // assert!(result);\n".to_string(),
            ReturnKind::Option =>
                "    assert!(result.is_some(), \"Expected Some value\");\n".to_string(),
            ReturnKind::Result =>
                "    assert!(result.is_ok(), \"Expected Ok result\");\n".to_string(),
            ReturnKind::Collection =>
                "    // Assert collection is non-empty or has expected length\n    // assert!(!result.is_empty());\n".to_string(),
            ReturnKind::StringLike =>
                "    assert!(!result.is_empty(), \"Expected non-empty string\");\n".to_string(),
            ReturnKind::Number =>
                "    // Assert expected numeric result\n    // assert_eq!(result, expected_value);\n".to_string(),
            ReturnKind::Other =>
                "    assert!(result, \"Expected valid result\");\n".to_string(),
            ReturnKind::Void =>
                "    // Function returns void; verify side effects\n    // assert!(condition_after_call);\n".to_string(),
        },
        TestFramework::Pytest => match kind {
            ReturnKind::Bool if is_bool_predicate =>
                "    assert result is True\n".to_string(),
            ReturnKind::Bool =>
                "    assert result in (True, False)\n".to_string(),
            ReturnKind::Option =>
                "    assert result is not None\n".to_string(),
            ReturnKind::Result =>
                "    assert result is not None\n".to_string(),
            ReturnKind::Collection =>
                "    assert len(result) >= 0\n".to_string(),
            ReturnKind::StringLike =>
                "    assert isinstance(result, str)\n".to_string(),
            ReturnKind::Number =>
                "    assert result is not None  # TODO: assert expected numeric value\n".to_string(),
            ReturnKind::Other =>
                "    assert result is not None\n".to_string(),
            ReturnKind::Void =>
                "    assert True  # TODO: verify side effects\n".to_string(),
        },
        TestFramework::Jest => match kind {
            ReturnKind::Bool if is_bool_predicate =>
                "  expect(result).toBe(true);\n".to_string(),
            ReturnKind::Bool =>
                "  expect(typeof result).toBe('boolean');\n".to_string(),
            ReturnKind::Option =>
                "  expect(result).not.toBeNull();\n".to_string(),
            ReturnKind::Result =>
                "  expect(result).toBeDefined();\n".to_string(),
            ReturnKind::Collection =>
                "  expect(Array.isArray(result)).toBe(true);\n".to_string(),
            ReturnKind::StringLike =>
                "  expect(typeof result).toBe('string');\n".to_string(),
            ReturnKind::Number =>
                "  expect(typeof result).toBe('number');\n".to_string(),
            ReturnKind::Other =>
                "  expect(result).toBeDefined();\n".to_string(),
            ReturnKind::Void =>
                "  expect(true).toBe(true); // TODO: verify side effects\n".to_string(),
        },
        TestFramework::Mocha => match kind {
            ReturnKind::Bool if is_bool_predicate =>
                "  assert.strictEqual(result, true);\n".to_string(),
            ReturnKind::Bool =>
                "  assert.strictEqual(typeof result, 'boolean');\n".to_string(),
            ReturnKind::Option =>
                "  assert.notStrictEqual(result, null);\n".to_string(),
            ReturnKind::Result =>
                "  assert.ok(result !== undefined);\n".to_string(),
            ReturnKind::Collection =>
                "  assert.ok(Array.isArray(result));\n".to_string(),
            ReturnKind::StringLike =>
                "  assert.strictEqual(typeof result, 'string');\n".to_string(),
            ReturnKind::Number =>
                "  assert.strictEqual(typeof result, 'number');\n".to_string(),
            ReturnKind::Other =>
                "  assert.ok(result !== undefined);\n".to_string(),
            ReturnKind::Void =>
                "  assert.ok(true); // TODO: verify side effects\n".to_string(),
        },
        TestFramework::JUnit => match kind {
            ReturnKind::Bool if is_bool_predicate =>
                "    assertTrue(result);\n".to_string(),
            ReturnKind::Bool =>
                "    assertNotNull(result);\n".to_string(),
            ReturnKind::Option =>
                "    assertNotNull(result);\n".to_string(),
            ReturnKind::Result =>
                "    assertNotNull(result);\n".to_string(),
            ReturnKind::Collection =>
                "    assertNotNull(result);\n".to_string(),
            ReturnKind::StringLike =>
                "    assertFalse(result.isEmpty());\n".to_string(),
            ReturnKind::Number =>
                "    assertNotNull(result); // TODO: assert expected numeric value\n".to_string(),
            ReturnKind::Other =>
                "    assertNotNull(result);\n".to_string(),
            ReturnKind::Void =>
                "    assertTrue(true); // TODO: verify side effects\n".to_string(),
        },
    }
}


/// Generate default argument expressions for a function call.
fn generate_default_args(signature: &str, framework: TestFramework) -> String {
    // Extract parameter names from signature
    let params = extract_param_names(signature);
    if params.is_empty() {
        return String::new();
    }
    let indent = match framework {
        TestFramework::RustBuiltin | TestFramework::JUnit => "        ",
        TestFramework::Pytest => "        ",
        TestFramework::Jest | TestFramework::Mocha => "        ",
    };
    params.iter()
        .map(|name| format!("{}{}, // TODO: provide test value", indent, name))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Extract parameter names from a function signature.
fn extract_param_names(signature: &str) -> Vec<String> {
    let mut names = Vec::new();
    // Find content between parentheses
    if let Some(start) = signature.find('(') {
        if let Some(end) = signature.find(')') {
            let params_str = &signature[start + 1..end];
            for param in params_str.split(',') {
                let trimmed = param.trim();
                if trimmed.is_empty() || trimmed == "&self" || trimmed == "&mut self" {
                    continue;
                }
                // Rust: `name: Type` -> take `name`
                if let Some(colon_pos) = trimmed.find(':') {
                    let name = trimmed[..colon_pos].trim().trim_start_matches("mut ");
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
                // Python/JS: just the name
                else if trimmed.contains(char::is_alphabetic) {
                    let name = trimmed.split(|c: char| !c.is_alphanumeric() && c != '_')
                        .next()
                        .unwrap_or("");
                    if !name.is_empty() && name != "self" {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    names
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
