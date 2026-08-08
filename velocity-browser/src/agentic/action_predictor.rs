use crate::dom::DomTree;
use super::adaptive_confidence::AdaptiveConfidence;

#[derive(Debug, Clone)]
pub struct PredictedActionTarget {
    pub target_selector: String,
    pub confidence_score: f32,
    pub action_type: String,
}

pub struct ActionPredictorEngine;

impl ActionPredictorEngine {
    /// Predict the next best action using hardcoded heuristic (legacy).
    pub fn predict_next_action(tree: &DomTree) -> Option<PredictedActionTarget> {
        Self::predict_with_confidence(tree, &AdaptiveConfidence::new(), "unknown")
    }

    /// Predict using learned adaptive confidence scores.
    pub fn predict_with_confidence(
        tree: &DomTree,
        confidence: &AdaptiveConfidence,
        domain: &str,
    ) -> Option<PredictedActionTarget> {
        let mut best: Option<PredictedActionTarget> = None;
        let mut best_score: f64 = 0.0;

        for n in &tree.nodes {
            let (role, action) = if n.tag_name == "button" {
                ("button", "click")
            } else if n.tag_name == "a" {
                ("link", "click")
            } else if n.tag_name == "input" {
                let input_type = n.attributes.get("type").map(|s| s.as_str()).unwrap_or("text");
                match input_type {
                    "submit" => ("button", "click"),
                    "text" | "email" | "password" | "search" | "tel" | "url" => ("textbox", "fill"),
                    _ => continue,
                }
            } else if n.tag_name == "textarea" {
                ("textbox", "fill")
            } else if n.tag_name == "select" {
                ("combobox", "select")
            } else if n.attributes.contains_key("onclick") || n.attributes.contains_key("role") {
                ("interactive", "click")
            } else {
                continue;
            };

            let text = n.attributes.get("value")
                .or_else(|| n.attributes.get("aria-label"))
                .map(|s| s.as_str())
                .unwrap_or("");

            let score = confidence.predict_with_text_hint(role, action, domain, text);
            if score > best_score {
                best_score = score;
                best = Some(PredictedActionTarget {
                    target_selector: format!("node_{}", n.id),
                    confidence_score: score as f32,
                    action_type: action.to_string(),
                });
            }
        }

        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_dom(tags: &[(&str, Vec<(&str, &str)>)]) -> DomTree {
        let mut tree = DomTree { nodes: Vec::new() };
        for (i, (tag, attrs)) in tags.iter().enumerate() {
            let mut attr_map = HashMap::new();
            for (k, v) in attrs {
                attr_map.insert(k.to_string(), v.to_string());
            }
            tree.nodes.push(DomNode {
                id: i,
                parent: None,
                children: Vec::new(),
                node_type: NodeType::Element,
                tag_name: tag.to_string(),
                attributes: attr_map,
                text_content: String::new(),
            });
        }
        tree
    }

    #[test]
    fn predicts_button_with_adaptive_confidence() {
        let tree = make_dom(&[("button", vec![("value", "Submit")])]);  
        let mut ac = AdaptiveConfidence::new();
        // Train high confidence for buttons
        for _ in 0..5 {
            ac.record("button", "click", "example.com", 0.95);
        }
        let pred = ActionPredictorEngine::predict_with_confidence(&tree, &ac, "example.com");
        assert!(pred.is_some());
        let p = pred.unwrap();
        assert!(p.confidence_score > 0.85);
        assert_eq!(p.action_type, "click");
    }

    #[test]
    fn prefers_higher_confidence_target() {
        let tree = make_dom(&[
            ("a", vec![]),
            ("button", vec![("value", "Login")]),
        ]);
        let mut ac = AdaptiveConfidence::new();
        // Buttons succeed more than links on this domain
        for _ in 0..5 {
            ac.record("button", "click", "site.com", 0.9);
            ac.record("link", "click", "site.com", 0.3);
        }
        let pred = ActionPredictorEngine::predict_with_confidence(&tree, &ac, "site.com");
        assert!(pred.is_some());
        let p = pred.unwrap();
        assert!(p.target_selector.contains("1")); // button is index 1
    }

    #[test]
    fn predicts_text_input_fill() {
        let tree = make_dom(&[("input", vec![("type", "text"), ("aria-label", "Name")])]);
        let pred = ActionPredictorEngine::predict_next_action(&tree);
        assert!(pred.is_some());
        assert_eq!(pred.unwrap().action_type, "fill");
    }

    #[test]
    fn predicts_textarea_fill() {
        let tree = make_dom(&[("textarea", vec![])]);
        let pred = ActionPredictorEngine::predict_next_action(&tree);
        assert!(pred.is_some());
        assert_eq!(pred.unwrap().action_type, "fill");
    }

    #[test]
    fn predicts_select_action() {
        let tree = make_dom(&[("select", vec![])]);
        let pred = ActionPredictorEngine::predict_next_action(&tree);
        assert!(pred.is_some());
        assert_eq!(pred.unwrap().action_type, "select");
    }

    #[test]
    fn empty_tree_no_prediction() {
        let tree = DomTree { nodes: Vec::new() };
        let pred = ActionPredictorEngine::predict_next_action(&tree);
        assert!(pred.is_none());
    }

    #[test]
    fn non_interactive_elements_ignored() {
        let tree = make_dom(&[("div", vec![]), ("span", vec![]), ("p", vec![])]);
        let pred = ActionPredictorEngine::predict_next_action(&tree);
        assert!(pred.is_none());
    }

    #[test]
    fn onclick_attribute_detected() {
        let tree = make_dom(&[("div", vec![("onclick", "doStuff()")])]);
        let pred = ActionPredictorEngine::predict_next_action(&tree);
        assert!(pred.is_some());
        assert_eq!(pred.unwrap().action_type, "click");
    }
}
