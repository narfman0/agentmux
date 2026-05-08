use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Widget},
};

const LABELS: [&str; 3] = ["Command", "Args   ", "Cwd    "];
const POPUP_W: u16 = 64;
const POPUP_H: u16 = 13;

pub struct StartParamsModal<'a> {
    pub cmd: &'a str,
    pub args: &'a str,
    pub cwd: &'a str,
    pub focused: usize,
}

impl<'a> StartParamsModal<'a> {
    pub fn render_popup(&self, area: Rect, buf: &mut Buffer) {
        let popup = centered_rect(POPUP_W, POPUP_H, area);
        Clear.render(popup, buf);

        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(Span::styled(
                " New Pane — custom params ",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ))
            .render(popup, buf);

        let inner = Rect {
            x: popup.x + 1,
            y: popup.y + 1,
            width: popup.width.saturating_sub(2),
            height: popup.height.saturating_sub(2),
        };

        let values = [self.cmd, self.args, self.cwd];
        for (i, (label, value)) in LABELS.iter().zip(values.iter()).enumerate() {
            let row_y = inner.y + (i as u16) * 3;
            if row_y >= inner.y + inner.height {
                break;
            }
            let focused = i == self.focused;
            let label_style = if focused {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let value_style = if focused {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let cursor = if focused { "_" } else { "" };
            Line::from(vec![
                Span::styled(format!("  {} : ", label), label_style),
                Span::styled(format!("{}{}", value, cursor), value_style),
            ])
            .render(Rect { x: inner.x, y: row_y, width: inner.width, height: 1 }, buf);
        }

        // Hint row at the bottom of the inner area
        let hint_y = inner.y + inner.height.saturating_sub(1);
        Line::from(vec![
            Span::styled("  Tab/↑↓: next field  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter: spawn  ", Style::default().fg(Color::Green)),
            Span::styled("Esc: cancel", Style::default().fg(Color::DarkGray)),
        ])
        .render(Rect { x: inner.x, y: hint_y, width: inner.width, height: 1 }, buf);
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
