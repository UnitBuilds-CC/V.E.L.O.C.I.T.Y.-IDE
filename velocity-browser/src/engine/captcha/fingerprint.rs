//! Provider fingerprinting — identifies captcha providers from structural signals.
//!
//! Uses URL patterns, container class patterns, script sources, and data
//! attributes to identify the captcha provider. Works alongside the visual
//! fingerprinter: visual = "what does it look like", provider = "who made it".

use super::challenge::{ChallengeDescriptor, ChallengeFeatures};
use super::observer::ChallengeSnapshot;

/// A provider signature — patterns that identify a specific captcha provider.
#[derive(Debug, Clone)]
pub struct ProviderSignature {
    /// Provider identifier string.
    pub provider: &'static str,
    /// URL patterns found in iframe src or script src.
    pub url_patterns: Vec<&'static str>,
    /// CSS class patterns on the container element.
    pub container_patterns: Vec<&'static str>,
    /// Script source patterns.
    pub script_patterns: Vec<&'static str>,
    /// Data attribute patterns (key, value_prefix).
    pub data_attr_patterns: Vec<(&'static str, &'static str)>,
}

impl ProviderSignature {
    /// Score how well a set of signals matches this provider (0.0 - 1.0).
    fn score(
        &self,
        urls: &[String],
        classes: &[String],
        scripts: &[String],
        data_attrs: &[(String, String)],
    ) -> f32 {
        let mut hits = 0u32;
        let mut total = 0u32;

        // URL matching (high weight)
        for pattern in &self.url_patterns {
            total += 2;
            if urls.iter().any(|u| u.contains(pattern)) {
                hits += 2;
            }
        }

        // Container class matching
        for pattern in &self.container_patterns {
            total += 1;
            if classes.iter().any(|c| c.contains(pattern)) {
                hits += 1;
            }
        }

        // Script source matching
        for pattern in &self.script_patterns {
            total += 1;
            if scripts.iter().any(|s| s.contains(pattern)) {
                hits += 1;
            }
        }

        // Data attribute matching
        for (key, prefix) in &self.data_attr_patterns {
            total += 1;
            if data_attrs
                .iter()
                .any(|(k, v)| k == key && v.starts_with(prefix))
            {
                hits += 1;
            }
        }

        if total == 0 {
            return 0.0;
        }
        hits as f32 / total as f32
    }
}

/// Known provider signatures.
pub fn known_providers() -> Vec<ProviderSignature> {
    vec![
        ProviderSignature {
            provider: "hcaptcha",
            url_patterns: vec!["hcaptcha.com", "h-captcha.com"],
            container_patterns: vec!["h-captcha", "hcaptcha"],
            script_patterns: vec!["hcaptcha.com/1/api.js"],
            data_attr_patterns: vec![("data-sitekey", ""), ("data-callback", "")],
        },
        ProviderSignature {
            provider: "recaptcha",
            url_patterns: vec!["recaptcha", "google.com/recaptcha"],
            container_patterns: vec!["g-recaptcha", "recaptcha"],
            script_patterns: vec!["recaptcha/api.js", "recaptcha/enterprise.js"],
            data_attr_patterns: vec![("data-sitekey", ""), ("data-badge", "")],
        },
        ProviderSignature {
            provider: "turnstile",
            url_patterns: vec!["turnstile", "challenges.cloudflare.com"],
            container_patterns: vec!["cf-turnstile", "turnstile"],
            script_patterns: vec!["challenges.cloudflare.com/turnstile"],
            data_attr_patterns: vec![("data-sitekey", ""), ("data-action", "")],
        },
        ProviderSignature {
            provider: "funcaptcha",
            url_patterns: vec!["funcaptcha.com", "arkoselabs.com"],
            container_patterns: vec!["funcaptcha", "arkose"],
            script_patterns: vec!["funcaptcha.com/api", "arkoselabs.com/v2"],
            data_attr_patterns: vec![("data-pkey", "")],
        },
        ProviderSignature {
            provider: "datadome",
            url_patterns: vec!["datadome.co", "geo.captcha-delivery.com"],
            container_patterns: vec!["datadome", "dd-captcha"],
            script_patterns: vec!["datadome.co/captcha"],
            data_attr_patterns: vec![],
        },
    ]
}

/// The provider fingerprinter — identifies which provider serves a challenge.
pub struct ProviderFingerprinter {
    signatures: Vec<ProviderSignature>,
}

impl Default for ProviderFingerprinter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderFingerprinter {
    pub fn new() -> Self {
        Self {
            signatures: known_providers(),
        }
    }

    /// Register a custom provider signature (for dynamic learning).
    pub fn register_provider(&mut self, sig: ProviderSignature) {
        self.signatures.push(sig);
    }

    /// Identify the provider from raw signals.
    pub fn identify(
        &self,
        urls: &[String],
        classes: &[String],
        scripts: &[String],
        data_attrs: &[(String, String)],
    ) -> Option<(String, f32)> {
        let mut best: Option<(String, f32)> = None;

        for sig in &self.signatures {
            let score = sig.score(urls, classes, scripts, data_attrs);
            if score > 0.0 {
                if let Some((_, best_score)) = &best {
                    if score > *best_score {
                        best = Some((sig.provider.to_string(), score));
                    }
                } else {
                    best = Some((sig.provider.to_string(), score));
                }
            }
        }

        best
    }

    /// Identify provider from a challenge snapshot (DOM observation).
    pub fn identify_from_snapshot(&self, snapshot: &ChallengeSnapshot) -> Option<(String, f32)> {
        let urls = Vec::new();
        let mut classes = Vec::new();
        let scripts = Vec::new();
        let mut data_attrs = Vec::new();

        // Extract signals from interactive elements
        for elem in &snapshot.interactive_elements {
            classes.extend(elem.classes.clone());
        }

        // Extract markers as data attributes
        for marker in &snapshot.structural_markers {
            if let Some(stripped) = marker.strip_prefix("class:") {
                classes.push(stripped.to_string());
            } else {
                data_attrs.push((marker.clone(), String::new()));
            }
        }

        self.identify(&urls, &classes, &scripts, &data_attrs)
    }

    /// Build a full ChallengeDescriptor from identification + visual hash + features.
    pub fn build_descriptor(
        &self,
        provider: &str,
        visual_hash: u64,
        features: ChallengeFeatures,
    ) -> ChallengeDescriptor {
        let variant = self.infer_variant(provider, &features);
        ChallengeDescriptor {
            provider: provider.to_string(),
            variant,
            features,
            visual_hash,
        }
    }

    /// Infer the variant from provider + features.
    fn infer_variant(&self, provider: &str, features: &ChallengeFeatures) -> String {
        match provider {
            "hcaptcha" => {
                if features.grid.is_some() {
                    if features.round_count > 1 {
                        "tile_flip_multi".to_string()
                    } else {
                        "tile_flip".to_string()
                    }
                } else {
                    "checkbox".to_string()
                }
            }
            "recaptcha" => {
                if features.grid == Some((3, 3)) || features.grid == Some((4, 4)) {
                    "image_select".to_string()
                } else if features.interactive_elements <= 1 {
                    "checkbox".to_string()
                } else {
                    "enterprise".to_string()
                }
            }
            "turnstile" => "managed".to_string(),
            "funcaptcha" => "puzzle".to_string(),
            "datadome" => "delivery".to_string(),
            _ => "unknown".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_hcaptcha() {
        let fp = ProviderFingerprinter::new();
        let urls = vec!["https://hcaptcha.com/1/api2/anchor".to_string()];
        let classes = vec!["h-captcha".to_string()];
        let result = fp.identify(&urls, &classes, &[], &[]);
        assert!(result.is_some());
        let (provider, score) = result.unwrap();
        assert_eq!(provider, "hcaptcha");
        assert!(score > 0.3, "score should be > 0.3, got {}", score);
    }

    #[test]
    fn identify_recaptcha() {
        let fp = ProviderFingerprinter::new();
        let scripts = vec!["https://www.google.com/recaptcha/api.js".to_string()];
        let classes = vec!["g-recaptcha".to_string()];
        let result = fp.identify(&[], &classes, &scripts, &[]);
        assert!(result.is_some());
        let (provider, _) = result.unwrap();
        assert_eq!(provider, "recaptcha");
    }

    #[test]
    fn identify_turnstile() {
        let fp = ProviderFingerprinter::new();
        let classes = vec!["cf-turnstile".to_string()];
        let result = fp.identify(&[], &classes, &[], &[]);
        assert!(result.is_some());
        let (provider, _) = result.unwrap();
        assert_eq!(provider, "turnstile");
    }

    #[test]
    fn identify_from_snapshot() {
        let fp = ProviderFingerprinter::new();
        let snapshot = ChallengeSnapshot {
            container_node_id: 0,
            interactive_elements: vec![],
            grid_layout: None,
            instruction_text: None,
            iframe_boundaries: vec![],
            structural_markers: vec!["class:h-captcha".to_string()],
            canvas_elements: vec![],
        };
        let result = fp.identify_from_snapshot(&snapshot);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "hcaptcha");
    }

    #[test]
    fn unknown_provider_fallback() {
        let fp = ProviderFingerprinter::new();
        let result = fp.identify(&[], &["random-class".to_string()], &[], &[]);
        assert!(result.is_none());
    }

    #[test]
    fn build_descriptor_with_variant() {
        let fp = ProviderFingerprinter::new();
        let features = ChallengeFeatures {
            grid: Some((3, 3)),
            interactive_elements: 9,
            iframe_depth: 1,
            round_count: 1,
            markers: vec![],
        };
        let desc = fp.build_descriptor("hcaptcha", 0xABCD, features);
        assert_eq!(desc.provider, "hcaptcha");
        assert_eq!(desc.variant, "tile_flip");
        assert_eq!(desc.visual_hash, 0xABCD);
    }
}
