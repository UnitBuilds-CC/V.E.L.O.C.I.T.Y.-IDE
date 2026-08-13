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
            &format!("{active} active \u{00b7} {total} installed"),
            palette.accent,
            palette.text_muted,
        );

        let mut rescan = false;
        let mut pending: Option<ExtAction> = None;

        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("\u{27f3} Rescan").size(10.0))
                .clicked()
            {
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
                        ui.label(RichText::new("\u{25c7}").size(26.0).color(palette.text_muted));
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
                        ExtensionState::Active => ("\u{25cf} active", palette.success),
                        ExtensionState::Installed => ("\u{25cb} installed", palette.text_muted),
                        ExtensionState::Disabled => ("\u{25cb} disabled", palette.warning),
                        ExtensionState::Error => ("\u{2716} error", palette.error),
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
                                RichText::new(format!("{cmds} command(s) \u{00b7} {kbs} keybinding(s)"))
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
                "up {} \u{00b7} {:.1} tasks/min",
                lo.session_uptime(),
                lo.throughput()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Session stat strip.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("\u{2714} {}", lo.total_tasks_completed))
                    .size(10.0)
                    .color(palette.success),
            );
            ui.label(
                RichText::new(format!("\u{2716} {}", lo.total_tasks_failed))
                    .size(10.0)
                    .color(palette.error),
            );
            ui.label(
                RichText::new(format!("\u{22ef} {} active", lo.worker_progress.len()))
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
                                "{} \u{00b7} {} file(s) changed",
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
                    "{} file(s) \u{00b7} {} symbols \u{00b7} {} lines",
                    result.files.len(),
                    result.total_symbols,
                    result.total_lines
                ))
                .size(9.0)
                .color(palette.text_muted),
            );
        } else {
            ui.label(
                RichText::new("Cache empty \u{2014} warm it to pre-index open files.")
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
            &format!("{status} \u{00b7} target: {target}"),
            palette.accent,
            palette.text_muted,
        );

        let mut run = false;
        let mut deploy = false;
        let mut rollback = false;
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("\u{25b6} Run build+test").size(10.0))
                .clicked()
            {
                run = true;
            }
            if ui
                .button(RichText::new("\u{25b2} Deploy").size(10.0))
                .clicked()
            {
                deploy = true;
            }
            if ui
                .add_enabled(
                    deployments >= 2,
                    egui::Button::new(RichText::new("\u{27f2} Rollback").size(10.0)),
                )
                .clicked()
            {
                rollback = true;
            }
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("pipeline_scroll")
            .show(ui, |ui| {
                if let Some(pipeline) = &self.deploy_pipeline {
                    for stage in PipelineStage::all() {
                        if let Some(sr) = pipeline.stages.iter().find(|s| s.stage == *stage) {
                            let (icon, color) = match &sr.status {
                                StageStatus::Passed => ("\u{2714}", palette.success),
                                StageStatus::Failed(_) => ("\u{2716}", palette.error),
                                StageStatus::Running => ("\u{22ef}", palette.warning),
                                StageStatus::Skipped => ("\u{21b7}", palette.text_muted),
                                StageStatus::Pending => ("\u{25cb}", palette.text_muted),
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
                        ui.add_space(2.0);
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
            });

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
                "{:.0}% recognized \u{00b7} {} total",
                self.voice_input.accuracy(),
                self.voice_input.total_commands
            ),
            palette.accent,
            palette.text_muted,
        );

        ui.horizontal(|ui| {
            let (label, color) = if listening {
                ("\u{25cf} Listening", palette.error)
            } else {
                ("\u{25cb} Start listening", palette.text_muted)
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
                    .hint_text("Type a phrase, e.g. 'run tests'\u{2026}")
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
                "{} source(s) \u{00b7} {} chunk(s)",
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
                    .hint_text("path to a file or folder\u{2026}")
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
                    .hint_text("search knowledge\u{2026}")
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
                            if ui
                                .small_button(RichText::new("\u{2716}").size(8.0))
                                .clicked()
                            {
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
                    if let Err(e) = self.knowledge_base.save(&ws) {
                        Self::persist_err(&mut self.toasts, "knowledge_base", &e);
                    }
                    self.toasts.push(crate::editor::toast::Toast::info(format!(
                        "Ingested {files} file(s), {chunks} chunk(s)"
                    )));
                } else {
                    match self.knowledge_base.ingest_path(&ws, &path) {
                        Ok(added) => {
                            if let Err(e) = self.knowledge_base.save(&ws) {
                                Self::persist_err(&mut self.toasts, "knowledge_base", &e);
                            }
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
            if let Err(e) = self.knowledge_base.save(&ws) {
                Self::persist_err(&mut self.toasts, "knowledge_base", &e);
            }
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
            if let Err(e) = self.knowledge_base.save(&ws) {
                Self::persist_err(&mut self.toasts, "knowledge_base", &e);
            }
            self.toasts
                .push(crate::editor::toast::Toast::info("Knowledge base cleared"));
        }
        if let Some(src) = remove {
            if self.knowledge_base.remove_source(&src) {
                let ws = self.workspace_root.clone();
                if let Err(e) = self.knowledge_base.save(&ws) {
                    Self::persist_err(&mut self.toasts, "knowledge_base", &e);
                }
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
            &format!(
                "{} trigger(s) \u{00b7} headless via --daemon",
                self.triggers.len()
            ),
            palette.accent,
            palette.text_muted,
        );

        // Add a schedule trigger: name · spec · prompt.
        let mut add = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.trigger_name_input)
                    .hint_text("name\u{2026}")
                    .desired_width(120.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.trigger_interval_input)
                    .hint_text("5m \u{00b7} 1h \u{00b7} daily@09:00")
                    .desired_width(140.0),
            );
            if ui.button(RichText::new("Add").size(10.0)).clicked() {
                add = true;
            }
        });
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.trigger_prompt_input)
                .hint_text("agent prompt to run when this schedule fires\u{2026}")
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
                                let dot = if t.enabled { "\u{25cf}" } else { "\u{25cb}" };
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
                                        if ui
                                            .small_button(RichText::new("\u{2716}").size(8.0))
                                            .clicked()
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
                if let Err(e) = self.triggers.save(&ws) {
                    Self::persist_err(&mut self.toasts, "triggers", &e);
                }
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
            if let Err(e) = self.triggers.save(&ws) {
                Self::persist_err(&mut self.toasts, "triggers", &e);
            }
        }
        if let Some(id) = remove {
            if self.triggers.remove(&id) {
                let ws = self.workspace_root.clone();
                if let Err(e) = self.triggers.save(&ws) {
                    Self::persist_err(&mut self.toasts, "triggers", &e);
                }
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
                    if let Err(e) = self.triggers.save(&ws) {
                        Self::persist_err(&mut self.toasts, "triggers", &e);
                    }
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
                            "Workflow '{}' \u{2192} {}",
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
                "{}/{} functions tested \u{00b7} {:.1}% coverage",
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
                .button(RichText::new("\u{1f50d} Analyze Coverage").size(10.0))
                .clicked()
            {
                analyze = true;
            }
            if ui
                .button(RichText::new("\u{2728} Generate Tests").size(10.0))
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
                            crate::editor::test_generator::Visibility::Public => {
                                ("pub", palette.success)
                            }
                            crate::editor::test_generator::Visibility::Private => {
                                ("priv", palette.text_muted)
                            }
                            crate::editor::test_generator::Visibility::CrateLocal => {
                                ("crate", palette.text_muted)
                            }
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
            let mut copy_idx: Option<usize> = None;
            egui::ScrollArea::vertical()
                .id_salt("test_gen_results_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for (idx, test) in self.test_generator.generated_tests.iter().enumerate() {
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
                                    .small_button(RichText::new("Copy code").size(8.0))
                                    .clicked()
                                {
                                    copy_idx = Some(idx);
                                }
                            });
                        ui.add_space(3.0);
                    }
                });
            if let Some(idx) = copy_idx {
                if let Some(test) = self.test_generator.generated_tests.get(idx) {
                    ui.ctx().copy_text(test.test_body.clone());
                    self.toasts
                        .push(crate::editor::toast::Toast::success(format!(
                            "Copied {} to clipboard",
                            test.test_name
                        )));
                }
            }
        }

        if analyze {
            let ws = self.workspace_root.clone();
            self.test_generator.analyze_coverage(&ws);
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
                    "Coverage analysis complete: {:.1}%",
                    self.test_generator.analysis.coverage_percent
                )));
        }
        if generate {
            let tests = self.test_generator.generate_tests();
            self.test_generator.generated_tests = tests;
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
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
        let total_memories: usize = self
            .agent_memory
            .stores
            .iter()
            .map(|s| s.memories.len())
            .sum();
        let member_count = self.agent_memory.stores.len();

        Self::tier3_header(
            ui,
            "Agent Memory",
            &format!(
                "{} member(s) \u{00b7} {} memories",
                member_count, total_memories
            ),
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut load = false;
        let mut save = false;
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("\u{1f504} Load All").size(10.0))
                .clicked()
            {
                load = true;
            }
            if ui
                .button(RichText::new("\u{1f4be} Save All").size(10.0))
                .clicked()
            {
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
                ui.label(
                    RichText::new("\u{25c7}")
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.label(
                    RichText::new(
                        "No agent memories yet. Memories are created during agent execution.",
                    )
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
                                "\u{1f464} {} ({} memories)",
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
            self.toasts
                .push(crate::editor::toast::Toast::success(format!(
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

    // ═══════════════════════════════════════════════════════════════════════
    // Live Orchestration — real-time multi-agent activity dashboard
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_live_orchestration_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let completed = self.live_orchestration.total_tasks_completed;
        let failed = self.live_orchestration.total_tasks_failed;
        let active_workers = self.live_orchestration.worker_progress.len();
        let events = self.live_orchestration.activity_feed.len();

        Self::tier3_header(
            ui,
            "Live Orchestration",
            &format!(
                "{} active \u{00b7} {} completed \u{00b7} {} failed",
                active_workers, completed, failed
            ),
            palette.accent,
            palette.text_muted,
        );

        // Stats
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} events", events))
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(format!(
                    "{} tokens",
                    self.live_orchestration.total_tokens_used
                ))
                .size(9.0)
                .color(palette.text_muted),
            );
            let elapsed = self.live_orchestration.session_start.elapsed();
            ui.label(
                RichText::new(format!("{}s elapsed", elapsed.as_secs()))
                    .size(9.0)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Active workers
        if !self.live_orchestration.worker_progress.is_empty() {
            ui.label(
                RichText::new("ACTIVE WORKERS")
                    .small()
                    .strong()
                    .color(palette.success),
            );
            egui::ScrollArea::vertical()
                .id_salt("orchestration_workers_scroll")
                .max_height(150.0)
                .show(ui, |ui| {
                    for worker in &self.live_orchestration.worker_progress {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("Task #{}", worker.task_id))
                                            .size(10.0)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(&worker.model_label)
                                            .size(8.0)
                                            .monospace()
                                            .color(palette.accent),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} files, {} events",
                                                    worker.files_changed, worker.events_count
                                                ))
                                                .size(8.0)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(&worker.title)
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                                if !worker.status_text.is_empty() {
                                    ui.label(
                                        RichText::new(&worker.status_text)
                                            .size(8.0)
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(3.0);
                    }
                });
            ui.add_space(6.0);
        }

        // Activity feed
        ui.label(
            RichText::new("ACTIVITY FEED")
                .small()
                .strong()
                .color(palette.accent),
        );
        egui::ScrollArea::vertical()
            .id_salt("orchestration_activity_scroll")
            .max_height(250.0)
            .show(ui, |ui| {
                if self.live_orchestration.activity_feed.is_empty() {
                    ui.label(
                        RichText::new("No activity yet. Activity appears when agents are running.")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                } else {
                    for event in self.live_orchestration.activity_feed.iter().rev() {
                        let color = match event.kind {
                            crate::editor::live_orchestration::ActivityEventKind::WorkerCompleted => {
                                palette.success
                            }
                            crate::editor::live_orchestration::ActivityEventKind::WorkerFailed => {
                                palette.error
                            }
                            crate::editor::live_orchestration::ActivityEventKind::WorkerBlocked
                            | crate::editor::live_orchestration::ActivityEventKind::InterventionQueued => {
                                palette.warning
                            }
                            _ => palette.text_muted,
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(event.kind.icon())
                                    .size(10.0)
                                    .color(color),
                            );
                            ui.label(
                                RichText::new(event.kind.label())
                                    .size(8.0)
                                    .monospace()
                                    .color(color),
                            );
                            ui.label(
                                RichText::new(&event.message)
                                    .size(9.0)
                                    .color(palette.text),
                            );
                        });
                    }
                }
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Semantic Search — TF-IDF based code search
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_semantic_search_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Semantic Search",
            if self.semantic_index.is_some() {
                "Index built"
            } else {
                "Not indexed"
            },
            palette.accent,
            palette.text_muted,
        );

        // Controls
        let mut build_index = false;
        ui.horizontal(|ui| {
            if self.semantic_index.is_none() {
                if ui
                    .button(RichText::new("\u{1f528} Build Index").size(10.0))
                    .clicked()
                {
                    build_index = true;
                }
            } else {
                if ui
                    .button(RichText::new("\u{1f504} Rebuild Index").size(10.0))
                    .clicked()
                {
                    build_index = true;
                }
            }
            ui.label(
                RichText::new("TF-IDF semantic search")
                    .size(8.0)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        if self.semantic_index.is_none() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{25c7}")
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.label(
                    RichText::new(
                        "Semantic index not built. Click 'Build Index' to enable semantic search.",
                    )
                    .size(10.0)
                    .color(palette.text_muted),
                );
            });
        } else {
            ui.label(
                RichText::new(
                    "Semantic search is active. Use the Search panel with semantic mode enabled.",
                )
                .size(9.0)
                .color(palette.text_muted),
            );
        }

        if build_index {
            let ws = self.workspace_root.clone();
            self.semantic_index = Some(crate::editor::semantic_search::SemanticIndex::build(&ws));
            self.toasts
                .push(crate::editor::toast::Toast::success("Semantic index built"));
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Snippets — code snippet library browser
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_snippets_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let count = self.snippet_collection.snippets.len();

        Self::tier3_header(
            ui,
            "Snippets",
            &format!("{} snippet(s) loaded", count),
            palette.accent,
            palette.text_muted,
        );

        // Search box
        ui.horizontal(|ui| {
            ui.label(RichText::new("Search:").size(9.0).color(palette.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut self.snippet_search_query)
                    .hint_text("filter snippets...")
                    .desired_width(ui.available_width() - 60.0),
            );
        });
        ui.add_space(6.0);

        if self.snippet_collection.snippets.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{25c7}")
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.label(
                    RichText::new(
                        "No snippets loaded. Snippets are loaded from .velocity/snippets.json",
                    )
                    .size(10.0)
                    .color(palette.text_muted),
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("snippets_scroll")
                .show(ui, |ui| {
                    let snippets_to_show: Vec<_> = if self.snippet_search_query.is_empty() {
                        self.snippet_collection.snippets.iter().collect()
                    } else {
                        self.snippet_collection
                            .matching(&self.snippet_search_query)
                            .into_iter()
                            .collect()
                    };

                    for snippet in snippets_to_show {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&snippet.name)
                                            .size(10.0)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if let Some(scope) = &snippet.scope {
                                                ui.label(
                                                    RichText::new(scope)
                                                        .size(8.0)
                                                        .monospace()
                                                        .color(palette.accent),
                                                );
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("Prefix: {}", snippet.prefix))
                                        .size(9.0)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                                if let Some(desc) = &snippet.description {
                                    ui.label(
                                        RichText::new(desc).size(8.0).color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(3.0);
                    }
                });
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // LSP Client — Language Server Protocol status and diagnostics
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_lsp_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        let (server_count, diag_count) = match &self.lsp_manager {
            Some(mgr) => (mgr.server_count(), mgr.diagnostics_count()),
            None => (0, 0),
        };

        let status_label = if server_count > 0 {
            format!(
                "{} server{}",
                server_count,
                if server_count == 1 { "" } else { "s" }
            )
        } else {
            "Not initialized".to_string()
        };

        Self::tier3_header(
            ui,
            "Language Servers",
            &status_label,
            palette.accent,
            palette.text_muted,
        );

        egui::ScrollArea::vertical()
            .id_salt("lsp_scroll")
            .show(ui, |ui| {
                if let Some(mgr) = &mut self.lsp_manager {
                    let snapshot = mgr.server_snapshot();

                    if snapshot.is_empty() {
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("\u{25c7}")
                                    .size(26.0)
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(
                                    "No language servers detected for this workspace.\n\
                                     Add Cargo.toml or package.json to auto-start servers.",
                                )
                                .size(10.0)
                                .color(palette.text_muted),
                            );
                        });
                    } else {
                        // Diagnostics summary
                        ui.label(
                            RichText::new(format!(
                                "{} diagnostic{} across all files",
                                diag_count,
                                if diag_count == 1 { "" } else { "s" }
                            ))
                            .size(9.0)
                            .color(if diag_count > 0 {
                                palette.warning
                            } else {
                                palette.text_muted
                            }),
                        );
                        ui.add_space(8.0);

                        // Per-server cards
                        for srv in &snapshot {
                            let alive_color = if srv.alive {
                                palette.success
                            } else {
                                palette.error
                            };
                            let init_label = if srv.initialized {
                                "initialized"
                            } else {
                                "starting..."
                            };

                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    // Status dot
                                    ui.label(
                                        RichText::new("\u{25cf}").size(10.0).color(alive_color),
                                    );
                                    ui.label(
                                        RichText::new(&srv.language)
                                            .size(11.0)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(init_label)
                                                    .size(9.0)
                                                    .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!(
                                        "command: {}  \u{00b7}  extensions: {}",
                                        srv.command,
                                        srv.extensions.join(", ")
                                    ))
                                    .size(8.0)
                                    .color(palette.text_muted),
                                );
                            });
                            ui.add_space(4.0);
                        }
                    }
                } else {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{25c7}")
                                .size(26.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(
                                "LSP not initialized. LSP servers are configured per-language.",
                            )
                            .size(10.0)
                            .color(palette.text_muted),
                        );
                    });
                }
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Debugger — DAP (Debug Adapter Protocol) controls
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_debugger_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Debugger",
            if self.dap_client.is_some() {
                "Connected"
            } else {
                "Not connected"
            },
            palette.accent,
            palette.text_muted,
        );

        if self.dap_client.is_none() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{25c7}")
                        .size(26.0)
                        .color(palette.text_muted),
                );
                ui.label(
                    RichText::new("Debugger not connected. Use 'Debug: Attach' from the toolbar.")
                        .size(10.0)
                        .color(palette.text_muted),
                );
            });
        } else {
            ui.label(
                RichText::new("Debugger is connected and ready for debugging.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);

            // Debug controls — defer DAP calls to avoid borrowing self during render.
            enum DbgAction {
                Continue,
                Pause,
                StepOver,
                StepInto,
                StepOut,
                Stop,
            }
            let mut dbg: Option<DbgAction> = None;

            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("\u{25b6} Continue").size(10.0))
                    .clicked()
                {
                    dbg = Some(DbgAction::Continue);
                }
                if ui
                    .button(RichText::new("\u{23f9} Pause").size(10.0))
                    .clicked()
                {
                    dbg = Some(DbgAction::Pause);
                }
                if ui
                    .button(RichText::new("\u{23ed} Step Over").size(10.0))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepOver);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("\u{2935} Step Into").size(10.0))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepInto);
                }
                if ui
                    .button(RichText::new("\u{2934} Step Out").size(10.0))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepOut);
                }
                if ui
                    .button(RichText::new("\u{23f9} Stop").size(10.0))
                    .clicked()
                {
                    dbg = Some(DbgAction::Stop);
                }
            });

            if let Some(dap) = &mut self.dap_client {
                if let Some(action) = dbg {
                    let result = match action {
                        DbgAction::Continue => dap.continue_execution(),
                        DbgAction::Pause => dap.pause(),
                        DbgAction::StepOver => dap.step_over(),
                        DbgAction::StepInto => dap.step_into(),
                        DbgAction::StepOut => dap.step_out(),
                        DbgAction::Stop => dap.stop(),
                    };
                    match result {
                        Ok(()) => {
                            let label = match action {
                                DbgAction::Continue => "Continue",
                                DbgAction::Pause => "Pause",
                                DbgAction::StepOver => "Step Over",
                                DbgAction::StepInto => "Step Into",
                                DbgAction::StepOut => "Step Out",
                                DbgAction::Stop => "Stop",
                            };
                            self.toasts
                                .push(crate::editor::toast::Toast::success(format!(
                                    "DAP: {label} sent"
                                )));
                        }
                        Err(e) => {
                            self.toasts.push(crate::editor::toast::Toast::error(e));
                        }
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Speculative Precomputation — cache status and contents
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_precomp_cache_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Precomputation Cache",
            "Speculative context pre-indexing",
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Pre-indexes scoped files before agent workers spawn, providing warm context caches that accelerate agent execution.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("precomp_cache_scroll")
            .show(ui, |ui| {
                ui.label(
                    RichText::new("CACHE STATUS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("\u{2022} Automatic: runs before each agent task")
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Background: does not block UI")
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Per-task: keyed by task ID")
                        .size(9.0)
                        .color(palette.text),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new("Each cached entry contains:")
                        .size(9.0)
                        .strong()
                        .color(palette.text_muted),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("\u{2022} File paths and line counts")
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Symbol outlines")
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Import lists")
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Top-level summaries")
                        .size(9.0)
                        .color(palette.text),
                );
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Multimodal Attachments
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_multimodal_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Multimodal Attachments",
            &format!("{} file(s) attached", self.multimodal_attachments.len()),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Attach images, documents, or audio files to chat turns. Images are encoded as data: URLs for vision models; documents use OCR fallback.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("multimodal_scroll")
            .show(ui, |ui| {
                if self.multimodal_attachments.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4ce}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "No attachments yet.\nUse the Chat panel to attach files.",
                            )
                            .size(10.0)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for att in &self.multimodal_attachments {
                        let kind_color = match att.kind {
                            crate::editor::multimodal::AttachmentKind::Image => palette.success,
                            crate::editor::multimodal::AttachmentKind::Document => palette.accent,
                            crate::editor::multimodal::AttachmentKind::Audio => palette.warning,
                        };
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("\u{25cf}").color(kind_color));
                                    ui.label(
                                        RichText::new(att.kind.label())
                                            .small()
                                            .strong()
                                            .color(kind_color),
                                    );
                                    ui.label(
                                        RichText::new(&att.mime).small().color(palette.text_muted),
                                    );
                                });
                                ui.label(
                                    RichText::new(att.path.display().to_string())
                                        .size(9.0)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!("{} bytes", att.data.len()))
                                        .small()
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(3.0);
                    }
                }
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Continuation Ledger
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_continuation_ledger_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Continuation Ledger",
            "Cross-model context handoff",
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Captures mission state, edit journals, and model provenance so a different AI model can seamlessly resume work.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("continuation_ledger_scroll")
            .show(ui, |ui| {
                match &self.continuation_ledger {
                    None => {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("\u{1f4cb}")
                                    .size(24.0)
                                    .color(palette.text_muted.gamma_multiply(0.5)),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "No active continuation ledger.\nA ledger is created when handing off context between models.",
                                )
                                .size(10.0)
                                .color(palette.text_muted),
                            );
                        });
                    }
                    Some(ledger) => {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(4.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("Ledger: {}", ledger.id))
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(format!("Mission: {}", ledger.mission.goal))
                                        .size(9.0)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Scoped files: {}",
                                        ledger.environment.scoped_files.len()
                                    ))
                                    .size(9.0)
                                    .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Edit journal: {} entries",
                                        ledger.journal.completed_edits.len()
                                    ))
                                    .size(9.0)
                                    .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Progress: {}/{} steps done",
                                        ledger
                                            .progress
                                            .steps
                                            .iter()
                                            .filter(|s| matches!(
                                                s.status,
                                                crate::editor::continuation_ledger::StepStatus::Done
                                            ))
                                            .count(),
                                        ledger.progress.steps.len()
                                    ))
                                    .size(9.0)
                                    .color(palette.success),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Provenance: {} model attempt(s)",
                                        ledger.provenance.len()
                                    ))
                                    .size(9.0)
                                    .color(palette.text_muted),
                                );
                            });
                    }
                }
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Plugin Registry
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_plugin_registry_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let plugin_count = self.plugin_registry.count();
        let plugins = self.plugin_registry.list();
        let all_tools = self.plugin_registry.all_tools();

        Self::tier3_header(
            ui,
            "Plugin Registry",
            &format!(
                "{plugin_count} plugin(s) \u{00b7} {} tool(s)",
                all_tools.len()
            ),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new("Plugins extend the IDE with additional tools and capabilities.")
                .size(9.0)
                .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("plugin_registry_scroll")
            .show(ui, |ui| {
                if plugins.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4e6}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "No plugins loaded.\nPlace plugin crates in the workspace to discover them.",
                            )
                            .size(10.0)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for info in &plugins {
                        let status_color = if info.enabled {
                            palette.success
                        } else {
                            palette.text_muted
                        };
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("\u{25cf}").color(status_color));
                                    ui.label(
                                        RichText::new(&info.name).strong().color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(format!("v{}", info.version))
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                });
                                ui.label(
                                    RichText::new(&info.description)
                                        .size(9.0)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} tool(s): {}",
                                        info.tool_count,
                                        info.tool_names.join(", ")
                                    ))
                                    .small()
                                    .color(palette.text_muted),
                                );
                            });
                        ui.add_space(3.0);
                    }
                }
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Skill Files
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_skill_files_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        Self::tier3_header(
            ui,
            "Skill Files",
            &format!("{} skill(s) loaded", self.skill_files.len()),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Skills are reusable capability definitions injected into agent system prompts when tasks are routed to team members.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("skill_files_scroll")
            .show(ui, |ui| {
                if self.skill_files.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4dc}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "No skill files loaded.\nSkills are loaded from .velocity/skills/.",
                            )
                            .size(10.0)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for skill in &self.skill_files {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&skill.name).strong().color(palette.accent),
                                    );
                                    ui.label(
                                        RichText::new(format!("[{}]", skill.id))
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                });
                                ui.label(
                                    RichText::new(&skill.description)
                                        .size(9.0)
                                        .color(palette.text),
                                );
                                // Show first 120 chars of body as preview
                                let preview: String = skill.body.chars().take(120).collect();
                                if preview.len() < skill.body.len() {
                                    ui.label(
                                        RichText::new(format!("{preview}\u{2026}"))
                                            .small()
                                            .monospace()
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(3.0);
                    }
                }
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Inline Suggestions
    // ═══════════════════════════════════════════════════════════════════════
    pub fn render_inline_suggestions_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();

        let enabled = self.inline_suggestions.config.enabled;
        let total_shown = self.inline_suggestions.total_shown;
        let total_accepted = self.inline_suggestions.total_accepted;
        let total_dismissed = self.inline_suggestions.total_dismissed;
        let cache_entries = self.inline_suggestions.suggestion_cache.len();
        let recent_count = self.inline_suggestions.recent_suggestions.len();
        let has_current = self.inline_suggestions.current_suggestion.is_some();

        Self::tier3_header(
            ui,
            "Inline Suggestions",
            if enabled { "enabled" } else { "disabled" },
            if enabled {
                palette.success
            } else {
                palette.text_muted
            },
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Ghost-text suggestions that appear inline as you type, powered by the completion engine. Press Tab to accept, Escape to dismiss.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("inline_suggestions_scroll")
            .show(ui, |ui| {
                // Configuration
                ui.label(
                    RichText::new("CONFIGURATION")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Enabled:")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    if ui
                        .checkbox(&mut self.inline_suggestions.config.enabled, "")
                        .changed()
                    {
                        // config updated
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Trigger delay:")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}ms",
                            self.inline_suggestions.config.trigger_delay_ms
                        ))
                        .size(9.0)
                        .color(palette.text),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Max chars:")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}",
                            self.inline_suggestions.config.max_suggestion_chars
                        ))
                        .size(9.0)
                        .color(palette.text),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Min confidence:")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{:.0}%",
                            self.inline_suggestions.config.min_confidence * 100.0
                        ))
                        .size(9.0)
                        .color(palette.text),
                    );
                });
                ui.add_space(6.0);

                // Statistics
                ui.label(
                    RichText::new("STATISTICS")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Shown: {total_shown}"))
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Accepted: {total_accepted}"))
                        .size(9.0)
                        .color(palette.success),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Dismissed: {total_dismissed}"))
                        .size(9.0)
                        .color(palette.warning),
                );
                let accept_rate = if total_shown > 0 {
                    (total_accepted as f32 / total_shown as f32) * 100.0
                } else {
                    0.0
                };
                ui.label(
                    RichText::new(format!("  \u{2022} Accept rate: {accept_rate:.1}%"))
                        .size(9.0)
                        .color(palette.accent),
                );
                ui.add_space(4.0);

                // Cache info
                ui.label(
                    RichText::new("CACHE")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Reuse cache: {cache_entries} entries"))
                        .size(9.0)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Recent: {recent_count} entries"))
                        .size(9.0)
                        .color(palette.text),
                );
                let status = if has_current {
                    ("Pending suggestion", palette.warning)
                } else {
                    ("Idle \u{2014} waiting for trigger", palette.text_muted)
                };
                ui.label(
                    RichText::new(format!("  \u{2022} Status: {}", status.0))
                        .size(9.0)
                        .color(status.1),
                );
            });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Agent Subsystem Panels
    // ═══════════════════════════════════════════════════════════════════════

    pub fn render_improvement_engine_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let failure_count = self.improvement_engine.failure_count();
        let has_data = self.improvement_engine.has_data();
        let directives = self.improvement_engine.analyze();

        Self::tier3_header(
            ui,
            "Self-Improvement Engine",
            &format!("{failure_count} failure(s) recorded"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Tracks failures during agent execution, classifies them into categories, and generates prompt refinements to avoid repeating mistakes.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("improvement_engine_scroll")
            .show(ui, |ui| {
                if !has_data {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{2699}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "No failures recorded this session.\nThe engine is idle.",
                            )
                            .size(10.0)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    ui.label(
                        RichText::new(format!("Generated {} directive(s):", directives.len()))
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(4.0);
                    for d in &directives {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{:?}", d.category))
                                        .small()
                                        .strong()
                                        .color(palette.warning),
                                );
                                ui.label(RichText::new(&d.directive).size(9.0).color(palette.text));
                                ui.label(
                                    RichText::new(format!(
                                        "confidence: {:.0}% \u{00b7} {} occurrence(s)",
                                        d.confidence * 100.0,
                                        d.occurrences
                                    ))
                                    .small()
                                    .color(palette.text_muted),
                                );
                            });
                        ui.add_space(3.0);
                    }
                }
            });
    }

    pub fn render_shared_memory_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let entry_count = self.shared_memory.entries.len();
        let annotation_count = self.shared_memory.annotations.len();

        Self::tier3_header(
            ui,
            "Shared Memory",
            &format!("{entry_count} entries \u{00b7} {annotation_count} annotations"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Shared knowledge base for multi-agent collaboration. Agents can publish and query knowledge entries across team boundaries.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("shared_memory_scroll")
            .show(ui, |ui| {
                if entry_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f4da}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No shared knowledge entries yet.")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                    });
                } else {
                    for (id, entry) in self.shared_memory.entries.iter().take(20) {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&entry.title).strong().color(palette.text));
                                ui.label(
                                    RichText::new(format!("[{id}] {:?}", entry.category))
                                        .small()
                                        .color(palette.text_muted),
                                );
                                let preview: String = entry.content.chars().take(120).collect();
                                ui.label(RichText::new(preview).size(9.0).color(palette.text));
                            });
                        ui.add_space(3.0);
                    }
                }
            });
    }

    pub fn render_background_agents_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let agent_count = self.background_agents.agents.len();
        let feed_len = self.background_agents.action_feed.len();

        Self::tier3_header(
            ui,
            "Background Agents",
            &format!("{agent_count} agent(s) \u{00b7} {feed_len} action(s)"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new("Background agents run autonomous tasks without blocking the UI.")
                .size(9.0)
                .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("background_agents_scroll")
            .show(ui, |ui| {
                if agent_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f916}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No background agents registered.")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                    });
                } else {
                    for agent in self.background_agents.agents.values() {
                        let status_color = if agent.enabled {
                            palette.success
                        } else {
                            palette.text_muted
                        };
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("\u{25cf}").color(status_color));
                                    ui.label(RichText::new(&agent.id).strong().color(palette.text));
                                    ui.label(
                                        RichText::new(if agent.enabled {
                                            "active"
                                        } else {
                                            "disabled"
                                        })
                                        .small()
                                        .color(status_color),
                                    );
                                });
                                ui.label(RichText::new(&agent.name).size(9.0).color(palette.text));
                            });
                        ui.add_space(3.0);
                    }
                }

                if feed_len > 0 {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("RECENT ACTIONS")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(2.0);
                    for action in self.background_agents.action_feed.iter().rev().take(10) {
                        ui.label(
                            RichText::new(format!("[{}] {}", action.id, action.title))
                                .size(9.0)
                                .color(palette.text),
                        );
                    }
                }
            });
    }

    pub fn render_conflict_resolver_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let lock_count = self.conflict_resolver.locks.len();
        let conflict_count = self.conflict_resolver.conflicts.len();

        Self::tier3_header(
            ui,
            "Conflict Resolver",
            &format!("{lock_count} lock(s) \u{00b7} {conflict_count} conflict(s)"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(format!(
                "Manages resource contention between concurrent agent operations. Strategy: {:?} \u{00b7} Lock timeout: {}s",
                self.conflict_resolver.default_resolution, self.conflict_resolver.lock_timeout_secs
            ))
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("conflict_resolver_scroll")
            .show(ui, |ui| {
                if lock_count == 0 && conflict_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{2714}")
                                .size(24.0)
                                .color(palette.success.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No active locks or conflicts.\nAll resources are free.")
                                .size(10.0)
                                .color(palette.success),
                        );
                    });
                } else {
                    if lock_count > 0 {
                        ui.label(
                            RichText::new("ACTIVE LOCKS")
                                .small()
                                .strong()
                                .color(palette.warning),
                        );
                        ui.add_space(2.0);
                        for (resource, locks) in self.conflict_resolver.locks.iter() {
                            egui::Frame::new()
                                .fill(palette.bg_tertiary)
                                .corner_radius(4.0)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} \u{2014} {} holder(s)",
                                            resource,
                                            locks.len()
                                        ))
                                        .size(9.0)
                                        .color(palette.text),
                                    );
                                });
                            ui.add_space(2.0);
                        }
                        ui.add_space(4.0);
                    }
                    if conflict_count > 0 {
                        ui.label(
                            RichText::new("RECENT CONFLICTS")
                                .small()
                                .strong()
                                .color(palette.error),
                        );
                        ui.add_space(2.0);
                        for c in self.conflict_resolver.conflicts.iter().rev().take(10) {
                            egui::Frame::new()
                                .fill(palette.bg_tertiary)
                                .corner_radius(4.0)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} vs {} on {}",
                                            c.op_a.actor_id, c.op_b.actor_id, c.resource
                                        ))
                                        .size(9.0)
                                        .color(palette.text),
                                    );
                                });
                            ui.add_space(2.0);
                        }
                    }
                }
            });
    }

    pub fn render_collaboration_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let user_count = self.collaboration.users.len();
        let session_count = self.collaboration.sessions.len();

        Self::tier3_header(
            ui,
            "Collaboration",
            &format!("{user_count} user(s) \u{00b7} {session_count} session(s)"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Manages shared editing sessions, user presence, and real-time collaboration between team members and remote agents.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("collaboration_scroll")
            .show(ui, |ui| {
                if user_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f465}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No users registered.\nCollaboration is idle.")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                    });
                } else {
                    ui.label(
                        RichText::new("USERS")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(2.0);
                    for (id, user) in &self.collaboration.users {
                        let online = self.collaboration.presence.contains_key(id);
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(4.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(if online { "\u{25cf}" } else { "\u{25cb}" })
                                            .color(if online {
                                                palette.success
                                            } else {
                                                palette.text_muted
                                            }),
                                    );
                                    ui.label(
                                        RichText::new(&user.name).size(10.0).color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(format!("[{id}]"))
                                            .small()
                                            .color(palette.text_muted),
                                    );
                                });
                            });
                        ui.add_space(2.0);
                    }

                    // Sessions section
                    if !self.collaboration.sessions.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("SESSIONS")
                                .small()
                                .strong()
                                .color(palette.accent),
                        );
                        ui.add_space(2.0);
                        for session in self.collaboration.sessions.values().take(10) {
                            let status_color = match session.status {
                                crate::agent::collaboration::SessionStatus::Active => {
                                    palette.success
                                }
                                crate::agent::collaboration::SessionStatus::Paused => {
                                    palette.warning
                                }
                                crate::agent::collaboration::SessionStatus::Completed => {
                                    palette.text_muted
                                }
                                _ => palette.text_muted,
                            };
                            egui::Frame::new()
                                .fill(palette.bg_tertiary)
                                .corner_radius(4.0)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("\u{25cf}").color(status_color));
                                        ui.label(
                                            RichText::new(&session.name)
                                                .strong()
                                                .color(palette.text),
                                        );
                                        ui.label(
                                            RichText::new(session.status.label())
                                                .small()
                                                .color(status_color),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{} participant(s) \u{00b7} {} message(s)",
                                            session.participants.len(),
                                            session.messages.len()
                                        ))
                                        .size(9.0)
                                        .color(palette.text_muted),
                                    );
                                });
                            ui.add_space(3.0);
                        }
                    }
                }
            });
    }

    pub fn render_persistent_memory_panel(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette();
        let entry_count = self.persistent_memory.len();

        Self::tier3_header(
            ui,
            "Persistent Memory",
            &format!("{entry_count} entries \u{00b7} NDA-encrypted at rest"),
            palette.accent,
            palette.text_muted,
        );

        ui.label(
            RichText::new(
                "Long-term memory store encrypted with NDA at rest. Agents can remember, recall, reinforce, and forget entries across sessions.",
            )
            .size(9.0)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("persistent_memory_scroll")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Storage: {} / max entries", entry_count))
                        .size(9.0)
                        .color(palette.text_muted),
                );
                ui.add_space(4.0);

                if entry_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f512}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Memory is empty.\nAgents will populate it during execution.",
                            )
                            .size(10.0)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    ui.label(
                        RichText::new("STORED ENTRIES")
                            .small()
                            .strong()
                            .color(palette.accent),
                    );
                    ui.add_space(2.0);
                    for entry in self.persistent_memory.iter().take(30) {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("\u{1f512}")
                                            .size(10.0)
                                            .color(palette.text_muted),
                                    );
                                    ui.label(
                                        RichText::new(&entry.key).strong().color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "accessed \u{00d7}{}",
                                            entry.access_count
                                        ))
                                        .small()
                                        .color(palette.text_muted),
                                    );
                                });
                                let preview: String = entry.content.chars().take(100).collect();
                                ui.label(RichText::new(preview).size(9.0).color(palette.text));
                            });
                        ui.add_space(3.0);
                    }
                    if entry_count > 30 {
                        ui.label(
                            RichText::new(format!("... and {} more entries", entry_count - 30))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                    }
                }
            });
    }
}

fn trigger_kind_label(kind: &crate::editor::triggers::TriggerKind) -> String {
    use crate::editor::triggers::TriggerKind;
    match kind {
        TriggerKind::Schedule { interval } => format!("schedule \u{00b7} {interval}"),
        TriggerKind::FileWatch { path, glob } => format!("file-watch \u{00b7} {path}/{glob}"),
        TriggerKind::Webhook { .. } => "webhook".to_string(),
        TriggerKind::Manual => "manual".to_string(),
    }
}

fn trigger_action_label(action: &crate::editor::triggers::TriggerAction) -> String {
    use crate::editor::triggers::TriggerAction;
    match action {
        TriggerAction::RunWorkflow { workflow_id } => format!("\u{2192} workflow {workflow_id}"),
        TriggerAction::AgentPrompt { prompt } => {
            let p: String = prompt.chars().take(60).collect();
            format!("\u{2192} agent: {p}")
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
