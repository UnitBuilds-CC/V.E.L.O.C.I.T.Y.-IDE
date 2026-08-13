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
                    if alt.to_lowercase().contains("captcha")
                        || alt.to_lowercase().contains("verification")
                    {
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
                let x = n
                    .attributes
                    .get("data-x")
                    .or(n.attributes.get("left"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let y = n
                    .attributes
                    .get("data-y")
                    .or(n.attributes.get("top"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let w = n
                    .attributes
                    .get("data-width")
                    .or(n.attributes.get("width"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(300.0);
                let h = n
                    .attributes
                    .get("data-height")
                    .or(n.attributes.get("height"))
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(150.0);
                return Some((
                    captcha_type,
                    CaptchaPosition {
                        x,
                        y,
                        width: w,
                        height: h,
                    },
                ));
            }
        }
        None
    }

    fn detect_captcha_from_node(node: &crate::parser::html::DomNode) -> Option<CaptchaType> {
        if let Some(src) = node.attributes.get("src") {
            if src.contains("hcaptcha.com") {
                return Some(CaptchaType::HCaptcha);
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
        let mut triples = Vec::new();

        // Record the captcha detection event
        triples.push(NdaTriple::new(
            session_id,
            250,
            &format!("captcha_detected:{:?}", captcha_type),
        ));

        // Type-specific solving strategy
        match captcha_type {
            CaptchaType::ReCaptchaV3 => {
                // ReCaptchaV3 is invisible/score-based — submit the form and let the
                // token be generated automatically. No user interaction needed.
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:auto_submit",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    252,
                    "captcha_action:submit_form_for_token",
                ));
            }
            CaptchaType::ReCaptchaV2 => {
                // ReCaptchaV2 requires clicking the checkbox and potentially solving image challenges
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:click_checkbox",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    252,
                    "captcha_action:click_recaptcha_checkbox",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    253,
                    "captcha_wait:iframe_result",
                ));
            }
            CaptchaType::HCaptcha => {
                // hCaptcha is similar to ReCaptchaV2 but uses a different provider
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:hcaptcha_click",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    252,
                    "captcha_action:click_hcaptcha_checkbox",
                ));
            }
            CaptchaType::Turnstile => {
                // Cloudflare Turnstile is typically invisible — wait for the token
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:wait_for_turnstile_token",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    252,
                    "captcha_action:wait_cf_clearance",
                ));
            }
            CaptchaType::FunCaptcha => {
                // FunCaptcha requires solving visual puzzles
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:funcaptcha_puzzle",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    252,
                    "captcha_action:analyze_funcaptcha_frame",
                ));
            }
            CaptchaType::TextCaptcha => {
                // Text captcha requires OCR or image recognition
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:ocr_text_recognition",
                ));
                triples.push(NdaTriple::new(
                    session_id,
                    252,
                    "captcha_action:extract_text_from_image",
                ));
            }
            CaptchaType::Unknown => {
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    "captcha_strategy:unknown_fallback",
                ));
            }
        }

        triples.push(NdaTriple::new(
            session_id,
            260,
            &format!("captcha_solving:{:?}", captcha_type),
        ));

        triples
    }

    /// Update a solve attempt's state based on observed results.
    pub fn update_solve_state(attempt: &mut SolveAttempt, success: bool) {
        attempt.attempts += 1;
        if success {
            attempt.state = SolveState::Solved;
        } else if attempt.attempts >= 3 {
            attempt.state = SolveState::Failed {
                reason: format!("Failed after {} attempts", attempt.attempts),
            };
        } else {
            attempt.state = SolveState::WaitingForSolution;
        }
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
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::HCaptcha)
        );
    }

    #[test]
    fn detect_recaptcha_v2() {
        let node = make_node(
            "script",
            &[("src", "https://www.google.com/recaptcha/api.js")],
        );
        let tree = DomTree::new(vec![node]);
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::ReCaptchaV2)
        );
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

    #[test]
    fn detect_recaptcha_v3() {
        let node = make_node(
            "script",
            &[("src", "https://www.google.com/recaptcha/enterprise.js")],
        );
        let tree = DomTree::new(vec![node]);
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::ReCaptchaV3)
        );
    }

    #[test]
    fn detect_turnstile() {
        let node = make_node(
            "script",
            &[(
                "src",
                "https://challenges.cloudflare.com/turnstile/v0/api.js",
            )],
        );
        let tree = DomTree::new(vec![node]);
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::Turnstile)
        );
    }

    #[test]
    fn detect_funcaptcha() {
        let node = make_node("script", &[("src", "https://api.funcaptcha.com/fc.js")]);
        let tree = DomTree::new(vec![node]);
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::FunCaptcha)
        );
    }

    #[test]
    fn detect_text_captcha_by_alt() {
        let node = make_node("img", &[("alt", "Enter the captcha text")]);
        let tree = DomTree::new(vec![node]);
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::TextCaptcha)
        );
    }

    #[test]
    fn detect_recaptcha_by_data_attrs() {
        let node = make_node(
            "div",
            &[("data-sitekey", "abc123"), ("data-callback", "onSubmit")],
        );
        let tree = DomTree::new(vec![node]);
        assert_eq!(
            CaptchaSolverEngine::detect_challenge(&tree),
            Some(CaptchaType::ReCaptchaV2)
        );
    }

    #[test]
    fn update_solve_success() {
        let mut attempt = CaptchaSolverEngine::start_solve(&CaptchaType::HCaptcha);
        CaptchaSolverEngine::update_solve_state(&mut attempt, true);
        assert_eq!(attempt.state, SolveState::Solved);
        assert_eq!(attempt.attempts, 1);
    }

    #[test]
    fn update_solve_failure_after_three() {
        let mut attempt = CaptchaSolverEngine::start_solve(&CaptchaType::ReCaptchaV2);
        CaptchaSolverEngine::update_solve_state(&mut attempt, false);
        assert_eq!(attempt.state, SolveState::WaitingForSolution);
        CaptchaSolverEngine::update_solve_state(&mut attempt, false);
        assert_eq!(attempt.state, SolveState::WaitingForSolution);
        CaptchaSolverEngine::update_solve_state(&mut attempt, false);
        // Third failure triggers Failed state
        match &attempt.state {
            SolveState::Failed { reason } => assert!(reason.contains("3")),
            _ => panic!("Expected Failed state"),
        }
    }

    #[test]
    fn solve_challenge_nda_recaptcha_v3() {
        let triples = CaptchaSolverEngine::solve_challenge_nda("sess", &CaptchaType::ReCaptchaV3);
        // detection + strategy + action + solving
        assert!(triples.len() >= 3);
        assert_eq!(triples[0].predicate_id, 250);
    }

    #[test]
    fn solve_challenge_nda_unknown() {
        let triples = CaptchaSolverEngine::solve_challenge_nda("sess", &CaptchaType::Unknown);
        // detection + unknown strategy + solving
        assert_eq!(triples.len(), 3);
    }
}
