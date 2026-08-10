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
            TabKind::Wiki => "Wiki".into(),
            TabKind::Settings => "Settings".into(),
            TabKind::Flows => "Flows".into(),
            TabKind::Targets => "Targets".into(),
            TabKind::Recordings => "Recordings".into(),
            TabKind::Logs => "Logs".into(),
            TabKind::Agents => "Agents".into(),
            TabKind::Queue => "Queue".into(),
            TabKind::Timeline => "Timeline".into(),
            TabKind::Metrics => "Metrics".into(),
            TabKind::Favorites => "Favorites".into(),
            TabKind::Bookmarks => "Bookmarks".into(),
            TabKind::AccessibilityAudit => "Audit".into(),
            TabKind::Terminal => "Terminal".into(),
            TabKind::Extensions => "Extensions".into(),
            TabKind::Activity => "Activity".into(),
            TabKind::Coverage => "Coverage".into(),
            TabKind::Pipeline => "Pipeline".into(),
            TabKind::Voice => "Voice".into(),
            TabKind::TestGenerator => "Test Gen".into(),
            TabKind::AgentMemory => "Memory".into(),
            TabKind::LiveOrchestration => "Orchestration".into(),
            TabKind::SemanticSearch => "Semantic".into(),
            TabKind::Knowledge => "Knowledge".into(),
            TabKind::Triggers => "Triggers".into(),
            TabKind::Workflows => "Workflows".into(),
            TabKind::Governance => "Governance".into(),
            TabKind::Changes => "Changes".into(),
            TabKind::Peers => "Peers".into(),
            TabKind::NdaDoc { path } => path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "NDA Document".into()),
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
    Wiki,
    Settings,
    // Mode-specific panel tabs
    Flows,
    Targets,
    Recordings,
    Logs,
    Agents,
    Queue,
    Timeline,
    Metrics,
    Favorites,
    Bookmarks,
    AccessibilityAudit,
    Terminal,
    // Tier-3 subsystem panels
    Extensions,
    Activity,
    Coverage,
    Pipeline,
    Voice,
    TestGenerator,
    AgentMemory,
    LiveOrchestration,
    SemanticSearch,
    // Knowledge / RAG panel
    Knowledge,
    // Unattended execution triggers
    Triggers,
    // Workflow composer
    Workflows,
    // Governance: policy, approvals, secrets, connectors
    Governance,
    // Recent changes timeline (git log + uncommitted changes)
    Changes,
    // Cross-device peer collaboration
    Peers,
    // NDA document editor (portable/sealed NDA1 with in-file history)
    NdaDoc {
        path: Option<PathBuf>,
    },
}

pub struct Command {
    pub label: &'static str,
    pub category: &'static str,
    pub shortcut: Option<&'static str>,
    pub action: fn(&mut super::VelocityApp),
    /// Which modes this command is available in (empty = all modes).
    pub modes: &'static [crate::editor::theme::WorkspaceProfile],
}

pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    /// Set when the palette is opened so the search field grabs focus on the
    /// first frame — you can type immediately without clicking.
    pub just_opened: bool,
}

/// Ctrl+P quick-open switcher: fuzzy-search workspace files and jump to them.
pub struct QuickOpen {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    /// Set on open so the search field grabs focus immediately.
    pub just_opened: bool,
    /// Cached file list (relative paths) gathered when the switcher opens.
    pub files: Vec<String>,
    /// Query the cached `filtered` indices were computed for.
    pub last_query: String,
    /// `files.len()` when `filtered` was computed (invalidates on repopulation).
    pub last_file_count: usize,
    /// Cached indices into `files` matching `last_query` (avoids per-frame cloning).
    pub filtered: Vec<usize>,
    /// One-shot: force the scroll view to the selected row (set on keyboard nav).
    pub scroll_to_selected: bool,
}

/// Ctrl+Tab most-recently-used tab switcher: hold Ctrl and tap Tab to cycle
/// open tabs in recency order; release Ctrl to commit.
pub struct MruSwitcher {
    pub open: bool,
    /// Index into `order` of the currently highlighted tab.
    pub selected: usize,
    /// Tab ids ordered most-recently-used first.
    pub order: Vec<TabId>,
}

/// A restorable cursor position for back/forward navigation (Alt+← / Alt+→).
#[derive(Clone, Debug)]
pub struct NavLocation {
    pub path: PathBuf,
    /// 1-based line, if known.
    pub line: Option<usize>,
}

pub struct ActiveChangePreview {
    pub file_label: String,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub preview: String,
    pub full_diff: String,
}

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

#[allow(dead_code)] // Constructed by wa.rs for future desktop automation panel
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAutomationMissionSummary {
    pub task_count: usize,
    pub live_count: usize,
    pub artifact_count: usize,
    pub awaiting_count: usize,
    pub state_labels: Vec<String>,
}

#[allow(dead_code)] // Constructed by wa.rs for future desktop automation panel
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

#[allow(dead_code)] // Fields used by DesktopAutomationSelectedTaskCues display
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
