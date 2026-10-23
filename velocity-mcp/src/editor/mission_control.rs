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
    /// True only when a reviewed plan may launch automatically. New missions
    /// deliberately start in plan-first mode so edits are never surprising.
    pub auto_execute: bool,
    pub active_sub_tab: usize,
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
            auto_execute: false,
            active_sub_tab: 0,
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

    /// Attempt to dispatch a queued intervention to the target task.
    /// Returns true if the note was delivered (disposition set to
    /// ApplyToRunningAgent), false if no running agent accepted it.
    #[must_use = "caller should report whether delivery succeeded"]
    pub fn dispatch_intervention(
        &mut self,
        id: u64,
        send_note: impl FnOnce(String) -> bool,
    ) -> bool {
        let Some(intervention) = self
            .interventions
            .iter_mut()
            .find(|iv| iv.id == id && iv.disposition.is_none())
        else {
            return false;
        };
        let delivered = send_note(intervention.note.clone());
        if delivered {
            intervention.disposition = Some(InterventionDisposition::ApplyToRunningAgent);
            intervention.status = "Delivered to running agent".to_string();
        } else {
            intervention.disposition = Some(InterventionDisposition::Dismissed);
            intervention.status = "No running agent to receive note".to_string();
        }
        delivered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_intervention_delivers_to_running_agent() {
        let mut mc = MissionControlState::new();
        mc.queue_intervention(1, "fix the selector".to_string());
        let delivered = mc.dispatch_intervention(1, |_note| true);
        assert!(delivered);
        let iv = &mc.interventions[0];
        assert_eq!(
            iv.disposition,
            Some(InterventionDisposition::ApplyToRunningAgent)
        );
        assert!(iv.status.contains("Delivered"));
    }

    #[test]
    fn dispatch_intervention_dismisses_when_no_agent() {
        let mut mc = MissionControlState::new();
        mc.queue_intervention(2, "retry step 3".to_string());
        let delivered = mc.dispatch_intervention(2, |_note| false);
        assert!(!delivered);
        let iv = &mc.interventions[0];
        assert_eq!(iv.disposition, Some(InterventionDisposition::Dismissed));
        assert!(iv.status.contains("No running agent"));
    }

    #[test]
    fn dispatch_already_handled_intervention_is_noop() {
        let mut mc = MissionControlState::new();
        mc.queue_intervention(3, "note".to_string());
        let _ = mc.dispatch_intervention(3, |_| true);
        // Second dispatch should be a no-op (disposition already set).
        let again = mc.dispatch_intervention(3, |_| true);
        assert!(!again);
        assert_eq!(mc.interventions.len(), 1);
    }
}
