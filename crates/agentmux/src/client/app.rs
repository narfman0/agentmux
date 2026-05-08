use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, EventStream, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt as _;
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
        start_params::StartParamsModal,
    },
};

pub struct AppState {
    pub panes: Vec<PaneState>,
    pub selected: usize,
    pub broadcast: bool,
    pub default_cwd: Option<String>,
}

impl AppState {
    pub fn new(initial_cmd: &str, initial_args: &[&str], cols: u16, rows: u16, cwd: Option<String>) -> Result<Self> {
        let pane = PaneState::new(initial_cmd, initial_args, cols, rows, cwd.as_deref())?;
        Ok(Self {
            panes: vec![pane],
            selected: 0,
            broadcast: false,
            default_cwd: cwd,
        })
    }

    fn selected_pane_mut(&mut self) -> Option<&mut PaneState> {
        self.panes.get_mut(self.selected)
    }

    fn add_pane(&mut self, cmd: &str, args: &[&str], agent_cwd: Option<&str>, cols: u16, rows: u16) -> Result<()> {
        let effective_cwd = agent_cwd.or(self.default_cwd.as_deref());
        let pane = PaneState::new(cmd, args, cols, rows, effective_cwd)?;
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

pub async fn run(initial_cmd: &str, cwd: Option<String>) -> Result<()> {
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
    let agent_args: Vec<Vec<String>> = if cfg.agent.is_empty() {
        vec![vec![]]
    } else {
        cfg.agent.iter().map(|a| a.args.clone()).collect()
    };
    let agent_cwds: Vec<Option<String>> = if cfg.agent.is_empty() {
        vec![None]
    } else {
        cfg.agent.iter().map(|a| a.cwd.clone()).collect()
    };

    let term_size = crossterm::terminal::size().unwrap_or((220, 50));
    let pane_cols = term_size.0.saturating_sub(2).max(1);
    let pane_rows = term_size.1.saturating_sub(3).max(1); // reserve status + border

    let initial_args: Vec<String> = agent_args.first().cloned().unwrap_or_default();
    let initial_args_refs: Vec<&str> = initial_args.iter().map(|s| s.as_str()).collect();
    let mut state = AppState::new(initial_cmd, &initial_args_refs, pane_cols, pane_rows, cwd)?;
    let mut current_size = term_size;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut mode = InputMode::Dashboard;
    let mut tick = tokio::time::interval(Duration::from_millis(33));
    let mut event_reader = EventStream::new();

    let result: Result<()> = async {
        loop {
            if state.panes.is_empty() {
                break;
            }

            tokio::select! {
                _ = tick.tick() => {
                    // Refresh snapshots only for panes with new output since last tick.
                    for pane in &mut state.panes {
                        pane.refresh_snapshot();
                    }

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
                            InputMode::Dashboard | InputMode::AgentPicker { .. } | InputMode::StartParams { .. } => {
                                let labels: Vec<String> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, p)| format!("[{}] {}", i + 1, p.label()))
                                    .collect();

                                let items: Vec<DashboardItem> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, pane)| DashboardItem {
                                        snapshot: &pane.snapshot,
                                        title: &labels[i],
                                        selected: i == sel,
                                        broadcast,
                                    })
                                    .collect();

                                Dashboard { items, cols: grid_cols }.render(content_area, f.buffer_mut());

                                if let InputMode::AgentPicker { selected: picker_sel, .. } = &mode {
                                    AgentPicker {
                                        agents: &agent_names,
                                        selected: *picker_sel,
                                    }
                                    .render_popup(content_area, f.buffer_mut());
                                }

                                if let InputMode::StartParams { cmd, args, cwd, focused } = &mode {
                                    StartParamsModal { cmd, args, cwd, focused: *focused }
                                        .render_popup(content_area, f.buffer_mut());
                                }

                                render_dashboard_status(status_area, f.buffer_mut(), broadcast);
                            }

                            InputMode::Detail => {
                                if let Some(pane) = state.panes.get(sel) {
                                    let label = format!("[{}] {}", sel + 1, pane.label());
                                    PaneView { snapshot: &pane.snapshot, focused: true, title: &label }
                                        .render(content_area, f.buffer_mut());

                                    if !pane.snapshot.cursor_hidden {
                                        let ix = content_area.x + 1;
                                        let iy = content_area.y + 1;
                                        let max_x = content_area.x + content_area.width.saturating_sub(2);
                                        let max_y = content_area.y + content_area.height.saturating_sub(2);
                                        let cx = (ix + pane.snapshot.cursor_col).min(max_x);
                                        let cy = (iy + pane.snapshot.cursor_row).min(max_y);
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

                            InputMode::Rename { buf } => {
                                let labels: Vec<String> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, p)| format!("[{}] {}", i + 1, p.label()))
                                    .collect();
                                let items: Vec<DashboardItem> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, pane)| DashboardItem {
                                        snapshot: &pane.snapshot,
                                        title: &labels[i],
                                        selected: i == sel,
                                        broadcast,
                                    })
                                    .collect();
                                Dashboard { items, cols: grid_cols }.render(content_area, f.buffer_mut());
                                render_rename_status(status_area, f.buffer_mut(), buf);
                            }
                        }
                    })?;
                }

                maybe_event = event_reader.next() => {
                    let first = match maybe_event {
                        Some(Ok(ev)) => Some(ev),
                        Some(Err(_)) | None => break,
                    };
                    let mut events = Vec::with_capacity(8);
                    events.extend(first);
                    while event::poll(Duration::from_millis(0))? {
                        events.push(event::read()?);
                    }
                    for ev in events {
                        match ev {
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
                                            let empty: Vec<String> = vec![];
                                            let args_strs = agent_args.get(idx).unwrap_or(&empty);
                                            let args_refs: Vec<&str> = args_strs.iter().map(|s| s.as_str()).collect();
                                            let agent_cwd = agent_cwds.get(idx).and_then(|c| c.as_deref());
                                            let gc = Dashboard::grid_cols(state.panes.len() + 1);
                                            let (tc, tr) = thumbnail_size(current_size, state.panes.len() + 1, gc);
                                            state.add_pane(cmd, &args_refs, agent_cwd, tc, tr)?;
                                            let (cols, rows) = current_size;
                                            let pc = cols.saturating_sub(2).max(1);
                                            let pr = rows.saturating_sub(3).max(1);
                                            if let Some(p) = state.selected_pane_mut() {
                                                p.resize(pc, pr);
                                            }
                                            mode = InputMode::Detail;
                                        }

                                        Action::PickerCancel | Action::PickerUp | Action::PickerDown => {}

                                        Action::StartRename => {
                                            let current = state.panes.get(state.selected)
                                                .map(|p: &PaneState| p.label())
                                                .unwrap_or_default();
                                            mode = InputMode::Rename { buf: current };
                                        }

                                        Action::ConfirmRename(name) => {
                                            if let Some(p) = state.panes.get_mut(state.selected) {
                                                p.name = if name.is_empty() { None } else { Some(name) };
                                            }
                                        }

                                        Action::CancelRename => {}

                                        Action::OpenStartParams => {
                                            mode = InputMode::StartParams {
                                                cmd: String::new(),
                                                args: String::new(),
                                                cwd: String::new(),
                                                focused: 0,
                                            };
                                        }

                                        Action::StartParamsConfirm { cmd, args: args_str, cwd } => {
                                            let parsed: Vec<String> = args_str.split_whitespace().map(String::from).collect();
                                            let args_refs: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
                                            let effective_cwd = if cwd.is_empty() { None } else { Some(cwd.as_str()) };
                                            let gc = Dashboard::grid_cols(state.panes.len() + 1);
                                            let (tc, tr) = thumbnail_size(current_size, state.panes.len() + 1, gc);
                                            if !cmd.is_empty() {
                                                state.add_pane(&cmd, &args_refs, effective_cwd, tc, tr)?;
                                                let (cols, rows) = current_size;
                                                let pc = cols.saturating_sub(2).max(1);
                                                let pr = rows.saturating_sub(3).max(1);
                                                if let Some(p) = state.selected_pane_mut() {
                                                    p.resize(pc, pr);
                                                }
                                                mode = InputMode::Detail;
                                            }
                                        }

                                        Action::StartParamsCancel => {}

                                        Action::SelectPane(idx) => {
                                            if idx < state.panes.len() {
                                                state.selected = idx;
                                                mode = InputMode::Detail;
                                                let (cols, rows) = current_size;
                                                let pc = cols.saturating_sub(2).max(1);
                                                let pr = rows.saturating_sub(3).max(1);
                                                if let Some(p) = state.selected_pane_mut() {
                                                    p.resize(pc, pr);
                                                }
                                            }
                                        }

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
        Span::styled("  ↑↓←→/hjkl: navigate  1-9: open  Enter: open  n: new  N: custom  x: close  r: rename", bg),
    ];
    if broadcast {
        spans.push(Span::styled("  [BROADCAST] ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::styled("  b: broadcast  q: quit", bg));
    }
    Line::from(spans).render(area, buf);
}

fn render_rename_status(area: Rect, buf: &mut ratatui::buffer::Buffer, input: &str) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) {
            c.set_char(' ');
            c.set_style(bg);
        }
    }
    let spans = vec![
        Span::styled(" RENAME ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled("  New name: ", bg),
        Span::styled(format!("{}_", input), Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  Enter: confirm  Esc: cancel", Style::default().bg(Color::DarkGray).fg(Color::Gray)),
    ];
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
