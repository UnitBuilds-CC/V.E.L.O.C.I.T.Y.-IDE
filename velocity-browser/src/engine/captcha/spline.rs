//! Spline-based shape extraction from rasterized captcha regions.
//!
//! Extends the OCR pixel-scanning approach from `visual_fingerprint` to produce
//! geometric shape descriptors instead of coarse layout signatures. The pipeline
//! is: binary mask → boundary tracing → contour ordering → spline fit +
//! rotation/scale-invariant [`ShapeSignature`]. These signatures let the solver
//! recognize objects (buses, puzzle pieces, rotated tiles) natively — by rule,
//! without spending LLM tokens — and are the substrate the shape matcher,
//! learning library and shadow matcher all build on.

use crate::engine::PixelBuffer;

/// Number of angular bins in a radial signature (10° resolution).
pub const RADIAL_BINS: usize = 36;

/// A 2-D point in pixel space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

impl Point2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Euclidean distance to another point.
    pub fn distance(&self, other: &Point2D) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

/// A quadratic spline segment (start, control, end) approximating a contour arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplineSegment {
    pub start: Point2D,
    pub control: Point2D,
    pub end: Point2D,
}

impl SplineSegment {
    /// Evaluate the quadratic Bézier at parameter `t` in [0, 1].
    pub fn eval(&self, t: f32) -> Point2D {
        let u = 1.0 - t;
        let x = u * u * self.start.x + 2.0 * u * t * self.control.x + t * t * self.end.x;
        let y = u * u * self.start.y + 2.0 * u * t * self.control.y + t * t * self.end.y;
        Point2D::new(x, y)
    }
}

/// Rotation- and scale-invariant descriptor of a closed shape contour.
///
/// The `radial_profile` records the maximum contour distance from the centroid
/// in each angular bin, normalized to [0, 1]. Rotation shifts the profile
/// cyclically (handled by the matcher's cyclic correlation); scale is removed by
/// the normalization. `compactness` (4πA/P²) is a pure scalar shape descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeSignature {
    pub centroid: Point2D,
    pub radial_profile: Vec<f32>,
    pub compactness: f32,
    pub area: f32,
    pub perimeter: f32,
    pub point_count: u32,
    pub hash: u64,
}

impl ShapeSignature {
    /// An empty signature used as a safe fallback when no contour is found.
    pub fn empty() -> Self {
        Self {
            centroid: Point2D::new(0.0, 0.0),
            radial_profile: vec![0.0; RADIAL_BINS],
            compactness: 0.0,
            area: 0.0,
            perimeter: 0.0,
            point_count: 0,
            hash: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.point_count == 0
    }
}

/// Extracts contours and spline shape signatures from pixel regions.
#[derive(Debug, Clone)]
pub struct SplineExtractor {
    luminance_threshold: u8,
}

impl Default for SplineExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SplineExtractor {
    pub fn new() -> Self {
        Self { luminance_threshold: 128 }
    }

    pub fn with_threshold(threshold: u8) -> Self {
        Self { luminance_threshold: threshold }
    }

    /// Extract the dominant closed contour from a region as an ordered set of
    /// boundary points. Region is `(x, y, width, height)`.
    ///
    /// Boundary pixels (foreground pixels adjacent to background) are collected,
    /// then ordered by a greedy nearest-neighbor walk so downstream spline
    /// fitting and shoelace area work on a coherent path.
    pub fn extract_contour(
        &self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
    ) -> Vec<Point2D> {
        let (rx, ry, rw, rh) = region;
        let rw = rw.min(buffer.width.saturating_sub(rx)).max(1);
        let rh = rh.min(buffer.height.saturating_sub(ry)).max(1);

        let mut boundary: Vec<Point2D> = Vec::new();
        for y in 0..rh {
            for x in 0..rw {
                if !self.is_foreground(buffer, rx + x, ry + y) {
                    continue;
                }
                // A foreground pixel is on the boundary if any 4-neighbor is
                // background (or outside the region).
                let mut edge = false;
                let neighbors = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)];
                for (dx, dy) in neighbors {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= rw as i32 || ny >= rh as i32 {
                        edge = true;
                        break;
                    }
                    if !self.is_foreground(buffer, rx + nx as usize, ry + ny as usize) {
                        edge = true;
                        break;
                    }
                }
                if edge {
                    boundary.push(Point2D::new(x as f32, y as f32));
                }
            }
        }

        self.order_by_walk(boundary)
    }

    /// Order a set of boundary points into a path via greedy nearest-neighbor.
    fn order_by_walk(&self, mut pts: Vec<Point2D>) -> Vec<Point2D> {
        if pts.len() < 3 {
            return pts;
        }
        let mut ordered = Vec::with_capacity(pts.len());
        let mut current = pts.swap_remove(0);
        ordered.push(current);
        while !pts.is_empty() {
            let mut best_i = 0;
            let mut best_d = f32::MAX;
            for (i, p) in pts.iter().enumerate() {
                let d = current.distance(p);
                if d < best_d {
                    best_d = d;
                    best_i = i;
                }
            }
            current = pts.swap_remove(best_i);
            ordered.push(current);
        }
        ordered
    }

    /// Fit a sequence of quadratic spline segments to an ordered contour.
    ///
    /// The contour is first simplified (Ramer–Douglas–Peucker) to key vertices,
    /// then each consecutive pair becomes a quadratic segment whose control
    /// point is the arc midpoint — a compact, resolution-independent shape.
    pub fn fit_spline(&self, contour: &[Point2D], epsilon: f32) -> Vec<SplineSegment> {
        if contour.len() < 3 {
            return Vec::new();
        }
        let key = rdp_simplify(contour, epsilon.max(0.5));
        if key.len() < 2 {
            return Vec::new();
        }
        let mut segments = Vec::with_capacity(key.len());
        for i in 0..key.len() {
            let start = key[i];
            let end = key[(i + 1) % key.len()];
            let control = Point2D::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
            segments.push(SplineSegment { start, control, end });
        }
        segments
    }

    /// Compute a rotation/scale-invariant [`ShapeSignature`] from an ordered contour.
    pub fn signature(&self, contour: &[Point2D]) -> ShapeSignature {
        if contour.len() < 3 {
            return ShapeSignature::empty();
        }
        let centroid = centroid_of(contour);

        // Radial profile: max distance per angular bin, then normalize.
        let mut profile = vec![0.0f32; RADIAL_BINS];
        let mut max_dist = 0.0f32;
        for p in contour {
            let dx = p.x - centroid.x;
            let dy = p.y - centroid.y;
            let dist = (dx * dx + dy * dy).sqrt();
            let mut angle = dy.atan2(dx); // -π..π
            if angle < 0.0 {
                angle += std::f32::consts::TAU;
            }
            let bin = ((angle / std::f32::consts::TAU) * RADIAL_BINS as f32) as usize % RADIAL_BINS;
            if dist > profile[bin] {
                profile[bin] = dist;
            }
            if dist > max_dist {
                max_dist = dist;
            }
        }
        if max_dist > 0.0 {
            for v in profile.iter_mut() {
                *v /= max_dist;
            }
        }

        let area = shoelace_area(contour);
        let perimeter = contour_perimeter(contour);
        let compactness = if perimeter > 0.0 {
            (4.0 * std::f32::consts::PI * area) / (perimeter * perimeter)
        } else {
            0.0
        };

        let hash = hash_profile(&profile, compactness);

        ShapeSignature {
            centroid,
            radial_profile: profile,
            compactness,
            area,
            perimeter,
            point_count: contour.len() as u32,
            hash,
        }
    }

    /// Convenience: extract a shape signature directly from a pixel region.
    pub fn extract_signature(
        &self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
    ) -> ShapeSignature {
        let contour = self.extract_contour(buffer, region);
        self.signature(&contour)
    }

    /// A pixel is foreground if its luminance is below the threshold (dark ink
    /// on light background — the common captcha rendering).
    fn is_foreground(&self, buffer: &PixelBuffer, x: usize, y: usize) -> bool {
        let px = buffer.get_pixel(x, y);
        if px[3] == 0 {
            return false; // fully transparent
        }
        let lum = (px[0] as u32 + px[1] as u32 + px[2] as u32) / 3;
        lum < self.luminance_threshold as u32
    }
}

/// Centroid (arithmetic mean) of a set of points.
fn centroid_of(pts: &[Point2D]) -> Point2D {
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    for p in pts {
        sx += p.x;
        sy += p.y;
    }
    let n = pts.len().max(1) as f32;
    Point2D::new(sx / n, sy / n)
}

/// Shoelace polygon area (absolute) for an ordered closed contour.
fn shoelace_area(pts: &[Point2D]) -> f32 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut acc = 0.0f32;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        acc += a.x * b.y - b.x * a.y;
    }
    (acc * 0.5).abs()
}

/// Total closed-path length of an ordered contour.
fn contour_perimeter(pts: &[Point2D]) -> f32 {
    if pts.len() < 2 {
        return 0.0;
    }
    let mut per = 0.0f32;
    for i in 0..pts.len() {
        per += pts[i].distance(&pts[(i + 1) % pts.len()]);
    }
    per
}

/// Ramer–Douglas–Peucker polyline simplification.
fn rdp_simplify(pts: &[Point2D], epsilon: f32) -> Vec<Point2D> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    rdp_recurse(pts, 0, pts.len() - 1, epsilon, &mut keep);
    pts.iter()
        .zip(keep.iter())
        .filter_map(|(p, &k)| if k { Some(*p) } else { None })
        .collect()
}

fn rdp_recurse(pts: &[Point2D], start: usize, end: usize, epsilon: f32, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let a = pts[start];
    let b = pts[end];
    let mut max_d = 0.0f32;
    let mut max_i = start;
    for (i, p) in pts.iter().enumerate().take(end).skip(start + 1) {
        let d = perpendicular_distance(*p, a, b);
        if d > max_d {
            max_d = d;
            max_i = i;
        }
    }
    if max_d > epsilon {
        keep[max_i] = true;
        rdp_recurse(pts, start, max_i, epsilon, keep);
        rdp_recurse(pts, max_i, end, epsilon, keep);
    }
}

/// Perpendicular distance from point `p` to the line through `a`–`b`.
fn perpendicular_distance(p: Point2D, a: Point2D, b: Point2D) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-6 {
        return p.distance(&a);
    }
    ((dx * (a.y - p.y) - (a.x - p.x) * dy).abs()) / len
}

/// FNV-1a hash of a quantized radial profile plus compactness.
fn hash_profile(profile: &[f32], compactness: f32) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &v in profile {
        let q = (v * 255.0).round().clamp(0.0, 255.0) as u8;
        hash ^= q as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let cq = (compactness * 255.0).round().clamp(0.0, 255.0) as u8;
    hash ^= cq as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw a filled dark square on a light buffer.
    fn buffer_with_square(size: usize, sq_x: usize, sq_y: usize, sq_w: usize) -> PixelBuffer {
        let mut buf = PixelBuffer::new(size, size);
        buf.fill_rect(sq_x, sq_y, sq_w, sq_w, 20, 20, 20, 255);
        buf
    }

    /// Draw a filled dark disc (approx circle) on a light buffer.
    fn buffer_with_disc(size: usize, cx: usize, cy: usize, radius: usize) -> PixelBuffer {
        let mut buf = PixelBuffer::new(size, size);
        for y in 0..size {
            for x in 0..size {
                let dx = x as i32 - cx as i32;
                let dy = y as i32 - cy as i32;
                if (dx * dx + dy * dy) <= (radius * radius) as i32 {
                    buf.set_pixel(x, y, 20, 20, 20, 255);
                }
            }
        }
        buf
    }

    #[test]
    fn point_distance_is_correct() {
        let a = Point2D::new(0.0, 0.0);
        let b = Point2D::new(3.0, 4.0);
        assert!((a.distance(&b) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn extract_contour_finds_square_boundary() {
        let buf = buffer_with_square(60, 20, 20, 20);
        let ex = SplineExtractor::new();
        let contour = ex.extract_contour(&buf, (0, 0, 60, 60));
        // A 20x20 square perimeter has ~76 boundary pixels; expect a healthy count.
        assert!(contour.len() > 40, "got {} boundary points", contour.len());
    }

    #[test]
    fn signature_of_square_is_deterministic() {
        let buf = buffer_with_square(60, 20, 20, 20);
        let ex = SplineExtractor::new();
        let s1 = ex.extract_signature(&buf, (0, 0, 60, 60));
        let s2 = ex.extract_signature(&buf, (0, 0, 60, 60));
        assert_eq!(s1.hash, s2.hash);
        assert_eq!(s1.radial_profile.len(), RADIAL_BINS);
    }

    #[test]
    fn disc_is_more_compact_than_square() {
        let ex = SplineExtractor::new();
        let disc = ex.extract_signature(&buffer_with_disc(80, 40, 40, 25), (0, 0, 80, 80));
        let square = ex.extract_signature(&buffer_with_square(80, 20, 20, 40), (0, 0, 80, 80));
        // A circle has compactness ~1.0; a square ~0.785. Disc should score higher.
        assert!(
            disc.compactness > square.compactness,
            "disc {} vs square {}",
            disc.compactness,
            square.compactness
        );
    }

    #[test]
    fn empty_region_yields_empty_signature() {
        let buf = PixelBuffer::new(40, 40); // all white
        let ex = SplineExtractor::new();
        let sig = ex.extract_signature(&buf, (0, 0, 40, 40));
        assert!(sig.is_empty());
    }

    #[test]
    fn fit_spline_produces_segments() {
        let buf = buffer_with_square(60, 20, 20, 20);
        let ex = SplineExtractor::new();
        let contour = ex.extract_contour(&buf, (0, 0, 60, 60));
        let segments = ex.fit_spline(&contour, 2.0);
        // A square simplifies to ~4 corners → ~4 segments.
        assert!(!segments.is_empty());
        // Spline evaluation stays finite.
        let mid = segments[0].eval(0.5);
        assert!(mid.x.is_finite() && mid.y.is_finite());
    }

    #[test]
    fn rdp_simplifies_collinear_points() {
        let line: Vec<Point2D> = (0..10).map(|i| Point2D::new(i as f32, 0.0)).collect();
        let simplified = rdp_simplify(&line, 0.5);
        // A straight run collapses to its two endpoints.
        assert_eq!(simplified.len(), 2);
    }
}
