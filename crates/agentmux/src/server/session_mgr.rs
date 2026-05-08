/// Server-side session state: holds all panes, their PTY masters/writers, and shared state.
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use agentmux_core::protocol::ScreenSnapshot;
use agentmux_core::session::{Layout, Pane, PaneId, Session, SessionId, Window, WindowId};
use uuid::Uuid;

use crate::layout_ops::{remove_from_layout, split_layout, SplitDir};
use crate::pty::spawn::spawn_agent;
use crate::server::pane_task::{self, ClientTx, PaneShared};

pub struct ServerPane {
    pub meta: Pane,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    // Held for its Drop impl — keeps the child process alive while the pane exists.
    _child: Box<dyn portable_pty::Child + Send>,
    pub shared: Arc<PaneShared>,
}

pub struct ServerSession {
    pub meta: Session,
    pub panes: HashMap<PaneId, ServerPane>,
}

impl ServerSession {
    pub fn new(name: &str, agent_cmd: &str, agent_args: &[&str], cols: u16, rows: u16) -> anyhow::Result<Self> {
        let session_id: SessionId = Uuid::new_v4();
        let window_id: WindowId = Uuid::new_v4();
        let pane_id: PaneId = Uuid::new_v4();

        let handle = spawn_agent(agent_cmd, agent_args, cols, rows, None)?;
        let writer = handle
            .master
            .take_writer()
            .map_err(|e| anyhow::anyhow!("PTY writer: {e}"))?;

        let shared = Arc::new(PaneShared::new(cols, rows));
        pane_task::spawn(pane_id, handle.master.as_ref(), shared.clone(), cols, rows)?;

        let pane_meta = Pane {
            id: pane_id,
            name: agent_cmd.to_string(),
            agent_name: agent_cmd.to_string(),
            cols,
            rows,
            alive: true,
        };

        let window = Window {
            id: window_id,
            name: "main".to_string(),
            layout: Layout::Leaf(pane_id),
            panes: vec![pane_meta.clone()],
            focused: pane_id,
        };

        let session = Session {
            id: session_id,
            name: name.to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            windows: vec![window],
            active_win: window_id,
        };

        let server_pane = ServerPane {
            meta: pane_meta,
            master: handle.master,
            writer,
            _child: handle.child,
            shared,
        };

        let mut panes = HashMap::new();
        panes.insert(pane_id, server_pane);

        Ok(Self { meta: session, panes })
    }

    /// Split an existing pane, spawning a new agent process.
    pub fn split_pane(
        &mut self,
        target_id: PaneId,
        dir: SplitDir,
        agent_cmd: &str,
        agent_args: &[&str],
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<PaneId> {
        let new_id: PaneId = Uuid::new_v4();
        let handle = spawn_agent(agent_cmd, agent_args, cols, rows, None)?;
        let writer = handle
            .master
            .take_writer()
            .map_err(|e| anyhow::anyhow!("PTY writer: {e}"))?;
        let shared = Arc::new(PaneShared::new(cols, rows));
        pane_task::spawn(new_id, handle.master.as_ref(), shared.clone(), cols, rows)?;

        let pane_meta = Pane {
            id: new_id,
            name: agent_cmd.to_string(),
            agent_name: agent_cmd.to_string(),
            cols,
            rows,
            alive: true,
        };

        self.panes.insert(
            new_id,
            ServerPane { meta: pane_meta.clone(), master: handle.master, writer, _child: handle.child, shared },
        );

        // Update the layout in the active window
        if let Some(win) = self.meta.windows.first_mut() {
            win.layout = split_layout(win.layout.clone(), target_id, new_id, dir);
            win.panes.push(pane_meta);
            win.focused = new_id;
        }

        Ok(new_id)
    }

    pub fn close_pane(&mut self, pane_id: PaneId) {
        self.panes.remove(&pane_id);
        if let Some(win) = self.meta.windows.first_mut() {
            if let Some(new_layout) = remove_from_layout(win.layout.clone(), pane_id) {
                win.layout = new_layout;
            }
            win.panes.retain(|p| p.id != pane_id);
            if win.focused == pane_id {
                win.focused = win.panes.first().map(|p| p.id).unwrap_or(pane_id);
            }
        }
    }

    pub fn resize_pane(&mut self, pane_id: PaneId, cols: u16, rows: u16) {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.master
                .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .ok();
            pane.meta.cols = cols;
            pane.meta.rows = rows;
        }
    }

    pub fn write_pane(&mut self, pane_id: PaneId, data: &[u8]) -> anyhow::Result<()> {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.writer.write_all(data).map_err(|e| anyhow::anyhow!("pty write: {e}"))
        } else {
            Err(anyhow::anyhow!("pane not found"))
        }
    }

    pub fn broadcast_input(&mut self, data: &[u8]) {
        for pane in self.panes.values_mut() {
            let _ = pane.writer.write_all(data);
        }
    }

    pub fn subscribe_pane(&self, pane_id: PaneId, tx: ClientTx) -> Option<ScreenSnapshot> {
        let pane = self.panes.get(&pane_id)?;
        pane.shared.subscribe(tx);
        Some(pane.shared.latest_snapshot())
    }

    pub fn session_meta(&self) -> Session {
        self.meta.clone()
    }
}

/// Global server state — one session per named pipe.
pub struct ServerState {
    pub sessions: HashMap<String, ServerSession>,
}

impl ServerState {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }
}
