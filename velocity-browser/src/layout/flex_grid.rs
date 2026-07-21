use crate::layout::LayoutBox;

pub enum FlexDirection {
    Row,
    Column,
}

pub struct FlexLayoutEngine;

impl FlexLayoutEngine {
    pub fn compute_flex_children(parent_box: &LayoutBox, children: &mut [LayoutBox], direction: FlexDirection) {
        let mut offset_x = parent_box.x;
        let mut offset_y = parent_box.y;

        for child in children {
            match direction {
                FlexDirection::Row => {
                    child.x = offset_x;
                    child.y = parent_box.y;
                    offset_x += child.width;
                }
                FlexDirection::Column => {
                    child.x = parent_box.x;
                    child.y = offset_y;
                    offset_y += child.height;
                }
            }
        }
    }
}
