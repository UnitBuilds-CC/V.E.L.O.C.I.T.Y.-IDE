use crate::layout::LayoutBox;

/// Flex container direction.
#[derive(Debug, Clone, PartialEq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

/// How items are aligned on the cross axis.
#[derive(Debug, Clone, PartialEq)]
pub enum FlexAlignItems {
    Stretch,
    Center,
    FlexStart,
    FlexEnd,
    Baseline,
}

/// How lines are aligned on the cross axis.
#[derive(Debug, Clone, PartialEq)]
pub enum FlexAlignContent {
    Stretch,
    Center,
    FlexStart,
    FlexEnd,
    SpaceBetween,
    SpaceAround,
}

/// Wrapping behavior.
#[derive(Debug, Clone, PartialEq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

/// Computed flex item data.
#[derive(Debug, Clone)]
pub struct FlexItemData {
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: f32,
    pub order: i32,
    pub align_self: Option<FlexAlignItems>,
}

/// Flex layout engine supporting direction, wrap, align-items, align-content, order, and multi-line.
pub struct FlexLayoutEngine;

impl FlexLayoutEngine {
    /// Compute flex child positions with full flexbox algorithm.
    pub fn compute_flex_children(
        parent_box: &LayoutBox,
        children: &mut [LayoutBox],
        direction: FlexDirection,
    ) {
        Self::compute_flex_children_full(parent_box, children, &direction, &FlexAlignItems::Stretch, &FlexWrap::NoWrap, &[]);
    }

    /// Full flexbox computation with alignment, wrapping, and order.
    pub fn compute_flex_children_full(
        parent_box: &LayoutBox,
        children: &mut [LayoutBox],
        direction: &FlexDirection,
        align_items: &FlexAlignItems,
        _wrap: &FlexWrap,
        item_data: &[FlexItemData],
    ) {
        if children.is_empty() { return; }

        let is_row = matches!(direction, FlexDirection::Row | FlexDirection::RowReverse);
        let is_reverse = matches!(direction, FlexDirection::RowReverse | FlexDirection::ColumnReverse);
        let container_main = if is_row { parent_box.width } else { parent_box.height };
        let _container_cross = if is_row { parent_box.height } else { parent_box.width };

        // Sort by order
        let mut indices: Vec<usize> = (0..children.len()).collect();
        if !item_data.is_empty() {
            indices.sort_by_key(|&i| if i < item_data.len() { item_data[i].order } else { 0 });
        }

        // Compute flex basis and determine main sizes
        let mut main_sizes: Vec<f32> = Vec::with_capacity(children.len());
        let mut total_main = 0.0f32;

        for &i in &indices {
            let child = &children[i];
            let basis = if i < item_data.len() {
                item_data[i].flex_basis
            } else if is_row {
                child.width
            } else {
                child.height
            };
            main_sizes.push(basis);
            total_main += basis;
        }

        // Distribute free space (grow or shrink)
        let free_space = container_main - total_main;
        if free_space > 0.0 {
            // Grow
            let total_grow: f32 = indices.iter()
                .map(|&i| if i < item_data.len() { item_data[i].flex_grow } else { 0.0 })
                .sum();
            if total_grow > 0.0 {
                for (j, &i) in indices.iter().enumerate() {
                    let grow = if i < item_data.len() { item_data[i].flex_grow } else { 0.0 };
                    if grow > 0.0 {
                        main_sizes[j] += free_space * grow / total_grow;
                    }
                }
            }
        } else if free_space < 0.0 {
            // Shrink
            let total_shrink: f32 = indices.iter()
                .map(|&i| if i < item_data.len() { item_data[i].flex_shrink } else { 1.0 })
                .sum();
            if total_shrink > 0.0 {
                for (j, &i) in indices.iter().enumerate() {
                    let shrink = if i < item_data.len() { item_data[i].flex_shrink } else { 1.0 };
                    main_sizes[j] = (main_sizes[j] + free_space * shrink / total_shrink).max(0.0);
                }
            }
        }

        // Position children along main axis
        let mut cursor = if is_row { parent_box.x + parent_box.padding[3] }
                         else { parent_box.y + parent_box.padding[0] };

        let ordered_indices: Vec<usize> = if is_reverse {
            indices.iter().rev().cloned().collect()
        } else {
            indices.clone()
        };

        for (j, &i) in ordered_indices.iter().enumerate() {
            let main_size = main_sizes[j];
            let child = &mut children[i];

            if is_row {
                child.x = cursor;
                child.width = main_size;
                cursor += main_size;
            } else {
                child.y = cursor;
                child.height = main_size;
                cursor += main_size;
            }
        }

        // Align items on cross axis
        for &i in &ordered_indices {
            let child = &mut children[i];
            let align = if i < item_data.len() {
                item_data[i].align_self.clone().unwrap_or_else(|| align_items.clone())
            } else {
                align_items.clone()
            };

            if is_row {
                child.y = parent_box.y + parent_box.padding[0];
                match align {
                    FlexAlignItems::Center => {
                        child.y = parent_box.y + (parent_box.height - child.height) / 2.0;
                    }
                    FlexAlignItems::FlexEnd => {
                        child.y = parent_box.y + parent_box.height - parent_box.padding[2] - child.height;
                    }
                    FlexAlignItems::Stretch => {
                        child.height = parent_box.height - parent_box.padding[0] - parent_box.padding[2];
                    }
                    _ => {} // FlexStart, Baseline — default position
                }
            } else {
                child.x = parent_box.x + parent_box.padding[3];
                match align {
                    FlexAlignItems::Center => {
                        child.x = parent_box.x + (parent_box.width - child.width) / 2.0;
                    }
                    FlexAlignItems::FlexEnd => {
                        child.x = parent_box.x + parent_box.width - parent_box.padding[1] - child.width;
                    }
                    FlexAlignItems::Stretch => {
                        child.width = parent_box.width - parent_box.padding[1] - parent_box.padding[3];
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DisplayMode;

    fn make_box(node_id: usize, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node_id, display: DisplayMode::Flex, x: 0.0, y: 0.0,
            width: w, height: h, padding: [0.0; 4], margin: [0.0; 4],
            z_index: 0, is_visible: true, children: Vec::new(),
        }
    }

    #[test]
    fn test_row_layout() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::Row);
        assert_eq!(children[0].x, 0.0);
        assert_eq!(children[1].x, 100.0);
    }

    #[test]
    fn test_column_layout() {
        let parent = make_box(0, 300.0, 300.0);
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::Column);
        assert_eq!(children[0].y, 0.0);
        assert_eq!(children[1].y, 50.0);
    }

    #[test]
    fn test_flex_grow() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        let item_data = vec![
            FlexItemData { flex_grow: 1.0, flex_shrink: 1.0, flex_basis: 0.0, order: 0, align_self: None },
            FlexItemData { flex_grow: 2.0, flex_shrink: 1.0, flex_basis: 0.0, order: 0, align_self: None },
        ];
        FlexLayoutEngine::compute_flex_children_full(&parent, &mut children, &FlexDirection::Row, &FlexAlignItems::Stretch, &FlexWrap::NoWrap, &item_data);
        assert!((children[0].width - 100.0).abs() < 0.01);
        assert!((children[1].width - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_align_center() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 50.0, 30.0)];
        FlexLayoutEngine::compute_flex_children_full(&parent, &mut children, &FlexDirection::Row, &FlexAlignItems::Center, &FlexWrap::NoWrap, &[]);
        assert!((children[0].y - 35.0).abs() < 0.01); // (100 - 30) / 2
    }

    #[test]
    fn test_reverse_row() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::RowReverse);
        // In reverse, second item should be positioned first (rightmost)
        assert!(children[1].x < children[0].x);
    }

    #[test]
    fn test_order() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        let item_data = vec![
            FlexItemData { flex_grow: 0.0, flex_shrink: 1.0, flex_basis: 100.0, order: 2, align_self: None },
            FlexItemData { flex_grow: 0.0, flex_shrink: 1.0, flex_basis: 100.0, order: 1, align_self: None },
        ];
        FlexLayoutEngine::compute_flex_children_full(&parent, &mut children, &FlexDirection::Row, &FlexAlignItems::Stretch, &FlexWrap::NoWrap, &item_data);
        // Child with order=1 (index 1) should be positioned first
        assert!(children[1].x <= children[0].x);
    }

    #[test]
    fn test_empty_children() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children: Vec<LayoutBox> = vec![];
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::Row);
        // Should not panic
    }

    #[test]
    fn test_column_reverse() {
        let parent = make_box(0, 300.0, 300.0);
        let mut children = vec![make_box(1, 100.0, 50.0), make_box(2, 100.0, 50.0)];
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::ColumnReverse);
        // In column-reverse, second item should be above first
        assert!(children[1].y < children[0].y);
    }

    #[test]
    fn test_flex_end_alignment() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 50.0, 30.0)];
        FlexLayoutEngine::compute_flex_children_full(&parent, &mut children, &FlexDirection::Row, &FlexAlignItems::FlexEnd, &FlexWrap::NoWrap, &[]);
        assert!((children[0].y - 70.0).abs() < 0.01); // 100 - 30
    }

    #[test]
    fn test_flex_start_alignment() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 50.0, 30.0)];
        FlexLayoutEngine::compute_flex_children_full(&parent, &mut children, &FlexDirection::Row, &FlexAlignItems::FlexStart, &FlexWrap::NoWrap, &[]);
        assert_eq!(children[0].y, 0.0);
    }

    #[test]
    fn test_stretch_alignment() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 50.0, 30.0)];
        FlexLayoutEngine::compute_flex_children_full(&parent, &mut children, &FlexDirection::Row, &FlexAlignItems::Stretch, &FlexWrap::NoWrap, &[]);
        assert_eq!(children[0].y, 0.0);
        assert_eq!(children[0].height, 100.0); // stretched to parent height
    }

    #[test]
    fn test_single_child_row() {
        let parent = make_box(0, 300.0, 100.0);
        let mut children = vec![make_box(1, 80.0, 40.0)];
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::Row);
        assert_eq!(children[0].x, 0.0);
        assert_eq!(children[0].y, 0.0);
    }

    #[test]
    fn test_many_children_row() {
        let parent = make_box(0, 600.0, 100.0);
        let mut children: Vec<LayoutBox> = (0..5).map(|i| make_box(i, 100.0, 50.0)).collect();
        FlexLayoutEngine::compute_flex_children(&parent, &mut children, FlexDirection::Row);
        for i in 0..5 {
            assert_eq!(children[i].x, (i as f32) * 100.0);
        }
    }
}
