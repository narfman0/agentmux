use agentmux_core::protocol::ScreenSnapshot;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Widget},
};

use super::pane_view::PaneView;

pub struct DashboardItem<'a> {
    pub snapshot: &'a ScreenSnapshot,
    pub title: &'a str,
    pub selected: bool,
}

pub struct Dashboard<'a> {
    pub items: Vec<DashboardItem<'a>>,
    pub cols: usize, // number of grid columns
}

impl<'a> Dashboard<'a> {
    /// Compute a sensible column count for N items.
    pub fn grid_cols(n: usize) -> usize {
        match n {
            0 | 1 => 1,
            2 => 2,
            3..=4 => 2,
            5..=6 => 3,
            _ => 3,
        }
    }

    /// Split the area into a grid of equal rects (row-major).
    pub fn compute_cells(area: Rect, rows: usize, cols: usize) -> Vec<Rect> {
        let cell_w = (area.width / cols as u16).max(1);
        let cell_h = (area.height / rows as u16).max(1);
        let mut cells = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                cells.push(Rect {
                    x: area.x + c as u16 * cell_w,
                    y: area.y + r as u16 * cell_h,
                    width: cell_w,
                    height: cell_h,
                });
            }
        }
        cells
    }
}

impl<'a> Widget for Dashboard<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len();
        let cols = self.cols.max(1);
        let rows = n.div_ceil(cols);
        let cells = Dashboard::compute_cells(area, rows, cols);

        for (i, item) in self.items.iter().enumerate() {
            let Some(&cell) = cells.get(i) else { break };

            let border_style = if item.selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let title_style = if item.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let hint = if item.selected { " [Enter]" } else { "" };
            let full_title = format!(" {}{} ", item.title, hint);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(full_title, title_style));

            let inner = block.inner(cell);
            block.render(cell, buf);

            PaneView { snapshot: item.snapshot, focused: false, title: "" }.render(inner, buf);
        }
    }
}
