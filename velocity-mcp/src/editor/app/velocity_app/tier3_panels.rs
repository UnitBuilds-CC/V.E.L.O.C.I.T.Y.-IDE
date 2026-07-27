//! Tier-3 subsystem panels: real UI surfaces for the extension registry,
//! live orchestration activity feed, speculative pre-computation cache,
//! auto test-coverage analyzer, deploy pipeline, and voice-to-task input.
//!
//! Each panel reads and mutates the corresponding subsystem state that lives on
//! [`VelocityApp`], turning previously headless engines into usable tools.

use eframe::egui;
use egui::RichText;

use crate::editor::deploy_pipeline::{PipelineStage, StageStatus};
use crate::editor::extensions::ExtensionState;
use super::struct_def::VelocityApp;

/// A deferred mutation captured while rendering the extensions list (avoids
/// borrowing `self` mutably during immutable iteration).
enum ExtAction {
    Activate(String),
    Disable(String),
}

impl VelocityApp {
    // ── Section header shared by the Tier-3 panels ─────────────────────────
    fn tier3_header(ui: &mut egui::Ui, title: &str, subtitle: &str, accent: egui::Color32, muted: egui::Color32) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new(title).strong().color(accent));
            ui.label(RichText::new(subtitle).small().color(muted));
        });
        ui.separator();
        ui.add_space(4.0);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Extensions — registry manager
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_extensions_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let active = self.extension_registry.active_count();
        let total = self.extension_registry.extensions.len();
        Self::tier3_header(
            ui,
            "Extensions",
            &format!("{active} active · {total} installed"),
            palette.accent,
            palette.text_muted,
        );

        let mut rescan = false;
        let mut pending: Option<ExtAction> = None;

        ui.horizontal(|ui| {
            if ui.button(RichText::new("⟳ Rescan").size(10.0)).clicked() {
                rescan = true;
            }
            ui.label(
                RichText::new(".velocity/extensions/")
                    .monospace()
                    .size(9.0)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("extensions_scroll")
            .show(ui, |ui| {
                if self.extension_registry.extensions.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("◇").size(26.0).color(palette.text_muted));
                        ui.label(
                            RichText::new("No extensions installed. Drop a manifest folder into .velocity/extensions/ and Rescan.")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                    });
                    return;
                }

                for ext in &self.extension_registry.extensions {
                    let (badge, badge_color) = match ext.state {
                        ExtensionState::Active => ("● active", palette.success),
                        ExtensionState::Installed => ("○ installed", palette.text_muted),
                        ExtensionState::Disabled => ("○ disabled", palette.warning),
                        ExtensionState::Error => ("✖ error", palette.error),
                    };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(6.0)
                        .inner_margin(10.0)
                        .stroke(egui::Stroke::new(0.5, palette.border))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&ext.manifest.name).strong().size(12.0).color(palette.text));
                                ui.label(RichText::new(format!("v{}", ext.manifest.version)).size(9.0).color(palette.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(badge).size(9.0).color(badge_color));
                                });
                            });
                            if let Some(desc) = &ext.manifest.description {
                                ui.label(RichText::new(desc).size(9.0).color(palette.text_muted));
                            }
                            let cmds = ext.manifest.contributes.commands.len();
                            let kbs = ext.manifest.contributes.keybindings.len();
                            ui.label(
                                RichText::new(format!("{cmds} command(s) · {kbs} keybinding(s)"))
                                    .size(8.0)
                                    .color(palette.text_muted),
                            );
                            ui.horizontal(|ui| {
                                if ext.state != ExtensionState::Active
                                    && ui.small_button(RichText::new("Activate").size(9.0)).clicked()
                                {
                                    pending = Some(ExtAction::Activate(ext.manifest.id.clone()));
                                }
                                if ext.state == ExtensionState::Active
                                    && ui.small_button(RichText::new("Disable").size(9.0)).clicked()
                                {
                                    pending = Some(ExtAction::Disable(ext.manifest.id.clone()));
                                }
                            });
                            if let Some(err) = &ext.error {
                                ui.label(RichText::new(err).size(8.0).color(palette.error));
                            }
                        });
                    ui.add_space(4.0);
                }
            });

        if rescan {
            let ws = self.workspace_root.clone();
            self.extension_registry.scan(&ws);
            self.toasts.push(crate::editor::toast::Toast::info("Extensions rescanned"));
        }
        match pending {
            Some(ExtAction::Activate(id)) => {
                if let Err(e) = self.extension_registry.activate(&id) {
                    self.toasts.push(crate::editor::toast::Toast::error(e));
                }
            }
            Some(ExtAction::Disable(id)) => self.extension_registry.disable(&id),
            None => {}
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Activity — live orchestration feed + pre-computation cache
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_activity_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let lo = &self.live_orchestration;
        Self::tier3_header(
            ui,
            "Live Activity",
            &format!(
                "up {} · {:.1} tasks/min",
                lo.session_uptime(),
                lo.throughput()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Session stat strip.
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("✔ {}", lo.total_tasks_completed)).size(10.0).color(palette.success));
            ui.label(RichText::new(format!("✖ {}", lo.total_tasks_failed)).size(10.0).color(palette.error));
            ui.label(RichText::new(format!("⋯ {} active", lo.worker_progress.len())).size(10.0).color(palette.warning));
        });
        ui.add_space(4.0);

        // Active worker progress bars.
        if !lo.worker_progress.is_empty() {
            ui.label(RichText::new("WORKERS").small().strong().color(palette.accent));
            for wp in &lo.worker_progress {
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(5.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("#{}", wp.task_id)).size(9.0).color(palette.text_muted));
                            ui.label(RichText::new(&wp.title).size(10.0).strong().color(palette.text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(wp.elapsed_label()).size(9.0).color(palette.text_muted));
                            });
                        });
                        ui.add(
                            egui::ProgressBar::new(wp.progress_fraction())
                                .desired_height(6.0)
                                .fill(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!("{} · {} file(s) changed", wp.status_text, wp.files_changed))
                                .size(8.0)
                                .color(palette.text_muted),
                        );
                    });
                ui.add_space(3.0);
            }
            ui.add_space(4.0);
        }

        // Pre-computation cache (id 0 = manual workspace warm).
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONTEXT CACHE").small().strong().color(palette.accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(RichText::new("Warm from open files").size(9.0)).clicked() {
                    self.warm_precompute_cache();
                }
            });
        });
        if let Some(result) = self.precomp_cache.peek(0) {
            ui.label(
                RichText::new(format!(
                    "{} file(s) · {} symbols · {} lines",
                    result.files.len(),
                    result.total_symbols,
                    result.total_lines
                ))
                .size(9.0)
                .color(palette.text_muted),
            );
        } else {
            ui.label(RichText::new("Cache empty — warm it to pre-index open files.").size(9.0).color(palette.text_muted));
        }
        ui.add_space(4.0);

        // Activity feed.
        ui.label(RichText::new("FEED").small().strong().color(palette.accent));
        egui::ScrollArea::vertical()
            .id_salt("activity_feed_scroll")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let feed = self.live_orchestration.filtered_feed();
                if feed.is_empty() {
                    ui.label(RichText::new("No activity yet. Events stream in as workers run.").size(9.0).color(palette.text_muted));
                }
                for ev in feed {
                    let color = match ev.kind {
                        crate::editor::live_orchestration::ActivityEventKind::WorkerCompleted => palette.success,
                        crate::editor::live_orchestration::ActivityEventKind::WorkerFailed => palette.error,
                        crate::editor::live_orchestration::ActivityEventKind::WorkerBlocked
                        | crate::editor::live_orchestration::ActivityEventKind::InterventionQueued => palette.warning,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ev.kind.icon()).size(10.0).color(color));
                        ui.label(RichText::new(ev.kind.label()).size(8.0).color(color));
                        ui.label(RichText::new(&ev.message).size(9.0).color(palette.text));
                    });
                }
            });
    }

    /// Pre-index the currently open editor files into the speculative cache
    /// under the manual slot (task id 0) and report a summary.
    pub fn warm_precompute_cache(&mut self) {
        let files: Vec<std::path::PathBuf> =
            self.tabs.iter().filter_map(|t| t.editor_path().cloned()).collect();
        if files.is_empty() {
            self.toasts.push(crate::editor::toast::Toast::info("No open files to pre-index"));
            return;
        }
        let result = crate::editor::speculative_precomp::precompute_files(&self.workspace_root, &files);
        let summary = format!(
            "Pre-indexed {} file(s), {} symbols",
            result.files.len(),
            result.total_symbols
        );
        self.precomp_cache.store(0, result);
        self.toasts.push(crate::editor::toast::Toast::success(summary));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Coverage — auto test-coverage analyzer
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_coverage_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Test Coverage",
            &self.test_generator.coverage_summary(),
            palette.accent,
            palette.text_muted,
        );

        let mut analyze = false;
        let mut analyze_lsp = false;
        let mut generate = false;
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Analyze workspace").size(10.0)).clicked() {
                analyze = true;
            }
            if ui
                .button(RichText::new("Analyze file (LSP)").size(10.0))
                .on_hover_text("Discover testable functions in the active file via the language server's documentSymbol outline")
                .clicked()
            {
                analyze_lsp = true;
            }
            let has_gaps = !self.test_generator.analysis.untested_functions.is_empty();
            if ui
                .add_enabled(has_gaps, egui::Button::new(RichText::new("Generate skeletons").size(10.0)))
                .clicked()
            {
                generate = true;
            }
            ui.checkbox(&mut self.test_generator.config.public_only, "Public only");
        });
        ui.add_space(4.0);

        let analysis = &self.test_generator.analysis;
        ui.add(
            egui::ProgressBar::new(analysis.coverage_percent / 100.0)
                .desired_height(8.0)
                .fill(palette.success)
                .text(format!("{:.1}% covered", analysis.coverage_percent)),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("coverage_scroll")
            .show(ui, |ui| {
                if !analysis.untested_functions.is_empty() {
                    ui.label(RichText::new("UNTESTED FUNCTIONS").small().strong().color(palette.accent));
                    for func in analysis.untested_functions.iter().take(200) {
                        let file = func
                            .file
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&func.name).monospace().size(9.0).color(palette.text));
                            ui.label(RichText::new(format!("{file}:{}", func.line)).size(8.0).color(palette.text_muted));
                        });
                    }
                    ui.add_space(6.0);
                }

                if !self.test_generator.generated_tests.is_empty() {
                    ui.label(RichText::new("GENERATED SKELETONS").small().strong().color(palette.accent));
                    for gen in &self.test_generator.generated_tests {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(5.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&gen.test_name).monospace().size(9.0).strong().color(palette.accent));
                                ui.label(RichText::new(&gen.test_body).monospace().size(8.0).color(palette.text_muted));
                            });
                        ui.add_space(3.0);
                    }
                }
            });

        if analyze {
            self.run_coverage_analysis();
        }
        if analyze_lsp {
            self.run_lsp_coverage_analysis();
        }
        if generate {
            let n = self.test_generator.generate_tests().len();
            self.toasts.push(crate::editor::toast::Toast::success(format!("Generated {n} test skeleton(s)")));
        }
    }

    /// Analyze the workspace for test-coverage gaps.
    pub fn run_coverage_analysis(&mut self) {
        let ws = self.workspace_root.clone();
        self.test_generator.analyze_coverage(&ws);
        let summary = self.test_generator.coverage_summary();
        self.toasts.push(crate::editor::toast::Toast::info(summary));
    }

    /// Analyze the active file for test-coverage gaps using the language
    /// server's `documentSymbol` outline (T3c). Degrades gracefully when no
    /// file is open or no language server is available.
    pub fn run_lsp_coverage_analysis(&mut self) {
        let Some((path, ext, content)) = self.active_lsp_target() else {
            self.toasts
                .push(crate::editor::toast::Toast::info("No file open for LSP coverage analysis"));
            return;
        };
        let symbols = match self.lsp_manager.as_mut() {
            Some(lsp) => lsp.document_symbols(&ext, &path, &content),
            None => Vec::new(),
        };
        if symbols.is_empty() {
            self.toasts.push(crate::editor::toast::Toast::info(
                "No symbols from language server (server absent or timed out)",
            ));
            return;
        }
        self.test_generator.ingest_lsp_symbol_list(&path, &symbols);
        let summary = self.test_generator.coverage_summary();
        self.toasts.push(crate::editor::toast::Toast::success(format!(
            "LSP coverage: {summary}"
        )));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pipeline — build/test/deploy manager
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_pipeline_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        self.init_deploy_pipeline();

        let (status, target, deployments): (String, String, usize) = self
            .deploy_pipeline
            .as_ref()
            .map(|p| (p.status_label().to_string(), p.config.deploy_target.clone(), p.deployments.len()))
            .unwrap_or_default();

        Self::tier3_header(
            ui,
            "Deploy Pipeline",
            &format!("{status} · target: {target}"),
            palette.accent,
            palette.text_muted,
        );

        let mut run = false;
        let mut deploy = false;
        let mut rollback = false;
        ui.horizontal(|ui| {
            if ui.button(RichText::new("▶ Run build+test").size(10.0)).clicked() {
                run = true;
            }
            if ui.button(RichText::new("▲ Deploy").size(10.0)).clicked() {
                deploy = true;
            }
            if ui
                .add_enabled(deployments >= 2, egui::Button::new(RichText::new("⟲ Rollback").size(10.0)))
                .clicked()
            {
                rollback = true;
            }
        });
        ui.add_space(6.0);

        if let Some(pipeline) = &self.deploy_pipeline {
            for stage in PipelineStage::all() {
                if let Some(sr) = pipeline.stages.iter().find(|s| s.stage == *stage) {
                    let (icon, color) = match &sr.status {
                        StageStatus::Passed => ("✔", palette.success),
                        StageStatus::Failed(_) => ("✖", palette.error),
                        StageStatus::Running => ("⋯", palette.warning),
                        StageStatus::Skipped => ("↷", palette.text_muted),
                        StageStatus::Pending => ("○", palette.text_muted),
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(11.0).color(color));
                        ui.label(RichText::new(stage.label()).size(10.0).strong().color(palette.text));
                        if let Some(ms) = sr.duration_ms {
                            ui.label(RichText::new(format!("{ms} ms")).size(8.0).color(palette.text_muted));
                        }
                    });
                }
            }
            ui.add_space(6.0);

            if !pipeline.deployments.is_empty() {
                ui.label(RichText::new("DEPLOYMENTS").small().strong().color(palette.accent));
                for dep in pipeline.deployments.iter().rev().take(10) {
                    let color = match dep.status {
                        StageStatus::Passed => palette.success,
                        StageStatus::Failed(_) => palette.error,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("#{}", dep.id)).size(8.0).color(palette.text_muted));
                        ui.label(RichText::new(&dep.version).monospace().size(9.0).color(palette.text));
                        ui.label(RichText::new(&dep.target).size(8.0).color(color));
                    });
                }
            }
        }

        if run {
            self.trigger_deploy();
        }
        if deploy {
            if let Some(pipeline) = &mut self.deploy_pipeline {
                match pipeline.deploy() {
                    Ok(()) => self.toasts.push(crate::editor::toast::Toast::success("Deploy stage complete")),
                    Err(e) => self.toasts.push(crate::editor::toast::Toast::error(e)),
                }
            }
        }
        if rollback {
            self.rollback_deploy();
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Voice — voice-to-task input
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_voice_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let listening = self.voice_input.listening;
        Self::tier3_header(
            ui,
            "Voice Commands",
            &format!("{:.0}% recognized · {} total", self.voice_input.accuracy(), self.voice_input.total_commands),
            palette.accent,
            palette.text_muted,
        );

        ui.horizontal(|ui| {
            let (label, color) = if listening {
                ("● Listening", palette.error)
            } else {
                ("○ Start listening", palette.text_muted)
            };
            if ui.button(RichText::new(label).size(10.0).color(color)).clicked() {
                self.voice_input.toggle_listening();
            }
        });
        ui.add_space(6.0);

        // Manual transcription entry (reuses last_transcription as scratch input).
        let mut parse = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.voice_input.last_transcription)
                    .hint_text("Type a phrase, e.g. 'run tests'…")
                    .desired_width(ui.available_width() - 70.0),
            );
            if ui.button(RichText::new("Parse").size(10.0)).clicked() {
                parse = true;
            }
        });
        ui.add_space(6.0);

        if let Some(cmd) = &self.voice_input.last_command {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(5.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Intent:").size(9.0).color(palette.text_muted));
                        ui.label(RichText::new(cmd.intent.label()).size(10.0).strong().color(palette.accent));
                        ui.label(RichText::new(format!("({:.0}%)", cmd.confidence * 100.0)).size(8.0).color(palette.text_muted));
                    });
                    if let Some(target) = cmd.parameters.get("target") {
                        ui.label(RichText::new(format!("Target: {target}")).size(9.0).color(palette.text));
                    }
                });
            ui.add_space(6.0);
        }

        ui.label(RichText::new("HISTORY").small().strong().color(palette.accent));
        egui::ScrollArea::vertical()
            .id_salt("voice_history_scroll")
            .show(ui, |ui| {
                if self.voice_input.command_history.is_empty() {
                    ui.label(RichText::new("No commands parsed yet.").size(9.0).color(palette.text_muted));
                }
                for cmd in self.voice_input.command_history.iter().rev() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(cmd.intent.label()).size(8.0).color(palette.accent));
                        ui.label(RichText::new(&cmd.raw_text).size(9.0).color(palette.text_muted));
                    });
                }
            });

        if parse {
            let text = self.voice_input.last_transcription.clone();
            if !text.trim().is_empty() {
                let intent = self.voice_input.process_transcription(&text).intent.label().to_string();
                self.toasts.push(crate::editor::toast::Toast::info(format!("Parsed intent: {intent}")));
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Knowledge — unified RAG store (ingest + search)
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_knowledge_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let sources = self.knowledge_base.sources();
        Self::tier3_header(
            ui,
            "Knowledge",
            &format!(
                "{} source(s) · {} chunk(s)",
                sources.len(),
                self.knowledge_base.chunk_count()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Ingestion: a path field (file or folder) plus whole-workspace index.
        let mut ingest_path = false;
        let mut ingest_workspace = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.knowledge_ingest_input)
                    .hint_text("path to a file or folder…")
                    .desired_width(ui.available_width() - 190.0),
            );
            if ui.button(RichText::new("Ingest").size(10.0)).clicked() {
                ingest_path = true;
            }
            if ui.button(RichText::new("Index workspace").size(10.0)).clicked() {
                ingest_workspace = true;
            }
        });
        ui.add_space(6.0);

        // Search box.
        let mut do_search = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.knowledge_query)
                    .hint_text("search knowledge…")
                    .desired_width(ui.available_width() - 70.0),
            );
            if ui.button(RichText::new("Search").size(10.0)).clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                do_search = true;
            }
        });
        ui.add_space(6.0);

        // Ranked results.
        egui::ScrollArea::vertical()
            .id_salt("knowledge_results_scroll")
            .max_height(220.0)
            .show(ui, |ui| {
                if self.knowledge_results.is_empty() {
                    ui.label(
                        RichText::new("No results. Ingest content and run a search.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for hit in &self.knowledge_results {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(5.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{}#{}", hit.source, hit.ordinal))
                                        .size(9.0)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("{:.3}", hit.score))
                                                .size(8.0)
                                                .color(palette.text_muted),
                                        );
                                    },
                                );
                            });
                            ui.label(RichText::new(&hit.snippet).size(9.0).color(palette.text));
                        });
                    ui.add_space(4.0);
                }
            });

        ui.add_space(8.0);
        let mut clear_all = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("SOURCES").small().strong().color(palette.accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.knowledge_base.is_empty()
                    && ui.small_button(RichText::new("Clear all").size(8.0)).clicked()
                {
                    clear_all = true;
                }
            });
        });
        let mut remove: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("knowledge_sources_scroll")
            .max_height(160.0)
            .show(ui, |ui| {
                if sources.is_empty() {
                    ui.label(
                        RichText::new("No sources ingested yet.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for (source, count) in &sources {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(source).size(9.0).color(palette.text));
                        ui.label(
                            RichText::new(format!("({count})"))
                                .size(8.0)
                                .color(palette.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                remove = Some(source.clone());
                            }
                        });
                    });
                }
            });

        // Deferred mutations (avoid borrowing self during rendering).
        if ingest_path {
            let raw = self.knowledge_ingest_input.trim().to_string();
            if !raw.is_empty() {
                let ws = self.workspace_root.clone();
                let candidate = std::path::PathBuf::from(&raw);
                let path = if candidate.is_absolute() {
                    candidate
                } else {
                    ws.join(&candidate)
                };
                if path.is_dir() {
                    let (files, chunks) = self.knowledge_base.ingest_dir(&ws, &path);
                    let _ = self.knowledge_base.save(&ws);
                    self.toasts.push(crate::editor::toast::Toast::info(format!(
                        "Ingested {files} file(s), {chunks} chunk(s)"
                    )));
                } else {
                    match self.knowledge_base.ingest_path(&ws, &path) {
                        Ok(added) => {
                            let _ = self.knowledge_base.save(&ws);
                            self.toasts.push(crate::editor::toast::Toast::info(format!(
                                "Ingested {added} chunk(s)"
                            )));
                        }
                        Err(e) => self.toasts.push(crate::editor::toast::Toast::error(e)),
                    }
                }
            }
        }
        if ingest_workspace {
            let ws = self.workspace_root.clone();
            let (files, chunks) = self.knowledge_base.ingest_dir(&ws, &ws);
            let _ = self.knowledge_base.save(&ws);
            self.toasts.push(crate::editor::toast::Toast::info(format!(
                "Indexed workspace: {files} file(s), {chunks} chunk(s)"
            )));
        }
        if do_search {
            let q = self.knowledge_query.clone();
            self.knowledge_results = self.knowledge_base.search(&q, 20);
        }
        if clear_all {
            self.knowledge_base.clear();
            self.knowledge_results.clear();
            let ws = self.workspace_root.clone();
            let _ = self.knowledge_base.save(&ws);
            self.toasts
                .push(crate::editor::toast::Toast::info("Knowledge base cleared"));
        }
        if let Some(src) = remove {
            if self.knowledge_base.remove_source(&src) {
                let ws = self.workspace_root.clone();
                let _ = self.knowledge_base.save(&ws);
                self.toasts
                    .push(crate::editor::toast::Toast::info(format!("Removed {src}")));
            }
        }
    }

    pub fn render_triggers_panel(&mut self, ui: &mut egui::Ui) {
        use crate::editor::triggers::{now_secs, parse_schedule, Trigger, TriggerAction, TriggerKind};
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Triggers",
            &format!("{} trigger(s) · headless via --daemon", self.triggers.len()),
            palette.accent,
            palette.text_muted,
        );

        // Add a schedule trigger: name · spec · prompt.
        let mut add = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.trigger_name_input)
                    .hint_text("name…")
                    .desired_width(120.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.trigger_interval_input)
                    .hint_text("5m · 1h · daily@09:00")
                    .desired_width(140.0),
            );
            if ui.button(RichText::new("Add").size(10.0)).clicked() {
                add = true;
            }
        });
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.trigger_prompt_input)
                .hint_text("agent prompt to run when this schedule fires…")
                .desired_rows(2)
                .desired_width(ui.available_width()),
        );
        if self.trigger_interval_input.trim().is_empty()
            || parse_schedule(self.trigger_interval_input.trim()).is_some()
        {
            // valid or empty — no warning
        } else {
            ui.label(
                RichText::new("unrecognized schedule spec")
                    .size(8.0)
                    .color(palette.error),
            );
        }
        ui.add_space(8.0);

        // Trigger list.
        let now = now_secs();
        let mut toggle: Option<String> = None;
        let mut remove: Option<String> = None;
        let mut run_now: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("triggers_list_scroll")
            .max_height(320.0)
            .show(ui, |ui| {
                if self.triggers.is_empty() {
                    ui.label(
                        RichText::new("No triggers yet. Add a schedule above.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for t in &self.triggers.triggers {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(5.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dot = if t.enabled { "●" } else { "○" };
                                ui.label(
                                    RichText::new(dot)
                                        .size(10.0)
                                        .color(if t.enabled { palette.success } else { palette.text_muted }),
                                );
                                ui.label(RichText::new(&t.name).size(10.0).strong().color(palette.text));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                            remove = Some(t.id.clone());
                                        }
                                        let label = if t.enabled { "Disable" } else { "Enable" };
                                        if ui.small_button(RichText::new(label).size(8.0)).clicked() {
                                            toggle = Some(t.id.clone());
                                        }
                                        if ui.small_button(RichText::new("Run now").size(8.0)).clicked() {
                                            run_now = Some(t.id.clone());
                                        }
                                    },
                                );
                            });
                            ui.label(
                                RichText::new(trigger_kind_label(&t.kind))
                                    .size(8.5)
                                    .color(palette.accent),
                            );
                            ui.label(
                                RichText::new(trigger_action_label(&t.action))
                                    .size(8.5)
                                    .color(palette.text_muted),
                            );
                            let due = match t.seconds_until_due(now) {
                                Some(0) => "due now".to_string(),
                                Some(secs) => format!("next in {}", human_secs(secs)),
                                None => "external / manual".to_string(),
                            };
                            ui.label(RichText::new(due).size(8.0).color(palette.text_muted));
                        });
                    ui.add_space(4.0);
                }
            });

        // Deferred mutations (avoid borrowing self during rendering).
        if add {
            let name = self.trigger_name_input.trim().to_string();
            let spec = self.trigger_interval_input.trim().to_string();
            let prompt = self.trigger_prompt_input.trim().to_string();
            if name.is_empty() || spec.is_empty() || prompt.is_empty() {
                self.toasts.push(crate::editor::toast::Toast::error(
                    "Trigger needs a name, schedule spec, and prompt",
                ));
            } else if parse_schedule(&spec).is_none() {
                self.toasts.push(crate::editor::toast::Toast::error(format!(
                    "Invalid schedule spec: {spec}"
                )));
            } else {
                let id = format!("trg-{}", now_secs());
                self.triggers.add(Trigger::new(
                    id,
                    name.clone(),
                    TriggerKind::Schedule { interval: spec },
                    TriggerAction::AgentPrompt { prompt },
                ));
                let ws = self.workspace_root.clone();
                let _ = self.triggers.save(&ws);
                self.trigger_name_input.clear();
                self.trigger_interval_input.clear();
                self.trigger_prompt_input.clear();
                self.toasts
                    .push(crate::editor::toast::Toast::info(format!("Added trigger '{name}'")));
            }
        }
        if let Some(id) = toggle {
            self.triggers.toggle(&id);
            let ws = self.workspace_root.clone();
            let _ = self.triggers.save(&ws);
        }
        if let Some(id) = remove {
            if self.triggers.remove(&id) {
                let ws = self.workspace_root.clone();
                let _ = self.triggers.save(&ws);
                self.toasts
                    .push(crate::editor::toast::Toast::info("Trigger removed"));
            }
        }
        if let Some(id) = run_now {
            let action = self.triggers.get(&id).map(|t| t.action.clone());
            match action {
                Some(TriggerAction::AgentPrompt { prompt }) => {
                    let _ = self
                        .agent_tx
                        .send(crate::agent::UiToAgentMessage::UserPrompt(prompt));
                    self.triggers.mark_run(&id, now_secs());
                    let ws = self.workspace_root.clone();
                    let _ = self.triggers.save(&ws);
                    self.toasts
                        .push(crate::editor::toast::Toast::info("Trigger dispatched to agent"));
                }
                Some(TriggerAction::RunWorkflow { workflow_id }) => {
                    if let Some(wf) = self.workflows.get(&workflow_id).cloned() {
                        let ws = self.workspace_root.clone();
                        let run = wf.execute(&ws);
                        self.triggers.mark_run(&id, now_secs());
                        let _ = self.triggers.save(&ws);
                        self.toasts.push(crate::editor::toast::Toast::info(format!(
                            "Workflow '{}' → {}",
                            wf.name,
                            run.status.label()
                        )));
                    } else {
                        self.toasts.push(crate::editor::toast::Toast::error(format!(
                            "Unknown workflow '{workflow_id}'"
                        )));
                    }
                }
                None => {}
            }
        }
    }

    pub fn render_workflows_panel(&mut self, ui: &mut egui::Ui) {
        use crate::editor::workflow::{RunStatus, StepOutcome, Workflow, WorkflowStep};
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Workflows",
            &format!("{} workflow(s) · list composer", self.workflows.len()),
            palette.accent,
            palette.text_muted,
        );

        // Create a new workflow.
        let mut create = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.workflow_name_input)
                    .hint_text("new workflow name…")
                    .desired_width(ui.available_width() - 80.0),
            );
            if ui.button(RichText::new("Create").size(10.0)).clicked() {
                create = true;
            }
        });
        ui.add_space(6.0);

        // Workflow list.
        let mut select: Option<String> = None;
        let mut run: Option<String> = None;
        let mut remove: Option<String> = None;
        let selected = self.workflow_selected.clone();
        egui::ScrollArea::vertical()
            .id_salt("workflow_list_scroll")
            .max_height(130.0)
            .show(ui, |ui| {
                if self.workflows.is_empty() {
                    ui.label(
                        RichText::new("No workflows yet. Create one above.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for wf in &self.workflows.workflows {
                    let is_sel = selected.as_deref() == Some(wf.id.as_str());
                    egui::Frame::new()
                        .fill(if is_sel { palette.bg_tertiary } else { palette.bg_secondary })
                        .corner_radius(5.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&wf.name).size(10.0).strong().color(palette.text));
                                ui.label(
                                    RichText::new(format!("({} step(s))", wf.steps.len()))
                                        .size(8.0)
                                        .color(palette.text_muted),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                        remove = Some(wf.id.clone());
                                    }
                                    if ui.small_button(RichText::new("Run").size(8.0)).clicked() {
                                        run = Some(wf.id.clone());
                                    }
                                    if ui.small_button(RichText::new("Edit").size(8.0)).clicked() {
                                        select = Some(wf.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(3.0);
                }
            });
        ui.add_space(6.0);

        // Step editor for the selected workflow (clone steps to avoid holding a
        // borrow of self.workflows while rendering the input rows).
        let mut add_tool = false;
        let mut add_agent = false;
        let mut move_up: Option<usize> = None;
        let mut move_down: Option<usize> = None;
        let mut remove_step: Option<usize> = None;
        if let Some(sel_id) = self.workflow_selected.clone() {
            let snapshot = self
                .workflows
                .get(&sel_id)
                .map(|w| (w.name.clone(), w.steps.clone()));
            match snapshot {
                None => self.workflow_selected = None,
                Some((wf_name, steps)) => {
                    ui.label(
                        RichText::new(format!("STEPS · {wf_name}"))
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    let step_count = steps.len();
                    for (i, step) in steps.iter().enumerate() {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{}. {}", i + 1, step.kind_label()))
                                            .size(9.0)
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                                remove_step = Some(i);
                                            }
                                            if i + 1 < step_count
                                                && ui.small_button(RichText::new("↓").size(8.0)).clicked()
                                            {
                                                move_down = Some(i);
                                            }
                                            if i > 0
                                                && ui.small_button(RichText::new("↑").size(8.0)).clicked()
                                            {
                                                move_up = Some(i);
                                            }
                                        },
                                    );
                                });
                                let detail = step_detail(step);
                                if !detail.is_empty() {
                                    ui.label(RichText::new(detail).size(8.0).color(palette.text_muted));
                                }
                            });
                        ui.add_space(2.0);
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_step_tool_input)
                                .hint_text("tool name")
                                .desired_width(110.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_step_args_input)
                                .hint_text("{\"json\":\"args\"}")
                                .desired_width(ui.available_width() - 70.0),
                        );
                        if ui.button(RichText::new("+tool").size(9.0)).clicked() {
                            add_tool = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_step_prompt_input)
                                .hint_text("agent prompt")
                                .desired_width(ui.available_width() - 70.0),
                        );
                        if ui.button(RichText::new("+agent").size(9.0)).clicked() {
                            add_agent = true;
                        }
                    });
                }
            }
        }

        // Run log.
        if let Some(runrec) = &self.workflow_last_run {
            ui.add_space(6.0);
            let status_color = match runrec.status {
                RunStatus::Success => palette.success,
                RunStatus::Failed => palette.error,
                RunStatus::Partial => palette.warning,
            };
            ui.label(
                RichText::new(format!("LAST RUN · {}", runrec.status.label()))
                    .small()
                    .strong()
                    .color(status_color),
            );
            egui::ScrollArea::vertical()
                .id_salt("workflow_run_log")
                .max_height(140.0)
                .show(ui, |ui| {
                    for rec in &runrec.steps {
                        let color = match rec.outcome {
                            StepOutcome::Ok => palette.success,
                            StepOutcome::Failed => palette.error,
                            StepOutcome::Skipped => palette.text_muted,
                        };
                        ui.label(
                            RichText::new(format!(
                                "{}. {} [{}]",
                                rec.index + 1,
                                rec.kind,
                                rec.outcome.label()
                            ))
                            .size(9.0)
                            .color(color),
                        );
                        let snippet: String = rec.output.chars().take(120).collect();
                        if !snippet.is_empty() {
                            ui.label(RichText::new(snippet).size(8.0).color(palette.text_muted));
                        }
                    }
                });
        }

        // Deferred mutations.
        let ws = self.workspace_root.clone();
        if create {
            let name = self.workflow_name_input.trim().to_string();
            if name.is_empty() {
                self.toasts
                    .push(crate::editor::toast::Toast::error("Workflow needs a name"));
            } else {
                let id = format!("wf-{}", crate::editor::triggers::now_secs());
                self.workflows.add(Workflow::new(id.clone(), name));
                let _ = self.workflows.save(&ws);
                self.workflow_selected = Some(id);
                self.workflow_name_input.clear();
            }
        }
        if let Some(id) = select {
            self.workflow_selected = Some(id);
            self.workflow_last_run = None;
        }
        if let Some(id) = remove {
            if self.workflows.remove(&id) {
                let _ = self.workflows.save(&ws);
                if self.workflow_selected.as_deref() == Some(id.as_str()) {
                    self.workflow_selected = None;
                }
            }
        }
        if add_tool {
            if let Some(sel) = self.workflow_selected.clone() {
                let name = self.workflow_step_tool_input.trim().to_string();
                if !name.is_empty() {
                    let args_raw = self.workflow_step_args_input.trim().to_string();
                    let parsed = if args_raw.is_empty() {
                        Some(serde_json::json!({}))
                    } else {
                        match serde_json::from_str::<serde_json::Value>(&args_raw) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                self.toasts.push(crate::editor::toast::Toast::error(format!(
                                    "Invalid JSON args: {e}"
                                )));
                                None
                            }
                        }
                    };
                    if let Some(args) = parsed {
                        if let Some(wf) = self.workflows.get_mut(&sel) {
                            wf.steps.push(WorkflowStep::Tool { name, args });
                        }
                        let _ = self.workflows.save(&ws);
                        self.workflow_step_tool_input.clear();
                        self.workflow_step_args_input.clear();
                    }
                }
            }
        }
        if add_agent {
            if let Some(sel) = self.workflow_selected.clone() {
                let prompt = self.workflow_step_prompt_input.trim().to_string();
                if !prompt.is_empty() {
                    if let Some(wf) = self.workflows.get_mut(&sel) {
                        wf.steps.push(WorkflowStep::AgentTask { prompt, team: None });
                    }
                    let _ = self.workflows.save(&ws);
                    self.workflow_step_prompt_input.clear();
                }
            }
        }
        if let Some(sel) = self.workflow_selected.clone() {
            let mut mutated = false;
            if let Some(i) = remove_step {
                if let Some(wf) = self.workflows.get_mut(&sel) {
                    if i < wf.steps.len() {
                        wf.steps.remove(i);
                        mutated = true;
                    }
                }
            }
            if let Some(i) = move_up {
                if let Some(wf) = self.workflows.get_mut(&sel) {
                    if i > 0 {
                        wf.steps.swap(i, i - 1);
                        mutated = true;
                    }
                }
            }
            if let Some(i) = move_down {
                if let Some(wf) = self.workflows.get_mut(&sel) {
                    if i + 1 < wf.steps.len() {
                        wf.steps.swap(i, i + 1);
                        mutated = true;
                    }
                }
            }
            if mutated {
                let _ = self.workflows.save(&ws);
            }
        }
        if let Some(id) = run {
            if let Some(wf) = self.workflows.get(&id).cloned() {
                let runrec = wf.execute(&ws);
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Workflow '{}' → {}",
                    wf.name,
                    runrec.status.label()
                )));
                self.workflow_last_run = Some(runrec);
                self.workflow_selected = Some(id);
            }
        }
    }

    pub fn render_governance_panel(&mut self, ui: &mut egui::Ui) {
        use crate::editor::governance::{Decision, Rule, RuleEffect};
        let palette = self.palette();
        Self::tier3_header(
            ui,
            "Governance",
            &format!(
                "{} rule(s) · {} pending · {} secret(s) · {} connector(s)",
                self.policy.rules.len(),
                self.approvals.pending().len(),
                self.secrets.len(),
                self.connectors.len()
            ),
            palette.accent,
            palette.text_muted,
        );
        if !self.gov_status.is_empty() {
            ui.label(RichText::new(&self.gov_status).size(9.0).color(palette.text_muted));
        }

        egui::ScrollArea::vertical()
            .id_salt("governance_scroll")
            .show(ui, |ui| {
                // ── Policy ─────────────────────────────────────────────────
                ui.add_space(4.0);
                ui.label(RichText::new("POLICY").small().strong().color(palette.accent));
                let mut cycle_default = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Default when no rule matches:").size(9.0).color(palette.text_muted));
                    let (txt, col) = match self.policy.default_decision {
                        Decision::Allow => ("allow", palette.success),
                        Decision::Deny => ("deny", palette.error),
                        Decision::NeedsApproval => ("needs-approval", palette.warning),
                    };
                    if ui.small_button(RichText::new(txt).size(9.0).color(col)).clicked() {
                        cycle_default = true;
                    }
                });

                // Budget quick-set.
                let mut set_tokens: Option<Option<u64>> = None;
                let mut set_cost: Option<Option<u64>> = None;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Budget: tokens {}, cost {}¢",
                            self.policy.budget.max_tokens.map(|t| t.to_string()).unwrap_or_else(|| "∞".into()),
                            self.policy.budget.max_cost_cents.map(|c| c.to_string()).unwrap_or_else(|| "∞".into()),
                        ))
                        .size(9.0)
                        .color(palette.text_muted),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("tokens:").size(8.0).color(palette.text_muted));
                    if ui.small_button(RichText::new("∞").size(8.0)).clicked() { set_tokens = Some(None); }
                    if ui.small_button(RichText::new("50k").size(8.0)).clicked() { set_tokens = Some(Some(50_000)); }
                    if ui.small_button(RichText::new("200k").size(8.0)).clicked() { set_tokens = Some(Some(200_000)); }
                    ui.label(RichText::new("cost¢:").size(8.0).color(palette.text_muted));
                    if ui.small_button(RichText::new("∞").size(8.0)).clicked() { set_cost = Some(None); }
                    if ui.small_button(RichText::new("100").size(8.0)).clicked() { set_cost = Some(Some(100)); }
                    if ui.small_button(RichText::new("500").size(8.0)).clicked() { set_cost = Some(Some(500)); }
                });

                // Rule list.
                let mut remove_rule: Option<usize> = None;
                for (i, rule) in self.policy.rules.iter().enumerate() {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(4.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let scope = match (&rule.path_prefix, &rule.domain) {
                                    (Some(p), _) => format!(" path:{p}"),
                                    (_, Some(d)) => format!(" domain:{d}"),
                                    _ => String::new(),
                                };
                                ui.label(
                                    RichText::new(format!("{} [{}]{}", rule.tool, rule.effect.label(), scope))
                                        .size(9.0)
                                        .color(palette.text),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                        remove_rule = Some(i);
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }
                // Add-rule row.
                let mut add_rule: Option<RuleEffect> = None;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_rule_tool_input)
                            .hint_text("tool or *")
                            .desired_width(90.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_rule_path_input)
                            .hint_text("path prefix (opt)")
                            .desired_width(ui.available_width() - 150.0),
                    );
                    if ui.small_button(RichText::new("+allow").size(8.0)).clicked() { add_rule = Some(RuleEffect::Allow); }
                    if ui.small_button(RichText::new("+deny").size(8.0)).clicked() { add_rule = Some(RuleEffect::Deny); }
                    if ui.small_button(RichText::new("+approve").size(8.0)).clicked() { add_rule = Some(RuleEffect::RequireApproval); }
                });

                // ── Approvals ──────────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(RichText::new("APPROVAL QUEUE").small().strong().color(palette.accent));
                let mut approve: Option<String> = None;
                let mut deny: Option<String> = None;
                let pending: Vec<crate::editor::governance::ApprovalItem> =
                    self.approvals.pending().into_iter().cloned().collect();
                if pending.is_empty() {
                    ui.label(RichText::new("No pending approvals.").size(9.0).color(palette.text_muted));
                }
                for item in &pending {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(4.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&item.summary).size(9.0).color(palette.text));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new("deny").size(8.0).color(palette.error)).clicked() {
                                        deny = Some(item.id.clone());
                                    }
                                    if ui.small_button(RichText::new("approve").size(8.0).color(palette.success)).clicked() {
                                        approve = Some(item.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }

                // ── Secrets ────────────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(RichText::new("SECRETS").small().strong().color(palette.accent));
                let mut remove_secret: Option<String> = None;
                let handles: Vec<String> = self.secrets.handles().into_iter().map(str::to_string).collect();
                if handles.is_empty() {
                    ui.label(RichText::new("No secrets stored.").size(9.0).color(palette.text_muted));
                }
                for handle in &handles {
                    let masked = self.secrets.masked(handle).unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(handle).size(9.0).color(palette.text));
                        ui.label(RichText::new(masked).size(8.0).color(palette.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                remove_secret = Some(handle.clone());
                            }
                        });
                    });
                }
                let mut add_secret = false;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_secret_name_input)
                            .hint_text("name")
                            .desired_width(110.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_secret_value_input)
                            .hint_text("value")
                            .password(true)
                            .desired_width(ui.available_width() - 70.0),
                    );
                    if ui.small_button(RichText::new("+add").size(8.0)).clicked() { add_secret = true; }
                });

                // ── Connectors ─────────────────────────────────────────────
                ui.add_space(8.0);
                ui.label(RichText::new("CONNECTORS").small().strong().color(palette.accent));
                let mut remove_connector: Option<String> = None;
                if self.connectors.is_empty() {
                    ui.label(RichText::new("No connectors configured.").size(9.0).color(palette.text_muted));
                }
                for c in &self.connectors.connectors {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(4.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&c.name).size(9.0).strong().color(palette.text));
                                ui.label(RichText::new(&c.base_url).size(8.0).color(palette.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(RichText::new("✖").size(8.0)).clicked() {
                                        remove_connector = Some(c.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }
                let mut add_connector = false;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_connector_id_input)
                            .hint_text("id/name")
                            .desired_width(90.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_connector_url_input)
                            .hint_text("https://base.url")
                            .desired_width(ui.available_width() - 190.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.gov_connector_secret_input)
                            .hint_text("secret handle (opt)")
                            .desired_width(90.0),
                    );
                    if ui.small_button(RichText::new("+add").size(8.0)).clicked() { add_connector = true; }
                });

                // ── Deferred mutations ─────────────────────────────────────
                let ws = self.workspace_root.clone();
                if cycle_default {
                    self.policy.default_decision = match self.policy.default_decision {
                        Decision::Allow => Decision::Deny,
                        Decision::Deny => Decision::NeedsApproval,
                        Decision::NeedsApproval => Decision::Allow,
                    };
                    let _ = self.policy.save(&ws);
                }
                if let Some(t) = set_tokens {
                    self.policy.budget.max_tokens = t;
                    let _ = self.policy.save(&ws);
                }
                if let Some(c) = set_cost {
                    self.policy.budget.max_cost_cents = c;
                    let _ = self.policy.save(&ws);
                }
                if let Some(i) = remove_rule {
                    if i < self.policy.rules.len() {
                        self.policy.rules.remove(i);
                        let _ = self.policy.save(&ws);
                    }
                }
                if let Some(effect) = add_rule {
                    let tool = self.gov_rule_tool_input.trim().to_string();
                    if tool.is_empty() {
                        self.gov_status = "Rule needs a tool name (or *).".to_string();
                    } else {
                        let path = self.gov_rule_path_input.trim();
                        self.policy.rules.push(Rule {
                            tool,
                            effect,
                            path_prefix: if path.is_empty() { None } else { Some(path.to_string()) },
                            domain: None,
                        });
                        let _ = self.policy.save(&ws);
                        self.gov_rule_tool_input.clear();
                        self.gov_rule_path_input.clear();
                        self.gov_status = "Rule added.".to_string();
                    }
                }
                if let Some(id) = approve {
                    self.approvals.approve(&id);
                    let _ = self.approvals.save(&ws);
                }
                if let Some(id) = deny {
                    self.approvals.deny(&id);
                    let _ = self.approvals.save(&ws);
                }
                if add_secret {
                    let name = self.gov_secret_name_input.trim().to_string();
                    let value = self.gov_secret_value_input.clone();
                    if name.is_empty() || value.is_empty() {
                        self.gov_status = "Secret needs a name and value.".to_string();
                    } else if self.secrets.set(name, value) {
                        match self.secrets.save(&ws) {
                            Ok(()) => {
                                self.gov_secret_name_input.clear();
                                self.gov_secret_value_input.clear();
                                self.gov_status = "Secret saved (encrypted).".to_string();
                            }
                            Err(e) => self.gov_status = format!("Secret save failed: {e}"),
                        }
                    }
                }
                if let Some(name) = remove_secret {
                    if self.secrets.remove(&name) {
                        let _ = self.secrets.save(&ws);
                    }
                }
                if add_connector {
                    let id = self.gov_connector_id_input.trim().to_string();
                    let url = self.gov_connector_url_input.trim().to_string();
                    if id.is_empty() || url.is_empty() {
                        self.gov_status = "Connector needs an id and base URL.".to_string();
                    } else {
                        let secret = self.gov_connector_secret_input.trim();
                        let mut cfg = crate::connectors::ConnectorConfig::generic(id.clone(), id, url);
                        if !secret.is_empty() {
                            cfg.auth_secret = Some(secret.to_string());
                            cfg.auth = crate::connectors::AuthScheme::Bearer;
                        }
                        self.connectors.add(cfg);
                        let _ = self.connectors.save(&ws);
                        self.gov_connector_id_input.clear();
                        self.gov_connector_url_input.clear();
                        self.gov_connector_secret_input.clear();
                        self.gov_status = "Connector added.".to_string();
                    }
                }
                if let Some(id) = remove_connector {
                    if self.connectors.remove(&id) {
                        let _ = self.connectors.save(&ws);
                    }
                }
            });
    }
}

fn step_detail(step: &crate::editor::workflow::WorkflowStep) -> String {
    use crate::editor::workflow::WorkflowStep;
    match step {
        WorkflowStep::AgentTask { prompt, .. } => prompt.chars().take(80).collect(),
        WorkflowStep::Tool { args, .. } => args.to_string().chars().take(80).collect(),
        WorkflowStep::Connector { req, .. } => req.to_string().chars().take(80).collect(),
        WorkflowStep::Condition { require } => format!("require prior == {}", require.label()),
    }
}

fn trigger_kind_label(kind: &crate::editor::triggers::TriggerKind) -> String {
    use crate::editor::triggers::TriggerKind;
    match kind {
        TriggerKind::Schedule { interval } => format!("schedule · {interval}"),
        TriggerKind::FileWatch { path, glob } => format!("file-watch · {path}/{glob}"),
        TriggerKind::Webhook { .. } => "webhook".to_string(),
        TriggerKind::Manual => "manual".to_string(),
    }
}

fn trigger_action_label(action: &crate::editor::triggers::TriggerAction) -> String {
    use crate::editor::triggers::TriggerAction;
    match action {
        TriggerAction::RunWorkflow { workflow_id } => format!("→ workflow {workflow_id}"),
        TriggerAction::AgentPrompt { prompt } => {
            let p: String = prompt.chars().take(60).collect();
            format!("→ agent: {p}")
        }
    }
}

fn human_secs(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}
