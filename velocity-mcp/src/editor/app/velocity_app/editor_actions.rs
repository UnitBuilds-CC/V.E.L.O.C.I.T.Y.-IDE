use std::path::PathBuf;

use super::super::types::*;
use super::struct_def::VelocityApp;
use crate::agent::UiToAgentMessage;

impl VelocityApp {
    pub fn focus_panel(&mut self, kind: TabKind) {
        if let Some(dock) = self.dock_state.as_mut() {
            let found_tab = dock
                .iter_all_tabs()
                .find(|(_, tab)| std::mem::discriminant(&tab.kind) == std::mem::discriminant(&kind))
                .map(|(_, tab)| tab.clone());

            if let Some(tab) = found_tab {
                if let Some(tab_path) = dock.find_tab(&tab) {
                    let _ = dock.set_active_tab(tab_path);
                    self.active_tab = Some(tab.id);
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

    pub fn toggle_panel(&mut self, kind: TabKind) {
        self.focus_panel(kind);
    }

    pub fn rebuild_dock(&mut self) {
        self.dock_state = Some(self.build_workspace_dock(self.appearance.profile));
    }

    pub fn build_active(&mut self) {
        self.command_output.clear();
        self.status_message = "Running local build...".into();
        self.agent_active = true;
        let _ = self.agent_tx.send(UiToAgentMessage::RunLocalBuild);
    }

    pub fn run_active(&mut self) {
        self.command_output.clear();
        self.status_message = "Running local execute...".into();
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
    /// `memory/`) — those are at-rest envelopes, never routed to the NDA editor.
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

    // ─── IDE Feature Helpers ───────────────────────────────────────────────

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

    /// Trigger code completion at cursor position. Merges language-server (LSP)
    /// completions with local sitemap/keyword/identifier suggestions; LSP items
    /// win on label clashes. Degrades gracefully when no language server exists.
    pub fn trigger_completion(&mut self) {
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

        let prefix = content
            .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("")
            .to_string();

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
            if let Some(lsp) = self.lsp_manager.as_mut() {
                lsp_items = lsp.completion(&ext, path, line, col, &content);
            }
        }

        let merged = crate::editor::completion::merge_completion_items(lsp_items, local);
        self.completion_state.show(merged);
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
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for go-to-definition".into();
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let locations = match self.lsp_manager.as_mut() {
            Some(lsp) => lsp.definition(&ext, &path, line, col, &content),
            None => Vec::new(),
        };
        let Some(target) = locations.into_iter().next() else {
            self.status_message = "No definition found (no language server result)".into();
            return;
        };
        let target_path = target.file.clone();
        let target_line = target.line + 1; // LSP 0-based -> editor 1-based
        self.push_nav_location();
        self.open_editor(Some(target.file));
        self.pending_cursor_line = Some(target_line);
        self.status_message = format!(
            "Definition \u{2192} {}:{}",
            target_path.display(),
            target_line
        );
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
        let locations = match self.lsp_manager.as_mut() {
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

    /// Show LSP hover information for the symbol under the cursor as a toast.
    pub fn show_hover_at_cursor(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.status_message = "No file open for hover".into();
            return;
        };
        let line = self.current_cursor_line;
        let col = self.current_cursor_col;
        let hover = match self.lsp_manager.as_mut() {
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
                egui::RichText::new(format!("\u{1F41E} {}", state_label))
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
                .add_enabled(can_continue, egui::Button::new("\u{25B6} Continue"))
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.continue_execution();
                }
            }
            if ui
                .add_enabled(can_step, egui::Button::new("\u{23ED} Step Over"))
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_over();
                }
            }
            if ui
                .add_enabled(can_step, egui::Button::new("\u{2B07} Step Into"))
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_into();
                }
            }
            if ui
                .add_enabled(can_step, egui::Button::new("\u{2B06} Step Out"))
                .clicked()
            {
                if let Some(dap) = &mut self.dap_client {
                    let _ = dap.step_out();
                }
            }
            if ui
                .add_enabled(
                    can_stop,
                    egui::Button::new("\u{23F9} Stop").fill(palette.error),
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
            ui.label(
                egui::RichText::new("No active debug session. Press F5 to start debugging.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
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
    }

    /// Launch a debug session. Auto-detects the debug adapter based on project type.
    pub fn launch_debug_session(&mut self) {
        use crate::editor::debugger::{DapClient, LaunchConfig};

        // Determine the binary to debug based on workspace type
        let cargo_toml = self.workspace_root.join("Cargo.toml");
        if cargo_toml.exists() {
            // Rust project — look for the target binary
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
                    self.status_message = format!("Debug: failed to launch — {}", e);
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

    // ─── Semantic Search Integration ─────────────────────────────────────────

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
            let hits = index.search(&self.search_query, 50);
            self.search_hits = hits
                .into_iter()
                .map(|h| crate::editor::search::SearchHit {
                    path: h.path,
                    line: 1,
                    text: format!("[{:.0}%] {}", h.score * 100.0, h.preview),
                })
                .collect();
        }
    }

    // ─── Inline Suggestions LLM Wiring ───────────────────────────────────────

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
            let editor_id = egui::Id::new("code_editor");
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

    // ─── Deploy Pipeline UI Integration ──────────────────────────────────────

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

    /// Trigger a full deploy run (build → test → package → deploy).
    pub fn trigger_deploy(&mut self) {
        self.init_deploy_pipeline();
        if let Some(ref mut pipeline) = self.deploy_pipeline {
            pipeline.trigger_run();
            self.status_message = "Deploy pipeline started.".into();
            self.toasts.push(crate::editor::toast::Toast::info(
                "▲ Deploy pipeline running",
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
}
