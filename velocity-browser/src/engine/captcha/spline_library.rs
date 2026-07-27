//! Online learning store mapping shape signatures to object classifications.
//!
//! This is the mechanism that makes the solver improve over time. When an LLM
//! (or a human) labels a tile — "this is a bus" — the solver extracts the tile's
//! [`ShapeSignature`] and files it under that class here. On later challenges the
//! library can classify a tile natively by fuzzy shape match, with no LLM call.
//! Repeated exemplars for a class are merged and reinforced so confidence grows
//! and the stored profile converges toward the class's stable shape.

use super::shape_match::ShapeMatcher;
use super::spline::{ShapeSignature, SplineExtractor};
use crate::engine::PixelBuffer;
use std::collections::HashMap;

/// A learned shape exemplar for a class, with reinforcement bookkeeping.
#[derive(Debug, Clone)]
pub struct ClassifiedShape {
    pub signature: ShapeSignature,
    /// How many times this exemplar has been reinforced.
    pub samples: u32,
    /// Confidence in [0, 1], increasing with samples.
    pub confidence: f32,
}

/// Learned association from shape signatures to object class names.
#[derive(Debug, Clone)]
pub struct SplineLibrary {
    by_class: HashMap<String, Vec<ClassifiedShape>>,
    matcher: ShapeMatcher,
    extractor: SplineExtractor,
    /// Similarity at/above which a new sample reinforces an existing exemplar
    /// rather than creating a new one.
    merge_similarity: f32,
    max_exemplars_per_class: usize,
}

impl Default for SplineLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl SplineLibrary {
    pub fn new() -> Self {
        Self {
            by_class: HashMap::new(),
            matcher: ShapeMatcher::with_tolerance(0.82),
            extractor: SplineExtractor::new(),
            merge_similarity: 0.92,
            max_exemplars_per_class: 8,
        }
    }

    /// Adjust the classification match tolerance.
    pub fn with_tolerance(mut self, tolerance: f32) -> Self {
        self.matcher = ShapeMatcher::with_tolerance(tolerance);
        self
    }

    /// Learn from a raw signature: reinforce the nearest exemplar of `class`
    /// (merging their profiles) or add it as a new exemplar.
    pub fn learn(&mut self, class: &str, signature: ShapeSignature) {
        if signature.is_empty() {
            return;
        }
        let entry = self.by_class.entry(class.to_string()).or_default();

        // Find the nearest existing exemplar for this class.
        let mut best_i = None;
        let mut best_sim = 0.0f32;
        for (i, ex) in entry.iter().enumerate() {
            let sim = self.matcher.similarity(&ex.signature, &signature);
            if sim > best_sim {
                best_sim = sim;
                best_i = Some(i);
            }
        }

        match best_i {
            Some(i) if best_sim >= self.merge_similarity => {
                let ex = &mut entry[i];
                blend_into(&mut ex.signature, &signature, ex.samples);
                ex.samples += 1;
                ex.confidence = confidence_for(ex.samples);
            }
            _ => {
                entry.push(ClassifiedShape {
                    signature,
                    samples: 1,
                    confidence: confidence_for(1),
                });
                // Evict the lowest-confidence exemplar if over capacity.
                if entry.len() > self.max_exemplars_per_class {
                    if let Some((min_i, _)) = entry
                        .iter()
                        .enumerate()
                        .min_by(|a, b| a.1.confidence.total_cmp(&b.1.confidence))
                    {
                        entry.remove(min_i);
                    }
                }
            }
        }
    }

    /// Learn directly from a pixel region (extracts the signature first).
    pub fn learn_from_region(
        &mut self,
        class: &str,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
    ) {
        let sig = self.extractor.extract_signature(buffer, region);
        self.learn(class, sig);
    }

    /// Classify a signature. Returns the best `(class, confidence)` whose shape
    /// similarity clears the matcher tolerance, or `None` if nothing matches.
    ///
    /// The returned confidence blends the shape similarity with how well-learned
    /// the matched exemplar is, so a barely-seen class can't outrank a solid one.
    pub fn classify(&self, signature: &ShapeSignature) -> Option<(String, f32)> {
        if signature.is_empty() {
            return None;
        }
        let mut best: Option<(String, f32)> = None;
        for (class, exemplars) in &self.by_class {
            for ex in exemplars {
                let sim = self.matcher.similarity(&ex.signature, signature);
                if sim < self.matcher.tolerance {
                    continue;
                }
                let score = sim * ex.confidence;
                if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                    best = Some((class.clone(), score));
                }
            }
        }
        best
    }

    /// Classify a pixel region directly.
    pub fn classify_region(
        &self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
    ) -> Option<(String, f32)> {
        let sig = self.extractor.extract_signature(buffer, region);
        self.classify(&sig)
    }

    pub fn class_count(&self) -> usize {
        self.by_class.len()
    }

    pub fn exemplar_count(&self, class: &str) -> usize {
        self.by_class.get(class).map(|v| v.len()).unwrap_or(0)
    }

    pub fn known_classes(&self) -> Vec<String> {
        let mut classes: Vec<String> = self.by_class.keys().cloned().collect();
        classes.sort();
        classes
    }
}

/// Confidence as a saturating function of sample count: 1 - 1/(1+n).
fn confidence_for(samples: u32) -> f32 {
    1.0 - 1.0 / (1.0 + samples as f32)
}

/// Blend `incoming` into `target` using a running average weighted by how many
/// samples `target` already represents, converging the profile toward stability.
fn blend_into(target: &mut ShapeSignature, incoming: &ShapeSignature, prior_samples: u32) {
    let w = prior_samples as f32;
    let denom = w + 1.0;
    let n = target.radial_profile.len().min(incoming.radial_profile.len());
    for i in 0..n {
        target.radial_profile[i] =
            (target.radial_profile[i] * w + incoming.radial_profile[i]) / denom;
    }
    target.compactness = (target.compactness * w + incoming.compactness) / denom;
    target.area = (target.area * w + incoming.area) / denom;
    target.perimeter = (target.perimeter * w + incoming.perimeter) / denom;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PixelBuffer;

    fn square_buffer(size: usize, x: usize, y: usize, w: usize) -> PixelBuffer {
        let mut buf = PixelBuffer::new(size, size);
        buf.fill_rect(x, y, w, w, 20, 20, 20, 255);
        buf
    }

    fn disc_buffer(size: usize, cx: usize, cy: usize, r: usize) -> PixelBuffer {
        let mut buf = PixelBuffer::new(size, size);
        for y in 0..size {
            for x in 0..size {
                let dx = x as i32 - cx as i32;
                let dy = y as i32 - cy as i32;
                if dx * dx + dy * dy <= (r * r) as i32 {
                    buf.set_pixel(x, y, 20, 20, 20, 255);
                }
            }
        }
        buf
    }

    #[test]
    fn learns_and_classifies_same_shape() {
        let mut lib = SplineLibrary::new();
        lib.learn_from_region("box", &square_buffer(80, 20, 20, 40), (0, 0, 80, 80));
        let result = lib.classify_region(&square_buffer(80, 22, 22, 38), (0, 0, 80, 80));
        assert!(matches!(result, Some((ref c, _)) if c == "box"), "got {:?}", result);
    }

    #[test]
    fn distinguishes_two_classes() {
        let mut lib = SplineLibrary::new();
        lib.learn_from_region("box", &square_buffer(80, 20, 20, 40), (0, 0, 80, 80));
        lib.learn_from_region("circle", &disc_buffer(80, 40, 40, 25), (0, 0, 80, 80));
        let r = lib.classify_region(&disc_buffer(80, 41, 39, 24), (0, 0, 80, 80));
        assert!(matches!(r, Some((ref c, _)) if c == "circle"), "got {:?}", r);
        assert_eq!(lib.class_count(), 2);
    }

    #[test]
    fn reinforcement_increases_confidence_without_new_exemplar() {
        let mut lib = SplineLibrary::new();
        let buf = square_buffer(80, 20, 20, 40);
        lib.learn_from_region("box", &buf, (0, 0, 80, 80));
        lib.learn_from_region("box", &buf, (0, 0, 80, 80));
        lib.learn_from_region("box", &buf, (0, 0, 80, 80));
        // Identical inputs merge into a single, reinforced exemplar.
        assert_eq!(lib.exemplar_count("box"), 1);
        let (_, conf) = lib.classify_region(&buf, (0, 0, 80, 80)).unwrap();
        // 3 samples → confidence 0.75 before the similarity weighting.
        assert!(conf > 0.6, "confidence too low: {}", conf);
    }

    #[test]
    fn unknown_shape_is_not_classified() {
        let mut lib = SplineLibrary::new();
        lib.learn_from_region("box", &square_buffer(80, 20, 20, 40), (0, 0, 80, 80));
        // A blank region has no contour and must not classify.
        let r = lib.classify_region(&PixelBuffer::new(80, 80), (0, 0, 80, 80));
        assert!(r.is_none());
    }

    #[test]
    fn empty_signature_is_ignored_on_learn() {
        let mut lib = SplineLibrary::new();
        lib.learn("nothing", ShapeSignature::empty());
        assert_eq!(lib.class_count(), 0);
    }

    #[test]
    fn known_classes_are_sorted() {
        let mut lib = SplineLibrary::new();
        lib.learn_from_region("zebra", &square_buffer(80, 20, 20, 40), (0, 0, 80, 80));
        lib.learn_from_region("apple", &disc_buffer(80, 40, 40, 25), (0, 0, 80, 80));
        assert_eq!(lib.known_classes(), vec!["apple".to_string(), "zebra".to_string()]);
    }
}
