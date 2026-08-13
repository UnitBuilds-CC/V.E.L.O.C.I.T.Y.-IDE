//! Pre-built workflow templates for common automation patterns.
//!
//! Templates provide one-click creation of frequently-used workflows,
//! lowering the barrier to entry and demonstrating best practices.
//! Users can customize templates after creation.

use super::workflow_canvas::{CanvasNodeKind, NodePosition, WorkflowCanvas};

/// A named template that can produce a [`WorkflowCanvas`].
pub struct WorkflowTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: TemplateCategory,
    builder: fn(&str, &str) -> WorkflowCanvas,
}

/// Categories for organizing templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCategory {
    CodeQuality,
    Automation,
    Review,
    Deployment,
    Research,
    Testing,
}

impl TemplateCategory {
    pub fn label(&self) -> &str {
        match self {
            Self::CodeQuality => "Code Quality",
            Self::Automation => "Automation",
            Self::Review => "Review",
            Self::Deployment => "Deployment",
            Self::Research => "Research",
            Self::Testing => "Testing",
        }
    }
}

impl WorkflowTemplate {
    /// Build the template into a canvas with the given id and name.
    pub fn build(&self, id: &str, name: &str) -> WorkflowCanvas {
        (self.builder)(id, name)
    }
}

/// All available workflow templates.
pub fn all_templates() -> Vec<WorkflowTemplate> {
    vec![
        WorkflowTemplate {
            id: "code-review-pipeline",
            name: "Code Review Pipeline",
            description: "Run compilation check, lint, then summarize changes",
            category: TemplateCategory::CodeQuality,
            builder: code_review_pipeline,
        },
        WorkflowTemplate {
            id: "test-and-report",
            name: "Test & Report",
            description: "Run tests, check coverage, generate summary",
            category: TemplateCategory::Testing,
            builder: test_and_report,
        },
        WorkflowTemplate {
            id: "refactor-safely",
            name: "Safe Refactor",
            description: "Analyze code, apply refactor, validate with tests",
            category: TemplateCategory::CodeQuality,
            builder: refactor_safely,
        },
        WorkflowTemplate {
            id: "research-and-document",
            name: "Research & Document",
            description: "Browse web for topic, summarize findings, write docs",
            category: TemplateCategory::Research,
            builder: research_and_document,
        },
        WorkflowTemplate {
            id: "build-deploy-check",
            name: "Build, Deploy & Verify",
            description: "Build project, deploy, run smoke tests",
            category: TemplateCategory::Deployment,
            builder: build_deploy_check,
        },
        WorkflowTemplate {
            id: "bug-investigation",
            name: "Bug Investigation",
            description: "Read logs, analyze error, propose fix, validate",
            category: TemplateCategory::Review,
            builder: bug_investigation,
        },
        WorkflowTemplate {
            id: "feature-implementation",
            name: "Feature Implementation",
            description: "Plan feature, implement, test, document",
            category: TemplateCategory::Automation,
            builder: feature_implementation,
        },
        WorkflowTemplate {
            id: "dependency-audit",
            name: "Dependency Audit",
            description: "Check for outdated deps, analyze breaking changes, update safely",
            category: TemplateCategory::CodeQuality,
            builder: dependency_audit,
        },
    ]
}

// ── Template Builders ─────────────────────────────────────────────────────

fn code_review_pipeline(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    // Position Start and End
    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition { x: 900.0, y: 200.0 };
    }

    let compile = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo check --all-targets 2>&1"}),
        },
        NodePosition { x: 280.0, y: 200.0 },
    );

    let lint = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo clippy --all-targets 2>&1"}),
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let summarize = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Summarize the code changes in this workspace. List modified files, key changes, and any potential issues.".into(),
            team: None,
        },
        NodePosition { x: 680.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", compile.clone());
    canvas.add_edge(compile, "ok", lint.clone());
    canvas.add_edge(lint, "ok", summarize.clone());
    canvas.add_edge(summarize, "ok", end_id);
    canvas
}

fn test_and_report(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition { x: 900.0, y: 200.0 };
    }

    let test = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo test --all-targets 2>&1"}),
        },
        NodePosition { x: 280.0, y: 200.0 },
    );

    let condition = canvas.add_node(
        CanvasNodeKind::Condition {
            description: "Tests passed?".into(),
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let report = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Generate a test report summarizing the results: how many passed, failed, and any patterns in failures.".into(),
            team: None,
        },
        NodePosition { x: 680.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", test.clone());
    canvas.add_edge(test, "ok", condition.clone());
    canvas.add_edge(condition, "ok", report.clone());
    canvas.add_edge(report, "ok", end_id);
    canvas
}

fn refactor_safely(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition {
            x: 1100.0,
            y: 200.0,
        };
    }

    let analyze = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Analyze the codebase structure and identify refactoring opportunities. Focus on code duplication, complex functions, and naming improvements.".into(),
            team: None,
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let refactor = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Apply the identified refactoring changes. Make small, focused changes that improve code quality without changing behavior.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let validate = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo test --all-targets 2>&1"}),
        },
        NodePosition { x: 710.0, y: 200.0 },
    );

    let condition = canvas.add_node(
        CanvasNodeKind::Condition {
            description: "Tests still pass?".into(),
        },
        NodePosition { x: 900.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", analyze.clone());
    canvas.add_edge(analyze, "ok", refactor.clone());
    canvas.add_edge(refactor, "ok", validate.clone());
    canvas.add_edge(validate, "ok", condition.clone());
    canvas.add_edge(condition, "ok", end_id);
    canvas
}

fn research_and_document(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition { x: 900.0, y: 200.0 };
    }

    let research = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Research the topic using web browsing. Find authoritative sources, recent developments, and key concepts.".into(),
            team: None,
        },
        NodePosition { x: 280.0, y: 200.0 },
    );

    let summarize = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Summarize the research findings into key points, organized by theme. Include source references.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let write_docs = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Write comprehensive documentation based on the research summary. Include examples and cross-references.".into(),
            team: None,
        },
        NodePosition { x: 680.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", research.clone());
    canvas.add_edge(research, "ok", summarize.clone());
    canvas.add_edge(summarize, "ok", write_docs.clone());
    canvas.add_edge(write_docs, "ok", end_id);
    canvas
}

fn build_deploy_check(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition {
            x: 1100.0,
            y: 200.0,
        };
    }

    let build = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo build --release 2>&1"}),
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let condition1 = canvas.add_node(
        CanvasNodeKind::Condition {
            description: "Build succeeded?".into(),
        },
        NodePosition { x: 450.0, y: 200.0 },
    );

    let deploy = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "echo 'Deploy step \u{2014} configure for your environment'"}),
        },
        NodePosition { x: 650.0, y: 200.0 },
    );

    let smoke = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Run smoke tests to verify the deployment is healthy. Check key endpoints and functionality.".into(),
            team: None,
        },
        NodePosition { x: 870.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", build.clone());
    canvas.add_edge(build, "ok", condition1.clone());
    canvas.add_edge(condition1, "ok", deploy.clone());
    canvas.add_edge(deploy, "ok", smoke.clone());
    canvas.add_edge(smoke, "ok", end_id);
    canvas
}

fn bug_investigation(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition {
            x: 1100.0,
            y: 200.0,
        };
    }

    let read_logs = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Read recent log files and error outputs. Identify the error pattern and affected components.".into(),
            team: None,
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let analyze = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Analyze the error in context of the codebase. Trace the execution path and identify the root cause.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let fix = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Propose and apply a fix for the identified root cause. Include error handling improvements.".into(),
            team: None,
        },
        NodePosition { x: 710.0, y: 200.0 },
    );

    let validate = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo test --all-targets 2>&1"}),
        },
        NodePosition { x: 900.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", read_logs.clone());
    canvas.add_edge(read_logs, "ok", analyze.clone());
    canvas.add_edge(analyze, "ok", fix.clone());
    canvas.add_edge(fix, "ok", validate.clone());
    canvas.add_edge(validate, "ok", end_id);
    canvas
}

fn feature_implementation(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition {
            x: 1300.0,
            y: 200.0,
        };
    }

    let plan = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Plan the feature implementation. Break it into small steps, identify affected files, and estimate complexity.".into(),
            team: None,
        },
        NodePosition { x: 230.0, y: 200.0 },
    );

    let implement = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Implement the feature according to the plan. Write clean, well-documented code following project conventions.".into(),
            team: None,
        },
        NodePosition { x: 460.0, y: 200.0 },
    );

    let test = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Write tests for the new feature. Cover happy paths, edge cases, and error conditions.".into(),
            team: None,
        },
        NodePosition { x: 690.0, y: 200.0 },
    );

    let validate = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo test --all-targets 2>&1"}),
        },
        NodePosition { x: 920.0, y: 200.0 },
    );

    let document = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Document the new feature. Update relevant README sections, add inline documentation, and note any API changes.".into(),
            team: None,
        },
        NodePosition { x: 1100.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", plan.clone());
    canvas.add_edge(plan, "ok", implement.clone());
    canvas.add_edge(implement, "ok", test.clone());
    canvas.add_edge(test, "ok", validate.clone());
    canvas.add_edge(validate, "ok", document.clone());
    canvas.add_edge(document, "ok", end_id);
    canvas
}

fn dependency_audit(id: &str, name: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition { x: 50.0, y: 200.0 };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition {
            x: 1100.0,
            y: 200.0,
        };
    }

    let check = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo outdated 2>&1 || echo 'cargo-outdated not installed'"}),
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let analyze = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Analyze the dependency list. Identify outdated packages, potential security issues, and breaking changes in newer versions.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let update = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Suggest safe dependency updates. Prioritize security patches, then minor version bumps. Flag any breaking changes.".into(),
            team: None,
        },
        NodePosition { x: 710.0, y: 200.0 },
    );

    let validate = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo check --all-targets 2>&1"}),
        },
        NodePosition { x: 900.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", check.clone());
    canvas.add_edge(check, "ok", analyze.clone());
    canvas.add_edge(analyze, "ok", update.clone());
    canvas.add_edge(update, "ok", validate.clone());
    canvas.add_edge(validate, "ok", end_id);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_templates_produce_valid_canvases() {
        for template in all_templates() {
            let canvas = template.build(&format!("test_{}", template.id), template.name);
            assert!(
                canvas.nodes.len() >= 2,
                "Template {} has too few nodes",
                template.id
            );
            assert!(
                canvas.execution_order().is_some(),
                "Template {} has a cycle",
                template.id
            );
            let wf = canvas.to_workflow();
            assert!(wf.is_some(), "Template {} failed to convert", template.id);
        }
    }

    #[test]
    fn templates_cover_all_categories() {
        let templates = all_templates();
        let categories: std::collections::HashSet<_> =
            templates.iter().map(|t| t.category).collect();
        assert!(categories.len() >= 5, "Should cover at least 5 categories");
    }
}
