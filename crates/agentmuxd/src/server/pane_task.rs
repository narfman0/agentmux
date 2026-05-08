/// Per-pane async task: reads PTY output, updates VT100 state, broadcasts snapshots.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agentmux_core::protocol::{PaneOutput, ScreenSnapshot, ServerMsg};
use agentmux_core::session::PaneId;
use tokio::sync::mpsc;

use crate::pty::reader::spawn_reader;
use crate::vt::screen::Screen;

pub type ClientTx = mpsc::Sender<ServerMsg>;

pub struct PaneShared {
    pub snapshot: Arc<Mutex<ScreenSnapshot>>,
    pub subscribers: Arc<Mutex<Vec<ClientTx>>>,
}

impl PaneShared {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(ScreenSnapshot::blank(cols, rows))),
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn subscribe(&self, tx: ClientTx) {
        self.subscribers.lock().unwrap().push(tx);
    }

    pub fn latest_snapshot(&self) -> ScreenSnapshot {
        self.snapshot.lock().unwrap().clone()
    }
}

pub fn spawn(
    pane_id: PaneId,
    master: &dyn portable_pty::MasterPty,
    shared: Arc<PaneShared>,
    cols: u16,
    rows: u16,
) -> anyhow::Result<()> {
    let mut rx = spawn_reader(master)?;

    tokio::spawn(async move {
        let mut screen = Screen::new(rows, cols);
        let mut last_sent = Instant::now();
        let throttle = Duration::from_millis(33);

        while let Some(data) = rx.recv().await {
            screen.feed(&data);
            let snap = screen.snapshot();
            *shared.snapshot.lock().unwrap() = snap.clone();

            if last_sent.elapsed() >= throttle {
                last_sent = Instant::now();
                let msg = ServerMsg::PaneOutput(PaneOutput { pane_id, snapshot: snap });
                let mut subs = shared.subscribers.lock().unwrap();
                subs.retain(|tx| tx.try_send(msg.clone()).is_ok());
            }
        }
    });

    Ok(())
}
