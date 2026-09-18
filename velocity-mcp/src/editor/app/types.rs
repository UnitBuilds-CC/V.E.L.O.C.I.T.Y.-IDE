use std::path::PathBuf;

use crate::editor::theme::WorkspaceProfile;

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
            TabKind::Snippets => "Snippets".into(),
            TabKind::LanguageServers => "LSP".into(),
            TabKind::Debugger => "Debugger".into(),
            TabKind::PrecompCache => "Precomp".into(),
            TabKind::Multimodal => "Attachments".into(),
            TabKind::ContinuationLedger => "Ledger".into(),
            TabKind::PluginRegistry => "Plugins".into(),
            TabKind::SkillFiles => "Skills".into(),
            TabKind::InlineSuggestions => "Suggestions".into(),
            TabKind::ImprovementEngine => "Improve".into(),
            TabKind::SharedMemory => "Shared Mem".into(),
            TabKind::BackgroundAgents => "Bg Agents".into(),
            TabKind::ConflictResolver => "Conflicts".into(),
            TabKind::Collaboration => "Collab".into(),
            TabKind::PersistentMemory => "Memory".into(),
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
    Snippets,
    LanguageServers,
    Debugger,
    PrecompCache,
    Multimodal,
    ContinuationLedger,
    PluginRegistry,
    SkillFiles,
    InlineSuggestions,
    ImprovementEngine,
    SharedMemory,
    BackgroundAgents,
    ConflictResolver,
    Collaboration,
    PersistentMemory,
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

// ─── Central dock membership ───────────────────────────────────────────────
//
// These live next to `TabKind`, and as free functions, so the rule that decides
// what the central area shows can be tested without constructing an editor.

/// The panels a workspace profile keeps in the dock: always present, so they
/// must not be mistaken for something the user just asked to open.
pub fn profile_primary_kinds(profile: WorkspaceProfile) -> Vec<TabKind> {
    match profile {
        WorkspaceProfile::Coder => vec![TabKind::Chat, TabKind::Output],
        WorkspaceProfile::AutomationOperator => {
            vec![TabKind::Orchestrator, TabKind::Chat, TabKind::Output]
        }
        WorkspaceProfile::MissionControl => {
            vec![TabKind::MissionControl, TabKind::Chat, TabKind::Output]
        }
        WorkspaceProfile::Accessibility => vec![TabKind::Chat, TabKind::Output],
    }
}

/// Derived from [`profile_primary_kinds`] rather than restated, so the two can
/// never disagree about which tabs are always in the dock.
pub fn is_profile_primary_kind(kind: &TabKind) -> bool {
    WorkspaceProfile::ALL.iter().any(|profile| {
        profile_primary_kinds(*profile)
            .iter()
            .any(|primary| std::mem::discriminant(primary) == std::mem::discriminant(kind))
    })
}

pub fn find_tab_by_kind(tabs: &[Tab], kind: &TabKind) -> Option<Tab> {
    tabs.iter()
        .find(|tab| std::mem::discriminant(&tab.kind) == std::mem::discriminant(kind))
        .cloned()
}

/// The tab currently holding the central area, if any. `None` after a close
/// leaves `active_tab` pointing at nothing.
pub fn focused_tab<'a>(tabs: &'a [Tab], active: Option<&TabId>) -> Option<&'a Tab> {
    active.and_then(|id| tabs.iter().find(|tab| &tab.id == id))
}

/// True when the focused tab is a panel the dock is the only host for, so the
/// dock has to be on screen for it to appear at all.
pub fn focused_tab_needs_dock(tabs: &[Tab], active: Option<&TabId>) -> bool {
    focused_tab(tabs, active).is_some_and(|tab| {
        !matches!(tab.kind, TabKind::Editor { .. }) && !is_profile_primary_kind(&tab.kind)
    })
}

/// True when some tab holds focus and it is not an editor, so what the user is
/// asking to look at is a panel and needs a dock to be drawn in.
pub fn focused_tab_is_panel(tabs: &[Tab], active: Option<&TabId>) -> bool {
    focused_tab(tabs, active).is_some_and(|tab| !matches!(tab.kind, TabKind::Editor { .. }))
}

/// Which of the two central hosts is on screen: the dock, or the welcome
/// screen. Single definition so the render gate and the GUI-bridge state
/// report cannot disagree about what the frame actually drew.
///
/// `panel_requested` carries the one thing the tab list cannot say for itself.
/// A profile's primary panels (Chat, Output, Orchestrator) are in the dock all
/// the time, so their mere presence must not evict the welcome screen a fresh
/// session starts on -- but once the user focuses one, the dock has to come on
/// screen or the click lands nowhere. Those two states are otherwise identical
/// in `tabs`/`active_tab`, which is why `Ctrl+J` and the welcome screen's own
/// Chat button could set the active tab and then draw the welcome screen again.
pub fn central_area_is_dock(tabs: &[Tab], active: Option<&TabId>, panel_requested: bool) -> bool {
    tabs.iter()
        .any(|tab| matches!(tab.kind, TabKind::Editor { .. }))
        || focused_tab_needs_dock(tabs, active)
        || (panel_requested && focused_tab_is_panel(tabs, active))
}

/// Panels the GUI control bridge can open by name.
///
/// Before this, an external driver could move the activity bar but could not
/// open a panel tab at all -- which also meant the Settings path had no way to
/// be verified from outside the process.
pub const BRIDGE_PANELS: &[(&str, TabKind)] = &[
    ("settings", TabKind::Settings),
    ("chat", TabKind::Chat),
    ("output", TabKind::Output),
    ("orchestrator", TabKind::Orchestrator),
    ("mission", TabKind::MissionControl),
    ("team", TabKind::TeamStudio),
    ("usage", TabKind::Usage),
    ("search", TabKind::Search),
    ("graph", TabKind::Graph),
    ("wiki", TabKind::Wiki),
    ("agents", TabKind::Agents),
    ("knowledge", TabKind::Knowledge),
    ("workflows", TabKind::Workflows),
    ("governance", TabKind::Governance),
    ("changes", TabKind::Changes),
    ("terminal", TabKind::Terminal),
    ("debugger", TabKind::Debugger),
    ("extensions", TabKind::Extensions),
];

/// Resolve a bridge panel name. Case-insensitive; unknown names are `None`
/// rather than a guess, so a typo surfaces instead of opening the wrong panel.
pub fn panel_kind_from_name(name: &str) -> Option<TabKind> {
    let needle = name.trim().to_ascii_lowercase();
    BRIDGE_PANELS
        .iter()
        .find(|(slug, _)| *slug == needle.as_str())
        .map(|(_, kind)| kind.clone())
}

pub fn bridge_panel_names() -> Vec<&'static str> {
    BRIDGE_PANELS.iter().map(|(name, _)| *name).collect()
}

/// Which tabs the central dock holds: open editors, the profile's primary
/// panels, and whatever the user last focused.
///
/// The focused tab belongs in the set. Filtering to editors + primaries alone
/// silently dropped a Settings tab the moment the dock was rebuilt, leaving it
/// in `tabs` with nowhere to render -- so opening Settings from a workspace
/// with no file open looked like a dead button.
pub fn dock_tab_set(tabs: &[Tab], profile: WorkspaceProfile, active: Option<&TabId>) -> Vec<Tab> {
    assemble_dock_tabs(tabs, profile, active, false, &mut 0)
}

/// [`dock_tab_set`] for the moment a profile is applied: the panels that define
/// the mode must exist, so any missing from `tabs` is created. A plain dock
/// rebuild must not invent tabs.
pub fn dock_tab_set_with_defaults(
    tabs: &[Tab],
    profile: WorkspaceProfile,
    active: Option<&TabId>,
    counter: &mut u64,
) -> Vec<Tab> {
    assemble_dock_tabs(tabs, profile, active, true, counter)
}

fn assemble_dock_tabs(
    tabs: &[Tab],
    profile: WorkspaceProfile,
    active: Option<&TabId>,
    create_missing_primaries: bool,
    counter: &mut u64,
) -> Vec<Tab> {
    let mut selected: Vec<Tab> = tabs
        .iter()
        .filter(|tab| matches!(tab.kind, TabKind::Editor { .. }))
        .cloned()
        .collect();

    for kind in profile_primary_kinds(profile) {
        let found = find_tab_by_kind(tabs, &kind);
        let tab = match (found, create_missing_primaries) {
            (Some(tab), _) => tab,
            (None, true) => Tab {
                id: TabId::next(counter),
                kind,
            },
            (None, false) => continue,
        };
        if !selected.iter().any(|existing| existing.id == tab.id) {
            selected.push(tab);
        }
    }

    if let Some(tab) = active.and_then(|id| tabs.iter().find(|candidate| &candidate.id == id)) {
        if !selected.iter().any(|existing| existing.id == tab.id) {
            selected.push(tab.clone());
        }
    }

    if selected.is_empty() {
        tabs.to_vec()
    } else {
        selected
    }
}

pub struct Command {
    pub label: &'static str,
    pub category: &'static str,
    pub shortcut: Option<&'static str>,
    pub action: fn(&mut super::VelocityApp),
    /// Which modes this command is available in (empty = all modes).
    pub modes: &'static [crate::editor::theme::WorkspaceProfile],
}

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    /// Set when the palette is opened so the search field grabs focus on the
    /// first frame — you can type immediately without clicking.
    pub just_opened: bool,
}

/// Ctrl+P quick-open switcher: fuzzy-search workspace files and jump to them.
#[derive(Default)]
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
#[derive(Default)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAutomationMissionSummary {
    pub task_count: usize,
    pub live_count: usize,
    pub artifact_count: usize,
    pub awaiting_count: usize,
    pub state_labels: Vec<String>,
}

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

/// Result of a background file I/O operation, sent back to the UI thread.
#[derive(Debug, Clone)]
pub enum FileIoResult {
    /// File content was read successfully.
    FileLoaded {
        tab_id: TabId,
        path: PathBuf,
        content: String,
        mtime: Option<std::time::SystemTime>,
    },
    /// File read failed.
    FileLoadFailed {
        tab_id: TabId,
        path: PathBuf,
        error: String,
    },
    /// File content was written successfully.
    FileSaved { path: PathBuf },
    /// File write failed.
    FileSaveFailed { path: PathBuf, error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, kind: TabKind) -> Tab {
        Tab {
            id: TabId(id),
            kind,
        }
    }

    fn editor(id: u64) -> Tab {
        tab(
            id,
            TabKind::Editor {
                path: None,
                buffer_id: TabId(id),
            },
        )
    }

    fn kinds(tabs: &[Tab]) -> Vec<TabKind> {
        tabs.iter().map(|t| t.kind.clone()).collect()
    }

    // ── Central-panel gate ────────────────────────────────────────────────

    /// The regression this whole change exists for: with no file open, a
    /// focused Settings tab still needs the dock, because the dock is the only
    /// place that tab can draw. Gating on editor tabs alone made the gear,
    /// the menu, Ctrl+,, the status-bar chip and the palette all no-ops.
    #[test]
    fn focused_settings_needs_dock_without_editor_tabs() {
        let tabs = [tab(1, TabKind::Settings)];
        assert!(focused_tab_needs_dock(&tabs, Some(&TabId(1))));
    }

    #[test]
    fn every_dock_only_panel_needs_the_dock() {
        for kind in [
            TabKind::Settings,
            TabKind::Wiki,
            TabKind::Graph,
            TabKind::Search,
            TabKind::TeamStudio,
            TabKind::Usage,
            TabKind::Knowledge,
        ] {
            let tabs = [tab(7, kind.clone())];
            assert!(
                focused_tab_needs_dock(&tabs, Some(&TabId(7))),
                "{kind:?} has no host outside the dock"
            );
        }
    }

    /// The welcome screen must survive: it owns the central area until the user
    /// asks for a panel, so the profile's always-present panels and editors
    /// neither of which are the welcome screen's competitor must not pull
    /// the dock on screen by themselves.
    #[test]
    fn primaries_and_editors_do_not_suppress_the_welcome_screen() {
        for kind in [
            TabKind::Chat,
            TabKind::Output,
            TabKind::Orchestrator,
            TabKind::MissionControl,
        ] {
            let tabs = [tab(3, kind.clone())];
            assert!(
                !focused_tab_needs_dock(&tabs, Some(&TabId(3))),
                "{:?} is a profile primary, so it is in the dock at startup and would hide the welcome screen forever",
                kind
            );
        }
        let tabs = [editor(4)];
        assert!(!focused_tab_needs_dock(&tabs, Some(&TabId(4))));
    }

    #[test]
    fn no_focused_tab_does_not_suppress_the_welcome_screen() {
        let tabs = [tab(1, TabKind::Settings)];
        assert!(!focused_tab_needs_dock(&tabs, None));
        // A stale id pointing at nothing (after a close) must not strand the
        // user on an empty dock either.
        assert!(!focused_tab_needs_dock(&tabs, Some(&TabId(99))));
    }

    #[test]
    fn profile_primary_membership_is_derived() {
        assert!(!is_profile_primary_kind(&TabKind::Settings));
        assert!(!is_profile_primary_kind(&TabKind::Wiki));
        for profile in WorkspaceProfile::ALL {
            for primary in profile_primary_kinds(profile) {
                assert!(
                    is_profile_primary_kind(&primary),
                    "{:?} is primary for {:?} but not reported as such",
                    primary,
                    profile
                );
            }
        }
    }

    // ── Dock membership ───────────────────────────────────────────────────

    /// A dock rebuild used to filter to editors + primaries, so a Settings tab
    /// vanished from the dock while still sitting in `tabs` -- and because
    /// `focus_panel` matched only the dock, a second copy piled up beside it.
    #[test]
    fn dock_rebuild_keeps_the_focused_panel_tab() {
        let tabs = [editor(1), tab(2, TabKind::Chat), tab(3, TabKind::Settings)];
        let docked = dock_tab_set(&tabs, WorkspaceProfile::Coder, Some(&TabId(3)));
        assert_eq!(
            kinds(&docked),
            vec![
                TabKind::Editor {
                    path: None,
                    buffer_id: TabId(1)
                },
                TabKind::Chat,
                TabKind::Settings,
            ]
        );
    }

    /// A rebuild selects; it must not invent tabs the user never opened.
    #[test]
    fn dock_rebuild_does_not_create_tabs() {
        let tabs = [tab(1, TabKind::Settings)];
        let docked = dock_tab_set(&tabs, WorkspaceProfile::MissionControl, Some(&TabId(1)));
        assert_eq!(kinds(&docked), vec![TabKind::Settings]);
    }

    /// Applying a profile is different: its defining panels have to exist.
    #[test]
    fn applying_a_profile_creates_missing_primary_panels() {
        let mut counter = 10;
        let tabs = [editor(1)];
        let desired = dock_tab_set_with_defaults(
            &tabs,
            WorkspaceProfile::MissionControl,
            Some(&TabId(1)),
            &mut counter,
        );
        assert_eq!(
            kinds(&desired),
            vec![
                TabKind::Editor {
                    path: None,
                    buffer_id: TabId(1)
                },
                TabKind::MissionControl,
                TabKind::Chat,
                TabKind::Output,
            ]
        );
        assert_eq!(counter, 13, "each created tab consumed one id");
    }

    #[test]
    fn applying_a_profile_reuses_existing_primary_tabs() {
        let mut counter = 10;
        let tabs = [tab(1, TabKind::Chat), tab(2, TabKind::Output)];
        let desired = dock_tab_set_with_defaults(
            &tabs,
            WorkspaceProfile::Coder,
            Some(&TabId(1)),
            &mut counter,
        );
        assert_eq!(kinds(&desired), vec![TabKind::Chat, TabKind::Output]);
        assert_eq!(counter, 10, "nothing to create, so no ids burned");
    }

    /// Nothing selected at all -- no editors, no primaries present, nothing
    /// focused -- would hand `egui_dock` an empty leaf set, which renders as a
    /// blank central area, so the full list is the fallback.
    #[test]
    fn nothing_selected_falls_back_to_the_whole_list() {
        let tabs = [tab(1, TabKind::Settings)];
        let docked = dock_tab_set(&tabs, WorkspaceProfile::Coder, None);
        assert_eq!(kinds(&docked), vec![TabKind::Settings]);

        let none: [Tab; 0] = [];
        assert!(dock_tab_set(&none, WorkspaceProfile::Coder, None).is_empty());
    }

    #[test]
    fn find_tab_by_kind_matches_on_kind_not_identity() {
        let tabs = [editor(1), tab(2, TabKind::Settings)];
        // Callers look up a panel by kind, holding an id they never allocated.
        let found = find_tab_by_kind(&tabs, &TabKind::Settings).unwrap();
        assert_eq!(found.id, TabId(2));
        assert!(find_tab_by_kind(&tabs, &TabKind::Wiki).is_none());
    }

    // ── GUI control bridge vocabulary ─────────────────────────────────────

    #[test]
    fn every_advertised_panel_name_resolves() {
        // The tool description is generated from this table, so a slug with no
        // working kind behind it would be advertised and then rejected.
        assert!(!BRIDGE_PANELS.is_empty());
        for (name, kind) in BRIDGE_PANELS {
            assert_eq!(
                panel_kind_from_name(name).as_ref(),
                Some(kind),
                "panel '{name}' is listed but does not resolve"
            );
            assert_eq!(
                panel_kind_from_name(&name.to_uppercase()).as_ref(),
                Some(kind),
                "lookup should be case-insensitive for '{name}'"
            );
            assert_eq!(*name, name.to_ascii_lowercase());
        }
        assert_eq!(
            bridge_panel_names().len(),
            BRIDGE_PANELS.len(),
            "the advertised list drifted from the table"
        );
    }

    #[test]
    fn unknown_panel_names_are_rejected_not_guessed() {
        assert!(panel_kind_from_name("settingz").is_none());
        assert!(panel_kind_from_name("").is_none());
        assert!(panel_kind_from_name("editor").is_none());
    }

    /// `settings` is the entry the user was clicking; it has to resolve to the
    /// tab the gear opens, and that tab has to be one the dock alone can host.
    #[test]
    fn settings_panel_is_bridge_reachable_and_dock_only() {
        let kind = panel_kind_from_name("settings").unwrap();
        assert_eq!(kind, TabKind::Settings);
        let tabs = [tab(1, kind)];
        // A dock-only panel needs the dock whether or not a request is on file:
        // nothing else can draw it.
        assert!(central_area_is_dock(&tabs, Some(&TabId(1)), false));
        assert!(central_area_is_dock(&tabs, Some(&TabId(1)), true));
    }

    #[test]
    fn central_area_prefers_the_dock_then_the_welcome_screen() {
        // An open editor is enough on its own.
        let tabs = [editor(1)];
        assert!(central_area_is_dock(&tabs, Some(&TabId(1)), false));
        // Nothing open at all: startup state, welcome screen.
        let none: [Tab; 0] = [];
        assert!(!central_area_is_dock(&none, None, false));
        // Primaries alone, unfocused, do not pull the dock on screen.
        let tabs = [tab(1, TabKind::Chat), tab(2, TabKind::Output)];
        assert!(!central_area_is_dock(&tabs, Some(&TabId(1)), false));
    }

    /// The dead-button case: a profile parks focus on Chat/Output/Orchestrator,
    /// so those tabs are always in `tabs`. When the user then clicks Chat (or
    /// presses Ctrl+J) the tab list looks exactly as it did at startup -- only
    /// the request distinguishes the two. Without honouring it, focusing Chat
    /// redrew the welcome screen and the click appeared to do nothing.
    #[test]
    fn a_user_request_focuses_a_primary_panel_on_screen() {
        let tabs = [tab(1, TabKind::Chat), tab(2, TabKind::Output)];
        assert!(central_area_is_dock(&tabs, Some(&TabId(1)), true));
    }

    /// The counterpart the welcome screen depends on: the preset's own landing
    /// focus must not count as a request, or no fresh session would ever see it.
    #[test]
    fn the_presets_own_landing_focus_keeps_the_welcome_screen() {
        let tabs = [tab(1, TabKind::Chat), tab(2, TabKind::Output)];
        assert!(!central_area_is_dock(&tabs, Some(&TabId(1)), false));
        let tabs = [tab(1, TabKind::Orchestrator)];
        assert!(!central_area_is_dock(&tabs, Some(&TabId(1)), false));
    }

    #[test]
    fn a_request_with_nothing_focused_does_not_invent_a_dock() {
        let none: [Tab; 0] = [];
        assert!(
            !central_area_is_dock(&none, None, true),
            "a stale request must not open a dock with no tab to show"
        );
        // A dangling id (closed tab) counts the same way.
        let tabs = [tab(1, TabKind::Chat)];
        assert!(!central_area_is_dock(&tabs, Some(&TabId(99)), true));
    }

    #[test]
    fn a_request_never_hides_an_open_editor() {
        let tabs = [editor(1), tab(2, TabKind::Chat)];
        assert!(central_area_is_dock(&tabs, Some(&TabId(1)), false));
        assert!(central_area_is_dock(&tabs, Some(&TabId(2)), true));
    }

    #[test]
    fn focused_tab_is_panel_separates_panels_from_editors() {
        assert!(focused_tab_is_panel(
            &[tab(1, TabKind::Chat)],
            Some(&TabId(1))
        ));
        assert!(!focused_tab_is_panel(&[editor(1)], Some(&TabId(1))));
        assert!(!focused_tab_is_panel(&[tab(1, TabKind::Chat)], None));
    }
}
