use std::collections::HashMap;

use agentmux_core::session::{Layout, PaneId};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDir {
    Horizontal, // left | right
    Vertical,   // top / bottom
}

#[derive(Debug, Clone, Copy)]
pub enum NavDir {
    Left,
    Right,
    Up,
    Down,
}

/// Recursively compute each leaf pane's screen Rect from the layout tree.
pub fn compute_rects(layout: &Layout, area: Rect, out: &mut HashMap<PaneId, Rect>) {
    match layout {
        Layout::Leaf(id) => {
            out.insert(*id, area);
        }
        Layout::HSplit { ratio, left, right } => {
            let left_w = ((area.width as f32 * ratio) as u16).max(1);
            let right_w = area.width.saturating_sub(left_w).max(1);
            let left_area = Rect::new(area.x, area.y, left_w, area.height);
            let right_area = Rect::new(area.x + left_w, area.y, right_w, area.height);
            compute_rects(left, left_area, out);
            compute_rects(right, right_area, out);
        }
        Layout::VSplit { ratio, top, bottom } => {
            let top_h = ((area.height as f32 * ratio) as u16).max(1);
            let bot_h = area.height.saturating_sub(top_h).max(1);
            let top_area = Rect::new(area.x, area.y, area.width, top_h);
            let bot_area = Rect::new(area.x, area.y + top_h, area.width, bot_h);
            compute_rects(top, top_area, out);
            compute_rects(bottom, bot_area, out);
        }
    }
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

/// Find the best pane to focus in a given direction using center-point scoring.
/// Penalises perpendicular offset so the "closest in direction" wins.
pub fn navigate(dir: NavDir, focused_id: PaneId, rects: &HashMap<PaneId, Rect>) -> Option<PaneId> {
    let fr = rects.get(&focused_id)?;
    let fc_x = fr.x as i32 + (fr.width / 2) as i32;
    let fc_y = fr.y as i32 + (fr.height / 2) as i32;

    let mut best: Option<(PaneId, i32)> = None;

    for (&id, rect) in rects {
        if id == focused_id {
            continue;
        }
        let cx = rect.x as i32 + (rect.width / 2) as i32;
        let cy = rect.y as i32 + (rect.height / 2) as i32;

        let score = match dir {
            NavDir::Left if cx < fc_x => (fc_x - cx) + (fc_y - cy).abs() * 3,
            NavDir::Right if cx > fc_x => (cx - fc_x) + (fc_y - cy).abs() * 3,
            NavDir::Up if cy < fc_y => (fc_y - cy) + (fc_x - cx).abs() * 3,
            NavDir::Down if cy > fc_y => (cy - fc_y) + (fc_x - cx).abs() * 3,
            _ => continue,
        };

        if best.map_or(true, |(_, s)| score < s) {
            best = Some((id, score));
        }
    }

    best.map(|(id, _)| id)
}

/// Collect all PaneIds in the layout, left-to-right / top-to-bottom.
pub fn collect_ids(layout: &Layout) -> Vec<PaneId> {
    match layout {
        Layout::Leaf(id) => vec![*id],
        Layout::HSplit { left, right, .. } => {
            let mut v = collect_ids(left);
            v.extend(collect_ids(right));
            v
        }
        Layout::VSplit { top, bottom, .. } => {
            let mut v = collect_ids(top);
            v.extend(collect_ids(bottom));
            v
        }
    }
}
