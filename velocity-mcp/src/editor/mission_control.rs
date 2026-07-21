#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterventionDisposition {
    ApplyToRunningAgent,
    SpawnRoutedFollowUp,
    Dismissed,
}

#[derive(Debug, Clone)]
pub struct MissionIntervention {
    pub id: u64,
    pub note: String,
    pub status: String,
    pub disposition: Option<InterventionDisposition>,
}

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct MissionControlState {
    pub brief: String,
    pub intervention_input: String,
    pub interventions: Vec<MissionIntervention>,
    pub auto_execute: bool,
    pub selected_task_id: Option<u64>,
    pub selected_task_note_input: String,
    pub mirrored_worker_event_counts: HashMap<u64, usize>,
}

impl MissionControlState {
    pub fn new() -> Self {
        Self {
            brief: String::new(),
            intervention_input: String::new(),
            interventions: Vec::new(),
            auto_execute: true,
            selected_task_id: None,
            selected_task_note_input: String::new(),
            mirrored_worker_event_counts: HashMap::new(),
        }
    }

    pub fn sync_selected_task(&mut self, valid_task_ids: &[u64]) {
        self.mirrored_worker_event_counts
            .retain(|task_id, _| valid_task_ids.contains(task_id));
        if self
            .selected_task_id
            .is_some_and(|selected_id| !valid_task_ids.contains(&selected_id))
        {
            self.selected_task_id = valid_task_ids.first().copied();
            self.selected_task_note_input.clear();
        }
    }

    pub fn set_selected_task(&mut self, selected_task_id: Option<u64>) {
        if self.selected_task_id != selected_task_id {
            self.selected_task_id = selected_task_id;
            self.selected_task_note_input.clear();
        }
    }

    pub fn clear_worker_event_tracking(&mut self) {
        self.mirrored_worker_event_counts.clear();
    }

    pub fn mirrored_worker_event_count(&self, task_id: u64) -> usize {
        self.mirrored_worker_event_counts
            .get(&task_id)
            .copied()
            .unwrap_or(0)
    }

    pub fn set_mirrored_worker_event_count(&mut self, task_id: u64, count: usize) {
        self.mirrored_worker_event_counts.insert(task_id, count);
    }

    pub fn queue_intervention(&mut self, id: u64, note: String) {
        self.interventions.push(MissionIntervention {
            id,
            note,
            status: "Queued for operator action".to_string(),
            disposition: None,
        });
    }
}
