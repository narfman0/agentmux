use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SessionId = Uuid;
pub type WindowId = Uuid;
pub type PaneId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub created_at: u64,
    pub windows: Vec<Window>,
    pub active_win: WindowId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub id: WindowId,
    pub name: String,
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub focused: PaneId,
}

/// Binary tree of splits — same model as tmux/zellij.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Layout {
    Leaf(PaneId),
    /// Left | Right
    HSplit {
        ratio: f32,
        left: Box<Layout>,
        right: Box<Layout>,
    },
    /// Top / Bottom
    VSplit {
        ratio: f32,
        top: Box<Layout>,
        bottom: Box<Layout>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pane {
    pub id: PaneId,
    pub name: String,
    pub agent_name: String,
    pub cols: u16,
    pub rows: u16,
    pub alive: bool,
}
