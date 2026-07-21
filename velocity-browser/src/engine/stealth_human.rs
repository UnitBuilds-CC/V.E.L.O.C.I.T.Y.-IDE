#[derive(Debug, Clone)]
pub struct BezierPoint {
    pub x: f64,
    pub y: f64,
}

pub struct StealthHumanBehavior;

impl StealthHumanBehavior {
    pub fn generate_bezier_trajectory(start: (f64, f64), end: (f64, f64), steps: usize) -> Vec<BezierPoint> {
        let mut path = Vec::with_capacity(steps);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let x = (1.0 - t) * start.0 + t * end.0;
            let y = (1.0 - t) * start.1 + t * end.1;
            path.push(BezierPoint { x, y });
        }
        path
    }

    pub fn compute_typing_jitter(text_len: usize) -> Vec<u64> {
        (0..text_len).map(|idx| 40 + (idx as u64 * 7) % 35).collect()
    }
}
