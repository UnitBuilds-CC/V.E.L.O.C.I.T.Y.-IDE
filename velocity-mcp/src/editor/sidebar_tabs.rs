//! Sidebar Tabs - Mode-specific left sidebar tab definitions and renderers.
//!
//! Each mode declares its own set of sidebar tabs. The left sidebar renders
//! whichever set the active `ModeConfig` returns.

use crate::editor::theme::IdePalette;
use eframe::egui;

// ═══════════════════════════════════════════════════════════════════════════
// SidebarTab enum
// ═══════════════════════════════════════════════════════════════════════════

/// All possible sidebar tabs across all modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SidebarTab {
    // Shared / Coder
    Files,
    Outline,
    Git,
    Search,

    // Operator
    Flows,
    Targets,
    Recordings,
    Logs,

    // Mission Control
    Agents,
    Queue,
    Timeline,
    Metrics,

    // Accessibility
    Favorites,
    Bookmarks,
    AccessibilityAudit,

    // Cross-mode utility
    Browse,
}

impl SidebarTab {
    /// Human-readable label shown in the sidebar tab strip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Outline => "Outline",
            Self::Git => "Git",
            Self::Search => "Search",
            Self::Flows => "Flows",
            Self::Targets => "Targets",
            Self::Recordings => "Recordings",
            Self::Logs => "Logs",
            Self::Agents => "Agents",
            Self::Queue => "Queue",
            Self::Timeline => "Timeline",
            Self::Metrics => "Metrics",
            Self::Favorites => "Favorites",
            Self::Bookmarks => "Bookmarks",
            Self::AccessibilityAudit => "Audit",
            Self::Browse => "Browse",
        }
    }

    /// Short glyph icon for the sidebar tab.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Files => "\u{25eb}",
            Self::Outline => "\u{2261}",
            Self::Git => "\u{2442}",
            Self::Search => "\u{2295}",
            Self::Flows => "\u{29c9}",
            Self::Targets => "\u{25ce}",
            Self::Recordings => "\u{25cf}",
            Self::Logs => "\u{2263}",
            Self::Agents => "\u{2299}",
            Self::Queue => "\u{229e}",
            Self::Timeline => "\u{23e4}",
            Self::Metrics => "\u{22bf}",
            Self::Favorites => "\u{2605}",
            Self::Bookmarks => "\u{229b}",
            Self::AccessibilityAudit => "\u{267f}",
            Self::Browse => "\u{2295}",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Sidebar Tab Strip Renderer
// ═══════════════════════════════════════════════════════════════════════════

/// Render the tab strip for the left sidebar, returning the index of the
/// selected tab. `active` is the current selection index.
///
/// Uses icon-only buttons with tooltips to save horizontal space — the
/// sidebar is narrow (180–420 px) so text labels would wrap or truncate.
pub fn render_sidebar_tab_strip(
    ui: &mut egui::Ui,
    tabs: &[SidebarTab],
    active: usize,
    palette: IdePalette,
) -> usize {
    let mut selected = active.min(tabs.len().saturating_sub(1));

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, tab) in tabs.iter().enumerate() {
            let is_active = i == selected;
            let icon_text = egui::RichText::new(tab.icon())
                .size(14.0)
                .color(if is_active {
                    palette.accent
                } else {
                    palette.text_muted
                });

            let resp = ui.selectable_label(is_active, icon_text);
            let tooltip = format!("{}  {}", tab.icon(), tab.label());
            let resp = if is_active {
                resp.on_hover_text(format!("{tooltip}  (active)"))
            } else {
                resp.on_hover_text(&tooltip)
            };
            if resp.clicked() {
                selected = i;
            }
        }
    });

    selected
}

// ═══════════════════════════════════════════════════════════════════════════
// Sidebar Tab Data Context (real data passed from VelocityApp)
// ═══════════════════════════════════════════════════════════════════════════

use std::path::{Path, PathBuf};

/// Git state summary for the Git sidebar tab.
pub struct GitTabData<'a> {
    pub branch: Option<&'a str>,
    pub changed_files: &'a [PathBuf],
    pub workspace_root: &'a Path,
    /// Full status entries for staging UI (if available).
    pub status_entries: &'a [crate::editor::git_ui::GitStatusEntry],
}

/// Action returned by the git tab UI.
pub enum GitTabAction {
    /// Open a file in the editor.
    OpenFile(PathBuf),
    /// Stage a specific file.
    StageFile(PathBuf),
    /// Unstage a specific file.
    UnstageFile(PathBuf),
    /// Stage all files.
    StageAll,
    /// Unstage all files.
    UnstageAll,
    /// No action.
    None,
}

/// Flow entry for the Flows sidebar tab.
pub struct FlowEntry {
    pub name: String,
    pub status: &'static str,
    pub step_count: usize,
}

/// Target entry for the Targets sidebar tab.
pub struct TargetEntry {
    pub url: String,
    pub label: String,
    pub last_visited: Option<String>,
}

/// Agent status entry for the Agents sidebar tab.
pub struct AgentEntry {
    pub id: u64,
    pub label: String,
    pub status: &'static str,
    pub tasks_done: usize,
}

/// Task queue entry for the Queue sidebar tab.
pub struct QueueEntry {
    pub id: u64,
    pub title: String,
    pub status: &'static str,
}

/// Metrics snapshot for the Metrics sidebar tab.
pub struct MetricsSnapshot {
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub tasks_pending: usize,
    pub avg_duration_ms: u32,
    pub total_tokens: u64,
}

/// Bookmark entry for the Bookmarks sidebar tab.
#[derive(Clone, Debug)]
pub struct BookmarkEntry {
    pub file: PathBuf,
    pub line: usize,
    pub label: String,
}

/// WCAG audit finding for the AccessibilityAudit tab.
pub struct AuditFinding {
    pub severity: &'static str,
    pub rule: String,
    pub element: String,
    pub suggestion: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Sidebar Tab Content Renderers (data-driven)
// ═══════════════════════════════════════════════════════════════════════════

/// Render the Git tab with real branch + changed files data.
pub fn render_git_content(
    ui: &mut egui::Ui,
    data: &GitTabData,
    palette: IdePalette,
) -> GitTabAction {
    let mut action = GitTabAction::None;
    ui.label(
        egui::RichText::new("\u{2442} Source Control")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);

    // Branch display
    let branch_label = data.branch.unwrap_or("(detached)");
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Branch:")
                .size(10.0)
                .color(palette.text_muted),
        );
        ui.label(
            egui::RichText::new(branch_label)
                .size(10.0)
                .strong()
                .color(palette.accent),
        );
    });
    ui.add_space(6.0);

    // Use full status entries if available, otherwise fall back to flat file list
    let has_entries = !data.status_entries.is_empty();
    let has_files = !data.changed_files.is_empty();

    if !has_entries && !has_files {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{2714}")
                    .size(18.0)
                    .color(palette.success),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Working tree clean")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Changes will appear here when you edit files.")
                    .size(8.0)
                    .color(palette.text_muted.gamma_multiply(0.7)),
            );
        });
    } else if has_entries {
        // Full staging UI with staged/unstaged sections
        let staged: Vec<_> = data.status_entries.iter().filter(|e| e.staged).collect();
        let unstaged: Vec<_> = data.status_entries.iter().filter(|e| !e.staged).collect();

        // Staged changes section
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("Staged ({})", staged.len()))
                    .size(10.0)
                    .strong()
                    .color(palette.success),
            );
            if !staged.is_empty() {
                if ui.small_button("Unstage All").clicked() {
                    action = GitTabAction::UnstageAll;
                }
            }
        });
        if staged.is_empty() {
            ui.label(
                egui::RichText::new("  No staged changes")
                    .size(8.0)
                    .color(palette.text_muted),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    for entry in &staged {
                        let name = entry.path.to_string_lossy();
                        let status_color = match entry.status {
                            crate::editor::git_ui::GitFileStatus::Modified => palette.warning,
                            crate::editor::git_ui::GitFileStatus::Added => palette.success,
                            crate::editor::git_ui::GitFileStatus::Deleted => palette.error,
                            _ => palette.text_muted,
                        };
                        ui.horizontal(|ui| {
                            if ui.small_button("\u{2212}").clicked() {
                                action = GitTabAction::UnstageFile(entry.path.clone());
                            }
                            ui.label(
                                egui::RichText::new(entry.status.icon())
                                    .monospace()
                                    .size(9.0)
                                    .strong()
                                    .color(status_color),
                            );
                            if ui
                                .link(
                                    egui::RichText::new(name.as_ref())
                                        .size(9.0)
                                        .color(palette.text),
                                )
                                .clicked()
                            {
                                action = GitTabAction::OpenFile(entry.path.clone());
                            }
                        });
                    }
                });
        }

        ui.add_space(6.0);

        // Unstaged changes section
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("Changes ({})", unstaged.len()))
                    .size(10.0)
                    .strong()
                    .color(palette.warning),
            );
            if !unstaged.is_empty() {
                if ui.small_button("Stage All").clicked() {
                    action = GitTabAction::StageAll;
                }
            }
        });
        if unstaged.is_empty() {
            ui.label(
                egui::RichText::new("  No unstaged changes")
                    .size(8.0)
                    .color(palette.text_muted),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for entry in &unstaged {
                        let name = entry.path.to_string_lossy();
                        let status_color = match entry.status {
                            crate::editor::git_ui::GitFileStatus::Modified => palette.warning,
                            crate::editor::git_ui::GitFileStatus::Added => palette.success,
                            crate::editor::git_ui::GitFileStatus::Deleted => palette.error,
                            crate::editor::git_ui::GitFileStatus::Untracked => palette.text_muted,
                            _ => palette.text_muted,
                        };
                        ui.horizontal(|ui| {
                            if ui.small_button("+").clicked() {
                                action = GitTabAction::StageFile(entry.path.clone());
                            }
                            ui.label(
                                egui::RichText::new(entry.status.icon())
                                    .monospace()
                                    .size(9.0)
                                    .strong()
                                    .color(status_color),
                            );
                            if ui
                                .link(
                                    egui::RichText::new(name.as_ref())
                                        .size(9.0)
                                        .color(palette.text),
                                )
                                .clicked()
                            {
                                action = GitTabAction::OpenFile(entry.path.clone());
                            }
                        });
                    }
                });
        }
    } else {
        // Fallback: flat file list (no staging info)
        ui.label(
            egui::RichText::new(format!("Changes ({})", data.changed_files.len()))
                .size(10.0)
                .strong()
                .color(palette.warning),
        );
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for file in data.changed_files {
                    let name = file
                        .strip_prefix(data.workspace_root)
                        .unwrap_or(file)
                        .to_string_lossy();
                    if ui
                        .link(
                            egui::RichText::new(format!("  M  {}", name))
                                .size(9.0)
                                .color(palette.text),
                        )
                        .clicked()
                    {
                        action = GitTabAction::OpenFile(file.clone());
                    }
                }
            });
    }
    action
}

/// Render the Flows tab with real automation flow data.
pub fn render_flows_content(ui: &mut egui::Ui, flows: &[FlowEntry], palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{29c9} Automation Flows")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if flows.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{29c9}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No flows recorded yet.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Press the \u{25cf} Record button in the toolbar to capture one.",
                )
                .size(8.0)
                .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(250.0)
            .show(ui, |ui| {
                for flow in flows {
                    ui.horizontal(|ui| {
                        let status_color = match flow.status {
                            "running" => palette.success,
                            "failed" => palette.error,
                            _ => palette.text_muted,
                        };
                        ui.label(
                            egui::RichText::new("\u{25cf}")
                                .size(8.0)
                                .color(status_color),
                        );
                        ui.label(
                            egui::RichText::new(&flow.name)
                                .size(10.0)
                                .color(palette.text),
                        );
                        ui.label(
                            egui::RichText::new(format!("({})", flow.step_count))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                    });
                }
            });
    }
}

/// Render the Targets tab with registered site targets.
pub fn render_targets_content(ui: &mut egui::Ui, targets: &[TargetEntry], palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{25ce} Site Targets")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if targets.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{25ce}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No targets registered.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Add projects to .velocity_projects.nda to see them here.")
                    .size(8.0)
                    .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(250.0)
            .show(ui, |ui| {
                for target in targets {
                    egui::Frame::new()
                        .fill(palette.bg_tertiary)
                        .corner_radius(4.0)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&target.label)
                                    .size(10.0)
                                    .strong()
                                    .color(palette.text),
                            );
                            ui.label(
                                egui::RichText::new(&target.url)
                                    .size(9.0)
                                    .color(palette.accent),
                            );
                            if let Some(visited) = &target.last_visited {
                                ui.label(
                                    egui::RichText::new(format!("Last: {}", visited))
                                        .size(8.0)
                                        .color(palette.text_muted),
                                );
                            }
                        });
                    ui.add_space(2.0);
                }
            });
    }
}

/// Render the Recordings tab with saved action sequences.
pub fn render_recordings_content(ui: &mut egui::Ui, recordings: &[String], palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{25cf} Recordings")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if recordings.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{25cf}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No recordings saved.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Click the \u{25cf} Record button in the toolbar to start.")
                    .size(8.0)
                    .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(250.0)
            .show(ui, |ui| {
                for (i, name) in recordings.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}.", i + 1))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.label(egui::RichText::new(name).size(10.0).color(palette.text));
                    });
                }
            });
    }
}

/// Render the Logs tab with execution history from the timeline.
pub fn render_logs_content(
    ui: &mut egui::Ui,
    command_output: &str,
    event_count: usize,
    palette: IdePalette,
) {
    ui.label(
        egui::RichText::new("\u{2263} Execution Logs")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("{} events in timeline", event_count))
            .size(9.0)
            .color(palette.text_muted),
    );
    ui.add_space(4.0);
    if command_output.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{2261}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No recent output.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Run a build or task to see execution logs here.")
                    .size(8.0)
                    .color(palette.text_muted.gamma_multiply(0.7)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(250.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(command_output)
                        .size(9.0)
                        .monospace()
                        .color(palette.text),
                );
            });
    }
}

/// Render the Agents tab with live agent roster.
pub fn render_agents_content(ui: &mut egui::Ui, agents: &[AgentEntry], palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{2299} Agent Roster")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if agents.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{2299}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No active agents.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Deploy an agent from Mission Control mode (Ctrl+3).")
                    .size(8.0)
                    .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for agent in agents {
                    egui::Frame::new()
                        .fill(palette.bg_tertiary)
                        .corner_radius(4.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let status_color = match agent.status {
                                    "running" => palette.success,
                                    "idle" => palette.text_muted,
                                    "failed" => palette.error,
                                    "blocked" => palette.warning,
                                    _ => palette.text_muted,
                                };
                                ui.label(
                                    egui::RichText::new("\u{25cf}")
                                        .size(8.0)
                                        .color(status_color),
                                );
                                ui.label(
                                    egui::RichText::new(&agent.label)
                                        .size(10.0)
                                        .strong()
                                        .color(palette.text),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("#{}", agent.id))
                                        .size(9.0)
                                        .color(palette.text_muted),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "\u{00b7} {} tasks done",
                                        agent.tasks_done
                                    ))
                                    .size(9.0)
                                    .color(palette.text_muted),
                                );
                                ui.label(
                                    egui::RichText::new(agent.status)
                                        .size(9.0)
                                        .color(palette.accent),
                                );
                            });
                        });
                    ui.add_space(2.0);
                }
            });
    }
}

/// Render the Queue tab with pending tasks.
pub fn render_queue_content(ui: &mut egui::Ui, queue: &[QueueEntry], palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{229e} Task Queue")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if queue.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{229e}")
                    .size(18.0)
                    .color(palette.success),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Queue empty \u{2014} all tasks dispatched.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("New tasks appear here when agents are working.")
                    .size(8.0)
                    .color(palette.text_muted.gamma_multiply(0.7)),
            );
        });
    } else {
        ui.label(
            egui::RichText::new(format!("{} pending", queue.len()))
                .size(9.0)
                .color(palette.warning),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for entry in queue {
                    ui.horizontal(|ui| {
                        let status_color = match entry.status {
                            "Pending" => palette.text_muted,
                            "Running" => palette.success,
                            "Failed" => palette.error,
                            "Follow-up" => palette.warning,
                            _ => palette.text_muted,
                        };
                        ui.label(
                            egui::RichText::new("\u{25aa}")
                                .size(8.0)
                                .color(status_color),
                        );
                        ui.label(
                            egui::RichText::new(format!("#{}", entry.id))
                                .size(9.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            egui::RichText::new(&entry.title)
                                .size(10.0)
                                .color(palette.text),
                        );
                        ui.label(
                            egui::RichText::new(entry.status)
                                .size(9.0)
                                .color(status_color),
                        );
                    });
                }
            });
    }
}

/// Render the Metrics tab with throughput/latency/error data.
pub fn render_metrics_content(ui: &mut egui::Ui, metrics: &MetricsSnapshot, palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{22bf} Metrics")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            let total = metrics.tasks_completed + metrics.tasks_failed + metrics.tasks_pending;
            // Summary cards
            egui::Grid::new("metrics_grid")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Total tasks:")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}", total))
                            .size(10.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Completed:")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}", metrics.tasks_completed))
                            .size(10.0)
                            .color(palette.success),
                    );
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Failed:")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}", metrics.tasks_failed))
                            .size(10.0)
                            .color(palette.error),
                    );
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Pending:")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}", metrics.tasks_pending))
                            .size(10.0)
                            .color(palette.warning),
                    );
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Avg duration:")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}ms", metrics.avg_duration_ms))
                            .size(10.0)
                            .color(palette.text),
                    );
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Total tokens:")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(format!("{}", metrics.total_tokens))
                            .size(10.0)
                            .color(palette.accent),
                    );
                    ui.end_row();
                });

            // Success rate bar
            if total > 0 {
                ui.add_space(8.0);
                let success_rate = metrics.tasks_completed as f32 / total as f32;
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Success rate:")
                            .size(9.0)
                            .color(palette.text_muted),
                    );
                    let bar_width = 100.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(bar_width, 8.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, palette.bg_tertiary);
                    let filled = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(bar_width * success_rate, 8.0),
                    );
                    ui.painter().rect_filled(filled, 3.0, palette.success);
                    ui.label(
                        egui::RichText::new(format!(" {:.0}%", success_rate * 100.0))
                            .size(9.0)
                            .color(palette.text),
                    );
                });
            }
        }); // ScrollArea
}

/// Render the Favorites tab with pinned files.
pub fn render_favorites_content(
    ui: &mut egui::Ui,
    favorites: &[PathBuf],
    workspace_root: &Path,
    palette: IdePalette,
) -> Option<PathBuf> {
    let mut clicked = None;
    ui.label(
        egui::RichText::new("\u{2605} Favorites")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if favorites.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{2605}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No favorites pinned.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Open a file and click \u{2605} to pin it for quick access.")
                    .size(8.0)
                    .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for path in favorites {
                    let display = path
                        .strip_prefix(workspace_root)
                        .unwrap_or(path)
                        .to_string_lossy();
                    let icon = crate::editor::search::icon_for_path(path);
                    if ui
                        .link(
                            egui::RichText::new(format!("{} {}", icon, display))
                                .size(10.0)
                                .color(palette.text),
                        )
                        .clicked()
                    {
                        clicked = Some(path.clone());
                    }
                }
            });
    }
    clicked
}

/// Render the Bookmarks tab with in-file bookmark entries.
pub fn render_bookmarks_content(
    ui: &mut egui::Ui,
    bookmarks: &[BookmarkEntry],
    workspace_root: &Path,
    palette: IdePalette,
) -> Option<(PathBuf, usize)> {
    let mut jump_to = None;
    ui.label(
        egui::RichText::new("\u{229b} Bookmarks")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if bookmarks.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{229b}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No bookmarks set.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Use Ctrl+Shift+B while editing to bookmark a line.")
                    .size(8.0)
                    .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for bm in bookmarks {
                    let display = bm
                        .file
                        .strip_prefix(workspace_root)
                        .unwrap_or(&bm.file)
                        .to_string_lossy();
                    if ui
                        .link(
                            egui::RichText::new(format!(
                                "{}:{} \u{2014} {}",
                                display,
                                bm.line + 1,
                                bm.label
                            ))
                            .size(9.0)
                            .color(palette.text),
                        )
                        .clicked()
                    {
                        jump_to = Some((bm.file.clone(), bm.line));
                    }
                }
            });
    }
    jump_to
}

/// Render the Accessibility Audit tab with WCAG findings.
pub fn render_audit_content(ui: &mut egui::Ui, findings: &[AuditFinding], palette: IdePalette) {
    ui.label(
        egui::RichText::new("\u{267f} Accessibility Audit")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    if findings.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("\u{267f}")
                    .size(18.0)
                    .color(palette.text_muted.gamma_multiply(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No audit findings.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Run an audit via the toolbar \u{2713} button to check WCAG compliance.",
                )
                .size(8.0)
                .color(palette.accent.gamma_multiply(0.8)),
            );
        });
    } else {
        ui.label(
            egui::RichText::new(format!("{} issue(s) found", findings.len()))
                .size(10.0)
                .color(palette.warning),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for finding in findings {
                    let severity_color = match finding.severity {
                        "error" => palette.error,
                        "warning" => palette.warning,
                        _ => palette.text_muted,
                    };
                    egui::Frame::new()
                        .fill(palette.bg_tertiary)
                        .corner_radius(4.0)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(finding.severity)
                                        .size(9.0)
                                        .strong()
                                        .color(severity_color),
                                );
                                ui.label(
                                    egui::RichText::new(&finding.rule)
                                        .size(9.0)
                                        .color(palette.text),
                                );
                            });
                            ui.label(
                                egui::RichText::new(&finding.element)
                                    .size(9.0)
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                egui::RichText::new(&finding.suggestion)
                                    .size(9.0)
                                    .color(palette.accent),
                            );
                        });
                    ui.add_space(2.0);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tabs_have_labels_and_icons() {
        let all = [
            SidebarTab::Files,
            SidebarTab::Outline,
            SidebarTab::Git,
            SidebarTab::Search,
            SidebarTab::Flows,
            SidebarTab::Targets,
            SidebarTab::Recordings,
            SidebarTab::Logs,
            SidebarTab::Agents,
            SidebarTab::Queue,
            SidebarTab::Timeline,
            SidebarTab::Metrics,
            SidebarTab::Favorites,
            SidebarTab::Bookmarks,
            SidebarTab::AccessibilityAudit,
            SidebarTab::Browse,
        ];
        for tab in all {
            assert!(!tab.label().is_empty());
            assert!(!tab.icon().is_empty());
        }
    }
}
