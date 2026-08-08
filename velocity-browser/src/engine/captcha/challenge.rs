//! Generic challenge model — replaces the hardcoded `CaptchaType` enum with a
//! descriptor that can represent ANY captcha provider and variant.

/// Generic challenge descriptor — works for ANY provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChallengeDescriptor {
    /// Provider identifier (e.g., "hcaptcha", "recaptcha", "turnstile", "datadome").
    pub provider: String,
    /// Variant within the provider (e.g., "tile_flip", "image_select", "checkbox").
    pub variant: String,
    /// Structural fingerprint: grid size, tile count, iframe depth, etc.
    pub features: ChallengeFeatures,
    /// Visual fingerprint from OCR-based pixel analysis (cache key for template store).
    pub visual_hash: u64,
}

/// Structural features extracted from DOM/pixel observation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ChallengeFeatures {
    /// Grid dimensions if applicable (rows, cols).
    pub grid: Option<(u8, u8)>,
    /// Number of interactive tiles/elements.
    pub interactive_elements: u8,
    /// Whether the challenge has an iframe boundary.
    pub iframe_depth: u8,
    /// Number of rounds/phases observed.
    pub round_count: u8,
    /// Presence of specific structural markers (data attributes, class patterns).
    pub markers: Vec<String>,
}

impl ChallengeDescriptor {
    /// Create a descriptor for a known provider/variant pair.
    pub fn from_known_provider(provider: &str, variant: &str) -> Self {
        Self {
            provider: provider.to_string(),
            variant: variant.to_string(),
            features: ChallengeFeatures::default(),
            visual_hash: 0,
        }
    }

    /// Create with a visual hash (from pixel fingerprinting).
    pub fn with_visual_hash(mut self, hash: u64) -> Self {
        self.visual_hash = hash;
        self
    }

    /// Create with features.
    pub fn with_features(mut self, features: ChallengeFeatures) -> Self {
        self.features = features;
        self
    }

    /// Convert to legacy CaptchaType for backward compatibility.
    pub fn to_legacy_type(&self) -> CaptchaType {
        match self.provider.as_str() {
            "hcaptcha" => CaptchaType::HCaptcha,
            "recaptcha" if self.variant.contains("v3") || self.variant.contains("enterprise") => {
                CaptchaType::ReCaptchaV3
            }
            "recaptcha" => CaptchaType::ReCaptchaV2,
            "turnstile" | "cloudflare" => CaptchaType::Turnstile,
            "funcaptcha" | "arkoselabs" => CaptchaType::FunCaptcha,
            _ if self.variant.contains("text") => CaptchaType::TextCaptcha,
            _ => CaptchaType::Unknown,
        }
    }
}

/// Legacy captcha type enum — kept for backward compatibility with existing
/// NDA verb codes and session integration.
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

impl From<&ChallengeDescriptor> for CaptchaType {
    fn from(desc: &ChallengeDescriptor) -> Self {
        desc.to_legacy_type()
    }
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

impl SolveAttempt {
    pub fn new(captcha_type: CaptchaType) -> Self {
        Self {
            captcha_type,
            state: SolveState::Detecting,
            attempts: 0,
            elapsed_ms: 0,
            position: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_construction() {
        let desc = ChallengeDescriptor::from_known_provider("hcaptcha", "tile_flip")
            .with_visual_hash(0xDEADBEEF);
        assert_eq!(desc.provider, "hcaptcha");
        assert_eq!(desc.variant, "tile_flip");
        assert_eq!(desc.visual_hash, 0xDEADBEEF);
    }

    #[test]
    fn feature_hashing_is_deterministic() {
        let f1 = ChallengeFeatures {
            grid: Some((3, 3)),
            interactive_elements: 9,
            iframe_depth: 1,
            round_count: 1,
            markers: vec!["h-captcha".to_string()],
        };
        let f2 = f1.clone();
        assert_eq!(f1, f2);
    }

    #[test]
    fn legacy_type_conversion() {
        let hc = ChallengeDescriptor::from_known_provider("hcaptcha", "tile_flip");
        assert_eq!(hc.to_legacy_type(), CaptchaType::HCaptcha);

        let rc3 = ChallengeDescriptor::from_known_provider("recaptcha", "enterprise_v3");
        assert_eq!(rc3.to_legacy_type(), CaptchaType::ReCaptchaV3);

        let rc2 = ChallengeDescriptor::from_known_provider("recaptcha", "image_select");
        assert_eq!(rc2.to_legacy_type(), CaptchaType::ReCaptchaV2);

        let ts = ChallengeDescriptor::from_known_provider("turnstile", "managed");
        assert_eq!(ts.to_legacy_type(), CaptchaType::Turnstile);

        let unk = ChallengeDescriptor::from_known_provider("newprovider", "slider");
        assert_eq!(unk.to_legacy_type(), CaptchaType::Unknown);
    }

    #[test]
    fn descriptor_equality_and_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let d1 = ChallengeDescriptor::from_known_provider("hcaptcha", "tile_flip")
            .with_visual_hash(42);
        let d2 = ChallengeDescriptor::from_known_provider("hcaptcha", "tile_flip")
            .with_visual_hash(42);
        assert_eq!(d1, d2);

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        d1.hash(&mut h1);
        d2.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn funcaptcha_and_arkoselabs_legacy_types() {
        let fa = ChallengeDescriptor::from_known_provider("funcaptcha", "spin");
        assert_eq!(fa.to_legacy_type(), CaptchaType::FunCaptcha);
        let ark = ChallengeDescriptor::from_known_provider("arkoselabs", "game");
        assert_eq!(ark.to_legacy_type(), CaptchaType::FunCaptcha);
    }

    #[test]
    fn cloudflare_maps_to_turnstile() {
        let cf = ChallengeDescriptor::from_known_provider("cloudflare", "managed");
        assert_eq!(cf.to_legacy_type(), CaptchaType::Turnstile);
    }

    #[test]
    fn text_variant_fallback() {
        let tc = ChallengeDescriptor::from_known_provider("custom", "text_challenge");
        assert_eq!(tc.to_legacy_type(), CaptchaType::TextCaptcha);
    }

    #[test]
    fn with_features_builder() {
        let features = ChallengeFeatures {
            grid: Some((4, 4)),
            interactive_elements: 16,
            iframe_depth: 2,
            round_count: 3,
            markers: vec!["data-hcaptcha".to_string()],
        };
        let desc = ChallengeDescriptor::from_known_provider("hcaptcha", "grid")
            .with_features(features.clone());
        assert_eq!(desc.features.grid, Some((4, 4)));
        assert_eq!(desc.features.interactive_elements, 16);
        assert_eq!(desc.features.markers.len(), 1);
    }

    #[test]
    fn solve_attempt_starts_in_detecting_state() {
        let attempt = SolveAttempt::new(CaptchaType::HCaptcha);
        assert_eq!(attempt.state, SolveState::Detecting);
        assert_eq!(attempt.attempts, 0);
        assert_eq!(attempt.elapsed_ms, 0);
        assert!(attempt.position.is_none());
    }

    #[test]
    fn from_trait_matches_to_legacy() {
        let desc = ChallengeDescriptor::from_known_provider("recaptcha", "v3_score");
        let ct: CaptchaType = (&desc).into();
        assert_eq!(ct, desc.to_legacy_type());
    }

    #[test]
    fn default_features_are_zero() {
        let f = ChallengeFeatures::default();
        assert_eq!(f.grid, None);
        assert_eq!(f.interactive_elements, 0);
        assert_eq!(f.iframe_depth, 0);
        assert_eq!(f.round_count, 0);
        assert!(f.markers.is_empty());
    }
}
