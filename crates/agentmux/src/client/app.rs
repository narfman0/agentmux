use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout as RLayout, Rect},
    prelude::Widget,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Terminal,
};

use crate::{
    client::input::{handle_key, Action, InputMode},
    config::loader,
    pane::PaneState,
    tui::widgets::{
        agent_picker::AgentPicker,
        dashboard::{Dashboard, DashboardItem},
        pane_view::PaneView,
    },
};

pub struct AppState {
    pub panes: Vec<PaneState>,
    pub selected: usize,
    pub broadcast: bool,
}

impl AppState {
    pub fn new(initial_cmd: &str, cols: u16, rows: u16) -> Result<Self> {
        let pane = PaneState::new(initial_cmd, cols, rows)?;
        Ok(Self {
            panes: vec![pane],
            selected: 0,
            broadcast: false,
        })
    }

    fn selected_pane_mut(&mut self) -> Option<&mut PaneState> {
        self.panes.get_mut(self.selected)
    }

    fn add_pane(&mut self, cmd: &str, cols: u16, rows: u16) -> Result<()> {
        let pane = PaneState::new(cmd, cols, rows)?;
        self.panes.push(pane);
        self.selected = self.panes.len() - 1;
        Ok(())
    }

    fn remove_selected(&mut self) {
        if self.panes.is_empty() {
            return;
        }
        self.panes.remove(self.selected);
        if self.selected >= self.panes.len() && !self.panes.is_empty() {
            self.selected = self.panes.len() - 1;
        }
    }

    fn navigate(&mut self, delta_row: i32, delta_col: i32, grid_cols: usize) {
        let n = self.panes.len();
        if n == 0 {
            return;
        }
        let row = (self.selected / grid_cols) as i32 + delta_row;
        let col = (self.selected % grid_cols) as i32 + delta_col;
        let rows = n.div_ceil(grid_cols) as i32;
        let cols = grid_cols as i32;

        let new_row = row.clamp(0, rows - 1) as usize;
        let new_col = col.clamp(0, cols - 1) as usize;
        let idx = (new_row * grid_cols + new_col).min(n - 1);
        self.selected = idx;
    }
}

pub async fn run(initial_cmd: &str) -> Result<()> {
    let cfg = loader::load();
    loader::ensure_example_config();

    let agent_names: Vec<String> = if cfg.agent.is_empty() {
        vec![initial_cmd.to_string()]
    } else {
        cfg.agent.iter().map(|a| a.name.clone()).collect()
    };
    let agent_commands: Vec<String> = if cfg.agent.is_empty() {
        vec![initial_cmd.to_string()]
    } else {
        cfg.agent.iter().map(|a| a.command.clone()).collect()
    };

    let term_size = crossterm::terminal::size().unwrap_or((220, 50));
    let pane_cols = term_size.0.saturating_sub(2).max(1);
    let pane_rows = term_size.1.saturating_sub(3).max(1); // reserve status + border

    let mut state = AppState::new(initial_cmd, pane_cols, pane_rows)?;
    let mut current_size = term_size;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut mode = InputMode::Dashboard;
    let mut tick = tokio::time::interval(Duration::from_millis(33));

    let result: Result<()> = async {
        loop {
            if state.panes.is_empty() {
                break;
            }

            tokio::select! {
                _ = tick.tick() => {
                    let sel = state.selected;
                    let broadcast = state.broadcast;
                    let grid_cols = Dashboard::grid_cols(state.panes.len());

                    terminal.draw(|f| {
                        let area = f.area();
                        let chunks = RLayout::default()
                            .direction(Direction::Vertical)
                            .constraints([Constraint::Min(1), Constraint::Length(1)])
                            .split(area);

                        let content_area = chunks[0];
                        let status_area = chunks[1];

                        match &mode {
                            InputMode::Dashboard | InputMode::AgentPicker { .. } => {
                                let snapshots: Vec<_> = state.panes.iter()
                                    .map(|p| p.snapshot.lock().unwrap().clone())
                                    .collect();
                                let labels: Vec<String> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, p)| format!("[{}] {}", i + 1, p.label()))
                                    .collect();

                                let items: Vec<DashboardItem> = snapshots.iter()
                                    .enumerate()
                                    .map(|(i, snap)| DashboardItem {
                                        snapshot: snap,
                                        title: &labels[i],
                                        selected: i == sel,
                                        broadcast,
                                    })
                                    .collect();

                                Dashboard { items, cols: grid_cols }.render(content_area, f.buffer_mut());

                                // Agent picker overlay
                                if let InputMode::AgentPicker { selected: picker_sel, .. } = &mode {
                                    AgentPicker {
                                        agents: &agent_names,
                                        selected: *picker_sel,
                                    }
                                    .render_popup(content_area, f.buffer_mut());
                                }

                                // Status bar: dashboard hint
                                render_dashboard_status(status_area, f.buffer_mut(), broadcast);
                            }

                            InputMode::Detail => {
                                // Full-screen view of selected pane
                                if let Some(pane) = state.panes.get(sel) {
                                    let snap = pane.snapshot.lock().unwrap().clone();
                                    let label = format!("[{}] {}", sel + 1, pane.label());
                                    PaneView { snapshot: &snap, focused: true, title: &label }
                                        .render(content_area, f.buffer_mut());

                                    if !snap.cursor_hidden {
                                        let ix = content_area.x + 1;
                                        let iy = content_area.y + 1;
                                        let max_x = content_area.x + content_area.width.saturating_sub(2);
                                        let max_y = content_area.y + content_area.height.saturating_sub(2);
                                        let cx = (ix + snap.cursor_col).min(max_x);
                                        let cy = (iy + snap.cursor_row).min(max_y);
                                        f.set_cursor_position((cx, cy));
                                    }
                                }

                                render_detail_status(
                                    status_area,
                                    f.buffer_mut(),
                                    state.panes.get(sel).map(|p| p.label()).as_deref().unwrap_or(""),
                                    broadcast,
                                );
                            }
                        }
                    })?;
                }

                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    if event::poll(Duration::from_millis(0))? {
                        match event::read()? {
                            Event::Key(key) if key.kind == KeyEventKind::Press => {
                                let grid_cols = Dashboard::grid_cols(state.panes.len());
                                if let Some(action) = handle_key(&mut mode, key) {
                                    match action {
                                        Action::Quit => return Ok(()),

                                        Action::DashboardUp    => state.navigate(-1,  0, grid_cols),
                                        Action::DashboardDown  => state.navigate( 1,  0, grid_cols),
                                        Action::DashboardLeft  => state.navigate( 0, -1, grid_cols),
                                        Action::DashboardRight => state.navigate( 0,  1, grid_cols),

                                        Action::DashboardSelect => {
                                            if !state.panes.is_empty() {
                                                mode = InputMode::Detail;
                                                // Resize selected pane to full content area on enter
                                                let (cols, rows) = current_size;
                                                let pc = cols.saturating_sub(2).max(1);
                                                let pr = rows.saturating_sub(3).max(1);
                                                if let Some(p) = state.selected_pane_mut() {
                                                    p.resize(pc, pr);
                                                }
                                            }
                                        }

                                        Action::BackToDashboard => {
                                            mode = InputMode::Dashboard;
                                            // Resize pane back to thumbnail size
                                            resize_all_for_dashboard(&mut state, current_size, grid_cols);
                                        }

                                        Action::AddPane => {
                                            let count = agent_names.len().max(1);
                                            mode = InputMode::AgentPicker { selected: 0, count };
                                        }

                                        Action::PickerConfirm(idx) => {
                                            let cmd = agent_commands.get(idx).map(|s| s.as_str()).unwrap_or(initial_cmd);
                                            let gc = Dashboard::grid_cols(state.panes.len() + 1);
                                            let (tc, tr) = thumbnail_size(current_size, state.panes.len() + 1, gc);
                                            state.add_pane(cmd, tc, tr)?;
                                        }

                                        Action::PickerCancel | Action::PickerUp | Action::PickerDown => {}

                                        Action::RemovePane => {
                                            state.remove_selected();
                                            if state.panes.is_empty() {
                                                return Ok(());
                                            }
                                        }

                                        Action::ToggleBroadcast => {
                                            state.broadcast = !state.broadcast;
                                        }

                                        Action::PaneInput(bytes) => {
                                            if state.broadcast {
                                                for p in &mut state.panes {
                                                    let _ = p.write_input(&bytes);
                                                }
                                            } else if let Some(p) = state.selected_pane_mut() {
                                                p.write_input(&bytes)?;
                                            }
                                        }
                                    }
                                }
                            }

                            Event::Resize(cols, rows) => {
                                current_size = (cols, rows);
                                let grid_cols = Dashboard::grid_cols(state.panes.len());
                                match mode {
                                    InputMode::Detail => {
                                        let pc = cols.saturating_sub(2).max(1);
                                        let pr = rows.saturating_sub(3).max(1);
                                        if let Some(p) = state.selected_pane_mut() {
                                            p.resize(pc, pr);
                                        }
                                    }
                                    _ => resize_all_for_dashboard(&mut state, (cols, rows), grid_cols),
                                }
                            }

                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn thumbnail_size(term: (u16, u16), n: usize, grid_cols: usize) -> (u16, u16) {
    let rows_count = n.div_ceil(grid_cols) as u16;
    let cell_w = (term.0 / grid_cols as u16).saturating_sub(2).max(1);
    let cell_h = ((term.1.saturating_sub(1)) / rows_count).saturating_sub(2).max(1);
    (cell_w, cell_h)
}

fn resize_all_for_dashboard(state: &mut AppState, term: (u16, u16), grid_cols: usize) {
    let n = state.panes.len();
    let (tc, tr) = thumbnail_size(term, n, grid_cols);
    for p in &mut state.panes {
        p.resize(tc, tr);
    }
}

fn render_dashboard_status(area: Rect, buf: &mut ratatui::buffer::Buffer, broadcast: bool) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) {
            c.set_char(' ');
            c.set_style(bg);
        }
    }

    let mut spans = vec![
        Span::styled(" DASHBOARD ", Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  ↑↓←→/hjkl: navigate  Enter: open  n: new  x: close", bg),
    ];
    if broadcast {
        spans.push(Span::styled("  [BROADCAST] ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::styled("  b: broadcast  q: quit", bg));
    }
    Line::from(spans).render(area, buf);
}

fn render_detail_status(area: Rect, buf: &mut ratatui::buffer::Buffer, agent: &str, broadcast: bool) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) {
            c.set_char(' ');
            c.set_style(bg);
        }
    }
    let mut spans = vec![
        Span::styled(format!(" {} ", agent), Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled("  Esc: back to dashboard", bg),
    ];
    if broadcast {
        spans.push(Span::styled("  [BROADCAST] ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)));
    }
    Line::from(spans).render(area, buf);
}
