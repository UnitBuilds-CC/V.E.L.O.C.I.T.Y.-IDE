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

pub struct FlexAlignmentSolver;

impl FlexAlignmentSolver {
    pub fn align_main_axis(parent_width: f32, children: &mut [LayoutBox], mode: JustifyContent) {
        if children.is_empty() { return; }

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
}
