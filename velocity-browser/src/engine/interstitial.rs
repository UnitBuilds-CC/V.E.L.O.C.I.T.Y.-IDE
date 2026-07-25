use crate::nda::NdaTriple;

#[derive(Debug, Clone, PartialEq)]
pub enum InterstitialKind {
    None,
    CloudflareTurnstile,
    DataDome,
    Akamai,
    Recaptcha,
    Hcaptcha,
    AuthRequired,
    SessionExpired,
    RateLimited,
    GeoBlocked,
    WafChallenge,
}

/// Strategy to attempt when bypassing an interstitial.
#[derive(Debug, Clone)]
pub enum BypassStrategy {
    /// Wait for a JS challenge to auto-resolve.
    WaitAndRetry { delay_ms: u64, max_retries: u32 },
    /// Solve a CAPTCHA challenge externally.
    SolveCaptcha,
    /// Rotate headers/cookies to avoid detection.
    RotateFingerprint,
    /// No bypass possible.
    GiveUp,
}

/// Result of classifying a page.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub kind: InterstitialKind,
    pub confidence: f32,
    pub suggested_strategy: BypassStrategy,
    pub signals: Vec<String>,
}

pub struct InterstitialClassifier;

impl InterstitialClassifier {
    pub fn classify_page(title: &str, html_snippet: &str) -> InterstitialKind {
        Self::classify_page_with_signals(title, html_snippet).kind
    }

    /// Full classification with confidence and signal tracking.
    pub fn classify_page_with_signals(title: &str, html_snippet: &str) -> ClassificationResult {
        let text_lower = format!("{} {}", title, html_snippet).to_lowercase();
        let mut signals = Vec::new();
        let mut score: f32 = 0.0;
        let mut kind = InterstitialKind::None;

        // Cloudflare Turnstile
        if text_lower.contains("just a moment") || text_lower.contains("turnstile") || text_lower.contains("cf-challenge") {
            signals.push("cf_challenge_detected".into());
            score += 0.9;
            if kind == InterstitialKind::None { kind = InterstitialKind::CloudflareTurnstile; }
        }
        if text_lower.contains("cloudflare") { signals.push("cloudflare_brand".into()); score += 0.1; }

        // DataDome
        if text_lower.contains("datadome") || text_lower.contains("blocked") && text_lower.contains("datadome") {
            signals.push("datadome_detected".into());
            score += 0.9;
            if kind == InterstitialKind::None { kind = InterstitialKind::DataDome; }
        }

        // Akamai
        if text_lower.contains("access denied") || text_lower.contains("akamai") {
            signals.push("akamai_detected".into());
            score += 0.8;
            if kind == InterstitialKind::None { kind = InterstitialKind::Akamai; }
        }

        // reCAPTCHA / hCaptcha
        if text_lower.contains("g-recaptcha") {
            signals.push("recaptcha_iframe".into());
            score += 0.95;
            if kind == InterstitialKind::None { kind = InterstitialKind::Recaptcha; }
        }
        if text_lower.contains("hcaptcha") {
            signals.push("hcaptcha_iframe".into());
            score += 0.95;
            if kind == InterstitialKind::None { kind = InterstitialKind::Hcaptcha; }
        }

        // Auth required
        if text_lower.contains("sign in") || text_lower.contains("log in") {
            signals.push("auth_page".into());
            score += 0.7;
            if kind == InterstitialKind::None { kind = InterstitialKind::AuthRequired; }
        }

        // Session expired
        if text_lower.contains("session expired") || text_lower.contains("your session has timed out") {
            signals.push("session_expired".into());
            score += 0.85;
            if kind == InterstitialKind::None { kind = InterstitialKind::SessionExpired; }
        }

        // Rate limited
        if text_lower.contains("rate limit") || text_lower.contains("too many requests") || text_lower.contains("429") {
            signals.push("rate_limited".into());
            score += 0.8;
            if kind == InterstitialKind::None { kind = InterstitialKind::RateLimited; }
        }

        // Geo-blocked
        if text_lower.contains("not available in your region") || text_lower.contains("geo-blocked") {
            signals.push("geo_blocked".into());
            score += 0.8;
            if kind == InterstitialKind::None { kind = InterstitialKind::GeoBlocked; }
        }

        // Generic WAF
        if text_lower.contains("web application firewall") || text_lower.contains("waf") && text_lower.contains("blocked") {
            signals.push("waf_detected".into());
            score += 0.7;
            if kind == InterstitialKind::None { kind = InterstitialKind::WafChallenge; }
        }

        let strategy = match &kind {
            InterstitialKind::None => BypassStrategy::GiveUp,
            InterstitialKind::CloudflareTurnstile | InterstitialKind::DataDome | InterstitialKind::WafChallenge => {
                BypassStrategy::WaitAndRetry { delay_ms: 5000, max_retries: 3 }
            }
            InterstitialKind::Recaptcha | InterstitialKind::Hcaptcha => BypassStrategy::SolveCaptcha,
            InterstitialKind::RateLimited => BypassStrategy::WaitAndRetry { delay_ms: 30000, max_retries: 2 },
            InterstitialKind::Akamai | InterstitialKind::GeoBlocked => BypassStrategy::RotateFingerprint,
            InterstitialKind::AuthRequired | InterstitialKind::SessionExpired => BypassStrategy::GiveUp,
        };

        ClassificationResult {
            kind,
            confidence: score.min(1.0),
            suggested_strategy: strategy,
            signals,
        }
    }

    pub fn to_nda_triple(url: &str, kind: InterstitialKind) -> NdaTriple {
        let kind_str = match kind {
            InterstitialKind::None => "none",
            InterstitialKind::CloudflareTurnstile => "cloudflare_turnstile",
            InterstitialKind::DataDome => "datadome",
            InterstitialKind::Akamai => "akamai",
            InterstitialKind::Recaptcha => "recaptcha",
            InterstitialKind::Hcaptcha => "hcaptcha",
            InterstitialKind::AuthRequired => "auth_required",
            InterstitialKind::SessionExpired => "session_expired",
            InterstitialKind::RateLimited => "rate_limited",
            InterstitialKind::GeoBlocked => "geo_blocked",
            InterstitialKind::WafChallenge => "waf_challenge",
        };
        NdaTriple::new(url, 50, kind_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_cloudflare() {
        let result = InterstitialClassifier::classify_page_with_signals(
            "Just a moment...", "<div class=\"cf-challenge\"></div>"
        );
        assert_eq!(result.kind, InterstitialKind::CloudflareTurnstile);
        assert!(result.confidence > 0.5);
    }

    #[test]
    fn classify_recaptcha() {
        let result = InterstitialClassifier::classify_page("Verify", "<div class=\"g-recaptcha\"></div>");
        assert_eq!(result, InterstitialKind::Recaptcha);
    }

    #[test]
    fn classify_rate_limited() {
        let result = InterstitialClassifier::classify_page("Error", "429 Too Many Requests");
        assert_eq!(result, InterstitialKind::RateLimited);
    }

    #[test]
    fn classify_none() {
        let result = InterstitialClassifier::classify_page("Home", "<p>Welcome to our site</p>");
        assert_eq!(result, InterstitialKind::None);
    }
}
