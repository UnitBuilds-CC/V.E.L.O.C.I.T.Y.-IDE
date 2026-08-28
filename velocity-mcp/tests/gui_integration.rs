//! Integration tests for GUI component state management.
//!
//! These tests verify the headless logic behind GUI components:
//! tab lifecycle, command palette filtering, MRU switching, quick-open,
//! file tree operations, and editor state transitions.

use std::path::PathBuf;

// ─── Tab & TabKind ──────────────────────────────────────────────────────────

/// Verify TabId generates unique, incrementing IDs.
#[test]
fn tab_id_generates_unique_ids() {
    use velocity_mcp::editor::app::types::TabId;

    let mut counter = 0u64;
    let id1 = TabId::next(&mut counter);
    let id2 = TabId::next(&mut counter);
    let id3 = TabId::next(&mut counter);

    assert_eq!(id1, TabId(1));
    assert_eq!(id2, TabId(2));
    assert_eq!(id3, TabId(3));
    assert_eq!(counter, 3);
}

/// Verify Tab::title() returns the filename for editor tabs.
#[test]
fn tab_title_editor_returns_filename() {
    use velocity_mcp::editor::app::types::{Tab, TabId, TabKind};

    let tab = Tab {
        id: TabId(1),
        kind: TabKind::Editor {
            path: Some(PathBuf::from("/home/user/project/src/main.rs")),
            buffer_id: TabId(0),
        },
    };
    assert_eq!(tab.title(), "main.rs");
}

/// Verify Tab::title() returns "untitled" for editor tabs without a path.
#[test]
fn tab_title_editor_no_path_returns_untitled() {
    use velocity_mcp::editor::app::types::{Tab, TabId, TabKind};

    let tab = Tab {
        id: TabId(1),
        kind: TabKind::Editor {
            path: None,
            buffer_id: TabId(0),
        },
    };
    assert_eq!(tab.title(), "untitled");
}

/// Verify Tab::title() returns correct names for special panel tabs.
#[test]
fn tab_title_special_panels() {
    use velocity_mcp::editor::app::types::{Tab, TabId, TabKind};

    let cases = vec![
        (TabKind::Chat, "Chat"),
        (TabKind::Output, "Output"),
        (TabKind::Orchestrator, "Orchestrator"),
        (TabKind::MissionControl, "Mission"),
        (TabKind::TeamStudio, "Team"),
        (TabKind::Usage, "Usage"),
        (TabKind::Search, "Search"),
        (TabKind::Graph, "Graph"),
        (TabKind::Wiki, "Wiki"),
        (TabKind::Settings, "Settings"),
        (TabKind::Terminal, "Terminal"),
        (TabKind::Debugger, "Debugger"),
        (TabKind::Extensions, "Extensions"),
        (TabKind::Governance, "Governance"),
        (TabKind::Workflows, "Workflows"),
        (TabKind::Knowledge, "Knowledge"),
        (TabKind::SharedMemory, "Shared Mem"),
        (TabKind::BackgroundAgents, "Bg Agents"),
        (TabKind::ConflictResolver, "Conflicts"),
        (TabKind::Collaboration, "Collab"),
        (TabKind::PersistentMemory, "Memory"),
        (TabKind::Triggers, "Triggers"),
        (TabKind::Changes, "Changes"),
        (TabKind::Peers, "Peers"),
    ];

    for (kind, expected) in cases {
        let tab = Tab {
            id: TabId(1),
            kind,
        };
        assert_eq!(tab.title(), expected, "TabKind should produce title '{}'", expected);
    }
}

/// Verify Tab::editor_path() returns Some only for Editor tabs.
#[test]
fn tab_editor_path_only_for_editor_tabs() {
    use velocity_mcp::editor::app::types::{Tab, TabId, TabKind};

    let editor_tab = Tab {
        id: TabId(1),
        kind: TabKind::Editor {
            path: Some(PathBuf::from("/src/lib.rs")),
            buffer_id: TabId(0),
        },
    };
    assert_eq!(
        editor_tab.editor_path(),
        Some(&PathBuf::from("/src/lib.rs"))
    );

    let chat_tab = Tab {
        id: TabId(2),
        kind: TabKind::Chat,
    };
    assert_eq!(chat_tab.editor_path(), None);

    let editor_no_path = Tab {
        id: TabId(3),
        kind: TabKind::Editor {
            path: None,
            buffer_id: TabId(0),
        },
    };
    assert_eq!(editor_no_path.editor_path(), None);
}

/// Verify TabKind::NdaDoc title extraction.
#[test]
fn tab_title_nda_doc() {
    use velocity_mcp::editor::app::types::{Tab, TabId, TabKind};

    let tab_with_path = Tab {
        id: TabId(1),
        kind: TabKind::NdaDoc {
            path: Some(PathBuf::from("/project/seals/contract.nda")),
        },
    };
    assert_eq!(tab_with_path.title(), "contract.nda");

    let tab_no_path = Tab {
        id: TabId(2),
        kind: TabKind::NdaDoc { path: None },
    };
    assert_eq!(tab_no_path.title(), "NDA Document");
}

// ─── CommandPalette ──────────────────────────────────────────────────────────

/// Verify CommandPalette initial state.
#[test]
fn command_palette_initial_state() {
    use velocity_mcp::editor::app::types::CommandPalette;

    let palette = CommandPalette {
        open: false,
        query: String::new(),
        selected: 0,
        just_opened: false,
    };
    assert!(!palette.open);
    assert!(palette.query.is_empty());
    assert_eq!(palette.selected, 0);
    assert!(!palette.just_opened);
}

// ─── QuickOpen ───────────────────────────────────────────────────────────────

/// Verify QuickOpen initial state and filter invalidation.
#[test]
fn quick_open_state_management() {
    use velocity_mcp::editor::app::types::QuickOpen;

    let mut qo = QuickOpen {
        open: false,
        query: String::new(),
        selected: 0,
        just_opened: false,
        files: vec![
            "src/main.rs".into(),
            "src/lib.rs".into(),
            "tests/integration.rs".into(),
            "Cargo.toml".into(),
        ],
        last_query: String::new(),
        last_file_count: 4,
        filtered: vec![],
        scroll_to_selected: false,
    };

    // Simulate opening
    qo.open = true;
    qo.just_opened = true;
    assert!(qo.open);
    assert!(qo.just_opened);

    // Simulate typing a query
    qo.query = "main".into();
    assert_ne!(qo.query, qo.last_query);
    // In the real app, filtered would be recomputed here
}

// ─── MruSwitcher ─────────────────────────────────────────────────────────────

/// Verify MRU tab switcher ordering.
#[test]
fn mru_switcher_tab_ordering() {
    use velocity_mcp::editor::app::types::{MruSwitcher, TabId};

    let mut switcher = MruSwitcher {
        open: false,
        selected: 0,
        order: vec![TabId(3), TabId(1), TabId(2)],
    };

    // Most recently used is first
    assert_eq!(switcher.order[0], TabId(3));

    // Simulate cycling: move selected forward
    switcher.open = true;
    switcher.selected = 1;
    assert_eq!(switcher.order[switcher.selected], TabId(1));

    // Commit: the selected tab moves to front
    let committed = switcher.order.remove(switcher.selected);
    switcher.order.insert(0, committed);
    assert_eq!(switcher.order[0], TabId(1));
}

// ─── NavLocation ─────────────────────────────────────────────────────────────

/// Verify NavLocation tracks cursor positions for back/forward navigation.
#[test]
fn nav_location_tracks_positions() {
    use velocity_mcp::editor::app::types::NavLocation;

    let loc1 = NavLocation {
        path: PathBuf::from("src/main.rs"),
        line: Some(42),
    };
    let loc2 = NavLocation {
        path: PathBuf::from("src/lib.rs"),
        line: Some(100),
    };
    let loc3 = NavLocation {
        path: PathBuf::from("Cargo.toml"),
        line: None,
    };

    // Navigation history stack
    let history = vec![loc1.clone(), loc2.clone(), loc3.clone()];
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].line, Some(42));
    assert_eq!(history[2].line, None);
}

// ─── FileIoResult ────────────────────────────────────────────────────────────

/// Verify FileIoResult variants carry correct data.
#[test]
fn file_io_result_variants() {
    use velocity_mcp::editor::app::types::{FileIoResult, TabId};
    use std::time::SystemTime;

    let success = FileIoResult::FileLoaded {
        tab_id: TabId(1),
        path: PathBuf::from("test.rs"),
        content: "fn main() {}".into(),
        mtime: Some(SystemTime::now()),
    };
    match &success {
        FileIoResult::FileLoaded { content, .. } => assert_eq!(content, "fn main() {}"),
        _ => panic!("expected FileLoaded"),
    }

    let failure = FileIoResult::FileLoadFailed {
        tab_id: TabId(2),
        path: PathBuf::from("missing.rs"),
        error: "No such file".into(),
    };
    match &failure {
        FileIoResult::FileLoadFailed { error, .. } => assert_eq!(error, "No such file"),
        _ => panic!("expected FileLoadFailed"),
    }

    let save_ok = FileIoResult::FileSaved {
        path: PathBuf::from("output.rs"),
    };
    match &save_ok {
        FileIoResult::FileSaved { path } => assert_eq!(path, &PathBuf::from("output.rs")),
        _ => panic!("expected FileSaved"),
    }

    let save_fail = FileIoResult::FileSaveFailed {
        path: PathBuf::from("readonly.rs"),
        error: "Permission denied".into(),
    };
    match &save_fail {
        FileIoResult::FileSaveFailed { path, error } => {
            assert_eq!(path, &PathBuf::from("readonly.rs"));
            assert_eq!(error, "Permission denied");
        }
        _ => panic!("expected FileSaveFailed"),
    }
}

// ─── FileNode ────────────────────────────────────────────────────────────────

/// Verify FileNode tree construction and traversal.
#[test]
fn file_node_tree_construction() {
    use velocity_mcp::editor::app::types::FileNode;

    let tree = FileNode {
        name: "src".into(),
        path: PathBuf::from("/project/src"),
        is_dir: true,
        children: Some(vec![
            FileNode {
                name: "main.rs".into(),
                path: PathBuf::from("/project/src/main.rs"),
                is_dir: false,
                children: None,
            },
            FileNode {
                name: "lib.rs".into(),
                path: PathBuf::from("/project/src/lib.rs"),
                is_dir: false,
                children: None,
            },
            FileNode {
                name: "model".into(),
                path: PathBuf::from("/project/src/model"),
                is_dir: true,
                children: Some(vec![FileNode {
                    name: "transformer.rs".into(),
                    path: PathBuf::from("/project/src/model/transformer.rs"),
                    is_dir: false,
                    children: None,
                }]),
            },
        ]),
    };

    assert!(tree.is_dir);
    let children = tree.children.as_ref().unwrap();
    assert_eq!(children.len(), 3);
    assert!(!children[0].is_dir);
    assert!(children[2].is_dir);

    let nested = children[2].children.as_ref().unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].name, "transformer.rs");
}

// ─── DesktopAutomationEvidenceState ──────────────────────────────────────────

/// Verify evidence state labels and details.
#[test]
fn desktop_automation_evidence_state_labels() {
    use velocity_mcp::editor::app::types::DesktopAutomationEvidenceState;

    let states = [
        DesktopAutomationEvidenceState::LiveEvidence,
        DesktopAutomationEvidenceState::ArtifactBacked,
        DesktopAutomationEvidenceState::AwaitingEvidence,
    ];

    for state in &states {
        let label = state.label();
        let detail = state.detail();
        assert!(!label.is_empty(), "label should not be empty");
        assert!(!detail.is_empty(), "detail should not be empty");
    }

    assert_eq!(
        DesktopAutomationEvidenceState::LiveEvidence.label(),
        "Live WA evidence"
    );
    assert_eq!(
        DesktopAutomationEvidenceState::AwaitingEvidence.label(),
        "Awaiting WA evidence"
    );
}

// ─── ActiveChangePreview ─────────────────────────────────────────────────────

/// Verify ActiveChangePreview tracks diff information.
#[test]
fn active_change_preview_tracks_diff() {
    use velocity_mcp::editor::app::types::ActiveChangePreview;

    let preview = ActiveChangePreview {
        file_label: "src/main.rs".into(),
        added_lines: 15,
        removed_lines: 3,
        preview: "+fn new_function() {\n+    println!(\"hello\");\n+}".into(),
        full_diff: "diff --git a/src/main.rs\n+fn new_function()...".into(),
    };

    assert_eq!(preview.added_lines, 15);
    assert_eq!(preview.removed_lines, 3);
    assert!(preview.preview.starts_with('+'));
    assert!(preview.full_diff.starts_with("diff"));
}

// ─── Cross-module: Health + Metrics integration ─────────────────────────────

/// Verify health and metrics modules work together.
#[test]
fn health_and_metrics_integration() {
    use velocity_mcp::health::{HealthChecker, HealthStatus};
    use velocity_mcp::metrics::{record_request, Metrics};
    use std::time::Duration;

    // Health check should work independently
    let checker = HealthChecker::new("/tmp/test-workspace");
    let basic = checker.check();
    assert!(
        basic.status == HealthStatus::Ok || basic.status == HealthStatus::Degraded,
        "health should be ok or degraded in test env"
    );

    // Record a request so metrics appear in output
    record_request("health_check_test", "success", Duration::from_millis(10));

    // Metrics should record independently
    let metrics = Metrics::global();
    let output = metrics.encode();
    assert!(output.contains("health_check_test"));

    // Both can coexist without interference
    let detailed = checker.check_detailed(vec![]);
    assert!(detailed.version.contains("1.0"));
}

// ─── Telemetry + Metrics cross-module ────────────────────────────────────────

/// Verify telemetry config and metrics can be initialized together.
#[test]
fn telemetry_config_and_metrics_coexist() {
    use velocity_mcp::metrics::{record_request, Metrics};
    use velocity_mcp::telemetry::TracingConfig;
    use std::time::Duration;

    // TracingConfig should be constructable
    let config = TracingConfig::default();
    assert_eq!(config.level, "info");

    // Metrics should still work
    record_request("test_method", "success", Duration::from_millis(50));
    let output = Metrics::global().encode();
    assert!(output.contains("test_method"));
}
