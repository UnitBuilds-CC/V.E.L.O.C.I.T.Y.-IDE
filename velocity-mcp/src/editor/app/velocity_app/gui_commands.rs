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
use crate::wa::window_mgmt::WindowState;
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
            GuiCommand::Screenshot { path, against } => self.cmd_screenshot(ctx, path, against),
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
            GuiCommand::DismissOverlays {} => self.cmd_dismiss_overlays(),
            GuiCommand::SubmitDialog { value } => self.cmd_submit_dialog(value),
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
            open_overlays: self
                .open_transient_ui()
                .into_iter()
                .map(str::to_string)
                .collect(),
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

    /// Capture the IDE's own window to an image file, so a driver can check
    /// what is on screen instead of only what the state struct claims.
    ///
    /// Goes through the same desktop-automation capture the browser subsystem
    /// uses (`wa::screenshot`, Win32 BitBlt of the window rect), which is
    /// already tested -- this handler only decides where the bytes go and what
    /// counts as a failure. It used to answer "not yet implemented", which left
    /// every pixel-level claim about the app unmade.
    ///
    /// Runs on the UI thread, so the frame loop stalls for the length of the
    /// grab (a few hundred ms: PowerShell is spawned to do the copy). That is
    /// the same trade every other capture in this codebase makes, and it is
    /// what lets the shot show the frame as it was when the call arrived.
    fn cmd_screenshot(
        &mut self,
        ctx: &egui::Context,
        path: String,
        against: Option<String>,
    ) -> GuiResponse {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let target = match resolve_screenshot_path(&path, &self.workspace_root, now_ms) {
            Ok(target) => target,
            Err(e) => return refusal(e),
        };

        // A minimised window has nothing on screen to copy: BitBlt reads the
        // desktop where the window used to be, which is somebody else's pixels.
        // Refusing beats saving that behind a success reply. See
        // [`window_is_minimised`] for why `egui` alone cannot answer this.
        let own_windows: Vec<WindowState> =
            crate::wa::window_mgmt::WindowManager::find_by_pid(std::process::id())
                .iter()
                .map(|w| w.state)
                .collect();
        if window_is_minimised(
            ctx.input(|i| i.viewport().minimized.unwrap_or(false)),
            &own_windows,
        ) {
            return refusal(
                "The IDE window is minimised, so a capture would show whatever is \
                 behind it rather than the app. Restore the window and ask again."
                    .to_string(),
            );
        }

        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return refusal(format!(
                    "Cannot create {} to hold the capture: {e}",
                    parent.display()
                ));
            }
        }

        let shot = crate::wa::screenshot::capture(&crate::wa::screenshot::CaptureTarget::Window(
            std::process::id(),
        ));
        // `capture` reports failure by handing back an empty image, so the pixel
        // count is the only status it carries. Checking it is the difference
        // between "saved" and "saved a file that proves nothing".
        if shot.pixel_count() == 0 {
            return refusal(format!(
                "Capture of the IDE window (pid {}) returned no pixels (source: {}). \
                 The window may be hidden, off-screen, or gone.",
                std::process::id(),
                shot.source
            ));
        }
        if let Err(e) = shot.save_to(&target) {
            return refusal(format!(
                "Captured {}x{} but could not write {}: {e}",
                shot.width,
                shot.height,
                target.display()
            ));
        }

        // Optional second question: is this frame different from that one? The
        // comparator has been in the tree since the desktop-automation work with
        // no caller at all, which is another way of saying no capture had ever
        // been checked against anything. A caller that asks is never left with a
        // silent "no change" standing in for "no comparison".
        let requested = against.as_deref().map(str::trim).unwrap_or("");
        let comparison = if requested.is_empty() {
            None
        } else {
            let reference = match resolve_screenshot_path(requested, &self.workspace_root, 0) {
                Ok(target) => target,
                Err(e) => {
                    return refusal(format!(
                        "{e} The capture was still written to {}.",
                        target.display()
                    ))
                }
            };
            let before = match crate::wa::screenshot::load_captured_image(&reference) {
                Some(image) => image,
                None => {
                    return refusal(format!(
                        "Could not read {} as an image, so no comparison was made. The capture \
                         is at {}.",
                        reference.display(),
                        target.display()
                    ))
                }
            };
            let config = crate::wa::screenshot::DiffConfig::default();
            let diff = crate::wa::screenshot::compare_screenshots(&before, &shot, &config);
            Some((
                reference,
                before,
                diff,
                config.channel_tolerance,
                config.max_diff_percentage,
            ))
        };

        let mut reply = serde_json::json!({
            "path": target.display().to_string(),
            "format": crate::wa::screenshot::image_format_for_path(&target),
            "width": shot.width,
            "height": shot.height,
            "captured_at_ms": shot.captured_at_ms,
        });
        if let Some((reference, before, diff, channel_tolerance, max_diff_percentage)) = comparison
        {
            reply["visual_diff"] = serde_json::json!({
                "against": reference.display().to_string(),
                "against_size": [before.width, before.height],
                // A resized window differs everywhere, which is not the same
                // finding as a redrawn one; without this a driver cannot tell
                // the two apart from the percentage alone.
                "dimensions_match": before.width == shot.width && before.height == shot.height,
                "diff_percentage": diff.diff_percentage,
                "diff_pixel_count": diff.diff_pixel_count,
                "total_pixels": diff.total_pixels,
                "matches": diff.matches,
                "diff_bounds": diff.diff_bounds,
                "channel_tolerance": channel_tolerance,
                "max_diff_percentage": max_diff_percentage,
            });
        }
        accepted(reply)
    }

    /// Stand down every transient overlay: the route back to a known state for
    /// a driver that has raised a palette or an in-app dialog. Escape is only
    /// heard by the overlay that owns the frame, so there is no single key press
    /// a remote call can make; this closes the whole stack at once.
    fn cmd_dismiss_overlays(&mut self) -> GuiResponse {
        let closed = self.dismiss_transient_ui();
        self.status_message = if closed.is_empty() {
            "Nothing to dismiss".to_string()
        } else {
            format!("Dismissed {}", closed.join(", "))
        };
        // The listener already asked for a repaint when the command arrived, so
        // the overlays go on screen as gone on the next frame; the response is
        // held until the UI thread has run this handler, which is that frame.
        accepted(serde_json::json!({
            "closed": closed,
            "state_after": self.ide_state(),
        }))
    }

    /// Answer the path prompt that is on screen. See
    /// [`Self::submit_open_dialog`] / [`Self::submit_save_as_dialog`] for what
    /// the value has to satisfy; this handler only works out which prompt it
    /// belongs to and reports the outcome.
    fn cmd_submit_dialog(&mut self, value: String) -> GuiResponse {
        let up = self.open_transient_ui();
        let target = match (
            self.pending_open_path.is_some(),
            self.pending_save_as_path.is_some(),
        ) {
            (true, true) => {
                // One value, two prompts asking for different things. Guessing
                // which to feed would write a file under a name nobody chose.
                return refusal(
                    "Both the Open File and Save As prompts are on screen, so a single value \
                     is ambiguous. DismissOverlays first, then raise just the one you mean."
                        .to_string(),
                );
            }
            (true, false) => "open file dialog",
            (false, true) => "save as dialog",
            (false, false) => {
                return refusal(format!(
                    "No path prompt is on screen to answer. Currently open: {}.",
                    if up.is_empty() {
                        "nothing".to_string()
                    } else {
                        up.join(", ")
                    }
                ));
            }
        };

        let written = if target == "open file dialog" {
            self.submit_open_dialog(&value)
        } else {
            self.submit_save_as_dialog(&value)
        };
        let written = match written {
            Ok(path) => path,
            // The prompt stays up after a refusal, so the driver can correct the
            // value or stand it down; saying so here is what makes the next call
            // obvious rather than guessed at.
            Err(e) => return refusal(format!("{e} The {target} is still on screen.")),
        };
        self.status_message = format!("{}", written.display());
        accepted(serde_json::json!({
            "dialog": target,
            "path": written.display().to_string(),
            "state_after": self.ide_state(),
        }))
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
/// Commands that raise an in-app dialog (`Open File…`, `Save As…`) are *not*
/// refused. They are ordinary palette entries that leave something on screen --
/// the same `egui` windows a click raises, with Cancel buttons -- and
/// `gui_dismiss_overlays` stands them back down. Only the tier gate below
/// applies to them, which is the same treatment `New File` gets.
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

/// Where a bridge-initiated capture may write, and under what name.
///
/// An empty path is the usual request -- "just show me the window" -- so the
/// default lands under `.velocity/screenshots/` beside the other machine state
/// rather than in the working directory, which for a GUI started from Explorer
/// or a service is somewhere the app has no business writing to. A path that is
/// given has to stay inside the workspace: the listener is localhost-and-token
/// bound, but holding the token should not turn a capture into an arbitrary file
/// overwrite.
///
/// The target normally does not exist yet, so [`std::path::Path::canonicalize`]
/// cannot be used on it directly. Instead the deepest ancestor that does exist
/// is resolved and the remainder hung off it, which still collapses any `..` a
/// caller used to walk out of the workspace.
pub(crate) fn resolve_screenshot_path(
    raw: &str,
    workspace_root: &std::path::Path,
    now_ms: u64,
) -> Result<std::path::PathBuf, String> {
    let raw = raw.trim();
    let requested = if raw.is_empty() {
        workspace_root
            .join(".velocity")
            .join("screenshots")
            .join(format!("gui-{now_ms}.png"))
    } else {
        let given = std::path::Path::new(raw);
        if given.is_absolute() {
            given.to_path_buf()
        } else {
            // Relative names resolve against the workspace rather than being
            // rejected: the caller already knows the workspace, and
            // `shots/now.png` is exactly what it means by that.
            workspace_root.join(given)
        }
    };

    let root = workspace_root.canonicalize().map_err(|e| {
        format!(
            "Cannot resolve workspace root {:?}: {e}",
            workspace_root.display()
        )
    })?;

    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = requested.as_path();
    let resolved = loop {
        if let Ok(base) = cursor.canonicalize() {
            let mut out = base;
            for name in missing.iter().rev() {
                out.push(name);
            }
            break out;
        }
        let (Some(name), Some(parent)) = (cursor.file_name(), cursor.parent()) else {
            return Err(format!(
                "Cannot work out an absolute save path for {:?}.",
                requested
            ));
        };
        missing.push(name.to_os_string());
        cursor = parent;
    };

    if !resolved.starts_with(&root) {
        return Err(format!(
            "Screenshot path {} resolves to {:?}, outside the workspace root {root:?}. \
             The bridge may only write inside the workspace it is attached to.",
            requested.display(),
            resolved
        ));
    }

    // The encoders `save_to` knows. Anything else would silently be written as
    // PNG, which is how `capture.png` once ended up holding BMP bytes behind a
    // `.png` name (bug #23) -- refuse the name instead of shipping that again.
    let extension = resolved
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if !matches!(
        extension.as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp")
    ) {
        return Err(format!(
            "{} has no extension a capture can be written as. Use one of .png .jpg \
             .jpeg .gif .webp .bmp.",
            resolved.display()
        ));
    }

    // Containment is decided against the canonical form, so the prefix can be
    // dropped from here on without anything left to re-interpret.
    let resolved = plain_windows_path(&resolved);
    Ok(resolved)
}

/// The same path without the `\\?\` prefix [`std::path::Path::canonicalize`]
/// puts in front of every Windows path it resolves.
///
/// Not cosmetic. `GetState` reports a plain `workspace_root`, so a driver told
/// its capture landed at `\\?\C:\ws\shot.png` cannot see that it landed in the
/// workspace it is already looking at, and PowerShell's `Resolve-Path` refuses
/// the extended form outright. Only safe once containment has been checked
/// against the canonical form, since that is what removed any `..` left in the
/// path -- stripping the prefix first would make it load-bearing again.
pub(crate) fn plain_windows_path(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => std::path::PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// Whether there is anything on screen for a window capture to copy.
///
/// `egui` reports `minimized` only after it has seen a minimise event, so an
/// instance launched straight into the taskbar -- which is how every headless
/// sweep and every automated run starts one -- reports nothing, and the capture
/// came back with zero pixels and a message saying "hidden, off-screen, or gone"
/// when the actual answer was "restore me and it will work". The window manager
/// knows the real state, so ask it as well.
///
/// Every window the process owns has to be minimised for this to say yes: an
/// instance with a floating panel still on screen has pixels worth capturing,
/// and refusing that would be the guard inventing a problem. An empty list says
/// nothing either -- that is the non-Windows case, where the enumeration is
/// deliberately empty and `egui`'s answer stands on its own.
///
/// Pure so the rule is testable without a desktop.
pub(crate) fn window_is_minimised(egui_says_minimised: bool, own_windows: &[WindowState]) -> bool {
    egui_says_minimised
        || (!own_windows.is_empty()
            && own_windows
                .iter()
                .all(|state| *state == WindowState::Minimized))
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

    /// `Open File…` and `Save As…` raise the app's own dialog, not a system
    /// modal, so the bridge may run them: the frame keeps drawing and
    /// `gui_dismiss_overlays` closes what they leave up. They get the ordinary
    /// opt-in for anything that can write, and nothing more than that -- an
    /// earlier version of this gate refused them outright on the belief that a
    /// native dialog was blocking the UI thread, which the actions do not do.
    #[test]
    fn a_dialog_command_is_pressable_and_reports_that_it_leaves_a_dialog_up() {
        for label in ["Open File\u{2026}", "Save As\u{2026}"] {
            let dialog = cmd(label, "File", &[]);
            // Held to the tier, not barred: the complaint is about the flag, and
            // setting it gets the command run.
            let err = command_gate(&dialog, WorkspaceProfile::Coder, false).unwrap_err();
            assert!(err.contains("allow_unsafe=true"), "{label}: {err}");
            assert!(!err.contains("modal"), "{label}: {err}");
            assert!(!err.contains("keyboard"), "{label}: {err}");
            assert_eq!(
                command_gate(&dialog, WorkspaceProfile::Coder, true),
                Ok(()),
                "{label} should run once the driver has opted in"
            );
            // A driver still learns that something is going to be on screen.
            assert!(command_is_interactive(label), "{label} must stay flagged");
        }
        // An ordinary write still goes down the opt-in path.
        let save = cmd("Save", "File", &[]);
        assert!(command_gate(&save, WorkspaceProfile::Coder, false).is_err());
        assert_eq!(command_gate(&save, WorkspaceProfile::Coder, true), Ok(()));
    }

    // ─── Where a capture is allowed to land ───────────────────────────────

    #[test]
    fn an_empty_capture_path_picks_a_timestamped_name_inside_the_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let got = resolve_screenshot_path("", ws.path(), 1_700_000_000_000).unwrap();
        assert!(
            got.starts_with(plain_windows_path(&ws.path().canonicalize().unwrap())),
            "{got:?} left the workspace"
        );
        // Under `.velocity/`, beside the other machine state, and not in the
        // process working directory -- which for a GUI launched from Explorer is
        // wherever the shortcut happened to point.
        assert_eq!(got.file_name().unwrap(), "gui-1700000000000.png");
        assert!(
            got.to_string_lossy()
                .replace('\\', "/")
                .contains(".velocity/screenshots/"),
            "{got:?}"
        );
    }

    #[test]
    fn a_relative_capture_name_resolves_against_the_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let got = resolve_screenshot_path("shots/now.png", ws.path(), 1).unwrap();
        assert_eq!(
            got,
            plain_windows_path(&ws.path().canonicalize().unwrap())
                .join("shots")
                .join("now.png")
        );
    }

    /// A reply the driver cannot compare against `workspace_root`, or open with
    /// the tools it has, is a reply about a file that might not exist. The
    /// `\\?\` form canonicalise hands back is both of those things.
    #[test]
    fn a_capture_path_comes_back_in_the_same_form_as_the_workspace_root() {
        let ws = tempfile::tempdir().unwrap();
        let got = resolve_screenshot_path("now.png", ws.path(), 1).unwrap();
        assert!(
            !got.display().to_string().starts_with(r"\\?\"),
            "{got:?} is still in the verbatim form"
        );
        assert_eq!(
            got.parent(),
            Some(plain_windows_path(&ws.path().canonicalize().unwrap()).as_path()),
            "{got:?}"
        );
    }

    /// The bridge is localhost-and-token bound, but a token in hand should not
    /// be an arbitrary-file overwrite. `..` is the interesting case: it has to
    /// be collapsed before the containment test, not after.
    #[test]
    fn a_capture_cannot_be_written_outside_the_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let neighbour = ws
            .path()
            .parent()
            .unwrap()
            .join("velocity-escape-was-here.png");
        for raw in [
            "../outside.png".to_string(),
            // Built from the running platform's separator. `\.\..\outside.png`
            // is a single ordinary file name on Unix, where `\` does not separate
            // directories, so there the resolver is right to accept it and the
            // case worth catching is the one that really traverses.
            format!(
                ".{}..{}outside.png",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            ),
            neighbour.display().to_string(),
        ] {
            let err = resolve_screenshot_path(&raw, ws.path(), 1).unwrap_err();
            assert!(err.contains("outside the workspace"), "{raw}: {err}");
            // Refusing is a refusal: nothing gets created on the way out.
            assert!(!neighbour.exists(), "refusing created {neighbour:?}");
        }
    }

    /// The hole the live sweep found: an instance launched straight into the
    /// taskbar never emits a minimise event, so `egui` reports nothing and the
    /// capture failed with a message about a window that had gone away. The
    /// window manager's answer is what makes the refusal actionable.
    #[test]
    fn a_window_launched_minimised_is_caught_by_the_window_manager() {
        use WindowState::{Maximized, Minimized, Normal};
        // Launched minimised: `egui` knows nothing, Win32 does.
        assert!(window_is_minimised(false, &[Minimized]));
        // A window `egui` minimised itself is still caught by its own flag.
        assert!(window_is_minimised(true, &[]));
        // A second window still on screen is pixels worth capturing, so the
        // guard does not invent a problem that is not there.
        assert!(!window_is_minimised(false, &[Minimized, Normal]));
        assert!(!window_is_minimised(false, &[Maximized, Normal]));
        // Off Windows the enumeration is deliberately empty, which has to mean
        // "no information" rather than "everything is minimised".
        assert!(!window_is_minimised(false, &[]));
    }

    /// `save_to` falls back to PNG for a name it does not recognise, which is
    /// how `capture.png` once ended up holding BMP bytes behind the extension
    /// (bug #23). A name that cannot be encoded honestly is refused instead.
    #[test]
    fn only_extensions_the_encoder_can_write_are_accepted() {
        let ws = tempfile::tempdir().unwrap();
        let err = resolve_screenshot_path("notes.txt", ws.path(), 1).unwrap_err();
        assert!(
            err.contains("no extension a capture can be written as"),
            "{err}"
        );
        for name in [
            "a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp", "a.bmp", "A.PNG",
        ] {
            assert!(
                resolve_screenshot_path(name, ws.path(), 1).is_ok(),
                "{name}"
            );
        }
        // A parent directory that does not exist yet is fine: the capture
        // creates it. Only the workspace itself has to be resolvable.
        assert!(resolve_screenshot_path("deep/deeper/x.png", ws.path(), 1).is_ok());
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

    // ── In-process handler coverage ───────────────────────────────────────
    //
    // Everything above exercises free functions. These run `execute_gui_command`
    // against a real `VelocityApp` -- `test_stub` had no callers at all until
    // here, so the harness existed but had never been executed -- which is what
    // lets a claim like "this command leaves a dialog up" be checked against the
    // state the handler actually leaves rather than against another reading of
    // the same code.

    fn harness() -> (VelocityApp, egui::Context) {
        (VelocityApp::test_stub(), egui::Context::default())
    }

    /// Put a real editor tab and its buffer on the app, leaving it focused, the
    /// way opening a file does. Several commands only mean something with an
    /// editor in front of them.
    fn attach_editor(app: &mut VelocityApp, name: &str) -> TabId {
        let id = TabId(900 + app.tabs.len() as u64);
        let path = app.workspace_root.join(name);
        let tab = Tab {
            id: id.clone(),
            kind: TabKind::Editor {
                path: Some(path.clone()),
                buffer_id: id.clone(),
            },
        };
        app.buffers.insert(
            id,
            crate::editor::buffer::EditorBuffer::new(Some(path), "hello\n".to_string()),
        );
        app.tabs.push(tab);
        app.active_tab = app.tabs.last().map(|t| t.id.clone());
        app.active_tab.clone().expect("tab just pushed")
    }

    /// Pump the frame loop's file-I/O drain until the read behind
    /// [`VelocityApp::open_editor`] lands. That call never blocks the UI thread:
    /// it spawns a reader whose result is applied by `poll_file_io_results` on
    /// the next repaint, so a test wanting the bytes has to drive the same event
    /// the app does rather than assume the call was synchronous.
    fn await_file_load(app: &mut VelocityApp, id: &TabId) -> String {
        for _ in 0..200 {
            app.poll_file_io_results();
            if !app.pending_file_loads.contains(id) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        app.poll_file_io_results();
        app.buffers
            .get(id)
            .map(|b| b.content().to_string())
            .unwrap_or_default()
    }

    #[test]
    fn the_tier_gate_holds_the_dialog_command_back_until_it_is_opted_into() {
        let (mut app, ctx) = harness();

        let denied = app.execute_gui_command(
            GuiCommand::RunCommand {
                label: "Open File\u{2026}".to_string(),
                allow_unsafe: None,
            },
            &ctx,
        );
        assert!(!denied.success, "a Modify-tier command ran unopted-in");
        assert!(denied.error.unwrap().contains("allow_unsafe=true"));
        // Refusing has to mean refusing: nothing left on screen to trip the next
        // call over.
        assert!(
            app.pending_open_path.is_none(),
            "the refused command raised a dialog anyway"
        );
        assert!(app.ide_state().open_overlays.is_empty());

        let ok = app.execute_gui_command(
            GuiCommand::RunCommand {
                label: "Open File\u{2026}".to_string(),
                allow_unsafe: Some(true),
            },
            &ctx,
        );
        assert!(ok.success, "{:?}", ok.error);
        let data = ok.data.unwrap();
        assert_eq!(data["interactive"], serde_json::json!(true));
        assert!(app.pending_open_path.is_some());
        // Both the embedded report and a following GetState say the same thing,
        // so a driver can wait on either.
        let listed = |overlays: &serde_json::Value| {
            overlays
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o == "open file dialog")
        };
        assert!(listed(&data["state_after"]["open_overlays"]));
        assert!(listed(
            &serde_json::to_value(app.ide_state()).unwrap()["open_overlays"]
        ));
    }

    #[test]
    fn save_as_only_promises_a_prompt_it_can_act_on() {
        let (mut app, ctx) = harness();
        // The stub starts on the chat tab, which has no path to name, so the
        // command says so instead of raising a dialog whose Save would fail.
        app.active_tab = None;
        let r = app.execute_gui_command(
            GuiCommand::RunCommand {
                label: "Save As\u{2026}".to_string(),
                allow_unsafe: Some(true),
            },
            &ctx,
        );
        assert!(r.success, "{:?}", r.error);
        assert!(app.pending_save_as_path.is_none());
        assert!(app.status_message.contains("No active editor"));

        attach_editor(&mut app, "name_me.txt");
        let r = app.execute_gui_command(
            GuiCommand::RunCommand {
                label: "Save As\u{2026}".to_string(),
                allow_unsafe: Some(true),
            },
            &ctx,
        );
        assert!(r.success, "{:?}", r.error);
        assert!(app.pending_save_as_path.is_some());
        assert!(app
            .ide_state()
            .open_overlays
            .iter()
            .any(|o| o == "save as dialog"));
    }

    /// Both prompts are labelled "relative to workspace" and neither one used to
    /// enforce it: `join` hands an absolute path straight back and `..` walks out
    /// of the tree, so a prompt reached files that `gui_open_file` refuses. The
    /// escape is checked as an absence of effect -- nothing written anywhere --
    /// rather than as a particular complaint.
    #[test]
    fn a_prompt_refuses_a_value_that_leaves_the_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let outside_file = elsewhere.path().join("outside.txt");
        std::fs::write(&outside_file, "not yours\n").unwrap();

        let (mut app, _ctx) = harness();
        app.workspace_root = ws.path().to_path_buf();
        attach_editor(&mut app, "inside.txt");

        for value in [
            "../escaped.txt".to_string(),
            outside_file.display().to_string(),
        ] {
            app.pending_open_path = Some(std::path::PathBuf::new());
            let refused = app.submit_open_dialog(&value).unwrap_err();
            assert!(refused.contains("escapes workspace"), "{value}: {refused}");
            // A refusal that also closes the prompt would hide what went wrong.
            assert!(app.pending_open_path.is_some(), "{value} closed the prompt");

            app.pending_save_as_path = Some(std::path::PathBuf::new());
            let refused = app.submit_save_as_dialog(&value).unwrap_err();
            assert!(refused.contains("escapes workspace"), "{value}: {refused}");
            assert!(
                app.pending_save_as_path.is_some(),
                "{value} closed the prompt"
            );
        }
        // The whole run wrote no file: not into the workspace, and not next door
        // to it either.
        assert_eq!(std::fs::read_dir(ws.path()).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 1);
    }

    /// The point of a prompt is what it does when it is answered, and until the
    /// bridge could answer one that path was only reachable by painting a frame
    /// and clicking a button -- so it had never been exercised at all.
    #[test]
    fn answering_the_save_prompt_writes_the_file_and_moves_the_tab_onto_it() {
        let ws = tempfile::tempdir().unwrap();
        let (mut app, ctx) = harness();
        app.workspace_root = ws.path().to_path_buf();
        let id = attach_editor(&mut app, "scratch.txt");
        app.pending_save_as_path = Some(std::path::PathBuf::new());

        let r = app.execute_gui_command(
            GuiCommand::SubmitDialog {
                value: "renamed.md".to_string(),
            },
            &ctx,
        );
        assert!(r.success, "{:?}", r.error);
        assert_eq!(
            r.data.as_ref().unwrap()["dialog"],
            serde_json::json!("save as dialog")
        );
        let written = ws.path().join("renamed.md");
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "hello\n");
        // The tab now points at the file it wrote, in the same plain form every
        // other path in the app uses, and the buffer counts as saved.
        assert!(
            app.tab_path(&id).unwrap().ends_with("renamed.md"),
            "{:?}",
            app.tab_path(&id)
        );
        assert!(!app.buffers.get(&id).unwrap().is_dirty());
        assert!(app.pending_save_as_path.is_none());
        assert!(app.ide_state().open_overlays.is_empty());
    }

    #[test]
    fn answering_the_open_prompt_loads_the_file_into_a_buffer() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("loadable.rs"), "fn main() {}\n").unwrap();
        let (mut app, ctx) = harness();
        app.workspace_root = ws.path().to_path_buf();
        app.pending_open_path = Some(std::path::PathBuf::new());

        let r = app.execute_gui_command(
            GuiCommand::SubmitDialog {
                value: "loadable.rs".to_string(),
            },
            &ctx,
        );
        assert!(r.success, "{:?}", r.error);
        assert_eq!(
            r.data.as_ref().unwrap()["dialog"],
            serde_json::json!("open file dialog")
        );
        // Not merely a tab with a plausible name: the focused buffer holds the
        // bytes that were on disk, arriving the way they do in the running app.
        let active = app.active_tab.clone().expect("a tab is focused");
        let (path, buffer_id) = {
            let tab = app
                .tabs
                .iter()
                .find(|t| t.id == active)
                .expect("focused tab");
            match &tab.kind {
                TabKind::Editor {
                    path: Some(p),
                    buffer_id,
                } => (p.clone(), buffer_id.clone()),
                other => panic!("the prompt opened a non-editor tab: {other:?}"),
            }
        };
        assert!(path.ends_with("loadable.rs"), "{path:?}");
        assert_eq!(await_file_load(&mut app, &buffer_id), "fn main() {}\n");
        assert!(app.pending_open_path.is_none());
        assert!(app.ide_state().open_overlays.is_empty());
    }

    /// One value, one prompt. Two prompts is a contradiction to report, not a
    /// coin to flip, and none is nothing to have answered.
    #[test]
    fn an_answer_means_something_only_when_exactly_one_prompt_is_up() {
        let (mut app, ctx) = harness();
        let none = app.execute_gui_command(
            GuiCommand::SubmitDialog {
                value: "a.txt".to_string(),
            },
            &ctx,
        );
        assert!(!none.success);
        let err = none.error.unwrap();
        assert!(err.contains("No path prompt"), "{err}");

        app.pending_open_path = Some(std::path::PathBuf::new());
        app.pending_save_as_path = Some(std::path::PathBuf::new());
        let both = app.execute_gui_command(
            GuiCommand::SubmitDialog {
                value: "a.txt".to_string(),
            },
            &ctx,
        );
        assert!(!both.success);
        let err = both.error.unwrap();
        assert!(err.contains("ambiguous"), "{err}");
        // Refusing the ambiguity does not resolve it by picking a victim.
        assert!(app.pending_open_path.is_some() && app.pending_save_as_path.is_some());
    }

    #[test]
    fn dismissing_closes_exactly_what_the_state_report_listed() {
        let (mut app, ctx) = harness();
        app.command_palette.open = true;
        app.quick_open.open = true;
        app.goto_line_open = true;
        app.show_shortcuts = true;
        app.pending_close_tab = Some(TabId(7));
        let editor = attach_editor(&mut app, "find_me.txt");
        app.buffers
            .get_mut(&editor)
            .expect("editor buffer")
            .find_replace
            .visible = true;

        let reported = app.ide_state().open_overlays.clone();
        assert_eq!(reported.len(), 6, "harness left {reported:?} up");

        let resp = app.execute_gui_command(GuiCommand::DismissOverlays {}, &ctx);
        assert!(resp.success, "{:?}", resp.error);
        let closed: Vec<String> = resp.data.as_ref().unwrap()["closed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // One list backs both the report and the close, so they cannot drift.
        assert_eq!(closed, reported, "closed a different set than it reported");

        let after = app.execute_gui_command(GuiCommand::GetState {}, &ctx);
        assert_eq!(
            after.data.unwrap()["open_overlays"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "something survived the dismiss"
        );
        assert!(!app.command_palette.open && !app.quick_open.open && !app.goto_line_open);
        assert!(app.pending_close_tab.is_none());
        assert!(
            !app.buffers
                .get(&editor)
                .expect("editor buffer")
                .find_replace
                .visible
        );
    }

    #[test]
    fn a_clean_app_reports_nothing_to_dismiss() {
        let (mut app, ctx) = harness();
        assert!(app.ide_state().open_overlays.is_empty());
        let r = app.execute_gui_command(GuiCommand::DismissOverlays {}, &ctx);
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.data.as_ref().unwrap()["closed"], serde_json::json!([]));
        assert_eq!(app.status_message, "Nothing to dismiss");
    }

    #[test]
    fn a_capture_destination_outside_the_workspace_is_refused_before_capture() {
        let (mut app, ctx) = harness();
        let before = app.status_message.clone();
        let r = app.execute_gui_command(
            GuiCommand::Screenshot {
                path: format!("..{}elsewhere.png", std::path::MAIN_SEPARATOR),
                against: None,
            },
            &ctx,
        );
        assert!(!r.success, "wrote a capture outside the workspace");
        assert!(r.error.unwrap().contains("workspace"));
        // Refusing must not spawn the PowerShell grab, which is the expensive
        // and externally visible half of the call.
        assert_eq!(app.status_message, before);
    }
}
