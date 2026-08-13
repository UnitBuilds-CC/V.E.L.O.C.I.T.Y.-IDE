//! AI-assisted workflow generation from natural language.
//!
//! Converts user descriptions into workflow canvas structures by parsing
//! intent keywords and mapping them to step patterns. This provides a
//! lightweight local implementation that doesn't require an LLM call for
//! basic patterns, while also supporting an LLM-powered path for complex
//! descriptions.

use super::workflow_canvas::{CanvasNodeKind, NodePosition, WorkflowCanvas};

/// Result of parsing a natural language description.
#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub canvas: WorkflowCanvas,
    pub confidence: f32,
    pub explanation: String,
}

/// Generate a workflow canvas from a natural language description.
///
/// This uses keyword/pattern matching for common automation intents.
/// For complex descriptions, it falls back to a structured template.
pub fn generate_from_description(id: &str, name: &str, description: &str) -> GenerationResult {
    let lower = description.to_lowercase();

    // Pattern: "review" / "lint" / "check" → code review pipeline
    if contains_any(&lower, &["review", "lint", "check code", "code quality"]) {
        return GenerationResult {
            canvas: build_review_pipeline(id, name, description),
            confidence: 0.85,
            explanation: "Detected code review intent \u{2014} built check\u{2192}lint\u{2192}summarize pipeline"
                .into(),
        };
    }

    // Pattern: "test" / "run tests" → test pipeline
    if contains_any(&lower, &["test", "run test", "testing", "validate"]) {
        return GenerationResult {
            canvas: build_test_pipeline(id, name, description),
            confidence: 0.85,
            explanation: "Detected testing intent \u{2014} built test\u{2192}condition\u{2192}report pipeline"
                .into(),
        };
    }

    // Pattern: "deploy" / "release" / "publish" → deploy pipeline
    if contains_any(&lower, &["deploy", "release", "publish", "ship"]) {
        return GenerationResult {
            canvas: build_deploy_pipeline(id, name, description),
            confidence: 0.80,
            explanation:
                "Detected deployment intent \u{2014} built build\u{2192}check\u{2192}deploy\u{2192}verify pipeline"
                    .into(),
        };
    }

    // Pattern: "fix" / "debug" / "investigate" → bug investigation
    if contains_any(&lower, &["fix", "debug", "investigate", "bug", "error"]) {
        return GenerationResult {
            canvas: build_debug_pipeline(id, name, description),
            confidence: 0.80,
            explanation:
                "Detected debugging intent \u{2014} built log\u{2192}analyze\u{2192}fix\u{2192}validate pipeline".into(),
        };
    }

    // Pattern: "document" / "write docs" / "readme" → documentation
    if contains_any(
        &lower,
        &["document", "docs", "readme", "write doc", "documentation"],
    ) {
        return GenerationResult {
            canvas: build_docs_pipeline(id, name, description),
            confidence: 0.75,
            explanation:
                "Detected documentation intent \u{2014} built research\u{2192}summarize\u{2192}write pipeline"
                    .into(),
        };
    }

    // Pattern: "refactor" → safe refactor pipeline
    if contains_any(
        &lower,
        &["refactor", "clean up", "improve code", "restructure"],
    ) {
        return GenerationResult {
            canvas: build_refactor_pipeline(id, name, description),
            confidence: 0.80,
            explanation:
                "Detected refactoring intent \u{2014} built analyze\u{2192}refactor\u{2192}test\u{2192}validate pipeline"
                    .into(),
        };
    }

    // Pattern: "feature" / "implement" / "build" / "create" → feature implementation
    if contains_any(
        &lower,
        &["feature", "implement", "build", "create", "add", "new"],
    ) {
        return GenerationResult {
            canvas: build_feature_pipeline(id, name, description),
            confidence: 0.70,
            explanation: "Detected feature implementation intent \u{2014} built plan\u{2192}implement\u{2192}test\u{2192}document pipeline".into(),
        };
    }

    // Fallback: generic 3-step pipeline
    GenerationResult {
        canvas: build_generic_pipeline(id, name, description),
        confidence: 0.50,
        explanation:
            "No specific pattern detected \u{2014} built generic analyze\u{2192}act\u{2192}verify pipeline".into(),
    }
}

/// Check if the text contains any of the given keywords.
fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

// ── Pipeline Builders ─────────────────────────────────────────────────────

fn build_review_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 900.0);

    let check = canvas.add_node(
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
            prompt: format!("Review the code changes in this workspace. {desc} Summarize findings and potential issues."),
            team: None,
        },
        NodePosition { x: 680.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", check.clone());
    canvas.add_edge(check, "ok", lint.clone());
    canvas.add_edge(lint, "ok", summarize.clone());
    canvas.add_edge(summarize, "ok", end_id);
    canvas
}

fn build_test_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 900.0);

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
            prompt: format!("Generate a test report. {desc} Summarize pass/fail counts and any failure patterns."),
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

fn build_deploy_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 1100.0);

    let build = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo build --release 2>&1"}),
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let check = canvas.add_node(
        CanvasNodeKind::Condition {
            description: "Build OK?".into(),
        },
        NodePosition { x: 450.0, y: 200.0 },
    );

    let deploy = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!(
                "Deploy the application. {desc} Run deployment steps and verify success."
            ),
            team: None,
        },
        NodePosition { x: 650.0, y: 200.0 },
    );

    let verify = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Run smoke tests to verify the deployment is healthy.".into(),
            team: None,
        },
        NodePosition { x: 870.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", build.clone());
    canvas.add_edge(build, "ok", check.clone());
    canvas.add_edge(check, "ok", deploy.clone());
    canvas.add_edge(deploy, "ok", verify.clone());
    canvas.add_edge(verify, "ok", end_id);
    canvas
}

fn build_debug_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 1100.0);

    let logs = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!(
                "Read recent logs and error outputs. {desc} Identify the error pattern."
            ),
            team: None,
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let analyze = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Analyze the error in context of the codebase. Trace the root cause.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let fix = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Propose and apply a fix for the root cause.".into(),
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

    canvas.add_edge(start_id, "ok", logs.clone());
    canvas.add_edge(logs, "ok", analyze.clone());
    canvas.add_edge(analyze, "ok", fix.clone());
    canvas.add_edge(fix, "ok", validate.clone());
    canvas.add_edge(validate, "ok", end_id);
    canvas
}

fn build_docs_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 900.0);

    let research = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!(
                "Research the topic: {desc}. Find authoritative sources and key concepts."
            ),
            team: None,
        },
        NodePosition { x: 280.0, y: 200.0 },
    );

    let summarize = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Summarize findings into organized key points with source references.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let write = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Write comprehensive documentation with examples and cross-references.".into(),
            team: None,
        },
        NodePosition { x: 680.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", research.clone());
    canvas.add_edge(research, "ok", summarize.clone());
    canvas.add_edge(summarize, "ok", write.clone());
    canvas.add_edge(write, "ok", end_id);
    canvas
}

fn build_refactor_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 1100.0);

    let analyze = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!("Analyze the codebase for refactoring. {desc} Identify improvements."),
            team: None,
        },
        NodePosition { x: 250.0, y: 200.0 },
    );

    let refactor = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Apply the refactoring changes while preserving behavior.".into(),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let test = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo test --all-targets 2>&1"}),
        },
        NodePosition { x: 710.0, y: 200.0 },
    );

    let condition = canvas.add_node(
        CanvasNodeKind::Condition {
            description: "Tests pass?".into(),
        },
        NodePosition { x: 900.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", analyze.clone());
    canvas.add_edge(analyze, "ok", refactor.clone());
    canvas.add_edge(refactor, "ok", test.clone());
    canvas.add_edge(test, "ok", condition.clone());
    canvas.add_edge(condition, "ok", end_id);
    canvas
}

fn build_feature_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 1300.0);

    let plan = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!(
                "Plan this feature: {desc}. Break into steps and identify affected files."
            ),
            team: None,
        },
        NodePosition { x: 230.0, y: 200.0 },
    );

    let implement = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Implement the feature according to the plan.".into(),
            team: None,
        },
        NodePosition { x: 460.0, y: 200.0 },
    );

    let test = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: "Write tests covering happy paths, edge cases, and error conditions.".into(),
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
            prompt: "Document the new feature. Update README and add inline docs.".into(),
            team: None,
        },
        NodePosition {
            x: 1100.0,
            y: 200.0,
        },
    );

    canvas.add_edge(start_id, "ok", plan.clone());
    canvas.add_edge(plan, "ok", implement.clone());
    canvas.add_edge(implement, "ok", test.clone());
    canvas.add_edge(test, "ok", validate.clone());
    canvas.add_edge(validate, "ok", document.clone());
    canvas.add_edge(document, "ok", end_id);
    canvas
}

fn build_generic_pipeline(id: &str, name: &str, desc: &str) -> WorkflowCanvas {
    let mut canvas = WorkflowCanvas::new(id, name);
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();

    position_endpoints(&mut canvas, 50.0, 900.0);

    let analyze = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!("Analyze the task: {desc}. Understand requirements and current state."),
            team: None,
        },
        NodePosition { x: 280.0, y: 200.0 },
    );

    let execute = canvas.add_node(
        CanvasNodeKind::AgentTask {
            prompt: format!("Execute the task: {desc}. Make the necessary changes."),
            team: None,
        },
        NodePosition { x: 480.0, y: 200.0 },
    );

    let verify = canvas.add_node(
        CanvasNodeKind::Tool {
            name: "run_command".into(),
            args: serde_json::json!({"command": "cargo check --all-targets 2>&1"}),
        },
        NodePosition { x: 680.0, y: 200.0 },
    );

    canvas.add_edge(start_id, "ok", analyze.clone());
    canvas.add_edge(analyze, "ok", execute.clone());
    canvas.add_edge(execute, "ok", verify.clone());
    canvas.add_edge(verify, "ok", end_id);
    canvas
}

/// Position start and end nodes at the given x coordinates.
fn position_endpoints(canvas: &mut WorkflowCanvas, start_x: f32, end_x: f32) {
    let start_id = canvas.nodes[0].id.clone();
    let end_id = canvas.nodes[1].id.clone();
    if let Some(n) = canvas.node_mut(&start_id) {
        n.position = NodePosition {
            x: start_x,
            y: 200.0,
        };
    }
    if let Some(n) = canvas.node_mut(&end_id) {
        n.position = NodePosition { x: end_x, y: 200.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_pattern_detected() {
        let result = generate_from_description("wf1", "Test", "Review the code quality");
        assert!(result.confidence > 0.7);
        assert!(result.canvas.nodes.len() > 2);
        assert!(result.canvas.execution_order().is_some());
    }

    #[test]
    fn test_pattern_detected() {
        let result = generate_from_description("wf2", "Test", "Run all tests and report");
        assert!(result.confidence > 0.7);
        assert!(result.canvas.nodes.len() > 2);
    }

    #[test]
    fn deploy_pattern_detected() {
        let result =
            generate_from_description("wf3", "Test", "Deploy the application to production");
        assert!(result.confidence > 0.7);
    }

    #[test]
    fn debug_pattern_detected() {
        let result = generate_from_description("wf4", "Test", "Fix the bug in the login flow");
        assert!(result.confidence > 0.7);
    }

    #[test]
    fn generic_fallback_for_unknown() {
        let result = generate_from_description("wf5", "Test", "xyzzy foobar");
        assert_eq!(result.confidence, 0.50);
        assert!(result.canvas.execution_order().is_some());
    }

    #[test]
    fn all_generated_canvases_convert_to_workflow() {
        let descriptions = [
            "review the code",
            "run tests",
            "deploy to prod",
            "fix the bug",
            "write docs",
            "refactor this",
            "implement feature X",
            "something unknown",
        ];
        for (i, desc) in descriptions.iter().enumerate() {
            let result = generate_from_description(&format!("wf{i}"), "Test", desc);
            let wf = result.canvas.to_workflow();
            assert!(wf.is_some(), "Failed for: {desc}");
            assert!(!wf.unwrap().steps.is_empty(), "Empty workflow for: {desc}");
        }
    }
}
