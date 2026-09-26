//! Focused sub-structs extracted from `VelocityApp` to reduce the god-struct
//! complexity. Each sub-struct groups related fields by domain.
//!
//! These are composed back into `VelocityApp` as named fields, making the
//! top-level struct more readable and the domain boundaries explicit.

use std::collections::HashMap;

// ─── Workflow AppState ────────────────────────────────────────────────────────

/// Workflow composer registry state and all related UI draft fields.
/// Extracted from VelocityApp to reduce god-struct complexity.
#[derive(Debug, Clone, Default)]
pub struct WorkflowAppState {
    /// Workflow composer registry shown in the Workflows panel.
    pub workflows: crate::editor::workflow::WorkflowRegistry,
    /// Draft workflow name in the Workflows panel create box.
    pub workflow_name_input: String,
    /// Id of the workflow currently open in the step editor.
    pub workflow_selected: Option<String>,
    /// Draft step: tool name in the Workflows panel add-step row.
    pub workflow_step_tool_input: String,
    /// Draft step: tool JSON args in the Workflows panel add-step row.
    pub workflow_step_args_input: String,
    /// Draft step: agent prompt in the Workflows panel add-step row.
    pub workflow_step_prompt_input: String,
    /// Last workflow run result rendered in the Workflows panel run log.
    pub workflow_last_run: Option<crate::editor::workflow::WorkflowRun>,
    /// Visual canvas instances keyed by workflow id.
    pub workflow_canvases: HashMap<String, crate::editor::workflow::canvas::WorkflowCanvas>,
    /// Id of the workflow currently open in the visual canvas editor.
    pub workflow_canvas_selected: Option<String>,
    /// Whether the visual canvas editor is active (vs list composer).
    pub workflow_visual_mode: bool,
    /// AI generation prompt input for natural language workflow creation.
    pub workflow_ai_prompt: String,
    /// Version history registry for workflows.
    pub workflow_versions: crate::editor::workflow::version::VersionRegistry,
}

impl WorkflowAppState {
    /// Create a new WorkflowAppState loaded from the workspace root.
    pub fn new(workspace_root: &std::path::Path) -> Self {
        Self {
            workflows: crate::editor::workflow::WorkflowRegistry::load(workspace_root),
            workflow_name_input: String::new(),
            workflow_selected: None,
            workflow_step_tool_input: String::new(),
            workflow_step_args_input: String::new(),
            workflow_step_prompt_input: String::new(),
            workflow_last_run: None,
            workflow_canvases: HashMap::new(),
            workflow_canvas_selected: None,
            workflow_visual_mode: false,
            workflow_ai_prompt: String::new(),
            workflow_versions: crate::editor::workflow::version::VersionRegistry::load(
                workspace_root,
            ),
        }
    }
}

// ─── GovernanceState ──────────────────────────────────────────────────────────

/// Governance policy engine, approval queue, secrets, connectors, and related
/// UI draft fields. Extracted from VelocityApp.
#[derive(Debug, Clone, Default)]
pub struct GovernanceState {
    /// Governance policy engine edited in the Governance panel.
    pub policy: crate::editor::governance::PolicyEngine,
    /// Approval queue shown in the Governance panel.
    pub approvals: crate::editor::governance::ApprovalQueue,
    /// Secret store (handles only, masked) shown in the Governance panel.
    pub secrets: crate::security::secrets::SecretStore,
    /// Connector registry shown/edited in the Governance panel.
    pub connectors: crate::connectors::ConnectorRegistry,
    /// Draft rule tool name in the Governance policy editor.
    pub gov_rule_tool_input: String,
    /// Draft rule path prefix in the Governance policy editor.
    pub gov_rule_path_input: String,
    /// Draft new secret name in the Governance secrets section.
    pub gov_secret_name_input: String,
    /// Draft new secret value in the Governance secrets section.
    pub gov_secret_value_input: String,
    /// Draft connector id in the Governance connectors section.
    pub gov_connector_id_input: String,
    /// Draft connector base URL in the Governance connectors section.
    pub gov_connector_url_input: String,
    /// Draft connector secret handle in the Governance connectors section.
    pub gov_connector_secret_input: String,
    /// Transient status line shown at the top of the Governance panel.
    pub gov_status: String,
}

impl GovernanceState {
    /// Create a new GovernanceState loaded from the workspace root.
    pub fn new(workspace_root: &std::path::Path) -> Self {
        Self {
            policy: crate::editor::governance::PolicyEngine::load(workspace_root),
            approvals: crate::editor::governance::ApprovalQueue::load(workspace_root),
            secrets: crate::security::secrets::SecretStore::load(workspace_root),
            connectors: crate::connectors::ConnectorRegistry::load(workspace_root),
            gov_rule_tool_input: String::new(),
            gov_rule_path_input: String::new(),
            gov_secret_name_input: String::new(),
            gov_secret_value_input: String::new(),
            gov_connector_id_input: String::new(),
            gov_connector_url_input: String::new(),
            gov_connector_secret_input: String::new(),
            gov_status: String::new(),
        }
    }
}

// ─── PeerCollabState ──────────────────────────────────────────────────────────

/// Cross-device peer collaboration state. Extracted from VelocityApp.
#[derive(Debug, Clone)]
pub struct PeerCollabState {
    /// Peer manager for cross-device agent collaboration.
    pub peer_manager: crate::agent::peer_link::PeerManager,
    /// Whether the peer API server is currently running.
    pub peer_server_running: bool,
    /// Port configured for the peer API server.
    pub peer_port: u16,
    /// Text buffer for the port field in the peer panel UI.
    pub peer_port_input: String,
    /// Draft peer host for adding a new peer connection.
    pub peer_add_host: String,
    /// Draft peer port for adding a new peer connection.
    pub peer_add_port: String,
    /// Draft peer name for adding a new peer connection.
    pub peer_add_name: String,
    /// Draft chat message for peer-to-peer messaging.
    pub peer_chat_message: String,
    /// Selected peer ID for the chat panel.
    pub peer_chat_selected: Option<String>,
    /// Transient status line for the peer panel.
    pub peer_status: String,
}

impl PeerCollabState {
    /// Create a new PeerCollabState initialized with the workspace root and hostname.
    pub fn new(workspace_root: &std::path::Path) -> Self {
        let mut mgr = crate::agent::peer_link::PeerManager::new();
        let hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "velocity-instance".to_string());
        mgr.init(workspace_root, &hostname);
        Self {
            peer_manager: mgr,
            peer_server_running: false,
            peer_port: 9191,
            peer_port_input: "9191".into(),
            peer_add_host: String::new(),
            peer_add_port: String::new(),
            peer_add_name: String::new(),
            peer_chat_message: String::new(),
            peer_chat_selected: None,
            peer_status: String::new(),
        }
    }
}

impl Default for PeerCollabState {
    fn default() -> Self {
        Self {
            peer_manager: crate::agent::peer_link::PeerManager::new(),
            peer_server_running: false,
            peer_port: 9191,
            peer_port_input: "9191".into(),
            peer_add_host: String::new(),
            peer_add_port: String::new(),
            peer_add_name: String::new(),
            peer_chat_message: String::new(),
            peer_chat_selected: None,
            peer_status: String::new(),
        }
    }
}

// ─── LspState ─────────────────────────────────────────────────────────────────

/// LSP client manager and related diagnostics state. Extracted from VelocityApp.
#[derive(Default)]
pub struct LspState {
    /// LSP client manager.
    pub lsp_manager: Option<crate::editor::lsp_client::LspManager>,
    /// Aggregated diagnostics from LSP.
    pub diagnostics: crate::editor::diagnostics::DiagnosticsState,
}

impl LspState {
    /// Create a new LspState with auto-detected language servers.
    pub fn new(workspace_root: &std::path::Path) -> Self {
        Self {
            lsp_manager: Some(crate::editor::lsp_client::LspManager::auto_detect(
                workspace_root,
            )),
            diagnostics: crate::editor::diagnostics::DiagnosticsState::default(),
        }
    }
}

// ─── Disk Hygiene overlay state ───────────────────────────────────────────────

/// One completed background operation reported to the overlay. Scans and
/// cleans run off the UI thread (same discipline as the file-tree build):
/// a large workspace can hold millions of artifact files and the frame loop
/// must never wait on the disk.
#[derive(Debug)]
pub enum HygieneEvent {
    ScanDone(crate::disk_hygiene::HygieneReport),
    CleanDone(crate::disk_hygiene::CleanResult),
}

/// State for the "Clean Build Artifacts..." overlay (Ctrl+Shift+K).
/// Grouped as a sub-struct so the scan lifecycle, selection, and
/// confirmation state live in one place instead of eight loose fields.
pub struct DiskHygieneState {
    /// Whether the overlay is on screen.
    pub open: bool,
    /// A background scan is in flight; the overlay shows "Scanning...".
    pub scanning: bool,
    /// A background clean is in flight (real delete or dry run).
    pub cleaning: bool,
    /// Reclaim was clicked and the button is waiting for its confirm click.
    pub confirming: bool,
    /// Most recent scan result, drives the table.
    pub report: Option<crate::disk_hygiene::HygieneReport>,
    /// Artifact trees the user checked; the cleanup target set.
    pub checked: Vec<String>,
    /// Last clean outcome rendered under the table ("Reclaimed 6.7 GiB ...").
    pub last_result: Option<String>,
    pub tx: crossbeam_channel::Sender<HygieneEvent>,
    pub rx: crossbeam_channel::Receiver<HygieneEvent>,
}

impl Default for DiskHygieneState {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskHygieneState {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::bounded(8);
        Self {
            open: false,
            scanning: false,
            cleaning: false,
            confirming: false,
            report: None,
            checked: Vec::new(),
            last_result: None,
            tx,
            rx,
        }
    }
}

