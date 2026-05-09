use std::io;
use std::time::Duration;

use agentmux_core::{
    config::load_config,
    ipc::client::connect_with_retry,
    protocol::{ClientMsg, ScreenSnapshot, ServerMsg},
    session::{PaneId, Session},
};
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
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    client::input::{handle_key, Action, InputMode},
    tui::widgets::{
        agent_picker::AgentPicker,
        dashboard::{Dashboard, DashboardItem},
        pane_view::PaneView,
        start_params::StartParamsModal,
    },
};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

pub struct RemotePane {
    pub pane_id: PaneId,
    pub agent_name: String,
    pub name: Option<String>,
    pub snapshot: ScreenSnapshot,
}

impl RemotePane {
    pub fn label(&self) -> String {
        self.name.as_deref().unwrap_or(&self.agent_name).to_string()
    }
}

pub struct AppState {
    pub panes: Vec<RemotePane>,
    pub selected: usize,
    pub title: String,
    pub cmd_tx: UnboundedSender<ClientMsg>,
    pub pending_enter_new: bool,
}

impl AppState {
    fn selected_pane_id(&self) -> Option<PaneId> {
        self.panes.get(self.selected).map(|p| p.pane_id)
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
        self.selected = (new_row * grid_cols + new_col).min(n - 1);
    }
}

// ---------------------------------------------------------------------------
// Session sync helpers
// ---------------------------------------------------------------------------

fn sync_panes_from_session(state: &mut AppState, session: &Session, current_size: (u16, u16)) {
    let all_panes: Vec<_> = session.windows.iter().flat_map(|w| w.panes.iter()).collect();

    for p in &all_panes {
        if !state.panes.iter().any(|rp| rp.pane_id == p.id) {
            let _ = state.cmd_tx.send(ClientMsg::SubscribePaneOutput { pane_id: p.id });
            let gc = Dashboard::grid_cols(state.panes.len() + 1);
            let (cols, rows) = thumb_size(current_size, state.panes.len() + 1, gc);
            let _ = state.cmd_tx.send(ClientMsg::ResizePane { pane_id: p.id, cols, rows });
            state.panes.push(RemotePane {
                pane_id: p.id,
                agent_name: p.agent_name.clone(),
                name: if p.name != p.agent_name { Some(p.name.clone()) } else { None },
                snapshot: ScreenSnapshot::blank(cols, rows),
            });
        }
    }

    let live: Vec<PaneId> = all_panes.iter().map(|p| p.id).collect();
    state.panes.retain(|rp| live.contains(&rp.pane_id));

    if state.selected >= state.panes.len() && !state.panes.is_empty() {
        state.selected = state.panes.len() - 1;
    }
}

fn handle_server_msg(msg: ServerMsg, state: &mut AppState, current_size: (u16, u16)) {
    match msg {
        ServerMsg::Attached { session } | ServerMsg::SessionUpdated { session } => {
            sync_panes_from_session(state, &session, current_size);
        }
        ServerMsg::PaneOutput(po) => {
            if let Some(pane) = state.panes.iter_mut().find(|p| p.pane_id == po.pane_id) {
                pane.snapshot = po.snapshot;
            }
        }
        ServerMsg::PaneExited { pane_id } => {
            state.panes.retain(|p| p.pane_id != pane_id);
            if state.selected >= state.panes.len() && !state.panes.is_empty() {
                state.selected = state.panes.len() - 1;
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Main run loop
// ---------------------------------------------------------------------------

pub async fn run(session_name: &str) -> Result<()> {
    let cfg = load_config();

    let agent_names: Vec<String> = if cfg.agent.is_empty() {
        vec!["claude".to_string()]
    } else {
        cfg.agent.iter().map(|a| a.name.clone()).collect()
    };
    let agent_commands: Vec<String> = if cfg.agent.is_empty() {
        vec!["claude".to_string()]
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

    let ipc = connect_with_retry(session_name).await?;
    let mut event_rx = ipc.event_rx;
    let cmd_tx = ipc.cmd_tx;

    let _ = cmd_tx.send(ClientMsg::AttachSession { name: session_name.to_string() });

    let mut state = AppState {
        panes: Vec::new(),
        selected: 0,
        title: session_name.to_string(),
        cmd_tx,
        pending_enter_new: false,
    };

    let mut current_size = crossterm::terminal::size().unwrap_or((220, 50));

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
            tokio::select! {
                _ = tick.tick() => {
                    let sel = state.selected;
                    let grid_cols = Dashboard::grid_cols(state.panes.len());

                    terminal.draw(|f| {
                        let area = f.area();
                        let chunks = RLayout::default()
                            .direction(Direction::Vertical)
                            .constraints([Constraint::Min(1), Constraint::Length(1)])
                            .split(area);
                        let content_area = chunks[0];
                        let status_area  = chunks[1];

                        match &mode {
                            InputMode::Dashboard
                            | InputMode::AgentPicker { .. }
                            | InputMode::StartParams { .. } => {
                                let dash_chunks = RLayout::default()
                                    .direction(Direction::Vertical)
                                    .constraints([Constraint::Length(1), Constraint::Min(1)])
                                    .split(content_area);
                                let title_area = dash_chunks[0];
                                let grid_area  = dash_chunks[1];

                                render_session_title(title_area, f.buffer_mut(), &state.title);

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
                                    })
                                    .collect();

                                Dashboard { items, cols: grid_cols }.render(grid_area, f.buffer_mut());

                                if let InputMode::AgentPicker { selected: ps, .. } = &mode {
                                    AgentPicker { agents: &agent_names, selected: *ps, show_custom: true }
                                        .render_popup(grid_area, f.buffer_mut());
                                }
                                if let InputMode::StartParams { cmd, args, cwd, title, focused } = &mode {
                                    StartParamsModal { cmd, args, cwd, title, focused: *focused }
                                        .render_popup(grid_area, f.buffer_mut());
                                }

                                render_dashboard_status(status_area, f.buffer_mut());
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
                                render_detail_status(status_area, f.buffer_mut(),
                                    state.panes.get(sel).map(|p| p.label()).as_deref().unwrap_or(""));
                            }

                            InputMode::Rename { buf } | InputMode::RenameTitle { buf } => {
                                let is_title = matches!(&mode, InputMode::RenameTitle { .. });
                                let dash_chunks = RLayout::default()
                                    .direction(Direction::Vertical)
                                    .constraints([Constraint::Length(1), Constraint::Min(1)])
                                    .split(content_area);
                                let display_title = if is_title { format!("{}_", buf) } else { state.title.clone() };
                                render_session_title(dash_chunks[0], f.buffer_mut(), &display_title);
                                let labels: Vec<String> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, p)| format!("[{}] {}", i + 1, p.label()))
                                    .collect();
                                let items: Vec<DashboardItem> = state.panes.iter()
                                    .enumerate()
                                    .map(|(i, pane)| DashboardItem {
                                        snapshot: &pane.snapshot, title: &labels[i],
                                        selected: i == sel,
                                    })
                                    .collect();
                                Dashboard { items, cols: grid_cols }.render(dash_chunks[1], f.buffer_mut());
                                if is_title {
                                    render_rename_title_status(status_area, f.buffer_mut());
                                } else {
                                    render_rename_status(status_area, f.buffer_mut(), buf);
                                }
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
                                                let (cols, rows) = current_size;
                                                let pc = cols.saturating_sub(2).max(1);
                                                let pr = rows.saturating_sub(3).max(1);
                                                if let Some(id) = state.selected_pane_id() {
                                                    let _ = state.cmd_tx.send(ClientMsg::ResizePane { pane_id: id, cols: pc, rows: pr });
                                                }
                                            }
                                        }

                                        Action::BackToDashboard => {
                                            mode = InputMode::Dashboard;
                                            resize_all_for_dashboard(&state, current_size, grid_cols);
                                        }

                                        Action::AddPane => {
                                            // +1 for the "Custom command..." entry at the bottom
                                            mode = InputMode::AgentPicker { selected: 0, count: agent_names.len() + 1 };
                                        }

                                        Action::AddPaneDirect => {
                                            let cmd  = agent_commands.first().cloned().unwrap_or_default();
                                            let args = agent_args.first().cloned().unwrap_or_default();
                                            let cwd  = agent_cwds.first().cloned().unwrap_or(None);
                                            let _ = state.cmd_tx.send(ClientMsg::SpawnPane { cmd, args, cwd });
                                            state.pending_enter_new = true;
                                        }

                                        Action::PickerConfirm(idx) => {
                                            if idx == agent_names.len() {
                                                // "Custom command..." selected — open the params form
                                                mode = InputMode::StartParams {
                                                    cmd: String::new(), args: String::new(),
                                                    cwd: String::new(), title: String::new(), focused: 0,
                                                };
                                            } else {
                                                let cmd  = agent_commands.get(idx).cloned().unwrap_or_default();
                                                let args = agent_args.get(idx).cloned().unwrap_or_default();
                                                let cwd  = agent_cwds.get(idx).cloned().unwrap_or(None);
                                                let _ = state.cmd_tx.send(ClientMsg::SpawnPane { cmd, args, cwd });
                                                state.pending_enter_new = true;
                                                mode = InputMode::Dashboard;
                                            }
                                        }

                                        Action::PickerCancel | Action::PickerUp | Action::PickerDown => {}

                                        Action::StartRename => {
                                            let current = state.panes.get(state.selected).map(|p| p.label()).unwrap_or_default();
                                            mode = InputMode::Rename { buf: current };
                                        }

                                        Action::ConfirmRename(name) => {
                                            if let Some(p) = state.panes.get_mut(state.selected) {
                                                p.name = if name.is_empty() { None } else { Some(name) };
                                            }
                                        }

                                        Action::CancelRename => {}

                                        Action::StartRenameTitle => {
                                            mode = InputMode::RenameTitle { buf: state.title.clone() };
                                        }

                                        Action::ConfirmRenameTitle(name) => {
                                            if !name.is_empty() { state.title = name; }
                                        }

                                        Action::CancelRenameTitle => {}

                                        Action::StartParamsConfirm { cmd, args: args_str, cwd, title } => {
                                            if !cmd.is_empty() {
                                                let args = args_str.split_whitespace().map(String::from).collect();
                                                let cwd_opt = if cwd.is_empty() { None } else { Some(cwd) };
                                                let _ = state.cmd_tx.send(ClientMsg::SpawnPane { cmd, args, cwd: cwd_opt });
                                                if !title.is_empty() { state.title = title; }
                                                state.pending_enter_new = true;
                                                mode = InputMode::Dashboard;
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
                                                if let Some(id) = state.selected_pane_id() {
                                                    let _ = state.cmd_tx.send(ClientMsg::ResizePane { pane_id: id, cols: pc, rows: pr });
                                                }
                                            }
                                        }

                                        Action::RemovePane => {
                                            if let Some(id) = state.selected_pane_id() {
                                                let _ = state.cmd_tx.send(ClientMsg::ClosePane { pane_id: id });
                                            }
                                        }

                                        Action::PaneInput(bytes) => {
                                            if let Some(id) = state.selected_pane_id() {
                                                let _ = state.cmd_tx.send(ClientMsg::PaneInput { pane_id: id, data: bytes });
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
                                        if let Some(id) = state.selected_pane_id() {
                                            let _ = state.cmd_tx.send(ClientMsg::ResizePane { pane_id: id, cols: pc, rows: pr });
                                        }
                                    }
                                    _ => resize_all_for_dashboard(&state, (cols, rows), grid_cols),
                                }
                            }

                            _ => {}
                        }
                    }
                }

                maybe_ipc = event_rx.recv() => {
                    match maybe_ipc {
                        None => break,
                        Some(msg) => {
                            let old_len = state.panes.len();
                            handle_server_msg(msg, &mut state, current_size);
                            if state.pending_enter_new && state.panes.len() > old_len {
                                state.pending_enter_new = false;
                                state.selected = state.panes.len() - 1;
                                mode = InputMode::Detail;
                                let (cols, rows) = current_size;
                                let pc = cols.saturating_sub(2).max(1);
                                let pr = rows.saturating_sub(3).max(1);
                                if let Some(id) = state.selected_pane_id() {
                                    let _ = state.cmd_tx.send(ClientMsg::ResizePane { pane_id: id, cols: pc, rows: pr });
                                }
                            }
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

// ---------------------------------------------------------------------------
// Resize helpers
// ---------------------------------------------------------------------------

fn thumb_size(term: (u16, u16), n: usize, grid_cols: usize) -> (u16, u16) {
    let rows_count = n.div_ceil(grid_cols) as u16;
    let cell_w = (term.0 / grid_cols as u16).saturating_sub(2).max(1);
    let cell_h = ((term.1.saturating_sub(2)) / rows_count).saturating_sub(2).max(1);
    (cell_w, cell_h)
}

fn resize_all_for_dashboard(state: &AppState, term: (u16, u16), grid_cols: usize) {
    let n = state.panes.len();
    if n == 0 {
        return;
    }
    let (tc, tr) = thumb_size(term, n, grid_cols);
    for pane in &state.panes {
        let _ = state.cmd_tx.send(ClientMsg::ResizePane { pane_id: pane.pane_id, cols: tc, rows: tr });
    }
}

// ---------------------------------------------------------------------------
// Status bar renderers
// ---------------------------------------------------------------------------

fn render_session_title(area: Rect, buf: &mut ratatui::buffer::Buffer, title: &str) {
    let amber = Color::Rgb(255, 176, 0);
    let centered = format!("{:^width$}", title, width = area.width as usize);
    Line::from(Span::styled(centered, Style::default().fg(amber).add_modifier(Modifier::BOLD)))
        .render(area, buf);
}

fn render_dashboard_status(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) { c.set_char(' '); c.set_style(bg); }
    }
    Line::from(vec![
        Span::styled(" DASHBOARD ", Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  ↑↓←→: navigate  1-9/Enter: open  n: new(enter)  N: pick agent/custom  x: close  r: rename  q: quit", bg),
    ]).render(area, buf);
}

fn render_rename_title_status(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) { c.set_char(' '); c.set_style(bg); }
    }
    Line::from(vec![
        Span::styled(" RENAME TITLE ", Style::default().bg(Color::Rgb(255, 176, 0)).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled("  Type new title  Enter: confirm  Esc: cancel", Style::default().bg(Color::DarkGray).fg(Color::Gray)),
    ]).render(area, buf);
}

fn render_rename_status(area: Rect, buf: &mut ratatui::buffer::Buffer, input: &str) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) { c.set_char(' '); c.set_style(bg); }
    }
    Line::from(vec![
        Span::styled(" RENAME ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled("  New name: ", bg),
        Span::styled(format!("{}_", input), Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  Enter: confirm  Esc: cancel", Style::default().bg(Color::DarkGray).fg(Color::Gray)),
    ]).render(area, buf);
}

fn render_detail_status(area: Rect, buf: &mut ratatui::buffer::Buffer, agent: &str) {
    let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
    for x in area.x..area.x + area.width {
        if let Some(c) = buf.cell_mut((x, area.y)) { c.set_char(' '); c.set_style(bg); }
    }
    Line::from(vec![
        Span::styled(format!(" {} ", agent), Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled("  Esc/Ctrl+\\: back to dashboard", bg),
    ]).render(area, buf);
}
