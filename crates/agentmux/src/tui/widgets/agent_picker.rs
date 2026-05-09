use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Widget},
};

pub struct AgentPicker<'a> {
    pub agents: &'a [String],
    pub selected: usize,
    pub show_custom: bool,
}

impl<'a> AgentPicker<'a> {
    /// Render a centered popup over the terminal.
    pub fn render_popup(&self, area: Rect, buf: &mut Buffer) {
        let total_rows = self.agents.len() + if self.show_custom { 1 } else { 0 };
        let popup = centered_rect(44, (total_rows as u16 + 4).min(22), area);
        Clear.render(popup, buf);

        let custom_idx = self.agents.len();
        let mut items: Vec<ListItem> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let style = if i == self.selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(format!("  {}  ", name), style)))
            })
            .collect();

        if self.show_custom {
            let style = if self.selected == custom_idx {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            items.push(ListItem::new(Line::from(Span::styled("  Custom command...  ", style))));
        }

        let mut state = ListState::default();
        state.select(Some(self.selected));

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(
                        " New Pane — pick agent or custom (↑↓ Enter) ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));

        ratatui::widgets::StatefulWidget::render(list, popup, buf, &mut state);
    }
}

/// Returns a centered Rect of fixed width/height within the given area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
