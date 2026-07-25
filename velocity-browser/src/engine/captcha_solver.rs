use crate::dom::DomTree;
use crate::nda::NdaTriple;

#[derive(Debug, Clone, PartialEq)]
pub enum CaptchaType {
    HCaptcha,
    ReCaptchaV2,
    ReCaptchaV3,
    Turnstile,
    FunCaptcha,
    TextCaptcha,
    Unknown,
}

/// Position of a captcha element on the page.
#[derive(Debug, Clone)]
pub struct CaptchaPosition {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// State of a captcha solving attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum SolveState {
    NotStarted,
    Detecting,
    WaitingForSolution,
    Solved,
    Failed { reason: String },
}

/// A solving attempt with metadata.
#[derive(Debug, Clone)]
pub struct SolveAttempt {
    pub captcha_type: CaptchaType,
    pub state: SolveState,
    pub attempts: u32,
    pub elapsed_ms: u64,
    pub position: Option<CaptchaPosition>,
}

pub struct CaptchaSolverEngine;

impl CaptchaSolverEngine {
    pub fn detect_challenge(tree: &DomTree) -> Option<CaptchaType> {
        for n in &tree.nodes {
            if let Some(src) = n.attributes.get("src") {
                if src.contains("hcaptcha.com") {
                    return Some(CaptchaType::HCaptcha);
                }
                if src.contains("recaptcha") && src.contains("enterprise") {
                    return Some(CaptchaType::ReCaptchaV3);
                }
                if src.contains("recaptcha") {
                    return Some(CaptchaType::ReCaptchaV2);
                }
                if src.contains("turnstile") {
                    return Some(CaptchaType::Turnstile);
                }
                if src.contains("funcaptcha") || src.contains("arkoselabs") {
                    return Some(CaptchaType::FunCaptcha);
                }
            }
            // Check for data attributes
            if let Some(_sitekey) = n.attributes.get("data-sitekey") {
                if n.attributes.contains_key("data-callback") {
                    return Some(CaptchaType::ReCaptchaV2);
                }
            }
            // Check for text-based captcha
            if n.tag_name == "img" {
                if let Some(alt) = n.attributes.get("alt") {
                    if alt.to_lowercase().contains("captcha") || alt.to_lowercase().contains("verification") {
                        return Some(CaptchaType::TextCaptcha);
                    }
                }
            }
        }
        None
    }

    /// Detect captcha and return its position on the page.
    pub fn detect_with_position(tree: &DomTree) -> Option<(CaptchaType, CaptchaPosition)> {
        for n in &tree.nodes {
            if let Some(captcha_type) = Self::detect_captcha_from_node(n) {
                // Extract position from style/layout attributes
                let x = n.attributes.get("data-x")
                    .or(n.attributes.get("left"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let y = n.attributes.get("data-y")
                    .or(n.attributes.get("top"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let w = n.attributes.get("data-width")
                    .or(n.attributes.get("width"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(300.0);
                let h = n.attributes.get("data-height")
                    .or(n.attributes.get("height"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(150.0);
                return Some((captcha_type, CaptchaPosition { x, y, width: w, height: h }));
            }
        }
        None
    }

    fn detect_captcha_from_node(node: &crate::parser::html::DomNode) -> Option<CaptchaType> {
        if let Some(src) = node.attributes.get("src") {
            if src.contains("hcaptcha.com") { return Some(CaptchaType::HCaptcha); }
            if src.contains("recaptcha") { return Some(CaptchaType::ReCaptchaV2); }
            if src.contains("turnstile") { return Some(CaptchaType::Turnstile); }
            if src.contains("funcaptcha") || src.contains("arkoselabs") { return Some(CaptchaType::FunCaptcha); }
        }
        None
    }

    /// Create a new solve attempt tracker.
    pub fn start_solve(captcha_type: &CaptchaType) -> SolveAttempt {
        SolveAttempt {
            captcha_type: captcha_type.clone(),
            state: SolveState::Detecting,
            attempts: 0,
            elapsed_ms: 0,
            position: None,
        }
    }

    pub fn solve_challenge_nda(session_id: &str, captcha_type: &CaptchaType) -> Vec<NdaTriple> {
        vec![NdaTriple::new(
            session_id,
            250,
            &format!("captcha_solved:{:?}", captcha_type),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DomTree;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(tag: &str, attrs: &[(&str, &str)]) -> DomNode {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }
        DomNode {
            id: 0,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes,
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn detect_hcaptcha() {
        let node = make_node("iframe", &[("src", "https://hcaptcha.com/1.html")]);
        let tree = DomTree::new(vec![node]);
        assert_eq!(CaptchaSolverEngine::detect_challenge(&tree), Some(CaptchaType::HCaptcha));
    }

    #[test]
    fn detect_recaptcha_v2() {
        let node = make_node("script", &[("src", "https://www.google.com/recaptcha/api.js")]);
        let tree = DomTree::new(vec![node]);
        assert_eq!(CaptchaSolverEngine::detect_challenge(&tree), Some(CaptchaType::ReCaptchaV2));
    }

    #[test]
    fn no_captcha() {
        let tree = DomTree::new(vec![]);
        assert_eq!(CaptchaSolverEngine::detect_challenge(&tree), None);
    }

    #[test]
    fn solve_attempt_tracking() {
        let attempt = CaptchaSolverEngine::start_solve(&CaptchaType::Turnstile);
        assert_eq!(attempt.state, SolveState::Detecting);
        assert_eq!(attempt.attempts, 0);
    }
}
