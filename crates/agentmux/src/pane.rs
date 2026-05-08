use std::io::Write;
use std::sync::{Arc, Mutex};

use agentmux_core::protocol::ScreenSnapshot;

use crate::pty::{reader::spawn_reader, spawn::spawn_agent};
use crate::vt::screen::Screen;

pub struct PaneState {
    pub agent_cmd: String,
    pub name: Option<String>,
    pub snapshot: Arc<Mutex<ScreenSnapshot>>,
    screen: Arc<Mutex<Screen>>,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    // Held for its Drop impl — keeps the child process alive while the pane exists.
    _child: Box<dyn portable_pty::Child + Send>,
}

impl PaneState {
    pub fn new(agent_cmd: &str, args: &[&str], cols: u16, rows: u16, cwd: Option<&str>) -> anyhow::Result<Self> {
        let handle = spawn_agent(agent_cmd, args, cols, rows, cwd)?;

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
            agent_cmd: agent_cmd.to_string(),
            name: None,
            snapshot,
            screen,
            master: handle.master,
            writer,
            _child: handle.child,
        })
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.master
            .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .ok();
        self.screen.lock().unwrap().set_size(rows, cols);
    }

    pub fn write_input(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(data).map_err(|e| anyhow::anyhow!("pty write: {e}"))?;
        self.writer.flush().map_err(|e| anyhow::anyhow!("pty flush: {e}"))
    }

    pub fn label(&self) -> String {
        self.name.as_deref().unwrap_or(&self.agent_cmd).to_string()
    }
}
