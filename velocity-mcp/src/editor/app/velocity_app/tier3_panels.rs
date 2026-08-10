//! Tier-3 subsystem panels: real UI surfaces for the extension registry,
//! live orchestration activity feed, speculative pre-computation cache,
//! auto test-coverage analyzer, deploy pipeline, and voice-to-task input.
//!
//! Each panel reads and mutates the corresponding subsystem state that lives on
//! [`VelocityApp`], turning previously headless engines into usable tools.

use eframe::egui;
use egui::RichText;

use super::struct_def::VelocityApp;
use crate::editor::deploy_pipeline::{PipelineStage, StageStatus};
use crate::editor::extensions::ExtensionState;

/// A deferred mutation captured while rendering the extensions list (avoids
/// borrowing `self` mutably during immutable iteration).
enum ExtAction {
    Activate(String),
    Disable(String),
}

impl VelocityApp {
    // ── Section header shared by the Tier-3 panels ─────────────────────────
    pub(crate) fn tier3_header(
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        accent: egui::Color32,
        muted: egui::Color32,
    ) {
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
            self.toasts
                .push(crate::editor::toast::Toast::info("Extensions rescanned"));
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
            ui.label(
                RichText::new(format!("✔ {}", lo.total_tasks_completed))
                    .size(10.0)
                    .color(palette.success),
            );
            ui.label(
                RichText::new(format!("✖ {}", lo.total_tasks_failed))
                    .size(10.0)
                    .color(palette.error),
            );
            ui.label(
                RichText::new(format!("⋯ {} active", lo.worker_progress.len()))
                    .size(10.0)
                    .color(palette.warning),
            );
        });
        ui.add_space(4.0);

        // Active worker progress bars.
        if !lo.worker_progress.is_empty() {
            ui.label(
                RichText::new("WORKERS")
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            for wp in &lo.worker_progress {
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(5.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("#{}", wp.task_id))
                                    .size(9.0)
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(&wp.title)
                                    .size(10.0)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(wp.elapsed_label())
                                            .size(9.0)
                                            .color(palette.text_muted),
                                    );
                                },
                            );
                        });
                        ui.add(
                            egui::ProgressBar::new(wp.progress_fraction())
                                .desired_height(6.0)
                                .fill(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {} file(s) changed",
                                wp.status_text, wp.files_changed
                            ))
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
            ui.label(
                RichText::new("CONTEXT CACHE")
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(RichText::new("Warm from open files").size(9.0))
                    .clicked()
                {
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
            ui.label(
                RichText::new("Cache empty — warm it to pre-index open files.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
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
        let files: Vec<std::path::PathBuf> = self
            .tabs
            .iter()
            .filter_map(|t| t.editor_path().cloned())
            .collect();
        if files.is_empty() {
            self.toasts.push(crate::editor::toast::Toast::info(
                "No open files to pre-index",
            ));
            return;
        }
        let result =
            crate::editor::speculative_precomp::precompute_files(&self.workspace_root, &files);
        let summary = format!(
            "Pre-indexed {} file(s), {} symbols",
            result.files.len(),
            result.total_symbols
        );
        self.precomp_cache.store(0, result);
        self.toasts
            .push(crate::editor::toast::Toast::success(summary));
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
                    ui.label(
                        RichText::new("UNTESTED FUNCTIONS")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    for func in analysis.untested_functions.iter().take(200) {
                        let file = func
                            .file
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&func.name)
                                    .monospace()
                                    .size(9.0)
                                    .color(palette.text),
                            );
                            ui.label(
                                RichText::new(format!("{file}:{}", func.line))
                                    .size(8.0)
                                    .color(palette.text_muted),
                            );
                        });
                    }
                    ui.add_space(6.0);
                }

                if !self.test_generator.generated_tests.is_empty() {
                    ui.label(
                        RichText::new("GENERATED SKELETONS")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    for gen in &self.test_generator.generated_tests {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(5.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(&gen.test_name)
                                        .monospace()
                                        .size(9.0)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(&gen.test_body)
                                        .monospace()
                                        .size(8.0)
                                        .color(palette.text_muted),
                                );
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
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
                    "Generated {n} test skeleton(s)"
                )));
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
            self.toasts.push(crate::editor::toast::Toast::info(
                "No file open for LSP coverage analysis",
            ));
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
        self.toasts
            .push(crate::editor::toast::Toast::success(format!(
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
            .map(|p| {
                (
                    p.status_label().to_string(),
                    p.config.deploy_target.clone(),
                    p.deployments.len(),
                )
            })
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
            if ui
                .button(RichText::new("▶ Run build+test").size(10.0))
                .clicked()
            {
                run = true;
            }
            if ui.button(RichText::new("▲ Deploy").size(10.0)).clicked() {
                deploy = true;
            }
            if ui
                .add_enabled(
                    deployments >= 2,
                    egui::Button::new(RichText::new("⟲ Rollback").size(10.0)),
                )
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
                        ui.label(
                            RichText::new(stage.label())
                                .size(10.0)
                                .strong()
                                .color(palette.text),
                        );
                        if let Some(ms) = sr.duration_ms {
                            ui.label(
                                RichText::new(format!("{ms} ms"))
                                    .size(8.0)
                                    .color(palette.text_muted),
                            );
                        }
                    });
                }
            }
            ui.add_space(6.0);

            if !pipeline.deployments.is_empty() {
                ui.label(
                    RichText::new("DEPLOYMENTS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                for dep in pipeline.deployments.iter().rev().take(10) {
                    let color = match dep.status {
                        StageStatus::Passed => palette.success,
                        StageStatus::Failed(_) => palette.error,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("#{}", dep.id))
                                .size(8.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(&dep.version)
                                .monospace()
                                .size(9.0)
                                .color(palette.text),
                        );
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
                    Ok(()) => self.toasts.push(crate::editor::toast::Toast::success(
                        "Deploy stage complete",
                    )),
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
            &format!(
                "{:.0}% recognized · {} total",
                self.voice_input.accuracy(),
                self.voice_input.total_commands
            ),
            palette.accent,
            palette.text_muted,
        );

        ui.horizontal(|ui| {
            let (label, color) = if listening {
                ("● Listening", palette.error)
            } else {
                ("○ Start listening", palette.text_muted)
            };
            if ui
                .button(RichText::new(label).size(10.0).color(color))
                .clicked()
            {
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
                        ui.label(
                            RichText::new(cmd.intent.label())
                                .size(10.0)
                                .strong()
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!("({:.0}%)", cmd.confidence * 100.0))
                                .size(8.0)
                                .color(palette.text_muted),
                        );
                    });
                    if let Some(target) = cmd.parameters.get("target") {
                        ui.label(
                            RichText::new(format!("Target: {target}"))
                                .size(9.0)
                                .color(palette.text),
                        );
                    }
                });
            ui.add_space(6.0);
        }

        ui.label(
            RichText::new("HISTORY")
                .small()
                .strong()
                .color(palette.accent),
        );
        egui::ScrollArea::vertical()
            .id_salt("voice_history_scroll")
            .show(ui, |ui| {
                if self.voice_input.command_history.is_empty() {
                    ui.label(
                        RichText::new("No commands parsed yet.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                }
                for cmd in self.voice_input.command_history.iter().rev() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(cmd.intent.label())
                                .size(8.0)
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(&cmd.raw_text)
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                    });
                }
            });

        if parse {
            let text = self.voice_input.last_transcription.clone();
            if !text.trim().is_empty() {
                let intent = self
                    .voice_input
                    .process_transcription(&text)
                    .intent
                    .label()
                    .to_string();
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Parsed intent: {intent}"
                )));
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
            if ui
                .button(RichText::new("Index workspace").size(10.0))
                .clicked()
            {
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
            ui.label(
                RichText::new("SOURCES")
                    .small()
                    .strong()
                    .color(palette.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.knowledge_base.is_empty()
                    && ui
                        .small_button(RichText::new("Clear all").size(8.0))
                        .clicked()
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
        use crate::editor::triggers::{
            now_secs, parse_schedule, Trigger, TriggerAction, TriggerKind,
        };
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
                                ui.label(RichText::new(dot).size(10.0).color(if t.enabled {
                                    palette.success
                                } else {
                                    palette.text_muted
                                }));
                                ui.label(
                                    RichText::new(&t.name)
                                        .size(10.0)
                                        .strong()
                                        .color(palette.text),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(RichText::new("✖").size(8.0)).clicked()
                                        {
                                            remove = Some(t.id.clone());
                                        }
                                        let label = if t.enabled { "Disable" } else { "Enable" };
                                        if ui.small_button(RichText::new(label).size(8.0)).clicked()
                                        {
                                            toggle = Some(t.id.clone());
                                        }
                                        if ui
                                            .small_button(RichText::new("Run now").size(8.0))
                                            .clicked()
                                        {
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
                self.toasts.push(crate::editor::toast::Toast::info(format!(
                    "Added trigger '{name}'"
                )));
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
                    self.toasts.push(crate::editor::toast::Toast::info(
                        "Trigger dispatched to agent",
                    ));
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

    // ═══════════════════════════════════════════════════════════════════════
    // Test Generator — coverage analysis and test generation
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_test_generator_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let total = self.test_generator.analysis.total_functions;
        let tested = self.test_generator.analysis.tested_functions;
        let coverage = self.test_generator.analysis.coverage_percent;

        Self::tier3_header(
            ui,
            "Test Generator",
            &format!(
                "{}/{} functions tested · {:.1}% coverage",
                tested, total, coverage
            ),
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut analyze = false;
        let mut generate = false;
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("🔍 Analyze Coverage").size(10.0))
                .clicked()
            {
                analyze = true;
            }
            if ui
                .button(RichText::new("✨ Generate Tests").size(10.0))
                .clicked()
            {
                generate = true;
            }
            ui.label(
                RichText::new(format!(
                    "{} test(s) generated",
                    self.test_generator.generated_tests.len()
                ))
                .size(9.0)
                .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Configuration
        egui::CollapsingHeader::new(RichText::new("Configuration").size(10.0).strong())
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Max tests per run:").size(9.0));
                    ui.add(
                        egui::DragValue::new(&mut self.test_generator.config.max_tests_per_run)
                            .range(1..=100)
                            .speed(1),
                    );
                });
                ui.checkbox(
                    &mut self.test_generator.config.public_only,
                    "Public functions only",
                );
                ui.checkbox(
                    &mut self.test_generator.config.include_assertions,
                    "Include assertion placeholders",
                );
            });
        ui.add_space(6.0);

        // Untested functions list
        if !self.test_generator.analysis.untested_functions.is_empty() {
            ui.label(
                RichText::new("UNTESTED FUNCTIONS")
                    .small()
                    .strong()
                    .color(palette.warning),
            );
            egui::ScrollArea::vertical()
                .id_salt("test_gen_untested_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for func in &self.test_generator.analysis.untested_functions {
                        let vis_badge = match func.visibility {
                            crate::editor::test_generator::Visibility::Public => ("pub", palette.success),
                            crate::editor::test_generator::Visibility::Private => ("priv", palette.text_muted),
                            crate::editor::test_generator::Visibility::CrateLocal => ("crate", palette.text_muted),
                        };
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(vis_badge.0)
                                            .size(8.0)
                                            .monospace()
                                            .color(vis_badge.1),
                                    );
                                    ui.label(
                                        RichText::new(&func.name)
                                            .size(10.0)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}:{}",
                                                    func.file.display(),
                                                    func.line
                                                ))
                                                .size(8.0)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(&func.signature)
                                        .size(8.0)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(3.0);
                    }
                });
        }

        // Generated tests preview
        if !self.test_generator.generated_tests.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new("GENERATED TESTS")
                    .small()
                    .strong()
                    .color(palette.success),
            );
            egui::ScrollArea::vertical()
                .id_salt("test_gen_results_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for test in &self.test_generator.generated_tests {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&test.test_name)
                                            .size(10.0)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{:.0}% confidence",
                                                    test.confidence * 100.0
                                                ))
                                                .size(8.0)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("for {}", test.function_name))
                                        .size(8.0)
                                        .color(palette.text_muted),
                                );
                                if ui
                                    .small_button(RichText::new("View code").size(8.0))
                                    .clicked()
                                {
                                    // Could open in editor or show modal
                                }
                            });
                        ui.add_space(3.0);
                    }
                });
        }

        if analyze {
            let ws = self.workspace_root.clone();
            self.test_generator.analyze_coverage(&ws);
            self.toasts.push(crate::editor::toast::Toast::success(format!(
                "Coverage analysis complete: {:.1}%",
                self.test_generator.analysis.coverage_percent
            )));
        }
        if generate {
            let tests = self.test_generator.generate_tests();
            self.test_generator.generated_tests = tests;
            self.toasts.push(crate::editor::toast::Toast::success(format!(
                "Generated {} test(s)",
                self.test_generator.generated_tests.len()
            )));
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Agent Memory — persistent per-member knowledge store
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_agent_memory_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let total_memories: usize = self.agent_memory.stores.iter().map(|s| s.memories.len()).sum();
        let member_count = self.agent_memory.stores.len();

        Self::tier3_header(
            ui,
            "Agent Memory",
            &format!("{} member(s) · {} memories", member_count, total_memories),
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut load = false;
        let mut save = false;
        ui.horizontal(|ui| {
            if ui.button(RichText::new("🔄 Load All").size(10.0)).clicked() {
                load = true;
            }
            if ui.button(RichText::new("💾 Save All").size(10.0)).clicked() {
                save = true;
            }
            ui.label(
                RichText::new("Encrypted with NDA")
                    .size(8.0)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Member stores
        if self.agent_memory.stores.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("◇").size(26.0).color(palette.text_muted));
                ui.label(
                    RichText::new("No agent memories yet. Memories are created during agent execution.")
                        .size(10.0)
                        .color(palette.text_muted),
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("agent_memory_scroll")
                .show(ui, |ui| {
                    for store in &self.agent_memory.stores {
                        egui::CollapsingHeader::new(
                            RichText::new(format!(
                                "👤 {} ({} memories)",
                                store.member_id,
                                store.memories.len()
                            ))
                            .size(11.0)
                            .strong(),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            for mem in &store.memories {
                                egui::Frame::new()
                                    .fill(palette.bg_secondary)
                                    .corner_radius(4.0)
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&mem.title)
                                                    .size(10.0)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(&mem.category)
                                                            .size(8.0)
                                                            .monospace()
                                                            .color(palette.accent),
                                                    );
                                                },
                                            );
                                        });
                                        ui.label(
                                            RichText::new(&mem.content)
                                                .size(9.0)
                                                .color(palette.text_muted),
                                        );
                                        if !mem.keywords.is_empty() {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new("Keywords:")
                                                        .size(8.0)
                                                        .color(palette.text_muted),
                                                );
                                                ui.label(
                                                    RichText::new(mem.keywords.join(", "))
                                                        .size(8.0)
                                                        .color(palette.text_muted),
                                                );
                                            });
                                        }
                                    });
                                ui.add_space(3.0);
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
        }

        if load {
            self.agent_memory.load_all();
            self.toasts.push(crate::editor::toast::Toast::success(format!(
                "Loaded {} member store(s)",
                self.agent_memory.stores.len()
            )));
        }
        if save {
            self.agent_memory.save_all();
            self.toasts
                .push(crate::editor::toast::Toast::success("All memories saved"));
        }
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
