use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskKind {
    Refactor,
    BugFix,
    Test,
    Documentation,
    Analysis,
    Planning,
    Merge,
    DesktopAutomation,
}

impl AgentTaskKind {
    pub const ALL: [AgentTaskKind; 8] = [
        AgentTaskKind::Refactor,
        AgentTaskKind::BugFix,
        AgentTaskKind::Test,
        AgentTaskKind::Documentation,
        AgentTaskKind::Analysis,
        AgentTaskKind::Planning,
        AgentTaskKind::Merge,
        AgentTaskKind::DesktopAutomation,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentTaskKind::Refactor => "refactor",
            AgentTaskKind::BugFix => "bug_fix",
            AgentTaskKind::Test => "test",
            AgentTaskKind::Documentation => "documentation",
            AgentTaskKind::Analysis => "analysis",
            AgentTaskKind::Planning => "planning",
            AgentTaskKind::Merge => "merge",
            AgentTaskKind::DesktopAutomation => "desktop_automation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "refactor" => Some(AgentTaskKind::Refactor),
            "bug_fix" => Some(AgentTaskKind::BugFix),
            "test" => Some(AgentTaskKind::Test),
            "documentation" => Some(AgentTaskKind::Documentation),
            "analysis" => Some(AgentTaskKind::Analysis),
            "planning" => Some(AgentTaskKind::Planning),
            "merge" => Some(AgentTaskKind::Merge),
            "desktop_automation" => Some(AgentTaskKind::DesktopAutomation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecompositionStyle {
    IsolatedFiles,
    CoupledComponents,
    SequentialPipeline,
}

impl DecompositionStyle {
    pub const ALL: [DecompositionStyle; 3] = [
        DecompositionStyle::IsolatedFiles,
        DecompositionStyle::CoupledComponents,
        DecompositionStyle::SequentialPipeline,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DecompositionStyle::IsolatedFiles => "isolated_files",
            DecompositionStyle::CoupledComponents => "coupled_components",
            DecompositionStyle::SequentialPipeline => "sequential_pipeline",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "isolated_files" => Some(DecompositionStyle::IsolatedFiles),
            "coupled_components" => Some(DecompositionStyle::CoupledComponents),
            "sequential_pipeline" => Some(DecompositionStyle::SequentialPipeline),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionTemplate {
    pub id: String,
    pub label: String,
    pub task_kind: AgentTaskKind,
    pub system_prompt: String,
    pub checklist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionPolicy {
    pub id: String,
    pub label: String,
    pub task_kind: AgentTaskKind,
    pub instruction_template_id: String,
    pub decomposition_style: DecompositionStyle,
    pub shared_expectations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferredPolicy {
    pub task_kind: AgentTaskKind,
    pub policy_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionRegistryFile {
    #[serde(default)]
    pub templates: Vec<InstructionTemplate>,
    #[serde(default)]
    pub policies: Vec<DecompositionPolicy>,
    #[serde(default)]
    pub preferred_policies: Vec<PreferredPolicy>,
}
