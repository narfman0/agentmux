use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

pub struct StatusBar<'a> {
    pub session_name: &'a str,
    pub pane_names: &'a [String],
    pub focused_idx: usize,
    pub broadcast: bool,
}

impl<'a> Widget for StatusBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, area.y)) {
                cell.set_char(' ');
                cell.set_style(Style::default().bg(Color::DarkGray).fg(Color::White));
            }
        }

        let mut spans = vec![
            Span::styled(
                format!(" {} ", self.session_name),
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(Color::DarkGray)),
        ];

        for (i, name) in self.pane_names.iter().enumerate() {
            let label = format!(" {} {} ", i + 1, name);
            if i == self.focused_idx {
                spans.push(Span::styled(
                    label,
                    Style::default()
                        .bg(Color::Cyan)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    label,
                    Style::default().bg(Color::DarkGray).fg(Color::White),
                ));
            }
            spans.push(Span::styled(" ", Style::default().bg(Color::DarkGray)));
        }

        if self.broadcast {
            spans.push(Span::styled(
                " [BROADCAST] ",
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let prefix_hint = Span::styled(
            " Ctrl+A: prefix ",
            Style::default().bg(Color::DarkGray).fg(Color::Gray),
        );

        let line = Line::from(spans);
        line.render(area, buf);

        // Right-align the hint
        let hint_len = " Ctrl+A: prefix ".len() as u16;
        if area.width > hint_len {
            let hint_x = area.x + area.width - hint_len;
            let hint_area = Rect::new(hint_x, area.y, hint_len, 1);
            Line::from(vec![prefix_hint]).render(hint_area, buf);
        }
    }
}
