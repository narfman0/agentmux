use agentmux_core::protocol::{ScreenSnapshot, SerColor};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Widget},
};

pub struct PaneView<'a> {
    pub snapshot: &'a ScreenSnapshot,
    pub focused: bool,
    pub title: &'a str,
}

impl<'a> Widget for PaneView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                format!(" {} ", self.title),
                if self.focused {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        let snap = self.snapshot;
        let view_cols = inner.width as usize;
        let view_rows = inner.height as usize;
        let snap_cols = snap.cols as usize;

        for row in 0..view_rows {
            for col in 0..view_cols {
                let snap_row = row;
                let snap_col = col;

                if snap_row >= snap.rows as usize || snap_col >= snap_cols {
                    continue;
                }

                let idx = snap_row * snap_cols + snap_col;
                if idx >= snap.cells.len() {
                    continue;
                }

                let cell = &snap.cells[idx];
                let buf_cell = buf.cell_mut((inner.x + col as u16, inner.y + row as u16));
                if let Some(buf_cell) = buf_cell {
                    let ch = if cell.ch == '\0' { ' ' } else { cell.ch };
                    buf_cell.set_char(ch);

                    let mut style = Style::default()
                        .fg(ser_color_to_ratatui(&cell.fg))
                        .bg(ser_color_to_ratatui(&cell.bg));

                    if cell.bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if cell.italic {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    if cell.underline {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    if cell.reverse {
                        style = style.add_modifier(Modifier::REVERSED);
                    }

                    buf_cell.set_style(style);
                }
            }
        }
    }
}

fn ser_color_to_ratatui(c: &SerColor) -> Color {
    match c {
        SerColor::Default => Color::Reset,
        SerColor::Ansi(i) => ansi_to_ratatui(*i),
        SerColor::Palette(i) => Color::Indexed(*i),
        SerColor::Rgb(r, g, b) => Color::Rgb(*r, *g, *b),
    }
}

fn ansi_to_ratatui(i: u8) -> Color {
    match i {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        _ => Color::Indexed(i),
    }
}
