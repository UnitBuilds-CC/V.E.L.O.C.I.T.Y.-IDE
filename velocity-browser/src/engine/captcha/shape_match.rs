//! Fuzzy shape matching with rotation and scale invariance.
//!
//! Compares [`ShapeSignature`]s produced by the [`super::spline`] extractor.
//! Scale is already removed by the extractor's normalization; rotation is
//! handled here by cyclically correlating the radial profiles across every bin
//! offset and taking the best alignment. This lets the solver recognize a
//! rotated square, a flipped puzzle piece, or the same object at a different
//! size as the same shape — the core of native (LLM-free) recognition.

use super::spline::{ShapeSignature, RADIAL_BINS};

/// Compares shape signatures with rotation/scale invariance.
#[derive(Debug, Clone)]
pub struct ShapeMatcher {
    /// Similarity threshold in [0, 1] above which two shapes are "the same".
    pub tolerance: f32,
    /// Weight of the radial-profile term vs. the compactness term.
    profile_weight: f32,
}

impl Default for ShapeMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeMatcher {
    pub fn new() -> Self {
        Self {
            tolerance: 0.85,
            profile_weight: 0.8,
        }
    }

    pub fn with_tolerance(tolerance: f32) -> Self {
        Self {
            tolerance: tolerance.clamp(0.0, 1.0),
            profile_weight: 0.8,
        }
    }

    /// Similarity in [0, 1]. `1.0` means identical shape up to rotation & scale.
    ///
    /// Combines the best cyclic profile alignment (rotation-invariant) with a
    /// compactness closeness term (a pure scalar that resists noise).
    pub fn similarity(&self, a: &ShapeSignature, b: &ShapeSignature) -> f32 {
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let (profile_sim, _) = self.best_alignment(a, b);
        let compactness_sim = 1.0 - (a.compactness - b.compactness).abs().clamp(0.0, 1.0);
        (self.profile_weight * profile_sim + (1.0 - self.profile_weight) * compactness_sim)
            .clamp(0.0, 1.0)
    }

    /// Best rotation offset (in angular bins) that aligns `b` onto `a`.
    pub fn best_rotation_bins(&self, a: &ShapeSignature, b: &ShapeSignature) -> usize {
        self.best_alignment(a, b).1
    }

    /// Best rotation (radians) that aligns `b` onto `a`.
    pub fn best_rotation(&self, a: &ShapeSignature, b: &ShapeSignature) -> f32 {
        let bins = self.best_rotation_bins(a, b) as f32;
        (bins / RADIAL_BINS as f32) * std::f32::consts::TAU
    }

    /// Whether two shapes match within tolerance.
    pub fn is_match(&self, a: &ShapeSignature, b: &ShapeSignature) -> bool {
        self.similarity(a, b) >= self.tolerance
    }

    /// Try every cyclic shift of `b`'s profile against `a`'s and return the
    /// `(best_similarity, best_shift_bins)` pair.
    fn best_alignment(&self, a: &ShapeSignature, b: &ShapeSignature) -> (f32, usize) {
        let pa = &a.radial_profile;
        let pb = &b.radial_profile;
        let n = pa.len().min(pb.len()).min(RADIAL_BINS);
        if n == 0 {
            return (0.0, 0);
        }
        let mut best_sim = 0.0f32;
        let mut best_shift = 0usize;
        for shift in 0..n {
            let mut diff_acc = 0.0f32;
            for i in 0..n {
                let bv = pb[(i + shift) % n];
                diff_acc += (pa[i] - bv).abs();
            }
            let sim = 1.0 - (diff_acc / n as f32);
            if sim > best_sim {
                best_sim = sim;
                best_shift = shift;
            }
        }
        (best_sim.clamp(0.0, 1.0), best_shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::captcha::spline::{Point2D, SplineExtractor};
    use crate::engine::PixelBuffer;

    fn sig_from_profile(profile: Vec<f32>, compactness: f32) -> ShapeSignature {
        ShapeSignature {
            centroid: Point2D::new(0.0, 0.0),
            radial_profile: profile,
            compactness,
            area: 100.0,
            perimeter: 40.0,
            point_count: 50,
            hash: 1,
        }
    }

    /// A profile with a single lobe, so a cyclic shift is unambiguous.
    fn lobed_profile(peak_bin: usize) -> Vec<f32> {
        let mut p = vec![0.2f32; RADIAL_BINS];
        p[peak_bin % RADIAL_BINS] = 1.0;
        p[(peak_bin + 1) % RADIAL_BINS] = 0.7;
        p
    }

    #[test]
    fn identical_shapes_score_one() {
        let a = sig_from_profile(lobed_profile(0), 0.78);
        let b = a.clone();
        let m = ShapeMatcher::new();
        assert!((m.similarity(&a, &b) - 1.0).abs() < 1e-4);
        assert!(m.is_match(&a, &b));
    }

    #[test]
    fn rotated_shape_still_matches() {
        // Same shape, profile cyclically shifted by 9 bins (= 90°).
        let a = sig_from_profile(lobed_profile(0), 0.78);
        let b = sig_from_profile(lobed_profile(9), 0.78);
        let m = ShapeMatcher::new();
        assert!(m.is_match(&a, &b), "sim = {}", m.similarity(&a, &b));
        assert_eq!(m.best_rotation_bins(&a, &b), 9);
    }

    #[test]
    fn best_rotation_radians_is_reasonable() {
        let a = sig_from_profile(lobed_profile(0), 0.78);
        let b = sig_from_profile(lobed_profile(9), 0.78);
        let m = ShapeMatcher::new();
        let rot = m.best_rotation(&a, &b);
        // 9/36 of a full turn ≈ π/2.
        assert!(
            (rot - std::f32::consts::FRAC_PI_2).abs() < 0.2,
            "rot = {}",
            rot
        );
    }

    #[test]
    fn empty_signature_never_matches() {
        let a = sig_from_profile(lobed_profile(0), 0.78);
        let empty = ShapeSignature::empty();
        let m = ShapeMatcher::new();
        assert_eq!(m.similarity(&a, &empty), 0.0);
        assert!(!m.is_match(&a, &empty));
    }

    #[test]
    fn distinct_shapes_score_low() {
        // Uniform disc-like profile vs. a sharply lobed profile.
        let disc = sig_from_profile(vec![1.0; RADIAL_BINS], 1.0);
        let spiky = sig_from_profile(lobed_profile(0), 0.4);
        let m = ShapeMatcher::new();
        assert!(
            !m.is_match(&disc, &spiky),
            "sim = {}",
            m.similarity(&disc, &spiky)
        );
    }

    #[test]
    fn scale_invariance_from_extractor_normalization() {
        // Two squares of different sizes should match (normalization removes scale).
        let ex = SplineExtractor::new();
        let mut small = PixelBuffer::new(80, 80);
        small.fill_rect(30, 30, 16, 16, 20, 20, 20, 255);
        let mut big = PixelBuffer::new(80, 80);
        big.fill_rect(20, 20, 40, 40, 20, 20, 20, 255);
        let sa = ex.extract_signature(&small, (0, 0, 80, 80));
        let sb = ex.extract_signature(&big, (0, 0, 80, 80));
        let m = ShapeMatcher::with_tolerance(0.8);
        assert!(m.is_match(&sa, &sb), "sim = {}", m.similarity(&sa, &sb));
    }
}
