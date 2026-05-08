use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use agentmux_core::protocol::ScreenSnapshot;

use crate::pty::{reader::spawn_reader, spawn::spawn_agent};
use crate::vt::screen::Screen;

pub struct PaneState {
    pub agent_cmd: String,
    pub name: Option<String>,
    /// Latest rendered snapshot — refreshed lazily via `refresh_snapshot()`.
    pub snapshot: ScreenSnapshot,
    screen: Arc<Mutex<Screen>>,
    dirty: Arc<AtomicBool>,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    // Held for its Drop impl — keeps the child process alive while the pane exists.
    _child: Box<dyn portable_pty::Child + Send>,
}

impl PaneState {
    pub fn new(agent_cmd: &str, args: &[&str], cols: u16, rows: u16, cwd: Option<&str>) -> anyhow::Result<Self> {
        let handle = spawn_agent(agent_cmd, args, cols, rows, cwd)?;

        let raw_writer = handle
            .master
            .take_writer()
            .map_err(|e| anyhow::anyhow!("PTY writer: {e}"))?;
        let writer = Arc::new(Mutex::new(raw_writer));
        let rx = spawn_reader(handle.master.as_ref())?;

        let screen = Arc::new(Mutex::new(Screen::new(rows, cols)));
        let dirty = Arc::new(AtomicBool::new(false));

        {
            let screen = screen.clone();
            let dirty = dirty.clone();
            let writer = writer.clone();
            tokio::spawn(async move {
                let mut rx = rx;
                let mut trust_accepted = false;
                while let Some(data) = rx.recv().await {
                    let snap_for_trust = {
                        let mut sc = screen.lock().unwrap();
                        sc.feed(&data);
                        if !trust_accepted { Some(sc.snapshot()) } else { None }
                    };
                    if let Some(snap) = snap_for_trust {
                        if is_trust_prompt(&snap) {
                            trust_accepted = true;
                            let mut w = writer.lock().unwrap();
                            let _ = w.write_all(b"\r");
                            let _ = w.flush();
                        }
                    }
                    dirty.store(true, Ordering::Release);
                }
            });
        }

        Ok(Self {
            agent_cmd: agent_cmd.to_string(),
            name: None,
            snapshot: ScreenSnapshot::blank(cols, rows),
            screen,
            dirty,
            master: handle.master,
            writer,
            _child: handle.child,
        })
    }

    /// Rebuild the snapshot only when the PTY has produced new output since the last call.
    /// No-op when clean, so safe to call unconditionally each render tick.
    pub fn refresh_snapshot(&mut self) {
        if self.dirty.swap(false, Ordering::AcqRel) {
            self.snapshot = self.screen.lock().unwrap().snapshot();
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.master
            .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .ok();
        self.screen.lock().unwrap().set_size(rows, cols);
    }

    pub fn write_input(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(data).map_err(|e| anyhow::anyhow!("pty write: {e}"))?;
        w.flush().map_err(|e| anyhow::anyhow!("pty flush: {e}"))
    }

    pub fn label(&self) -> String {
        self.name.as_deref().unwrap_or(&self.agent_cmd).to_string()
    }
}

/// Detects the Claude Code workspace-trust dialog so we can auto-accept it.
/// Matches "do you trust" or "trust" near "workspace" (case-insensitive, on the VT screen).
fn is_trust_prompt(snap: &ScreenSnapshot) -> bool {
    let text: String = snap.cells.iter()
        .map(|c| c.ch.to_ascii_lowercase())
        .collect();
    text.contains("do you trust") || (text.contains("trust") && text.contains("workspace"))
}
