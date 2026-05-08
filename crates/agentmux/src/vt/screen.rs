use agentmux_core::protocol::{ScreenSnapshot, SnapshotCell, SerColor};

pub struct Screen {
    parser: vt100::Parser,
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 0),
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(rows, cols);
    }

    pub fn snapshot(&self) -> ScreenSnapshot {
        let screen = self.parser.screen();
        let (rows, cols) = (screen.size().0, screen.size().1);
        let (cursor_row, cursor_col) = screen.cursor_position();

        let mut cells = Vec::with_capacity((rows as usize) * (cols as usize));
        for row in 0..rows {
            for col in 0..cols {
                let cell = screen.cell(row, col);
                let (ch, fg, bg, attrs) = match cell {
                    Some(c) => {
                        let ch = c.contents().chars().next().unwrap_or(' ');
                        let fg = vt100_color(c.fgcolor());
                        let bg = vt100_color(c.bgcolor());
                        (ch, fg, bg, (c.bold(), c.italic(), c.underline(), c.inverse()))
                    }
                    None => (' ', SerColor::Default, SerColor::Default, (false, false, false, false)),
                };
                cells.push(SnapshotCell {
                    ch,
                    fg,
                    bg,
                    bold: attrs.0,
                    italic: attrs.1,
                    underline: attrs.2,
                    reverse: attrs.3,
                });
            }
        }

        ScreenSnapshot {
            cols,
            rows,
            cursor_col,
            cursor_row,
            cursor_hidden: screen.hide_cursor(),
            cells,
        }
    }
}

fn vt100_color(c: vt100::Color) -> SerColor {
    match c {
        vt100::Color::Default => SerColor::Default,
        vt100::Color::Idx(i) => {
            if i < 16 {
                SerColor::Ansi(i)
            } else {
                SerColor::Palette(i)
            }
        }
        vt100::Color::Rgb(r, g, b) => SerColor::Rgb(r, g, b),
    }
}
