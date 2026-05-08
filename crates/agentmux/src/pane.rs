use std::io::Write;
use std::sync::{Arc, Mutex};

use agentmux_core::protocol::ScreenSnapshot;
use agentmux_core::session::PaneId;
use uuid::Uuid;

use crate::pty::{reader::spawn_reader, spawn::spawn_agent};
use crate::vt::screen::Screen;

pub struct PaneState {
    pub id: PaneId,
    pub agent_cmd: String,
    pub snapshot: Arc<Mutex<ScreenSnapshot>>,
    /// Shared with the reader task for resize.
    screen: Arc<Mutex<Screen>>,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
}

impl PaneState {
    pub fn new(agent_cmd: &str, cols: u16, rows: u16) -> anyhow::Result<Self> {
        let id: PaneId = Uuid::new_v4();
        let handle = spawn_agent(agent_cmd, &[], cols, rows)?;

        let writer = handle
            .master
            .take_writer()
            .map_err(|e| anyhow::anyhow!("PTY writer: {e}"))?;
        let rx = spawn_reader(handle.master.as_ref())?;

        let screen = Arc::new(Mutex::new(Screen::new(rows, cols)));
        let snapshot = Arc::new(Mutex::new(ScreenSnapshot::blank(cols, rows)));

        {
            let screen = screen.clone();
            let snapshot = snapshot.clone();
            tokio::spawn(async move {
                let mut rx = rx;
                while let Some(data) = rx.recv().await {
                    let mut sc = screen.lock().unwrap();
                    sc.feed(&data);
                    *snapshot.lock().unwrap() = sc.snapshot();
                }
            });
        }

        Ok(Self {
            id,
            agent_cmd: agent_cmd.to_string(),
            snapshot,
            screen,
            master: handle.master,
            writer,
            child: handle.child,
        })
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.master
            .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .ok();
        self.screen.lock().unwrap().set_size(rows, cols);
    }

    pub fn write_input(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer
            .write_all(data)
            .map_err(|e| anyhow::anyhow!("pty write: {e}"))
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    pub fn label(&self) -> String {
        self.agent_cmd.clone()
    }
}
