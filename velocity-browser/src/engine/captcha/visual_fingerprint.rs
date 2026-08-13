//! OCR-based visual fingerprinting for zero-token captcha variant detection.
//!
//! Extends the `VelocityOcrEngine`'s pixel-scanning approach to produce compact
//! structural signatures from raw RGBA data. The fingerprint acts as a cache key
//! for the template store: if we've solved this visual layout before, replay the
//! stored solution without spending any LLM tokens.

use crate::engine::PixelBuffer;

/// Compact visual signature of a challenge region, derived from pixel analysis.
/// Costs zero tokens — pure arithmetic on RGBA data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VisualFingerprint {
    /// Primary hash for template lookup (combines all signals below via FNV-1a).
    pub hash: u64,
    /// Detected grid dimensions (rows, cols) from regular spacing of dark regions.
    pub grid_signature: Option<(u8, u8)>,
    /// Number of distinct opaque regions detected.
    pub region_count: u8,
    /// Aspect ratio bucket of the challenge container.
    pub aspect_bucket: AspectBucket,
    /// Horizontal symmetry score (0-255). High = slider/checkbox. Low = image grid.
    pub symmetry_score: u8,
    /// Density of dark pixels in the region (0-255).
    pub density: u8,
}

/// Coarse aspect ratio classification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AspectBucket {
    Wide,   // width > 1.5 * height
    Square, // roughly equal
    Tall,   // height > 1.5 * width
}

/// Challenge archetype derived from visual fingerprint analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum ChallengeArchetype {
    Checkbox,
    ImageGridSelect,
    TileFlip,
    Slider,
    TextEntry,
    MultiRound,
    Unknown,
}

/// Lightweight pixel-analysis engine that produces structural fingerprints
/// from rasterized challenge regions.
pub struct VisualFingerprinter {
    luminance_threshold: u8,
}

impl Default for VisualFingerprinter {
    fn default() -> Self {
        Self::new()
    }
}

impl VisualFingerprinter {
    pub fn new() -> Self {
        Self {
            luminance_threshold: 128,
        }
    }

    pub fn with_threshold(threshold: u8) -> Self {
        Self {
            luminance_threshold: threshold,
        }
    }

    /// Produce a full visual fingerprint from a sub-region of a pixel buffer.
    /// Region is (x, y, width, height).
    pub fn fingerprint(
        &self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
    ) -> VisualFingerprint {
        let (rx, ry, rw, rh) = region;
        let rw = rw.min(buffer.width.saturating_sub(rx)).max(1);
        let rh = rh.min(buffer.height.saturating_sub(ry)).max(1);

        let density = self.compute_density(buffer, rx, ry, rw, rh);
        let symmetry_score = self.compute_symmetry(buffer, rx, ry, rw, rh);
        let grid_signature = self.detect_grid(buffer, rx, ry, rw, rh);
        let region_count = self.count_regions(buffer, rx, ry, rw, rh);
        let aspect_bucket = Self::classify_aspect(rw, rh);

        let hash = Self::fnv1a_hash(&[
            density as u64,
            symmetry_score as u64,
            region_count as u64,
            grid_signature
                .map(|(r, c)| (r as u64) << 8 | c as u64)
                .unwrap_or(0),
            match aspect_bucket {
                AspectBucket::Wide => 1,
                AspectBucket::Square => 2,
                AspectBucket::Tall => 3,
            },
        ]);

        VisualFingerprint {
            hash,
            grid_signature,
            region_count,
            aspect_bucket,
            symmetry_score,
            density,
        }
    }

    /// Detect grid structure via projection profile analysis.
    /// Returns (rows, cols) if a regular grid is detected.
    pub fn detect_grid(
        &self,
        buffer: &PixelBuffer,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) -> Option<(u8, u8)> {
        if rw < 30 || rh < 30 {
            return None;
        }

        // Vertical projection: count dark pixels per column
        let col_profile = self.vertical_projection(buffer, rx, ry, rw, rh);
        // Horizontal projection: count dark pixels per row
        let row_profile = self.horizontal_projection(buffer, rx, ry, rw, rh);

        // Find gaps (columns/rows with very few dark pixels = grid lines)
        let col_gaps = Self::find_regular_gaps(&col_profile, rw);
        let row_gaps = Self::find_regular_gaps(&row_profile, rh);

        // Need at least 2 gaps in each direction to form a grid
        let cols = if col_gaps >= 2 {
            col_gaps + 1
        } else {
            return None;
        };
        let rows = if row_gaps >= 2 {
            row_gaps + 1
        } else {
            return None;
        };

        // Sanity: grids are typically 2x2 to 5x5
        if (2..=5).contains(&rows) && (2..=5).contains(&cols) {
            Some((rows as u8, cols as u8))
        } else {
            None
        }
    }

    /// Classify the challenge archetype from a visual fingerprint.
    pub fn classify_archetype(fp: &VisualFingerprint) -> ChallengeArchetype {
        match fp.grid_signature {
            Some((r, c)) if r >= 3 && c >= 3 => {
                // Grid with high density = image selection; varying = tile flip
                if fp.density > 100 {
                    ChallengeArchetype::ImageGridSelect
                } else {
                    ChallengeArchetype::TileFlip
                }
            }
            Some(_) => ChallengeArchetype::ImageGridSelect,
            None => {
                if fp.symmetry_score > 180 && fp.aspect_bucket == AspectBucket::Wide {
                    ChallengeArchetype::Slider
                } else if fp.region_count <= 3 && fp.aspect_bucket == AspectBucket::Square {
                    ChallengeArchetype::Checkbox
                } else if fp.density > 150 && fp.region_count > 5 {
                    ChallengeArchetype::TextEntry
                } else {
                    ChallengeArchetype::Unknown
                }
            }
        }
    }

    // --- Internal helpers ---

    fn compute_density(
        &self,
        buffer: &PixelBuffer,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) -> u8 {
        let mut dark_count = 0u32;
        let mut total = 0u32;
        for y in (ry..ry + rh).step_by(4) {
            for x in (rx..rx + rw).step_by(4) {
                let pixel = buffer.get_pixel(x, y);
                let lum = (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3;
                if lum < self.luminance_threshold as u32 {
                    dark_count += 1;
                }
                total += 1;
            }
        }
        if total == 0 {
            return 0;
        }
        ((dark_count as f64 / total as f64) * 255.0) as u8
    }

    fn compute_symmetry(
        &self,
        buffer: &PixelBuffer,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) -> u8 {
        if rw < 4 {
            return 128;
        }
        let half_w = rw / 2;
        let mut matches = 0u32;
        let mut total = 0u32;

        for y in (ry..ry + rh).step_by(8) {
            for dx in (0..half_w).step_by(4) {
                let left = buffer.get_pixel(rx + dx, y);
                let right = buffer.get_pixel(rx + rw - 1 - dx, y);
                let lum_l = (left[0] as u32 + left[1] as u32 + left[2] as u32) / 3;
                let lum_r = (right[0] as u32 + right[1] as u32 + right[2] as u32) / 3;
                let diff = (lum_l as i32 - lum_r as i32).unsigned_abs();
                if diff < 40 {
                    matches += 1;
                }
                total += 1;
            }
        }
        if total == 0 {
            return 128;
        }
        ((matches as f64 / total as f64) * 255.0) as u8
    }

    fn count_regions(
        &self,
        buffer: &PixelBuffer,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) -> u8 {
        // Simplified connected-component count via run-length transitions
        let mut regions = 0u8;
        let mut in_region = false;
        for y in (ry..ry + rh).step_by(10) {
            for x in (rx..rx + rw).step_by(10) {
                let pixel = buffer.get_pixel(x, y);
                let lum = (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3;
                let is_dark = lum < self.luminance_threshold as u32;
                if is_dark && !in_region {
                    regions = regions.saturating_add(1);
                    in_region = true;
                } else if !is_dark {
                    in_region = false;
                }
            }
            in_region = false; // reset per row
        }
        regions
    }

    fn vertical_projection(
        &self,
        buffer: &PixelBuffer,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) -> Vec<u32> {
        let mut profile = vec![0u32; rw];
        for (x, count) in profile.iter_mut().enumerate() {
            for y in (0..rh).step_by(2) {
                let pixel = buffer.get_pixel(rx + x, ry + y);
                let lum = (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3;
                if lum < self.luminance_threshold as u32 {
                    *count += 1;
                }
            }
        }
        profile
    }

    fn horizontal_projection(
        &self,
        buffer: &PixelBuffer,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) -> Vec<u32> {
        let mut profile = vec![0u32; rh];
        for (y, count) in profile.iter_mut().enumerate() {
            for x in (0..rw).step_by(2) {
                let pixel = buffer.get_pixel(rx + x, ry + y);
                let lum = (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3;
                if lum < self.luminance_threshold as u32 {
                    *count += 1;
                }
            }
        }
        profile
    }

    /// Find regularly-spaced gaps in a projection profile.
    /// A "gap" is a run of near-zero values. Returns the number of internal gaps.
    fn find_regular_gaps(profile: &[u32], size: usize) -> usize {
        if size < 20 {
            return 0;
        }
        let threshold = 2u32; // near-zero
        let min_gap_width = size / 20; // gaps must be at least 5% of dimension
        let mut gaps = 0;
        let mut gap_start: Option<usize> = None;

        // Skip edges (first/last 10%)
        let margin = size / 10;
        let end = size.saturating_sub(margin);
        for (i, &val) in profile.iter().enumerate().take(end).skip(margin) {
            if val <= threshold {
                if gap_start.is_none() {
                    gap_start = Some(i);
                }
            } else if let Some(start) = gap_start {
                let gap_width = i - start;
                if gap_width >= min_gap_width {
                    gaps += 1;
                }
                gap_start = None;
            }
        }
        gaps
    }

    fn classify_aspect(w: usize, h: usize) -> AspectBucket {
        let ratio = w as f64 / h.max(1) as f64;
        if ratio > 1.5 {
            AspectBucket::Wide
        } else if ratio < 0.67 {
            AspectBucket::Tall
        } else {
            AspectBucket::Square
        }
    }

    /// FNV-1a hash combining multiple u64 values into a single fingerprint.
    fn fnv1a_hash(values: &[u64]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
        for &val in values {
            for byte in val.to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x100000001b3); // FNV prime
            }
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grid_buffer(rows: usize, cols: usize, cell_size: usize, gap: usize) -> PixelBuffer {
        let w = cols * cell_size + (cols - 1) * gap + gap * 2;
        let h = rows * cell_size + (rows - 1) * gap + gap * 2;
        let mut buf = PixelBuffer::new(w, h);
        // Fill background white (already default)
        // Draw dark cells
        for r in 0..rows {
            for c in 0..cols {
                let x0 = gap + c * (cell_size + gap);
                let y0 = gap + r * (cell_size + gap);
                buf.fill_rect(x0, y0, cell_size, cell_size, 30, 30, 30, 255);
            }
        }
        buf
    }

    #[test]
    fn grid_detection_3x3() {
        // Use wider gaps (10px) so they exceed the 5% minimum threshold
        let buf = make_grid_buffer(3, 3, 40, 10);
        let fp = VisualFingerprinter::new();
        let grid = fp.detect_grid(&buf, 0, 0, buf.width, buf.height);
        assert_eq!(grid, Some((3, 3)));
    }

    #[test]
    fn grid_detection_4x4() {
        // Use wider gaps (8px) for 4x4 grid
        let buf = make_grid_buffer(4, 4, 30, 8);
        let fp = VisualFingerprinter::new();
        let grid = fp.detect_grid(&buf, 0, 0, buf.width, buf.height);
        assert_eq!(grid, Some((4, 4)));
    }

    #[test]
    fn symmetry_high_for_symmetric_content() {
        // Create a vertically symmetric buffer (slider-like)
        let mut buf = PixelBuffer::new(200, 50);
        // Draw a centered dark bar (symmetric)
        buf.fill_rect(80, 20, 40, 10, 20, 20, 20, 255);
        let fp = VisualFingerprinter::new();
        let result = fp.fingerprint(&buf, (0, 0, 200, 50));
        assert!(
            result.symmetry_score > 200,
            "symmetric content should have high symmetry, got {}",
            result.symmetry_score
        );
    }

    #[test]
    fn symmetry_low_for_asymmetric_grid() {
        // Create an asymmetric buffer
        let mut buf = PixelBuffer::new(200, 200);
        buf.fill_rect(10, 10, 50, 50, 20, 20, 20, 255);
        buf.fill_rect(140, 140, 50, 50, 20, 20, 20, 255);
        let fp = VisualFingerprinter::new();
        let result = fp.fingerprint(&buf, (0, 0, 200, 200));
        assert!(
            result.symmetry_score < 200,
            "asymmetric content should have lower symmetry, got {}",
            result.symmetry_score
        );
    }

    #[test]
    fn density_classification() {
        // Mostly dark buffer
        let mut buf = PixelBuffer::new(100, 100);
        buf.fill_rect(0, 0, 100, 100, 10, 10, 10, 255);
        let fp = VisualFingerprinter::new();
        let result = fp.fingerprint(&buf, (0, 0, 100, 100));
        assert!(
            result.density > 200,
            "dark buffer should have high density, got {}",
            result.density
        );

        // Mostly white buffer
        let buf2 = PixelBuffer::new(100, 100);
        let result2 = fp.fingerprint(&buf2, (0, 0, 100, 100));
        assert!(
            result2.density < 50,
            "white buffer should have low density, got {}",
            result2.density
        );
    }

    #[test]
    fn aspect_bucket_classification() {
        assert_eq!(
            VisualFingerprinter::classify_aspect(300, 100),
            AspectBucket::Wide
        );
        assert_eq!(
            VisualFingerprinter::classify_aspect(100, 100),
            AspectBucket::Square
        );
        assert_eq!(
            VisualFingerprinter::classify_aspect(100, 300),
            AspectBucket::Tall
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let buf = make_grid_buffer(3, 3, 40, 6);
        let fp = VisualFingerprinter::new();
        let r1 = fp.fingerprint(&buf, (0, 0, buf.width, buf.height));
        let r2 = fp.fingerprint(&buf, (0, 0, buf.width, buf.height));
        assert_eq!(r1.hash, r2.hash);
        assert_eq!(r1, r2);
    }
}
