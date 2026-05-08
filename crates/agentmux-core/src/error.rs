use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentmuxError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("pane not found")]
    PaneNotFound,
    #[error("agent not configured: {0}")]
    AgentNotFound(String),
    #[error("PTY spawn failed: {0}")]
    PtySpawnFailed(String),
    #[error("IPC error: {0}")]
    Ipc(String),
    #[error("config error: {0}")]
    Config(String),
}
