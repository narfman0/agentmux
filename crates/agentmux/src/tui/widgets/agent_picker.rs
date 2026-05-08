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
}

impl<'a> AgentPicker<'a> {
    /// Render a centered popup over the terminal.
    pub fn render_popup(&self, area: Rect, buf: &mut Buffer) {
        let popup = centered_rect(40, (self.agents.len() as u16 + 4).min(20), area);
        Clear.render(popup, buf);

        let items: Vec<ListItem> = self
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

        let mut state = ListState::default();
        state.select(Some(self.selected));

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(
                        " New Pane — select agent (↑↓ Enter) ",
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
