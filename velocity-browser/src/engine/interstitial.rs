use crate::nda::NdaTriple;

#[derive(Debug, Clone, PartialEq)]
pub enum InterstitialKind {
    None,
    CloudflareTurnstile,
    DataDome,
    Akamai,
    Recaptcha,
    AuthRequired,
    SessionExpired,
}

pub struct InterstitialClassifier;

impl InterstitialClassifier {
    pub fn classify_page(title: &str, html_snippet: &str) -> InterstitialKind {
        let text_lower = format!("{} {}", title, html_snippet).to_lowercase();

        if text_lower.contains("just a moment") || text_lower.contains("turnstile") || text_lower.contains("cf-challenge") {
            InterstitialKind::CloudflareTurnstile
        } else if text_lower.contains("datadome") {
            InterstitialKind::DataDome
        } else if text_lower.contains("access denied") || text_lower.contains("akamai") {
            InterstitialKind::Akamai
        } else if text_lower.contains("g-recaptcha") || text_lower.contains("hcaptcha") {
            InterstitialKind::Recaptcha
        } else if text_lower.contains("sign in") || text_lower.contains("log in") {
            InterstitialKind::AuthRequired
        } else if text_lower.contains("session expired") {
            InterstitialKind::SessionExpired
        } else {
            InterstitialKind::None
        }
    }

    pub fn to_nda_triple(url: &str, kind: InterstitialKind) -> NdaTriple {
        let kind_str = match kind {
            InterstitialKind::None => "none",
            InterstitialKind::CloudflareTurnstile => "cloudflare_turnstile",
            InterstitialKind::DataDome => "datadome",
            InterstitialKind::Akamai => "akamai",
            InterstitialKind::Recaptcha => "recaptcha",
            InterstitialKind::AuthRequired => "auth_required",
            InterstitialKind::SessionExpired => "session_expired",
        };
        NdaTriple::new(url, 50, kind_str)
    }
}
