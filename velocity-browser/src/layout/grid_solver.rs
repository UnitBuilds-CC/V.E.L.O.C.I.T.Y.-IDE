use crate::layout::LayoutBox;

#[derive(Debug, Clone)]
pub struct GridTrack {
    pub flex_fraction: f32, // e.g. 1fr, 2fr
    pub px_size: f32,
}

pub struct GridTrackSolver;

impl GridTrackSolver {
    pub fn solve_tracks(container_width: f32, track_specs: &[GridTrack]) -> Vec<f32> {
        let mut fixed_px = 0.0;
        let mut total_fr = 0.0;

        for spec in track_specs {
            fixed_px += spec.px_size;
            total_fr += spec.flex_fraction;
        }

        let remaining_px = (container_width - fixed_px).max(0.0);
        let fr_unit = if total_fr > 0.0 { remaining_px / total_fr } else { 0.0 };

        track_specs.iter().map(|s| s.px_size + s.flex_fraction * fr_unit).collect()
    }
}
