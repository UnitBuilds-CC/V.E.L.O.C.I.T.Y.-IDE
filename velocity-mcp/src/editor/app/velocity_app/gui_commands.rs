//! GUI command processing — executes commands received from external processes
//! (MCP server, AI agents) on the main UI thread.

use super::struct_def::VelocityApp;
use crate::editor::app::app_map::{
    command_is_interactive, command_risk, edge_json, mode_command_label, profile_from_label,
    rail_from_name, split_section_id, sub_tab, AppMap, CommandRisk, CommandSpec, MapNode, NodeKind,
    RAILS, ROOT,
};
use crate::editor::app::types::{
    activity_category_index, activity_category_name, bridge_panel_names, panel_kind_from_name,
    panel_slug_for_kind, Command, Tab, TabKind,
};
use crate::editor::app::velocity_app::actions::fuzzy_subsequence;
use crate::editor::gui_control::{GuiCommand, GuiResponse, IdeState};
use crate::editor::theme::WorkspaceProfile;
use eframe::egui;

/// Shorthand for the refusal shape every guarded path returns.
fn refusal(error: impl Into<String>) -> GuiResponse {
    GuiResponse {
        success: false,
        data: None,
        error: Some(error.into()),
    }
}

fn accepted(data: serde_json::Value) -> GuiResponse {
    GuiResponse {
        success: true,
        data: Some(data),
        error: None,
    }
}

impl VelocityApp {
    /// Process any pending GUI control commands from external processes.
    /// Called at the start of each egui frame.
    /// `ctx` is used for viewport-level actions (e.g. closing the window on
    /// a remote quit request).
    pub fn process_gui_commands(&mut self, ctx: &egui::Context) {
        // Take the receiver out temporarily to satisfy the borrow checker.
        let rx = match self.gui_cmd_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        // Process all pending commands (non-blocking).
        while let Ok((cmd, resp_tx)) = rx.try_recv() {
            let response = self.execute_gui_command(cmd, ctx);
            let _ = resp_tx.send(response);
        }

        // Put the receiver back.
        self.gui_cmd_rx = Some(rx);
    }

    /// Execute a single GUI command and return the response.
    fn execute_gui_command(&mut self, cmd: GuiCommand, ctx: &egui::Context) -> GuiResponse {
        match cmd {
            GuiCommand::OpenFile { path } => self.cmd_open_file(path),
            GuiCommand::GetState {} => self.cmd_get_state(),
            GuiCommand::NavigatePanel { panel } => self.cmd_navigate_panel(panel),
            GuiCommand::TogglePanel { panel } => self.cmd_toggle_panel(panel),
            GuiCommand::Screenshot { path } => self.cmd_screenshot(path),
            GuiCommand::Quit {} => self.cmd_quit(ctx),
            GuiCommand::ListCommands { category } => self.cmd_list_commands(category),
            GuiCommand::RunCommand {
                label,
                allow_unsafe,
            } => self.cmd_run_command(label, allow_unsafe.unwrap_or(false)),
            GuiCommand::AppMap { from, to } => self.cmd_app_map(from, to),
            GuiCommand::NavigateTo { target } => self.cmd_navigate_to(target),
            GuiCommand::ListTabs {} => self.cmd_list_tabs(),
            GuiCommand::SelectTab { tab } => self.cmd_select_tab(tab),
            GuiCommand::SelectSubTab { rail, sub_tab } => self.cmd_select_sub_tab(rail, sub_tab),
        }
    }

    /// Open a file in the editor.
    /// Validates the path is absolute, resolves symlinks, and ensures it's
    /// within the workspace root.
    fn cmd_open_file(&mut self, path: String) -> GuiResponse {
        // Validate path security: must be absolute, within workspace, no symlink escapes
        let validated_path =
            match crate::editor::gui_control::validate_open_path(&path, &self.workspace_root) {
                Ok(p) => p,
                Err(e) => {
                    return GuiResponse {
                        success: false,
                        data: None,
                        error: Some(e),
                    };
                }
            };

        self.open_editor(Some(validated_path.clone()));
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({ "opened": validated_path.display().to_string() })),
            error: None,
        }
    }

    /// Get current IDE state.
    fn cmd_get_state(&mut self) -> GuiResponse {
        accepted(serde_json::to_value(self.ide_state()).unwrap_or_default())
    }

    /// Snapshot of what the frame would show. `GetState` returns this on its
    /// own, and every mutating command embeds it as `state_after` so a driver
    /// asserts against the same shape rather than reconstructing one per call.
    pub(super) fn ide_state(&self) -> IdeState {
        let open_files: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                if let TabKind::Editor {
                    path: Some(ref p), ..
                } = tab.kind
                {
                    Some(p.display().to_string())
                } else {
                    None
                }
            })
            .collect();

        let active_file = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
            .and_then(|tab| {
                if let TabKind::Editor {
                    path: Some(ref p), ..
                } = tab.kind
                {
                    Some(p.display().to_string())
                } else {
                    None
                }
            });

        // The rail categories, from the one table the render loop also reads.
        // This used to be a private 8-name array restated in two commands, which
        // drifted from `ALL_PANELS` the moment a panel was added.
        let rail = self.activity_bar_selection;
        let active_panel = activity_category_name(rail).to_string();
        // The sub-tab the rail is actually showing, read the same clamped way
        // `render_rail_tabs` reads it, so a stale restored index reports the
        // section the user sees rather than one that does not exist.
        let active_section =
            sub_tab(rail, self.activity_sub_panel[rail]).map(|s| s.label.to_string());

        // What the central area is actually showing. Read the same way the frame
        // reads it, so a driver can tell "the tab exists and is drawing" apart
        // from "the tab exists but the welcome screen owns the panel".
        let focused_tab =
            crate::editor::app::types::focused_tab(&self.tabs, self.active_tab.as_ref())
                .map(|tab| tab.title());
        let central_area = if self.central_shows_dock() {
            "dock"
        } else {
            "welcome"
        }
        .to_string();

        IdeState {
            open_files,
            active_file,
            active_panel,
            workspace_root: self.workspace_root.display().to_string(),
            sidebar_visible: self.left_sidebar_visible,
            chat_message_count: self.chat.messages.len(),
            git_branch: if self.git_state.branch.is_empty() {
                None
            } else {
                Some(self.git_state.branch.clone())
            },
            focused_tab,
            central_area,
            active_section,
            mode: self.appearance.profile.label().to_string(),
        }
    }

    /// Open (or, if already focused, close) a central panel tab. Goes through
    /// `toggle_panel` -- the identical entry point the activity-bar gear, the
    /// menu item, Ctrl+, and the status-bar provider chip use -- so a driver
    /// reaching Settings by this route exercises the same code a click does.
    fn cmd_toggle_panel(&mut self, panel: String) -> GuiResponse {
        let kind = match panel_kind_from_name(&panel) {
            Some(kind) => kind,
            None => {
                return GuiResponse {
                    success: false,
                    data: None,
                    error: Some(format!(
                        "Unknown panel '{panel}'. Valid: {:?}",
                        bridge_panel_names()
                    )),
                };
            }
        };

        self.toggle_panel(kind);
        // Report what the central area ended up showing: opening can equally
        // have toggled the tab closed, and the caller should not have to guess.
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({
                "panel": panel,
                "focused_tab": crate::editor::app::types::focused_tab(
                    &self.tabs,
                    self.active_tab.as_ref()
                )
                .map(|tab| tab.title()),
                "central_area": if self.central_shows_dock() {
                    "dock"
                } else {
                    "welcome"
                },
            })),
            error: None,
        }
    }

    /// Select an activity bar rail. Takes a category name (`files`, `git`,
    /// ...) not a dock panel slug -- use [`Self::cmd_toggle_panel`] or
    /// `gui_navigate_to` for the panels inside the centre.
    fn cmd_navigate_panel(&mut self, panel: String) -> GuiResponse {
        let idx = crate::editor::app::types::activity_category_index(&panel);
        match idx {
            Some(i) => {
                self.activity_bar_selection = i;
                self.left_sidebar_visible = true;
                GuiResponse {
                    success: true,
                    data: Some(serde_json::json!({
                        "panel": crate::editor::app::types::ACTIVITY_CATEGORY_NAMES[i],
                        "categories": crate::editor::app::types::ACTIVITY_CATEGORY_NAMES,
                    })),
                    error: None,
                }
            }
            None => GuiResponse {
                success: false,
                data: None,
                error: Some(format!(
                    "Unknown activity category '{panel}'. Valid: {:?}",
                    crate::editor::app::types::ACTIVITY_CATEGORY_NAMES
                )),
            },
        }
    }

    /// Capture a screenshot and save to disk.
    fn cmd_screenshot(&mut self, _path: String) -> GuiResponse {
        // egui doesn't have built-in screenshot capture from the app side.
        // This would require the egui Context to capture the next frame.
        // For now, return a not-implemented response.
        GuiResponse {
            success: false,
            data: None,
            error: Some("Screenshot capture not yet implemented".into()),
        }
    }

    /// Quit the IDE. Sends a viewport Close command through the egui
    /// context, which drives the normal window-close flow — eframe still
    /// calls `on_exit`, so workspace preferences are saved.
    fn cmd_quit(&mut self, ctx: &egui::Context) -> GuiResponse {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        GuiResponse {
            success: true,
            data: Some(serde_json::json!({ "quitting": true })),
            error: None,
        }
    }

    // ─── Command surface ───────────────────────────────────────────────────

    /// Every palette entry, with the tier the bridge will hold it to. Derived
    /// from [`VelocityApp::commands`] rather than a parallel list, which is how
    /// the old hardcoded activity-name arrays drifted: a command added to the
    /// palette becomes drivable, enumerable and covered by the inventory tests
    /// with no second place to update.
    pub(crate) fn command_inventory(&self) -> Vec<CommandSpec> {
        self.commands()
            .iter()
            .map(|cmd| CommandSpec {
                label: cmd.label.to_string(),
                category: cmd.category.to_string(),
                shortcut: cmd.shortcut.map(str::to_string),
                risk: command_risk(cmd.category, cmd.label),
                interactive: command_is_interactive(cmd.label),
            })
            .collect()
    }

    /// Enumerate the command palette. With no filter this is the whole surface a
    /// driver is claiming to have covered, so the per-tier totals are part of
    /// the answer: `execute` entries are the ones a sweep must opt in to run.
    fn cmd_list_commands(&mut self, category: Option<String>) -> GuiResponse {
        let all = self.command_inventory();
        let filter = category
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty());

        let entries: Vec<&CommandSpec> = match &filter {
            Some(want) => all
                .iter()
                .filter(|cmd| cmd.category.to_ascii_lowercase() == *want)
                .collect(),
            None => all.iter().collect(),
        };

        let mut categories: Vec<(&str, usize)> = Vec::new();
        let mut tiers = std::collections::BTreeMap::new();
        for spec in &all {
            match categories
                .iter_mut()
                .find(|(name, _)| *name == spec.category.as_str())
            {
                Some(entry) => entry.1 += 1,
                None => categories.push((spec.category.as_str(), 1)),
            }
            *tiers.entry(spec.risk.as_str()).or_insert(0) += 1;
        }

        accepted(serde_json::json!({
            "count": entries.len(),
            "filter": filter,
            "categories": categories.iter().map(|(name, n)| serde_json::json!({
                "name": name, "commands": n
            })).collect::<Vec<_>>(),
            "tiers": tiers,
            "commands": entries.iter().map(|spec| serde_json::json!({
                "label": spec.label,
                "category": spec.category,
                "shortcut": spec.shortcut,
                "risk": spec.risk.as_str(),
                "interactive": spec.interactive,
            })).collect::<Vec<_>>(),
        }))
    }

    /// Invoke a palette entry by label. This is the one bridge call that can run
    /// arbitrary app code, so it carries two gates: the risk tier (a driver has
    /// to opt in to anything that writes, spawns or opens a dialog) and the mode
    /// filter the palette itself applies, so the bridge cannot reach a control
    /// the current workspace profile deliberately hid.
    fn cmd_run_command(&mut self, label: String, allow_unsafe: bool) -> GuiResponse {
        let commands = self.commands();
        let cmd = match find_command(&commands, &label) {
            Some(cmd) => cmd,
            None => return refusal(unknown_command_message(&commands, &label)),
        };
        let profile = self.appearance.profile;
        if let Err(reason) = command_gate(cmd, profile, allow_unsafe) {
            return refusal(reason);
        }
        // Copy the fn pointer out so the palette borrow ends before the action
        // runs -- every action takes `&mut VelocityApp`.
        let action = cmd.action;
        let (label, category, shortcut) = (cmd.label, cmd.category, cmd.shortcut);
        action(self);
        accepted(serde_json::json!({
            "command": label,
            "category": category,
            "shortcut": shortcut,
            "risk": command_risk(category, label).as_str(),
            "interactive": command_is_interactive(label),
            "status_message": self.status_message,
            "state_after": self.ide_state(),
        }))
    }

    /// Report the navigable graph, or the shortest route across it.
    fn cmd_app_map(&mut self, from: Option<String>, to: Option<String>) -> GuiResponse {
        let mut map = AppMap::build();
        map.add_commands(&self.command_inventory());

        let to = to.filter(|t| !t.trim().is_empty());
        let from = from.filter(|f| !f.trim().is_empty());

        if let Some(goal) = to.clone() {
            let (goal_id, label) = match map.resolve(&goal) {
                Some(node) => (node.id.clone(), node.label.clone()),
                None => {
                    return refusal(format!(
                        "Nothing in the app map matches '{goal}'. Try a rail slug \
                         ({:?}), a panel slug, or gui_list_commands labels.",
                        RAILS.iter().map(|rail| rail.slug).collect::<Vec<_>>()
                    ))
                }
            };
            let start_id = match from.as_deref().or(Some(ROOT)) {
                Some(name) => match map.resolve(name) {
                    Some(node) => node.id.clone(),
                    None => return refusal(format!("Nothing in the app map matches '{name}'.")),
                },
                None => ROOT.to_string(),
            };
            let edges = match map.path(&start_id, &goal_id) {
                Some(edges) => edges,
                None => return refusal(format!("No route from '{start_id}' to '{goal_id}'.")),
            };
            // A route is only worth replaying if every hop has a call. Menu
            // hops do not, which is why the BFS prefers `focus_panel` edges.
            let replayable = edges.iter().all(|e| e.action.bridge_call().is_some());
            return accepted(serde_json::json!({
                "from": start_id,
                "to": goal_id,
                "label": label,
                "hops": edges.len(),
                "replayable": replayable,
                "route": edges.iter().map(edge_json).collect::<Vec<_>>(),
            }));
        }

        // No destination: hand back the whole graph, plus the exits from
        // wherever the caller says it is, so a sweep can walk it greedily.
        let mut payload = map.to_json();
        if let Some(start) = from {
            match map.resolve(&start) {
                Some(node) => {
                    let id = node.id.clone();
                    let exits: Vec<serde_json::Value> = map
                        .edges
                        .iter()
                        .filter(|edge| edge.from == id)
                        .map(edge_json)
                        .collect();
                    payload["from"] = serde_json::json!(id);
                    payload["exits"] = serde_json::json!(exits);
                }
                None => return refusal(format!("Nothing in the app map matches '{start}'.")),
            }
        }
        accepted(payload)
    }

    /// Move the UI to a node in the app map, walking there through the same
    /// entry points a click uses. Never runs anything tiered above `navigate`:
    /// a call named "go look at this" must not be able to build or save.
    fn cmd_navigate_to(&mut self, target: String) -> GuiResponse {
        let mut map = AppMap::build();
        map.add_commands(&self.command_inventory());
        let node = match map.resolve(&target) {
            Some(node) => node,
            None => {
                return refusal(format!(
                    "Nothing in the app map matches '{target}'. Use gui_app_map to see the nodes."
                ))
            }
        };
        let id = node.id.clone();
        let step = nav_step(node, &self.command_inventory());
        let route: Vec<serde_json::Value> = map
            .path(ROOT, &id)
            .unwrap_or_default()
            .iter()
            .map(edge_json)
            .collect();
        drop(map);

        let response = match step {
            NavStep::Refuse(reason) => return with_error_route(reason, serde_json::json!(route)),
            NavStep::SelectRail(index) => {
                self.activity_bar_selection = index;
                self.left_sidebar_visible = true;
                accepted(serde_json::json!({
                    "arrived": id,
                    "rail": activity_category_name(index),
                }))
            }
            NavStep::SelectSubTab(rail, section) => {
                self.activity_bar_selection = rail;
                self.activity_sub_panel[rail] = section;
                self.left_sidebar_visible = true;
                accepted(serde_json::json!({
                    "arrived": id,
                    "rail": activity_category_name(rail),
                    "section": sub_tab(rail, section).map(|s| s.label),
                }))
            }
            NavStep::FocusPanel(kind) => {
                let title = Tab {
                    id: crate::editor::app::types::TabId(0),
                    kind: kind.clone(),
                }
                .title();
                // `focus_panel`, not `toggle_panel`: navigating to something
                // that happens to already be open must leave it open.
                self.focus_panel(kind);
                accepted(serde_json::json!({ "arrived": id, "panel": title }))
            }
            NavStep::RunCommand(label) => self.cmd_run_command(label, false),
        };
        with_route(response, serde_json::json!(route))
    }

    // ─── Dock tabs ─────────────────────────────────────────────────────────

    /// Every tab the editor holds, and whether the dock is actually drawing it.
    /// `docked` is reported separately from membership in `tabs` because a
    /// rebuild can leave a tab in the list that the dock has forgotten, and a
    /// sweep needs to tell "not switched to" apart from "cannot be switched to".
    fn cmd_list_tabs(&mut self) -> GuiResponse {
        let active = self.active_tab.clone();
        let docked_ids: Vec<u64> = self
            .dock_state
            .as_ref()
            .map(|dock| dock.iter_all_tabs().map(|(_, tab)| tab.id.0).collect())
            .unwrap_or_default();

        let tabs: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .map(|tab| {
                serde_json::json!({
                    "tab_id": tab.id.0,
                    "title": tab.title(),
                    "slug": panel_slug_for_kind(&tab.kind),
                    "is_editor": matches!(tab.kind, TabKind::Editor { .. }),
                    "path": tab.editor_path().map(|p| p.display().to_string()),
                    "docked": docked_ids.contains(&tab.id.0),
                    "active": active.as_ref() == Some(&tab.id),
                })
            })
            .collect();

        accepted(serde_json::json!({
            "count": tabs.len(),
            "dock_present": self.dock_state.is_some(),
            "central_area": if self.central_shows_dock() { "dock" } else { "welcome" },
            "active_tab": active.map(|id| id.0),
            "tabs": tabs,
        }))
    }

    /// Switch the tab holding the central area, by id, title or panel slug.
    fn cmd_select_tab(&mut self, tab: String) -> GuiResponse {
        let matches: Vec<usize> = find_tab_indices(&self.tabs, &tab);
        let Some(&index) = matches.first() else {
            return refusal(format!(
                "No tab matches '{tab}'. Open tabs: {}",
                self.tabs
                    .iter()
                    .map(|t| t.title())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        let id = self.tabs[index].id.clone();
        let title = self.tabs[index].title();
        let kind = self.tabs[index].kind.clone();

        // Editor tabs all share a `TabKind` discriminant, so `focus_panel`'
        // lookup-by-kind can only ever find the first one. Select those by id
        // through the dock; panels keep going through the click path.
        if matches!(kind, TabKind::Editor { .. }) {
            let path = self
                .dock_state
                .as_ref()
                .and_then(|dock| dock.find_tab_from(|docked: &Tab| docked.id == id));
            match path {
                Some(path) => {
                    if let Some(dock) = self.dock_state.as_mut() {
                        if let Err(err) = dock.set_active_tab(path) {
                            return refusal(format!(
                                "The dock refused to activate '{title}': {err}"
                            ));
                        }
                    }
                }
                None => {
                    return refusal(format!(
                        "'{title}' is not in the dock, so it cannot be activated."
                    ))
                }
            }
        } else {
            self.focus_panel(kind);
        }

        self.active_tab = Some(id.clone());
        self.touch_mru(&id);
        accepted(serde_json::json!({
            "selected": title,
            "tab_id": id.0,
            "ambiguous": matches.len() > 1,
            "matches": matches.len(),
            "state_after": self.ide_state(),
        }))
    }

    /// Pick a sub-tab within an activity-bar rail, in one call.
    fn cmd_select_sub_tab(&mut self, rail: String, sub_tab: String) -> GuiResponse {
        let Some(rail_spec) = rail_from_name(&rail) else {
            return refusal(format!(
                "Unknown rail '{rail}'. Valid: {:?}",
                RAILS.iter().map(|r| r.slug).collect::<Vec<_>>()
            ));
        };
        let index = rail_spec.index();
        let Some(section) = rail_spec.sub_tab_index(&sub_tab) else {
            return refusal(format!(
                "Rail '{rail}' has no section '{sub_tab}'. Valid: {:?}",
                rail_spec
                    .sub_tabs
                    .iter()
                    .map(|s| s.slug)
                    .collect::<Vec<_>>()
            ));
        };
        self.activity_bar_selection = index;
        self.activity_sub_panel[index] = section;
        self.left_sidebar_visible = true;
        accepted(serde_json::json!({
            "rail": rail_spec.slug,
            "section": rail_spec.sub_tabs[section].label,
            "state_after": self.ide_state(),
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Pure decision logic
//
// The handlers above only apply what these return. Keeping the gates and the
// name resolution out here means they are testable without an `eframe`
// context, which is the difference between asserting "the sweep reached
// Settings" and asserting "the sweep could have".
// ═══════════════════════════════════════════════════════════════════════════

/// Fold away what a driver does not type faithfully: surrounding whitespace,
/// the `…` a label carries but a caller drops, and repeated internal spaces.
/// `Save As…` and `save as` have to reach the same command.
pub(crate) fn normalize_command(label: &str) -> String {
    label
        .trim()
        .trim_end_matches('\u{2026}')
        .trim_end_matches('.')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Resolve a palette label. Exact first, then normalized, then a substring only
/// when it singles one out -- a ambiguous guess would run some other command
/// than the one asked for, which is worse than an error.
pub(crate) fn find_command<'a>(commands: &'a [Command], label: &str) -> Option<&'a Command> {
    let needle = normalize_command(label);
    if needle.is_empty() {
        return None;
    }
    commands
        .iter()
        .find(|cmd| cmd.label.trim() == label.trim())
        .or_else(|| {
            commands
                .iter()
                .find(|cmd| normalize_command(cmd.label) == needle)
        })
        .or_else(|| {
            let hits: Vec<&Command> = commands
                .iter()
                .filter(|cmd| normalize_command(cmd.label).contains(&needle))
                .collect();
            if hits.len() == 1 {
                hits.into_iter().next()
            } else {
                None
            }
        })
}

/// Why a command may not be run, or `None` when it may.
///
/// The mode gate mirrors the palette: an entry listing modes refuses when the
/// active profile is not among them, so the bridge and the visible list cannot
/// disagree about what exists in this workspace.
///
/// Checked next, before the tier: a native file dialog is modal to the window
/// and the frame loop stops until someone answers it. No `allow_unsafe` value
/// changes that, so opting in is not enough to make the call safe to make and
/// the refusal is unconditional rather than tiered.
pub(crate) fn command_gate(
    cmd: &Command,
    profile: WorkspaceProfile,
    allow_unsafe: bool,
) -> Result<(), String> {
    let risk = command_risk(cmd.category, cmd.label);
    if !cmd.modes.is_empty() && !cmd.modes.contains(&profile) {
        return Err(format!(
            "'{}' is only available in {} (current mode: {})",
            cmd.label,
            cmd.modes
                .iter()
                .map(|m| m.label())
                .collect::<Vec<_>>()
                .join("/"),
            profile.label()
        ));
    }
    if command_is_interactive(cmd.label) {
        return Err(format!(
            "'{}' opens a native modal, which blocks the UI thread until a person answers \
             it and cannot be dismissed over the bridge. gui_list_commands reports it as \
             interactive; press it with a keyboard instead.",
            cmd.label
        ));
    }
    if risk.needs_opt_in() && !allow_unsafe {
        return Err(format!(
            "'{}' is tiered '{}' and can write files, spawn processes or open a dialog. \
             Re-run gui_run_command with allow_unsafe=true.",
            cmd.label,
            risk.as_str()
        ));
    }
    Ok(())
}

/// Nearest labels for an unknown-command error. Fuzzy, the way the palette
/// matches, so a driver that typed `tsb` gets told about `Toggle Sidebar`.
pub(crate) fn unknown_command_message(commands: &[Command], label: &str) -> String {
    let needle = normalize_command(label);
    let mut hints: Vec<&str> = commands
        .iter()
        .filter(|cmd| {
            !needle.is_empty() && fuzzy_subsequence(&normalize_command(cmd.label), &needle)
        })
        .take(12)
        .map(|cmd| cmd.label)
        .collect();
    if hints.is_empty() {
        hints = commands.iter().take(12).map(|cmd| cmd.label).collect();
    }
    format!(
        "Unknown command '{label}'. Nearest matches: {}",
        hints.join(", ")
    )
}

/// What to do with a resolved map node. Decided apart from the handler so the
/// "navigation must never mutate" rule is assertable directly.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NavStep {
    SelectRail(usize),
    SelectSubTab(usize, usize),
    FocusPanel(TabKind),
    RunCommand(String),
    Refuse(String),
}

/// Turn a node into the one action that lands there.
///
/// Command nodes are checked against `inventory` here rather than left to the
/// runner: `gui_navigate_to` is a focus call, so a node whose tier is `modify`
/// or `execute` is refused outright instead of being dispatched and refused a
/// step later by a message about `allow_unsafe` that the caller cannot pass.
pub(crate) fn nav_step(node: &MapNode, inventory: &[CommandSpec]) -> NavStep {
    match node.kind {
        NodeKind::Root => NavStep::Refuse("the app root is where you already are".to_string()),
        NodeKind::Menu => NavStep::Refuse(format!(
            "'{}' is a menu, not a destination: it closes as soon as you click away. \
             Route to the panel inside it instead.",
            node.label
        )),
        NodeKind::Rail => match activity_category_index(node.id.trim_start_matches("rail:")) {
            Some(index) => NavStep::SelectRail(index),
            None => NavStep::Refuse(format!("'{}' is not an activity rail", node.id)),
        },
        NodeKind::RailSection => {
            let decoded = split_section_id(&node.id).and_then(|(rail, section)| {
                Some((
                    activity_category_index(rail)?,
                    rail_from_name(rail)?.sub_tab_index(section)?,
                ))
            });
            match decoded {
                Some((rail, section)) => NavStep::SelectSubTab(rail, section),
                None => NavStep::Refuse(format!("'{}' names no rail section", node.id)),
            }
        }
        NodeKind::Panel => match panel_kind_from_name(node.id.trim_start_matches("panel:")) {
            Some(kind) => NavStep::FocusPanel(kind),
            None => NavStep::Refuse(format!("'{}' names no dock panel", node.id)),
        },
        NodeKind::Mode => match profile_from_label(node.id.trim_start_matches("mode:")) {
            // A mode switch persists workspace preferences, so it goes through
            // the same tier gate as a palette command rather than being waved
            // through by having its own node kind.
            Some(profile) => nav_command(&mode_command_label(profile), inventory),
            None => NavStep::Refuse(format!("'{}' names no workspace profile", node.id)),
        },
        NodeKind::Command => nav_command(&node.label, inventory),
    }
}

/// The map nodes that resolve to a palette command, gated on that command's
/// tier. Shared by the `Command` and `Mode` arms so neither can be reached
/// through `gui_navigate_to` while carrying a side effect.
fn nav_command(label: &str, inventory: &[CommandSpec]) -> NavStep {
    let want = normalize_command(label);
    let tier = inventory
        .iter()
        .find(|spec| normalize_command(&spec.label) == want)
        .map(|spec| spec.risk);
    match tier {
        Some(risk) if risk != CommandRisk::Navigate => NavStep::Refuse(format!(
            "'{label}' is tiered '{}' and would change state. gui_navigate_to only moves \
             focus; run it with gui_run_command and allow_unsafe=true.",
            risk.as_str()
        )),
        // Not in the inventory means the map and the palette disagree, which
        // the drift test catches; still gated at dispatch.
        _ => NavStep::RunCommand(label.to_string()),
    }
}

/// Which tab a `gui_select_tab` argument names. Ids win, then titles, then
/// panel slugs; the result is every match so the handler can say it picked
/// between two tabs with the same visible title (`Memory` is shared by the
/// agent and persistent memory panels).
pub(crate) fn find_tab_indices(tabs: &[Tab], needle: &str) -> Vec<usize> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let id: Option<u64> = needle.parse().ok();
    tabs.iter()
        .enumerate()
        .filter(|(_, tab)| {
            id == Some(tab.id.0)
                || tab.title().eq_ignore_ascii_case(needle)
                || panel_slug_for_kind(&tab.kind)
                    .is_some_and(|slug| slug.eq_ignore_ascii_case(needle))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Attach the route walked to an accepted response's payload.
fn with_route(mut response: GuiResponse, route: serde_json::Value) -> GuiResponse {
    if let Some(Some(object)) = response.data.as_mut().map(serde_json::Value::as_object_mut) {
        object.insert("route".to_string(), route);
    }
    response
}

/// Same, for a refusal: a driver debugging a bad route wants to see how far it
/// got even when the last hop was rejected.
fn with_error_route(reason: String, route: serde_json::Value) -> GuiResponse {
    let mut response = refusal(reason);
    response.data = Some(serde_json::json!({ "route": route }));
    response
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
//
// These cover the decision logic, not the rendering. That split is deliberate:
// the handlers need a live `eframe` context, so everything that decides
// *whether* a driver may do something is a pure function with its own test,
// and what is left untested is only the mechanical application of the answer.
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::app::types::TabId;

    fn cmd(
        label: &'static str,
        category: &'static str,
        modes: &'static [WorkspaceProfile],
    ) -> Command {
        Command {
            label,
            category,
            shortcut: None,
            action: |_: &mut VelocityApp| {},
            modes,
        }
    }

    fn node(id: &str, kind: NodeKind, label: &str) -> MapNode {
        MapNode {
            id: id.to_string(),
            kind,
            label: label.to_string(),
            detail: None,
        }
    }

    fn spec(label: &str, category: &str, risk: CommandRisk) -> CommandSpec {
        CommandSpec {
            label: label.to_string(),
            category: category.to_string(),
            shortcut: None,
            risk,
            interactive: false,
        }
    }

    #[test]
    fn normalization_absorbs_what_a_caller_forgets_to_type() {
        for variant in ["Save As…", "save as", "  Save  As  ", "Save As..."] {
            assert_eq!(normalize_command(variant), "save as", "{variant}");
        }
    }

    #[test]
    fn lookup_accepts_the_label_a_driver_actually_types() {
        let commands = vec![
            cmd("Save As\u{2026}", "File", &[]),
            cmd("Toggle Sidebar", "View", &[]),
        ];
        assert_eq!(
            find_command(&commands, "save as").unwrap().label,
            "Save As\u{2026}"
        );
        assert_eq!(
            find_command(&commands, "TOGGLE SIDEBAR").unwrap().label,
            "Toggle Sidebar"
        );
        assert_eq!(find_command(&commands, "  ").map(|c| c.label), None);
    }

    #[test]
    fn an_ambiguous_fragment_is_an_error_not_a_coin_flip() {
        let commands = vec![
            cmd("Wiki: Export to Markdown", "Knowledge", &[]),
            cmd("Export Map", "Knowledge", &[]),
        ];
        // "export" matches both, so neither is chosen: silently running the
        // wrong one is worse than telling the driver to be specific.
        assert!(find_command(&commands, "export").is_none());
        assert_eq!(
            find_command(&commands, "export map").unwrap().label,
            "Export Map"
        );
    }

    #[test]
    fn navigate_tier_needs_no_opt_in_and_the_other_two_do() {
        let focus = cmd("Toggle Sidebar", "View", &[]);
        assert_eq!(command_gate(&focus, WorkspaceProfile::Coder, false), Ok(()));

        let write = cmd("Save", "File", &[]);
        let err = command_gate(&write, WorkspaceProfile::Coder, false).unwrap_err();
        assert!(err.contains("allow_unsafe"), "{err}");
        assert_eq!(command_gate(&write, WorkspaceProfile::Coder, true), Ok(()));

        let spawn = cmd("Run Selected Flow", "Automation", &[]);
        assert!(command_gate(&spawn, WorkspaceProfile::Coder, false).is_err());
        assert_eq!(command_gate(&spawn, WorkspaceProfile::Coder, true), Ok(()));
    }

    #[test]
    fn a_mode_gated_command_stays_gated_over_the_bridge() {
        let flow = cmd(
            "Run Selected Flow",
            "Automation",
            &[WorkspaceProfile::AutomationOperator],
        );
        let err = command_gate(&flow, WorkspaceProfile::Coder, true).unwrap_err();
        // Says which mode would have it, so the driver can switch rather than
        // conclude the command does not exist.
        assert!(err.contains("Automation Operator"), "{err}");
        assert!(err.contains("current mode: Coder"), "{err}");
        assert_eq!(
            command_gate(&flow, WorkspaceProfile::AutomationOperator, true),
            Ok(())
        );
    }

    #[test]
    fn the_mode_gate_is_checked_before_the_tier_gate() {
        // A command hidden by the profile must report the mode, not beg for
        // allow_unsafe: opting in would still not make it legal here.
        let hidden = cmd("Save", "File", &[WorkspaceProfile::MissionControl]);
        let err = command_gate(&hidden, WorkspaceProfile::Coder, false).unwrap_err();
        assert!(err.starts_with("'Save' is only available in"), "{err}");
    }

    /// `Open File…` blocks the frame loop until a person answers the dialog, and
    /// the bridge has no way to answer it, so `allow_unsafe` would not make the
    /// call safe to make -- it would just hang the app. Refused either way, and
    /// with a message that does not point at the flag that will not help.
    #[test]
    fn a_dialog_command_is_refused_even_with_allow_unsafe() {
        for label in ["Open File\u{2026}", "Save As\u{2026}"] {
            let dialog = cmd(label, "File", &[]);
            for allow_unsafe in [false, true] {
                let err = command_gate(&dialog, WorkspaceProfile::Coder, allow_unsafe).unwrap_err();
                assert!(
                    err.contains("native modal"),
                    "{label} allow={allow_unsafe}: {err}"
                );
                assert!(
                    !err.contains("allow_unsafe"),
                    "must not ask for a flag that changes nothing: {err}"
                );
            }
        }
        // An ordinary write still goes down the opt-in path.
        let save = cmd("Save", "File", &[]);
        assert!(command_gate(&save, WorkspaceProfile::Coder, false).is_err());
        assert_eq!(command_gate(&save, WorkspaceProfile::Coder, true), Ok(()));
    }

    #[test]
    fn the_error_message_points_at_the_command_the_driver_meant() {
        let commands = vec![
            cmd("Toggle Sidebar", "View", &[]),
            cmd("New File", "File", &[]),
        ];
        let msg = unknown_command_message(&commands, "tsb");
        assert!(msg.contains("Toggle Sidebar"), "{msg}");
        assert!(!msg.contains("New File"), "{msg}");
    }

    #[test]
    fn panel_nodes_focus_rather_than_toggle() {
        // Navigating to something already open has to leave it open, which is
        // why the step carries a kind for `focus_panel` and not a toggle.
        let step = nav_step(&node("panel:settings", NodeKind::Panel, "Settings"), &[]);
        assert_eq!(step, NavStep::FocusPanel(TabKind::Settings));
    }

    #[test]
    fn rail_and_section_nodes_resolve_to_indices_the_renderer_uses() {
        let rail = nav_step(&node("rail:git", NodeKind::Rail, "Source Control"), &[]);
        let expected = activity_category_index("git").unwrap();
        assert_eq!(rail, NavStep::SelectRail(expected));

        let section = nav_step(
            &node("rail:git/changes", NodeKind::RailSection, "Changes"),
            &[],
        );
        match section {
            NavStep::SelectSubTab(rail_index, sub) => {
                assert_eq!(rail_index, expected);
                assert_eq!(sub_tab(rail_index, sub).unwrap().slug, "changes");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn menus_and_the_root_are_refused_with_a_reason() {
        for (id, kind, label) in [
            ("menu:Tools", NodeKind::Menu, "Tools"),
            ("app", NodeKind::Root, "Velocity"),
        ] {
            let step = nav_step(&node(id, kind, label), &[]);
            let NavStep::Refuse(reason) = step else {
                panic!("{id} should not be a destination: {step:?}");
            };
            assert!(!reason.is_empty());
        }
    }

    #[test]
    fn a_malformed_node_id_refuses_instead_of_reaching_a_default() {
        // `resolve` can hand back an id the tables have no entry for if the map
        // and the tables ever disagree; that has to be an error, not rail zero.
        assert!(matches!(
            nav_step(&node("rail:nonexistent", NodeKind::Rail, "Nope"), &[]),
            NavStep::Refuse(_)
        ));
        assert!(matches!(
            nav_step(&node("panel:nonexistent", NodeKind::Panel, "Nope"), &[]),
            NavStep::Refuse(_)
        ));
        assert!(matches!(
            nav_step(&node("mode:nonexistent", NodeKind::Mode, "Nope"), &[]),
            NavStep::Refuse(_)
        ));
    }

    #[test]
    fn modes_are_reached_through_their_palette_command() {
        // With no inventory to consult the node decodes to its palette label;
        // the dispatch path re-checks the tier against the live palette.
        let step = nav_step(
            &node(
                "mode:automation-operator",
                NodeKind::Mode,
                "automation-operator",
            ),
            &[],
        );
        assert_eq!(
            step,
            NavStep::RunCommand("Mode: Automation Operator".to_string())
        );
    }

    #[test]
    fn a_mode_node_is_refused_once_its_tier_is_known() {
        // `set_work_mode` calls `save_workspace_preferences`, so switching mode
        // is a state change and `gui_navigate_to` is not allowed to do it.
        let inventory = vec![spec(
            "Mode: Automation Operator",
            "Workspace",
            CommandRisk::Modify,
        )];
        let step = nav_step(
            &node(
                "mode:automation-operator",
                NodeKind::Mode,
                "automation-operator",
            ),
            &inventory,
        );
        let NavStep::Refuse(reason) = step else {
            panic!("a persisting mode switch must not be a focus move: {step:?}");
        };
        assert!(reason.contains("gui_run_command"), "{reason}");
    }

    #[test]
    fn navigating_never_runs_anything_above_the_navigate_tier() {
        let inventory = vec![
            spec("Toggle Sidebar", "View", CommandRisk::Navigate),
            spec("Save", "File", CommandRisk::Modify),
            spec("Build", "Build", CommandRisk::Execute),
        ];
        assert_eq!(
            nav_step(
                &node("cmd:Toggle-Sidebar", NodeKind::Command, "Toggle Sidebar"),
                &inventory
            ),
            NavStep::RunCommand("Toggle Sidebar".to_string())
        );
        for label in ["Save", "Build"] {
            let step = nav_step(&node("cmd:x", NodeKind::Command, label), &inventory);
            let NavStep::Refuse(reason) = step else {
                panic!("gui_navigate_to must refuse {label}");
            };
            assert!(reason.contains("gui_run_command"), "{reason}");
        }
    }

    #[test]
    fn tab_selection_accepts_id_title_or_slug() {
        let tabs = vec![
            Tab {
                id: TabId(1),
                kind: TabKind::Editor {
                    path: Some("a.rs".into()),
                    buffer_id: TabId(11),
                },
            },
            Tab {
                id: TabId(2),
                kind: TabKind::Settings,
            },
            Tab {
                id: TabId(3),
                kind: TabKind::AgentMemory,
            },
            Tab {
                id: TabId(4),
                kind: TabKind::PersistentMemory,
            },
        ];
        assert_eq!(find_tab_indices(&tabs, "2"), vec![1]);
        assert_eq!(find_tab_indices(&tabs, "settings"), vec![1]);
        assert_eq!(find_tab_indices(&tabs, "Settings"), vec![1]);
        // Both memory panels are titled `Memory`, so a bare title names two and
        // the handler reports the ambiguity instead of hiding it.
        assert_eq!(find_tab_indices(&tabs, "Memory"), vec![2, 3]);
        assert_eq!(find_tab_indices(&tabs, "agent-memory"), vec![2]);
        assert_eq!(find_tab_indices(&tabs, "   ").len(), 0);
        assert_eq!(find_tab_indices(&tabs, "nothing"), Vec::<usize>::new());
    }

    #[test]
    fn the_route_is_attached_whether_the_call_succeeded_or_not() {
        let ok = with_route(
            accepted(serde_json::json!({ "arrived": "panel:wiki" })),
            serde_json::json!([1, 2]),
        );
        assert!(ok.success);
        assert_eq!(ok.data.unwrap()["route"], serde_json::json!([1, 2]));

        let bad = with_error_route("nope".to_string(), serde_json::json!([]));
        assert!(!bad.success);
        assert_eq!(bad.error.as_deref(), Some("nope"));
        assert_eq!(bad.data.unwrap()["route"], serde_json::json!([]));
    }
}
