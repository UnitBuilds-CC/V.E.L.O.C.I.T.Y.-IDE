/// A CSS grid track definition supporting fr, px, minmax(), auto, and repeat().
#[derive(Debug, Clone)]
pub struct GridTrack {
    pub flex_fraction: f32,
    pub px_size: f32,
    /// Minimum size for minmax() tracks.
    pub min_size: Option<f32>,
    /// Maximum size for minmax() tracks.
    pub max_size: Option<f32>,
    /// Whether this track is 'auto' sized.
    pub is_auto: bool,
    /// Column/row span for items placed in this track.
    pub span: usize,
}

impl GridTrack {
    /// Create a fr track.
    pub fn fr(flex: f32) -> Self {
        Self { flex_fraction: flex, px_size: 0.0, min_size: None, max_size: None, is_auto: false, span: 1 }
    }
    /// Create a px track.
    pub fn px(size: f32) -> Self {
        Self { flex_fraction: 0.0, px_size: size, min_size: None, max_size: None, is_auto: false, span: 1 }
    }
    /// Create an auto track.
    pub fn auto() -> Self {
        Self { flex_fraction: 0.0, px_size: 0.0, min_size: None, max_size: None, is_auto: true, span: 1 }
    }
    /// Create a minmax() track.
    pub fn minmax(min: f32, max: f32) -> Self {
        Self { flex_fraction: 0.0, px_size: 0.0, min_size: Some(min), max_size: Some(max), is_auto: false, span: 1 }
    }
}

/// A grid item placement with row/column positions.
#[derive(Debug, Clone)]
pub struct GridItem {
    pub col_start: usize,
    pub col_end: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub min_width: f32,
    pub min_height: f32,
}

/// Grid track solver supporting fr, px, minmax(), auto, repeat(), and auto-placement.
pub struct GridTrackSolver;

impl GridTrackSolver {
    /// Solve track sizes given a container dimension and track specs.
    pub fn solve_tracks(container_width: f32, track_specs: &[GridTrack]) -> Vec<f32> {
        let mut sizes = vec![0.0f32; track_specs.len()];

        // Phase 1: Assign fixed px tracks
        for (i, spec) in track_specs.iter().enumerate() {
            if spec.px_size > 0.0 && spec.flex_fraction == 0.0 && !spec.is_auto {
                let mut size = spec.px_size;
                if let Some(min) = spec.min_size { size = size.max(min); }
                if let Some(max) = spec.max_size { size = size.min(max); }
                sizes[i] = size;
            }
        }

        // Phase 2: Resolve auto tracks (use content minimum or 0)
        for (i, spec) in track_specs.iter().enumerate() {
            if spec.is_auto {
                sizes[i] = spec.min_size.unwrap_or(0.0);
            }
        }

        // Phase 3: Resolve minmax() tracks
        for (i, spec) in track_specs.iter().enumerate() {
            if (spec.min_size.is_some() || spec.max_size.is_some())
                && spec.flex_fraction == 0.0 && !spec.is_auto && spec.px_size == 0.0 {
                    let min = spec.min_size.unwrap_or(0.0);
                    let max = spec.max_size.unwrap_or(f32::MAX);
                    sizes[i] = min.min(max);
                }
        }

        // Phase 4: Distribute remaining space to fr tracks
        let used: f32 = sizes.iter().sum();
        let remaining = (container_width - used).max(0.0);
        let total_fr: f32 = track_specs.iter()
            .filter(|s| s.flex_fraction > 0.0)
            .map(|s| s.flex_fraction)
            .sum();

        if total_fr > 0.0 {
            let fr_unit = remaining / total_fr;
            for (i, spec) in track_specs.iter().enumerate() {
                if spec.flex_fraction > 0.0 {
                    let mut size = spec.flex_fraction * fr_unit;
                    if let Some(min) = spec.min_size { size = size.max(min); }
                    if let Some(max) = spec.max_size { size = size.min(max); }
                    sizes[i] = size;
                }
            }
        }

        sizes
    }

    /// Expand repeat(auto-fill, track_spec) into concrete track list.
    pub fn expand_repeat(container_width: f32, track_px: f32, gap: f32) -> Vec<GridTrack> {
        if track_px <= 0.0 {
            return vec![GridTrack::fr(1.0)];
        }
        let mut count = 0;
        let mut used = 0.0;
        loop {
            let next = used + track_px + if count > 0 { gap } else { 0.0 };
            if next > container_width { break; }
            used = next;
            count += 1;
            if count > 1000 { break; } // safety limit
        }
        (0..count.max(1)).map(|_| GridTrack::px(track_px)).collect()
    }

    /// Auto-place grid items into tracks, returning their resolved positions.
    pub fn auto_place_items(
        items: &[GridItem],
        col_count: usize,
    ) -> Vec<(usize, usize, usize, usize)> {
        let mut placed = Vec::new();
        let mut cursor = 0usize; // linear cell index

        for item in items {
            // Check if item has any explicit positioning (non-default coordinates)
            let is_explicit = item.col_start > 0 || item.row_start > 0
                || item.col_end > 1 || item.row_end > 1;
            if is_explicit {
                // Explicitly placed
                placed.push((item.col_start, item.row_start, item.col_end, item.row_end));
                continue;
            }
            // Auto-place: find next available cell
            let span_cols = (item.col_end - item.col_start).max(1);
            let span_rows = (item.row_end - item.row_start).max(1);

            loop {
                let col = cursor % col_count;
                let row = cursor / col_count;
                // Check if item fits
                if col + span_cols <= col_count {
                    placed.push((col + 1, row + 1, col + span_cols, row + span_rows));
                    cursor += span_cols;
                    break;
                }
                // Move to next row
                cursor = (row + 1) * col_count;
                if cursor > 10000 { break; } // safety
            }
        }

        placed
    }

    /// Compute gap (gutter) between tracks.
    pub fn apply_gap(sizes: &mut [f32], gap: f32) {
        if sizes.len() <= 1 { return; }
        let total_gap = gap * (sizes.len() - 1) as f32;
        // Distribute gap by reducing fr tracks proportionally
        let total_size: f32 = sizes.iter().sum();
        let available = total_size - total_gap;
        if available > 0.0 && total_size > 0.0 {
            let scale = available / total_size;
            for s in sizes.iter_mut() {
                *s *= scale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fr_tracks() {
        let specs = vec![GridTrack::fr(1.0), GridTrack::fr(2.0), GridTrack::fr(1.0)];
        let sizes = GridTrackSolver::solve_tracks(400.0, &specs);
        assert_eq!(sizes.len(), 3);
        assert!((sizes[0] - 100.0).abs() < 0.01);
        assert!((sizes[1] - 200.0).abs() < 0.01);
        assert!((sizes[2] - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_mixed_fr_px() {
        let specs = vec![GridTrack::px(100.0), GridTrack::fr(1.0), GridTrack::fr(1.0)];
        let sizes = GridTrackSolver::solve_tracks(500.0, &specs);
        assert!((sizes[0] - 100.0).abs() < 0.01);
        assert!((sizes[1] - 200.0).abs() < 0.01);
        assert!((sizes[2] - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_minmax() {
        let specs = vec![GridTrack::minmax(100.0, 300.0)];
        let sizes = GridTrackSolver::solve_tracks(500.0, &specs);
        assert!(sizes[0] >= 100.0);
        assert!(sizes[0] <= 300.0);
    }

    #[test]
    fn test_auto_track() {
        let specs = vec![GridTrack::auto(), GridTrack::fr(1.0)];
        let sizes = GridTrackSolver::solve_tracks(400.0, &specs);
        assert_eq!(sizes[0], 0.0); // auto with no content = 0
        assert!((sizes[1] - 400.0).abs() < 0.01);
    }

    #[test]
    fn test_expand_repeat() {
        let tracks = GridTrackSolver::expand_repeat(300.0, 100.0, 0.0);
        assert_eq!(tracks.len(), 3);
    }

    #[test]
    fn test_expand_repeat_with_gap() {
        let tracks = GridTrackSolver::expand_repeat(320.0, 100.0, 10.0);
        // 100 + 10 + 100 + 10 + 100 = 320
        assert_eq!(tracks.len(), 3);
    }

    #[test]
    fn test_auto_place() {
        let items = vec![
            GridItem { col_start: 0, col_end: 1, row_start: 0, row_end: 1, min_width: 0.0, min_height: 0.0 },
            GridItem { col_start: 0, col_end: 1, row_start: 0, row_end: 1, min_width: 0.0, min_height: 0.0 },
            GridItem { col_start: 0, col_end: 1, row_start: 0, row_end: 1, min_width: 0.0, min_height: 0.0 },
        ];
        let placed = GridTrackSolver::auto_place_items(&items, 2);
        assert_eq!(placed.len(), 3);
    }

    #[test]
    fn test_explicit_placement() {
        let items = vec![
            GridItem { col_start: 2, col_end: 3, row_start: 1, row_end: 2, min_width: 0.0, min_height: 0.0 },
        ];
        let placed = GridTrackSolver::auto_place_items(&items, 3);
        assert_eq!(placed[0], (2, 1, 3, 2));
    }

    #[test]
    fn test_span_item() {
        let items = vec![
            GridItem { col_start: 0, col_end: 2, row_start: 0, row_end: 1, min_width: 0.0, min_height: 0.0 },
        ];
        let placed = GridTrackSolver::auto_place_items(&items, 3);
        assert_eq!(placed[0].2 - placed[0].0, 2); // spans 2 columns
    }
}
