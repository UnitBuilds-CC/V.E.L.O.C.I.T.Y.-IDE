//! IntersectionObserver API: reports when elements enter/exit the viewport.
//!
//! Since velocity-browser is agent-facing (no visual viewport), we model
//! intersection based on the layout bounding box vs. the root element's
//! dimensions. The agent can trigger an observe and we'll compute whether
//! the element is "visible" based on simple box intersection.

use crate::js::vm::JsValue;
use std::collections::HashMap;

/// A single intersection entry matching the IntersectionObserverEntry interface.
#[derive(Debug, Clone)]
pub struct IntersectionEntry {
    pub target_node_id: usize,
    pub is_intersecting: bool,
    pub intersection_ratio: f64,
    pub bounding_rect: DomRect,
    pub intersection_rect: DomRect,
    pub root_bounds: DomRect,
}

/// Simple DOMRect representation.
#[derive(Debug, Clone, Default)]
pub struct DomRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DomRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn to_js_value(&self) -> JsValue {
        let mut map = HashMap::new();
        map.insert("x".to_string(), JsValue::Number(self.x));
        map.insert("y".to_string(), JsValue::Number(self.y));
        map.insert("width".to_string(), JsValue::Number(self.width));
        map.insert("height".to_string(), JsValue::Number(self.height));
        map.insert("top".to_string(), JsValue::Number(self.y));
        map.insert("left".to_string(), JsValue::Number(self.x));
        map.insert("bottom".to_string(), JsValue::Number(self.y + self.height));
        map.insert("right".to_string(), JsValue::Number(self.x + self.width));
        JsValue::Object(map)
    }

    /// Compute the intersection area between two rects.
    pub fn intersect(&self, other: &DomRect) -> DomRect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let w = (right - x).max(0.0);
        let h = (bottom - y).max(0.0);
        DomRect::new(x, y, w, h)
    }

    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}

impl IntersectionEntry {
    pub fn to_js_value(&self) -> JsValue {
        let mut map = HashMap::new();
        map.insert("target".to_string(), JsValue::Object({
            let mut t = HashMap::new();
            t.insert("__node_id__".to_string(), JsValue::Number(self.target_node_id as f64));
            t
        }));
        map.insert("isIntersecting".to_string(), JsValue::Boolean(self.is_intersecting));
        map.insert("intersectionRatio".to_string(), JsValue::Number(self.intersection_ratio));
        map.insert("boundingClientRect".to_string(), self.bounding_rect.to_js_value());
        map.insert("intersectionRect".to_string(), self.intersection_rect.to_js_value());
        map.insert("rootBounds".to_string(), self.root_bounds.to_js_value());
        JsValue::Object(map)
    }
}

/// Configuration thresholds for the observer.
#[derive(Debug, Clone)]
pub struct IntersectionObserverInit {
    /// Thresholds at which to report intersection (0.0 to 1.0).
    pub thresholds: Vec<f64>,
    /// Root margin as CSS-like offsets (top, right, bottom, left) in pixels.
    pub root_margin: [f64; 4],
    /// Viewport dimensions (defaults to 1920x1080 for agent).
    pub root_bounds: DomRect,
}

impl Default for IntersectionObserverInit {
    fn default() -> Self {
        Self {
            thresholds: vec![0.0],
            root_margin: [0.0; 4],
            root_bounds: DomRect::new(0.0, 0.0, 1920.0, 1080.0),
        }
    }
}

pub struct NativeIntersectionObserver {
    pub callback: Option<JsValue>,
    pub config: IntersectionObserverInit,
    pub observed_targets: Vec<usize>,
    pub last_entries: Vec<IntersectionEntry>,
}

impl Default for NativeIntersectionObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeIntersectionObserver {
    pub fn new() -> Self {
        Self {
            callback: None,
            config: IntersectionObserverInit::default(),
            observed_targets: Vec::new(),
            last_entries: Vec::new(),
        }
    }

    pub fn with_callback(callback: JsValue, config: IntersectionObserverInit) -> Self {
        Self {
            callback: Some(callback),
            config,
            observed_targets: Vec::new(),
            last_entries: Vec::new(),
        }
    }

    /// Start observing a target element by node ID.
    pub fn observe(&mut self, target_node_id: usize) {
        if !self.observed_targets.contains(&target_node_id) {
            self.observed_targets.push(target_node_id);
        }
    }

    /// Stop observing a specific target.
    pub fn unobserve(&mut self, target_node_id: usize) {
        self.observed_targets.retain(|&id| id != target_node_id);
    }

    /// Stop observing all targets.
    pub fn disconnect(&mut self) {
        self.observed_targets.clear();
        self.last_entries.clear();
    }

    /// Compute intersection for a given element bounding box.
    /// Returns the entry if any threshold is crossed.
    pub fn compute_entry(&self, target_node_id: usize, element_rect: &DomRect) -> IntersectionEntry {
        let effective_root = DomRect::new(
            self.config.root_bounds.x - self.config.root_margin[3],
            self.config.root_bounds.y - self.config.root_margin[0],
            self.config.root_bounds.width + self.config.root_margin[1] + self.config.root_margin[3],
            self.config.root_bounds.height + self.config.root_margin[0] + self.config.root_margin[2],
        );

        let intersection = effective_root.intersect(element_rect);
        let elem_area = element_rect.area();
        let ratio = if elem_area > 0.0 {
            intersection.area() / elem_area
        } else {
            0.0
        };
        let is_intersecting = ratio > 0.0;

        IntersectionEntry {
            target_node_id,
            is_intersecting,
            intersection_ratio: ratio,
            bounding_rect: element_rect.clone(),
            intersection_rect: intersection,
            root_bounds: effective_root,
        }
    }

    /// Check intersection for all observed targets given their bounding rects.
    /// Returns entries where thresholds are crossed.
    pub fn check_intersections(&mut self, rects: &[(usize, DomRect)]) -> Vec<IntersectionEntry> {
        let mut entries = Vec::new();
        for &target_id in &self.observed_targets {
            if let Some((_, rect)) = rects.iter().find(|(id, _)| *id == target_id) {
                let entry = self.compute_entry(target_id, rect);
                // Only report if intersecting or if threshold 0.0 is included and element is visible
                let dominated = self.config.thresholds.iter().any(|&t| {
                    if t == 0.0 {
                        entry.is_intersecting
                    } else {
                        entry.intersection_ratio >= t
                    }
                });
                if dominated {
                    entries.push(entry);
                }
            }
        }
        self.last_entries = entries.clone();
        entries
    }

    /// Get callback and entries for JS delivery.
    pub fn flush_to_callback(&mut self) -> Option<(JsValue, JsValue)> {
        if self.last_entries.is_empty() || self.callback.is_none() {
            return None;
        }
        let entries_js: Vec<JsValue> = self.last_entries.iter().map(|e| e.to_js_value()).collect();
        self.last_entries.clear();
        let callback = self.callback.clone().unwrap();
        Some((callback, JsValue::Array(entries_js)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_visible_element() {
        let observer = NativeIntersectionObserver::new();
        let elem = DomRect::new(100.0, 100.0, 200.0, 150.0);
        let entry = observer.compute_entry(1, &elem);
        assert!(entry.is_intersecting);
        assert!((entry.intersection_ratio - 1.0).abs() < 0.001);
    }

    #[test]
    fn partially_visible_element() {
        let observer = NativeIntersectionObserver::new();
        // Element half off-screen to the right
        let elem = DomRect::new(1820.0, 100.0, 200.0, 100.0);
        let entry = observer.compute_entry(2, &elem);
        assert!(entry.is_intersecting);
        assert!(entry.intersection_ratio > 0.0);
        assert!(entry.intersection_ratio < 1.0);
    }

    #[test]
    fn element_outside_viewport() {
        let observer = NativeIntersectionObserver::new();
        let elem = DomRect::new(2000.0, 2000.0, 100.0, 100.0);
        let entry = observer.compute_entry(3, &elem);
        assert!(!entry.is_intersecting);
        assert_eq!(entry.intersection_ratio, 0.0);
    }

    #[test]
    fn root_margin_expands_viewport() {
        let config = IntersectionObserverInit {
            thresholds: vec![0.0],
            root_margin: [100.0, 100.0, 100.0, 100.0],
            root_bounds: DomRect::new(0.0, 0.0, 1920.0, 1080.0),
        };
        let observer = NativeIntersectionObserver::with_callback(JsValue::Undefined, config);
        // Element just past the right edge
        let elem = DomRect::new(1950.0, 100.0, 50.0, 50.0);
        let entry = observer.compute_entry(4, &elem);
        assert!(entry.is_intersecting);
    }

    #[test]
    fn observe_and_check_intersections() {
        let mut observer = NativeIntersectionObserver::new();
        observer.observe(1);
        observer.observe(2);

        let rects = vec![
            (1, DomRect::new(100.0, 100.0, 50.0, 50.0)),
            (2, DomRect::new(3000.0, 3000.0, 50.0, 50.0)),
        ];

        let entries = observer.check_intersections(&rects);
        // Only element 1 should be intersecting (element 2 is off-screen)
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].target_node_id, 1);
    }

    #[test]
    fn dom_rect_intersect() {
        let a = DomRect::new(0.0, 0.0, 100.0, 100.0);
        let b = DomRect::new(50.0, 50.0, 100.0, 100.0);
        let inter = a.intersect(&b);
        assert_eq!(inter.x, 50.0);
        assert_eq!(inter.y, 50.0);
        assert_eq!(inter.width, 50.0);
        assert_eq!(inter.height, 50.0);
    }

    #[test]
    fn dom_rect_no_overlap() {
        let a = DomRect::new(0.0, 0.0, 50.0, 50.0);
        let b = DomRect::new(100.0, 100.0, 50.0, 50.0);
        let inter = a.intersect(&b);
        assert_eq!(inter.width, 0.0);
        assert_eq!(inter.height, 0.0);
        assert_eq!(inter.area(), 0.0);
    }

    #[test]
    fn dom_rect_area_computation() {
        let r = DomRect::new(0.0, 0.0, 200.0, 150.0);
        assert_eq!(r.area(), 30000.0);
    }

    #[test]
    fn dom_rect_zero_area() {
        let r = DomRect::new(10.0, 10.0, 0.0, 0.0);
        assert_eq!(r.area(), 0.0);
    }

    #[test]
    fn unobserve_removes_target() {
        let mut observer = NativeIntersectionObserver::new();
        observer.observe(1);
        observer.observe(2);
        observer.observe(3);
        assert_eq!(observer.observed_targets.len(), 3);
        observer.unobserve(2);
        assert_eq!(observer.observed_targets.len(), 2);
        assert!(!observer.observed_targets.contains(&2));
    }

    #[test]
    fn disconnect_clears_all() {
        let mut observer = NativeIntersectionObserver::new();
        observer.observe(1);
        observer.observe(2);
        observer.disconnect();
        assert!(observer.observed_targets.is_empty());
        assert!(observer.last_entries.is_empty());
    }

    #[test]
    fn observe_deduplicates_same_target() {
        let mut observer = NativeIntersectionObserver::new();
        observer.observe(1);
        observer.observe(1);
        observer.observe(1);
        assert_eq!(observer.observed_targets.len(), 1);
    }

    #[test]
    fn threshold_filtering() {
        let config = IntersectionObserverInit {
            thresholds: vec![0.5, 1.0],
            root_margin: [0.0; 4],
            root_bounds: DomRect::new(0.0, 0.0, 1920.0, 1080.0),
        };
        let mut observer = NativeIntersectionObserver::with_callback(JsValue::Undefined, config);
        observer.observe(1);
        // Element only 25% visible — below 0.5 threshold
        let elem = DomRect::new(1820.0, 0.0, 400.0, 100.0);
        let entries = observer.check_intersections(&[(1, elem)]);
        // Ratio is about 25%, which is below 0.5 threshold
        // But 0.0 is not in thresholds, so not-intersecting won't match either
        // The element IS intersecting (ratio > 0), but ratio < 0.5
        assert!(entries.is_empty() || entries[0].intersection_ratio >= 0.5);
    }

    #[test]
    fn dom_rect_to_js_value_has_all_fields() {
        let r = DomRect::new(10.0, 20.0, 100.0, 200.0);
        let js = r.to_js_value();
        if let JsValue::Object(map) = js {
            assert_eq!(map.get("x"), Some(&JsValue::Number(10.0)));
            assert_eq!(map.get("y"), Some(&JsValue::Number(20.0)));
            assert_eq!(map.get("width"), Some(&JsValue::Number(100.0)));
            assert_eq!(map.get("height"), Some(&JsValue::Number(200.0)));
            assert_eq!(map.get("top"), Some(&JsValue::Number(20.0)));
            assert_eq!(map.get("left"), Some(&JsValue::Number(10.0)));
            assert_eq!(map.get("bottom"), Some(&JsValue::Number(220.0)));
            assert_eq!(map.get("right"), Some(&JsValue::Number(110.0)));
        } else {
            panic!("Expected Object");
        }
    }
}
