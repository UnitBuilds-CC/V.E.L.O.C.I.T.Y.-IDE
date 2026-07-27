//! CSS Transitions.
//!
//! Implements the `transition` property model and a runtime that interpolates
//! property values when a node's computed style changes. This complements the
//! `@keyframes` animation system in [`cascade`]: animations run on a timeline
//! regardless of state, whereas transitions interpolate *between* two computed
//! values whenever a property changes.
//!
//! The layout/render layer drives this by calling
//! [`TransitionManager::set_property`] whenever a node's computed value changes
//! and [`TransitionManager::tick`] once per frame to obtain interpolated
//! overrides.

use std::collections::HashMap;

use crate::style::cascade::{interpolate_value, TimingFunction};

/// A parsed `transition` specification for a single property.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionSpec {
    /// The CSS property this transition applies to (e.g. `opacity`). `all`
    /// matches every property.
    pub property: String,
    /// Duration of the transition in milliseconds.
    pub duration_ms: f64,
    /// Delay before the transition starts, in milliseconds.
    pub delay_ms: f64,
    /// Easing function.
    pub timing_function: TimingFunction,
}

impl TransitionSpec {
    /// Parse a single `transition` shorthand entry:
    /// `property duration timing-function delay`.
    ///
    /// The two time values are positional: the first is the duration, the
    /// second (if present) is the delay — mirroring the CSS specification.
    pub fn parse(shorthand: &str) -> Self {
        let parts: Vec<&str> = shorthand.split_whitespace().collect();
        let mut property = "all".to_string();
        let mut duration_ms = 0.0;
        let mut delay_ms = 0.0;
        let mut timing = TimingFunction::Ease;
        let mut time_idx = 0;

        for part in &parts {
            if part.ends_with("ms") {
                let val = part.trim_end_matches("ms").parse::<f64>().unwrap_or(0.0);
                if time_idx == 0 {
                    duration_ms = val;
                } else {
                    delay_ms = val;
                }
                time_idx += 1;
            } else if part.ends_with('s') && !part.ends_with("ms") {
                let val = part.trim_end_matches('s').parse::<f64>().unwrap_or(0.0) * 1000.0;
                if time_idx == 0 {
                    duration_ms = val;
                } else {
                    delay_ms = val;
                }
                time_idx += 1;
            } else if matches!(*part, "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out")
                || part.starts_with("cubic-bezier")
                || part.starts_with("steps")
            {
                timing = TimingFunction::parse(part);
            } else if property == "all" {
                property = part.to_string();
            }
        }

        Self { property, duration_ms, delay_ms, timing_function: timing }
    }

    /// Parse a full `transition` declaration that may list several transitions
    /// separated by commas, e.g. `opacity 0.3s, transform 200ms ease-in`.
    pub fn parse_many(declaration: &str) -> Vec<TransitionSpec> {
        declaration.split(',').map(Self::parse).collect()
    }

    /// Whether this spec applies to the given property name.
    pub fn matches(&self, property: &str) -> bool {
        self.property == "all" || self.property.eq_ignore_ascii_case(property)
    }
}

/// The runtime state of a transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionState {
    /// Waiting out the delay period.
    Delayed,
    /// Actively interpolating.
    Running,
    /// Reached the target value.
    Finished,
}

/// A running transition instance for one property on one node.
#[derive(Debug, Clone)]
pub struct TransitionInstance {
    pub property: String,
    pub from: String,
    pub to: String,
    pub spec: TransitionSpec,
    pub start_time_ms: f64,
    pub state: TransitionState,
}

impl TransitionInstance {
    /// Advance the transition to `now_ms`, returning the interpolated value.
    pub fn tick(&mut self, now_ms: f64) -> String {
        let elapsed = now_ms - self.start_time_ms - self.spec.delay_ms;
        if elapsed < 0.0 {
            self.state = TransitionState::Delayed;
            return self.from.clone();
        }
        if self.spec.duration_ms <= 0.0 {
            self.state = TransitionState::Finished;
            return self.to.clone();
        }
        let progress = elapsed / self.spec.duration_ms;
        if progress >= 1.0 {
            self.state = TransitionState::Finished;
            return self.to.clone();
        }
        self.state = TransitionState::Running;
        let eased = self.spec.timing_function.evaluate(progress);
        interpolate_value(&self.from, &self.to, eased)
    }
}

/// Manages active CSS transitions across the document.
#[derive(Debug, Clone, Default)]
pub struct TransitionManager {
    /// Active transitions keyed by DOM node ID, then property name.
    active: HashMap<usize, HashMap<String, TransitionInstance>>,
    /// The last known computed value per node/property (the transition origin).
    base: HashMap<usize, HashMap<String, String>>,
    /// Transition specs declared per node (from the `transition` property).
    specs: HashMap<usize, Vec<TransitionSpec>>,
    /// Whether any transitions are currently running.
    pub has_active: bool,
}

impl TransitionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare the `transition` specs in effect for a node.
    pub fn set_transition_specs(&mut self, node_id: usize, specs: Vec<TransitionSpec>) {
        if specs.is_empty() {
            self.specs.remove(&node_id);
        } else {
            self.specs.insert(node_id, specs);
        }
    }

    /// Update a node's computed value for `property`. If a matching transition
    /// is declared and the value actually changed, a transition is started from
    /// the previous value to the new one.
    pub fn set_property(&mut self, node_id: usize, property: &str, new_value: &str, now_ms: f64) {
        let old = self
            .base
            .entry(node_id)
            .or_default()
            .insert(property.to_string(), new_value.to_string());

        // Determine whether a transition applies to this property change.
        let spec = self
            .specs
            .get(&node_id)
            .and_then(|specs| specs.iter().find(|s| s.matches(property)).cloned());

        let Some(spec) = spec else { return };
        let Some(old) = old else { return }; // first value: nothing to transition from
        if old == new_value {
            return; // no change, no transition
        }

        let instance = TransitionInstance {
            property: property.to_string(),
            from: old,
            to: new_value.to_string(),
            spec,
            start_time_ms: now_ms,
            state: TransitionState::Delayed,
        };
        let entry = self.active.entry(node_id).or_default();
        entry.insert(property.to_string(), instance);
        self.has_active = true;
    }

    /// Advance all active transitions to `now_ms`, returning interpolated style
    /// overrides per node. Finished transitions are pruned automatically.
    pub fn tick(&mut self, now_ms: f64) -> HashMap<usize, HashMap<String, String>> {
        let mut overrides: HashMap<usize, HashMap<String, String>> = HashMap::new();
        let mut any_active = false;

        for (node_id, instances) in self.active.iter_mut() {
            let mut merged = HashMap::new();
            for instance in instances.values_mut() {
                let value = instance.tick(now_ms);
                merged.insert(instance.property.clone(), value);
                if instance.state != TransitionState::Finished {
                    any_active = true;
                }
            }
            if !merged.is_empty() {
                overrides.insert(*node_id, merged);
            }
        }

        for instances in self.active.values_mut() {
            instances.retain(|_, i| i.state != TransitionState::Finished);
        }
        self.active.retain(|_, v| !v.is_empty());
        self.has_active = any_active;

        overrides
    }

    /// Current base (target) value recorded for a node/property, if any.
    pub fn base_value(&self, node_id: usize, property: &str) -> Option<&String> {
        self.base.get(&node_id).and_then(|m| m.get(property))
    }

    /// Number of nodes with active transitions.
    pub fn active_node_count(&self) -> usize {
        self.active.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_transition_shorthand_full() {
        let spec = TransitionSpec::parse("opacity 0.3s ease-in-out 100ms");
        assert_eq!(spec.property, "opacity");
        assert_eq!(spec.duration_ms, 300.0);
        assert_eq!(spec.delay_ms, 100.0);
        assert_eq!(spec.timing_function, TimingFunction::EaseInOut);
    }

    #[test]
    fn parse_transition_defaults() {
        let spec = TransitionSpec::parse("transform 200ms");
        assert_eq!(spec.property, "transform");
        assert_eq!(spec.duration_ms, 200.0);
        assert_eq!(spec.delay_ms, 0.0);
        assert_eq!(spec.timing_function, TimingFunction::Ease);
    }

    #[test]
    fn parse_many_splits_on_comma() {
        let specs = TransitionSpec::parse_many("opacity 0.3s, transform 200ms linear");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].property, "opacity");
        assert_eq!(specs[1].property, "transform");
        assert_eq!(specs[1].timing_function, TimingFunction::Linear);
    }

    #[test]
    fn spec_matches_property_or_all() {
        assert!(TransitionSpec::parse("opacity 1s").matches("opacity"));
        assert!(!TransitionSpec::parse("opacity 1s").matches("color"));
        assert!(TransitionSpec::parse("all 1s").matches("color"));
    }

    #[test]
    fn transition_interpolates_midpoint() {
        let mut mgr = TransitionManager::new();
        mgr.set_transition_specs(1, TransitionSpec::parse_many("opacity 1s linear"));
        // First value establishes the origin; no transition yet.
        mgr.set_property(1, "opacity", "0", 0.0);
        assert!(!mgr.has_active);
        // Changing the value starts the transition.
        mgr.set_property(1, "opacity", "1", 0.0);
        assert!(mgr.has_active);

        let overrides = mgr.tick(500.0);
        assert_eq!(overrides.get(&1).unwrap().get("opacity").unwrap(), "0.5");
    }

    #[test]
    fn transition_finishes_and_prunes() {
        let mut mgr = TransitionManager::new();
        mgr.set_transition_specs(7, TransitionSpec::parse_many("width 200ms linear"));
        mgr.set_property(7, "width", "0px", 0.0);
        mgr.set_property(7, "width", "100px", 0.0);

        let overrides = mgr.tick(250.0);
        assert_eq!(overrides.get(&7).unwrap().get("width").unwrap(), "100px");
        assert!(!mgr.has_active);
        assert_eq!(mgr.active_node_count(), 0);
    }

    #[test]
    fn no_transition_without_value_change() {
        let mut mgr = TransitionManager::new();
        mgr.set_transition_specs(3, TransitionSpec::parse_many("opacity 1s linear"));
        mgr.set_property(3, "opacity", "1", 0.0);
        mgr.set_property(3, "opacity", "1", 10.0); // identical value
        assert!(!mgr.has_active);
    }

    #[test]
    fn transition_respects_delay() {
        let mut mgr = TransitionManager::new();
        mgr.set_transition_specs(9, TransitionSpec::parse_many("opacity 1s linear 500ms"));
        mgr.set_property(9, "opacity", "0", 0.0);
        mgr.set_property(9, "opacity", "1", 0.0);

        // During the delay the origin value is held.
        let overrides = mgr.tick(250.0);
        assert_eq!(overrides.get(&9).unwrap().get("opacity").unwrap(), "0");
        // After the delay elapses, interpolation proceeds.
        let overrides = mgr.tick(1000.0);
        assert_eq!(overrides.get(&9).unwrap().get("opacity").unwrap(), "0.5");
    }
}
