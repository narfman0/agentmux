use agentmux_core::session::{Layout, PaneId};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDir {
    Horizontal, // left | right
    Vertical,   // top / bottom
}

/// Split the leaf containing `target`, inserting `new_id` as a sibling.
pub fn split_layout(layout: Layout, target: PaneId, new_id: PaneId, dir: SplitDir) -> Layout {
    match layout {
        Layout::Leaf(id) if id == target => match dir {
            SplitDir::Horizontal => Layout::HSplit {
                ratio: 0.5,
                left: Box::new(Layout::Leaf(id)),
                right: Box::new(Layout::Leaf(new_id)),
            },
            SplitDir::Vertical => Layout::VSplit {
                ratio: 0.5,
                top: Box::new(Layout::Leaf(id)),
                bottom: Box::new(Layout::Leaf(new_id)),
            },
        },
        Layout::Leaf(id) => Layout::Leaf(id),
        Layout::HSplit { ratio, left, right } => Layout::HSplit {
            ratio,
            left: Box::new(split_layout(*left, target, new_id, dir)),
            right: Box::new(split_layout(*right, target, new_id, dir)),
        },
        Layout::VSplit { ratio, top, bottom } => Layout::VSplit {
            ratio,
            top: Box::new(split_layout(*top, target, new_id, dir)),
            bottom: Box::new(split_layout(*bottom, target, new_id, dir)),
        },
    }
}

/// Remove a pane from the layout. Returns None if the whole tree was removed.
/// When one side of a split is removed, the other side takes the full space.
pub fn remove_from_layout(layout: Layout, target: PaneId) -> Option<Layout> {
    match layout {
        Layout::Leaf(id) if id == target => None,
        Layout::Leaf(id) => Some(Layout::Leaf(id)),
        Layout::HSplit { ratio, left, right } => {
            match (remove_from_layout(*left, target), remove_from_layout(*right, target)) {
                (None, None) => None,
                (Some(l), None) => Some(l),
                (None, Some(r)) => Some(r),
                (Some(l), Some(r)) => Some(Layout::HSplit { ratio, left: Box::new(l), right: Box::new(r) }),
            }
        }
        Layout::VSplit { ratio, top, bottom } => {
            match (remove_from_layout(*top, target), remove_from_layout(*bottom, target)) {
                (None, None) => None,
                (Some(t), None) => Some(t),
                (None, Some(b)) => Some(b),
                (Some(t), Some(b)) => Some(Layout::VSplit { ratio, top: Box::new(t), bottom: Box::new(b) }),
            }
        }
    }
}
