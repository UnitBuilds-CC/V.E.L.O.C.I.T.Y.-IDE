use std::path::PathBuf;

use super::super::types::*;
use super::struct_def::VelocityApp;
use super::tier3_common::primary_button;
use crate::agent::UiToAgentMessage;

/// Which LSP location request drives a go-to-* navigation. All four return the
/// same `Location[]` shape and share one navigation path; only the request and
/// the user-facing noun differ.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GotoKind {
    Definition,
    Declaration,
    TypeDefinition,
    Implementation,
}

impl GotoKind {
    fn noun(self) -> &'static str {
        match self {
            GotoKind::Definition => "Definition",
            GotoKind::Declaration => "Declaration",
            GotoKind::TypeDefinition => "Type definition",
            GotoKind::Implementation => "Implementation",
        }
    }
}

impl VelocityApp {
    /// The user asked to see this panel. Every click, shortcut, palette entry
    /// and bridge command lands here, so this is where the request gets
    /// recorded -- see [`central_shows_dock`].
    pub fn focus_panel(&mut self, kind: TabKind) {
        self.set_focused_panel(kind, true);
    }

    /// Place the panel without claiming the user asked for it. Used only by
    /// profile application, whose choice of landing panel is the preset's,
    /// not a request to hide the welcome screen. Kept out of the public API so
    /// a UI handler cannot accidentally take the easy route and leave its own
    /// click with no visible effect.
    pub(super) fn focus_panel_quiet(&mut self, kind: TabKind) {
        self.set_focused_panel(kind, false);
    }

    fn set_focused_panel(&mut self, kind: TabKind, requested: bool) {
        // Every panel entry point funnels through here. With no dock there was
        // nowhere to put the tab and the call did nothing at all -- it did not
        // even record the tab in `self.tabs`, so the click left no trace.
        if self.dock_state.is_none() {
            self.rebuild_dock();
        }
        if requested {
            self.panel_requested = true;
        }
        if let Some(dock) = self.dock_state.as_mut() {
            // Resolve from the tab list, not from the dock. A rebuild can leave a
            // panel tab in `self.tabs` that the dock has forgotten, and matching
            // only the dock let a second copy pile up beside the orphaned first.
            if let Some(tab) = find_tab_by_kind(&self.tabs, &kind) {
                let id = tab.id.clone();
                let docked = dock.iter_all_tabs().any(|(_, docked)| docked.id == id);
                if !docked {
                    dock.push_to_focused_leaf(tab.clone());
                }
                if let Some(tab_path) = dock.find_tab(&tab) {
                    let _ = dock.set_active_tab(tab_path);
                    self.active_tab = Some(id);
                    return;
                }
            }

            let id = TabId::next(&mut self.tab_counter);
            let tab = Tab {
                id: id.clone(),
                kind,
            };
            if !self.tabs.iter().any(|t| t.id == id) {
                self.tabs.push(tab.clone());
            }
            dock.push_to_focused_leaf(tab.clone());
            if let Some(tab_path) = dock.find_tab(&tab) {
                let _ = dock.set_active_tab(tab_path);
            }
            self.active_tab = Some(id);
        }
    }

    /// Which host the central area should draw. The render loop and the GUI
    /// bridge state report both ask here so neither can disagree with the other
    /// about what the frame actually showed.
    pub fn central_shows_dock(&self) -> bool {
        crate::editor::app::types::central_area_is_dock(
            &self.tabs,
            self.active_tab.as_ref(),
            self.panel_requested,
        )
    }

    pub fn toggle_panel(&mut self, kind: TabKind) {
        // If the panel is already open AND is the active tab, close it (toggle off).
        // Otherwise, open/focus it (toggle on).
        let dominated = self
            .tabs
            .iter()
            .find(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(&kind))
            .map(|t| t.id.clone());

        if let Some(ref id) = dominated {
            if self.active_tab.as_ref() == Some(id) {
                // Panel is active -- toggle it off. The user has now taken back
                // their request, so with no editor open the welcome screen is
                // the central area again rather than an abandoned dock.
                let id = id.clone();
                self.tabs.retain(|t| t.id != id);
                self.buffers.remove(&id);
                self.active_tab = self.tabs.first().map(|t| t.id.clone());
                self.panel_requested = false;
                self.rebuild_dock();
                return;
            }
        }

        // Panel is either not open or not active -- focus/open it.
        self.focus_panel(kind);
    }

    pub fn rebuild_dock(&mut self) {
        self.dock_state = Some(self.build_workspace_dock(self.appearance.profile));
    }

    pub fn build_active(&mut self) {
        // A targeted node runs the same cargo check remotely; with no target
        // (or a stale one) nothing changes and the agent thread builds here.
        if let Some(node) = self.routed_node() {
            let cmd =
                crate::agent::instance_tools::wrap_for_work_dir(&node.work_dir, "cargo check");
            self.command_output.clear();
            self.status_message = format!("Checking on {}...", node.name);
            self.toasts.push(crate::editor::toast::Toast::info(format!(
                "Build routed to {}...",
                node.name
            )));
            self.agent_active = true;
            self.run_on_node(node.id, cmd, true);
            return;
        }
        self.command_output.clear();
        self.status_message = "Running local build...".into();
        self.toasts
            .push(crate::editor::toast::Toast::info("Build started..."));
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalBuild);
    }

    pub fn run_active(&mut self) {
        if let Some(node) = self.routed_node() {
            let cmd = crate::agent::instance_tools::wrap_for_work_dir(&node.work_dir, "cargo run");
            self.command_output.clear();
            self.status_message = format!("Running on {}...", node.name);
            self.toasts.push(crate::editor::toast::Toast::info(format!(
                "Execute routed to {}...",
                node.name
            )));
            self.agent_active = true;
            self.run_on_node(node.id, cmd, true);
            return;
        }
        self.command_output.clear();
        self.status_message = "Running local execute...".into();
        self.toasts
            .push(crate::editor::toast::Toast::info("Execute started..."));
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalRun);
    }

    pub fn toggle_orchestrator(&mut self) {
        self.toggle_panel(TabKind::Orchestrator);
    }

    pub fn toggle_mission_control(&mut self) {
        self.toggle_panel(TabKind::MissionControl);
    }

    /// Export the sitemap-generated wiki to `.wiki/` as interlinked Markdown.
    pub fn export_wiki_markdown(&mut self) {
        let workspace_root = self.workspace_root.clone();
        self.wiki_view.export(&workspace_root, &mut self.toasts);
    }

    /// True for `.nda` files that live in internal state dirs (`.velocity/`,
    /// `memory/`) -- those are at-rest envelopes, never routed to the NDA editor.
    pub(crate) fn is_internal_nda_path(path: &std::path::Path) -> bool {
        path.components()
            .any(|c| matches!(c.as_os_str().to_str(), Some(".velocity") | Some("memory")))
    }

    /// Open (or focus) an NDA document tab. With `None`, opens a fresh blank
    /// document for native authoring.
    pub fn open_nda_document(&mut self, path: Option<PathBuf>) {
        if let Some(ref p) = path {
            let existing = self.tabs.iter().find_map(|tab| match &tab.kind {
                TabKind::NdaDoc { path: Some(tp) } if tp == p => Some(tab.id.clone()),
                _ => None,
            });
            if let Some(id) = existing {
                self.active_tab = Some(id.clone());
                self.touch_mru(&id);
                return;
            }
        }
        let id = TabId::next(&mut self.tab_counter);
        let tab = Tab {
            id: id.clone(),
            kind: TabKind::NdaDoc { path: path.clone() },
        };
        let mut view = crate::editor::nda_document::NdaDocumentView::new();
        if let Some(ref p) = path {
            let ws = self.workspace_root.clone();
            view.open(&ws, p);
        }
        self.nda_docs.insert(id.clone(), view);
        self.tabs.push(tab.clone());
        if let Some(dock) = self.dock_state.as_mut() {
            dock.push_to_focused_leaf(tab);
        }
        self.active_tab = Some(id.clone());
        self.touch_mru(&id);
    }

    /// Command: open a blank NDA document for authoring.
    pub fn new_nda_document(&mut self) {
        self.open_nda_document(None);
    }

    /// Command: write the standalone NDA PWA viewer to `.velocity/` and open it
    /// in the default browser (it can then load any `.nda` file).
    pub fn open_nda_viewer(&mut self) {
        let dir = self.workspace_root.join(".velocity");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.toasts.push(crate::editor::toast::Toast::error(format!(
                "Viewer failed: {e}"
            )));
            return;
        }
        let out = dir.join("nda_viewer.html");
        match std::fs::write(&out, crate::editor::nda_viewer::pwa_viewer_html()) {
            Ok(_) => {
                self.toasts
                    .push(crate::editor::toast::Toast::success(format!(
                        "NDA viewer at {}",
                        out.display()
                    )));
                crate::editor::nda_document::open_in_browser(&out);
            }
            Err(e) => self.toasts.push(crate::editor::toast::Toast::error(format!(
                "Viewer failed: {e}"
            ))),
        }
    }

    /// Command: convert a workspace file into a portable NDA document and open it.
    pub fn import_file_to_nda(&mut self) {
        let path = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tab_path(id).cloned());
        let Some(path) = path else {
            self.toasts.push(crate::editor::toast::Toast::info(
                "Open a file first, then import it to NDA",
            ));
            return;
        };
        match crate::editor::nda_document::convert_file_to_doc(&path) {
            Ok(doc) => {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "imported".to_string());
                let safe: String = stem
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let out = self.workspace_root.join(format!("{safe}.nda"));
                match crate::editor::nda_document::save_to_disk(
                    &self.workspace_root,
                    &out,
                    &doc,
                    false,
                ) {
                    Ok(_) => {
                        self.toasts
                            .push(crate::editor::toast::Toast::success(format!(
                                "Imported to {}",
                                out.display()
                            )));
                        self.open_nda_document(Some(out));
                    }
                    Err(e) => self.toasts.push(crate::editor::toast::Toast::error(format!(
                        "Import failed: {e}"
                    ))),
                }
            }
            Err(e) => self.toasts.push(crate::editor::toast::Toast::error(format!(
                "Import failed: {e}"
            ))),
        }
    }

    /// Show a sub-tab within an activity-bar rail, returning whether both names
    /// resolve against `app_map::RAILS`.
    ///
    /// One writer for `activity_bar_selection` + `activity_sub_panel`, because
    /// the menu commands and the GUI bridge both need to move the strip: kept
    /// separate, the bridge could select a section the renderer then clamped
    /// away, so the call reported success and the sidebar showed something else.
    pub fn select_rail_section(&mut self, rail: &str, section: &str) -> bool {
        let Some(rail_spec) = crate::editor::app::app_map::rail_from_name(rail) else {
            return false;
        };
        let Some(index) = rail_spec.sub_tab_index(section) else {
            return false;
        };
        let rail_index = rail_spec.index();
        self.activity_bar_selection = rail_index;
        self.activity_sub_panel[rail_index] = index;
        self.left_sidebar_visible = true;
        true
    }

    /// Reveal the integrated research browser in the sidebar.
    ///
    /// This used to switch the whole workspace profile to Coder and then set
    /// `left_sidebar_tab` from the mode's left-tab list. Nothing renders that
    /// list any more -- the activity bar replaced it, and the field was never
    /// read back -- so the menu item reflowed the window and showed no browser.
    /// It now selects the section that actually draws one, and leaves the user's
    /// mode alone.
    pub fn open_browse_workspace(&mut self) {
        self.select_rail_section("chat", "browser");
    }

    pub fn toggle_search(&mut self) {
        self.toggle_panel(TabKind::Search);
    }

    pub fn toggle_settings(&mut self) {
        self.toggle_panel(TabKind::Settings);
    }

    /// Rescan extensions from disk and open the Extensions manager panel.
    pub fn toggle_extensions(&mut self) {
        let ws = self.workspace_root.clone();
        self.extension_registry.scan(&ws);
        self.toggle_panel(TabKind::Extensions);
    }

    /// Open the live orchestration Activity panel.
    pub fn toggle_activity(&mut self) {
        self.toggle_panel(TabKind::Activity);
    }

    /// Analyze coverage on first open, then show the Coverage panel.
    pub fn toggle_coverage(&mut self) {
        if self.test_generator.analysis.total_functions == 0 {
            self.run_coverage_analysis();
        }
        self.toggle_panel(TabKind::Coverage);
    }

    /// Initialize the deploy pipeline and open the Pipeline panel.
    pub fn toggle_pipeline(&mut self) {
        self.init_deploy_pipeline();
        self.toggle_panel(TabKind::Pipeline);
    }

    /// Open the Voice command panel.
    pub fn toggle_voice(&mut self) {
        self.toggle_panel(TabKind::Voice);
    }

    /// Open the Test Generator panel.
    pub fn toggle_test_generator(&mut self) {
        self.toggle_panel(TabKind::TestGenerator);
    }

    /// Open the Agent Memory panel.
    pub fn toggle_agent_memory(&mut self) {
        self.toggle_panel(TabKind::AgentMemory);
    }

    /// Open the Live Orchestration panel.
    pub fn toggle_live_orchestration(&mut self) {
        self.toggle_panel(TabKind::LiveOrchestration);
    }

    /// Open the Semantic Search panel.
    pub fn toggle_semantic_search(&mut self) {
        self.toggle_panel(TabKind::SemanticSearch);
    }

    /// Open the Snippets panel.
    pub fn toggle_snippets(&mut self) {
        self.toggle_panel(TabKind::Snippets);
    }

    /// Open the Language Servers panel.
    pub fn toggle_language_servers(&mut self) {
        self.toggle_panel(TabKind::LanguageServers);
    }

    /// Open the Debugger panel.
    pub fn toggle_debugger(&mut self) {
        self.toggle_panel(TabKind::Debugger);
    }

    /// Open the Precomputation Cache panel.
    pub fn toggle_precomp_cache(&mut self) {
        self.toggle_panel(TabKind::PrecompCache);
    }

    /// Open the Multimodal Attachments panel.
    pub fn toggle_multimodal(&mut self) {
        self.toggle_panel(TabKind::Multimodal);
    }

    /// Open the Continuation Ledger panel.
    pub fn toggle_continuation_ledger(&mut self) {
        self.toggle_panel(TabKind::ContinuationLedger);
    }

    /// Open the Plugin Registry panel.
    pub fn toggle_plugin_registry(&mut self) {
        self.toggle_panel(TabKind::PluginRegistry);
    }

    /// Open the Skill Files panel.
    pub fn toggle_skill_files(&mut self) {
        self.toggle_panel(TabKind::SkillFiles);
    }

    /// Open the Inline Suggestions panel.
    pub fn toggle_inline_suggestions(&mut self) {
        self.toggle_panel(TabKind::InlineSuggestions);
    }

    /// Open the Favorites panel.
    pub fn toggle_favorites(&mut self) {
        self.toggle_panel(TabKind::Favorites);
    }

    /// Open the Bookmarks panel.
    pub fn toggle_bookmarks(&mut self) {
        self.toggle_panel(TabKind::Bookmarks);
    }

    /// Open the Recordings panel.
    pub fn toggle_recordings(&mut self) {
        self.toggle_panel(TabKind::Recordings);
    }

    /// Open the Targets panel.
    pub fn toggle_targets(&mut self) {
        self.toggle_panel(TabKind::Targets);
    }

    /// Open the Accessibility Audit panel.
    pub fn toggle_accessibility_audit(&mut self) {
        self.toggle_panel(TabKind::AccessibilityAudit);
    }

    /// Open the Improvement Engine panel.
    pub fn toggle_improvement_engine(&mut self) {
        self.toggle_panel(TabKind::ImprovementEngine);
    }

    /// Open the Shared Memory panel.
    pub fn toggle_shared_memory(&mut self) {
        self.toggle_panel(TabKind::SharedMemory);
    }

    /// Open the Background Agents panel.
    pub fn toggle_background_agents(&mut self) {
        self.toggle_panel(TabKind::BackgroundAgents);
    }

    /// Open the Conflict Resolver panel.
    pub fn toggle_conflict_resolver(&mut self) {
        self.toggle_panel(TabKind::ConflictResolver);
    }

    /// Open the Collaboration panel.
    pub fn toggle_collaboration(&mut self) {
        self.toggle_panel(TabKind::Collaboration);
    }

    /// Open the Persistent Memory panel.
    pub fn toggle_persistent_memory(&mut self) {
        self.toggle_panel(TabKind::PersistentMemory);
    }

    /// Open the Knowledge / RAG panel.
    pub fn toggle_knowledge(&mut self) {
        self.toggle_panel(TabKind::Knowledge);
    }

    /// Open the unattended-execution Triggers panel.
    pub fn toggle_triggers(&mut self) {
        self.toggle_panel(TabKind::Triggers);
    }

    /// Open the Workflow composer panel.
    pub fn toggle_workflows(&mut self) {
        self.toggle_panel(TabKind::Workflows);
    }

    /// Open the Governance panel (policy, approvals, secrets, connectors).
    pub fn toggle_governance(&mut self) {
        self.toggle_panel(TabKind::Governance);
    }

    pub fn toggle_left_sidebar(&mut self) {
        self.left_sidebar_visible = !self.left_sidebar_visible;
        self.save_workspace_preferences();
    }

    pub fn toggle_right_sidebar(&mut self) {
        self.right_sidebar_visible = !self.right_sidebar_visible;
        self.save_workspace_preferences();
    }

    pub fn reset_workspace_layout(&mut self) {
        let profile = self.appearance.profile;
        self.apply_workspace_profile(profile);
        self.left_sidebar_visible = true;
        self.left_sidebar_width = 240.0;
        self.right_sidebar_visible = true;
        self.right_sidebar_width = 280.0;
        self.save_workspace_preferences();
    }

    // â”€â”€â”€ IDE Feature Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Toggle breakpoint on the current cursor line.
    pub fn toggle_breakpoint_current_line(&mut self) {
        if let Some(id) = &self.active_tab {
            if let Some(buf) = self.buffers.get_mut(id) {
                // Use tracked cursor line (updated during rendering)
                let line = self.current_cursor_line;
                if let Some(pos) = buf.breakpoints.iter().position(|&l| l == line) {
                    buf.breakpoints.remove(pos);
                } else {
                    buf.breakpoints.push(line);
                }
            }
        }
    }

    /// Toggle a bookmark on the current cursor line.
    pub fn toggle_bookmark_current_line(&mut self) {
        let Some(id) = self.active_tab.clone() else {
            self.status_message = "No active editor for bookmark".into();
            return;
        };
        let Some(path) = self.tab_path(&id).cloned() else {
            self.status_message = "Buffer has no file path for bookmark".into();
            return;
        };
        let line = self.current_cursor_line;
        let label = self
            .buffers
            .get(&id)
            .and_then(|buf| buf.content().split('\n').nth(line))
            .map(|l| {
                let t = l.trim();
                if t.chars().count() > 50 {
                    t.chars().take(47).collect::<String>() + "\u{2026}"
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_default();
        let removed = crate::editor::sidebar_tabs::toggle_line_bookmark(
            &mut self.bookmarks,
            &path,
            line,
            &label,
        );
        self.status_message = if removed {
            format!("Bookmark removed (line {})", line + 1)
        } else {
            format!("Bookmark added (line {})", line + 1)
        };
    }

    /// Trigger code completion at cursor position. Merges language-server (LSP)
    /// completions with local sitemap/keyword/identifier suggestions; LSP items
    /// win on label clashes. Degrades gracefully when no language server exists.
    pub fn trigger_completion(&mut self, ctx: &egui::Context) {
        let active_id = self.active_tab.clone();
        let Some(id) = active_id else { return };
        // Snapshot buffer content/path so the immutable borrow ends before we
        // mutably borrow the LSP manager below.
        let snapshot = self
            .buffers
            .get(&id)
            .map(|buf| (buf.content.clone(), buf.path.clone()));
        let Some((content, path)) = snapshot else {
            return;
        };

        // Prefix is anchored at the live caret, not the end of the buffer —
        // otherwise triggering mid-file would offer matches for the last word
        // typed anywhere and commit would replace the wrong span.
        let caret = self
            .active_caret_range(ctx)
            .map(|(_, b)| b)
            .unwrap_or_else(|| content.chars().count());
        let (prefix, prefix_start) =
            crate::editor::completion::char_word_prefix_at(&content, caret);

        // Local (sitemap) suggestions, filtered by prefix.
        let local = crate::editor::completion::CompletionState::compute_items(
            &prefix,
            &self.workspace_symbols,
        );

        // Language-server suggestions at the cursor (empty when unavailable).
        let mut lsp_items = Vec::new();
        if let Some(path) = path.as_ref() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("txt")
                .to_string();
            let line = self.current_cursor_line;
            let col = self.current_cursor_col;
            if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                lsp_items = lsp.completion(&ext, path, line, col, &content);
            }
        }

        let merged = crate::editor::completion::merge_completion_items(lsp_items, local);
        if merged.is_empty() {
            self.status_message = "No completions available".into();
            return;
        }
        // Record the replacement span (in chars) so committing the selection
        // overwrites exactly the typed prefix instead of appending to it.
        self.completion_anchor_tab = Some(id);
        self.completion_state.open(prefix, prefix_start, merged);
    }

    /// Arrow-key navigation while the completion popup is open. No-ops when
    /// the popup is hidden, so plain editor arrows keep caret behavior.
    pub fn completion_move(&mut self, down: bool) {
        if !self.completion_state.active {
            return;
        }
        if down {
            self.completion_state.select_next();
        } else {
            self.completion_state.select_prev();
        }
    }

    /// Commit the highlighted completion item (Tab/Enter with the popup
    /// open): replaces the recorded prefix through the live caret with the
    /// item's insert text, expanding LSP snippet syntax so `$0`/`${1:foo}`
    /// markers never land in the buffer, and parks the caret at the first
    /// tab stop.
    pub fn commit_selected_completion(&mut self, ctx: &egui::Context) {
        if !self.completion_state.active {
            return;
        }
        let item = self
            .completion_state
            .filtered
            .get(self.completion_state.selected)
            .and_then(|&i| self.completion_state.items.get(i))
            .cloned();
        let Some(item) = item else {
            self.completion_state.close();
            return;
        };
        let (insert, stop) = if item.kind == crate::editor::completion::CompletionKind::Snippet {
            crate::editor::completion::expand_snippet_text(&item.insert_text)
        } else {
            let n = item.insert_text.chars().count();
            (item.insert_text.clone(), n)
        };
        let Some(id) = self.active_tab.clone() else {
            self.completion_state.close();
            return;
        };
        let Some(content) = self.buffers.get(&id).map(|b| b.content().to_owned()) else {
            self.completion_state.close();
            return;
        };
        let len = content.chars().count();
        // Replace from the recorded prefix start to the caret (falling back
        // to end-of-buffer when the editor has no live cursor state yet).
        let start = self.completion_state.prefix_start.min(len);
        let end = self
            .active_caret_range(ctx)
            .map(|(_, b)| b)
            .unwrap_or(len)
            .max(start)
            .min(len);
        let (new_text, s, _e) = crate::editor::editor_menu::splice(&content, start, end, &insert);
        let caret = (s + stop).min(s + insert.chars().count());
        self.completion_state.close();
        self.set_active_text_and_selection(ctx, new_text, caret, caret);
        self.status_message = format!("Completed: {}", item.label);
    }

    /// Frame-top completion upkeep: fires a queued (auto-)trigger and then
    /// re-anchors the open popup at the caret's current word, so typing
    /// narrows the list live and commit always replaces exactly what was
    /// typed. Dismisses the popup once nothing matches or the caret left.
    pub fn completion_tick(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.completion_queued_trigger) && !self.completion_state.active {
            self.trigger_completion(ctx);
        }
        if !self.completion_state.active {
            return;
        }
        let Some(id) = self.active_tab.clone() else {
            self.completion_state.close();
            return;
        };
        // Tab switch dismisses: a popup anchored in another buffer would
        // offer that buffer's matches against this buffer's prefix.
        if self.completion_anchor_tab.as_ref() != Some(&id) {
            self.completion_state.close();
            self.completion_anchor_tab = None;
            return;
        }
        let Some(content) = self.buffers.get(&id).map(|b| b.content().to_owned()) else {
            self.completion_state.close();
            return;
        };
        let len = content.chars().count();
        let caret = self.active_caret_range(ctx).map(|(_, b)| b).unwrap_or(len);
        let (prefix, prefix_start) =
            crate::editor::completion::char_word_prefix_at(&content, caret);
        self.completion_state.prefix = prefix;
        self.completion_state.prefix_start = prefix_start;
        self.completion_state.refilter();
        if self.completion_state.filtered.is_empty() {
            self.completion_state.close();
        }
    }

    /// Ask the language server for parameter hints (signature help) at the
    /// caret. `trigger_char` is the `(` or `,` just typed (sent as a
    /// TriggerCharacter request) or `None` for a manual invoke. Cheap and
    /// non-fatal when no server is registered or none advertises a
    /// `signatureHelpProvider` (the manager gate returns `None` without
    /// blocking). Stores the result for [`Self::signature_help_ui`] to render.
    pub fn request_signature_help(&mut self, trigger_char: Option<&str>) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let help = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.signature_help(&ext, &path, line, col, &content, trigger_char),
            None => None,
        };
        self.signature_help = help;
    }

    /// Frame hook, run right after the editor's caret is synced: when the caret
    /// sits immediately after a `(` or `,` (a call just opened or advanced to
    /// the next argument), request parameter hints. De-duped by caret position
    /// so resting on the character fires once, not every frame. Content and
    /// caret are fresh, so the server sees the text as typed.
    pub fn maybe_trigger_signature_help(&mut self) {
        let cur = (self.current_cursor_line, self.current_cursor_col);
        let trigger = self
            .active_tab
            .as_ref()
            .and_then(|id| self.buffers.get(id))
            .and_then(|buf| {
                if cur.1 == 0 {
                    return None;
                }
                let line = buf.content().split('\n').nth(cur.0)?;
                match line.chars().nth(cur.1 - 1) {
                    Some(c) if c == '(' || c == ',' => Some(c.to_string()),
                    _ => None,
                }
            });
        if let Some(ch) = trigger {
            if self.sig_last_pos != Some(cur) {
                self.sig_last_pos = Some(cur);
                self.request_signature_help(Some(&ch));
            }
        }
    }

    // ─── Buffer history commands (single source of truth for chords + dispatch) ───
    //
    // Find/Replace already route through `open_find_active` /
    // `open_find_replace_active`; undo/redo previously lived only as inline
    // chord bodies, so extracting them lets the palette and a rebound
    // keybinding invoke the exact same operation as the stock Ctrl+Z chord.

    /// Ctrl+Z: undo the active buffer's last edit.
    pub fn undo_active(&mut self) {
        if let Some(id) = &self.active_tab {
            if let Some(buf) = self.buffers.get_mut(id) {
                buf.undo();
            }
        }
    }

    /// Ctrl+Shift+Z: redo the active buffer's last undone edit.
    pub fn redo_active(&mut self) {
        if let Some(id) = &self.active_tab {
            if let Some(buf) = self.buffers.get_mut(id) {
                buf.redo();
            }
        }
    }

    /// Snapshot the active buffer's (path, extension, content) for an LSP request.
    pub(crate) fn active_lsp_target(&self) -> Option<(PathBuf, String, String)> {
        let id = self.active_tab.clone()?;
        let buf = self.buffers.get(&id)?;
        let path = buf.path.clone()?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_string();
        Some((path, ext, buf.content.clone()))
    }

    /// F12: jump to the definition of the symbol under the cursor via LSP.
    pub fn goto_definition_at_cursor(&mut self) {
        self.goto_location(GotoKind::Definition);
    }

    /// Jump to the *declaration* of the symbol under the cursor — the header
    /// prototype when definition points at the implementation (C/C++ split).
    pub fn goto_declaration_at_cursor(&mut self) {
        self.goto_location(GotoKind::Declaration);
    }

    /// Jump to the *type* of the symbol under the cursor (Ctrl+click-style
    /// navigation, exposed via the command palette).
    pub fn goto_type_definition_at_cursor(&mut self) {
        self.goto_location(GotoKind::TypeDefinition);
    }

    /// Jump to the concrete implementation(s) of the interface / trait member
    /// under the cursor.
    pub fn goto_implementation_at_cursor(&mut self) {
        self.goto_location(GotoKind::Implementation);
    }

    /// Shared go-to-* navigation: issue the chosen LSP location request at the
    /// caret, then open the first result and jump to its line. Reuses the exact
    /// definition path (push nav history, open editor, set pending cursor line),
    /// degrading to a status message when nothing resolves.
    fn goto_location(&mut self, kind: GotoKind) {
        let noun = kind.noun();
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = format!("No file open for go-to-{}", noun.to_lowercase());
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let locations = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => match kind {
                GotoKind::Definition => lsp.definition(&ext, &path, line, col, &content),
                GotoKind::Declaration => lsp.declaration(&ext, &path, line, col, &content),
                GotoKind::TypeDefinition => lsp.type_definition(&ext, &path, line, col, &content),
                GotoKind::Implementation => lsp.implementation(&ext, &path, line, col, &content),
            },
            None => Vec::new(),
        };
        let Some(target) = locations.into_iter().next() else {
            self.status_message = format!("No {noun} found (no language server result)");
            return;
        };
        let target_path = target.file.clone();
        let target_line = target.line + 1; // LSP 0-based -> editor 1-based
        self.push_nav_location();
        self.open_editor(Some(target.file));
        self.pending_cursor_line = Some(target_line);
        self.status_message = format!("{noun} \u{2192} {}:{}", target_path.display(), target_line);
    }

    /// Ctrl/Cmd+click go-to-definition — the mouse form of F12 that every
    /// competitor ships. The editor widget resolves the click through its
    /// laid-out galley to a char offset ([`crate::editor::code_editor::CodeEditor::goto_request`]);
    /// here the caret is aimed at that offset through the editor's live
    /// `TextEditState` (the shared mechanism behind `find_goto_match` and
    /// `goto_change`, so the click also visually lands the cursor), the app's
    /// tracked line/column is resynced from the tested
    /// [`crate::editor::line_ops::line_col_of_offset`], and the existing LSP
    /// definition request runs at the clicked position.
    pub fn ctrl_click_goto(
        &mut self,
        ctx: &egui::Context,
        id: crate::editor::app::types::TabId,
        char_offset: usize,
    ) {
        let Some(content) = self.buffers.get(&id).map(|b| b.content().to_owned()) else {
            return;
        };
        let (line, col) = crate::editor::line_ops::line_col_of_offset(&content, char_offset);
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(char_offset),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line = line;
        self.current_cursor_col = col;
        self.active_tab = Some(id);
        self.goto_definition_at_cursor();
    }

    /// Act on a link the user Ctrl+clicked in the terminal: URLs open in the
    /// default browser; `path[:line]` file references open in the editor with
    /// navigation history (terminal output lines are 1-based, matching
    /// `pending_cursor_line`). A missing file warns instead of opening a tab.
    pub fn handle_terminal_link(&mut self, link: crate::editor::terminal::LinkKind) {
        use crate::editor::terminal::LinkKind;
        match link {
            LinkKind::Url(url) => {
                crate::editor::terminal::open_url_in_browser(&url);
                self.status_message = format!("Opened {} in browser", url);
            }
            LinkKind::File { path, line } => {
                let raw = std::path::Path::new(&path);
                let abs = if raw.is_absolute() {
                    raw.to_path_buf()
                } else {
                    self.workspace_root.join(raw)
                };
                if !abs.is_file() {
                    self.toasts.push(crate::editor::toast::Toast::warn(format!(
                        "No such file: {}",
                        path
                    )));
                    return;
                }
                self.push_nav_location();
                self.open_editor(Some(abs));
                if let Some(line) = line {
                    self.pending_cursor_line = Some(line.max(1));
                }
            }
        }
    }

    /// Shift+F12: find all references to the symbol under the cursor via LSP and
    /// present them in a navigable popup.
    pub fn find_references_at_cursor(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for find-references".into();
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let locations = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.references(&ext, &path, line, col, &content),
            None => Vec::new(),
        };
        if locations.is_empty() {
            self.references_open = false;
            self.status_message = "No references found (no language server result)".into();
            return;
        }
        self.references_results = locations
            .into_iter()
            .map(|l| (l.file, l.line + 1)) // store 1-based line for the editor
            .collect();
        self.references_selected = 0;
        self.references_open = true;
        self.status_message = format!("{} reference(s) found", self.references_results.len());
    }

    /// Show the LSP call hierarchy for the symbol under the caret: when
    /// `incoming` is set, the functions that *call* it; otherwise the functions
    /// it *calls*. Degrades to a status message when no server answers or the
    /// caret is not on a callable symbol. The result feeds
    /// [`Self::call_hierarchy_ui`], whose rows jump to the caller/callee site.
    pub fn show_call_hierarchy(&mut self, incoming: bool) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for call hierarchy".into();
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let items = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => {
                if incoming {
                    lsp.incoming_calls(&ext, &path, line, col, &content)
                } else {
                    lsp.outgoing_calls(&ext, &path, line, col, &content)
                }
            }
            None => Vec::new(),
        };
        if items.is_empty() {
            self.call_hierarchy_open = false;
            let dir = if incoming { "incoming" } else { "outgoing" };
            self.status_message =
                format!("No {dir} calls (no language server result or not a callable)");
            return;
        }
        let n = items.len();
        self.call_hierarchy_items = items;
        self.call_hierarchy_selected = 0;
        self.call_hierarchy_incoming = incoming;
        self.call_hierarchy_open = true;
        self.status_message = format!(
            "{n} {} call(s)",
            if incoming { "incoming" } else { "outgoing" }
        );
    }

    /// Show LSP hover information for the symbol under the cursor as a toast.
    pub fn show_hover_at_cursor(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for hover".into();
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let hover = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.hover(&ext, &path, line, col, &content),
            None => None,
        };
        match hover {
            Some(h) => {
                let snippet = if h.contents.len() > 240 {
                    format!("{}\u{2026}", &h.contents[..240])
                } else {
                    h.contents.clone()
                };
                self.toasts.push(crate::editor::toast::Toast::info(snippet));
            }
            None => self.status_message = "No hover info (no language server result)".into(),
        }
    }

    /// Format the active document through the language server's formatter
    /// (textDocument/formatting). The buffer's detected indent style is passed
    /// as formatting options; the rewritten content goes through the normal
    /// dirty/undo path so Ctrl+Z restores the pre-format text.
    pub fn format_document_via_lsp(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for formatting".into();
            return;
        };
        let (tab_size, insert_spaces) = match self
            .active_tab
            .as_ref()
            .and_then(|id| self.buffers.get(id))
            .map(|b| b.indent_style)
            .unwrap_or_default()
        {
            crate::editor::auto_indent::IndentStyle::Tabs => (4, false),
            crate::editor::auto_indent::IndentStyle::Spaces(w) => ((w as u64).max(1), true),
        };
        let formatted = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.format_document(&ext, &path, &content, tab_size, insert_spaces),
            None => None,
        };
        match formatted {
            Some(text) => {
                let changed = text != content;
                if let Some(id) = self.active_tab.clone() {
                    if let Some(buf) = self.buffers.get_mut(&id) {
                        // content_mut() marks the buffer mutated, so the next
                        // pre_frame_snapshot pushes the old text onto the undo
                        // stack before the new content renders.
                        *buf.content_mut() = text.clone();
                    }
                }
                // Feed the formatted text back so the server's model stays current.
                if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                    lsp.sync_document(&ext, &path, &text);
                }
                self.status_message = if changed {
                    "Formatted via language server".into()
                } else {
                    "Already formatted (server proposed no edits)".into()
                };
            }
            None => {
                self.status_message =
                    "No formatter available (server lacks a formatting provider)".into()
            }
        }
    }

    /// Open the LSP rename overlay, capturing the caret position so the
    /// rename targets the symbol the cursor was under even after focus moves
    /// to the text input.
    pub fn open_rename_overlay(&mut self) {
        if self.active_lsp_target().is_none() {
            self.status_message = "No file open for rename".into();
            return;
        }
        self.rename_pos = (self.current_cursor_line, self.current_cursor_col);
        self.rename_input.clear();
        self.rename_just_opened = true;
        self.rename_open = true;
    }

    /// Current text of a file: prefer an open (possibly unsaved) buffer so a
    /// rename never clobbers in-editor edits, else the bytes on disk. `None`
    /// when the file is neither open nor readable.
    fn snapshot_file_content(&self, path: &std::path::Path) -> Option<String> {
        if let Some((_, buf)) = self
            .buffers
            .iter()
            .find(|(_, b)| b.path.as_deref() == Some(path))
        {
            return Some(buf.content.clone());
        }
        std::fs::read_to_string(path).ok()
    }

    /// Apply the language server's semantic rename (`textDocument/rename`) across
    /// every file it touched. Open buffers are rewritten in memory through the
    /// normal dirty/undo path; files that are not open are written to disk.
    /// Re-syncs the server's document model afterwards. This is the cross-file,
    /// shadowing-safe counterpart to a blind textual find-replace.
    pub fn commit_rename(&mut self) {
        self.rename_open = false;
        let new_name = self.rename_input.trim().to_string();
        if new_name.is_empty() {
            self.status_message = "Rename cancelled (empty name)".into();
            return;
        }
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for rename".into();
            return;
        };
        let (line, col) = self.rename_pos;
        let edit = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.rename_symbol(&ext, &path, line, col, &new_name, &content),
            None => None,
        };
        let Some(edit) = edit else {
            self.status_message =
                "Rename unavailable (server lacks a rename provider or proposed no edits)".into();
            return;
        };
        let files_changed = self.apply_lsp_workspace_edit(&edit);
        self.status_message = format!("Renamed to '{}' across {} file(s)", new_name, files_changed);
    }

    /// Apply a language-server `WorkspaceEdit` (rename or code action) across
    /// every file it touched. Open buffers are rewritten in memory through the
    /// normal dirty/undo path; files that are not open are written to disk.
    /// Re-syncs the server's document model afterwards so its view matches the
    /// new text. Returns the number of files rewritten.
    pub(crate) fn apply_lsp_workspace_edit(
        &mut self,
        edit: &crate::editor::lsp_client::LspWorkspaceEdit,
    ) -> usize {
        // Resolve every touched file's current text up front so the closure's
        // immutable borrow of `self` ends before the mutation loop below.
        let rewritten = crate::editor::lsp_client::apply_workspace_edit(edit, |p| {
            self.snapshot_file_content(p)
        });
        let mut files_changed = 0usize;
        for (fp, text) in &rewritten {
            // Decide open-vs-on-disk with an owned id so the iterator borrow
            // ends before `get_mut`.
            let open_id = self
                .buffers
                .iter()
                .find(|(_, b)| b.path.as_deref() == Some(fp.as_path()))
                .map(|(id, _)| id.clone());
            if let Some(id) = open_id {
                if let Some(buf) = self.buffers.get_mut(&id) {
                    *buf.content_mut() = text.clone();
                }
            } else {
                let _ = std::fs::write(fp, text);
            }
            files_changed += 1;
        }
        // Feed rewritten buffers back so the server's model stays current.
        if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
            for (fp, text) in &rewritten {
                let fe = fp
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt")
                    .to_string();
                lsp.sync_document(&fe, fp, text);
            }
        }
        files_changed
    }

    /// Open the LSP code-action overlay (quick fixes / refactorings) at the
    /// caret's line. Fetches synchronously through the shared manager (bounded
    /// await), so a server that lacks a `codeActionProvider` or times out just
    /// leaves a status message rather than an empty popup.
    pub fn open_code_actions_overlay(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for code actions".into();
            return;
        };
        let (line, col) = (self.current_cursor_line, self.current_cursor_col);
        self.code_actions_pos = (line, col);
        let actions = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.code_actions(&ext, &path, ((line, col), (line, col)), &content),
            None => Vec::new(),
        };
        if actions.is_empty() {
            self.status_message = "No code actions available here".into();
            return;
        }
        self.code_actions = actions;
        self.code_action_selected = 0;
        self.code_actions_open = true;
    }

    /// Apply the currently-selected code action. Actions carrying an inline
    /// `workspaceEdit` go through the same multi-file apply path as rename.
    /// Actions that only name a server-side `command` are run via
    /// `workspace/executeCommand`; the edits the server pushes back (through
    /// `workspace/applyEdit`) are then applied the same way.
    pub fn apply_selected_code_action(&mut self) {
        self.code_actions_open = false;
        let Some(action) = self.code_actions.get(self.code_action_selected).cloned() else {
            return;
        };
        // Servers advertising `resolveProvider` hand back lightweight actions
        // with no edit and no command (rust-analyzer's expensive refactorings
        // work this way): echo the action to `codeAction/resolve` and use the
        // completed copy before the dispatch below. If the server can't or
        // won't resolve, the original stays and the paths below report it
        // produced no edits — same outcome as before, never a wrong apply.
        let mut action = action;
        if action.edit.as_ref().is_none_or(|e| e.is_empty()) && action.command.is_none() {
            if let Some((_, ext, _)) = self.active_lsp_target() {
                if let Some(lsp) = self.lsp_state.lsp_manager.as_mut() {
                    if let Some(resolved) = lsp.resolve_code_action(&ext, &action) {
                        action = resolved;
                    }
                }
            }
        }
        // Prefer an inline edit; otherwise resolve edits by running the command.
        let edit = match (action.edit.clone(), action.command.clone()) {
            (Some(edit), _) if !edit.is_empty() => Some(edit),
            (_, Some(cmd)) => {
                let Some((path, ext, content)) = self.active_lsp_target() else {
                    self.status_message = "No file open to run code action".into();
                    return;
                };
                match self.lsp_state.lsp_manager.as_mut() {
                    Some(lsp) => {
                        lsp.execute_code_action(&ext, &path, &cmd.command, cmd.arguments, &content)
                    }
                    None => None,
                }
            }
            (Some(edit), _) => Some(edit), // empty inline edit → reported below
            (None, None) => None,
        };
        let Some(edit) = edit else {
            self.status_message = format!("'{}' produced no edits", action.title);
            return;
        };
        if edit.is_empty() {
            self.status_message = format!("'{}' proposed no edits", action.title);
            return;
        }
        let files_changed = self.apply_lsp_workspace_edit(&edit);
        self.status_message = format!(
            "Applied '{}' across {} file(s)",
            action.title, files_changed
        );
    }

    /// Open the in-file Find overlay on the active editor.
    pub fn open_find_active(&mut self) {
        if let Some(id) = self.active_tab.clone() {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.find_replace.open_find();
            }
        }
    }

    /// Open the in-file Find+Replace overlay on the active editor.
    pub fn open_find_replace_active(&mut self) {
        if let Some(id) = self.active_tab.clone() {
            if let Some(buf) = self.buffers.get_mut(&id) {
                buf.find_replace.open_find_replace();
            }
        }
    }

    /// Get git status for the workspace.
    pub fn refresh_git_status(&mut self) {
        self.git_state = crate::editor::git_ui::GitState::from_workspace(&self.workspace_root);
    }

    /// Render the debug panel (call stack, variables, watches, toolbar).
    pub fn render_debug_panel(
        &mut self,
        ui: &mut eframe::egui::Ui,
        palette: crate::editor::theme::IdePalette,
    ) {
        use crate::editor::debugger::DebugState;
        use eframe::egui;

        let state = self
            .dap_client
            .as_ref()
            .map(|d| d.state)
            .unwrap_or(DebugState::Inactive);

        // Debug toolbar
        ui.horizontal(|ui| {
            let state_label = match state {
                DebugState::Inactive => "Inactive",
                DebugState::Starting => "Starting",
                DebugState::Running => "Running",
                DebugState::Paused => "Paused",
                DebugState::Stopped => "Stopped",
            };
            ui.label(
                egui::RichText::new(format!("{} {}", egui_phosphor::regular::BUG, state_label))
                    .size(10.0)
                    .color(match state {
                        DebugState::Running => palette.success,
                        DebugState::Paused => palette.warning,
                        DebugState::Stopped => palette.error,
                        _ => palette.text_muted,
                    }),
            );

            ui.add_space(8.0);
            let can_continue = state == DebugState::Paused;
            let can_step = state == DebugState::Paused;
            let can_stop = state == DebugState::Running || state == DebugState::Paused;

            if ui
                .add_enabled(
                    can_continue,
                    egui::Button::new(format!("{} Continue", egui_phosphor::regular::PLAY)),
                )
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.continue_execution();
                }
            }
            if ui
                .add_enabled(
                    can_step,
                    egui::Button::new(format!("{} Step Over", egui_phosphor::regular::ARROW_RIGHT)),
                )
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_over();
                }
            }
            if ui
                .add_enabled(
                    can_step,
                    egui::Button::new(format!("{} Step Into", egui_phosphor::regular::ARROW_DOWN)),
                )
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_into();
                }
            }
            if ui
                .add_enabled(
                    can_step,
                    egui::Button::new(format!("{} Step Out", egui_phosphor::regular::ARROW_UP)),
                )
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_out();
                }
            }
            if ui
                .add_enabled(
                    can_stop,
                    egui::Button::new(format!("{} Stop", egui_phosphor::regular::STOP))
                        .fill(palette.error),
                )
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.stop();
                }
            }
        });
        ui.separator();

        if state == DebugState::Inactive {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(egui_phosphor::regular::BUG)
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.6)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("No active debug session")
                        .size(11.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Launch a session to hit breakpoints and inspect state.")
                        .size(9.0)
                        .color(palette.text_muted),
                );
                ui.add_space(8.0);
                if primary_button(
                    ui,
                    palette,
                    format!("{} Start debugging", egui_phosphor::regular::PLAY),
                )
                .on_hover_text("Launch a DAP session for the active target (F5)")
                .clicked()
                {
                    self.launch_debug_session();
                }
            });
            return;
        }

        // Split: Call Stack | Variables | Watches
        ui.columns(3, |cols| {
            // Call Stack
            cols[0].label(
                egui::RichText::new("Call Stack")
                    .size(9.0)
                    .strong()
                    .color(palette.accent),
            );
            if let Some(dap) = &self.dap_client {
                for frame in &dap.stack_frames {
                    let file = frame
                        .file
                        .as_ref()
                        .map(|f| {
                            f.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                        })
                        .unwrap_or_default();
                    cols[0].label(
                        egui::RichText::new(format!("  {} ({}:{})", frame.name, file, frame.line))
                            .monospace()
                            .size(9.0)
                            .color(palette.text),
                    );
                }
                if dap.stack_frames.is_empty() {
                    cols[0].label(
                        egui::RichText::new("  (no frames)")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
            }

            // Variables
            cols[1].label(
                egui::RichText::new("Variables")
                    .size(9.0)
                    .strong()
                    .color(palette.accent),
            );
            if let Some(dap) = &self.dap_client {
                for var in &dap.variables {
                    let type_hint = var.type_name.as_deref().unwrap_or("");
                    cols[1].label(
                        egui::RichText::new(format!(
                            "  {} = {} {}",
                            var.name, var.value, type_hint
                        ))
                        .monospace()
                        .size(9.0)
                        .color(palette.text),
                    );
                }
                if dap.variables.is_empty() {
                    cols[1].label(
                        egui::RichText::new("  (no variables)")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
            }

            // Watches
            cols[2].label(
                egui::RichText::new("Watches")
                    .size(9.0)
                    .strong()
                    .color(palette.accent),
            );
            if let Some(dap) = &self.dap_client {
                for watch in &dap.watches {
                    let result = watch.result.as_deref().unwrap_or("<not evaluated>");
                    cols[2].label(
                        egui::RichText::new(format!("  {} = {}", watch.expression, result))
                            .monospace()
                            .size(9.0)
                            .color(palette.text),
                    );
                }
                if dap.watches.is_empty() {
                    cols[2].label(
                        egui::RichText::new("  (no watches)")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
            }
        });

        // Breakpoints list
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Breakpoints")
                    .size(9.0)
                    .strong()
                    .color(palette.accent),
            );
            if let Some(dap) = &self.dap_client {
                ui.label(
                    egui::RichText::new(format!("({})", dap.breakpoints.len()))
                        .size(9.0)
                        .color(palette.text_muted),
                );
            }
        });
        if let Some(dap) = &self.dap_client {
            if dap.breakpoints.is_empty() {
                ui.label(
                    egui::RichText::new("  No breakpoints set. Click in the gutter to toggle.")
                        .size(8.0)
                        .color(palette.text_muted),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(80.0)
                    .show(ui, |ui| {
                        for bp in &dap.breakpoints {
                            let file_name =
                                bp.file.file_name().unwrap_or_default().to_string_lossy();
                            let enabled_mark = if bp.enabled { "\u{2611}" } else { "\u{2610}" };
                            ui.label(
                                egui::RichText::new(format!(
                                    "  {} {}:{}",
                                    enabled_mark, file_name, bp.line
                                ))
                                .monospace()
                                .size(9.0)
                                .color(if bp.enabled {
                                    palette.text
                                } else {
                                    palette.text_muted
                                }),
                            );
                        }
                    });
            }
        }

        // Watch expression input
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Add Watch:")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            let mut new_watch = String::new();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut new_watch)
                        .hint_text("expression\u{2026}")
                        .desired_width(150.0),
                )
                .lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !new_watch.trim().is_empty()
            {
                if let Some(dap) = &mut self.dap_client {
                    dap.add_watch(new_watch.trim().to_string());
                }
            }
        });
    }

    /// Launch a debug session. Auto-detects the debug adapter based on project type.
    pub fn launch_debug_session(&mut self) {
        use crate::editor::debugger::{DapClient, LaunchConfig};

        // Determine the binary to debug based on workspace type
        let cargo_toml = self.workspace_root.join("Cargo.toml");
        if cargo_toml.exists() {
            // Rust project -- look for the target binary
            let target_dir = self.workspace_root.join("target").join("debug");
            let project_name = self
                .workspace_root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .replace('-', "_");

            let binary = if cfg!(target_os = "windows") {
                target_dir.join(format!("{}.exe", project_name))
            } else {
                target_dir.join(&project_name)
            };

            if !binary.exists() {
                self.status_message = format!(
                    "Debug: binary not found at {}. Run 'cargo build' first.",
                    binary.display()
                );
                self.toasts.push(crate::editor::toast::Toast::error(
                    "Build project before debugging (cargo build)",
                ));
                return;
            }

            let config = LaunchConfig::rust_debug(&binary, &self.workspace_root);
            let mut dap = DapClient::new();
            match dap.launch(&config) {
                Ok(()) => {
                    self.dap_client = Some(dap);
                    self.status_message = "Debug: session started".to_string();
                    self.toasts.push(crate::editor::toast::Toast::success(
                        "Debug session launched",
                    ));
                    // Open debug tab in bottom panel
                    self.bottom_panel_state.collapsed = false;
                    self.bottom_panel_state.active_tab = 2; // Debug tab
                }
                Err(e) => {
                    self.status_message = format!("Debug: failed to launch \u{2014} {}", e);
                    self.toasts.push(crate::editor::toast::Toast::error(format!(
                        "Debug launch failed: {}",
                        e
                    )));
                }
            }
        } else {
            self.status_message = "Debug: no supported project found (Cargo.toml)".to_string();
            self.toasts.push(crate::editor::toast::Toast::info(
                "No debuggable project detected. Only Rust (codelldb) is supported currently.",
            ));
        }
    }

    // ─── Debug control (single source of truth for F5/F10/F11 + palette) ───

    /// F5: continue a running session, or launch one when none is active.
    pub fn debug_start_or_continue(&mut self) {
        if let Some(dap) = &mut self.dap_client {
            let _ = dap.continue_execution();
        } else {
            self.launch_debug_session();
        }
    }

    /// Shift+F5: terminate the debug session and drop the client so the next
    /// F5 launches fresh (mirrors clearing the `debugActive` context).
    pub fn debug_stop(&mut self) {
        if let Some(dap) = &mut self.dap_client {
            let _ = dap.stop();
        }
        self.dap_client = None;
        self.status_message = "Debug: session stopped".to_string();
    }

    /// F10: step over the current line. No-op when no session is active.
    pub fn debug_step_over(&mut self) {
        if let Some(dap) = &mut self.dap_client {
            let _ = dap.step_over();
        }
    }

    /// F11: step into the call on the current line.
    pub fn debug_step_into(&mut self) {
        if let Some(dap) = &mut self.dap_client {
            let _ = dap.step_into();
        }
    }

    /// Shift+F11: step out of the current function back to its caller.
    pub fn debug_step_out(&mut self) {
        if let Some(dap) = &mut self.dap_client {
            let _ = dap.step_out();
        }
    }

    // â”€â”€â”€ Semantic Search Integration â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Run a semantic (TF-IDF similarity) search and produce SearchHit results.
    pub fn run_semantic_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_hits.clear();
            return;
        }
        // Ensure the index is built.
        if self.semantic_index.is_none() {
            self.semantic_index = Some(crate::editor::semantic_search::SemanticIndex::build(
                &self.workspace_root,
            ));
        }
        if let Some(ref index) = self.semantic_index {
            let hits = index
                .search(&self.search_query, 50)
                .into_iter()
                .map(|h| crate::editor::search::SearchHit {
                    path: h.path,
                    line: 1,
                    text: format!("[{:.0}%] {}", h.score * 100.0, h.preview),
                })
                .collect();
            self.update_search_hits(hits);
        }
    }

    // â”€â”€â”€ Inline Suggestions LLM Wiring â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Request an inline ghost-text suggestion from the configured LLM provider.
    /// Called on cursor pause after debounce timer (see code_editor integration).
    pub fn request_inline_suggestion(&mut self) {
        use crate::editor::inline_suggestions::SuggestionRequest;

        let (file_path, prefix, suffix, language) = match self.active_tab.as_ref().and_then(|id| {
            let path = self.tab_path(id)?.clone();
            let buf = self.buffers.get(id)?;
            let content = buf.content().to_string();
            // Split at roughly the middle or the end (no cursor byte available)
            // Use the last 500 chars as prefix context
            let split = content.len().min(2000);
            let prefix = content[..split].to_string();
            let suffix = content[split..].to_string();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");
            let language = match ext {
                "rs" => "rust",
                "py" => "python",
                "js" | "jsx" => "javascript",
                "ts" | "tsx" => "typescript",
                "go" => "go",
                "java" => "java",
                _ => "plaintext",
            };
            Some((path, prefix, suffix, language.to_string()))
        }) {
            Some(tuple) => tuple,
            None => return,
        };

        let request = SuggestionRequest {
            file_path,
            prefix,
            suffix,
            language,
        };

        // Submit to the suggestion engine for async resolution.
        self.inline_suggestions.submit_request(
            request,
            self.provider,
            &self.selected_model,
            self.workspace_root.clone(),
        );
    }

    /// Apply a whole-line editing operation (duplicate / delete / move up /
    /// move down) to the active buffer at the caret's line. The transform runs
    /// in the pure [`crate::editor::line_ops`] module over a char-offset caret,
    /// so the multi-byte / edge-case behaviour is unit-tested independently of
    /// egui. The rewritten text is installed through `content_mut()` (the same
    /// dirty/generation path LSP formatting uses), and the caret is re-aimed by
    /// mutating egui's live `TextEditState` -- which preserves the shared undoer
    /// so the whole-line edit stays undoable via Ctrl+Z.
    pub fn apply_line_op(&mut self, ctx: &egui::Context, op: crate::editor::line_ops::LineOp) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        // Prefer the live caret char offset from egui's edit state; fall back to
        // the last line/column the app tracked (e.g. a rebound chord fired while
        // the editor is not focused) so the command still has a sensible target.
        let live_cursor = egui::widgets::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let char_idx: usize = r.primary.index.into();
                char_idx
            });
        let Some(content) = self.buffers.get(&id).map(|buf| buf.content().to_owned()) else {
            return;
        };
        let cursor = live_cursor.unwrap_or_else(|| {
            crate::editor::line_ops::offset_of_line_col(
                &content,
                self.current_cursor_line,
                self.current_cursor_col,
            )
        });
        let Some((new_content, new_cursor)) =
            crate::editor::line_ops::apply_line_op(&content, op, cursor)
        else {
            // No-op at this position (e.g. move up from the first line): leave
            // the buffer and undo stack untouched.
            return;
        };
        if let Some(buf) = self.buffers.get_mut(&id) {
            *buf.content_mut() = new_content.clone();
        }
        // Re-aim the caret by cloning + mutating the stored state so the shared
        // undoer (an `Arc` inside `TextEditState`) survives the round-trip.
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(new_cursor),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line =
            crate::editor::line_ops::line_number_of_offset(&new_content, new_cursor) - 1;
    }

    /// Toggle a line comment across the active editor's selection (or caret
    /// line). The comment token is chosen from the file extension via
    /// [`crate::editor::line_ops::line_comment_token`]; unknown or
    /// block-comment-only types are a silent no-op rather than a corruption.
    /// The transform runs in the pure line-ops module over char offsets, the
    /// result is installed through `content_mut()` (the same dirty/generation
    /// path the whole-line edits use), and the caret/selection is re-aimed on
    /// egui's live `TextEditState` so the shared undoer survives and the change
    /// stays undoable via Ctrl+Z.
    pub fn toggle_line_comment(&mut self, ctx: &egui::Context) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        let Some(content) = self.buffers.get(&id).map(|buf| buf.content().to_owned()) else {
            return;
        };
        // Resolve the token from the extension; bail (with a status hint) when we
        // have no safe line-comment syntax for this file.
        let ext = self
            .buffers
            .get(&id)
            .and_then(|b| b.path.as_ref())
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let Some(token) = crate::editor::line_ops::line_comment_token(&ext) else {
            self.status_message = "Toggle comment: no line-comment syntax for this file".into();
            return;
        };
        let char_count = content.chars().count();
        // Prefer the live selection (primary/secondary char offsets); fall back
        // to the tracked caret line so a rebound chord fired while the editor is
        // not focused still has a sensible target.
        let (start, end) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let a: usize = r.primary.index.into();
                let b: usize = r.secondary.index.into();
                (a.min(char_count), b.min(char_count))
            })
            .unwrap_or_else(|| {
                let off = crate::editor::line_ops::offset_of_line_col(
                    &content,
                    self.current_cursor_line,
                    self.current_cursor_col,
                );
                (off, off)
            });
        let Some((new_content, sel_start, sel_end)) =
            crate::editor::line_ops::toggle_line_comment(&content, token, start, end)
        else {
            // Nothing to change (blank block / empty doc): leave buffer + undo
            // stack untouched.
            return;
        };
        if let Some(buf) = self.buffers.get_mut(&id) {
            *buf.content_mut() = new_content.clone();
        }
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(sel_start),
                    egui::text::CCursor::new(sel_end),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line =
            crate::editor::line_ops::line_number_of_offset(&new_content, sel_end) - 1;
    }

    /// Indent (`indent == true`) or dedent the block of lines the current
    /// selection (or caret line) touches, using the buffer's detected indent
    /// style. The rewrite runs through the already-tested
    /// [`crate::editor::auto_indent`] transforms; the caret is then re-aimed to
    /// re-select the affected block on egui's live `TextEditState`, so the
    /// whole operation stays undoable via Ctrl+Z (the shared undoer survives the
    /// load/clone/store round-trip). A no-op (e.g. dedenting already
    /// column-zero lines) leaves the buffer and undo stack untouched.
    pub fn apply_indent(&mut self, ctx: &egui::Context, indent: bool) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        let (content, style) = match self.buffers.get(&id) {
            Some(buf) => (buf.content().to_owned(), buf.indent_style),
            None => return,
        };
        let char_count = content.chars().count();
        // Live selection endpoints (char offsets); fall back to the tracked caret
        // line so a rebound chord fired while the editor is not focused still
        // has a sensible target.
        let (start, end) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let a: usize = r.primary.index.into();
                let b: usize = r.secondary.index.into();
                (a.min(char_count), b.min(char_count))
            })
            .unwrap_or_else(|| {
                let off = crate::editor::line_ops::offset_of_line_col(
                    &content,
                    self.current_cursor_line,
                    self.current_cursor_col,
                );
                (off, off)
            });
        let (first, last) = crate::editor::line_ops::line_range_of(&content, start, end);
        let new_content = if indent {
            crate::editor::auto_indent::indent_lines(&content, first, last, style)
        } else {
            crate::editor::auto_indent::dedent_lines(&content, first, last, style)
        };
        if new_content == content {
            return; // nothing changed: leave buffer + undo stack untouched
        }
        if let Some(buf) = self.buffers.get_mut(&id) {
            *buf.content_mut() = new_content.clone();
        }
        // Re-select the affected block in the editor.
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            let s = crate::editor::line_ops::offset_of_line_col(&new_content, first, 0);
            let e = crate::editor::line_ops::offset_of_line_col(&new_content, last, usize::MAX);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(s),
                    egui::text::CCursor::new(e),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line = first;
    }

    /// The live caret range (char offsets, ordered + clamped) on the active
    /// editor's `TextEditState`; `None` when no editor tab or no caret state.
    fn active_caret_range(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        let id = self.active_tab.as_ref()?;
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(id);
        let len = self.buffers.get(id)?.content().chars().count();
        let range = egui::widgets::text_edit::TextEditState::load(ctx, editor_id)?
            .cursor
            .char_range()?;
        let a: usize = range.primary.index.into();
        let b: usize = range.secondary.index.into();
        Some(crate::editor::editor_menu::normalize_range(a, b, len))
    }

    /// Install new text on the active buffer and re-aim the caret/selection on
    /// the live `TextEditState` — the same load/mutate/store round-trip
    /// `apply_line_op` uses, which keeps the shared undoer (and thus Ctrl+Z)
    /// alive across programmatic edits.
    fn set_active_text_and_selection(
        &mut self,
        ctx: &egui::Context,
        text: String,
        sel_start: usize,
        sel_end: usize,
    ) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        if let Some(buf) = self.buffers.get_mut(&id) {
            *buf.content_mut() = text.clone();
        }
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(sel_start),
                    egui::text::CCursor::new(sel_end),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line =
            crate::editor::line_ops::line_number_of_offset(&text, sel_end) - 1;
    }

    /// Execute one action collected from the editor's right-click context
    /// menu. Clipboard ops work through the live selection on egui's
    /// `TextEditState` and the pure `editor_menu` text operations; the
    /// remainder delegate to the existing editor commands so the menu never
    /// forks behavior (toggle comment, LSP format, code actions, go-to).
    pub fn run_editor_menu_action(
        &mut self,
        ctx: &egui::Context,
        action: crate::editor::editor_menu::EditorMenuAction,
    ) {
        use crate::editor::editor_menu::{self, EditorMenuAction};
        match action {
            EditorMenuAction::ToggleComment => self.toggle_line_comment(ctx),
            EditorMenuAction::FormatDocument => self.format_document_via_lsp(),
            EditorMenuAction::CodeActions => self.open_code_actions_overlay(),
            EditorMenuAction::GoToDefinition => self.goto_definition_at_cursor(),
            EditorMenuAction::Rename => self.open_rename_overlay(),
            EditorMenuAction::CopyPath(p) => {
                ctx.copy_text(p.display().to_string());
                self.status_message = format!("Copied path: {}", p.display());
            }
            EditorMenuAction::Copy => match self.active_caret_range(ctx) {
                Some((a, b)) if a != b => {
                    let text = self
                        .active_tab
                        .as_ref()
                        .and_then(|id| self.buffers.get(id))
                        .map(|buf| editor_menu::slice_chars(buf.content(), a, b))
                        .unwrap_or_default();
                    ctx.copy_text(text.clone());
                    self.status_message = format!("Copied {} characters", text.chars().count());
                }
                _ => self.status_message = "Copy: nothing selected".into(),
            },
            EditorMenuAction::Cut => {
                let Some((a, b)) = self.active_caret_range(ctx) else {
                    self.status_message = "Cut: nothing selected".into();
                    return;
                };
                if a == b {
                    self.status_message = "Cut: nothing selected".into();
                    return;
                }
                let Some(content) = self
                    .active_tab
                    .as_ref()
                    .and_then(|id| self.buffers.get(id))
                    .map(|buf| buf.content().to_owned())
                else {
                    return;
                };
                ctx.copy_text(editor_menu::slice_chars(&content, a, b));
                let (new_content, s, e) = editor_menu::splice(&content, a, b, "");
                self.status_message = "Cut".into();
                self.set_active_text_and_selection(ctx, new_content, s, e);
            }
            EditorMenuAction::Paste => {
                let Some((a, b)) = self.active_caret_range(ctx) else {
                    self.status_message = "Paste: no editor caret".into();
                    return;
                };
                let Some(content) = self
                    .active_tab
                    .as_ref()
                    .and_then(|id| self.buffers.get(id))
                    .map(|buf| buf.content().to_owned())
                else {
                    return;
                };
                // egui can only write the clipboard; reads go through the
                // system clipboard directly (already a transitive dependency).
                let clip = arboard::Clipboard::new()
                    .and_then(|mut c| c.get_text())
                    .unwrap_or_default();
                if clip.is_empty() {
                    self.status_message = "Paste: clipboard is empty".into();
                    return;
                }
                let (new_content, s, e) = editor_menu::splice(&content, a, b, &clip);
                self.status_message = "Pasted from clipboard".into();
                self.set_active_text_and_selection(ctx, new_content, s, e);
            }
            EditorMenuAction::SelectAll => {
                let Some(id) = self.active_tab.clone() else {
                    return;
                };
                let Some(len) = self
                    .buffers
                    .get(&id)
                    .map(|buf| buf.content().chars().count())
                else {
                    return;
                };
                let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
                if let Some(mut state) =
                    egui::widgets::text_edit::TextEditState::load(ctx, editor_id)
                {
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(len),
                        )));
                    state.store(ctx, editor_id);
                    self.status_message = "Selected all".into();
                }
            }
        }
    }

    /// Jump the editor caret to the find overlay's current match and advance the
    /// cursor for the next call — this is `edit.find_next` (F3) / `edit.find_prev`
    /// (Shift+F3). The find bar keeps its own focus while the user cycles, so
    /// navigation must drive the editor's `TextEditState` directly: the match's
    /// byte offsets are converted to char indices (via the tested
    /// [`crate::editor::find_replace::char_index_of_byte`]) and applied as a
    /// selection, which both highlights the hit and scrolls it into view on the
    /// next layout. No-ops quietly when no find is open or there are no matches.
    pub fn find_goto_match(&mut self, ctx: &egui::Context, forward: bool) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        let Some(content) = self.buffers.get(&id).map(|buf| buf.content().to_owned()) else {
            return;
        };
        // Resolve (and advance) under a mutable borrow that ends before we touch
        // egui's per-widget state.
        let target = self.buffers.get_mut(&id).and_then(|buf| {
            let fr = &mut buf.find_replace;
            if !fr.visible || fr.query.is_empty() {
                return None;
            }
            // Recompute so the byte offsets are valid against the live buffer.
            fr.recompute_matches(&content);
            if fr.matches.is_empty() {
                return None;
            }
            let (start, end) = fr.matches[fr.current_match];
            // Move the cursor for the following invocation.
            if forward {
                fr.next_match();
            } else {
                fr.prev_match();
            }
            Some((
                crate::editor::find_replace::char_index_of_byte(&content, start),
                crate::editor::find_replace::char_index_of_byte(&content, end),
            ))
        });
        let Some((cs, ce)) = target else {
            return;
        };
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(cs),
                    egui::text::CCursor::new(ce),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line = crate::editor::line_ops::line_number_of_offset(&content, cs) - 1;
    }

    /// Go to next / previous change (`edit.next_change` / `edit.prev_change`,
    /// Alt+Down / Alt+Up in other editors): jump the caret to the line that
    /// differs from the buffer's last-saved baseline, scanning forward (or
    /// backward) from the caret and wrapping around, so repeated presses cycle
    /// through every unsaved hunk. Reuses the gutter's diff machinery —
    /// `refresh_diff_marks` + the tested [`crate::editor::line_ops::next_changed_line`]
    /// — and drives the caret through the editor's live `TextEditState`, the
    /// same mechanism `find_goto_match` uses, so the target line scrolls into
    /// view. On a clean buffer (or one with no changes left of the caret in
    /// this direction … which wrapping makes rare) it reports and no-ops.
    pub fn goto_change(&mut self, ctx: &egui::Context, forward: bool) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        let from_line = self.current_cursor_line;
        let mut target = None;
        let mut content = String::new();
        if let Some(buf) = self.buffers.get_mut(&id) {
            buf.refresh_diff_marks();
            target =
                crate::editor::line_ops::next_changed_line(&buf.diff_marks, from_line, forward);
            content = buf.content().to_owned();
        }
        let Some(line) = target else {
            self.status_message = "No unsaved changes to jump to".into();
            return;
        };
        let caret = crate::editor::line_ops::offset_of_line_col(&content, line, 0);
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(caret),
                    egui::text::CCursor::new(caret),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line = line;
        self.current_cursor_col = 0;
        self.status_message = format!("Jumped to change on line {}", line + 1);
    }

    /// F8 / Shift+F8 — go to the next/previous problem, the navigation half of
    /// the Problems panel every competitor ships. Selection is the tested
    /// [`crate::editor::diagnostics::DiagnosticsState::problem_at`]: panel
    /// order, filter-honouring, wrapping. The jump reuses the same machinery
    /// as symbol navigation — `open_editor` reuses or focuses the file's tab
    /// (path identity keeps it to one), and `pending_cursor_line` aims the
    /// caret on the next frame. The tracked caret is resynced immediately so
    /// repeated presses keep cycling instead of ping-ponging on one problem.
    pub fn goto_problem(&mut self, forward: bool) {
        let active_path = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tab_path(id).cloned());
        let from_line = self.current_cursor_line;
        let Some(diag) = self
            .lsp_state
            .diagnostics
            .problem_at(active_path.as_deref(), from_line, forward)
            .cloned()
        else {
            self.status_message = if self.lsp_state.diagnostics.items.is_empty() {
                "No problems reported".into()
            } else {
                "No problems match the filter".into()
            };
            return;
        };
        self.jump_to_diagnostic(&diag);
    }

    /// Shared landing sink for F8 navigation *and* Problems-panel row clicks:
    /// opens the reported file (path identity keeps it to one tab), aims the
    /// caret on the next frame, and shows severity · file:line · message in
    /// the status bar. The tracked caret is resynced immediately so repeated
    /// presses keep cycling instead of ping-ponging on one problem.
    pub fn jump_to_diagnostic(&mut self, diag: &crate::editor::diagnostics::LspDiagnostic) {
        let active_path = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tab_path(id).cloned());
        if active_path.as_deref() != Some(diag.file.as_path()) {
            self.push_nav_location();
            self.open_editor(Some(diag.file.clone()));
        }
        self.pending_cursor_line = Some(diag.line + 1);
        self.current_cursor_line = diag.line;
        self.current_cursor_col = diag.col;
        let name = diag
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| diag.file.display().to_string());
        let severity = match diag.severity {
            crate::editor::diagnostics::DiagnosticSeverity::Error => "Error",
            crate::editor::diagnostics::DiagnosticSeverity::Warning => "Warning",
            crate::editor::diagnostics::DiagnosticSeverity::Info => "Info",
            crate::editor::diagnostics::DiagnosticSeverity::Hint => "Hint",
        };
        self.status_message = format!(
            "{severity} \u{00b7} {}:{} \u{2014} {}",
            name,
            diag.line + 1,
            diag.message
        );
    }

    /// "Smart selection" (Expand Selection / Shrink Selection): grow or reduce
    /// the current selection to the next semantic scope reported by the
    /// language server's `textDocument/selectionRange`. The nested ranges are
    /// converted to char offsets (via the tested `line_ops` helpers) and walked
    /// with the pure [`crate::editor::lsp_client::wider_selection`] /
    /// [`narrower_selection`] transforms; the result is installed on the
    /// editor's live `TextEditState`, the same mechanism `edit.find_next` uses,
    /// so the new selection highlights and scrolls into view. Degrades quietly
    /// when no provider answers or the selection is already at the
    /// innermost/outermost scope.
    pub fn select_surrounding_item(&mut self, ctx: &egui::Context, expand: bool) {
        let Some(id) = self.active_tab.clone() else {
            return;
        };
        let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for smart selection".into();
            return;
        };
        let char_count = content.chars().count();
        // Current selection endpoints (char offsets); fall back to the tracked
        // caret when the editor is not focused / has no selection.
        let (cur_s, cur_e) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let a: usize = r.primary.index.into();
                let b: usize = r.secondary.index.into();
                (a.min(char_count), b.min(char_count))
            })
            .unwrap_or_else(|| {
                let off = crate::editor::line_ops::offset_of_line_col(
                    &content,
                    self.current_cursor_line,
                    self.current_cursor_col,
                );
                (off, off)
            });
        let ranges = match self.lsp_state.lsp_manager.as_mut() {
            Some(lsp) => lsp.selection_ranges(
                &ext,
                &path,
                self.current_cursor_line,
                self.current_cursor_col,
                &content,
            ),
            None => Vec::new(),
        };
        if ranges.is_empty() {
            self.status_message = "Smart selection unavailable (no selectionRange provider)".into();
            return;
        }
        // LSP (line, character) -> char offsets, clamped into the document.
        let spans: Vec<(usize, usize)> = ranges
            .iter()
            .map(|r| {
                let s = crate::editor::line_ops::offset_of_line_col(
                    &content,
                    r.start_line,
                    r.start_char,
                );
                let e =
                    crate::editor::line_ops::offset_of_line_col(&content, r.end_line, r.end_char);
                (s.min(char_count), e.min(char_count))
            })
            .collect();
        let next = if expand {
            crate::editor::lsp_client::wider_selection(&spans, cur_s, cur_e)
        } else {
            crate::editor::lsp_client::narrower_selection(&spans, cur_s, cur_e)
        };
        let Some((ns, ne)) = next else {
            self.status_message = if expand {
                "Selection already at outermost scope".into()
            } else {
                "Selection already at innermost scope".into()
            };
            return;
        };
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(ns),
                    egui::text::CCursor::new(ne),
                )));
            state.store(ctx, editor_id);
        }
        self.current_cursor_line = crate::editor::line_ops::line_number_of_offset(&content, ne) - 1;
    }

    /// Accept the active inline suggestion: insert its text at the current
    /// cursor position in the active buffer and record acceptance telemetry.
    pub fn accept_inline_suggestion(&mut self, ctx: &egui::Context) {
        let Some(text) = self.inline_suggestions.accept() else {
            return;
        };
        let Some(id) = self.active_tab.clone() else {
            return;
        };

        // Resolve the current cursor byte offset from the editor's text state.
        let cursor_char = self.buffers.get(&id).and_then(|buf| {
            let editor_id = crate::editor::code_editor::CodeEditor::textedit_id(&id);
            let state = egui::widgets::text_edit::TextEditState::load(ctx, editor_id)?;
            let char_idx: usize = state.cursor.char_range()?.primary.index.into();
            Some(char_idx.min(buf.content().chars().count()))
        });

        if let Some(buf) = self.buffers.get_mut(&id) {
            let content = buf.content().to_string();
            let byte_pos = cursor_char
                .map(|ci| {
                    content
                        .char_indices()
                        .nth(ci)
                        .map(|(b, _)| b)
                        .unwrap_or(content.len())
                })
                .unwrap_or(content.len());
            let inserted = text.chars().count();
            let mut new_content = content;
            new_content.insert_str(byte_pos, &text);
            buf.update_content(new_content);
            self.status_message = format!("Inserted inline suggestion ({inserted} chars)");
        }
    }

    /// Floating panel that surfaces the pending inline suggestion with its
    /// source/confidence and Accept (Tab) / Dismiss (Esc) affordances.
    pub fn suggestion_panel_ui(&mut self, ctx: &egui::Context) {
        use crate::editor::inline_suggestions::SuggestionState;
        if self.inline_suggestions.state != SuggestionState::Showing {
            return;
        }
        let Some(suggestion) = self.inline_suggestions.ghost_text().cloned() else {
            return;
        };
        let palette = self.palette();
        egui::Area::new(egui::Id::new("inline_suggestion_panel"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::Vec2::new(-16.0, -16.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.horizontal(|ui| {
                        ui.colored_label(palette.accent, "\u{2728} Suggestion");
                        ui.colored_label(
                            palette.text_muted,
                            format!(
                                "{} \u{00b7} {:.0}%",
                                suggestion.source,
                                suggestion.confidence * 100.0
                            ),
                        );
                    });
                    ui.separator();
                    let preview: String = suggestion.text.chars().take(240).collect();
                    ui.colored_label(palette.text_muted, preview);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Accept (Tab)").clicked() {
                            self.accept_inline_suggestion(ctx);
                        }
                        if ui.button("Dismiss (Esc)").clicked() {
                            self.inline_suggestions.dismiss();
                        }
                        let rate = self.inline_suggestions.acceptance_rate();
                        ui.colored_label(palette.text_muted, format!("accept rate {rate:.0}%"));
                    });
                });
            });
    }

    // â”€â”€â”€ Deploy Pipeline UI Integration â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Initialize the deploy pipeline from workspace configuration.
    pub fn init_deploy_pipeline(&mut self) {
        if self.deploy_pipeline.is_none() {
            self.deploy_pipeline = Some(
                crate::editor::deploy_pipeline::PipelineManager::from_workspace(
                    &self.workspace_root,
                ),
            );
        }
    }

    /// Trigger a full deploy run (build â†’ test â†’ package â†’ deploy).
    pub fn trigger_deploy(&mut self) {
        self.init_deploy_pipeline();
        if let Some(ref mut pipeline) = self.deploy_pipeline {
            pipeline.trigger_run();
            self.status_message = "Deploy pipeline started.".into();
            self.toasts.push(crate::editor::toast::Toast::info(
                "\u{25b2} Deploy pipeline running",
            ));
        }
    }

    /// Rollback to the previous successful deployment.
    pub fn rollback_deploy(&mut self) {
        if let Some(ref mut pipeline) = self.deploy_pipeline {
            match pipeline.rollback() {
                Ok(()) => {
                    self.status_message = "Rolled back to previous deployment.".into();
                    self.toasts
                        .push(crate::editor::toast::Toast::success("Rollback successful"));
                }
                Err(e) => {
                    self.toasts.push(crate::editor::toast::Toast::error(format!(
                        "Rollback failed: {}",
                        e
                    )));
                }
            }
        }
    }

    /// Per-frame blame annotation update. Checks whether the caret moved to a
    /// new line; if so, looks up the blame cache or spawns a background
    /// `git blame` for the active file. Drains completed background results
    /// into the cache and formats the annotation for the status bar.
    pub fn update_blame(&mut self) {
        // Drain any completed background blame computation.
        if let Some(rx) = self.blame_rx.take() {
            match rx.try_recv() {
                Ok((path, lines)) => {
                    self.blame_cache.insert(path, lines);
                }
                Err(_) => {
                    // Not ready yet; put it back.
                    self.blame_rx = Some(rx);
                }
            }
        }

        // Determine active file + caret line.
        let Some(path) = self
            .active_tab
            .as_ref()
            .and_then(|id| self.tab_path(id).cloned())
        else {
            self.blame_annotation = None;
            return;
        };
        let line = self.current_cursor_line;

        // If the cache has this file, format immediately.
        if let Some(blames) = self.blame_cache.get(&path) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            self.blame_annotation = crate::editor::blame::blame_for_line(blames, line)
                .and_then(|b| crate::editor::blame::format_blame_annotation(b, now));
            return;
        }

        // Not cached: spawn a background blame if we haven't already for this file.
        let already_requested = self
            .blame_requested_for
            .as_ref()
            .is_some_and(|(p, _)| p == &path);
        if !already_requested {
            let ws = self.workspace_root.clone();
            let p = path.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            self.blame_rx = Some(rx);
            self.blame_requested_for = Some((path.clone(), line));
            std::thread::spawn(move || {
                let lines = crate::editor::git_ui::GitState::blame_file(&ws, &p);
                let _ = tx.send((p, lines));
            });
        }
        // Annotation stays whatever it was until the result arrives.
    }

    /// Open the git branch switcher overlay: populate the branch list from
    /// `GitState::branches()` and show the filter UI.
    pub fn open_branch_switcher(&mut self) {
        self.branch_list = crate::editor::git_ui::GitState::branches(&self.workspace_root);
        self.branch_filter.clear();
        self.branch_selected = 0;
        self.branch_just_opened = true;
        self.branch_creating = false;
        self.branch_new_name.clear();
        self.branch_switcher_open = true;
    }

    /// Execute the branch checkout for the currently-selected branch.
    pub fn do_checkout_branch(&mut self) {
        let visible = self.visible_branches();
        let Some(name) = visible.get(self.branch_selected).cloned() else {
            return;
        };
        match self.git_state.checkout_branch(&self.workspace_root, &name) {
            Ok(()) => {
                self.status_message = format!("Switched to branch '{}'", name);
                self.toasts
                    .push(crate::editor::toast::Toast::success(format!(
                        "Checked out {}",
                        name
                    )));
            }
            Err(e) => {
                self.toasts.push(crate::editor::toast::Toast::error(e));
            }
        }
        self.branch_switcher_open = false;
    }

    /// Create and check out a new branch from the current HEAD.
    pub fn do_create_branch(&mut self) {
        let name = self.branch_new_name.trim().to_string();
        if name.is_empty() {
            self.toasts.push(crate::editor::toast::Toast::warn(
                "Branch name cannot be empty",
            ));
            return;
        }
        match self.git_state.create_branch(&self.workspace_root, &name) {
            Ok(()) => {
                self.status_message = format!("Created and switched to '{}'", name);
                self.toasts
                    .push(crate::editor::toast::Toast::success(format!(
                        "Created branch {}",
                        name
                    )));
            }
            Err(e) => {
                self.toasts.push(crate::editor::toast::Toast::error(e));
            }
        }
        self.branch_switcher_open = false;
    }

    /// Filter the branch list by the current query (case-insensitive substring).
    pub fn visible_branches(&self) -> Vec<String> {
        let q = self.branch_filter.to_lowercase();
        if q.is_empty() {
            return self.branch_list.clone();
        }
        self.branch_list
            .iter()
            .filter(|b| b.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }
}
