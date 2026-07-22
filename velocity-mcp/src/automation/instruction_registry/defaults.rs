use super::types::*;

pub fn default_templates() -> Vec<InstructionTemplate> {
    vec![
        InstructionTemplate {
            id: "planning-architect".to_string(),
            label: "Planning architect".to_string(),
            task_kind: AgentTaskKind::Planning,
            system_prompt: "You are a planning specialist. Break the request into dependency-aware implementation steps, identify risk, and preserve architectural constraints from the surrounding workspace.".to_string(),
            checklist: vec![
                "State the intended outcome clearly.".to_string(),
                "Split work into dependency-ordered tasks.".to_string(),
                "Call out risky files and validation steps.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "refactor-guardian".to_string(),
            label: "Refactor guardian".to_string(),
            task_kind: AgentTaskKind::Refactor,
            system_prompt: "You are a refactoring specialist. Improve structure without changing behavior, keep edits scoped, and respect existing architectural patterns and performance constraints.".to_string(),
            checklist: vec![
                "Preserve observable behavior.".to_string(),
                "Keep interfaces stable unless explicitly authorized.".to_string(),
                "Leave code easier to reason about than before.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "bugfix-responder".to_string(),
            label: "Bugfix responder".to_string(),
            task_kind: AgentTaskKind::BugFix,
            system_prompt: "You are a debugging specialist. Identify the smallest correct fix, explain the root cause, and avoid unrelated changes.".to_string(),
            checklist: vec![
                "Find the root cause before editing.".to_string(),
                "Patch only the required surface area.".to_string(),
                "Validate the fix against the reported failure mode.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "test-hardener".to_string(),
            label: "Test hardener".to_string(),
            task_kind: AgentTaskKind::Test,
            system_prompt: "You are a testing specialist. Strengthen confidence in the target change using the smallest effective validation available in the existing toolchain.".to_string(),
            checklist: vec![
                "Prefer targeted validation first.".to_string(),
                "Cover the modified behavior directly.".to_string(),
                "Avoid introducing flaky or redundant checks.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "analysis-cartographer".to_string(),
            label: "Analysis cartographer".to_string(),
            task_kind: AgentTaskKind::Analysis,
            system_prompt: "You are an analysis specialist. Build a precise map of the relevant code paths, dependencies, and constraints before recommending changes.".to_string(),
            checklist: vec![
                "Trace the real execution path.".to_string(),
                "Document important dependencies.".to_string(),
                "Summarize actionable findings concisely.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "docs-curator".to_string(),
            label: "Docs curator".to_string(),
            task_kind: AgentTaskKind::Documentation,
            system_prompt: "You are a documentation specialist. Explain behavior and architecture accurately, matching the project’s terminology and keeping docs synchronized with code.".to_string(),
            checklist: vec![
                "Match code reality exactly.".to_string(),
                "Prefer concise, high-signal wording.".to_string(),
                "Update only directly related documentation.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "merge-mediator".to_string(),
            label: "Merge mediator".to_string(),
            task_kind: AgentTaskKind::Merge,
            system_prompt: "You are a merge and compatibility specialist. Reconcile concurrent changes, preserve contracts, and produce structurally safe integration steps.".to_string(),
            checklist: vec![
                "List the interfaces at risk.".to_string(),
                "Prefer backward-compatible adapters when possible.".to_string(),
                "State what must be revalidated after integration.".to_string(),
            ],
        },
        InstructionTemplate {
            id: "desktop-wa-operator".to_string(),
            label: "Desktop WA operator".to_string(),
            task_kind: AgentTaskKind::DesktopAutomation,
            system_prompt: "You are a Windows automation specialist. Prefer deterministic WA flows over fuzzy inference, reuse the existing wa_* tool surface, keep scripts narrow, verify focus/value postconditions honestly, and stay NDA-first and modular.".to_string(),
            checklist: vec![
                "Prefer saved WA sessions, snapshots, and scripts over ad-hoc freeform steps.".to_string(),
                "Use truthful wa_capture_windows_snapshot, wa_execute_windows_action, wa_wait_for_windows_condition, and wa_run_script flows when live evidence is required.".to_string(),
                "Record clear validation and remaining runtime limitations instead of implying unsupported behavior works.".to_string(),
            ],
        },
    ]
}

pub fn default_policies() -> Vec<DecompositionPolicy> {
    vec![
        DecompositionPolicy {
            id: "planning-phased".to_string(),
            label: "Planning phased".to_string(),
            task_kind: AgentTaskKind::Planning,
            instruction_template_id: "planning-architect".to_string(),
            decomposition_style: DecompositionStyle::SequentialPipeline,
            shared_expectations: vec![
                "Order tasks by dependency and architectural risk.".to_string(),
                "Prefer producing a narrow set of high-confidence executable steps.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "refactor-coupled".to_string(),
            label: "Refactor coupled".to_string(),
            task_kind: AgentTaskKind::Refactor,
            instruction_template_id: "refactor-guardian".to_string(),
            decomposition_style: DecompositionStyle::CoupledComponents,
            shared_expectations: vec![
                "Group files that share structural coupling or contracts.".to_string(),
                "Minimize cross-agent interface churn during refactors.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "bugfix-focused".to_string(),
            label: "Bugfix focused".to_string(),
            task_kind: AgentTaskKind::BugFix,
            instruction_template_id: "bugfix-responder".to_string(),
            decomposition_style: DecompositionStyle::CoupledComponents,
            shared_expectations: vec![
                "Keep root-cause analysis and the repair path together when files are tightly related.".to_string(),
                "Avoid splitting a bug fix into isolated edits that hide causality.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "test-isolated".to_string(),
            label: "Test isolated".to_string(),
            task_kind: AgentTaskKind::Test,
            instruction_template_id: "test-hardener".to_string(),
            decomposition_style: DecompositionStyle::IsolatedFiles,
            shared_expectations: vec![
                "Prefer targeted validation bundles per touched surface.".to_string(),
                "Keep test additions close to the behavior they cover.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "analysis-coupled".to_string(),
            label: "Analysis coupled".to_string(),
            task_kind: AgentTaskKind::Analysis,
            instruction_template_id: "analysis-cartographer".to_string(),
            decomposition_style: DecompositionStyle::CoupledComponents,
            shared_expectations: vec![
                "Trace connected execution paths together.".to_string(),
                "Preserve enough context for downstream planning or fixes.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "docs-isolated".to_string(),
            label: "Docs isolated".to_string(),
            task_kind: AgentTaskKind::Documentation,
            instruction_template_id: "docs-curator".to_string(),
            decomposition_style: DecompositionStyle::IsolatedFiles,
            shared_expectations: vec![
                "Prefer localized documentation edits unless broader concepts are shared.".to_string(),
                "Keep terminology synchronized with the relevant code surface.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "merge-phased".to_string(),
            label: "Merge phased".to_string(),
            task_kind: AgentTaskKind::Merge,
            instruction_template_id: "merge-mediator".to_string(),
            decomposition_style: DecompositionStyle::SequentialPipeline,
            shared_expectations: vec![
                "Sequence compatibility work before final integration.".to_string(),
                "Prefer adapter steps when direct reconciliation would collide.".to_string(),
            ],
        },
        DecompositionPolicy {
            id: "desktop-wa-phased".to_string(),
            label: "Desktop WA phased".to_string(),
            task_kind: AgentTaskKind::DesktopAutomation,
            instruction_template_id: "desktop-wa-operator".to_string(),
            decomposition_style: DecompositionStyle::SequentialPipeline,
            shared_expectations: vec![
                "Prefer narrow deterministic Windows automation slices over broad speculative rewrites.".to_string(),
                "Keep execution grounded in the existing WA registry tools, persisted NDA artifacts, and explicit postcondition verification.".to_string(),
            ],
        },
    ]
}
