//! Temporal frame-differencing monitor for transient captcha states.
//!
//! Many modern challenges hide their answer in motion: tiles flip on a timer,
//! one letter periodically changes, or an object animates. A single snapshot
//! cannot capture this. The [`TemporalMonitor`] keeps a short ring buffer of
//! reduced frames (per-cell luminance means) over a window (~15s) and reports
//! which cells changed and at what period — enough to answer "which tile flips"
//! or "which letter changes" by rule, without an LLM.

use crate::engine::PixelBuffer;
use std::collections::VecDeque;

/// One captured frame reduced to per-cell mean luminance for cheap diffing.
#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub timestamp_ms: u64,
    /// Mean luminance per grid cell, row-major (`rows * cols` entries).
    pub cell_means: Vec<u8>,
    pub rows: usize,
    pub cols: usize,
}

/// A grid cell whose content changed across the observation window.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedRegion {
    pub cell_index: usize,
    pub row: usize,
    pub col: usize,
    /// Accumulated absolute luminance delta across all frame transitions.
    pub magnitude: u32,
    /// Number of transitions whose delta exceeded the change threshold.
    pub transitions: u32,
}

/// Rolling monitor that differences successive frames to find transient change.
#[derive(Debug, Clone)]
pub struct TemporalMonitor {
    capacity: usize,
    frames: VecDeque<FrameSnapshot>,
    change_threshold: u8,
}

impl TemporalMonitor {
    /// Create a monitor holding up to `capacity` frames.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(2),
            frames: VecDeque::new(),
            change_threshold: 24,
        }
    }

    pub fn with_threshold(capacity: usize, change_threshold: u8) -> Self {
        Self {
            capacity: capacity.max(2),
            frames: VecDeque::new(),
            change_threshold,
        }
    }

    /// Reduce a buffer region into `rows * cols` cell means and store as a frame.
    pub fn capture(
        &mut self,
        buffer: &PixelBuffer,
        region: (usize, usize, usize, usize),
        rows: usize,
        cols: usize,
        timestamp_ms: u64,
    ) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let (rx, ry, rw, rh) = region;
        let rw = rw.min(buffer.width.saturating_sub(rx)).max(1);
        let rh = rh.min(buffer.height.saturating_sub(ry)).max(1);
        let cell_w = (rw / cols).max(1);
        let cell_h = (rh / rows).max(1);

        let mut cell_means = Vec::with_capacity(rows * cols);
        for cr in 0..rows {
            for cc in 0..cols {
                let cx = cc * cell_w;
                let cy = cr * cell_h;
                let mut sum = 0u64;
                let mut count = 0u64;
                for y in (0..cell_h).step_by(2) {
                    for x in (0..cell_w).step_by(2) {
                        let px = buffer.get_pixel(rx + cx + x, ry + cy + y);
                        let lum = (px[0] as u64 + px[1] as u64 + px[2] as u64) / 3;
                        sum += lum;
                        count += 1;
                    }
                }
                let mean = if count > 0 { (sum / count) as u8 } else { 0 };
                cell_means.push(mean);
            }
        }

        self.push(FrameSnapshot { timestamp_ms, cell_means, rows, cols });
    }

    /// Push a pre-reduced frame directly (low-level / testing).
    pub fn push(&mut self, snapshot: FrameSnapshot) {
        self.frames.push_back(snapshot);
        while self.frames.len() > self.capacity {
            self.frames.pop_front();
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// Accumulated per-cell change across the window, sorted by magnitude
    /// (descending). Only cells with at least one supra-threshold transition
    /// are returned.
    pub fn changed_regions(&self) -> Vec<ChangedRegion> {
        if self.frames.len() < 2 {
            return Vec::new();
        }
        let first = &self.frames[0];
        let cells = first.cell_means.len();
        let cols = first.cols.max(1);
        let mut magnitude = vec![0u32; cells];
        let mut transitions = vec![0u32; cells];

        for w in self.frames.iter().collect::<Vec<_>>().windows(2) {
            let (prev, cur) = (w[0], w[1]);
            if prev.cell_means.len() != cells || cur.cell_means.len() != cells {
                continue; // grid shape changed; skip mismatched transition
            }
            for i in 0..cells {
                let d = (cur.cell_means[i] as i32 - prev.cell_means[i] as i32).unsigned_abs();
                magnitude[i] += d;
                if d >= self.change_threshold as u32 {
                    transitions[i] += 1;
                }
            }
        }

        let mut regions: Vec<ChangedRegion> = (0..cells)
            .filter(|&i| transitions[i] > 0)
            .map(|i| ChangedRegion {
                cell_index: i,
                row: i / cols,
                col: i % cols,
                magnitude: magnitude[i],
                transitions: transitions[i],
            })
            .collect();
        regions.sort_by(|a, b| b.magnitude.cmp(&a.magnitude));
        regions
    }

    /// The single most-changed cell — e.g. the tile that flips or the letter
    /// that changes.
    pub fn most_changed_cell(&self) -> Option<ChangedRegion> {
        self.changed_regions().into_iter().next()
    }

    /// Estimate the dominant change period (ms) from the most-changed cell by
    /// averaging the interval between its supra-threshold transitions.
    pub fn detect_period_ms(&self) -> Option<u64> {
        let target = self.most_changed_cell()?;
        let idx = target.cell_index;
        let mut change_times: Vec<u64> = Vec::new();
        let frames: Vec<&FrameSnapshot> = self.frames.iter().collect();
        for w in frames.windows(2) {
            let (prev, cur) = (w[0], w[1]);
            if idx >= prev.cell_means.len() || idx >= cur.cell_means.len() {
                continue;
            }
            let d = (cur.cell_means[idx] as i32 - prev.cell_means[idx] as i32).unsigned_abs();
            if d >= self.change_threshold as u32 {
                change_times.push(cur.timestamp_ms);
            }
        }
        if change_times.len() < 2 {
            return None;
        }
        let mut total = 0u64;
        for w in change_times.windows(2) {
            total += w[1].saturating_sub(w[0]);
        }
        Some(total / (change_times.len() as u64 - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ts: u64, means: &[u8], rows: usize, cols: usize) -> FrameSnapshot {
        FrameSnapshot { timestamp_ms: ts, cell_means: means.to_vec(), rows, cols }
    }

    #[test]
    fn static_frames_report_no_change() {
        let mut mon = TemporalMonitor::new(8);
        for t in 0..4 {
            mon.push(frame(t * 100, &[50, 50, 50, 50], 2, 2));
        }
        assert!(mon.changed_regions().is_empty());
        assert!(mon.most_changed_cell().is_none());
    }

    #[test]
    fn detects_the_flipping_cell() {
        let mut mon = TemporalMonitor::new(8);
        // Cell index 2 alternates between dark and light; others static.
        mon.push(frame(0, &[50, 50, 10, 50], 2, 2));
        mon.push(frame(100, &[50, 50, 200, 50], 2, 2));
        mon.push(frame(200, &[50, 50, 10, 50], 2, 2));
        mon.push(frame(300, &[50, 50, 200, 50], 2, 2));
        let target = mon.most_changed_cell().expect("a changing cell");
        assert_eq!(target.cell_index, 2);
        assert_eq!(target.row, 1);
        assert_eq!(target.col, 0);
        assert!(target.transitions >= 3);
    }

    #[test]
    fn ring_buffer_respects_capacity() {
        let mut mon = TemporalMonitor::new(3);
        for t in 0..10 {
            mon.push(frame(t, &[0, 0], 1, 2));
        }
        assert_eq!(mon.frame_count(), 3);
    }

    #[test]
    fn detects_flip_period() {
        let mut mon = TemporalMonitor::new(16);
        // Flip every 150ms.
        for i in 0..6u64 {
            let v = if i % 2 == 0 { 10 } else { 200 };
            mon.push(frame(i * 150, &[50, v], 1, 2));
        }
        let period = mon.detect_period_ms().expect("a detectable period");
        assert!((period as i64 - 150).abs() <= 10, "period = {}", period);
    }

    #[test]
    fn capture_reduces_buffer_to_cells() {
        let mut buf = PixelBuffer::new(40, 40);
        // Darken the top-left quadrant.
        buf.fill_rect(0, 0, 20, 20, 10, 10, 10, 255);
        let mut mon = TemporalMonitor::new(4);
        mon.capture(&buf, (0, 0, 40, 40), 2, 2, 0);
        assert_eq!(mon.frame_count(), 1);
    }

    #[test]
    fn changed_regions_sorted_by_magnitude() {
        let mut mon = TemporalMonitor::new(8);
        // Cell 0 changes a lot, cell 1 a little (below threshold 24 → excluded).
        mon.push(frame(0, &[10, 50, 50, 50], 2, 2));
        mon.push(frame(100, &[200, 60, 50, 50], 2, 2));
        let regions = mon.changed_regions();
        assert_eq!(regions[0].cell_index, 0);
        // Cell 1's delta (10) is below the threshold, so it should not appear.
        assert!(regions.iter().all(|r| r.cell_index != 1));
    }
}
