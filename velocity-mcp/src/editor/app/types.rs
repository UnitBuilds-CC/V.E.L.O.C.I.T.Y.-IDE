use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

impl TabId {
    pub fn next(counter: &mut u64) -> Self {
        *counter += 1;
        TabId(*counter)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
}

impl Tab {
    pub fn title(&self) -> String {
        match &self.kind {
            TabKind::Editor { path, .. } => path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled".into()),
            TabKind::Chat => "Chat".into(),
            TabKind::Output => "Output".into(),
            TabKind::Orchestrator => "Orchestrator".into(),
            TabKind::MissionControl => "Mission".into(),
            TabKind::TeamStudio => "Team".into(),
            TabKind::Usage => "Usage".into(),
            TabKind::Search => "Search".into(),
            TabKind::Graph => "Graph".into(),
            TabKind::Settings => "Settings".into(),
        }
    }

    pub fn editor_path(&self) -> Option<&PathBuf> {
        match &self.kind {
            TabKind::Editor { path, .. } => path.as_ref(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum TabKind {
    Editor {
        path: Option<PathBuf>,
        buffer_id: TabId,
    },
    Chat,
    Output,
    Orchestrator,
    MissionControl,
    TeamStudio,
    Usage,
    Search,
    Graph,
    Settings,
}

pub struct Command {
    pub label: &'static str,
    pub category: &'static str,
    pub shortcut: Option<&'static str>,
    pub action: fn(&mut super::VelocityApp),
}

pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

#[allow(dead_code)]
pub struct ActiveChangePreview {
    pub file_label: String,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub preview: String,
    pub full_diff: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAutomationEvidenceState {
    LiveEvidence,
    ArtifactBacked,
    AwaitingEvidence,
}

impl DesktopAutomationEvidenceState {
    pub fn label(self) -> &'static str {
        match self {
            DesktopAutomationEvidenceState::LiveEvidence => "Live WA evidence",
            DesktopAutomationEvidenceState::ArtifactBacked => "WA artifacts captured",
            DesktopAutomationEvidenceState::AwaitingEvidence => "Awaiting WA evidence",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            DesktopAutomationEvidenceState::LiveEvidence => {
                "Worker is still producing live desktop automation evidence."
            }
            DesktopAutomationEvidenceState::ArtifactBacked => {
                "Run summary or NDA facts are available for truthful desktop-test review."
            }
            DesktopAutomationEvidenceState::AwaitingEvidence => {
                "Desktop automation tasks should capture live WA evidence before they are treated as complete."
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAutomationMissionSummary {
    pub task_count: usize,
    pub live_count: usize,
    pub artifact_count: usize,
    pub awaiting_count: usize,
    pub state_labels: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAutomationSelectedTaskStatus {
    pub state_label: &'static str,
    pub state_detail: &'static str,
    pub artifact_count: usize,
    pub output_count: usize,
    pub evidence_update_count: usize,
    pub has_transcript: bool,
    pub has_operator_notes: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAutomationSelectedTaskCues {
    pub artifact_lines: Vec<String>,
    pub next_action: &'static str,
}

#[derive(Clone, Debug)]
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Option<Vec<FileNode>>,
}
