//! Tier-3 panels: orchestration, background agents, continuation ledger, conflict resolver, collaboration.
//!
//! Extracted verbatim from `tier3_panels.rs` (no logic changes).

use super::struct_def::VelocityApp;
use super::tier3_common::primary_button;
use crate::editor::theme::{
    CARD_INNER_MARGIN, CARD_RADIUS, FONT_CAPTION, FONT_SMALL, ITEM_SPACING, SECTION_SPACING,
};
use eframe::egui;
use egui::RichText;

impl VelocityApp {
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
        let mut go_orchestrator = false;
        egui::ScrollArea::vertical()
            .id_salt("orchestration_activity_scroll")
            .max_height(250.0)
            .show(ui, |ui| {
                if self.live_orchestration.activity_feed.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(egui_phosphor::regular::PULSE)
                                .size(22.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No activity yet")
                                .size(FONT_SMALL)
                                .strong()
                                .color(palette.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Events stream in while agents run orchestrated tasks.")
                                .size(FONT_CAPTION)
                                .color(palette.text_muted),
                        );
                        ui.add_space(ITEM_SPACING);
                        if primary_button(
                            ui,
                            palette,
                            format!("{} Open Orchestrator", egui_phosphor::regular::ROBOT),
                        )
                        .clicked()
                        {
                            go_orchestrator = true;
                        }
                    });
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
        if go_orchestrator {
            self.focus_orchestrator_tab();
        }
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

        let mut go_orchestrator = false;
        egui::ScrollArea::vertical()
            .id_salt("continuation_ledger_scroll")
            .show(ui, |ui| match &self.continuation_ledger {
                None => {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(egui_phosphor::regular::CLIPBOARD)
                                .size(24.0)
                                .color(palette.text_muted.gamma_multiply(0.5)),
                        );
                        ui.add_space(ITEM_SPACING);
                        ui.label(
                            RichText::new("No active continuation ledger")
                                .size(FONT_SMALL)
                                .strong()
                                .color(palette.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(
                                "A ledger is created when handing off context between models.",
                            )
                            .size(FONT_CAPTION)
                            .color(palette.text_muted),
                        );
                        ui.add_space(ITEM_SPACING);
                        if primary_button(
                            ui,
                            palette,
                            format!("{} Open Orchestrator", egui_phosphor::regular::ROBOT),
                        )
                        .clicked()
                        {
                            go_orchestrator = true;
                        }
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
            });
        if go_orchestrator {
            self.focus_orchestrator_tab();
        }
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
                            RichText::new(egui_phosphor::regular::ROBOT)
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
                                    ui.label(
                                        RichText::new(egui_phosphor::regular::CIRCLE)
                                            .color(status_color),
                                    );
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
                            RichText::new(egui_phosphor::regular::CHECK)
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
                            RichText::new(egui_phosphor::regular::USERS)
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
                                        RichText::new(&user.name)
                                            .size(FONT_SMALL)
                                            .color(palette.text),
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
                                        ui.label(
                                            RichText::new(egui_phosphor::regular::CIRCLE)
                                                .color(status_color),
                                        );
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
}
