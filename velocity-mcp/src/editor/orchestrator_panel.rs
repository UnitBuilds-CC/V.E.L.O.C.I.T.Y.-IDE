//! Optional IDE panel showing the orchestrator task graph.

use crate::orchestrator::blueprint::{Task, TaskGraph};
use crate::orchestrator::registry::{OrchestratorRegistry, TaskStatus};
use crate::orchestrator::scheduler;
use crate::orchestrator::TaskId;
use eframe::egui;
use egui::{ScrollArea, Ui};

#[derive(Debug, Default)]
pub struct OrchestratorPanel {
    pub graph: TaskGraph,
    pub registry: Option<OrchestratorRegistry>,
    pub expanded: bool,
}

impl OrchestratorPanel {
    pub fn new() -> Self {
        let graph = TaskGraph::example_game();
        let registry = OrchestratorRegistry::new(&graph);
        Self {
            graph,
            registry: Some(registry),
            expanded: true,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Orchestrator");
            let label = if self.expanded { "−" } else { "+" };
            ui.toggle_value(&mut self.expanded, label);
        });
        ui.separator();

        let plan = scheduler::plan(&self.graph);
        ui.label(format!("Tasks: {} | Phases: {}", self.graph.tasks.len(), plan.phases.len()));

        ScrollArea::vertical().show(ui, |ui| {
            for (phase_idx, phase) in plan.phases.iter().enumerate() {
                ui.group(|ui| {
                    ui.label(format!("Phase {}", phase_idx + 1));
                    for id in phase {
                        if let Some(task) = self.graph.tasks.get(id) {
                            self.task_row(ui, task);
                        }
                    }
                });
            }
        });
    }

    fn task_row(&self, ui: &mut Ui, task: &Task) {
        let status_label = match self.registry.as_ref().and_then(|r| r.statuses.get(&task.id)) {
            Some(TaskStatus::Pending) => "⏳ Pending",
            Some(TaskStatus::Running) => "🔵 Running",
            Some(TaskStatus::Done(_)) => "✅ Done",
            Some(TaskStatus::Failed(_)) => "❌ Failed",
            None => "⏳ Pending",
        };

        ui.horizontal(|ui| {
            ui.label(status_label);
            ui.label(&task.title);
        });

        let expanded = self.expanded;
        if expanded {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&task.description).small().weak());
            });
        }
    }
}
