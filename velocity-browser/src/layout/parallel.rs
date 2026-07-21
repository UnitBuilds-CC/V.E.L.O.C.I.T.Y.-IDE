use crate::layout::LayoutBox;

pub struct ParallelLayoutEngine {
    pub worker_threads: usize,
}

impl ParallelLayoutEngine {
    pub fn new(worker_threads: usize) -> Self {
        Self { worker_threads }
    }

    pub fn compute_parallel_subtrees(&self, root_box: &mut LayoutBox) {
        root_box.is_visible = true;
        for child in &mut root_box.children {
            child.is_visible = true;
        }
    }
}
