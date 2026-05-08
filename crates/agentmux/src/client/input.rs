use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    /// Grid overview — arrow keys navigate, Enter enters detail view.
    Dashboard,
    /// Full-screen agent interaction — Escape returns to dashboard.
    Detail,
    /// Agent picker overlay (Ctrl+A n from dashboard).
    AgentPicker { selected: usize, count: usize },
}

#[derive(Debug, Clone)]
pub enum Action {
    // Dashboard navigation
    DashboardUp,
    DashboardDown,
    DashboardLeft,
    DashboardRight,
    DashboardSelect,   // Enter → Detail mode
    AddPane,           // n → open agent picker
    RemovePane,        // x → close selected pane
    ToggleBroadcast,   // b

    // Agent picker
    PickerUp,
    PickerDown,
    PickerConfirm(usize),
    PickerCancel,

    // Detail mode
    BackToDashboard,   // Escape → Dashboard mode
    PaneInput(Vec<u8>), // pass-through to PTY

    Quit,
}

/// Process a key event given the current input mode.
/// Returns an Action when one should be dispatched.
pub fn handle_key(mode: &mut InputMode, ev: KeyEvent) -> Option<Action> {
    match mode {
        InputMode::AgentPicker { selected, count } => {
            let n = *count;
            match ev.code {
                KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                    Some(Action::PickerUp)
                }
                KeyCode::Down => {
                    *selected = (*selected + 1).min(n.saturating_sub(1));
                    Some(Action::PickerDown)
                }
                KeyCode::Enter => {
                    let idx = *selected;
                    *mode = InputMode::Dashboard;
                    Some(Action::PickerConfirm(idx))
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    *mode = InputMode::Dashboard;
                    Some(Action::PickerCancel)
                }
                _ => None,
            }
        }

        InputMode::Dashboard => match ev.code {
            KeyCode::Up    => Some(Action::DashboardUp),
            KeyCode::Down  => Some(Action::DashboardDown),
            KeyCode::Left  => Some(Action::DashboardLeft),
            KeyCode::Right => Some(Action::DashboardRight),
            KeyCode::Enter                       => Some(Action::DashboardSelect),
            KeyCode::Char('n')                   => Some(Action::AddPane),
            KeyCode::Char('x')                   => Some(Action::RemovePane),
            KeyCode::Char('b')                   => Some(Action::ToggleBroadcast),
            KeyCode::Char('q') | KeyCode::Esc   => Some(Action::Quit),
            _ => None,
        },

        InputMode::Detail => {
            if ev.code == KeyCode::Esc {
                *mode = InputMode::Dashboard;
                return Some(Action::BackToDashboard);
            }
            let bytes = keyevent_to_bytes(ev);
            if bytes.is_empty() { None } else { Some(Action::PaneInput(bytes)) }
        }
    }
}

/// Encode a crossterm KeyEvent into the raw VT bytes an agent process expects.
pub fn keyevent_to_bytes(ev: KeyEvent) -> Vec<u8> {
    match ev.code {
        KeyCode::Char(c) => {
            if ev.modifiers == KeyModifiers::CONTROL {
                let byte = (c as u8).to_ascii_uppercase();
                if (b'A'..=b'Z').contains(&byte) {
                    return vec![byte - b'A' + 1];
                }
            }
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        KeyCode::Enter     => vec![b'\r'],
        KeyCode::Tab       => vec![b'\t'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Delete    => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Esc       => vec![0x1b],
        KeyCode::Up        => vec![0x1b, b'[', b'A'],
        KeyCode::Down      => vec![0x1b, b'[', b'B'],
        KeyCode::Right     => vec![0x1b, b'[', b'C'],
        KeyCode::Left      => vec![0x1b, b'[', b'D'],
        KeyCode::Home      => vec![0x1b, b'[', b'H'],
        KeyCode::End       => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp    => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown  => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::F(n)      => f_key_bytes(n),
        _                  => vec![],
    }
}

fn f_key_bytes(n: u8) -> Vec<u8> {
    match n {
        1  => vec![0x1b, b'O', b'P'],
        2  => vec![0x1b, b'O', b'Q'],
        3  => vec![0x1b, b'O', b'R'],
        4  => vec![0x1b, b'O', b'S'],
        5  => vec![0x1b, b'[', b'1', b'5', b'~'],
        6  => vec![0x1b, b'[', b'1', b'7', b'~'],
        7  => vec![0x1b, b'[', b'1', b'8', b'~'],
        8  => vec![0x1b, b'[', b'1', b'9', b'~'],
        9  => vec![0x1b, b'[', b'2', b'0', b'~'],
        10 => vec![0x1b, b'[', b'2', b'1', b'~'],
        11 => vec![0x1b, b'[', b'2', b'3', b'~'],
        12 => vec![0x1b, b'[', b'2', b'4', b'~'],
        _  => vec![],
    }
}
