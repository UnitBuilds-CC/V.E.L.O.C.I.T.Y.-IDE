use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub inline: usize,
    pub ids: usize,
    pub classes_attrs_pseudos: usize,
    pub tags_elements: usize,
}

impl Specificity {
    pub fn new(inline: usize, ids: usize, classes: usize, tags: usize) -> Self {
        Self {
            inline,
            ids,
            classes_attrs_pseudos: classes,
            tags_elements: tags,
        }
    }

    pub fn compute(selector: &str) -> Self {
        let selector = selector.trim();
        let mut ids = 0;
        let mut classes = 0;
        let mut tags = 0;

        for part in selector.split_whitespace() {
            if part.contains('#') {
                ids += part.matches('#').count();
            }
            if part.contains('.') {
                classes += part.matches('.').count();
            }
            if part.contains('[') {
                classes += part.matches('[').count();
            }
            if part.contains(':') {
                classes += part.matches(':').count();
            }
            let clean = part.split('#').next().unwrap_or(part).split('.').next().unwrap_or(part).split('[').next().unwrap_or(part);
            if !clean.is_empty() && !clean.starts_with('#') && !clean.starts_with('.') && !clean.starts_with('[') {
                tags += 1;
            }
        }

        Self::new(0, ids, classes, tags)
    }
}

#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: String,
    pub specificity: Specificity,
    pub declarations: HashMap<String, String>,
}

/// A @media condition to evaluate.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    MinWidth(f64),
    MaxWidth(f64),
    MinHeight(f64),
    MaxHeight(f64),
    Orientation(String),   // "portrait" or "landscape"
    PrefersColorScheme(String), // "dark" or "light"
    PreferReducedMotion,
}

/// A parsed @media query block.
#[derive(Debug, Clone)]
pub struct MediaQuery {
    pub features: Vec<MediaFeature>,
    pub rules: Vec<CssRule>,
}

impl MediaQuery {
    /// Evaluate whether this media query matches given the viewport dimensions.
    pub fn matches(&self, viewport: &ViewportConfig) -> bool {
        self.features.iter().all(|f| match f {
            MediaFeature::MinWidth(w) => viewport.width >= *w,
            MediaFeature::MaxWidth(w) => viewport.width <= *w,
            MediaFeature::MinHeight(h) => viewport.height >= *h,
            MediaFeature::MaxHeight(h) => viewport.height <= *h,
            MediaFeature::Orientation(o) => {
                let actual = if viewport.width >= viewport.height { "landscape" } else { "portrait" };
                actual == o
            }
            MediaFeature::PrefersColorScheme(scheme) => &viewport.color_scheme == scheme,
            MediaFeature::PreferReducedMotion => viewport.prefers_reduced_motion,
        })
    }
}

/// Viewport/environment configuration for media query evaluation.
#[derive(Debug, Clone)]
pub struct ViewportConfig {
    pub width: f64,
    pub height: f64,
    pub color_scheme: String,
    pub prefers_reduced_motion: bool,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            width: 1920.0,
            height: 1080.0,
            color_scheme: "light".to_string(),
            prefers_reduced_motion: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StyleCascader {
    pub rules: Vec<CssRule>,
    pub media_queries: Vec<MediaQuery>,
    pub viewport: ViewportConfig,
}

impl Default for StyleCascader {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleCascader {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            media_queries: Vec::new(),
            viewport: ViewportConfig::default(),
        }
    }

    /// Create with a specific viewport configuration.
    pub fn with_viewport(viewport: ViewportConfig) -> Self {
        Self {
            rules: Vec::new(),
            media_queries: Vec::new(),
            viewport,
        }
    }

    pub fn add_rule(&mut self, selector: &str, declarations: HashMap<String, String>) {
        let spec = Specificity::compute(selector);
        self.rules.push(CssRule {
            selector: selector.to_string(),
            specificity: spec,
            declarations,
        });
    }

    /// Add a @media block with features and rules.
    pub fn add_media_query(&mut self, features: Vec<MediaFeature>, rules: Vec<(String, HashMap<String, String>)>) {
        let css_rules: Vec<CssRule> = rules.into_iter().map(|(sel, decls)| {
            CssRule {
                selector: sel.clone(),
                specificity: Specificity::compute(&sel),
                declarations: decls,
            }
        }).collect();
        self.media_queries.push(MediaQuery { features, rules: css_rules });
    }

    /// Parse a simple media query string like "(min-width: 768px)" into features.
    pub fn parse_media_features(query: &str) -> Vec<MediaFeature> {
        let mut features = Vec::new();
        let query = query.trim();
        // Split on "and" for compound queries
        for part in query.split(" and ") {
            let part = part.trim().trim_start_matches('(').trim_end_matches(')');
            if let Some((key, val)) = part.split_once(':') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "min-width" => {
                        if let Some(px) = parse_px_value(val) {
                            features.push(MediaFeature::MinWidth(px));
                        }
                    }
                    "max-width" => {
                        if let Some(px) = parse_px_value(val) {
                            features.push(MediaFeature::MaxWidth(px));
                        }
                    }
                    "min-height" => {
                        if let Some(px) = parse_px_value(val) {
                            features.push(MediaFeature::MinHeight(px));
                        }
                    }
                    "max-height" => {
                        if let Some(px) = parse_px_value(val) {
                            features.push(MediaFeature::MaxHeight(px));
                        }
                    }
                    "orientation" => {
                        features.push(MediaFeature::Orientation(val.to_string()));
                    }
                    "prefers-color-scheme" => {
                        features.push(MediaFeature::PrefersColorScheme(val.to_string()));
                    }
                    "prefers-reduced-motion" => {
                        if val == "reduce" {
                            features.push(MediaFeature::PreferReducedMotion);
                        }
                    }
                    _ => {}
                }
            }
        }
        features
    }

    /// Compute the final computed style, including @media rules that match.
    pub fn compute_computed_style(&self, selector_match_fn: impl Fn(&str) -> bool) -> HashMap<String, String> {
        let mut computed = HashMap::new();
        let mut applicable_rules: Vec<&CssRule> = self.rules.iter().filter(|r| selector_match_fn(&r.selector)).collect();

        // Also include rules from matching media queries
        for mq in &self.media_queries {
            if mq.matches(&self.viewport) {
                for rule in &mq.rules {
                    if selector_match_fn(&rule.selector) {
                        applicable_rules.push(rule);
                    }
                }
            }
        }

        // Sort rules by specificity ascending so higher specificity overwrites lower
        applicable_rules.sort_by(|a, b| a.specificity.cmp(&b.specificity));

        for rule in applicable_rules {
            for (prop, val) in &rule.declarations {
                computed.insert(prop.clone(), val.clone());
            }
        }

        computed
    }
}

/// Parse a CSS pixel value like "768px" or "1024px".
fn parse_px_value(val: &str) -> Option<f64> {
    let val = val.trim().trim_end_matches("px");
    val.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specificity_computation() {
        let s = Specificity::compute("#main .header a");
        assert_eq!(s.ids, 1);
        assert_eq!(s.classes_attrs_pseudos, 1);
        assert_eq!(s.tags_elements, 1);
    }

    #[test]
    fn media_query_min_width_matches() {
        let mq = MediaQuery {
            features: vec![MediaFeature::MinWidth(768.0)],
            rules: vec![],
        };
        let vp = ViewportConfig { width: 1920.0, height: 1080.0, ..Default::default() };
        assert!(mq.matches(&vp));

        let small_vp = ViewportConfig { width: 600.0, height: 800.0, ..Default::default() };
        assert!(!mq.matches(&small_vp));
    }

    #[test]
    fn media_query_max_width() {
        let mq = MediaQuery {
            features: vec![MediaFeature::MaxWidth(768.0)],
            rules: vec![],
        };
        let small = ViewportConfig { width: 375.0, ..Default::default() };
        assert!(mq.matches(&small));

        let large = ViewportConfig { width: 1024.0, ..Default::default() };
        assert!(!mq.matches(&large));
    }

    #[test]
    fn media_query_orientation() {
        let mq = MediaQuery {
            features: vec![MediaFeature::Orientation("portrait".to_string())],
            rules: vec![],
        };
        let portrait = ViewportConfig { width: 375.0, height: 812.0, ..Default::default() };
        assert!(mq.matches(&portrait));

        let landscape = ViewportConfig { width: 1920.0, height: 1080.0, ..Default::default() };
        assert!(!mq.matches(&landscape));
    }

    #[test]
    fn media_query_prefers_color_scheme() {
        let mq = MediaQuery {
            features: vec![MediaFeature::PrefersColorScheme("dark".to_string())],
            rules: vec![],
        };
        let dark = ViewportConfig { color_scheme: "dark".to_string(), ..Default::default() };
        assert!(mq.matches(&dark));

        let light = ViewportConfig::default();
        assert!(!mq.matches(&light));
    }

    #[test]
    fn cascader_applies_media_query_rules() {
        let mut cascader = StyleCascader::with_viewport(ViewportConfig {
            width: 1920.0,
            height: 1080.0,
            ..Default::default()
        });

        // Base rule
        let mut base_decls = HashMap::new();
        base_decls.insert("font-size".to_string(), "16px".to_string());
        cascader.add_rule(".content", base_decls);

        // Media query that matches (min-width: 1024px)
        let mut mq_decls = HashMap::new();
        mq_decls.insert("font-size".to_string(), "18px".to_string());
        cascader.add_media_query(
            vec![MediaFeature::MinWidth(1024.0)],
            vec![(".content".to_string(), mq_decls)],
        );

        let computed = cascader.compute_computed_style(|s| s == ".content");
        // Media query rule should override because it comes after in cascade
        assert_eq!(computed.get("font-size"), Some(&"18px".to_string()));
    }

    #[test]
    fn cascader_skips_non_matching_media() {
        let mut cascader = StyleCascader::with_viewport(ViewportConfig {
            width: 375.0,
            height: 812.0,
            ..Default::default()
        });

        let mut base_decls = HashMap::new();
        base_decls.insert("display".to_string(), "block".to_string());
        cascader.add_rule(".sidebar", base_decls);

        let mut mq_decls = HashMap::new();
        mq_decls.insert("display".to_string(), "none".to_string());
        // Only hide sidebar on large screens
        cascader.add_media_query(
            vec![MediaFeature::MinWidth(1024.0)],
            vec![(".sidebar".to_string(), mq_decls)],
        );

        let computed = cascader.compute_computed_style(|s| s == ".sidebar");
        assert_eq!(computed.get("display"), Some(&"block".to_string()));
    }

    #[test]
    fn parse_media_features_compound() {
        let features = StyleCascader::parse_media_features("(min-width: 768px) and (max-width: 1024px)");
        assert_eq!(features.len(), 2);
        assert_eq!(features[0], MediaFeature::MinWidth(768.0));
        assert_eq!(features[1], MediaFeature::MaxWidth(1024.0));
    }
}
