//! Multi-step challenge state machine.
//!
//! Models captcha solving as an explicit FSM: states represent observable
//! challenge phases, transitions represent actions the solver can take.
//! Pre-built state graphs exist for common archetypes (checkbox, image grid,
//! tile flip, slider, multi-round).

use std::collections::HashMap;

use super::challenge::ChallengeDescriptor;
use super::visual_fingerprint::ChallengeArchetype;

/// A named state in the challenge FSM.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChallengeState {
    pub id: String,
    /// Observable signature that identifies this state (e.g., DOM marker, pixel hash).
    pub observable_signature: String,
}

/// Kinds of actions the solver can execute.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    Click,
    TypeText(String),
    Wait { timeout_ms: u64 },
    SelectTiles { indices: Vec<u8> },
    RotateTile { index: u8, degrees: f64 },
    DragSlider { offset_x: f64 },
    Submit,
    Custom(String),
}

/// An action with optional target and parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct ChallengeAction {
    pub kind: ActionKind,
    pub target: Option<String>,
    pub params: HashMap<String, String>,
}

impl ChallengeAction {
    pub fn click(target: &str) -> Self {
        Self {
            kind: ActionKind::Click,
            target: Some(target.to_string()),
            params: HashMap::new(),
        }
    }

    pub fn wait(timeout_ms: u64) -> Self {
        Self {
            kind: ActionKind::Wait { timeout_ms },
            target: None,
            params: HashMap::new(),
        }
    }

    pub fn select_tiles(indices: Vec<u8>) -> Self {
        Self {
            kind: ActionKind::SelectTiles { indices },
            target: None,
            params: HashMap::new(),
        }
    }

    pub fn rotate_tile(index: u8, degrees: f64) -> Self {
        Self {
            kind: ActionKind::RotateTile { index, degrees },
            target: None,
            params: HashMap::new(),
        }
    }

    pub fn drag_slider(offset_x: f64) -> Self {
        Self {
            kind: ActionKind::DragSlider { offset_x },
            target: None,
            params: HashMap::new(),
        }
    }

    pub fn submit() -> Self {
        Self {
            kind: ActionKind::Submit,
            target: None,
            params: HashMap::new(),
        }
    }

    pub fn type_text(text: &str, target: &str) -> Self {
        Self {
            kind: ActionKind::TypeText(text.to_string()),
            target: Some(target.to_string()),
            params: HashMap::new(),
        }
    }
}

/// A directed transition in the state machine.
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from: String,
    pub action: ChallengeAction,
    pub to: String,
    pub confidence: f32,
}

/// The challenge state machine — tracks current state, available transitions,
/// and execution history.
#[derive(Debug)]
pub struct ChallengeStateMachine {
    pub descriptor: ChallengeDescriptor,
    pub states: Vec<ChallengeState>,
    pub transitions: Vec<StateTransition>,
    pub current_state_id: String,
    pub history: Vec<StateTransition>,
    pub step_count: u32,
}

impl ChallengeStateMachine {
    /// Create a state machine from an archetype with pre-built state graph.
    pub fn from_archetype(descriptor: ChallengeDescriptor, archetype: &ChallengeArchetype) -> Self {
        let (states, transitions, initial) = match archetype {
            ChallengeArchetype::Checkbox => Self::checkbox_graph(),
            ChallengeArchetype::ImageGridSelect => Self::image_grid_select_graph(),
            ChallengeArchetype::TileFlip => Self::tile_flip_graph(),
            ChallengeArchetype::Slider => Self::slider_graph(),
            ChallengeArchetype::TextEntry => Self::text_entry_graph(),
            ChallengeArchetype::MultiRound => Self::multi_round_graph(3),
            ChallengeArchetype::Unknown => Self::generic_graph(),
        };

        Self {
            descriptor,
            states,
            transitions,
            current_state_id: initial,
            history: Vec::new(),
            step_count: 0,
        }
    }

    /// Get available actions from the current state.
    pub fn available_actions(&self) -> Vec<&StateTransition> {
        self.transitions
            .iter()
            .filter(|t| t.from == self.current_state_id)
            .collect()
    }

    /// Execute a transition by index into available_actions().
    /// Returns true if the transition was valid and applied.
    pub fn execute_transition(&mut self, action: &ChallengeAction) -> bool {
        let transition = self
            .transitions
            .iter()
            .find(|t| t.from == self.current_state_id && t.action.kind == action.kind);

        if let Some(t) = transition {
            let t = t.clone();
            self.current_state_id = t.to.clone();
            self.history.push(t);
            self.step_count += 1;
            true
        } else {
            false
        }
    }

    /// Check if the machine is in a terminal (solved/failed) state.
    pub fn is_terminal(&self) -> bool {
        self.current_state_id == "solved" || self.current_state_id == "failed"
    }

    /// Check if the challenge is solved.
    pub fn is_solved(&self) -> bool {
        self.current_state_id == "solved"
    }

    /// Reset the machine to its initial state.
    pub fn reset(&mut self) {
        if let Some(first) = self.states.first() {
            self.current_state_id = first.id.clone();
        }
        self.history.clear();
        self.step_count = 0;
    }

    /// Get the recorded solve sequence (for template storage).
    pub fn solve_sequence(&self) -> Vec<(String, ChallengeAction)> {
        self.history
            .iter()
            .map(|t| (t.from.clone(), t.action.clone()))
            .collect()
    }

    // --- Pre-built state graphs ---

    fn checkbox_graph() -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let states = vec![
            ChallengeState {
                id: "shown".into(),
                observable_signature: "checkbox_visible".into(),
            },
            ChallengeState {
                id: "solved".into(),
                observable_signature: "token_received".into(),
            },
        ];
        let transitions = vec![StateTransition {
            from: "shown".into(),
            action: ChallengeAction::click("checkbox"),
            to: "solved".into(),
            confidence: 0.95,
        }];
        (states, transitions, "shown".into())
    }

    fn image_grid_select_graph() -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let states = vec![
            ChallengeState {
                id: "shown".into(),
                observable_signature: "grid_visible".into(),
            },
            ChallengeState {
                id: "selecting".into(),
                observable_signature: "tiles_highlighted".into(),
            },
            ChallengeState {
                id: "verifying".into(),
                observable_signature: "spinner_active".into(),
            },
            ChallengeState {
                id: "solved".into(),
                observable_signature: "token_received".into(),
            },
            ChallengeState {
                id: "retry".into(),
                observable_signature: "new_grid_shown".into(),
            },
        ];
        let transitions = vec![
            StateTransition {
                from: "shown".into(),
                action: ChallengeAction::select_tiles(vec![0, 1, 2]),
                to: "selecting".into(),
                confidence: 0.8,
            },
            StateTransition {
                from: "selecting".into(),
                action: ChallengeAction::submit(),
                to: "verifying".into(),
                confidence: 0.9,
            },
            StateTransition {
                from: "verifying".into(),
                action: ChallengeAction::wait(2000),
                to: "solved".into(),
                confidence: 0.7,
            },
            StateTransition {
                from: "verifying".into(),
                action: ChallengeAction::wait(3000),
                to: "retry".into(),
                confidence: 0.3,
            },
            StateTransition {
                from: "retry".into(),
                action: ChallengeAction::select_tiles(vec![3, 4, 5]),
                to: "selecting".into(),
                confidence: 0.6,
            },
        ];
        (states, transitions, "shown".into())
    }

    fn tile_flip_graph() -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let states = vec![
            ChallengeState {
                id: "shown".into(),
                observable_signature: "tiles_visible".into(),
            },
            ChallengeState {
                id: "flipping".into(),
                observable_signature: "tile_animating".into(),
            },
            ChallengeState {
                id: "verifying".into(),
                observable_signature: "checking_alignment".into(),
            },
            ChallengeState {
                id: "solved".into(),
                observable_signature: "all_aligned".into(),
            },
            ChallengeState {
                id: "rotate_again".into(),
                observable_signature: "partial_alignment".into(),
            },
        ];
        let transitions = vec![
            StateTransition {
                from: "shown".into(),
                action: ChallengeAction::rotate_tile(0, 90.0),
                to: "flipping".into(),
                confidence: 0.85,
            },
            StateTransition {
                from: "flipping".into(),
                action: ChallengeAction::submit(),
                to: "verifying".into(),
                confidence: 0.9,
            },
            StateTransition {
                from: "verifying".into(),
                action: ChallengeAction::wait(1500),
                to: "solved".into(),
                confidence: 0.7,
            },
            StateTransition {
                from: "verifying".into(),
                action: ChallengeAction::wait(2000),
                to: "rotate_again".into(),
                confidence: 0.3,
            },
            StateTransition {
                from: "rotate_again".into(),
                action: ChallengeAction::rotate_tile(1, 90.0),
                to: "flipping".into(),
                confidence: 0.75,
            },
        ];
        (states, transitions, "shown".into())
    }

    fn slider_graph() -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let states = vec![
            ChallengeState {
                id: "shown".into(),
                observable_signature: "slider_track_visible".into(),
            },
            ChallengeState {
                id: "dragging".into(),
                observable_signature: "handle_moving".into(),
            },
            ChallengeState {
                id: "verifying".into(),
                observable_signature: "checking_position".into(),
            },
            ChallengeState {
                id: "solved".into(),
                observable_signature: "puzzle_complete".into(),
            },
        ];
        let transitions = vec![
            StateTransition {
                from: "shown".into(),
                action: ChallengeAction::drag_slider(250.0),
                to: "dragging".into(),
                confidence: 0.85,
            },
            StateTransition {
                from: "dragging".into(),
                action: ChallengeAction::submit(),
                to: "verifying".into(),
                confidence: 0.9,
            },
            StateTransition {
                from: "verifying".into(),
                action: ChallengeAction::wait(1000),
                to: "solved".into(),
                confidence: 0.8,
            },
        ];
        (states, transitions, "shown".into())
    }

    fn text_entry_graph() -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let states = vec![
            ChallengeState {
                id: "shown".into(),
                observable_signature: "text_image_visible".into(),
            },
            ChallengeState {
                id: "typing".into(),
                observable_signature: "input_focused".into(),
            },
            ChallengeState {
                id: "solved".into(),
                observable_signature: "verified".into(),
            },
        ];
        let transitions = vec![
            StateTransition {
                from: "shown".into(),
                action: ChallengeAction::type_text("abc123", "input"),
                to: "typing".into(),
                confidence: 0.7,
            },
            StateTransition {
                from: "typing".into(),
                action: ChallengeAction::submit(),
                to: "solved".into(),
                confidence: 0.8,
            },
        ];
        (states, transitions, "shown".into())
    }

    fn multi_round_graph(rounds: u8) -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let mut states = vec![ChallengeState {
            id: "shown".into(),
            observable_signature: "round_1_visible".into(),
        }];
        let mut transitions = Vec::new();

        for r in 1..=rounds {
            let round_state = format!("round_{}", r);
            let next_state = if r < rounds {
                format!("round_{}", r + 1)
            } else {
                "solved".to_string()
            };

            if r > 1 {
                states.push(ChallengeState {
                    id: round_state.clone(),
                    observable_signature: format!("round_{}_visible", r),
                });
            }

            transitions.push(StateTransition {
                from: if r == 1 {
                    "shown".into()
                } else {
                    round_state.clone()
                },
                action: ChallengeAction::select_tiles(vec![r - 1]),
                to: next_state.clone(),
                confidence: 0.7,
            });
        }

        states.push(ChallengeState {
            id: "solved".into(),
            observable_signature: "all_rounds_complete".into(),
        });
        (states, transitions, "shown".into())
    }

    fn generic_graph() -> (Vec<ChallengeState>, Vec<StateTransition>, String) {
        let states = vec![
            ChallengeState {
                id: "shown".into(),
                observable_signature: "challenge_visible".into(),
            },
            ChallengeState {
                id: "interacting".into(),
                observable_signature: "user_action".into(),
            },
            ChallengeState {
                id: "solved".into(),
                observable_signature: "complete".into(),
            },
            ChallengeState {
                id: "failed".into(),
                observable_signature: "error".into(),
            },
        ];
        let transitions = vec![
            StateTransition {
                from: "shown".into(),
                action: ChallengeAction::click("challenge"),
                to: "interacting".into(),
                confidence: 0.5,
            },
            StateTransition {
                from: "interacting".into(),
                action: ChallengeAction::submit(),
                to: "solved".into(),
                confidence: 0.5,
            },
            StateTransition {
                from: "interacting".into(),
                action: ChallengeAction::wait(5000),
                to: "failed".into(),
                confidence: 0.2,
            },
        ];
        (states, transitions, "shown".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::captcha::challenge::ChallengeDescriptor;

    fn test_descriptor() -> ChallengeDescriptor {
        ChallengeDescriptor::from_known_provider("hcaptcha", "tile_flip")
    }

    #[test]
    fn checkbox_flow_reaches_solved() {
        let mut sm =
            ChallengeStateMachine::from_archetype(test_descriptor(), &ChallengeArchetype::Checkbox);
        assert_eq!(sm.current_state_id, "shown");
        assert!(!sm.is_terminal());

        let actions = sm.available_actions();
        assert_eq!(actions.len(), 1);

        sm.execute_transition(&ChallengeAction::click("checkbox"));
        assert!(sm.is_solved());
        assert!(sm.is_terminal());
        assert_eq!(sm.step_count, 1);
    }

    #[test]
    fn grid_select_flow() {
        let mut sm = ChallengeStateMachine::from_archetype(
            test_descriptor(),
            &ChallengeArchetype::ImageGridSelect,
        );
        assert_eq!(sm.current_state_id, "shown");

        sm.execute_transition(&ChallengeAction::select_tiles(vec![0, 1, 2]));
        assert_eq!(sm.current_state_id, "selecting");

        sm.execute_transition(&ChallengeAction::submit());
        assert_eq!(sm.current_state_id, "verifying");

        // Wait can go to solved or retry
        let actions = sm.available_actions();
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn tile_flip_flow() {
        let mut sm =
            ChallengeStateMachine::from_archetype(test_descriptor(), &ChallengeArchetype::TileFlip);
        assert_eq!(sm.current_state_id, "shown");

        sm.execute_transition(&ChallengeAction::rotate_tile(0, 90.0));
        assert_eq!(sm.current_state_id, "flipping");

        sm.execute_transition(&ChallengeAction::submit());
        assert_eq!(sm.current_state_id, "verifying");
    }

    #[test]
    fn invalid_transition_rejected() {
        let mut sm =
            ChallengeStateMachine::from_archetype(test_descriptor(), &ChallengeArchetype::Checkbox);
        // Try an action that's not available from "shown"
        let result = sm.execute_transition(&ChallengeAction::submit());
        assert!(!result);
        assert_eq!(sm.current_state_id, "shown");
        assert_eq!(sm.step_count, 0);
    }

    #[test]
    fn reset_returns_to_initial() {
        let mut sm =
            ChallengeStateMachine::from_archetype(test_descriptor(), &ChallengeArchetype::Slider);
        sm.execute_transition(&ChallengeAction::drag_slider(250.0));
        assert_eq!(sm.current_state_id, "dragging");
        assert_eq!(sm.step_count, 1);

        sm.reset();
        assert_eq!(sm.current_state_id, "shown");
        assert_eq!(sm.step_count, 0);
        assert!(sm.history.is_empty());
    }

    #[test]
    fn terminal_state_detection() {
        let mut sm =
            ChallengeStateMachine::from_archetype(test_descriptor(), &ChallengeArchetype::Checkbox);
        assert!(!sm.is_terminal());

        sm.execute_transition(&ChallengeAction::click("checkbox"));
        assert!(sm.is_terminal());
        assert!(sm.is_solved());
    }

    #[test]
    fn solve_sequence_recorded() {
        let mut sm =
            ChallengeStateMachine::from_archetype(test_descriptor(), &ChallengeArchetype::Slider);
        sm.execute_transition(&ChallengeAction::drag_slider(250.0));
        sm.execute_transition(&ChallengeAction::submit());

        let seq = sm.solve_sequence();
        assert_eq!(seq.len(), 2);
        assert_eq!(seq[0].0, "shown");
        assert_eq!(seq[1].0, "dragging");
    }
}
