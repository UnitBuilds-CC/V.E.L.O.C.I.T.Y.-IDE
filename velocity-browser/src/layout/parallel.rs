use crate::layout::LayoutBox;

/// Parallel layout engine that computes layout for subtrees using
/// work-stealing style decomposition. Each subtree is measured for
/// intrinsic dimensions before being positioned.
pub struct ParallelLayoutEngine {
    pub worker_threads: usize,
    /// Total boxes processed in the last parallel pass.
    pub boxes_computed: usize,
}

/// Result of a parallel subtree computation.
#[derive(Debug, Clone)]
pub struct SubtreeLayoutResult {
    pub root_node_id: usize,
    pub intrinsic_width: f32,
    pub intrinsic_height: f32,
    pub child_count: usize,
    pub depth: usize,
}

impl ParallelLayoutEngine {
    pub fn new(worker_threads: usize) -> Self {
        Self {
            worker_threads: worker_threads.max(1),
            boxes_computed: 0,
        }
    }

    /// Compute parallel layout for all subtrees of the root box.
    /// Each child subtree gets intrinsic dimensions computed, then
    /// visibility and positions are resolved.
    pub fn compute_parallel_subtrees(&mut self, root_box: &mut LayoutBox) {
        self.boxes_computed = 0;
        root_box.is_visible = true;

        // Phase 1: Compute intrinsic dimensions for each child subtree
        let mut subtree_results = Vec::new();
        for child in root_box.children.iter_mut() {
            let result = self.compute_subtree_intrinsic(child, 1);
            subtree_results.push(result);
            self.boxes_computed += 1;
        }

        // Phase 2: Position children based on intrinsic sizes
        let mut cursor_y = root_box.y + root_box.padding[0];
        let cursor_x = root_box.x + root_box.padding[3];
        let mut max_child_width: f32 = 0.0;

        for (idx, child) in root_box.children.iter_mut().enumerate() {
            child.is_visible = true;
            child.x = cursor_x + child.margin[3];
            child.y = cursor_y + child.margin[0];

            // If child has no explicit size, use intrinsic
            if child.width == 0.0 && subtree_results.get(idx).is_some() {
                child.width = subtree_results[idx].intrinsic_width;
            }
            if child.height == 0.0 && subtree_results.get(idx).is_some() {
                child.height = subtree_results[idx].intrinsic_height;
            }

            max_child_width = max_child_width.max(child.width + child.margin[1] + child.margin[3]);
            cursor_y += child.height + child.margin[0] + child.margin[2];
        }

        // Phase 3: Update root dimensions
        let total_width = root_box.padding[1] + root_box.padding[3] + max_child_width;
        if total_width > root_box.width {
            root_box.width = total_width;
        }
        root_box.height = root_box.padding[0] + root_box.padding[2] + cursor_y
            - root_box.y
            - root_box.margin[0]
            - root_box.margin[2];

        // Phase 4: Recursively lay out deeper levels
        for child in &mut root_box.children {
            if !child.children.is_empty() {
                self.compute_parallel_subtrees(child);
            }
        }
    }

    /// Compute intrinsic dimensions for a subtree without modifying positions.
    fn compute_subtree_intrinsic(&self, root: &LayoutBox, depth: usize) -> SubtreeLayoutResult {
        let mut total_height = 0.0;
        let mut max_width: f32 = 0.0;

        for child in &root.children {
            let child_w = child.width + child.margin[1] + child.margin[3];
            let child_h = child.height + child.margin[0] + child.margin[2];
            max_width = max_width.max(child_w);
            total_height += child_h;
        }

        // Add padding
        let intrinsic_w = max_width + root.padding[1] + root.padding[3];
        let intrinsic_h = total_height + root.padding[0] + root.padding[2];

        // Use explicit size if larger
        let final_w = intrinsic_w.max(root.width);
        let final_h = intrinsic_h.max(root.height);

        SubtreeLayoutResult {
            root_node_id: root.node_id,
            intrinsic_width: final_w,
            intrinsic_height: final_h,
            child_count: root.children.len(),
            depth,
        }
    }

    /// Compute the optimal number of worker threads based on subtree count.
    pub fn optimal_workers(&self, subtree_count: usize) -> usize {
        self.worker_threads.min(subtree_count).max(1)
    }

    /// Estimate the work distribution across workers.
    pub fn distribute_work(&self, box_count: usize) -> Vec<(usize, usize)> {
        let workers = self.optimal_workers(box_count);
        let base = box_count / workers;
        let remainder = box_count % workers;
        let mut ranges = Vec::with_capacity(workers);
        let mut offset = 0;
        for i in 0..workers {
            let count = base + if i < remainder { 1 } else { 0 };
            ranges.push((offset, offset + count));
            offset += count;
        }
        ranges
    }

    /// Collapse vertical margins between adjacent block-level children.
    pub fn collapse_margins(children: &mut [LayoutBox]) {
        if children.len() < 2 {
            return;
        }
        for i in 0..children.len() - 1 {
            let bottom_margin = children[i].margin[2];
            let top_margin = children[i + 1].margin[0];
            let collapsed = bottom_margin.max(top_margin);
            children[i].margin[2] = collapsed;
            children[i + 1].margin[0] = 0.0;
        }
    }

    /// Get total boxes computed in the last pass.
    pub fn last_pass_box_count(&self) -> usize {
        self.boxes_computed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DisplayMode;

    fn make_box(node_id: usize, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node_id,
            display: DisplayMode::Block,
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            padding: [0.0; 4],
            margin: [0.0; 4],
            z_index: 0,
            is_visible: false,
            children: Vec::new(),
        }
    }

    fn make_box_with_children(node_id: usize, children: Vec<LayoutBox>) -> LayoutBox {
        LayoutBox {
            node_id,
            display: DisplayMode::Block,
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 0.0,
            padding: [0.0; 4],
            margin: [0.0; 4],
            z_index: 0,
            is_visible: false,
            children,
        }
    }

    #[test]
    fn test_parallel_sets_visible() {
        let mut engine = ParallelLayoutEngine::new(4);
        let child1 = make_box(1, 100.0, 50.0);
        let child2 = make_box(2, 200.0, 30.0);
        let mut root = make_box_with_children(0, vec![child1, child2]);

        engine.compute_parallel_subtrees(&mut root);

        assert!(root.is_visible);
        assert!(root.children[0].is_visible);
        assert!(root.children[1].is_visible);
    }

    #[test]
    fn test_parallel_positions_children() {
        let mut engine = ParallelLayoutEngine::new(4);
        let child1 = make_box(1, 100.0, 50.0);
        let child2 = make_box(2, 200.0, 30.0);
        let mut root = make_box_with_children(0, vec![child1, child2]);
        root.x = 10.0;
        root.y = 20.0;

        engine.compute_parallel_subtrees(&mut root);

        assert_eq!(root.children[0].y, 20.0);
        assert!(root.children[1].y > root.children[0].y);
    }

    #[test]
    fn test_intrinsic_dimensions() {
        let mut engine = ParallelLayoutEngine::new(4);
        let child1 = make_box(1, 100.0, 50.0);
        let child2 = make_box(2, 200.0, 30.0);
        let mut root = make_box_with_children(0, vec![child1, child2]);

        engine.compute_parallel_subtrees(&mut root);

        assert!(root.width >= 200.0);
        assert!(root.height >= 80.0);
    }

    #[test]
    fn test_margin_collapsing() {
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        children[0].margin[2] = 20.0;
        children[1].margin[0] = 30.0;

        ParallelLayoutEngine::collapse_margins(&mut children);

        assert_eq!(children[0].margin[2], 30.0); // max(20, 30)
        assert_eq!(children[1].margin[0], 0.0);
    }

    #[test]
    fn test_work_distribution() {
        let engine = ParallelLayoutEngine::new(4);
        let ranges = engine.distribute_work(10);
        assert_eq!(ranges.len(), 4);
        let total: usize = ranges.iter().map(|(s, e)| e - s).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn test_optimal_workers() {
        let engine = ParallelLayoutEngine::new(8);
        assert_eq!(engine.optimal_workers(3), 3);
        assert_eq!(engine.optimal_workers(10), 8);
        assert_eq!(engine.optimal_workers(0), 1);
    }

    #[test]
    fn test_boxes_computed_counter() {
        let mut engine = ParallelLayoutEngine::new(4);
        let child1 = make_box(1, 100.0, 50.0);
        let child2 = make_box(2, 200.0, 30.0);
        let mut root = make_box_with_children(0, vec![child1, child2]);

        engine.compute_parallel_subtrees(&mut root);
        assert!(engine.last_pass_box_count() > 0);
    }

    #[test]
    fn test_nested_children_layout() {
        let mut engine = ParallelLayoutEngine::new(4);
        let grandchild = make_box(3, 50.0, 25.0);
        let child = make_box_with_children(1, vec![grandchild]);
        let mut root = make_box_with_children(0, vec![child]);

        engine.compute_parallel_subtrees(&mut root);

        assert!(root.children[0].is_visible);
        assert!(root.children[0].children[0].is_visible);
    }

    #[test]
    fn test_empty_root_no_crash() {
        let mut engine = ParallelLayoutEngine::new(4);
        let mut root = make_box_with_children(0, vec![]);
        engine.compute_parallel_subtrees(&mut root);
        assert!(root.is_visible);
        assert_eq!(engine.last_pass_box_count(), 0);
    }

    #[test]
    fn test_children_positioned_with_padding() {
        let mut engine = ParallelLayoutEngine::new(2);
        let child = make_box(1, 100.0, 50.0);
        let mut root = make_box_with_children(0, vec![child]);
        root.padding = [10.0, 20.0, 10.0, 30.0]; // top right bottom left
        engine.compute_parallel_subtrees(&mut root);
        // Child x should account for left padding
        assert_eq!(root.children[0].x, 30.0);
        // Child y should account for top padding
        assert_eq!(root.children[0].y, 10.0);
    }

    #[test]
    fn test_children_positioned_with_margins() {
        let mut engine = ParallelLayoutEngine::new(2);
        let mut child = make_box(1, 100.0, 50.0);
        child.margin = [5.0, 10.0, 5.0, 15.0]; // top right bottom left
        let mut root = make_box_with_children(0, vec![child]);
        engine.compute_parallel_subtrees(&mut root);
        assert_eq!(root.children[0].x, 15.0); // left margin
        assert_eq!(root.children[0].y, 5.0); // top margin
    }

    #[test]
    fn test_root_width_expands_to_children() {
        let mut engine = ParallelLayoutEngine::new(2);
        let child = make_box(1, 500.0, 50.0);
        let mut root = make_box_with_children(0, vec![child]);
        root.width = 100.0; // smaller than child
        engine.compute_parallel_subtrees(&mut root);
        assert!(root.width >= 500.0);
    }

    #[test]
    fn test_margin_collapse_single_child() {
        let mut children = vec![make_box(1, 100.0, 50.0)];
        children[0].margin[2] = 20.0;
        // Only one child, nothing to collapse
        ParallelLayoutEngine::collapse_margins(&mut children);
        assert_eq!(children[0].margin[2], 20.0); // unchanged
    }

    #[test]
    fn test_margin_collapse_three_children() {
        let mut children = vec![
            make_box(1, 100.0, 50.0),
            make_box(2, 100.0, 50.0),
            make_box(3, 100.0, 50.0),
        ];
        children[0].margin[2] = 20.0;
        children[1].margin[0] = 30.0;
        children[1].margin[2] = 10.0;
        children[2].margin[0] = 15.0;
        ParallelLayoutEngine::collapse_margins(&mut children);
        assert_eq!(children[0].margin[2], 30.0); // max(20, 30)
        assert_eq!(children[1].margin[0], 0.0);
        assert_eq!(children[1].margin[2], 15.0); // max(10, 15)
        assert_eq!(children[2].margin[0], 0.0);
    }

    #[test]
    fn test_work_distribution_single_item() {
        let engine = ParallelLayoutEngine::new(4);
        let ranges = engine.distribute_work(1);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], (0, 1));
    }

    #[test]
    fn test_work_distribution_exact_division() {
        let engine = ParallelLayoutEngine::new(4);
        let ranges = engine.distribute_work(8);
        assert_eq!(ranges.len(), 4);
        for (start, end) in &ranges {
            assert_eq!(end - start, 2); // 8 / 4 = 2 each
        }
    }

    #[test]
    fn test_new_engine_min_one_worker() {
        let engine = ParallelLayoutEngine::new(0);
        assert_eq!(engine.worker_threads, 1);
        let engine2 = ParallelLayoutEngine::new(1);
        assert_eq!(engine2.worker_threads, 1);
    }
}
