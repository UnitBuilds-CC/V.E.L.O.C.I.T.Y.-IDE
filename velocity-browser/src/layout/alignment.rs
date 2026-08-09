use crate::layout::LayoutBox;

#[derive(Debug, Clone, PartialEq)]
pub enum JustifyContent {
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlignItems {
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
}

pub struct FlexAlignmentSolver;

impl FlexAlignmentSolver {
    pub fn align_main_axis(parent_width: f32, children: &mut [LayoutBox], mode: JustifyContent) {
        if children.is_empty() {
            return;
        }

        let total_child_width: f32 = children.iter().map(|c| c.width).sum();
        let remaining_space = (parent_width - total_child_width).max(0.0);

        match mode {
            JustifyContent::FlexStart => {
                let mut curr_x = 0.0;
                for child in children {
                    child.x = curr_x;
                    curr_x += child.width;
                }
            }
            JustifyContent::FlexEnd => {
                let mut curr_x = remaining_space;
                for child in children {
                    child.x = curr_x;
                    curr_x += child.width;
                }
            }
            JustifyContent::Center => {
                let mut curr_x = remaining_space / 2.0;
                for child in children {
                    child.x = curr_x;
                    curr_x += child.width;
                }
            }
            JustifyContent::SpaceBetween => {
                if children.len() == 1 {
                    children[0].x = 0.0;
                    return;
                }
                let gap = remaining_space / (children.len() - 1) as f32;
                let mut curr_x = 0.0;
                for child in children {
                    child.x = curr_x;
                    curr_x += child.width + gap;
                }
            }
            JustifyContent::SpaceAround => {
                let gap = remaining_space / children.len() as f32;
                let mut curr_x = gap / 2.0;
                for child in children {
                    child.x = curr_x;
                    curr_x += child.width + gap;
                }
            }
        }
    }

    /// Align children along the cross axis (vertical) within a container of given height.
    pub fn align_cross_axis(parent_height: f32, children: &mut [LayoutBox], mode: AlignItems) {
        if children.is_empty() {
            return;
        }

        for child in children {
            let child_height = child.height;
            let remaining = (parent_height - child_height).max(0.0);

            match mode {
                AlignItems::FlexStart => {
                    child.y = 0.0;
                }
                AlignItems::FlexEnd => {
                    child.y = remaining;
                }
                AlignItems::Center => {
                    child.y = remaining / 2.0;
                }
                AlignItems::Stretch => {
                    child.y = 0.0;
                    child.height = parent_height;
                }
            }
        }
    }

    /// Wrap children into multiple lines when they exceed parent_width.
    /// Returns a vector of lines, where each line is a slice of children indices.
    pub fn compute_flex_wrap(
        children: &[LayoutBox],
        parent_width: f32,
        wrap: FlexWrap,
    ) -> Vec<Vec<usize>> {
        if children.is_empty() {
            return vec![];
        }
        if wrap == FlexWrap::NoWrap {
            return vec![(0..children.len()).collect()];
        }

        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut current_line: Vec<usize> = Vec::new();
        let mut current_width = 0.0f32;

        for (i, child) in children.iter().enumerate() {
            if current_width + child.width > parent_width && !current_line.is_empty() {
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0.0;
            }
            current_line.push(i);
            current_width += child.width;
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::engine::{DisplayMode, LayoutBox};

    fn make_box(width: f32, height: f32) -> LayoutBox {
        LayoutBox {
            node_id: 0,
            display: DisplayMode::Block,
            x: 0.0,
            y: 0.0,
            width,
            height,
            padding: [0.0; 4],
            margin: [0.0; 4],
            z_index: 0,
            is_visible: true,
            children: Vec::new(),
        }
    }

    #[test]
    fn flex_start_positions_children_at_origin() {
        let mut children = vec![make_box(100.0, 50.0), make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_main_axis(400.0, &mut children, JustifyContent::FlexStart);
        assert_eq!(children[0].x, 0.0);
        assert_eq!(children[1].x, 100.0);
    }

    #[test]
    fn flex_end_positions_children_at_end() {
        let mut children = vec![make_box(100.0, 50.0), make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_main_axis(400.0, &mut children, JustifyContent::FlexEnd);
        assert_eq!(children[0].x, 200.0);
        assert_eq!(children[1].x, 300.0);
    }

    #[test]
    fn center_positions_children_in_middle() {
        let mut children = vec![make_box(100.0, 50.0), make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_main_axis(400.0, &mut children, JustifyContent::Center);
        assert_eq!(children[0].x, 100.0);
        assert_eq!(children[1].x, 200.0);
    }

    #[test]
    fn space_between_distributes_evenly() {
        let mut children = vec![
            make_box(100.0, 50.0),
            make_box(100.0, 50.0),
            make_box(100.0, 50.0),
        ];
        FlexAlignmentSolver::align_main_axis(600.0, &mut children, JustifyContent::SpaceBetween);
        assert_eq!(children[0].x, 0.0);
        assert_eq!(children[1].x, 250.0);
        assert_eq!(children[2].x, 500.0);
    }

    #[test]
    fn space_around_distributes_with_half_gaps() {
        let mut children = vec![make_box(100.0, 50.0), make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_main_axis(400.0, &mut children, JustifyContent::SpaceAround);
        assert_eq!(children[0].x, 50.0);
        assert_eq!(children[1].x, 250.0);
    }

    #[test]
    fn cross_axis_flex_start() {
        let mut children = vec![make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_cross_axis(200.0, &mut children, AlignItems::FlexStart);
        assert_eq!(children[0].y, 0.0);
    }

    #[test]
    fn cross_axis_flex_end() {
        let mut children = vec![make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_cross_axis(200.0, &mut children, AlignItems::FlexEnd);
        assert_eq!(children[0].y, 150.0);
    }

    #[test]
    fn cross_axis_center() {
        let mut children = vec![make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_cross_axis(200.0, &mut children, AlignItems::Center);
        assert_eq!(children[0].y, 75.0);
    }

    #[test]
    fn cross_axis_stretch_expands_height() {
        let mut children = vec![make_box(100.0, 50.0)];
        FlexAlignmentSolver::align_cross_axis(200.0, &mut children, AlignItems::Stretch);
        assert_eq!(children[0].y, 0.0);
        assert_eq!(children[0].height, 200.0);
    }

    #[test]
    fn flex_wrap_no_wrap_single_line() {
        let children = vec![make_box(100.0, 50.0), make_box(100.0, 50.0)];
        let lines = FlexAlignmentSolver::compute_flex_wrap(&children, 150.0, FlexWrap::NoWrap);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], vec![0, 1]);
    }

    #[test]
    fn flex_wrap_wraps_to_multiple_lines() {
        let children = vec![
            make_box(100.0, 50.0),
            make_box(100.0, 50.0),
            make_box(100.0, 50.0),
        ];
        let lines = FlexAlignmentSolver::compute_flex_wrap(&children, 150.0, FlexWrap::Wrap);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], vec![0]);
        assert_eq!(lines[1], vec![1]);
        assert_eq!(lines[2], vec![2]);
    }

    #[test]
    fn flex_wrap_groups_children_correctly() {
        let children = vec![
            make_box(50.0, 50.0),
            make_box(50.0, 50.0),
            make_box(50.0, 50.0),
            make_box(50.0, 50.0),
        ];
        let lines = FlexAlignmentSolver::compute_flex_wrap(&children, 120.0, FlexWrap::Wrap);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], vec![0, 1]);
        assert_eq!(lines[1], vec![2, 3]);
    }

    #[test]
    fn empty_children_handled_gracefully() {
        let mut children: Vec<LayoutBox> = vec![];
        FlexAlignmentSolver::align_main_axis(400.0, &mut children, JustifyContent::Center);
        FlexAlignmentSolver::align_cross_axis(200.0, &mut children, AlignItems::Center);
        let lines = FlexAlignmentSolver::compute_flex_wrap(&children, 400.0, FlexWrap::Wrap);
        assert!(lines.is_empty());
    }
}
