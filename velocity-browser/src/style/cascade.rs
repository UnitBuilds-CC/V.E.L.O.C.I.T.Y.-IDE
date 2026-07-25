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

// ═══════════════════════════════════════════════════════════════════════════
// CSS Animations (@keyframes)
// ═══════════════════════════════════════════════════════════════════════════

/// Timing function for animation easing.
#[derive(Debug, Clone, PartialEq)]
pub enum TimingFunction {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f64, f64, f64, f64),
    Steps(i32, StepPosition),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepPosition {
    Start,
    End,
}

impl TimingFunction {
    /// Parse a CSS timing function string.
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "linear" => Self::Linear,
            "ease" => Self::Ease,
            "ease-in" => Self::EaseIn,
            "ease-out" => Self::EaseOut,
            "ease-in-out" => Self::EaseInOut,
            s if s.starts_with("cubic-bezier(") => {
                let inner = s.trim_start_matches("cubic-bezier(").trim_end_matches(')');
                let parts: Vec<f64> = inner.split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect();
                if parts.len() == 4 {
                    Self::CubicBezier(parts[0], parts[1], parts[2], parts[3])
                } else {
                    Self::Ease
                }
            }
            s if s.starts_with("steps(") => {
                let inner = s.trim_start_matches("steps(").trim_end_matches(')');
                let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
                let count = parts.first().and_then(|p| p.parse().ok()).unwrap_or(1);
                let pos = if parts.get(1).map(|p| p.trim()) == Some("start") {
                    StepPosition::Start
                } else {
                    StepPosition::End
                };
                Self::Steps(count, pos)
            }
            _ => Self::Ease,
        }
    }

    /// Evaluate the easing function at progress t (0.0..=1.0).
    pub fn evaluate(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Self::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
            Self::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
            Self::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, t),
            Self::Steps(steps, pos) => {
                let steps = *steps as f64;
                match pos {
                    StepPosition::Start => (t * steps).ceil() / steps,
                    StepPosition::End => (t * steps).floor() / steps,
                }
            }
        }
    }
}

/// Simple cubic bezier approximation using De Casteljau's algorithm.
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    // Newton-Raphson to find t for given x, then compute y
    let mut guess = t;
    for _ in 0..8 {
        let cx = 3.0 * x1;
        let bx = 3.0 * (x2 - x1) - cx;
        let ax = 1.0 - cx - bx;
        let current_x = ((ax * guess + bx) * guess + cx) * guess;
        let dx = (3.0 * ax * guess + 2.0 * bx) * guess + cx;
        if dx.abs() < 1e-6 { break; }
        guess -= (current_x - t) / dx;
    }
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    ((ay * guess + by) * guess + cy) * guess
}

/// Animation fill mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FillMode {
    None,
    Forwards,
    Backwards,
    Both,
}

/// Animation direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationDirection {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// Animation play state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayState {
    Running,
    Paused,
}

/// A single CSS animation attached to an element.
#[derive(Debug, Clone)]
pub struct CssAnimation {
    pub name: String,
    pub duration_ms: f64,
    pub delay_ms: f64,
    pub iteration_count: f64, // f64::INFINITY for infinite
    pub timing_function: TimingFunction,
    pub fill_mode: FillMode,
    pub direction: AnimationDirection,
    pub play_state: PlayState,
}

impl CssAnimation {
    /// Parse an animation shorthand: "name duration timing delay iterations direction fill play".
    pub fn parse(shorthand: &str) -> Self {
        let parts: Vec<&str> = shorthand.split_whitespace().collect();
        let mut name = String::new();
        let mut duration_ms = 0.0;
        let mut delay_ms = 0.0;
        let mut timing = TimingFunction::Ease;
        let mut iterations = 1.0;
        let mut direction = AnimationDirection::Normal;
        let mut fill = FillMode::None;
        let mut play = PlayState::Running;

        let mut time_idx = 0;
        for part in &parts {
            if part.ends_with("ms") {
                let val = part.trim_end_matches("ms").parse::<f64>().unwrap_or(0.0);
                if time_idx == 0 { duration_ms = val; time_idx += 1; }
                else { delay_ms = val; }
            } else if part.ends_with('s') && !part.ends_with("ms") {
                let val = part.trim_end_matches('s').parse::<f64>().unwrap_or(0.0) * 1000.0;
                if time_idx == 0 { duration_ms = val; time_idx += 1; }
                else { delay_ms = val; }
            } else if matches!(*part, "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out") || part.starts_with("cubic-bezier") || part.starts_with("steps") {
                timing = TimingFunction::parse(part);
            } else if *part == "infinite" {
                iterations = f64::INFINITY;
            } else if let Ok(n) = part.parse::<f64>() {
                iterations = n;
            } else if matches!(*part, "normal" | "reverse" | "alternate" | "alternate-reverse") {
                direction = match *part {
                    "reverse" => AnimationDirection::Reverse,
                    "alternate" => AnimationDirection::Alternate,
                    "alternate-reverse" => AnimationDirection::AlternateReverse,
                    _ => AnimationDirection::Normal,
                };
            } else if matches!(*part, "none" | "forwards" | "backwards" | "both") {
                fill = match *part {
                    "forwards" => FillMode::Forwards,
                    "backwards" => FillMode::Backwards,
                    "both" => FillMode::Both,
                    _ => FillMode::None,
                };
            } else if matches!(*part, "running" | "paused") {
                play = if *part == "paused" { PlayState::Paused } else { PlayState::Running };
            } else if name.is_empty() {
                name = part.to_string();
            }
        }

        Self {
            name, duration_ms, delay_ms, iteration_count: iterations,
            timing_function: timing, fill_mode: fill, direction, play_state: play,
        }
    }
}

/// A single keyframe stop with progress (0.0..=1.0) and property declarations.
#[derive(Debug, Clone)]
pub struct KeyframeStop {
    pub progress: f64,
    pub declarations: HashMap<String, String>,
}

/// A complete @keyframes rule with named stops.
#[derive(Debug, Clone)]
pub struct KeyframesRule {
    pub name: String,
    pub stops: Vec<KeyframeStop>,
}

impl KeyframesRule {
    /// Interpolate declarations between two keyframe stops at a given progress.
    pub fn interpolate(&self, progress: f64) -> HashMap<String, String> {
        if self.stops.is_empty() { return HashMap::new(); }
        if self.stops.len() == 1 { return self.stops[0].declarations.clone(); }

        let p = progress.clamp(0.0, 1.0);
        // Find the two surrounding stops
        let mut lower_idx = 0;
        for (i, stop) in self.stops.iter().enumerate() {
            if stop.progress <= p { lower_idx = i; }
        }
        let upper_idx = (lower_idx + 1).min(self.stops.len() - 1);

        if lower_idx == upper_idx {
            return self.stops[lower_idx].declarations.clone();
        }

        let lower = &self.stops[lower_idx];
        let upper = &self.stops[upper_idx];
        let range = upper.progress - lower.progress;
        let t = if range > 0.0 { (p - lower.progress) / range } else { 0.0 };

        // Merge declarations: interpolate numeric values, snap non-numeric
        let mut result = lower.declarations.clone();
        for (prop, upper_val) in &upper.declarations {
            if let Some(lower_val) = lower.declarations.get(prop) {
                result.insert(prop.clone(), interpolate_value(lower_val, upper_val, t));
            } else {
                if t >= 0.5 { result.insert(prop.clone(), upper_val.clone()); }
            }
        }
        result
    }
}

/// Interpolate between two CSS values at progress t.
/// Numeric pixel/number values are lerped; others snap at t >= 0.5.
fn interpolate_value(from: &str, to: &str, t: f64) -> String {
    // Try numeric interpolation
    let from_num = from.trim_end_matches("px").parse::<f64>().ok();
    let to_num = to.trim_end_matches("px").parse::<f64>().ok();
    if let (Some(a), Some(b)) = (from_num, to_num) {
        let val = a + (b - a) * t;
        if from.ends_with("px") || to.ends_with("px") {
            return format!("{:.1}px", val);
        }
        return format!("{}", val);
    }
    // Color interpolation for hex colors
    if from.starts_with('#') && to.starts_with('#') && from.len() == 7 && to.len() == 7 {
        let r1 = u8::from_str_radix(&from[1..3], 16).unwrap_or(0);
        let g1 = u8::from_str_radix(&from[3..5], 16).unwrap_or(0);
        let b1 = u8::from_str_radix(&from[5..7], 16).unwrap_or(0);
        let r2 = u8::from_str_radix(&to[1..3], 16).unwrap_or(0);
        let g2 = u8::from_str_radix(&to[3..5], 16).unwrap_or(0);
        let b2 = u8::from_str_radix(&to[5..7], 16).unwrap_or(0);
        let r = (r1 as f64 + (r2 as f64 - r1 as f64) * t).round() as u8;
        let g = (g1 as f64 + (g2 as f64 - g1 as f64) * t).round() as u8;
        let b = (b1 as f64 + (b2 as f64 - b1 as f64) * t).round() as u8;
        return format!("#{:02x}{:02x}{:02x}", r, g, b);
    }
    // Snap for non-numeric values
    if t >= 0.5 { to.to_string() } else { from.to_string() }
}

/// Animation state machine: tracks the current state of a running animation.
#[derive(Debug, Clone)]
pub struct AnimationInstance {
    pub animation: CssAnimation,
    pub keyframes: KeyframesRule,
    pub start_time_ms: f64,
    pub current_iteration: f64,
    pub state: AnimationState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationState {
    Delayed,
    Running,
    Paused,
    Finished,
}

impl AnimationInstance {
    /// Advance the animation to the given time and return the interpolated declarations.
    pub fn tick(&mut self, now_ms: f64) -> HashMap<String, String> {
        let elapsed = now_ms - self.start_time_ms - self.animation.delay_ms;
        if elapsed < 0.0 {
            self.state = AnimationState::Delayed;
            return match self.animation.fill_mode {
                FillMode::Backwards | FillMode::Both => {
                    self.keyframes.stops.first().map(|s| s.declarations.clone()).unwrap_or_default()
                }
                _ => HashMap::new(),
            };
        }
        if self.animation.play_state == PlayState::Paused {
            self.state = AnimationState::Paused;
        }
        let duration = self.animation.duration_ms;
        if duration <= 0.0 { return HashMap::new(); }
        let raw_progress = elapsed / duration;
        self.current_iteration = raw_progress.floor();
        if raw_progress >= self.animation.iteration_count {
            self.state = AnimationState::Finished;
            return match self.animation.fill_mode {
                FillMode::Forwards | FillMode::Both => {
                    let final_p = match self.animation.direction {
                        AnimationDirection::Normal | AnimationDirection::Alternate => 1.0,
                        AnimationDirection::Reverse | AnimationDirection::AlternateReverse => 0.0,
                    };
                    self.keyframes.interpolate(final_p)
                }
                _ => HashMap::new(),
            };
        }
        self.state = AnimationState::Running;
        // Apply direction
        let cycle_progress = raw_progress.fract();
        let is_reversed = match self.animation.direction {
            AnimationDirection::Normal => false,
            AnimationDirection::Reverse => true,
            AnimationDirection::Alternate => (self.current_iteration as i64) % 2 == 1,
            AnimationDirection::AlternateReverse => (self.current_iteration as i64) % 2 == 0,
        };
        let progress = if is_reversed { 1.0 - cycle_progress } else { cycle_progress };
        // Apply timing function
        let eased = self.animation.timing_function.evaluate(progress);
        self.keyframes.interpolate(eased)
    }
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
