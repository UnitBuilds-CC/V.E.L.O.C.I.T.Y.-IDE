use crate::dom::DomTree;

#[derive(Debug, Clone)]
pub struct PredictedActionTarget {
    pub target_selector: String,
    pub confidence_score: f32,
    pub action_type: String,
}

pub struct ActionPredictorEngine;

impl ActionPredictorEngine {
    pub fn predict_next_action(tree: &DomTree) -> Option<PredictedActionTarget> {
        for n in &tree.nodes {
            if n.tag_name == "button" || n.attributes.contains_key("type") {
                return Some(PredictedActionTarget {
                    target_selector: format!("node_{}", n.id),
                    confidence_score: 0.96,
                    action_type: "click".to_string(),
                });
            }
        }
        None
    }
}
