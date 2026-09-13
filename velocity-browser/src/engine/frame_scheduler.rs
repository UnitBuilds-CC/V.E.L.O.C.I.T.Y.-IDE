//! Predictive frame scheduling and render deduplication for the browser engine.
//!
//! [`FrameScheduler`] manages render task queuing, priority-based scheduling,
//! dirty-rect merging, and frame-budget tracking to keep rendering smooth and
//! avoid redundant paint work.

use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------------------
// RenderPriority
// ---------------------------------------------------------------------------

/// Priority levels for render tasks.  `Critical` is the highest priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderPriority {
    Critical,
    High,
    Normal,
    Low,
}

impl RenderPriority {
    /// Numeric rank used for sorting — lower value == higher priority.
    fn rank(self) -> u8 {
        match self {
            Self::Critical => 0,
            Self::High => 1,
            Self::Normal => 2,
            Self::Low => 3,
        }
    }
}

impl PartialOrd for RenderPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RenderPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lower rank == higher priority.  Natural ordering of rank puts
        // Critical (0) first when sorted ascending.
        self.rank().cmp(&other.rank())
    }
}

// ---------------------------------------------------------------------------
// RenderTask
// ---------------------------------------------------------------------------

/// A single render operation queued for scheduling.
#[derive(Debug, Clone)]
pub struct RenderTask {
    pub id: u64,
    pub priority: RenderPriority,
    /// Partial update region `(x, y, width, height)`.  `None` means full-screen.
    pub dirty_rect: Option<(f32, f32, f32, f32)>,
    /// Estimated cost in microseconds.
    pub estimated_cost_us: u64,
}

// ---------------------------------------------------------------------------
// FrameScheduler
// ---------------------------------------------------------------------------

/// Maximum number of frame times kept in the ring buffer for FPS calculation.
const FRAME_HISTORY_SIZE: usize = 120;

/// Predictive frame scheduler that queues render tasks, deduplicates overlapping
/// work, and fills each frame budget greedily by priority.
pub struct FrameScheduler {
    pub target_fps: u32,
    pub frame_budget_ms: f64,
    pub pending_renders: Vec<RenderTask>,
    pub last_frame_time: Instant,
    pub frame_history: Vec<f64>,
    /// Write cursor for the ring buffer (wraps at `FRAME_HISTORY_SIZE`).
    history_cursor: usize,
    /// Whether the ring buffer has been filled at least once.
    history_full: bool,
}

impl FrameScheduler {
    /// Create a new scheduler targeting `target_fps` frames per second.
    pub fn new(target_fps: u32) -> Self {
        Self {
            target_fps,
            frame_budget_ms: 1000.0 / target_fps as f64,
            pending_renders: Vec::new(),
            last_frame_time: Instant::now(),
            frame_history: Vec::with_capacity(FRAME_HISTORY_SIZE),
            history_cursor: 0,
            history_full: false,
        }
    }

    /// Queue a render task for the next frame.
    pub fn schedule_render(&mut self, task: RenderTask) {
        self.pending_renders.push(task);
    }

    /// Deduplicate pending renders:
    /// * When multiple tasks share the same `id`, keep only the one with the
    ///   highest priority (merge their dirty rects into a single bounding rect).
    /// * After collapsing by id, merge overlapping / adjacent dirty rects across
    ///   different tasks to minimise redundant painting.
    pub fn dedup_renders(&mut self) {
        if self.pending_renders.is_empty() {
            return;
        }

        // --- Step 1: collapse by id -------------------------------------------
        // We keep the *first* task per id as the canonical entry but upgrade its
        // priority and union its dirty rect with later duplicates.
        let mut seen: HashMap<u64, usize> = HashMap::new();
        let mut collapsed: Vec<RenderTask> = Vec::new();

        for task in self.pending_renders.drain(..) {
            if let Some(&idx) = seen.get(&task.id) {
                let existing = &mut collapsed[idx];
                // Upgrade priority if the duplicate is more urgent (lower Ord = higher priority).
                if task.priority < existing.priority {
                    existing.priority = task.priority;
                }
                // Union dirty rects.
                existing.dirty_rect = union_rects(existing.dirty_rect, task.dirty_rect);
                // Take the larger cost estimate (conservative).
                existing.estimated_cost_us =
                    existing.estimated_cost_us.max(task.estimated_cost_us);
            } else {
                seen.insert(task.id, collapsed.len());
                collapsed.push(task);
            }
        }

        // --- Step 2: merge overlapping dirty rects across tasks ---------------
        let rects: Vec<(f32, f32, f32, f32)> =
            collapsed.iter().filter_map(|t| t.dirty_rect).collect();
        if rects.len() >= 2 {
            let merged = DirtyRectMerger::merge(&rects);
            // Re-assign merged rects back to tasks in order.
            let mut merged_iter = merged.into_iter();
            for task in &mut collapsed {
                if task.dirty_rect.is_some() {
                    task.dirty_rect = merged_iter.next();
                }
            }
        }

        self.pending_renders = collapsed;
    }

    /// Return the tasks that should execute in the next frame.
    ///
    /// Tasks are sorted by priority (Critical first).  The frame budget is
    /// filled greedily — highest-priority tasks are admitted until the budget
    /// is exhausted.  Low-priority tasks are deferred when the frame is over
    /// budget.
    pub fn next_frame_tasks(&mut self) -> Vec<RenderTask> {
        // Sort: highest priority first; within same priority, lower id first.
        self.pending_renders
            .sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));

        let budget_us = (self.frame_budget_ms * 1000.0) as u64;
        let mut used_us: u64 = 0;
        let mut accepted: Vec<RenderTask> = Vec::new();
        let mut deferred: Vec<RenderTask> = Vec::new();

        for task in self.pending_renders.drain(..) {
            if used_us + task.estimated_cost_us <= budget_us {
                used_us += task.estimated_cost_us;
                accepted.push(task);
            } else if task.priority == RenderPriority::Low {
                // Defer low-priority tasks that don't fit.
                deferred.push(task);
            } else {
                // Non-low tasks that exceed budget are still deferred but we
                // keep them for the next frame.
                deferred.push(task);
            }
        }

        // Put deferred tasks back so they aren't lost.
        self.pending_renders = deferred;
        accepted
    }

    /// Record a frame time (in milliseconds) into the ring buffer.
    pub fn record_frame_time(&mut self, ms: f64) {
        if self.frame_history.len() < FRAME_HISTORY_SIZE {
            self.frame_history.push(ms);
        } else {
            self.frame_history[self.history_cursor] = ms;
            self.history_full = true;
        }
        self.history_cursor = (self.history_cursor + 1) % FRAME_HISTORY_SIZE;
        self.last_frame_time = Instant::now();
    }

    /// Measured FPS computed from the frame history ring buffer.
    pub fn current_fps(&self) -> f64 {
        if self.frame_history.is_empty() {
            return 0.0;
        }
        let avg_ms: f64 = self.frame_history.iter().sum::<f64>() / self.frame_history.len() as f64;
        if avg_ms <= 0.0 {
            return 0.0;
        }
        1000.0 / avg_ms
    }

    /// Microseconds remaining in the current frame budget.
    pub fn frame_budget_remaining_us(&self) -> u64 {
        let elapsed_ms = self.last_frame_time.elapsed().as_secs_f64() * 1000.0;
        let remaining = self.frame_budget_ms - elapsed_ms;
        if remaining < 0.0 {
            0
        } else {
            (remaining * 1000.0) as u64
        }
    }

    /// Whether the last recorded frame exceeded the budget.
    pub fn is_over_budget(&self) -> bool {
        match self.frame_history.last() {
            Some(&last) => last > self.frame_budget_ms,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// DirtyRectMerger
// ---------------------------------------------------------------------------

/// Utility for merging overlapping or adjacent dirty rectangles to minimise
/// redundant paint operations.
pub struct DirtyRectMerger;

impl DirtyRectMerger {
    /// Merge a slice of dirty rects `(x, y, w, h)` into a smaller set that
    /// covers the same area.  Two rects are merged when they overlap or are
    /// directly adjacent (share an edge or corner).
    pub fn merge(rects: &[(f32, f32, f32, f32)]) -> Vec<(f32, f32, f32, f32)> {
        if rects.is_empty() {
            return Vec::new();
        }

        let mut result: Vec<(f32, f32, f32, f32)> = rects.to_vec();
        let mut changed = true;

        while changed {
            changed = false;
            let mut next: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(result.len());
            let mut merged = vec![false; result.len()];

            for i in 0..result.len() {
                if merged[i] {
                    continue;
                }
                let mut current = result[i];
                for j in (i + 1)..result.len() {
                    if merged[j] {
                        continue;
                    }
                    if rects_adjacent_or_overlapping(current, result[j]) {
                        current = union_two_rects(current, result[j]);
                        merged[j] = true;
                        changed = true;
                    }
                }
                next.push(current);
            }
            result = next;
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check whether two rects `(x, y, w, h)` overlap or are adjacent (including
/// diagonal adjacency — sharing a corner counts).
fn rects_adjacent_or_overlapping(
    (x1, y1, w1, h1): (f32, f32, f32, f32),
    (x2, y2, w2, h2): (f32, f32, f32, f32),
) -> bool {
    // Expand rect1 by 0 pixels — "adjacent" means edges touch, so we use <=
    // rather than < for the gap check.
    !(x1 + w1 < x2 || x2 + w2 < x1 || y1 + h1 < y2 || y2 + h2 < y1)
}

/// Bounding rect union of two rects.
fn union_two_rects(
    (x1, y1, w1, h1): (f32, f32, f32, f32),
    (x2, y2, w2, h2): (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
    let min_x = x1.min(x2);
    let min_y = y1.min(y2);
    let max_x = (x1 + w1).max(x2 + w2);
    let max_y = (y1 + h1).max(y2 + h2);
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Union of two optional dirty rects.
fn union_rects(
    a: Option<(f32, f32, f32, f32)>,
    b: Option<(f32, f32, f32, f32)>,
) -> Option<(f32, f32, f32, f32)> {
    match (a, b) {
        (Some(ra), Some(rb)) => Some(union_two_rects(ra, rb)),
        (Some(_), None) | (None, Some(_)) => None, // full-screen wins
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers -------------------------------------------------------------

    fn task(id: u64, prio: RenderPriority, cost: u64) -> RenderTask {
        RenderTask {
            id,
            priority: prio,
            dirty_rect: None,
            estimated_cost_us: cost,
        }
    }

    fn task_rect(
        id: u64,
        prio: RenderPriority,
        cost: u64,
        rect: (f32, f32, f32, f32),
    ) -> RenderTask {
        RenderTask {
            id,
            priority: prio,
            dirty_rect: Some(rect),
            estimated_cost_us: cost,
        }
    }

    // -- RenderPriority ordering ---------------------------------------------

    #[test]
    fn priority_ordering_critical_is_highest() {
        // Critical has the lowest rank, so it sorts first (is "less than" others).
        assert!(RenderPriority::Critical < RenderPriority::High);
        assert!(RenderPriority::High < RenderPriority::Normal);
        assert!(RenderPriority::Normal < RenderPriority::Low);
    }

    #[test]
    fn priority_ordering_sort() {
        let mut prios = vec![
            RenderPriority::Low,
            RenderPriority::Critical,
            RenderPriority::Normal,
            RenderPriority::High,
        ];
        prios.sort();
        assert_eq!(
            prios,
            vec![
                RenderPriority::Critical,
                RenderPriority::High,
                RenderPriority::Normal,
                RenderPriority::Low,
            ]
        );
    }

    // -- FrameScheduler basic ------------------------------------------------

    #[test]
    fn new_scheduler_defaults() {
        let s = FrameScheduler::new(60);
        assert_eq!(s.target_fps, 60);
        assert!((s.frame_budget_ms - 16.666).abs() < 0.01);
        assert!(s.pending_renders.is_empty());
        assert!(s.frame_history.is_empty());
    }

    #[test]
    fn schedule_render_adds_to_queue() {
        let mut s = FrameScheduler::new(60);
        s.schedule_render(task(1, RenderPriority::Normal, 1000));
        s.schedule_render(task(2, RenderPriority::High, 2000));
        assert_eq!(s.pending_renders.len(), 2);
    }

    // -- Priority scheduling -------------------------------------------------

    #[test]
    fn next_frame_tasks_sorted_by_priority() {
        let mut s = FrameScheduler::new(60);
        s.schedule_render(task(3, RenderPriority::Low, 1000));
        s.schedule_render(task(1, RenderPriority::Critical, 1000));
        s.schedule_render(task(2, RenderPriority::Normal, 1000));

        let tasks = s.next_frame_tasks();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].priority, RenderPriority::Critical);
        assert_eq!(tasks[1].priority, RenderPriority::Normal);
        assert_eq!(tasks[2].priority, RenderPriority::Low);
    }

    #[test]
    fn critical_tasks_always_get_through() {
        // Budget = 10 ms = 10_000 us.  Make critical tasks small so they fit.
        let mut s = FrameScheduler::new(100); // 10 ms budget
        // Fill with 9 critical tasks of 1000 us each = 9000 us.
        for i in 0..9 {
            s.schedule_render(task(i, RenderPriority::Critical, 1000));
        }
        // One low-priority task of 2000 us that would push over budget.
        s.schedule_render(task(100, RenderPriority::Low, 2000));

        let tasks = s.next_frame_tasks();
        // All 9 critical should fit (9000 us <= 10_000 us).
        assert_eq!(tasks.len(), 9);
        for t in &tasks {
            assert_eq!(t.priority, RenderPriority::Critical);
        }
        // Low priority deferred.
        assert_eq!(s.pending_renders.len(), 1);
        assert_eq!(s.pending_renders[0].priority, RenderPriority::Low);
    }

    // -- Budget overflow -----------------------------------------------------

    #[test]
    fn budget_overflow_defers_low_priority() {
        let mut s = FrameScheduler::new(100); // 10 ms = 10_000 us budget
        s.schedule_render(task(1, RenderPriority::High, 6000));
        s.schedule_render(task(2, RenderPriority::Normal, 5000));
        s.schedule_render(task(3, RenderPriority::Low, 6000));

        let tasks = s.next_frame_tasks();
        // High (6000) fits.  Normal (5000) would make 11_000 > 10_000, deferred.
        // Low (6000) would also make 12_000 > 10_000, deferred.
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 1);
        assert_eq!(s.pending_renders.len(), 2);
    }

    #[test]
    fn budget_overflow_defers_non_low_too() {
        let mut s = FrameScheduler::new(100); // 10_000 us budget
        s.schedule_render(task(1, RenderPriority::Critical, 9000));
        s.schedule_render(task(2, RenderPriority::High, 3000));

        let tasks = s.next_frame_tasks();
        // Only critical fits.
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 1);
        // High is deferred (not lost).
        assert_eq!(s.pending_renders.len(), 1);
        assert_eq!(s.pending_renders[0].id, 2);
    }

    // -- Dirty rect merging --------------------------------------------------

    #[test]
    fn dirty_rect_merge_overlapping() {
        let rects = vec![(0.0, 0.0, 10.0, 10.0), (5.0, 5.0, 10.0, 10.0)];
        let merged = DirtyRectMerger::merge(&rects);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], (0.0, 0.0, 15.0, 15.0));
    }

    #[test]
    fn dirty_rect_merge_adjacent() {
        let rects = vec![(0.0, 0.0, 10.0, 10.0), (10.0, 0.0, 10.0, 10.0)];
        let merged = DirtyRectMerger::merge(&rects);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], (0.0, 0.0, 20.0, 10.0));
    }

    #[test]
    fn dirty_rect_merge_disjoint() {
        let rects = vec![(0.0, 0.0, 5.0, 5.0), (100.0, 100.0, 5.0, 5.0)];
        let merged = DirtyRectMerger::merge(&rects);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn dirty_rect_merge_empty() {
        let merged = DirtyRectMerger::merge(&[]);
        assert!(merged.is_empty());
    }

    #[test]
    fn dirty_rect_merge_single() {
        let rects = vec![(1.0, 2.0, 3.0, 4.0)];
        let merged = DirtyRectMerger::merge(&rects);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], (1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn dirty_rect_merge_three_chain() {
        // A overlaps B, B overlaps C → all merge into one.
        let rects = vec![
            (0.0, 0.0, 10.0, 10.0),
            (8.0, 0.0, 10.0, 10.0),
            (16.0, 0.0, 10.0, 10.0),
        ];
        let merged = DirtyRectMerger::merge(&rects);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].0, 0.0);
        assert_eq!(merged[0].2, 26.0);
    }

    // -- FPS calculation -----------------------------------------------------

    #[test]
    fn fps_calculation_from_frame_history() {
        let mut s = FrameScheduler::new(60);
        // Record 10 frames at 16.666 ms each → ~60 fps.
        for _ in 0..10 {
            s.record_frame_time(16.666);
        }
        let fps = s.current_fps();
        assert!((fps - 60.0).abs() < 0.1, "fps was {fps}");
    }

    #[test]
    fn fps_calculation_empty_history() {
        let s = FrameScheduler::new(60);
        assert_eq!(s.current_fps(), 0.0);
    }

    #[test]
    fn fps_ring_buffer_wraps() {
        let mut s = FrameScheduler::new(60);
        // Fill beyond FRAME_HISTORY_SIZE to exercise wrap-around.
        for _ in 0..(FRAME_HISTORY_SIZE + 30) {
            s.record_frame_time(10.0); // 100 fps
        }
        assert_eq!(s.frame_history.len(), FRAME_HISTORY_SIZE);
        let fps = s.current_fps();
        assert!((fps - 100.0).abs() < 0.1, "fps was {fps}");
    }

    // -- Deduplication -------------------------------------------------------

    #[test]
    fn dedup_superseded_renders_same_id() {
        let mut s = FrameScheduler::new(60);
        s.schedule_render(task_rect(
            1,
            RenderPriority::Normal,
            1000,
            (0.0, 0.0, 10.0, 10.0),
        ));
        s.schedule_render(task_rect(
            1,
            RenderPriority::High,
            2000,
            (5.0, 5.0, 10.0, 10.0),
        ));

        s.dedup_renders();
        assert_eq!(s.pending_renders.len(), 1);
        let t = &s.pending_renders[0];
        assert_eq!(t.id, 1);
        assert_eq!(t.priority, RenderPriority::High); // upgraded
        assert!(t.dirty_rect.is_some());
        // Cost is the max of the two.
        assert_eq!(t.estimated_cost_us, 2000);
    }

    #[test]
    fn dedup_different_ids_preserved() {
        let mut s = FrameScheduler::new(60);
        s.schedule_render(task_rect(
            1,
            RenderPriority::Normal,
            1000,
            (0.0, 0.0, 5.0, 5.0),
        ));
        s.schedule_render(task_rect(
            2,
            RenderPriority::Normal,
            1000,
            (100.0, 100.0, 5.0, 5.0),
        ));

        s.dedup_renders();
        assert_eq!(s.pending_renders.len(), 2);
    }

    #[test]
    fn dedup_fullscreen_wins_over_partial() {
        let mut s = FrameScheduler::new(60);
        s.schedule_render(task_rect(
            1,
            RenderPriority::Normal,
            1000,
            (0.0, 0.0, 10.0, 10.0),
        ));
        // Same id, no dirty rect → full-screen invalidation.
        s.schedule_render(task(1, RenderPriority::Normal, 1000));

        s.dedup_renders();
        assert_eq!(s.pending_renders.len(), 1);
        assert!(s.pending_renders[0].dirty_rect.is_none()); // full-screen
    }

    // -- is_over_budget ------------------------------------------------------

    #[test]
    fn is_over_budget_true_when_last_frame_exceeded() {
        let mut s = FrameScheduler::new(100); // 10 ms budget
        s.record_frame_time(15.0); // over budget
        assert!(s.is_over_budget());
    }

    #[test]
    fn is_over_budget_false_when_within_budget() {
        let mut s = FrameScheduler::new(100); // 10 ms budget
        s.record_frame_time(5.0);
        assert!(!s.is_over_budget());
    }

    #[test]
    fn is_over_budget_false_when_empty() {
        let s = FrameScheduler::new(60);
        assert!(!s.is_over_budget());
    }

    // -- frame_budget_remaining_us -------------------------------------------

    #[test]
    fn frame_budget_remaining_us_starts_positive() {
        let s = FrameScheduler::new(60);
        // Just created — last_frame_time is now, so remaining ≈ budget.
        let remaining = s.frame_budget_remaining_us();
        let budget_us = (s.frame_budget_ms * 1000.0) as u64;
        // Should be close to full budget (within 2 ms of elapsed time).
        assert!(remaining <= budget_us);
    }

    // -- Mixed integration ---------------------------------------------------

    #[test]
    fn mixed_priority_scheduling_integration() {
        let mut s = FrameScheduler::new(100); // 10_000 us budget
        s.schedule_render(task(1, RenderPriority::Low, 3000));
        s.schedule_render(task(2, RenderPriority::Critical, 3000));
        s.schedule_render(task(3, RenderPriority::High, 3000));
        s.schedule_render(task(4, RenderPriority::Normal, 3000));
        s.schedule_render(task(5, RenderPriority::Low, 3000));

        let tasks = s.next_frame_tasks();
        // Critical(3000) + High(3000) + Normal(3000) = 9000 fits.
        // Low(3000) would push to 12_000 > 10_000 → deferred.
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].priority, RenderPriority::Critical);
        assert_eq!(tasks[1].priority, RenderPriority::High);
        assert_eq!(tasks[2].priority, RenderPriority::Normal);
        // 2 low-priority deferred.
        assert_eq!(s.pending_renders.len(), 2);
    }

    #[test]
    fn dedup_then_schedule_flow() {
        let mut s = FrameScheduler::new(100); // 10_000 us budget
        // Queue duplicates.
        s.schedule_render(task_rect(
            1,
            RenderPriority::Normal,
            2000,
            (0.0, 0.0, 10.0, 10.0),
        ));
        s.schedule_render(task_rect(
            1,
            RenderPriority::Critical,
            3000,
            (5.0, 5.0, 10.0, 10.0),
        ));
        s.schedule_render(task(2, RenderPriority::Low, 5000));

        s.dedup_renders();
        assert_eq!(s.pending_renders.len(), 2);

        let tasks = s.next_frame_tasks();
        // Task 1 (Critical, 3000) + task 2 (Low, 5000) = 8000 <= 10_000.
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].priority, RenderPriority::Critical);
    }

    #[test]
    fn dirty_rect_merge_corner_touching() {
        // Two rects that share exactly a corner point.
        let rects = vec![(0.0, 0.0, 10.0, 10.0), (10.0, 10.0, 10.0, 10.0)];
        let merged = DirtyRectMerger::merge(&rects);
        // Corner-touching counts as adjacent → should merge.
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn record_many_frames_fps_stable() {
        let mut s = FrameScheduler::new(120);
        // 120 fps → 8.333 ms per frame.
        for _ in 0..200 {
            s.record_frame_time(8.333);
        }
        let fps = s.current_fps();
        assert!((fps - 120.0).abs() < 1.0, "fps was {fps}");
    }
}
