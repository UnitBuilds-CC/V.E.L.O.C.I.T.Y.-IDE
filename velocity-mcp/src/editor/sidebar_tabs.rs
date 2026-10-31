//! Sidebar Tab Content - the panels the left sidebar draws for each section.
//!
//! The tab *strip* used to live here too, driven by a `SidebarTab` enum and a
//! per-mode list. The activity bar (`app_map::RAILS`) replaced it and nothing
//! read the enum, so the strip and its enum are gone; what is left is the set
//! of content renderers `render.rs` calls.

use crate::editor::theme::IdePalette;
use egui;

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

/// Target entry for the Targets sidebar tab: one saved browser workflow's
/// starting site. [`crate::editor::app::velocity_app::panel_sources`] fills it.
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

/// Toggle a bookmark at the given (file, line).
/// If a bookmark already exists at that location it is removed; otherwise a new one is inserted
/// (the list stays sorted by file then line). Returns `true` if removed, `false` if added.
pub fn toggle_line_bookmark(
    bookmarks: &mut Vec<BookmarkEntry>,
    file: &Path,
    line: usize,
    label: &str,
) -> bool {
    if let Some(pos) = bookmarks
        .iter()
        .position(|b| b.file == file && b.line == line)
    {
        bookmarks.remove(pos);
        true // removed
    } else {
        let entry = BookmarkEntry {
            file: file.to_path_buf(),
            line,
            label: label.to_string(),
        };
        let insert_pos = bookmarks
            .iter()
            .position(|b| (b.file.as_path(), b.line) > (entry.file.as_path(), entry.line))
            .unwrap_or(bookmarks.len());
        bookmarks.insert(insert_pos, entry);
        false // added
    }
}

/// WCAG audit finding for the AccessibilityAudit tab.
pub struct AuditFinding {
    pub severity: &'static str,
    pub rule: String,
    pub element: String,
    pub suggestion: String,
}

/// The audit the Accessibility Audit panel runs: every shipped palette, every
/// pair [`crate::editor::theme::palette_contrast_checks`] names, only the pairs
/// that fail.
///
/// What this can see is the editor's own colours, which is a real and bounded
/// claim: the IDE ships no document view, no focus ring and no accessible tree
/// of its own (there is no AccessKit integration in the binary), so contrast is
/// the one WCAG rule it can measure without inventing a subject. It does not
/// repaint anything — a finding is a finding, and changing a palette is a
/// decision a person makes with the hex in front of them.
pub fn audit_palette_contrast() -> Vec<AuditFinding> {
    let catalog: Vec<(&str, IdePalette)> = crate::editor::theme::ThemeVariant::ALL
        .iter()
        .map(|variant| ((*variant).label(), (*variant).palette()))
        .collect();
    audit_palettes(&catalog)
}

/// The audit over an explicit catalog, so a test can hand it a palette it owns
/// instead of trusting whichever colours the shipped themes happen to have today.
fn audit_palettes(entries: &[(&str, IdePalette)]) -> Vec<AuditFinding> {
    use crate::editor::theme::MIN_LARGE_TEXT_CONTRAST;
    let mut findings = Vec::new();
    for (theme, palette) in entries {
        for check in crate::editor::theme::palette_contrast_checks(palette) {
            if check.passes() {
                continue;
            }
            let remedy = check.remedy();
            findings.push(AuditFinding {
                // Under the large-text floor there is no reading of the rule
                // this passes, so it is not a warning about style.
                severity: if check.ratio < MIN_LARGE_TEXT_CONTRAST {
                    "error"
                } else {
                    "warning"
                },
                rule: format!("WCAG 1.4.3 - {} on {}", check.ink, check.surface),
                element: format!("{} theme measures {:.2}:1", theme, check.ratio),
                suggestion: format!(
                    "needs {:.1}:1 - paint it with #{:02X}{:02X}{:02X}",
                    check.required,
                    remedy.r(),
                    remedy.g(),
                    remedy.b()
                ),
            });
        }
    }
    findings
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
                    .color(palette.faded_muted(0.7)),
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
            if !staged.is_empty() && ui.small_button("Unstage All").clicked() {
                action = GitTabAction::UnstageAll;
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
            if !unstaged.is_empty() && ui.small_button("Stage All").clicked() {
                action = GitTabAction::StageAll;
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
                    .color(palette.faded_muted(0.6)),
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
                .color(palette.faded(palette.accent, 0.8)),
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

/// Render the Targets tab with the starting site of each saved workflow.
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
                    .color(palette.faded_muted(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No targets registered.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Each saved browser workflow contributes the site it starts from.",
                )
                .size(8.0)
                .color(palette.faded(palette.accent, 0.8)),
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
                    .color(palette.faded_muted(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No recordings saved.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Saved scripts appear here from .velocity/wa-scripts/ once the workflow tools record one.",
                )
                    .size(8.0)
                    .color(palette.faded(palette.accent, 0.8)),
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
                    .color(palette.faded_muted(0.6)),
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
                    .color(palette.faded_muted(0.7)),
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
                    .color(palette.faded_muted(0.6)),
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
                    .color(palette.faded(palette.accent, 0.8)),
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
                    .color(palette.faded_muted(0.7)),
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
                    .color(palette.faded_muted(0.6)),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("No favorites pinned.")
                    .size(9.0)
                    .color(palette.text_muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Open a file, then right-click its tab to add it here.")
                    .size(8.0)
                    .color(palette.faded(palette.accent, 0.8)),
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
                    .color(palette.faded_muted(0.6)),
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
                    .color(palette.faded(palette.accent, 0.8)),
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

/// Render the Accessibility Audit tab.
///
/// `findings` is `None` until the audit runs, which is a different fact from
/// `Some(&[])`: a panel that says "no findings" before anything has measured
/// anything is the most misleading thing a checker can do, and it is exactly
/// what this tab used to say every time it was opened.
pub fn render_audit_content(
    ui: &mut egui::Ui,
    findings: Option<&[AuditFinding]>,
    palette: IdePalette,
) {
    ui.label(
        egui::RichText::new("\u{267f} Accessibility Audit")
            .size(11.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(4.0);
    match findings {
        None | Some([]) => {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("\u{267f}")
                        .size(18.0)
                        .color(palette.faded_muted(0.6)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(match findings {
                        None => "The audit has not been run.",
                        _ => "No contrast findings.",
                    })
                    .size(9.0)
                    .color(palette.text_muted),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(match findings {
                        None => "Press Run audit to measure the shipped palettes.",
                        _ => "Every audited pair clears the floor it is held to.",
                    })
                    .size(8.0)
                    .color(palette.faded(palette.accent, 0.8)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "It measures the text colours of the {} shipped palettes \
                         against the fills they are painted on, at the WCAG 1.4.3 \
                         floor. Contrast is the whole of what it can see.",
                        crate::editor::theme::ThemeVariant::ALL.len()
                    ))
                    .size(8.0)
                    .color(palette.text_muted),
                );
                ui.label(
                    egui::RichText::new(
                        "No focus order, no keyboard traps, no document: the IDE \
                         renders no page and builds no accessible tree, so there is \
                         nothing here for those rules to be measured against.",
                    )
                    .size(8.0)
                    .color(palette.text_muted),
                );
            });
        }
        Some(rows) => {
            ui.label(
                egui::RichText::new(format!("{} issue(s) found", rows.len()))
                    .size(10.0)
                    .color(palette.warning),
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for finding in rows {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn add_bookmark_to_empty_list() {
        let mut bm = Vec::new();
        let removed = toggle_line_bookmark(&mut bm, Path::new("a.rs"), 10, "fn main()");
        assert!(!removed);
        assert_eq!(bm.len(), 1);
        assert_eq!(bm[0].line, 10);
        assert_eq!(bm[0].label, "fn main()");
    }

    #[test]
    fn toggle_removes_existing_bookmark() {
        let mut bm = Vec::new();
        toggle_line_bookmark(&mut bm, Path::new("a.rs"), 5, "x");
        let removed = toggle_line_bookmark(&mut bm, Path::new("a.rs"), 5, "x");
        assert!(removed);
        assert!(bm.is_empty());
    }

    #[test]
    fn bookmarks_stay_sorted() {
        let mut bm = Vec::new();
        toggle_line_bookmark(&mut bm, Path::new("b.rs"), 1, "second");
        toggle_line_bookmark(&mut bm, Path::new("a.rs"), 20, "first_file");
        toggle_line_bookmark(&mut bm, Path::new("a.rs"), 5, "early");
        assert_eq!(bm[0].file, Path::new("a.rs"));
        assert_eq!(bm[0].line, 5);
        assert_eq!(bm[1].file, Path::new("a.rs"));
        assert_eq!(bm[1].line, 20);
        assert_eq!(bm[2].file, Path::new("b.rs"));
    }

    // ── The contrast audit behind the Accessibility Audit panel ────────────
    //
    // The audit is this panel's whole claim, so these tests read the findings
    // the way the panel does: which pairs appear, which do not, what the
    // severity is meant to tell the reader, and whether the colour offered as a
    // remedy actually clears the floor it was measured against.

    use crate::editor::headless_render::painted_text;
    use crate::editor::theme::{
        contrast_ratio, ThemeVariant, MIN_LARGE_TEXT_CONTRAST, MIN_TEXT_CONTRAST,
    };
    use std::collections::HashSet;

    fn rule_list(rows: &[AuditFinding]) -> Vec<String> {
        rows.iter().map(|finding| finding.rule.clone()).collect()
    }

    /// A grey whose contrast against `surface` lands between the two floors:
    /// readable as large text, short of the body-text standard. Scanned rather
    /// than hard-coded, so the fixture cannot drift from the palette it was
    /// drawn against.
    fn ink_between_the_floors(surface: egui::Color32) -> egui::Color32 {
        (0u8..=255)
            .map(|g| egui::Color32::from_rgb(g, g, g))
            .find(|ink| {
                let ratio = contrast_ratio(*ink, surface);
                (MIN_LARGE_TEXT_CONTRAST..MIN_TEXT_CONTRAST).contains(&ratio)
            })
            .expect("no grey lands between the two floors")
    }

    /// A grey that fails even the large-text floor.
    fn ink_below_the_large_text_floor(surface: egui::Color32) -> egui::Color32 {
        (0u8..=255)
            .map(|g| egui::Color32::from_rgb(g, g, g))
            .find(|ink| contrast_ratio(*ink, surface) < MIN_LARGE_TEXT_CONTRAST)
            .expect("no grey falls below the large-text floor")
    }

    /// A palette that is clean everywhere except the one pair under test.
    fn probe_palette(ink: egui::Color32) -> IdePalette {
        let mut palette = ThemeVariant::HighContrast.palette();
        palette.text = ink;
        palette
    }

    fn text_on_bg_primary(rows: &[AuditFinding]) -> &AuditFinding {
        rows.iter()
            .find(|finding| finding.rule.ends_with("text on bg_primary"))
            .unwrap_or_else(|| panic!("no text-on-bg_primary finding among {:?}", rule_list(rows)))
    }

    #[test]
    fn the_audit_is_silent_about_a_palette_that_clears_every_pair() {
        let rows = audit_palettes(&[("High Contrast", ThemeVariant::HighContrast.palette())]);
        assert!(
            rows.is_empty(),
            "a clean palette produced findings: {:?}",
            rule_list(&rows)
        );
    }

    #[test]
    fn the_audit_reports_each_failing_pair_once_and_names_its_theme() {
        let rows = audit_palettes(&[("Daylight", ThemeVariant::Daylight.palette())]);
        assert!(
            !rows.is_empty(),
            "Daylight is expected to fail at least one pair"
        );
        let mut seen = HashSet::new();
        for finding in &rows {
            assert!(
                finding.rule.starts_with("WCAG 1.4.3 - "),
                "{} carries no rule",
                finding.rule
            );
            assert!(
                finding.element.starts_with("Daylight theme measures "),
                "{} does not name the theme it measured",
                finding.element
            );
            assert!(
                seen.insert(finding.rule.clone()),
                "{} is reported twice",
                finding.rule
            );
        }
    }

    #[test]
    fn severity_separates_unreadable_from_merely_below_the_standard() {
        let surface = ThemeVariant::HighContrast.palette().bg_primary;
        let mid = audit_palettes(&[("Probe", probe_palette(ink_between_the_floors(surface)))]);
        assert_eq!(
            text_on_bg_primary(&mid).severity,
            "warning",
            "a pair that still reads as large text is not an error"
        );
        let bad = audit_palettes(&[(
            "Probe",
            probe_palette(ink_below_the_large_text_floor(surface)),
        )]);
        assert_eq!(
            text_on_bg_primary(&bad).severity,
            "error",
            "a pair below the large-text floor passes the rule on no reading"
        );
    }

    #[test]
    fn the_colour_offered_as_a_remedy_clears_the_floor_it_was_asked_for() {
        let palette = probe_palette(ink_below_the_large_text_floor(
            ThemeVariant::HighContrast.palette().bg_primary,
        ));
        let rows = audit_palettes(&[("Probe", palette)]);
        let finding = text_on_bg_primary(&rows);
        assert!(
            finding.suggestion.contains("needs 4.5:1"),
            "{}",
            finding.suggestion
        );
        let hex = finding
            .suggestion
            .rsplit_once('#')
            .map(|(_, digits)| digits)
            .unwrap_or_else(|| panic!("{} offers no hex colour", finding.suggestion));
        let packed = u32::from_str_radix(hex, 16).unwrap_or_else(|_| panic!("{hex} is not hex"));
        let remedy =
            egui::Color32::from_rgb((packed >> 16) as u8, (packed >> 8) as u8, packed as u8);
        let measured = contrast_ratio(remedy, palette.bg_primary);
        assert!(
            measured + 0.005 >= MIN_TEXT_CONTRAST,
            "the offered remedy {hex} measures {measured:.2}:1 on the surface it was offered for"
        );
    }

    #[test]
    fn the_shipped_catalog_is_audited_in_theme_order_and_answers_twice_the_same() {
        let first = audit_palette_contrast();
        let again = audit_palette_contrast();
        let key = |rows: &[AuditFinding]| {
            rows.iter()
                .map(|f| {
                    (
                        f.severity,
                        f.rule.clone(),
                        f.element.clone(),
                        f.suggestion.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&first), key(&again), "the audit is not deterministic");
        assert!(
            !first.is_empty(),
            "the shipped palettes are expected to fail somewhere"
        );
        // Themes are walked in the order the theme picker offers them, so the
        // panel lists findings in the order a reader thinks about themes.
        let order: Vec<&str> = ThemeVariant::ALL
            .iter()
            .map(|variant| (*variant).label())
            .collect();
        let mut themes: Vec<&str> = Vec::new();
        for finding in &first {
            let named = order
                .iter()
                .copied()
                .find(|label| finding.element.starts_with(&format!("{label} theme")))
                .unwrap_or_else(|| panic!("{} names no shipped theme", finding.element));
            if themes.last() != Some(&named) {
                themes.push(named);
            }
        }
        let mut sorted = themes.clone();
        sorted.sort_by_key(|label| order.iter().position(|l| l == label).unwrap_or(usize::MAX));
        assert_eq!(
            themes, sorted,
            "findings are not grouped in theme order: {themes:?}"
        );
    }

    #[test]
    fn the_panel_distinguishes_never_having_run_from_having_found_nothing() {
        let palette = ThemeVariant::Midnight.palette();
        let unrun = painted_text(|ui| render_audit_content(ui, None, palette));
        let clean = painted_text(|ui| render_audit_content(ui, Some(&[]), palette));
        assert!(unrun.contains("The audit has not been run."), "{unrun}");
        assert!(unrun.contains("Press Run audit"), "{unrun}");
        assert!(clean.contains("No contrast findings."), "{clean}");
        assert!(
            clean.contains("Every audited pair clears the floor"),
            "{clean}"
        );
        // Two different facts, so the panel must not say the same sentence for
        // both of them.
        assert!(!unrun.contains("No contrast findings."), "{unrun}");
        assert!(!clean.contains("has not been run"), "{clean}");
        // And the old copy pointed the reader at a toolbar button that does not
        // exist in this app.
        for text in [&unrun, &clean] {
            assert!(!text.contains("toolbar"), "{text}");
        }
        // Both empty states still state the bounded scope of the audit.
        assert!(clean.contains("WCAG 1.4.3"), "{clean}");
    }

    #[test]
    fn the_panel_prints_every_field_of_a_finding_it_is_given() {
        let palette = ThemeVariant::Midnight.palette();
        // A bounded list, because the findings rows live in a scroll area and
        // the probe only sees the shapes inside that area's clip rect. The
        // shipped catalog is longer than one viewport on purpose; the test asks
        // only that a row the panel shows shows all four of its fields.
        let surface = ThemeVariant::HighContrast.palette().bg_primary;
        let rows = audit_palettes(&[(
            "Probe",
            probe_palette(ink_below_the_large_text_floor(surface)),
        )]);
        assert!(!rows.is_empty(), "the probe palette should fail somewhere");
        let text = painted_text(|ui| render_audit_content(ui, Some(&rows), palette));
        assert!(
            text.contains(&format!("{} issue(s) found", rows.len())),
            "{text}"
        );
        for finding in &rows {
            assert!(
                text.contains(&finding.rule),
                "missing rule {}",
                finding.rule
            );
            assert!(
                text.contains(&finding.element),
                "missing element {}",
                finding.element
            );
            assert!(
                text.contains(&finding.suggestion),
                "missing suggestion {}",
                finding.suggestion
            );
            assert!(
                text.contains(finding.severity),
                "missing severity {}",
                finding.severity
            );
        }
        // The heading counts what it was given even when the list is longer
        // than the area it is drawn in.
        let shipped = audit_palette_contrast();
        let long = painted_text(|ui| render_audit_content(ui, Some(&shipped), palette));
        assert!(
            long.contains(&format!("{} issue(s) found", shipped.len())),
            "{long}"
        );
    }

    #[test]
    fn different_lines_same_file_are_independent() {
        let mut bm = Vec::new();
        toggle_line_bookmark(&mut bm, Path::new("x.rs"), 1, "a");
        toggle_line_bookmark(&mut bm, Path::new("x.rs"), 2, "b");
        assert_eq!(bm.len(), 2);
        toggle_line_bookmark(&mut bm, Path::new("x.rs"), 1, "a");
        assert_eq!(bm.len(), 1);
        assert_eq!(bm[0].line, 2);
    }
}
