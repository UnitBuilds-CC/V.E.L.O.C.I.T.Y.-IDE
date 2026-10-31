//! Use-case segmentation of the tool surface the agent loop advertises.
//!
//! `tool_definitions::get_tools()` is the whole registry — measured at 347 tools
//! and 274,898 characters of name + description + JSON schema, which is ~68,700
//! tokens shipped with *every* turn. A porting dispatch that calls `read_file`,
//! `edit_file` and `run_command` pays for all of it: the three browser sets
//! together are 22.8% of those bytes, the two org sets 22.8%, the two Windows
//! sets 12.8%, and `org_browser` alone — 5 mega-schemas — is 14.1%. The whole
//! file-editing surface is 8,793 bytes, less than a quarter of that one set.
//!
//! On the local models that run dispatched workers at 40-100 tok/s, those tokens
//! are not a rounding error: re-prefilling a ~69,000-token prompt cost 1,520 s
//! (25 minutes) on the 3-tier llama.cpp build, and any change to the front of the
//! prompt invalidates the KV cache and pays it again.
//!
//! So a turn advertises the groups its use case needs and nothing else. The full
//! registry stays available two ways: a narrowed turn can ask for another group
//! with [`REQUEST_TOOL_GROUPS_TOOL`] for the rest of the run, and the MCP-facing
//! `tools/list` (what an external client such as the IDE's own connector sees) is
//! never narrowed here.
//!
//! Membership comes from the definition modules, but only three of them
//! (`generation`, `team`, `drone`+`instances`) are a single use case, so those
//! need no name list. The other three modules hold several at once and are split
//! by explicit tables: `system` by use case, `browser` into reading a page,
//! acting on it and its session plumbing, `wa` into driving an app and controlling
//! the machine, and the UI-verification tools are lifted out of *both* drivers
//! into one set. [`tests`] proves every registered built-in tool lands in exactly
//! one group, so a new tool that is not placed fails the build instead of
//! silently vanishing from a worker's surface.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::tool_definitions::{
    self, browser, company, drone, generation, instances, rules, system, team, wa,
};
use super::types::Tool;

/// A use case: the unit a turn opts into.
///
/// The split is by *what the run is trying to accomplish*, not by which driver
/// implements it. `UiVerify` deliberately spans the browser and the Windows
/// automation modules: recording a session, diffing a snapshot and asserting a
/// label are one job whether the widget is a tab or a Win32 window, and a UI
/// test should not have to opt into `wa_registry_write` or the org-login
/// mega-schema to do it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolGroup {
    /// Reading and writing the workspace: `read_file`, `edit_file`, `list_dir`,
    /// `grep_search`, `index_workspace`, disk-artifact cleanup.
    Files,
    /// Running commands and fetching a repository: `run_command`, `git_clone`,
    /// `connector_call`.
    Shell,
    /// The durable agent state a run reads and writes about itself: checkpoints,
    /// `agent_memory_*`, the codebase event trail, `audit_status`.
    Memory,
    /// Workspace intelligence: the NDA sandbox, test/coverage analysis,
    /// knowledge ingest and search, skills, workflows, panel data.
    Workspace,
    /// The dynamic-registration tools themselves: `tool_register`,
    /// `tool_unregister`, `tool_list_custom`.
    CustomTools,
    /// The rules lifecycle: `rule_create`, `rule_adjust`, `rule_retire`,
    /// `rule_bypass` and the readers. Part of the core floor rather than an
    /// opt-in because the call that repeats a mistake is made by whichever
    /// worker is running — a run that cannot record "never query the live db"
    /// mid-flight has to stop and ask for a group first, which is how a rule
    /// ends up not existing.
    Rules,
    /// Driving the IDE window: `gui_*`, `fetch_panel_data`.
    IdeGui,
    /// Image and video generation, generation-model registry, wiki rendering.
    Media,
    /// Reading a page: extract, brief, links, find, page text, the session
    /// transcript. The half of browser work that never mutates anything.
    WebContent,
    /// Acting on a page: click, fill, type, submit, select, tabs, waits, the
    /// native and runtime action surfaces.
    BrowserInteract,
    /// Browser session and credential plumbing: create/list sessions, auth
    /// profiles, cookie and storage writes, access diagnostics.
    BrowserSessions,
    /// Driving a Windows app: windows, input sequences, UIA lookup, selectors,
    /// dialogs, clipboard, tray, OCR, scripted actions.
    DesktopDrive,
    /// Poking the machine itself rather than a widget: registry, processes,
    /// virtual desktops, scheduled triggers, notifications, system info.
    OsControl,
    /// The UI-automation/verification set across both drivers: record and
    /// replay, checkpoints and snapshots and their diffs, screenshots and
    /// visual captures, saved workflow suites, assertions.
    UiVerify,
    /// Expert teams and dispatch: `team_dispatch`, `run_rewrite_mission`,
    /// `repo_rewrite`, `create_expert_team`.
    Team,
    /// Company org machinery: roles, playbooks, credentials, approvals.
    OrgAdmin,
    /// The five org-browser mega-schemas that plan and drive a browser through
    /// the company org. Kept apart from [`ToolGroup::OrgAdmin`] because those
    /// five are 38,698 bytes — 14.1% of the catalog, four times the whole
    /// file-editing surface — and never needed by a run that only manages roles.
    OrgBrowser,
    /// Remote nodes: `drone_*` and `instance_*`.
    Infra,
}

impl ToolGroup {
    pub const ALL: [ToolGroup; 18] = [
        ToolGroup::Files,
        ToolGroup::Shell,
        ToolGroup::Memory,
        ToolGroup::Workspace,
        ToolGroup::CustomTools,
        ToolGroup::Rules,
        ToolGroup::IdeGui,
        ToolGroup::Media,
        ToolGroup::WebContent,
        ToolGroup::BrowserInteract,
        ToolGroup::BrowserSessions,
        ToolGroup::DesktopDrive,
        ToolGroup::OsControl,
        ToolGroup::UiVerify,
        ToolGroup::Team,
        ToolGroup::OrgAdmin,
        ToolGroup::OrgBrowser,
        ToolGroup::Infra,
    ];

    /// The name a model uses to ask for this group.
    pub fn slug(self) -> &'static str {
        match self {
            ToolGroup::Files => "files",
            ToolGroup::Shell => "shell",
            ToolGroup::Memory => "memory",
            ToolGroup::Workspace => "workspace",
            ToolGroup::CustomTools => "custom_tools",
            ToolGroup::Rules => "rules",
            ToolGroup::IdeGui => "ide",
            ToolGroup::Media => "media",
            ToolGroup::WebContent => "web",
            ToolGroup::BrowserInteract => "browser_interact",
            ToolGroup::BrowserSessions => "browser_sessions",
            ToolGroup::DesktopDrive => "desktop_drive",
            ToolGroup::OsControl => "os_control",
            ToolGroup::UiVerify => "ui_verify",
            ToolGroup::Team => "team",
            ToolGroup::OrgAdmin => "org_admin",
            ToolGroup::OrgBrowser => "org_browser",
            ToolGroup::Infra => "infra",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|g| g.slug() == value)
    }

    /// What the group holds, in the one line the model reads when choosing.
    pub fn blurb(self) -> &'static str {
        match self {
            ToolGroup::Files => "read, write and search workspace files",
            ToolGroup::Shell => "run shell commands, clone repositories",
            ToolGroup::Memory => "checkpoints, agent memory, the codebase event trail",
            ToolGroup::Workspace => {
                "indexing, test and coverage analysis, knowledge, skills, workflows"
            }
            ToolGroup::CustomTools => "register and run custom shell-backed tools",
            ToolGroup::Rules => {
                "create, adjust, retire and bypass the deterministic rules that refuse tool calls"
            }
            ToolGroup::IdeGui => "drive the IDE window and read its panels",
            ToolGroup::Media => "generate images, video and wiki pages",
            ToolGroup::WebContent => "read and extract from a web page without changing it",
            ToolGroup::BrowserInteract => "click, type, submit and wait in a browser page",
            ToolGroup::BrowserSessions => {
                "create browser sessions, saved auth profiles, cookie and storage writes"
            }
            ToolGroup::DesktopDrive => "drive app windows: input, UIA elements, selectors, dialogs",
            ToolGroup::OsControl => "registry, processes, virtual desktops, system triggers",
            ToolGroup::UiVerify => {
                "record and replay a UI run, snapshot and diff it, assert on labels and screenshots"
            }
            ToolGroup::Team => "expert teams and dispatching work to other agents",
            ToolGroup::OrgAdmin => "company org roles, playbooks, credentials, approvals",
            ToolGroup::OrgBrowser => "plan and drive a browser login through the company org",
            ToolGroup::Infra => "remote drones and deployed instances",
        }
    }

    /// The always-on floor of any surface: the file tools, the command runner and
    /// the rules lifecycle. Nothing a dispatched worker can do without them, and
    /// nothing a narrowed surface should ever hide — the artifact gate, the write
    /// lock and the build checks all reach the filesystem through these. `Rules`
    /// is on the floor for the same reason the tools are: the worker that is
    /// about to repeat the mistake is the one that can record the rule that stops
    /// it, and a rule the user asked for cannot wait for a group request.
    fn core() -> [ToolGroup; 3] {
        [ToolGroup::Files, ToolGroup::Shell, ToolGroup::Rules]
    }
}

/// The `system` module defines tools for several use cases at once, so its
/// membership is stated by name rather than inferred from the module.
const SYSTEM_FILES_TOOLS: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "list_dir",
    "grep_search",
    "delete_file",
    "index_workspace",
    "scan_disk_artifacts",
    "clean_disk_artifacts",
];
const SYSTEM_SHELL_TOOLS: &[&str] = &["run_command", "git_clone", "connector_call"];
const SYSTEM_MEMORY_TOOLS: &[&str] = &[
    "agent_checkpoint_create",
    "agent_checkpoint_restore",
    "agent_checkpoint_list",
    "agent_memory_remember",
    "agent_memory_recall",
    "agent_memory_forget",
    "event_record",
    "event_history",
    "event_context",
    "event_mark_outcome",
    "event_timeline",
    "event_attach_context",
    "audit_status",
];
const SYSTEM_CUSTOM_TOOLS: &[&str] = &["tool_register", "tool_unregister", "tool_list_custom"];
const SYSTEM_WORKSPACE_TOOLS: &[&str] = &[
    "convert_to_nda",
    "read_nda",
    "execute_nda",
    "code_generate_tests",
    "code_coverage_analyze",
    "knowledge_ingest",
    "knowledge_search",
    "use_skill",
    "workflow_list",
    "workflow_run",
];

/// The UI-testing automations, across both drivers. This is the set a run asks
/// for when its job is to *prove* a UI still works: record a session, replay it,
/// snapshot the tree, diff against the saved baseline, assert a label, keep the
/// screenshot as evidence. It spans `browser_*` and `wa_*` on purpose — the use
/// case is one thing whichever widget it targets — and it is why the two driver
/// groups no longer imply it: a run driving a form does not need the regression
/// machinery, and a run checking a baseline does not need `wa_registry_write`.
const UI_VERIFY_TOOLS: &[&str] = &[
    // Browser: assertions and the state they compare against.
    "browser_native_assert",
    "browser_native_check_label",
    "browser_native_validate",
    "browser_native_checkpoint",
    "browser_save_checkpoint",
    "browser_read_checkpoint",
    "browser_list_checkpoints",
    "browser_diff_checkpoints",
    "browser_restore_checkpoint",
    "browser_native_screenshot",
    "browser_native_screencast",
    "browser_runtime_capture",
    "browser_runtime_visual_capture",
    "browser_read_visual_fallback",
    "runtime_capture_session",
    "browser_read_snapshot",
    "browser_list_snapshots",
    "browser_diff_snapshots",
    // Recorded flows and suites: the regression harness itself.
    "browser_list_workflows",
    "browser_read_workflow",
    "browser_save_workflow",
    "browser_list_workflow_runs",
    "browser_read_workflow_run",
    "browser_list_workflow_suites",
    "browser_read_workflow_suite",
    "browser_save_workflow_suite",
    "browser_list_workflow_suite_runs",
    "browser_read_workflow_suite_run",
    "browser_replay_workflow",
    "browser_run_workflow_suite",
    // Windows: the same job on a desktop widget.
    "wa_capture_windows_snapshot",
    "wa_platform_tree_snapshot",
    "wa_save_snapshot",
    "wa_read_snapshot",
    "wa_list_snapshots",
    "wa_screenshot",
    "wa_browser_screenshot",
    "wa_record_ingest",
    "wa_record_pause",
    "wa_record_replay_plan",
    "wa_record_start",
    "wa_record_status",
    "wa_record_stop",
    "wa_bridge_run",
    "wa_list_runs",
    "wa_read_run",
    "wa_run_script",
    "wa_save_script",
    "wa_list_scripts",
];

/// Reading a page without changing it.
const WEB_CONTENT_TOOLS: &[&str] = &[
    "web_navigate",
    "browser_native_brief",
    "browser_native_extract",
    "browser_native_find",
    "browser_native_links",
    "browser_native_observe",
    "browser_native_page_text",
    "browser_native_read",
    "browser_native_read_form",
    "browser_native_history",
    "browser_native_network",
    "browser_native_cookies",
    "browser_native_storage",
    "browser_native_export_nda",
    "browser_get_session_network",
    "browser_get_cookies",
    "browser_get_storage",
    "browser_get_trace_logs",
    "browser_get_trace_summary",
    "browser_read_session_transcript",
];

/// Session and credential plumbing: what has to exist before a page can be
/// driven, and the saved logins that let a run get past a wall.
const BROWSER_SESSION_TOOLS: &[&str] = &[
    "browser_create_session",
    "browser_get_session",
    "browser_list_sessions",
    "browser_session_health",
    "browser_read_auth_profile",
    "browser_save_auth_profile",
    "browser_list_auth_profiles",
    "browser_apply_auth_profile",
    "browser_auth_diagnostics",
    "browser_access_diagnostics",
    "browser_reseed_auth",
    "browser_set_cookies",
    "browser_set_storage",
    "browser_set_session_network",
    "runtime_create_session",
    "runtime_close_session",
    "runtime_get_session",
    "runtime_reseed_auth",
];

/// Poking the machine rather than a widget. Deliberately separate from
/// [`ToolGroup::DesktopDrive`]: editing the registry, killing a process tree or
/// moving windows onto another virtual desktop is not something a run should
/// gain just because it asked to click a button.
const OS_CONTROL_TOOLS: &[&str] = &[
    "wa_registry_delete",
    "wa_registry_enumerate",
    "wa_registry_exists",
    "wa_registry_read",
    "wa_registry_write",
    "wa_process_info",
    "wa_process_kill",
    "wa_process_kill_tree",
    "wa_process_launch",
    "wa_process_list",
    "wa_process_running",
    "wa_process_terminate",
    "wa_process_wait",
    "wa_system_dark_mode",
    "wa_system_dpi",
    "wa_platform_info",
    "wa_uac_detect",
    "wa_monitor_list",
    "wa_notifications_dismiss",
    "wa_notifications_list",
    "wa_vdesktop_create",
    "wa_vdesktop_move_window",
    "wa_vdesktop_remove",
    "wa_vdesktop_window_info",
    "wa_virtual_desktop_list",
    "wa_virtual_desktop_switch",
    "wa_trigger_fire",
    "wa_trigger_list",
    "wa_trigger_register",
    "wa_trigger_remove",
];

/// The org-browser mega-schemas, split out of the org machinery because these
/// five are 38,698 bytes of the 62,695 the org family costs, and a run that only
/// needs to add a role or approve an action has no use for a browser driver.
const ORG_BROWSER_TOOLS: &[&str] = &[
    "plan_org_browser_login",
    "plan_org_browser_broadcast",
    "execute_org_browser_task",
    "execute_org_browser_task_for_request",
    "execute_org_browser_broadcast",
];

/// Which group a `browser`-module tool belongs to. Everything the read/session
/// tables do not place is acting on a page, so a newly added `browser_native_*`
/// verb lands in [`ToolGroup::BrowserInteract`] rather than going unplaced.
fn group_of_browser_tool(name: &str) -> ToolGroup {
    if UI_VERIFY_TOOLS.contains(&name) {
        ToolGroup::UiVerify
    } else if WEB_CONTENT_TOOLS.contains(&name) {
        ToolGroup::WebContent
    } else if BROWSER_SESSION_TOOLS.contains(&name) {
        ToolGroup::BrowserSessions
    } else {
        ToolGroup::BrowserInteract
    }
}

/// Which group a `wa`-module tool belongs to; the residual is driving an app.
fn group_of_wa_tool(name: &str) -> ToolGroup {
    if UI_VERIFY_TOOLS.contains(&name) {
        ToolGroup::UiVerify
    } else if OS_CONTROL_TOOLS.contains(&name) {
        ToolGroup::OsControl
    } else {
        ToolGroup::DesktopDrive
    }
}

/// Which group a `company`-module tool belongs to; the residual is org admin.
fn group_of_company_tool(name: &str) -> ToolGroup {
    if ORG_BROWSER_TOOLS.contains(&name) {
        ToolGroup::OrgBrowser
    } else {
        ToolGroup::OrgAdmin
    }
}

/// Which group a `system`-module tool belongs to. Anything the tables above do
/// not place is IDE control (`gui_*`) or media (`describe_image`), which the
/// prefixes state without a 55-entry list; a tool that matches neither is
/// unplaced, and [`unplaced_builtin_tools`] fails the build for it.
fn group_of_system_tool(name: &str) -> Option<ToolGroup> {
    if SYSTEM_FILES_TOOLS.contains(&name) {
        Some(ToolGroup::Files)
    } else if SYSTEM_SHELL_TOOLS.contains(&name) {
        Some(ToolGroup::Shell)
    } else if SYSTEM_MEMORY_TOOLS.contains(&name) {
        Some(ToolGroup::Memory)
    } else if SYSTEM_CUSTOM_TOOLS.contains(&name) {
        Some(ToolGroup::CustomTools)
    } else if SYSTEM_WORKSPACE_TOOLS.contains(&name) {
        Some(ToolGroup::Workspace)
    } else if name.starts_with("gui_") || name == "fetch_panel_data" {
        Some(ToolGroup::IdeGui)
    } else if name == "describe_image" {
        Some(ToolGroup::Media)
    } else {
        None
    }
}

/// Every group whose whole module-based membership is one use case.
fn module_group_tools(group: ToolGroup) -> Vec<Tool> {
    match group {
        ToolGroup::Team => team::get_team_tools(),
        ToolGroup::Rules => rules::get_rules_tools(),
        ToolGroup::Infra => {
            let mut both = drone::get_drone_tools();
            both.extend(instances::get_instance_tools());
            both
        }
        ToolGroup::Media => {
            let mut media = generation::get_generation_tools();
            media.extend(
                system::get_system_tools()
                    .into_iter()
                    .filter(|t| group_of_system_tool(&t.name) == Some(ToolGroup::Media)),
            );
            media
        }
        // The remaining modules carry several use cases at once and are split by
        // name; see `split_group_tools`.
        _ => Vec::new(),
    }
}

/// The module-backed groups, in [`ToolGroup::ALL`] order.
const MODULE_GROUPS: [ToolGroup; 4] = [
    ToolGroup::Rules,
    ToolGroup::Media,
    ToolGroup::Team,
    ToolGroup::Infra,
];

/// The three modules that hold more than one use case, walked tool by tool.
/// `UiVerify` is drawn from both driver modules in this one pass, so a tool can
/// only ever be claimed once — the invariant
/// [`tests::every_builtin_tool_belongs_to_exactly_one_group`] checks.
fn split_group_tools() -> Vec<(ToolGroup, Tool)> {
    let mut out = Vec::new();
    for tool in browser::get_browser_tools() {
        out.push((group_of_browser_tool(&tool.name), tool));
    }
    for tool in wa::get_wa_tools() {
        out.push((group_of_wa_tool(&tool.name), tool));
    }
    for tool in company::get_company_tools() {
        out.push((group_of_company_tool(&tool.name), tool));
    }
    out
}

/// Every built-in tool name with the group that owns it. A name absent from this
/// map is a dynamically registered custom tool, which stays on every surface:
/// segmentation must never hide a tool a human added for the agent to use.
pub fn group_map() -> HashMap<String, ToolGroup> {
    let mut map = HashMap::new();
    for tool in system::get_system_tools() {
        if let Some(group) = group_of_system_tool(&tool.name) {
            map.insert(tool.name, group);
        }
    }
    for (group, tool) in split_group_tools() {
        map.insert(tool.name, group);
    }
    for group in MODULE_GROUPS {
        for tool in module_group_tools(group) {
            map.insert(tool.name, group);
        }
    }
    map
}

/// Built-in tools that no group claims — the drift guard behind
/// [`tests::every_builtin_tool_belongs_to_exactly_one_group`]. A tool the tables
/// do not place would otherwise vanish from every narrowed surface with nothing
/// to show for it.
pub fn unplaced_builtin_tools() -> Vec<String> {
    let map = group_map();
    let mut missing: Vec<String> = tool_definitions::get_tools()
        .iter()
        .map(|t| t.name.clone())
        .filter(|name| !map.contains_key(name))
        // Dynamically registered custom tools are unmapped by design.
        .filter(|name| {
            let root = std::env::current_dir().unwrap_or_default();
            !super::custom_tools::list_tools(&root)
                .iter()
                .any(|t| t.name == *name)
        })
        .collect();
    missing.sort();
    missing
}

/// How many built-in tools a group holds, for the sizes the tests and the
/// surface note quote.
pub fn group_size(group: ToolGroup) -> usize {
    group_map().values().filter(|g| **g == group).count()
}

/// The groups one turn advertises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurface {
    groups: Vec<ToolGroup>,
}

impl ToolSurface {
    pub fn new(groups: impl IntoIterator<Item = ToolGroup>) -> Self {
        let mut picked: Vec<ToolGroup> = Vec::new();
        for group in groups {
            if !picked.contains(&group) {
                picked.push(group);
            }
        }
        // Canonical order, so two runs with the same groups advertise the same
        // array. The tool list sits at the front of the prompt on native-schema
        // providers: an arbitrary order would invalidate the KV prefix between
        // otherwise identical attempts of the same dispatch.
        picked.sort_by_key(|g| {
            ToolGroup::ALL
                .iter()
                .position(|x| x == g)
                .unwrap_or(usize::MAX)
        });
        Self { groups: picked }
    }

    /// The floor every dispatched worker gets, whatever its use case.
    pub fn core() -> Self {
        Self::new(ToolGroup::core())
    }

    /// Everything the registry has: what an interactive IDE turn advertises,
    /// where the task is whatever the human types next.
    pub fn broad() -> Self {
        Self::new(ToolGroup::ALL)
    }

    /// A dispatched code worker: files, commands, and the state it can ask for.
    pub fn code() -> Self {
        Self::new(ToolGroup::core())
    }

    /// A test-writing worker also needs the generator and the coverage reader.
    pub fn code_and_workspace() -> Self {
        Self::new(
            ToolGroup::core()
                .into_iter()
                .chain([ToolGroup::Workspace, ToolGroup::Memory]),
        )
    }

    /// Authoring and consulting expert teams: the dispatch and team-definition
    /// tools plus the files a team is described in. Deliberately not
    /// [`ToolSurface::broad`] — the org-browser and remote-node families stay
    /// behind `request_tool_groups` even here.
    pub fn team_work() -> Self {
        Self::new(ToolGroup::core().into_iter().chain([
            ToolGroup::Memory,
            ToolGroup::Workspace,
            ToolGroup::Team,
        ]))
    }

    /// The interactive IDE surface: everything a local session does without the
    /// web, browser, desktop, OS-control, org and remote-node machinery and
    /// without the team dispatch machinery — the families that together are 86%
    /// of the catalog's bytes (237,317 of 274,898) and that a chat turn only
    /// reaches for when the human asks for them. One `request_tool_groups` call
    /// adds any of them, by the narrowest slug that fits, for the rest of the run.
    pub fn interactive() -> Self {
        Self::new([
            ToolGroup::Files,
            ToolGroup::Shell,
            ToolGroup::Memory,
            ToolGroup::Workspace,
            ToolGroup::CustomTools,
            ToolGroup::Rules,
            ToolGroup::IdeGui,
            ToolGroup::Media,
        ])
    }

    /// A UI-automation run: drive a window, and prove what happened. The
    /// verification set is the reason this surface exists — 49 tools and 35,556
    /// bytes that the old `desktop` family buried among registry writes, process
    /// kills and virtual-desktop switches, none of which a run needs in order to
    /// assert that a dialog still says "Save". Web work is one
    /// `request_tool_groups {"groups": ["web", "browser_interact"]}` call away,
    /// and so are the saved credentials (`browser_sessions`) and the machine
    /// itself (`os_control`) — each far more than a test run should be handed on
    /// trust.
    pub fn ui_test() -> Self {
        Self::new(ToolGroup::core().into_iter().chain([
            ToolGroup::IdeGui,
            ToolGroup::DesktopDrive,
            ToolGroup::UiVerify,
        ]))
    }

    /// The surface an orchestrator worker should run with, from the task kind the
    /// router assigned. Unknown kinds get the core floor: an unplaced task is
    /// still a task about files, and asking for more is one call away.
    pub fn for_task_kind(kind: &str) -> Self {
        match kind {
            "desktop_automation" => Self::ui_test(),
            "test" => Self::code_and_workspace(),
            "analysis" | "planning" => Self::new(
                ToolGroup::core()
                    .into_iter()
                    .chain([ToolGroup::Memory, ToolGroup::Workspace]),
            ),
            "documentation" => Self::new(ToolGroup::core().into_iter().chain([ToolGroup::Memory])),
            _ => Self::code(),
        }
    }

    pub fn groups(&self) -> &[ToolGroup] {
        &self.groups
    }

    pub fn contains(&self, group: ToolGroup) -> bool {
        self.groups.contains(&group)
    }

    /// True when at least one group is withheld, i.e. when the escape-hatch tool
    /// has something to offer.
    pub fn is_narrow(&self) -> bool {
        self.groups.len() < ToolGroup::ALL.len()
    }

    /// Add groups for the rest of the run. Returns the ones that were actually
    /// new, so the tool result can tell the model what it gained rather than
    /// repeating itself.
    pub fn request(&mut self, wanted: &[ToolGroup]) -> Vec<ToolGroup> {
        let mut added = Vec::new();
        for group in wanted.iter().copied() {
            if !self.groups.contains(&group) {
                self.groups.push(group);
                added.push(group);
            }
        }
        if !added.is_empty() {
            // Re-sort to keep the canonical order the note above promises.
            self.groups.sort_by_key(|g| {
                ToolGroup::ALL
                    .iter()
                    .position(|x| *x == *g)
                    .unwrap_or(usize::MAX)
            });
        }
        added
    }

    /// The group that withholds `name` from this surface, or `None` when the
    /// surface advertises it. A name with no group — a custom tool registered at
    /// runtime — is never withheld, so segmentation cannot hide what a human
    /// added for the agent to use. Consulted live, so a tool registered after
    /// this turn's schema array was built is still callable.
    pub fn withholds(&self, name: &str) -> Option<ToolGroup> {
        let group = *group_map().get(name)?;
        if self.groups.contains(&group) {
            None
        } else {
            Some(group)
        }
    }

    pub fn hidden_groups(&self) -> Vec<ToolGroup> {
        ToolGroup::ALL
            .iter()
            .copied()
            .filter(|g| !self.groups.contains(g))
            .collect()
    }

    /// Slugs of the withheld groups, in canonical order — the list a schema-less
    /// model reads to know what it can ask for.
    pub fn hidden_slugs(&self) -> String {
        self.hidden_groups()
            .iter()
            .map(|g| g.slug())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The tools this surface advertises, in registry order. A tool with no
    /// group — a custom tool registered at runtime — is always included.
    pub fn tools(&self) -> Vec<Tool> {
        self.select_from(&tool_definitions::get_tools())
            .into_iter()
            .cloned()
            .collect()
    }

    /// The same selection drawn from a registry read the caller already holds,
    /// so a loop that rebuilds its schema array every turn does not pay for the
    /// registry twice.
    pub fn select_from<'a>(&self, registered: &'a [Tool]) -> Vec<&'a Tool> {
        let map = group_map();
        registered
            .iter()
            .filter(|tool| match map.get(&tool.name) {
                Some(group) => self.groups.contains(group),
                None => true,
            })
            .collect()
    }

    /// Names of the tools a group contributes to *this* surface, for the
    /// confirmation the escape-hatch tool returns.
    pub fn tool_names_in(&self, group: ToolGroup) -> Vec<String> {
        let map = group_map();
        self.tools()
            .iter()
            .filter(|t| map.get(&t.name) == Some(&group))
            .map(|t| t.name.clone())
            .collect()
    }

    pub fn slug_list(&self) -> String {
        self.groups
            .iter()
            .map(|g| g.slug())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Default for ToolSurface {
    /// A surface nobody configured is the interactive one: the widest set that
    /// still leaves the automation, org and dispatch families out, rather than a
    /// silent `core()` that strands a chat turn.
    fn default() -> Self {
        ToolSurface::interactive()
    }
}

/// The escape hatch advertised on every narrowed surface.
pub const REQUEST_TOOL_GROUPS_TOOL: &str = "request_tool_groups";

/// The schema of the escape hatch, naming the groups this surface withheld so
/// the model can ask for the right one. `items` is a schema object with its
/// `enum` inside it — the shape llama.cpp's tool-grammar compiler demands; a bare
/// `"items": [...]` makes it 500 the whole request.
pub fn request_tool_groups_schema(surface: &ToolSurface) -> Value {
    let slugs: Vec<&str> = ToolGroup::ALL.iter().map(|g| g.slug()).collect();
    json!({
        "type": "function",
        "function": {
            "name": REQUEST_TOOL_GROUPS_TOOL,
            "description": format!(
                "This turn advertises only the tool groups its task needs. Call this with \
                 another group's name to get its tools for the rest of the run; they are then \
                 callable on the very next turn. Hidden groups: {}.",
                surface.hidden_slugs()
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "groups": {
                        "type": "array",
                        "items": { "type": "string", "enum": slugs },
                        "description": "Which groups to add."
                    }
                },
                "required": ["groups"]
            }
        }
    })
}

/// The same control stated in prose, for the inline-catalog path where the model
/// receives no schema array at all.
pub fn request_tool_groups_prose(hidden_slugs: &str) -> String {
    format!(
        "### {REQUEST_TOOL_GROUPS_TOOL}\n\
         This turn advertises only the tool groups its task needs. Call it with another \
         group's name to get those tools for the rest of the run.\n\
         \x20 - `groups` (array, required): which groups to add. Not advertised now: \
         {hidden_slugs}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_tool_belongs_to_exactly_one_group() {
        // The premise of this whole module: a turn that advertises a subset can
        // only be trusted if the subset is a *partition* of the registry. A tool
        // the system tables forget would disappear from every dispatched worker
        // with nothing failing to say so; a tool two families claim makes
        // `group_map`'s answer depend on insertion order. Both are caught here.
        assert!(
            unplaced_builtin_tools().is_empty(),
            "registered built-in tools no group claims: {:?}",
            unplaced_builtin_tools()
        );
        let mut claimed: Vec<String> = Vec::new();
        let mut collisions: Vec<String> = Vec::new();
        let claim = |name: String, claimed: &mut Vec<String>, collisions: &mut Vec<String>| {
            if claimed.contains(&name) {
                collisions.push(name);
            } else {
                claimed.push(name);
            }
        };
        for tool in system::get_system_tools() {
            claim(tool.name, &mut claimed, &mut collisions);
        }
        // The three multi-use-case modules are walked tool by tool. Their tables
        // are read in precedence order, so a name listed twice inside `browser`
        // would be resolved silently rather than colliding; the disjointness
        // assert in `split_tables_do_not_overlap` is what catches that.
        for (_group, tool) in split_group_tools() {
            claim(tool.name, &mut claimed, &mut collisions);
        }
        for group in MODULE_GROUPS {
            for tool in module_group_tools(group) {
                // `Media` re-reads the system module for `describe_image`, so its
                // system-sourced half was already claimed above.
                if group != ToolGroup::Media || group_of_system_tool(&tool.name).is_none() {
                    claim(tool.name, &mut claimed, &mut collisions);
                }
            }
        }
        assert!(
            collisions.is_empty(),
            "tool named by two families: {collisions:?}"
        );
        let map = group_map();
        assert_eq!(
            map.len(),
            claimed.len(),
            "the map and the families disagree: {} mapped, {} claimed",
            map.len(),
            claimed.len()
        );
        for group in ToolGroup::ALL {
            assert!(group_size(group) > 0, "{:?} is empty", group);
        }
    }

    #[test]
    fn surfaces_partition_the_registry_without_hiding_custom_tools() {
        let broad = ToolSurface::broad();
        let registry = tool_definitions::get_tools();
        assert_eq!(
            broad.tools().len(),
            registry.len(),
            "the broad surface must be the registry itself"
        );
        // The mapped groups tile the built-ins: every mapped name is reachable
        // from exactly one single-group surface.
        let map = group_map();
        let mut seen: Vec<String> = Vec::new();
        for group in ToolGroup::ALL {
            for tool in ToolSurface::new([group]).tools() {
                if map.contains_key(&tool.name) {
                    assert!(
                        !seen.contains(&tool.name),
                        "{} reachable from two groups",
                        tool.name
                    );
                    seen.push(tool.name);
                }
            }
        }
        assert_eq!(seen.len(), map.len(), "a mapped tool reaches no surface");
        // What is left over is a custom tool, and those are on every surface:
        // segmentation must not hide a tool a human registered for the agent.
        let root = std::env::current_dir().unwrap_or_default();
        for tool in registry.iter().filter(|t| !map.contains_key(&t.name)) {
            assert!(
                super::super::custom_tools::get_tool(&root, &tool.name).is_some(),
                "{} is in the registry but in no group and not a custom tool",
                tool.name
            );
            assert!(ToolSurface::code()
                .tools()
                .iter()
                .any(|t| t.name == tool.name));
        }
    }

    #[test]
    fn the_editing_tools_a_worker_cannot_do_without_are_always_on() {
        for surface in [
            ToolSurface::core(),
            ToolSurface::code(),
            ToolSurface::code_and_workspace(),
            ToolSurface::interactive(),
            ToolSurface::ui_test(),
            ToolSurface::for_task_kind("desktop_automation"),
            ToolSurface::for_task_kind("planning"),
        ] {
            let names: Vec<String> = surface.tools().iter().map(|t| t.name.clone()).collect();
            for needed in [
                "read_file",
                "write_file",
                "edit_file",
                "list_dir",
                "grep_search",
            ] {
                assert!(
                    names.iter().any(|n| n == needed),
                    "{needed} missing from a surface"
                );
            }
        }
    }

    #[test]
    fn the_far_families_are_off_a_code_workers_surface() {
        // This is the whole point of the module, measured: the automation, org
        // and dispatch families are the great majority of the catalog's bytes. If
        // one of them reappears in the code surface the byte cost returns with it.
        //
        // The samples are checked against `group_map()` first because the assert
        // below is an absence: a renamed tool would make this pass while telling
        // the reader nothing, which is exactly how `browser_navigate` and
        // `wa_click` — names that never existed — sat in this list green.
        let surface = ToolSurface::code();
        let names: Vec<String> = surface.tools().iter().map(|t| t.name.clone()).collect();
        let map = group_map();
        for (absent, family) in [
            ("web_navigate", ToolGroup::WebContent),
            ("browser_session_click", ToolGroup::BrowserInteract),
            ("browser_native_assert", ToolGroup::UiVerify),
            ("wa_create_session", ToolGroup::DesktopDrive),
            ("wa_registry_write", ToolGroup::OsControl),
            ("team_dispatch", ToolGroup::Team),
            ("plan_org_browser_login", ToolGroup::OrgBrowser),
            ("add_org_role", ToolGroup::OrgAdmin),
            ("drone_deploy", ToolGroup::Infra),
            ("generate_image", ToolGroup::Media),
        ] {
            assert_eq!(
                map.get(absent),
                Some(&family),
                "{absent} is not the registered member of {family:?} it samples"
            );
            assert!(
                !names.iter().any(|n| n == absent),
                "{absent} is advertised to a code worker"
            );
        }
        for present in ["read_file", "run_command"] {
            assert!(names.iter().any(|n| n == present));
        }
    }

    #[test]
    fn split_tables_do_not_overlap() {
        // The split classifiers read these tables in precedence order, so a name
        // listed twice would resolve to whichever table comes first and nothing
        // would say so. The absence checks in the partition test cannot see it
        // (`split_group_tools` still yields the tool once), so compare the tables
        // directly.
        let mut seen: Vec<&str> = Vec::new();
        for table in [
            UI_VERIFY_TOOLS,
            WEB_CONTENT_TOOLS,
            BROWSER_SESSION_TOOLS,
            OS_CONTROL_TOOLS,
            ORG_BROWSER_TOOLS,
        ] {
            for name in table.iter() {
                assert!(!seen.contains(name), "{name} is placed by two split tables");
                seen.push(*name);
            }
        }
    }

    #[test]
    fn one_set_covers_ui_testing_across_both_drivers() {
        // The point of lifting the verification tools out of `browser` and `wa`:
        // a run whose job is to prove a UI still works asks for one group and
        // gets the recorder, the baseline diff and the assertion from whichever
        // driver the widget lives behind — and nothing else those modules hold.
        let map = group_map();
        for sampled in [
            "browser_native_assert",
            "browser_run_workflow_suite",
            "browser_diff_snapshots",
            "wa_record_replay_plan",
            "wa_save_snapshot",
            "wa_bridge_run",
        ] {
            assert_eq!(
                map.get(sampled),
                Some(&ToolGroup::UiVerify),
                "{sampled} is UI verification"
            );
        }
        let surface = ToolSurface::ui_test();
        assert!(surface.contains(ToolGroup::UiVerify));
        assert!(surface.contains(ToolGroup::DesktopDrive));
        // …and the surfaces that are *not* testing stay off it. `wa_registry_write`
        // and `wa_process_kill` used to arrive free with any desktop automation;
        // the org-login mega-schema is 11,883 bytes a UI test never reads.
        for absent in [
            "wa_registry_write",
            "wa_process_kill_tree",
            "plan_org_browser_login",
            "browser_save_auth_profile",
            "team_dispatch",
        ] {
            assert!(
                surface.withholds(absent).is_some(),
                "{absent} is advertised to a UI test run"
            );
        }
        // The browser half of verification is on the same group, so a web UI test
        // reaches it by asking for one slug rather than the whole 109-tool family.
        assert!(
            ToolSurface::new([ToolGroup::Files, ToolGroup::Shell, ToolGroup::UiVerify])
                .tools()
                .iter()
                .any(|t| t.name == "browser_native_assert")
        );
        // Reachable by name, or the split only costs the run a group it cannot get.
        assert_eq!(ToolGroup::parse("ui_verify"), Some(ToolGroup::UiVerify));
        assert!(ToolSurface::code().hidden_slugs().contains("ui_verify"));
    }

    #[test]
    fn a_desktop_automation_worker_is_not_handed_the_machine() {
        // Separating the automations is also a safety split: asking to drive a
        // window must not grant registry writes, process kills or a switch to
        // another virtual desktop.
        let surface = ToolSurface::for_task_kind("desktop_automation");
        assert!(surface.contains(ToolGroup::DesktopDrive));
        assert!(surface.contains(ToolGroup::UiVerify));
        assert!(!surface.contains(ToolGroup::OsControl));
        assert_eq!(
            surface.withholds("wa_registry_write"),
            Some(ToolGroup::OsControl)
        );
        assert_eq!(
            surface.withholds("wa_virtual_desktop_switch"),
            Some(ToolGroup::OsControl)
        );
    }

    #[test]
    fn a_segmented_surface_is_a_fraction_of_the_full_catalog() {
        let shipped = |tools: &[Tool]| -> usize {
            tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema
                        }
                    })
                    .to_string()
                    .len()
                })
                .sum()
        };
        let full = shipped(&tool_definitions::get_tools());
        let code = shipped(&ToolSurface::code().tools());
        let interactive = shipped(&ToolSurface::interactive().tools());
        // ~4 chars/token on these schemas: the full catalog measured 274,898
        // characters against a ~68,700-token prompt.
        assert!(
            code * 12 < full,
            "a code worker pays {code} of {full} chars; the segmentation must \
             cut the catalog to under a twelfth"
        );
        assert!(
            interactive * 3 < full,
            "the interactive surface pays {code} of {full} chars"
        );
    }

    #[test]
    fn group_order_is_canonical_so_the_advertised_array_is_stable() {
        let forwards = ToolSurface::new([ToolGroup::Shell, ToolGroup::Files, ToolGroup::Memory]);
        let backwards = ToolSurface::new([ToolGroup::Memory, ToolGroup::Files, ToolGroup::Shell]);
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.slug_list(), "files, shell, memory");
    }

    #[test]
    fn requesting_a_group_only_reports_what_was_new() {
        let mut surface = ToolSurface::code();
        let added = surface.request(&[ToolGroup::UiVerify, ToolGroup::Files]);
        assert_eq!(added, vec![ToolGroup::UiVerify], "already-on group echoed");
        assert!(surface.contains(ToolGroup::UiVerify));
        assert_eq!(
            surface.groups(),
            &[
                ToolGroup::Files,
                ToolGroup::Shell,
                ToolGroup::Rules,
                ToolGroup::UiVerify
            ][..]
        );
        assert_eq!(
            surface.request(&[ToolGroup::UiVerify]),
            Vec::new(),
            "a repeat request must not claim a second gain"
        );
    }

    #[test]
    fn a_narrow_surface_admits_the_escape_hatch_and_a_broad_one_does_not() {
        assert!(ToolSurface::code().is_narrow());
        assert!(ToolSurface::interactive().is_narrow());
        assert!(ToolSurface::ui_test().is_narrow());
        assert!(!ToolSurface::broad().is_narrow());
        assert_eq!(ToolSurface::broad().hidden_slugs(), "");
        assert!(ToolSurface::code()
            .hidden_slugs()
            .contains("browser_interact"));
    }

    #[test]
    fn the_escape_hatch_lists_every_group_and_names_what_is_hidden() {
        let surface = ToolSurface::code();
        let schema = request_tool_groups_schema(&surface);
        let enum_values = schema["function"]["parameters"]["properties"]["groups"]["items"]["enum"]
            .as_array()
            .expect("items must be a schema object carrying the enum");
        for group in ToolGroup::ALL {
            assert!(
                enum_values.iter().any(|v| v.as_str() == Some(group.slug())),
                "{} unreachable by name",
                group.slug()
            );
        }
        // Strict backends reject a non-object `items` for the *whole* request, so
        // this schema is only legal while `items` stays an object.
        assert!(schema["function"]["parameters"]["properties"]["groups"]["items"].is_object());
        // The description is the only place a model learns what it is missing.
        let description = schema["function"]["description"].as_str().unwrap();
        assert!(description.contains("ui_verify"), "{description}");
        assert!(
            !description.contains(", files,"),
            "an already-advertised group listed as hidden"
        );
    }

    #[test]
    fn task_kinds_reach_the_surfaces_their_work_needs() {
        let automation = ToolSurface::for_task_kind("desktop_automation");
        assert!(automation.contains(ToolGroup::DesktopDrive));
        assert!(automation.contains(ToolGroup::UiVerify));
        assert!(ToolSurface::for_task_kind("test").contains(ToolGroup::Workspace));
        assert!(!ToolSurface::for_task_kind("refactor").contains(ToolGroup::Workspace));
        // An unrecognised kind still gets the floor rather than nothing.
        assert!(ToolSurface::for_task_kind("wat").contains(ToolGroup::Files));
        assert!(ToolSurface::for_task_kind("").is_narrow());
    }

    #[test]
    fn every_dispatchable_task_kind_ships_a_minority_of_the_registry() {
        // Regression for the gap this module exists to close: a worker dispatched
        // by the orchestrator used to build its schema array from
        // `get_tools()` — all 347 tools, ~68,000 tokens — on every turn. The
        // surface is chosen from `assignment.task_kind`, so the reach of that
        // bug is exactly the set of kinds the orchestrator can hand out. Pinning
        // the whole enum (not a sample) means a new kind that falls through to
        // `broad()` fails here instead of silently re-inflating every prompt.
        let registry = tool_definitions::get_tools();
        for kind in crate::automation::AgentTaskKind::ALL {
            let surface = ToolSurface::for_task_kind(kind.as_str());
            let advertised = surface.select_from(&registry).len();
            assert!(
                surface.is_narrow(),
                "{:?} dispatches a worker with the whole catalog",
                kind
            );
            assert!(
                advertised * 3 < registry.len() * 2,
                "{:?} advertises {advertised} of {} tools — over two thirds of a prompt that \
                 gets re-sent and re-prefilled every turn",
                kind,
                registry.len()
            );
            // Whatever the kind, a worker that changes files needs the floor.
            assert!(
                surface.contains(ToolGroup::Files) && surface.contains(ToolGroup::Shell),
                "{:?} cannot edit or build",
                kind
            );
        }
    }
}
