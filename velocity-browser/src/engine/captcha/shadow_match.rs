//! Shadow-image matching for Azure-style "pick the matching silhouette" challenges.
//!
//! This is the hardest common type: a reference object is shown alongside
//! several shadow silhouettes, and the solver must pick the shadow whose shape
//! matches the reference under rotation, scale and translation. We reuse the
//! rotation/scale-invariant [`ShapeSignature`] pipeline: extract a signature for
//! the reference and each candidate, score them with the [`ShapeMatcher`], and
//! return the best candidate plus the [`Transform2D`] that aligns it to the
//! reference (useful for drag-to-fit variants).

use super::shape_match::ShapeMatcher;
use super::spline::{ShapeSignature, SplineExtractor, RADIAL_BINS};
use crate::engine::PixelBuffer;

/// A similarity transform aligning a candidate shadow onto the reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    /// Rotation in radians.
    pub rotation: f32,
    /// Uniform scale factor (candidate → reference).
    pub scale: f32,
    /// Translation of the centroid (dx, dy) in pixels.
    pub dx: f32,
    pub dy: f32,
}

impl Transform2D {
    pub fn identity() -> Self {
        Self {
            rotation: 0.0,
            scale: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }
}

/// The result of matching a reference shape against candidate shadows.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowMatch {
    /// Index of the best-matching candidate.
    pub index: usize,
    /// Similarity score in [0, 1].
    pub score: f32,
    /// Transform aligning that candidate onto the reference.
    pub transform: Transform2D,
}

/// Matches a reference object to its silhouette among candidate shadows.
#[derive(Debug, Clone)]
pub struct ShadowMatcher {
    matcher: ShapeMatcher,
    extractor: SplineExtractor,
}

impl Default for ShadowMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ShadowMatcher {
    pub fn new() -> Self {
        Self {
            // Shadows are noisier than clean tiles; loosen the threshold.
            matcher: ShapeMatcher::with_tolerance(0.75),
            extractor: SplineExtractor::new(),
        }
    }

    pub fn with_tolerance(tolerance: f32) -> Self {
        Self {
            matcher: ShapeMatcher::with_tolerance(tolerance),
            extractor: SplineExtractor::new(),
        }
    }

    /// Compute the alignment transform from a candidate signature to the
    /// reference: rotation from the best cyclic profile shift, scale from the
    /// square-root of the area ratio, translation from centroid difference.
    pub fn align(&self, reference: &ShapeSignature, candidate: &ShapeSignature) -> Transform2D {
        if reference.is_empty() || candidate.is_empty() {
            return Transform2D::identity();
        }
        let bins = self.matcher.best_rotation_bins(reference, candidate) as f32;
        let rotation = (bins / RADIAL_BINS as f32) * std::f32::consts::TAU;
        let scale = if candidate.area > 0.0 {
            (reference.area / candidate.area).sqrt()
        } else {
            1.0
        };
        Transform2D {
            rotation,
            scale,
            dx: reference.centroid.x - candidate.centroid.x,
            dy: reference.centroid.y - candidate.centroid.y,
        }
    }

    /// Score a single candidate signature against the reference.
    pub fn score(&self, reference: &ShapeSignature, candidate: &ShapeSignature) -> f32 {
        self.matcher.similarity(reference, candidate)
    }

    /// Match a reference signature against candidate signatures. Returns the
    /// best match if its score clears tolerance, else `None`.
    pub fn best_match(
        &self,
        reference: &ShapeSignature,
        candidates: &[ShapeSignature],
    ) -> Option<ShadowMatch> {
        if reference.is_empty() {
            return None;
        }
        let mut best: Option<ShadowMatch> = None;
        for (i, cand) in candidates.iter().enumerate() {
            let score = self.matcher.similarity(reference, cand);
            if best.as_ref().map(|m| score > m.score).unwrap_or(true) {
                best = Some(ShadowMatch {
                    index: i,
                    score,
                    transform: self.align(reference, cand),
                });
            }
        }
        best.filter(|m| m.score >= self.matcher.tolerance)
    }

    /// Extract signatures from pixel regions and match the reference region
    /// against the candidate regions.
    pub fn best_match_regions(
        &self,
        buffer: &PixelBuffer,
        reference_region: (usize, usize, usize, usize),
        candidate_regions: &[(usize, usize, usize, usize)],
    ) -> Option<ShadowMatch> {
        let reference = self.extractor.extract_signature(buffer, reference_region);
        let candidates: Vec<ShapeSignature> = candidate_regions
            .iter()
            .map(|&r| self.extractor.extract_signature(buffer, r))
            .collect();
        self.best_match(&reference, &candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: usize, x: usize, y: usize, w: usize) -> PixelBuffer {
        let mut buf = PixelBuffer::new(size, size);
        buf.fill_rect(x, y, w, w, 20, 20, 20, 255);
        buf
    }

    fn disc(size: usize, cx: usize, cy: usize, r: usize) -> PixelBuffer {
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
    fn picks_matching_shadow_among_candidates() {
        let ex = SplineExtractor::new();
        // Reference is a square; candidates are [disc, square, disc].
        let reference = ex.extract_signature(&square(80, 20, 20, 40), (0, 0, 80, 80));
        let candidates = vec![
            ex.extract_signature(&disc(80, 40, 40, 22), (0, 0, 80, 80)),
            ex.extract_signature(&square(80, 24, 24, 36), (0, 0, 80, 80)),
            ex.extract_signature(&disc(80, 40, 40, 18), (0, 0, 80, 80)),
        ];
        let m = ShadowMatcher::new();
        let best = m.best_match(&reference, &candidates).expect("a match");
        assert_eq!(best.index, 1, "score = {}", best.score);
    }

    #[test]
    fn no_candidate_clears_tolerance() {
        let ex = SplineExtractor::new();
        let reference = ex.extract_signature(&square(80, 20, 20, 40), (0, 0, 80, 80));
        // Only a very different disc is offered under a strict tolerance.
        let candidates = vec![ex.extract_signature(&disc(80, 40, 40, 22), (0, 0, 80, 80))];
        let m = ShadowMatcher::with_tolerance(0.97);
        assert!(m.best_match(&reference, &candidates).is_none());
    }

    #[test]
    fn empty_reference_yields_no_match() {
        let m = ShadowMatcher::new();
        assert!(m.best_match(&ShapeSignature::empty(), &[]).is_none());
    }

    #[test]
    fn align_scale_reflects_area_ratio() {
        let ex = SplineExtractor::new();
        // Reference square is 4x the linear size of the candidate → scale ~2.
        let reference = ex.extract_signature(&square(120, 20, 20, 80), (0, 0, 120, 120));
        let candidate = ex.extract_signature(&square(120, 40, 40, 40), (0, 0, 120, 120));
        let t = m_align(&ex, &reference, &candidate);
        assert!(t.scale > 1.4 && t.scale < 2.6, "scale = {}", t.scale);
    }

    #[test]
    fn identity_transform_defaults() {
        let t = Transform2D::identity();
        assert_eq!(t.scale, 1.0);
        assert_eq!(t.rotation, 0.0);
    }

    #[test]
    fn region_matching_selects_reference_like_candidate() {
        // One buffer holding a reference square and two candidate regions.
        let mut buf = PixelBuffer::new(180, 60);
        buf.fill_rect(10, 10, 40, 40, 20, 20, 20, 255); // reference: square
                                                        // candidate 0: disc
        for y in 0..60 {
            for x in 60..120 {
                let dx = x as i32 - 90;
                let dy = y as i32 - 30;
                if dx * dx + dy * dy <= 20 * 20 {
                    buf.set_pixel(x, y, 20, 20, 20, 255);
                }
            }
        }
        buf.fill_rect(130, 10, 40, 40, 20, 20, 20, 255); // candidate 1: square
        let m = ShadowMatcher::new();
        let best = m
            .best_match_regions(&buf, (0, 0, 60, 60), &[(60, 0, 60, 60), (120, 0, 60, 60)])
            .expect("a match");
        assert_eq!(best.index, 1, "score = {}", best.score);
    }

    fn m_align(ex: &SplineExtractor, a: &ShapeSignature, b: &ShapeSignature) -> Transform2D {
        let _ = ex;
        ShadowMatcher::new().align(a, b)
    }
}
