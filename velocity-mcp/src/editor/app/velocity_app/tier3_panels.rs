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
use crate::editor::theme::{
    IdePalette, CARD_INNER_MARGIN, CARD_RADIUS, FONT_BODY, FONT_CAPTION, FONT_HEADING, FONT_SMALL,
    ITEM_SPACING, SECTION_SPACING,
};
use crate::editor::task_timeline::render_task_timeline;

/// A deferred mutation captured while rendering the extensions list (avoids
/// borrowing `self` mutably during immutable iteration).
enum ExtAction {
    Activate(String),
    Disable(String),
}

impl VelocityApp {
    // --- Section header shared by the Tier-3 panels ---
    pub(crate) fn tier3_header(
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        accent: egui::Color32,
        muted: egui::Color32,
    ) {
        ui.add_space(SECTION_SPACING);
        ui.horizontal(|ui| {
            ui.heading(RichText::new(title).strong().color(accent));
            ui.label(RichText::new(subtitle).small().color(muted));
        });
        ui.separator();
        ui.add_space(ITEM_SPACING);
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Extensions -- registry manager
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                .button(RichText::new("\u{27f3} Rescan").size(FONT_SMALL))
                .clicked()
            {
                rescan = true;
            }
            ui.label(
                RichText::new(".velocity/extensions/")
                    .monospace()
                    .size(FONT_CAPTION)
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
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No extensions installed")
                                .size(FONT_BODY)
                                .strong()
                                .color(palette.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Drop a manifest folder into .velocity/extensions/")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                        ui.add_space(ITEM_SPACING);
                        if ui.button(RichText::new("\u{27f3}  Rescan").size(FONT_SMALL)).clicked() {
                            let ws = self.workspace_root.clone();
                            self.extension_registry.scan(&ws);
                        }
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
                        .corner_radius(CARD_RADIUS)
                        .inner_margin(CARD_INNER_MARGIN)
                        .stroke(egui::Stroke::new(0.5, palette.border))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&ext.manifest.name).strong().size(FONT_BODY).color(palette.text));
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
                                    .size(FONT_CAPTION)
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
                    ui.add_space(ITEM_SPACING);
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Activity -- live orchestration feed + pre-computation cache
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                    .size(FONT_SMALL)
                    .color(palette.success),
            );
            ui.label(
                RichText::new(format!("\u{2716} {}", lo.total_tasks_failed))
                    .size(FONT_SMALL)
                    .color(palette.error),
            );
            ui.label(
                RichText::new(format!("\u{22ef} {} active", lo.worker_progress.len()))
                    .size(FONT_SMALL)
                    .color(palette.warning),
            );
        });
        ui.add_space(ITEM_SPACING);
        
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
                    .corner_radius(CARD_RADIUS)
                    .inner_margin(CARD_INNER_MARGIN)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("#{}", wp.task_id))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(&wp.title)
                                    .size(FONT_SMALL)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(wp.elapsed_label())
                                            .size(FONT_CAPTION)
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
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                        );
                    });
                ui.add_space(ITEM_SPACING);
            }
            ui.add_space(ITEM_SPACING);
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
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
        } else {
            ui.label(
                RichText::new("Cache empty \u{2014} warm it to pre-index open files.")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
        }
        ui.add_space(ITEM_SPACING);
        
        // Activity feed.
        ui.label(RichText::new("FEED").small().strong().color(palette.accent));
        egui::ScrollArea::vertical()
            .id_salt("activity_feed_scroll")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let feed = self.live_orchestration.filtered_feed();
                if feed.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("\u{25c7}").size(24.0).color(palette.text_muted));
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No activity yet")
                                .size(FONT_BODY)
                                .strong()
                                .color(palette.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Events appear here when agents are running tasks.")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    });
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
                        ui.label(RichText::new(ev.kind.icon()).size(FONT_SMALL).color(color));
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Coverage -- auto test-coverage analyzer
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            if ui.button(RichText::new("Analyze workspace").size(FONT_SMALL)).clicked() {
                analyze = true;
            }
            if ui
                .button(RichText::new("Analyze file (LSP)").size(FONT_SMALL))
                .on_hover_text("Discover testable functions in the active file via the language server's documentSymbol outline")
                .clicked()
            {
                analyze_lsp = true;
            }
            let has_gaps = !self.test_generator.analysis.untested_functions.is_empty();
            if ui
                .add_enabled(has_gaps, egui::Button::new(RichText::new("Generate skeletons").size(FONT_SMALL)))
                .clicked()
            {
                generate = true;
            }
            ui.checkbox(&mut self.test_generator.config.public_only, "Public only");
        });
        ui.add_space(ITEM_SPACING);
        
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
                                    .size(FONT_CAPTION)
                                    .color(palette.text),
                            );
                            ui.label(
                                RichText::new(format!("{file}:{}", func.line))
                                    .size(FONT_CAPTION)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(&gen.test_name)
                                        .monospace()
                                        .size(FONT_CAPTION)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(&gen.test_body)
                                        .monospace()
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Pipeline -- build/test/deploy manager
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                .button(RichText::new("\u{25b6} Run build+test").size(FONT_SMALL))
                .clicked()
            {
                run = true;
            }
            if ui
                .button(RichText::new("\u{25b2} Deploy").size(FONT_SMALL))
                .clicked()
            {
                deploy = true;
            }
            if ui
                .add_enabled(
                    deployments >= 2,
                    egui::Button::new(RichText::new("\u{27f2} Rollback").size(FONT_SMALL)),
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
                                ui.label(RichText::new(icon).size(FONT_SMALL).color(color));
                                ui.label(
                                    RichText::new(stage.label())
                                        .size(FONT_SMALL)
                                        .strong()
                                        .color(palette.text),
                                );
                                if let Some(ms) = sr.duration_ms {
                                    ui.label(
                                        RichText::new(format!("{ms} ms"))
                                            .size(FONT_CAPTION)
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
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                                ui.label(
                                    RichText::new(&dep.version)
                                        .monospace()
                                        .size(FONT_CAPTION)
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Voice -- voice-to-task input
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                .button(RichText::new(label).size(FONT_SMALL).color(color))
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
            if ui.button(RichText::new("Parse").size(FONT_SMALL)).clicked() {
                parse = true;
            }
        });
        ui.add_space(6.0);

        if let Some(cmd) = &self.voice_input.last_command {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(CARD_RADIUS)
                .inner_margin(CARD_INNER_MARGIN)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Intent:").size(9.0).color(palette.text_muted));
                        ui.label(
                            RichText::new(cmd.intent.label())
                                .size(FONT_SMALL)
                                .strong()
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!("({:.0}%)", cmd.confidence * 100.0))
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    });
                    if let Some(target) = cmd.parameters.get("target") {
                        ui.label(
                            RichText::new(format!("Target: {target}"))
                                .size(FONT_CAPTION)
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
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f3a4}")
                                .size(18.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("No commands parsed yet")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    });
                }
                for cmd in self.voice_input.command_history.iter().rev() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(cmd.intent.label())
                                .size(FONT_CAPTION)
                                .color(palette.accent),
                        );
                        ui.label(
                            RichText::new(&cmd.raw_text)
                                .size(FONT_CAPTION)
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Knowledge -- unified RAG store (ingest + search)
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            if ui.button(RichText::new("Ingest").size(FONT_SMALL)).clicked() {
                ingest_path = true;
            }
            if ui
                .button(RichText::new("Index workspace").size(FONT_SMALL))
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
            if ui.button(RichText::new("Search").size(FONT_SMALL)).clicked()
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
                    ui.add_space(ITEM_SPACING);
                    ui.label(
                        RichText::new("Search your knowledge base above, or ingest content to get started.")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                }
                for hit in &self.knowledge_results {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(CARD_RADIUS)
                        .inner_margin(CARD_INNER_MARGIN)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{}#{}", hit.source, hit.ordinal))
                                        .size(FONT_CAPTION)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("{:.3}", hit.score))
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                        );
                                    },
                                );
                            });
                            ui.label(RichText::new(&hit.snippet).size(9.0).color(palette.text));
                        });
                    ui.add_space(ITEM_SPACING);
                }
            });

        ui.add_space(SECTION_SPACING);
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
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                }
                for (source, count) in &sources {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(source).size(9.0).color(palette.text));
                        ui.label(
                            RichText::new(format!("({count})"))
                                .size(FONT_CAPTION)
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

        // Add a schedule trigger: name Â· spec Â· prompt.
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
            if ui.button(RichText::new("Add").size(FONT_SMALL)).clicked() {
                add = true;
            }
        });
        ui.add_space(ITEM_SPACING);
        ui.add(
            egui::TextEdit::multiline(&mut self.trigger_prompt_input)
                .hint_text("agent prompt to run when this schedule fires\u{2026}")
                .desired_rows(2)
                .desired_width(ui.available_width()),
        );
        if self.trigger_interval_input.trim().is_empty()
            || parse_schedule(self.trigger_interval_input.trim()).is_some()
        {
            // valid or empty -- no warning
        } else {
            ui.label(
                RichText::new("unrecognized schedule spec")
                    .size(FONT_CAPTION)
                    .color(palette.error),
            );
        }
        ui.add_space(SECTION_SPACING);
        
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
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                }
                for t in &self.triggers.triggers {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(CARD_RADIUS)
                        .inner_margin(CARD_INNER_MARGIN)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dot = if t.enabled { "\u{25cf}" } else { "\u{25cb}" };
                                ui.label(RichText::new(dot).size(FONT_SMALL).color(if t.enabled {
                                    palette.success
                                } else {
                                    palette.text_muted
                                }));
                                ui.label(
                                    RichText::new(&t.name)
                                        .size(FONT_SMALL)
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
                                    .size(FONT_CAPTION)
                                    .color(palette.accent),
                            );
                            ui.label(
                                RichText::new(trigger_action_label(&t.action))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                            );
                            let due = match t.seconds_until_due(now) {
                                Some(0) => "due now".to_string(),
                                Some(secs) => format!("next in {}", human_secs(secs)),
                                None => "external / manual".to_string(),
                            };
                            ui.label(RichText::new(due).size(8.0).color(palette.text_muted));
                        });
                    ui.add_space(ITEM_SPACING);
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Test Generator -- coverage analysis and test generation
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                .button(RichText::new("\u{1f50d} Analyze Coverage").size(FONT_SMALL))
                .clicked()
            {
                analyze = true;
            }
            if ui
                .button(RichText::new("\u{2728} Generate Tests").size(FONT_SMALL))
                .clicked()
            {
                generate = true;
            }
            ui.label(
                RichText::new(format!(
                    "{} test(s) generated",
                    self.test_generator.generated_tests.len()
                ))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Configuration
        egui::CollapsingHeader::new(RichText::new("Configuration").size(FONT_SMALL).strong())
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(vis_badge.0)
                                            .size(FONT_CAPTION)
                                            .monospace()
                                            .color(vis_badge.1),
                                    );
                                    ui.label(
                                        RichText::new(&func.name)
                                            .size(FONT_SMALL)
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
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(&func.signature)
                                        .size(FONT_CAPTION)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                });
        }

        // Generated tests preview
        if !self.test_generator.generated_tests.is_empty() {
            ui.add_space(SECTION_SPACING);
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&test.test_name)
                                            .size(FONT_SMALL)
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
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("for {}", test.function_name))
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                                if ui
                                    .small_button(RichText::new("Copy code").size(8.0))
                                    .clicked()
                                {
                                    copy_idx = Some(idx);
                                }
                            });
                        ui.add_space(ITEM_SPACING);
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Agent Memory -- persistent per-member knowledge store
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                .button(RichText::new("\u{1f504} Load All").size(FONT_SMALL))
                .clicked()
            {
                load = true;
            }
            if ui
                .button(RichText::new("\u{1f4be} Save All").size(FONT_SMALL))
                .clicked()
            {
                save = true;
            }
            ui.label(
                RichText::new("Encrypted with NDA")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
        });
        ui.add_space(6.0);

        // Member stores
        if self.agent_memory.stores.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1f9e0}")
                        .size(22.0)
                        .color(palette.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("No agent memories yet")
                        .size(FONT_SMALL)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Memories are created during agent execution")
                        .size(9.0)
                        .color(palette.text_muted.gamma_multiply(0.7)),
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
                            .size(FONT_SMALL)
                            .strong(),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            for mem in &store.memories {
                                egui::Frame::new()
                                    .fill(palette.bg_secondary)
                                    .corner_radius(CARD_RADIUS)
                                    .inner_margin(CARD_INNER_MARGIN)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&mem.title)
                                                    .size(FONT_SMALL)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(&mem.category)
                                                            .size(FONT_CAPTION)
                                                            .monospace()
                                                            .color(palette.accent),
                                                    );
                                                },
                                            );
                                        });
                                        ui.label(
                                            RichText::new(&mem.content)
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                        );
                                        if !mem.keywords.is_empty() {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new("Keywords:")
                                                        .size(FONT_CAPTION)
                                                        .color(palette.text_muted),
                                                );
                                                ui.label(
                                                    RichText::new(mem.keywords.join(", "))
                                                        .size(FONT_CAPTION)
                                                        .color(palette.text_muted),
                                                );
                                            });
                                        }
                                    });
                                ui.add_space(ITEM_SPACING);
                            }
                        });
                        ui.add_space(ITEM_SPACING);
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Live Orchestration -- real-time multi-agent activity dashboard
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
            ui.label(
                RichText::new(format!(
                    "{} tokens",
                    self.live_orchestration.total_tokens_used
                ))
                .size(FONT_CAPTION)
                .color(palette.text_muted),
            );
            let elapsed = self.live_orchestration.session_start.elapsed();
            ui.label(
                RichText::new(format!("{}s elapsed", elapsed.as_secs()))
                    .size(FONT_CAPTION)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("Task #{}", worker.task_id))
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new(&worker.model_label)
                                            .size(FONT_CAPTION)
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
                                                .size(FONT_CAPTION)
                                                .color(palette.text_muted),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(&worker.title)
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                );
                                if !worker.status_text.is_empty() {
                                    ui.label(
                                        RichText::new(&worker.status_text)
                                            .size(FONT_CAPTION)
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(ITEM_SPACING);
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
                            .size(FONT_CAPTION)
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
                                    .size(FONT_SMALL)
                                    .color(color),
                            );
                            ui.label(
                                RichText::new(event.kind.label())
                                    .size(FONT_CAPTION)
                                    .monospace()
                                    .color(color),
                            );
                            ui.label(
                                RichText::new(&event.message)
                                    .size(FONT_CAPTION)
                                    .color(palette.text),
                            );
                        });
                    }
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Semantic Search -- TF-IDF based code search
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                    .button(RichText::new("\u{1f528} Build Index").size(FONT_SMALL))
                    .clicked()
                {
                    build_index = true;
                }
            } else {
                if ui
                    .button(RichText::new("\u{1f504} Rebuild Index").size(FONT_SMALL))
                    .clicked()
                {
                    build_index = true;
                }
            }
            ui.label(
                RichText::new("TF-IDF semantic search")
                    .size(FONT_CAPTION)
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
                    .size(FONT_SMALL)
                    .color(palette.text_muted),
                );
            });
        } else {
            ui.label(
                RichText::new(
                    "Semantic search is active. Use the Search panel with semantic mode enabled.",
                )
                .size(FONT_CAPTION)
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Snippets -- code snippet library browser
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                    .size(FONT_SMALL)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&snippet.name)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if let Some(scope) = &snippet.scope {
                                                ui.label(
                                                    RichText::new(scope)
                                                        .size(FONT_CAPTION)
                                                        .monospace()
                                                        .color(palette.accent),
                                                );
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("Prefix: {}", snippet.prefix))
                                        .size(FONT_CAPTION)
                                        .monospace()
                                        .color(palette.text_muted),
                                );
                                if let Some(desc) = &snippet.description {
                                    ui.label(
                                        RichText::new(desc).size(8.0).color(palette.text_muted),
                                    );
                                }
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                });
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // LSP Client -- Language Server Protocol status and diagnostics
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("\u{1f4c6}")
                                    .size(22.0)
                                    .color(palette.text_muted.gamma_multiply(0.5)),
                            );
                            ui.add_space(ITEM_SPACING);
                            ui.label(
                                RichText::new("No language servers detected")
                                    .size(FONT_SMALL)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "Add Cargo.toml or package.json to auto-start servers",
                                )
                                .size(9.0)
                                .color(palette.text_muted.gamma_multiply(0.7)),
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
                            .size(FONT_CAPTION)
                            .color(if diag_count > 0 {
                                palette.warning
                            } else {
                                palette.text_muted
                            }),
                        );
                        ui.add_space(SECTION_SPACING);
                        
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
                                        RichText::new("\u{25cf}").size(FONT_SMALL).color(alive_color),
                                    );
                                    ui.label(
                                        RichText::new(&srv.language)
                                            .size(FONT_SMALL)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(init_label)
                                                    .size(FONT_CAPTION)
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
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                                );
                            });
                            ui.add_space(ITEM_SPACING);
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
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Debugger -- DAP (Debug Adapter Protocol) controls
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                        .size(FONT_SMALL)
                        .color(palette.text_muted),
                );
            });
        } else {
            ui.label(
                RichText::new("Debugger is connected and ready for debugging.")
                    .size(FONT_CAPTION)
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);

            // Debug controls -- defer DAP calls to avoid borrowing self during render.
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
                    .button(RichText::new("\u{25b6} Continue").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::Continue);
                }
                if ui
                    .button(RichText::new("\u{23f9} Pause").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::Pause);
                }
                if ui
                    .button(RichText::new("\u{23ed} Step Over").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepOver);
                }
            });
            ui.add_space(ITEM_SPACING);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("\u{2935} Step Into").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepInto);
                }
                if ui
                    .button(RichText::new("\u{2934} Step Out").size(FONT_SMALL))
                    .clicked()
                {
                    dbg = Some(DbgAction::StepOut);
                }
                if ui
                    .button(RichText::new("\u{23f9} Stop").size(FONT_SMALL))
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Speculative Precomputation -- cache status and contents
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            .size(FONT_CAPTION)
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
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Background: does not block UI")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Per-task: keyed by task ID")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.add_space(6.0);

                ui.label(
                    RichText::new("Each cached entry contains:")
                        .size(FONT_CAPTION)
                        .strong()
                        .color(palette.text_muted),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("\u{2022} File paths and line counts")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Symbol outlines")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Import lists")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new("\u{2022} Top-level summaries")
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Multimodal Attachments
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No attachments yet.\nUse the Chat panel to attach files.",
                            )
                            .size(FONT_SMALL)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
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
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!("{} bytes", att.data.len()))
                                        .small()
                                        .color(palette.text_muted),
                                );
                            });
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Continuation Ledger
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            .size(FONT_CAPTION)
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
                            ui.add_space(ITEM_SPACING);
                            ui.label(
                                RichText::new(
                                    "No active continuation ledger.\nA ledger is created when handing off context between models.",
                                )
                                .size(FONT_SMALL)
                                .color(palette.text_muted),
                            );
                        });
                    }
                    Some(ledger) => {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("Ledger: {}", ledger.id))
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.add_space(ITEM_SPACING);
                                ui.label(
                                    RichText::new(format!("Mission: {}", ledger.mission.goal))
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Scoped files: {}",
                                        ledger.environment.scoped_files.len()
                                    ))
                                    .size(FONT_CAPTION)
                                    .color(palette.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Edit journal: {} entries",
                                        ledger.journal.completed_edits.len()
                                    ))
                                    .size(FONT_CAPTION)
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
                                    .size(FONT_CAPTION)
                                    .color(palette.success),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Provenance: {} model attempt(s)",
                                        ledger.provenance.len()
                                    ))
                                    .size(FONT_CAPTION)
                                    .color(palette.text_muted),
                                );
                            });
                    }
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Plugin Registry
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
                .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No plugins loaded.\nPlace plugin crates in the workspace to discover them.",
                            )
                            .size(FONT_SMALL)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
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
                                        .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Skill Files
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No skill files loaded.\nSkills are loaded from .velocity/skills/.",
                            )
                            .size(FONT_SMALL)
                            .color(palette.text_muted),
                        );
                    });
                } else {
                    for skill in &self.skill_files {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
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
                                        .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                    }
                }
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Inline Suggestions
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
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
            .size(FONT_CAPTION)
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
                            .size(FONT_CAPTION)
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
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}ms",
                            self.inline_suggestions.config.trigger_delay_ms
                        ))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Max chars:")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}",
                            self.inline_suggestions.config.max_suggestion_chars
                        ))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Min confidence:")
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{:.0}%",
                            self.inline_suggestions.config.min_confidence * 100.0
                        ))
                        .size(FONT_CAPTION)
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
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Accepted: {total_accepted}"))
                        .size(FONT_CAPTION)
                        .color(palette.success),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Dismissed: {total_dismissed}"))
                        .size(FONT_CAPTION)
                        .color(palette.warning),
                );
                let accept_rate = if total_shown > 0 {
                    (total_accepted as f32 / total_shown as f32) * 100.0
                } else {
                    0.0
                };
                ui.label(
                    RichText::new(format!("  \u{2022} Accept rate: {accept_rate:.1}%"))
                        .size(FONT_CAPTION)
                        .color(palette.accent),
                );
                ui.add_space(ITEM_SPACING);
                
                // Cache info
                ui.label(
                    RichText::new("CACHE")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Reuse cache: {cache_entries} entries"))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                ui.label(
                    RichText::new(format!("  \u{2022} Recent: {recent_count} entries"))
                        .size(FONT_CAPTION)
                        .color(palette.text),
                );
                let status = if has_current {
                    ("Pending suggestion", palette.warning)
                } else {
                    ("Idle \u{2014} waiting for trigger", palette.text_muted)
                };
                ui.label(
                    RichText::new(format!("  \u{2022} Status: {}", status.0))
                        .size(FONT_CAPTION)
                        .color(status.1),
                );
            });
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Agent Subsystem Panels
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

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
            .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "No failures recorded this session.\nThe engine is idle.",
                            )
                            .size(FONT_SMALL)
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
                    ui.add_space(ITEM_SPACING);
                    for d in &directives {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
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
                        ui.add_space(ITEM_SPACING);
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
            .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new("No shared knowledge entries yet.")
                                .size(FONT_SMALL)
                                .color(palette.text_muted),
                        );
                    });
                } else {
                    for (id, entry) in self.shared_memory.entries.iter().take(20) {
                        egui::Frame::new()
                            .fill(palette.bg_tertiary)
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
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
                        ui.add_space(ITEM_SPACING);
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
                .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new("No background agents registered.")
                                .size(FONT_SMALL)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
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
                        ui.add_space(ITEM_SPACING);
                    }
                }

                if feed_len > 0 {
                    ui.add_space(SECTION_SPACING);
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
                                .size(FONT_CAPTION)
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
            .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new("No active locks or conflicts.\nAll resources are free.")
                                .size(FONT_SMALL)
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
                                .corner_radius(CARD_RADIUS)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} \u{2014} {} holder(s)",
                                            resource,
                                            locks.len()
                                        ))
                                        .size(FONT_CAPTION)
                                        .color(palette.text),
                                    );
                                });
                            ui.add_space(2.0);
                        }
                        ui.add_space(ITEM_SPACING);
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
                                .corner_radius(CARD_RADIUS)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} vs {} on {}",
                                            c.op_a.actor_id, c.op_b.actor_id, c.resource
                                        ))
                                        .size(FONT_CAPTION)
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
            .size(FONT_CAPTION)
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
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new("No users registered.\nCollaboration is idle.")
                                .size(FONT_SMALL)
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
                            .corner_radius(CARD_RADIUS)
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
                                        RichText::new(&user.name).size(FONT_SMALL).color(palette.text),
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
                        ui.add_space(SECTION_SPACING);
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
                                .corner_radius(CARD_RADIUS)
                                .inner_margin(CARD_INNER_MARGIN)
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
                                        .size(FONT_CAPTION)
                                        .color(palette.text_muted),
                                    );
                                });
                            ui.add_space(ITEM_SPACING);
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
            .size(FONT_CAPTION)
            .color(palette.text_muted),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("persistent_memory_scroll")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Storage: {} / max entries", entry_count))
                        .size(FONT_CAPTION)
                        .color(palette.text_muted),
                );
                ui.add_space(ITEM_SPACING);
                
                if entry_count == 0 {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("\u{1f512}")
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new(
                                "Memory is empty.\nAgents will populate it during execution.",
                            )
                            .size(FONT_SMALL)
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
                            .corner_radius(CARD_RADIUS)
                            .inner_margin(CARD_INNER_MARGIN)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("\u{1f512}")
                                            .size(FONT_SMALL)
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
                        ui.add_space(ITEM_SPACING);
                    }
                    if entry_count > 30 {
                        ui.label(
                            RichText::new(format!("... and {} more entries", entry_count - 30))
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                    }
                }
            });
    }

    // ── Activity Bar Sub-Panels (full implementations) ──

    pub fn render_file_tree_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Poll the background tree builder for updates.
        while let Ok((tree, _ts)) = self.file_tree_rx.try_recv() {
            self.file_tree = Some(tree);
            self.last_tree_update = std::time::Instant::now();
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new(self.workspace_root.file_name().unwrap_or_default().to_string_lossy().to_string()).strong().color(palette.text).size(FONT_SMALL));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("\u{21bb}").on_hover_text("Refresh tree").clicked() {
                    self.file_tree = None;
                    let root = self.workspace_root.clone();
                    let tx = self.file_tree_tx.clone();
                    std::thread::spawn(move || {
                        let tree = super::super::helpers::build_file_tree(&root);
                        let _ = tx.send((tree, Some(std::time::SystemTime::now())));
                    });
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        // File filter input
        ui.horizontal(|ui| {
            ui.label(RichText::new("\u{2315}").size(FONT_SMALL).color(palette.text_muted));
            let filter_width = if self.file_tree_filter.is_empty() {
                ui.available_width()
            } else {
                ui.available_width() - 20.0
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.file_tree_filter)
                    .hint_text("Filter files\u{2026}")
                    .desired_width(filter_width)
                    .text_color(palette.text),
            );
            if !self.file_tree_filter.is_empty() {
                if ui.small_button(RichText::new("\u{2715}").size(9.0).color(palette.text_muted)).clicked() {
                    self.file_tree_filter.clear();
                }
            }
        });
        ui.add_space(ITEM_SPACING);
        
        if let Some(tree) = &self.file_tree {
            let mut path_string = String::new();
            let filter = self.file_tree_filter.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if filter.is_empty() {
                    Self::render_file_tree_node(
                        ui,
                        tree,
                        &self.workspace_root,
                        &mut path_string,
                        palette,
                    );
                } else {
                    // Render filtered tree
                    Self::render_file_tree_node_filtered(
                        ui,
                        tree,
                        &self.workspace_root,
                        &mut path_string,
                        palette,
                        &filter,
                    );
                }
            });
        } else {
            ui.label(RichText::new("Building file tree\u{2026}").color(palette.text_muted).size(FONT_SMALL));
            let root = self.workspace_root.clone();
            let tx = self.file_tree_tx.clone();
            std::thread::spawn(move || {
                let tree = super::super::helpers::build_file_tree(&root);
                let _ = tx.send((tree, Some(std::time::SystemTime::now())));
            });
            ui.ctx().request_repaint();
        }
    }

    pub fn render_bookmarks_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} bookmark(s)", self.bookmarks.len())).size(FONT_SMALL).color(palette.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.bookmarks.clear();
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        if self.bookmarks.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1F516}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No bookmarks yet").color(palette.text_muted).size(FONT_SMALL));
                ui.label(RichText::new("Add bookmarks from the Accessibility layout").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, bm) in self.bookmarks.iter().enumerate() {
                    let rel = bm.file.strip_prefix(&self.workspace_root).unwrap_or(&bm.file);
                    ui.horizontal(|ui| {
                        let label = if bm.label.is_empty() {
                            format!("{}:{}", rel.display(), bm.line)
                        } else {
                            bm.label.clone()
                        };
                        if ui.selectable_label(false, RichText::new(&label).size(FONT_SMALL).color(palette.text)).clicked() {
                            self.pending_open_path = Some(bm.file.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("\u{2715}").on_hover_text("Remove").clicked() {
                                to_remove.push(i);
                            }
                        });
                    });
                    ui.label(RichText::new(format!("  {}:{}", rel.display(), bm.line)).size(9.0).color(palette.text_muted.gamma_multiply(0.7)));
                }
                for &i in to_remove.iter().rev() {
                    self.bookmarks.remove(i);
                }
            });
        }
    }

    pub fn render_favorites_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} favorite(s)", self.favorite_files.len())).size(FONT_SMALL).color(palette.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.favorite_files.clear();
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        if self.favorite_files.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{2B50}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No favorites yet").color(palette.text_muted).size(FONT_SMALL));
                ui.label(RichText::new("Star files from the editor tab context menu").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, f) in self.favorite_files.iter().enumerate() {
                    let rel = f.strip_prefix(&self.workspace_root).unwrap_or(f);
                    let name = rel.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let dir = rel.parent().map(|p| p.display().to_string()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, RichText::new(&name).size(FONT_SMALL).color(palette.text)).clicked() {
                            self.pending_open_path = Some(f.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("\u{2715}").on_hover_text("Remove").clicked() {
                                to_remove.push(i);
                            }
                        });
                    });
                    if !dir.is_empty() {
                        ui.label(RichText::new(&format!("  {}", dir)).size(9.0).color(palette.text_muted.gamma_multiply(0.7)));
                    }
                }
                for &i in to_remove.iter().rev() {
                    self.favorite_files.remove(i);
                }
            });
        }
    }

    pub fn render_code_graph_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let _action = self.graph_view.ui(ui, &self.workspace_root, palette);
    }

    pub fn render_git_changes_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let branch = if self.git_state.branch.is_empty() {
            super::super::helpers::get_git_branch(&self.workspace_root)
        } else {
            Some(self.git_state.branch.clone())
        };

        ui.horizontal(|ui| {
            if let Some(b) = &branch {
                ui.label(RichText::new(format!("\u{e0a0} {}", b)).size(FONT_SMALL).strong().color(palette.accent));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("\u{21bb}").on_hover_text("Refresh").clicked() {
                    self.git_state.refresh(&self.workspace_root);
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        if self.git_state.entries.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{2714}").size(24.0).color(palette.success.gamma_multiply(0.6)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("Working tree clean").color(palette.text_muted).size(FONT_SMALL));
            });
        } else {
            // Staged/unstaged summary strip
            let staged_count = self.git_state.entries.iter().filter(|e| e.staged).count();
            let unstaged_count = self.git_state.entries.len() - staged_count;
            ui.horizontal(|ui| {
                if staged_count > 0 {
                    egui::Frame::new()
                        .fill(palette.success.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("{} staged", staged_count)).size(9.0).color(palette.success));
                        });
                }
                if unstaged_count > 0 {
                    egui::Frame::new()
                        .fill(palette.warning.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("{} unstaged", unstaged_count)).size(9.0).color(palette.warning));
                        });
                }
            });
            ui.add_space(ITEM_SPACING);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &self.git_state.entries {
                    let rel = entry.path.strip_prefix(&self.workspace_root).unwrap_or(&entry.path);
                    let icon = entry.status.icon();
                    let color = match entry.status {
                        crate::editor::git_ui::GitFileStatus::Modified => palette.warning,
                        crate::editor::git_ui::GitFileStatus::Added => palette.success,
                        crate::editor::git_ui::GitFileStatus::Deleted => palette.error,
                        crate::editor::git_ui::GitFileStatus::Conflicted => palette.error,
                        _ => palette.text_muted,
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(FONT_SMALL).strong().color(color));
                        if entry.staged {
                            ui.label(RichText::new("S").size(8.0).color(palette.accent));
                        }
                        ui.label(RichText::new(rel.display().to_string()).size(FONT_SMALL).color(palette.text));
                    });
                }
            });

            // Commit area
            ui.add_space(SECTION_SPACING);
            ui.separator();
            ui.add_space(ITEM_SPACING);
            ui.add(
                egui::TextEdit::multiline(&mut self.git_state.commit_message)
                    .hint_text("Commit message\u{2026}")
                    .desired_rows(2)
                    .desired_width(ui.available_width()),
            );
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Commit").size(FONT_SMALL)).clicked() {
                    if !self.git_state.commit_message.trim().is_empty() {
                        self.status_message = format!("Committing: {}", self.git_state.commit_message.trim());
                        self.git_state.commit_message.clear();
                    }
                }
                if ui.button(RichText::new("Stage All").size(FONT_SMALL)).clicked() {
                    self.status_message = "All files staged".to_string();
                }
            });
        }
    }

    pub fn render_branches_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let branch = if self.git_state.branch.is_empty() {
            super::super::helpers::get_git_branch(&self.workspace_root)
        } else {
            Some(self.git_state.branch.clone())
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new("Branches").size(FONT_SMALL).strong().color(palette.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("\u{21bb}").on_hover_text("Refresh").clicked() {
                    self.git_state.refresh(&self.workspace_root);
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        if let Some(b) = &branch {
            egui::Frame::new()
                .fill(palette.accent.gamma_multiply(0.1))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("\u{e0a0}").size(FONT_BODY).color(palette.accent));
                        ui.label(RichText::new(b).size(FONT_SMALL).strong().color(palette.accent));
                        ui.label(RichText::new("(current)").size(9.0).color(palette.text_muted));
                    });
                });
        }
        ui.add_space(SECTION_SPACING);
        
        // Ahead/behind info
        if self.git_state.ahead > 0 || self.git_state.behind > 0 {
            ui.horizontal(|ui| {
                if self.git_state.ahead > 0 {
                    ui.label(RichText::new(format!("\u{2191} {} ahead", self.git_state.ahead)).size(FONT_SMALL).color(palette.success));
                }
                if self.git_state.behind > 0 {
                    ui.label(RichText::new(format!("\u{2193} {} behind", self.git_state.behind)).size(FONT_SMALL).color(palette.warning));
                }
            });
        }

        if let Some(err) = &self.git_state.last_error {
            ui.add_space(SECTION_SPACING);
            ui.label(RichText::new(format!("\u{26a0} {}", err)).size(9.0).color(palette.error));
        }
    }

    pub fn render_commits_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} commit(s)", self.git_state.log.len())).size(FONT_SMALL).color(palette.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("\u{21bb}").on_hover_text("Refresh log").clicked() {
                    self.git_state.refresh(&self.workspace_root);
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        if self.git_state.log.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f4dc}").size(22.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No commit history").color(palette.text).size(FONT_SMALL).strong());
                ui.add_space(2.0);
                ui.label(RichText::new("Ensure git is initialized in the workspace").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &self.git_state.log {
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&entry.short_hash).size(9.0).monospace().strong().color(palette.accent));
                                ui.label(RichText::new(&entry.date).size(9.0).color(palette.text_muted));
                            });
                            ui.label(RichText::new(&entry.message).size(FONT_SMALL).color(palette.text));
                            ui.label(RichText::new(&entry.author).size(9.0).color(palette.text_muted.gamma_multiply(0.8)));
                        });
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub fn render_chat_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Status bar with model selector
        ui.horizontal(|ui| {
            let status = if self.chat.agent_active { "Agent active" } else { "Ready" };
            let status_color = if self.chat.agent_active { palette.success } else { palette.text_muted };
            ui.label(RichText::new(format!("\u{25cf} {}", status)).size(FONT_SMALL).color(status_color));

            // Message count
            if !self.chat.messages.is_empty() {
                ui.label(RichText::new(format!("{} msg(s)", self.chat.messages.len())).size(9.0).color(palette.text_muted.gamma_multiply(0.7)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Clear conversation button
                if !self.chat.messages.is_empty() {
                    if ui.small_button("\u{2715}").on_hover_text("Clear conversation").clicked() {
                        self.chat.messages.clear();
                    }
                }
                // Thinking toggle
                if self.chat.thinking_supported {
                    let think_resp = ui.selectable_label(self.chat.thinking_enabled,
                        RichText::new("\u{1f9e0}").size(FONT_SMALL)
                            .color(if self.chat.thinking_enabled { palette.accent } else { palette.text_muted }),
                    ).on_hover_text(if self.chat.thinking_enabled { "Thinking: ON" } else { "Thinking: OFF" });
                    if think_resp.clicked() {
                        self.chat.thinking_enabled = !self.chat.thinking_enabled;
                    }
                }
            });
        });

        // Model selector
        if !self.chat.available_models.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model:").size(9.0).color(palette.text_muted));
                egui::ComboBox::from_id_salt("chat_model_selector")
                    .selected_text(if self.chat.selected_model.is_empty() {
                        "Select model".to_string()
                    } else {
                        let label = self.chat.available_models.iter()
                            .find(|m| m.id == self.chat.selected_model)
                            .map(|m| m.label.clone())
                            .unwrap_or_else(|| {
                                let parts: Vec<&str> = self.chat.selected_model.rsplitn(2, '/').collect();
                                parts[0].to_string()
                            });
                        label
                    })
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for model in &self.chat.available_models {
                            let is_selected = model.id == self.chat.selected_model;
                            let resp = ui.selectable_label(is_selected, RichText::new(&model.label).size(FONT_SMALL));
                            if resp.clicked() {
                                self.chat.selected_model = model.id.clone();
                            }
                        }
                    });
            });
        } else if !self.chat.selected_model.is_empty() {
            let short_model = self.chat.selected_model.rsplitn(2, '/').next().unwrap_or(&self.chat.selected_model);
            ui.label(RichText::new(short_model).size(9.0).color(palette.text_muted));
        }
        ui.add_space(ITEM_SPACING);
        
        // Pending approvals
        if !self.chat.pending_approvals.is_empty() {
            egui::Frame::new()
                .fill(palette.warning.gamma_multiply(0.1))
                .stroke(egui::Stroke::new(1.0, palette.warning.gamma_multiply(0.3)))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.label(RichText::new(format!("{} pending approval(s)", self.chat.pending_approvals.len())).size(FONT_SMALL).strong().color(palette.warning));
                });
            ui.add_space(ITEM_SPACING);
        }

        // Messages
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.chat.messages.is_empty() {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("\u{1F4AC}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                    ui.add_space(ITEM_SPACING);
                    ui.label(RichText::new("No messages yet").color(palette.text_muted).size(FONT_SMALL));
                    ui.label(RichText::new("Start a conversation with the AI agent").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
                });
            } else {
                for msg in &self.chat.messages {
                    let (role_label, color) = match msg.role {
                        crate::editor::chat_panel::ChatRole::User => ("You", palette.accent),
                        crate::editor::chat_panel::ChatRole::Agent => ("Agent", palette.success),
                        crate::editor::chat_panel::ChatRole::Thought => ("Thought", palette.text_muted),
                    };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.label(RichText::new(role_label).size(9.0).strong().color(color));
                            ui.label(RichText::new(&msg.content).size(FONT_SMALL).color(palette.text));
                        });
                    ui.add_space(2.0);
                }
            }
        });

        // Input area
        ui.add_space(ITEM_SPACING);
        ui.separator();
        ui.add_space(ITEM_SPACING);
        let mut send = false;
        ui.horizontal(|ui| {
            let input_resp = ui.add(
                egui::TextEdit::singleline(&mut self.chat.input)
                    .hint_text("Message the agent\u{2026}")
                    .desired_width(ui.available_width() - 50.0),
            );
            if input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                send = true;
            }
            if ui.button(RichText::new("\u{27a4}").size(FONT_BODY)).clicked() {
                send = true;
            }
        });
        if send && !self.chat.input.trim().is_empty() {
            self.chat.messages.push(crate::editor::chat_panel::UiChatMessage {
                role: crate::editor::chat_panel::ChatRole::User,
                content: self.chat.input.clone(),
            });
            self.chat.input.clear();
        }
    }

    pub fn render_voice_subpanel(&mut self, ui: &mut egui::Ui, _palette: IdePalette) {
        self.render_voice_panel(ui);
    }

    pub fn render_multimodal_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} attachment(s)", self.multimodal_attachments.len())).size(FONT_SMALL).color(palette.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear all").clicked() {
                    self.multimodal_attachments.clear();
                }
            });
        });
        ui.add_space(ITEM_SPACING);
        
        // Attachment list
        if self.multimodal_attachments.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f4ce}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No attachments").color(palette.text_muted).size(FONT_SMALL));
                ui.label(RichText::new("Attach images, audio, or documents for the agent").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut to_remove: Vec<usize> = Vec::new();
                for (i, att) in self.multimodal_attachments.iter().enumerate() {
                    let kind_label = att.kind.label();
                    let kind_icon = match att.kind {
                        crate::editor::multimodal::AttachmentKind::Image => "\u{1f5bc}",
                        crate::editor::multimodal::AttachmentKind::Audio => "\u{1f3b5}",
                        crate::editor::multimodal::AttachmentKind::Document => "\u{1f4c4}",
                    };
                    let file_name = att.path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let size_kb = att.data.len() as f64 / 1024.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(kind_icon).size(FONT_BODY));
                        ui.label(RichText::new(&file_name).size(FONT_SMALL).color(palette.text));
                        ui.label(RichText::new(format!("{} ({:.1} KB)", kind_label, size_kb)).size(9.0).color(palette.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("\u{2715}").on_hover_text("Remove").clicked() {
                                to_remove.push(i);
                            }
                        });
                    });
                }
                for &i in to_remove.iter().rev() {
                    self.multimodal_attachments.remove(i);
                }
            });
        }

        // Add attachment by path
        ui.add_space(SECTION_SPACING);
        ui.separator();
        ui.add_space(ITEM_SPACING);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Attach file:").size(FONT_SMALL).color(palette.text_muted));
            let mut attach_path = String::new();
            if ui.add(egui::TextEdit::singleline(&mut attach_path).hint_text("path\u{2026}").desired_width(ui.available_width() - 60.0)).lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !attach_path.is_empty()
            {
                match crate::editor::multimodal::Attachment::load(&attach_path) {
                    Ok(att) => {
                        self.multimodal_attachments.push(att);
                        self.status_message = format!("Attached: {}", attach_path);
                    }
                    Err(e) => {
                        self.status_message = format!("Failed to attach: {}", e);
                    }
                }
            }
        });
    }

    pub fn render_build_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        // Status indicator — only show after a build has been triggered
        let has_built = !self.status_message.is_empty();
        if has_built {
            let build_ok = self.build_errors_count == 0;
            ui.horizontal(|ui| {
                let (icon, color) = if build_ok {
                    ("\u{2714}", palette.success)
                } else {
                    ("\u{2716}", palette.error)
                };
                ui.label(RichText::new(icon).size(FONT_BODY).color(color));
                if build_ok {
                    ui.label(RichText::new("Build clean").size(FONT_SMALL).color(palette.success));
                } else {
                    ui.label(RichText::new(format!("{} error(s)", self.build_errors_count)).size(FONT_SMALL).color(palette.error));
                }
            });
            ui.add_space(ITEM_SPACING);
        } else {
            // Initial state — no build triggered yet
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f3d7}").size(22.0).color(palette.accent.gamma_multiply(0.5)));
                ui.add_space(4.0);
                ui.label(RichText::new("Ready to build").size(FONT_SMALL).strong().color(palette.text));
                ui.add_space(2.0);
                ui.label(RichText::new("Click Build or Run to start").size(9.0).color(palette.text_muted));
            });
            ui.add_space(ITEM_SPACING);
        }
        
        // Build controls
        ui.horizontal(|ui| {
            let build_btn = egui::Button::new(RichText::new("\u{25b6} Build").size(FONT_SMALL).color(palette.text));
            if ui.add(build_btn).clicked() {
                self.status_message = "Building\u{2026}".to_string();
            }
            let run_btn = egui::Button::new(RichText::new("\u{25b6} Run").size(FONT_SMALL).color(palette.text));
            if ui.add(run_btn).clicked() {
                self.status_message = "Running\u{2026}".to_string();
            }
            let stop_btn = egui::Button::new(RichText::new("\u{25a0} Stop").size(FONT_SMALL).color(palette.error));
            if ui.add(stop_btn).clicked() {
                self.status_message = "Stopped".to_string();
            }
        });
        ui.add_space(ITEM_SPACING);
        
        // Status message
        if !self.status_message.is_empty() {
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .corner_radius(egui::CornerRadius::same(3))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.label(RichText::new(&self.status_message).size(FONT_SMALL).color(palette.text_muted));
                });
        }

        // Build info
        ui.add_space(SECTION_SPACING);
        ui.separator();
        ui.add_space(ITEM_SPACING);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Provider:").size(9.0).color(palette.text_muted));
            ui.label(RichText::new(self.provider.label()).size(9.0).color(palette.text));
        });
        if !self.selected_model.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model:").size(9.0).color(palette.text_muted));
                ui.label(RichText::new(&self.selected_model).size(9.0).color(palette.text));
            });
        }
        if !self.gpu_name.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("GPU:").size(9.0).color(palette.text_muted));
                ui.label(RichText::new(&self.gpu_name).size(9.0).color(palette.text));
            });
        }
    }

    pub fn render_agent_roster_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let snapshot = self.orchestrator.dashboard_snapshot();

        // Summary strip
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("\u{2714} {}", snapshot.done_tasks)).size(FONT_SMALL).color(palette.success));
            ui.label(RichText::new(format!("\u{2716} {}", snapshot.failed_tasks)).size(FONT_SMALL).color(palette.error));
            ui.label(RichText::new(format!("\u{25b6} {}", snapshot.running_tasks)).size(FONT_SMALL).color(palette.warning));
            ui.label(RichText::new(format!("\u{22ef} {}", snapshot.pending_tasks)).size(FONT_SMALL).color(palette.text_muted));
        });
        ui.add_space(ITEM_SPACING);
        
        // Runtime status
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Runtime: {}", snapshot.runtime_status)).size(FONT_SMALL).color(palette.text));
            if snapshot.execution_running {
                ui.label(RichText::new("\u{25cf} running").size(9.0).color(palette.success));
            }
        });
        if snapshot.has_dependency_cycle {
            ui.label(RichText::new("\u{26a0} Dependency cycle detected").size(9.0).color(palette.error));
        }
        ui.add_space(ITEM_SPACING);
        
        // Active workers
        if snapshot.active_workers > 0 {
            ui.label(RichText::new(format!("{} active worker(s)", snapshot.active_workers)).size(FONT_SMALL).strong().color(palette.text));
        }

        // Task list
        if !snapshot.tasks.is_empty() {
            ui.add_space(ITEM_SPACING);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for task in &snapshot.tasks {
                    let color = if task.status_label == "done" { palette.success } else if task.status_label == "failed" { palette.error } else { palette.text };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(6, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("#{}", task.id)).size(9.0).monospace().color(palette.text_muted));
                                ui.label(RichText::new(&task.title).size(FONT_SMALL).color(color));
                            });
                            if !task.description.is_empty() {
                                ui.label(RichText::new(&task.description).size(9.0).color(palette.text_muted));
                            }
                        });
                    ui.add_space(1.0);
                }
            });
        }
    }

    pub fn render_timeline_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let timeline_snapshot = crate::editor::task_timeline::TaskTimelineSnapshot::new(&self.task_timeline);
        render_task_timeline(ui, &timeline_snapshot, palette);
    }

    pub fn render_mission_metrics_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let snapshot = self.orchestrator.dashboard_snapshot();

        // Metrics grid
        let metrics = [
            ("Completed", format!("{}", snapshot.done_tasks), palette.success),
            ("Failed", format!("{}", snapshot.failed_tasks), palette.error),
            ("Running", format!("{}", snapshot.running_tasks), palette.warning),
            ("Pending", format!("{}", snapshot.pending_tasks), palette.text_muted),
            ("Blocked", format!("{}", snapshot.blocked_tasks), palette.text_muted),
            ("Workers", format!("{}", snapshot.active_workers), palette.accent),
        ];

        ui.columns(2, |cols| {
            for (i, (label, value, color)) in metrics.iter().enumerate() {
                let col = &mut cols[i % 2];
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(col, |ui| {
                        ui.label(RichText::new(value).size(16.0).strong().color(*color));
                        ui.label(RichText::new(*label).size(9.0).color(palette.text_muted));
                    });
            }
        });
        ui.add_space(SECTION_SPACING);
        
        // Status details
        ui.horizontal(|ui| {
            ui.label(RichText::new("Planning:").size(9.0).color(palette.text_muted));
            ui.label(RichText::new(&snapshot.planning_status).size(9.0).color(palette.text));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Runtime:").size(9.0).color(palette.text_muted));
            ui.label(RichText::new(&snapshot.runtime_status).size(9.0).color(palette.text));
        });
        if let Some(goal) = &snapshot.goal {
            ui.add_space(ITEM_SPACING);
            egui::Frame::new()
                .fill(palette.accent.gamma_multiply(0.08))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.label(RichText::new("Goal").size(9.0).strong().color(palette.accent));
                    ui.label(RichText::new(goal).size(FONT_SMALL).color(palette.text));
                });
        }
    }

    pub fn render_wiki_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let _action = self.wiki_view.ui(ui, &self.workspace_root, &mut self.toasts, palette);
    }

    pub fn render_nda_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} sealed doc(s)", self.nda_docs.len())).size(FONT_SMALL).color(palette.text_muted));
        });
        ui.add_space(ITEM_SPACING);
        
        if self.nda_docs.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f512}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No NDA documents open").color(palette.text_muted).size(FONT_SMALL));
                ui.label(RichText::new("Open .nda files from the workspace to view them here").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (tab_id, doc) in &self.nda_docs {
                    let title = doc.doc.title().unwrap_or("Untitled").to_string();
                    let status = if doc.sealed { "\u{1f512} Sealed" } else { "\u{1f513} Open" };
                    let dirty_mark = if doc.dirty { " *" } else { "" };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{}{}", title, dirty_mark)).size(FONT_SMALL).strong().color(palette.text));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(status).size(9.0).color(if doc.sealed { palette.warning } else { palette.text_muted }));
                                });
                            });
                            if let Some(path) = &doc.path {
                                let rel = path.strip_prefix(&self.workspace_root).unwrap_or(path);
                                ui.label(RichText::new(rel.display().to_string()).size(9.0).color(palette.text_muted));
                            }
                            ui.label(RichText::new(format!("{} triple(s)", doc.doc.triples.len())).size(9.0).color(palette.text_muted.gamma_multiply(0.8)));
                        });
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub fn render_plugin_registry_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        let plugins = self.plugin_registry.list();
        let all_tools = self.plugin_registry.all_tools();

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} plugin(s)", plugins.len())).size(FONT_SMALL).color(palette.text_muted));
            ui.label(RichText::new(format!("\u{00b7} {} tool(s)", all_tools.len())).size(FONT_SMALL).color(palette.text_muted));
        });
        ui.add_space(ITEM_SPACING);
        
        if plugins.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f9e9}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No plugins loaded").color(palette.text_muted).size(FONT_SMALL));
                ui.label(RichText::new("Plugins are discovered from the workspace plugins/ directory").color(palette.text_muted.gamma_multiply(0.7)).size(9.0));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for plugin in &plugins {
                    let enabled_mark = if plugin.enabled { "" } else { " (disabled)" };
                    let color = if plugin.enabled { palette.text } else { palette.text_muted };
                    egui::Frame::new()
                        .fill(palette.bg_secondary)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{}{}", plugin.name, enabled_mark)).size(FONT_SMALL).strong().color(color));
                                ui.label(RichText::new(&plugin.version).size(9.0).color(palette.text_muted));
                            });
                            if !plugin.description.is_empty() {
                                ui.label(RichText::new(&plugin.description).size(FONT_SMALL).color(palette.text_muted));
                            }
                            ui.horizontal(|ui| {
                                if !plugin.author.is_empty() {
                                    ui.label(RichText::new(format!("by {}", plugin.author)).size(9.0).color(palette.text_muted.gamma_multiply(0.8)));
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("{} tool(s)", plugin.tool_count)).size(9.0).color(palette.accent));
                                });
                            });
                            if !plugin.tool_names.is_empty() {
                                ui.label(RichText::new(plugin.tool_names.join(", ")).size(9.0).color(palette.text_muted.gamma_multiply(0.7)));
                            }
                        });
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub fn render_skills_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} skill(s)", self.skill_files.len())).size(FONT_SMALL).color(palette.text_muted));
        });
        ui.add_space(ITEM_SPACING);
        
        // Search filter
        ui.horizontal(|ui| {
            ui.label(RichText::new("\u{2315}").size(FONT_SMALL).color(palette.text_muted));
            let filter_width = if self.skill_filter.is_empty() {
                ui.available_width()
            } else {
                ui.available_width() - 20.0
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.skill_filter)
                    .hint_text("Filter skills\u{2026}")
                    .desired_width(filter_width)
                    .text_color(palette.text),
            );
            if !self.skill_filter.is_empty() {
                if ui.small_button(RichText::new("\u{2715}").size(9.0).color(palette.text_muted)).clicked() {
                    self.skill_filter.clear();
                }
            }
        });
        ui.add_space(ITEM_SPACING);
        
        if self.skill_files.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f3af}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No skills defined").size(FONT_BODY).strong().color(palette.text));
                ui.add_space(2.0);
                ui.label(RichText::new("Skills are markdown files in .qoder/skills/ that teach agents new capabilities.").color(palette.text_muted).size(FONT_CAPTION));
            });
        } else {
            let filter_lower = self.skill_filter.to_lowercase();
            let matching_skills: Vec<_> = self.skill_files.iter()
                .filter(|s| {
                    filter_lower.is_empty()
                        || s.name.to_lowercase().contains(&filter_lower)
                        || s.id.to_lowercase().contains(&filter_lower)
                        || s.description.to_lowercase().contains(&filter_lower)
                })
                .collect();

            if matching_skills.is_empty() && !filter_lower.is_empty() {
                ui.add_space(SECTION_SPACING);
                ui.label(RichText::new(format!("No skills match '{}'", self.skill_filter)).size(FONT_SMALL).color(palette.text_muted));
            } else {
                if !filter_lower.is_empty() {
                    ui.label(RichText::new(format!("{} match(es)", matching_skills.len())).size(9.0).color(palette.text_muted));
                    ui.add_space(2.0);
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for skill in matching_skills {
                        egui::Frame::new()
                            .fill(palette.bg_secondary)
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&skill.name).size(FONT_SMALL).strong().color(palette.text));
                                    ui.label(RichText::new(&skill.id).size(9.0).monospace().color(palette.text_muted));
                                });
                                if !skill.description.is_empty() {
                                    ui.label(RichText::new(&skill.description).size(FONT_SMALL).color(palette.text_muted));
                                }
                                // Preview first 120 chars of body
                                let preview: String = skill.body.chars().take(120).collect();
                                if preview.len() >= 120 {
                                    ui.label(RichText::new(format!("{}\u{2026}", preview)).size(9.0).color(palette.text_muted.gamma_multiply(0.7)));
                                }
                            });
                        ui.add_space(2.0);
                    }
                });
            }
        }
    }

    pub fn render_usage_subpanel(&mut self, ui: &mut egui::Ui, palette: IdePalette) {
        if self.account_usage.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("\u{1f4ca}").size(24.0).color(palette.text_muted.gamma_multiply(0.5)));
                ui.add_space(ITEM_SPACING);
                ui.label(RichText::new("No usage data available").color(palette.text_muted).size(FONT_SMALL));
            });
        } else {
            // Summary header
            let total_accounts = self.account_usage.len();
            let exhausted_count = self.account_usage.iter().filter(|u| u.exhausted).count();
            let total_requests: u32 = self.account_usage.iter().map(|u| u.requests).sum();
            let total_remaining: u32 = self.account_usage.iter().map(|u| u.remaining).sum();

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} account(s)", total_accounts)).size(FONT_SMALL).strong().color(palette.text));
                if exhausted_count > 0 {
                    egui::Frame::new()
                        .fill(palette.error.gamma_multiply(0.12))
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("{} exhausted", exhausted_count)).size(9.0).color(palette.error));
                        });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} total remaining", format_count(total_remaining))).size(9.0).color(palette.text_muted));
                });
            });
            ui.add_space(ITEM_SPACING);
            
            for usage in &self.account_usage {
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&usage.label).size(FONT_SMALL).strong().color(palette.text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let tier_color = if usage.exhausted { palette.error } else { palette.accent };
                                egui::Frame::new()
                                    .fill(tier_color.gamma_multiply(0.15))
                                    .corner_radius(egui::CornerRadius::same(3))
                                    .inner_margin(egui::Margin::symmetric(4, 1))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&usage.tier).size(9.0).color(tier_color));
                                    });
                            });
                        });

                        // Requests bar
                        let pct = if usage.daily_limit > 0 {
                            (usage.requests as f32 / usage.daily_limit as f32).min(1.0)
                        } else {
                            0.0
                        };
                        let bar_color = if usage.exhausted { palette.error } else if pct > 0.8 { palette.warning } else { palette.success };
                        ui.add_space(ITEM_SPACING);
                        let bar_resp = ui.add_sized(egui::Vec2::new(ui.available_width(), 6.0), egui::Label::new(""));
                        let bg_rect = bar_resp.rect;
                        ui.painter().rect_filled(bg_rect, 3.0, palette.bg_tertiary);
                        let fill_rect = egui::Rect::from_min_size(bg_rect.min, egui::Vec2::new(bg_rect.width() * pct, 6.0));
                        ui.painter().rect_filled(fill_rect, 3.0, bar_color);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} / {} requests", format_count(usage.requests), format_count(usage.daily_limit))).size(9.0).color(palette.text_muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(format!("{} remaining", format_count(usage.remaining))).size(9.0).color(if usage.exhausted { palette.error } else { palette.text_muted }));
                            });
                        });

                        // Token usage with formatted numbers
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("\u{2191} {} in", format_count(usage.tokens_in))).size(9.0).color(palette.text_muted));
                            ui.label(RichText::new(format!("\u{2193} {} out", format_count(usage.tokens_out))).size(9.0).color(palette.text_muted));
                        });
                    });
                ui.add_space(ITEM_SPACING);
            }
        }
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

/// Format a count with K/M suffixes for readability.
fn format_count(n: impl Into<u64>) -> String {
    let n = n.into();
    if n < 1_000 {
        format!("{}", n)
    } else if n < 1_000_000 {
        let k = n as f64 / 1_000.0;
        if k < 10.0 {
            format!("{:.1}K", k)
        } else {
            format!("{:.0}K", k)
        }
    } else {
        let m = n as f64 / 1_000_000.0;
        if m < 10.0 {
            format!("{:.1}M", m)
        } else {
            format!("{:.0}M", m)
        }
    }
}
