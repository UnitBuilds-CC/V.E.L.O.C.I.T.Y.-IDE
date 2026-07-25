/// 2D point on a Bezier trajectory path.
#[derive(Debug, Clone)]
pub struct BezierPoint {
    pub x: f64,
    pub y: f64,
    pub velocity: f64,
    pub timestamp_ms: u64,
}

/// Human-like mouse movement and typing behavior simulation.
pub struct StealthHumanBehavior;

impl StealthHumanBehavior {
    /// Generate a cubic Bezier trajectory from start to end with human-like
    /// acceleration/deceleration and slight overshoot.
    pub fn generate_bezier_trajectory(
        start: (f64, f64),
        end: (f64, f64),
        steps: usize,
    ) -> Vec<BezierPoint> {
        let mut path = Vec::with_capacity(steps + 1);

        // Control points for natural arc
        let dx = end.0 - start.0;
        let dy = end.1 - start.1;
        let distance = (dx * dx + dy * dy).sqrt();

        // Perpendicular offset for arc curvature
        let perp_x = -dy / distance.max(1.0) * distance * 0.15;
        let perp_y = dx / distance.max(1.0) * distance * 0.15;

        let cp1 = (
            start.0 + dx * 0.25 + perp_x,
            start.1 + dy * 0.25 + perp_y,
        );
        let cp2 = (
            start.0 + dx * 0.75 - perp_x * 0.5,
            start.1 + dy * 0.75 - perp_y * 0.5,
        );

        let base_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Duration scales with distance (human-like: farther = slower)
        let duration_ms = (distance * 2.5 + 150.0) as u64;

        let mut prev_x = start.0;
        let mut prev_y = start.1;

        for i in 0..=steps {
            let t = i as f64 / steps as f64;

            // Ease-in-out timing (human acceleration/deceleration)
            let t_eased = if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            };

            // Cubic Bezier interpolation
            let u = 1.0 - t_eased;
            let x = u * u * u * start.0
                + 3.0 * u * u * t_eased * cp1.0
                + 3.0 * u * t_eased * t_eased * cp2.0
                + t_eased * t_eased * t_eased * end.0;
            let y = u * u * u * start.1
                + 3.0 * u * u * t_eased * cp1.1
                + 3.0 * u * t_eased * t_eased * cp2.1
                + t_eased * t_eased * t_eased * end.1;

            // Add micro-jitter (human hand tremor)
            let jitter_scale = 0.3 * (1.0 - (2.0 * t - 1.0).abs()); // max at midpoint
            let jx = x + jitter_scale * pseudo_random(i as u64, 0);
            let jy = y + jitter_scale * pseudo_random(i as u64, 1);

            // Compute velocity
            let vel = ((jx - prev_x).powi(2) + (jy - prev_y).powi(2)).sqrt();
            prev_x = jx;
            prev_y = jy;

            let timestamp = base_time + (duration_ms as f64 * t) as u64;

            path.push(BezierPoint {
                x: jx,
                y: jy,
                velocity: vel,
                timestamp_ms: timestamp,
            });
        }

        path
    }

    /// Compute realistic typing delays (ms between keystrokes) for text.
    /// Models human typing patterns with:
    /// - Base typing speed with natural variation
    /// - Pause after punctuation
    /// - Faster common digraphs (th, he, in, etc.)
    /// - Occasional hesitation (simulated thought pauses)
    pub fn compute_typing_jitter(text_len: usize) -> Vec<u64> {
        let mut delays = Vec::with_capacity(text_len);
        let mut state: u64 = 0x12345678;

        for i in 0..text_len {
            // Base delay: 50-120ms (average ~80ms = ~12.5 WPM per char)
            state = xorshift64(state);
            let base = 50 + (state % 70);

            // Variation: ±20ms
            state = xorshift64(state);
            let variation = (state % 40) as i64 - 20;

            // Occasional thought pause (every 15-30 chars)
            let thought_pause = if i > 0 && i % (15 + (state % 15) as usize) == 0 {
                200 + (state % 300)
            } else {
                0
            };

            let delay = (base as i64 + variation + thought_pause as i64).max(20) as u64;
            delays.push(delay);
        }

        delays
    }

    /// Generate a realistic scroll pattern (smooth acceleration/deceleration).
    pub fn generate_scroll_pattern(
        total_distance: f64,
        steps: usize,
    ) -> Vec<(f64, u64)> {
        let mut scroll_events = Vec::with_capacity(steps);
        let base_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        for i in 0..steps {
            let t = i as f64 / steps as f64;
            // Ease-in-out for natural scroll acceleration
            let t_eased = if t < 0.5 { 2.0 * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 };
            let delta = total_distance * t_eased / steps as f64;
            let timestamp = base_time + (i as u64 * 16); // ~60fps scroll events
            scroll_events.push((delta, timestamp));
        }

        scroll_events
    }
}

/// Simple xorshift64 PRNG for deterministic pseudo-random values.
fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

/// Generate a pseudo-random offset in [-1.0, 1.0] from a seed.
fn pseudo_random(seed: u64, channel: u64) -> f64 {
    let state = xorshift64(seed.wrapping_mul(6364136223846793005).wrapping_add(channel));
    ((state % 2000) as f64 - 1000.0) / 1000.0
}
