use serde::{Deserialize, Serialize};

use crate::session::{Pane, PaneId, Session, SessionId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCode {
    SessionNotFound,
    PaneNotFound,
    AgentNotFound,
    PtySpawnFailed,
}

/// Client → Server messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    ListSessions,
    NewSession { name: String },
    AttachSession { name: String },
    DetachSession { session_id: SessionId },
    KillSession { session_id: SessionId },
    SplitPane { pane_id: PaneId, direction: SplitDir, agent: String },
    ClosePane { pane_id: PaneId },
    ResizePane { pane_id: PaneId, cols: u16, rows: u16 },
    FocusPane { pane_id: PaneId },
    PaneInput { pane_id: PaneId, data: Vec<u8> },
    BroadcastInput { session_id: SessionId, data: Vec<u8> },
    SubscribePaneOutput { pane_id: PaneId },
    UnsubscribePaneOutput { pane_id: PaneId },
    /// Spawn a new pane with an arbitrary command (server picks split target).
    SpawnPane { cmd: String, args: Vec<String>, cwd: Option<String> },
}

/// Payload for ServerMsg::PaneOutput — extracted so pane_task can construct it directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneOutput {
    pub pane_id: PaneId,
    pub snapshot: ScreenSnapshot,
}

/// Server → Client messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMsg {
    SessionList { sessions: Vec<Session> },
    SessionCreated { session: Session },
    Attached { session: Session },
    SessionUpdated { session: Session },
    PaneOutput(PaneOutput),
    PaneExited { pane_id: PaneId },
    PaneUpdated { pane: Pane },
    Error { code: ErrorCode, message: String },
}

/// A full VT100 screen state snapshot, serialized over IPC on each changed frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cursor_hidden: bool,
    /// Row-major, cols × rows entries.
    pub cells: Vec<SnapshotCell>,
}

impl ScreenSnapshot {
    pub fn blank(cols: u16, rows: u16) -> Self {
        let cell = SnapshotCell {
            ch: ' ',
            fg: SerColor::Default,
            bg: SerColor::Default,
            bold: false,
            italic: false,
            underline: false,
            reverse: false,
        };
        Self {
            cols,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            cursor_hidden: false,
            cells: vec![cell; (cols as usize) * (rows as usize)],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotCell {
    pub ch: char,
    pub fg: SerColor,
    pub bg: SerColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerColor {
    Default,
    Ansi(u8),       // 0-7 standard ANSI, 8-15 bright
    Palette(u8),    // 0-255 256-color palette
    Rgb(u8, u8, u8),
}
