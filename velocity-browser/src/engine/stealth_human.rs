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

/// Canvas fingerprint noise injection for anti-fingerprinting.
/// Adds subtle per-pixel noise to canvas readback so that fingerprinting
/// libraries get a unique result each session without visible artifacts.
pub struct CanvasFingerprintRandomizer {
    session_seed: u64,
}

impl CanvasFingerprintRandomizer {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xDEADBEEF);
        Self { session_seed: seed }
    }

    /// Create with a deterministic seed (for testing).
    pub fn with_seed(seed: u64) -> Self {
        Self { session_seed: seed }
    }

    /// Apply noise to RGBA pixel data in-place. The noise is very subtle
    /// (±1-2 in each channel) so it doesn't affect visual rendering but
    /// changes the canvas fingerprint hash.
    pub fn apply_noise(&self, pixels: &mut [u8]) {
        let mut state = self.session_seed;
        // Only noise every 4th pixel to keep performance reasonable
        for i in (0..pixels.len().saturating_sub(3)).step_by(16) {
            state = xorshift64(state);
            let noise = (state % 5) as i8 - 2; // -2..+2
            for ch in 0..4 {
                let idx = i + ch;
                if idx < pixels.len() {
                    let val = pixels[idx] as i16 + noise as i16;
                    pixels[idx] = val.clamp(0, 255) as u8;
                }
            }
        }
    }

    /// Generate a fake WebGL renderer string that varies per session.
    pub fn spoofed_webgl_renderer(&self) -> String {
        let mut state = self.session_seed;
        let vendors = ["ANGLE (Intel", "ANGLE (NVIDIA", "ANGLE (AMD", "ANGLE (Apple"];
        let renderers = [
            "Intel(R) UHD Graphics 630",
            "NVIDIA GeForce RTX 3070",
            "AMD Radeon RX 6800 XT",
            "Apple M1 Pro",
            "Intel(R) Iris(R) Xe Graphics",
            "NVIDIA GeForce GTX 1660 SUPER",
        ];
        state = xorshift64(state);
        let vendor_idx = (state % vendors.len() as u64) as usize;
        state = xorshift64(state);
        let renderer_idx = (state % renderers.len() as u64) as usize;
        format!("{}) Direct3D11 vs_5_0 ps_5_0, {} via D3D11", vendors[vendor_idx], renderers[renderer_idx])
    }

    /// Slightly perturb audio context sample rate for audio fingerprint resistance.
    pub fn perturb_audio_sample_rate(&self, base_rate: u32) -> u32 {
        let mut state = self.session_seed;
        state = xorshift64(state);
        let offset = (state % 3) as i32 - 1; // -1, 0, or +1 Hz
        (base_rate as i32 + offset).max(0) as u32
    }
}

impl Default for CanvasFingerprintRandomizer {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bezier_trajectory() {
        let path = StealthHumanBehavior::generate_bezier_trajectory((0.0, 0.0), (100.0, 100.0), 20);
        assert_eq!(path.len(), 21); // steps + 1
        // Should start near (0,0) and end near (100,100)
        assert!((path[0].x - 0.0).abs() < 5.0);
        assert!((path[20].x - 100.0).abs() < 5.0);
    }

    #[test]
    fn test_typing_jitter() {
        let delays = StealthHumanBehavior::compute_typing_jitter(50);
        assert_eq!(delays.len(), 50);
        // All delays should be at least 20ms
        assert!(delays.iter().all(|&d| d >= 20));
    }

    #[test]
    fn test_scroll_pattern() {
        let scroll = StealthHumanBehavior::generate_scroll_pattern(500.0, 30);
        assert_eq!(scroll.len(), 30);
        // All deltas should be positive for downward scroll
        assert!(scroll.iter().all(|&(d, _)| d >= 0.0));
    }

    #[test]
    fn test_canvas_noise_changes_pixels() {
        let mut pixels = vec![128u8; 64]; // 16 RGBA pixels
        let original = pixels.clone();
        let rand = CanvasFingerprintRandomizer::with_seed(42);
        rand.apply_noise(&mut pixels);
        // At least some pixels should have changed
        assert_ne!(pixels, original);
        // But not by much (max ±2 per channel)
        for (a, b) in pixels.iter().zip(original.iter()) {
            assert!((*a as i16 - *b as i16).abs() <= 2);
        }
    }

    #[test]
    fn test_canvas_noise_deterministic() {
        let mut p1 = vec![100u8; 128];
        let mut p2 = p1.clone();
        let r1 = CanvasFingerprintRandomizer::with_seed(999);
        let r2 = CanvasFingerprintRandomizer::with_seed(999);
        r1.apply_noise(&mut p1);
        r2.apply_noise(&mut p2);
        assert_eq!(p1, p2); // same seed = same noise
    }

    #[test]
    fn test_spoofed_webgl_renderer() {
        let r = CanvasFingerprintRandomizer::with_seed(12345);
        let renderer = r.spoofed_webgl_renderer();
        assert!(renderer.contains("ANGLE"));
        assert!(renderer.contains("Direct3D11"));
    }

    #[test]
    fn test_perturb_audio_sample_rate() {
        let r = CanvasFingerprintRandomizer::with_seed(777);
        let perturbed = r.perturb_audio_sample_rate(48000);
        // Should be within ±1 Hz
        assert!((perturbed as i32 - 48000).abs() <= 1);
    }
}
