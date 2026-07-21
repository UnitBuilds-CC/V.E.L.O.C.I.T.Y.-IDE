use crate::dom::DomTree;
use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub enum CaptchaType {
    HCaptcha,
    ReCaptchaV2,
    Turnstile,
    Unknown,
}

pub struct CaptchaSolverEngine;

impl CaptchaSolverEngine {
    pub fn detect_challenge(tree: &DomTree) -> Option<CaptchaType> {
        for n in &tree.nodes {
            if let Some(src) = n.attributes.get("src") {
                if src.contains("hcaptcha.com") {
                    return Some(CaptchaType::HCaptcha);
                }
                if src.contains("recaptcha") {
                    return Some(CaptchaType::ReCaptchaV2);
                }
                if src.contains("turnstile") {
                    return Some(CaptchaType::Turnstile);
                }
            }
        }
        None
    }

    pub fn solve_challenge_nda(session_id: &str, captcha_type: &CaptchaType) -> Vec<NdaTriple> {
        vec![NdaTriple::new(
            session_id,
            250,
            &format!("captcha_solved:{:?}", captcha_type),
        )]
    }
}
