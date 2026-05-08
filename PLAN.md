# agentmux — Implementation Plan

Windows-native terminal multiplexer for AI coding agent TUIs (Claude Code, OpenCode, Codex CLI).
Keyboard-first, session persistence, no WSL required.

---

## Workspace Structure

```
agentmux/
  Cargo.toml                    ← workspace manifest (TODO: create)
  PLAN.md                       ← this file
  config/
    agents.toml                 ← example agent config
    keybinds.toml               ← example keybind config
  crates/
    agentmux-core/              ← shared: protocol types, data model, config
      Cargo.toml
      src/
        lib.rs
        protocol.rs             ← ClientMsg / ServerMsg IPC enums
        session.rs              ← Session, Window, Pane, Layout types
        config.rs               ← AgentConfig, KeybindDef, AppConfig
        error.rs                ← AgentmuxError enum
    agentmux/                   ← binary: server daemon + TUI client (TODO: create)
      Cargo.toml
      src/
        main.rs                 ← clap dispatch
        server/
          mod.rs
          daemon.rs             ← spawn detached Windows process (DETACHED_PROCESS flag)
          listener.rs           ← accept named pipe connections
          session_mgr.rs        ← CRUD on sessions/panes
          pane_task.rs          ← per-pane: PTY output → vt100 → broadcast snapshots
        client/
          mod.rs
          app.rs                ← ratatui render loop (30 fps)
          connection.rs         ← connect to named pipe, framed IPC reader/writer
          input.rs              ← prefix-key state machine → Action enum
          renderer.rs           ← pane grid layout + PaneViewWidget
        ipc/
          mod.rs
          codec.rs              ← LengthDelimitedCodec + bincode framing
          transport.rs          ← Windows named pipe wrappers
        pty/
          mod.rs
          spawn.rs              ← portable-pty PtyPair + child spawn
          reader.rs             ← spawn_blocking bridge: sync pty → tokio channel
        vt/
          mod.rs
          screen.rs             ← vt100::Parser wrapper → ScreenSnapshot
        tui/
          mod.rs
          widgets/
            mod.rs
            pane_view.rs        ← renders ScreenSnapshot into ratatui Buffer
            status_bar.rs       ← bottom bar: session, panes, broadcast indicator
            cmd_prompt.rs       ← Ctrl+a : command overlay
```

---

## Step 0 — Fix Workspace Setup

The `crates/agentmux-core` crate was initialized but it got its own `.git` dir — delete that
and create a proper top-level workspace `Cargo.toml`:

```powershell
# Remove stray .git in sub-crate
Remove-Item -Recurse -Force C:\workspace\agentmux\crates\agentmux-core\.git

# Init git at the workspace root
cd C:\workspace\agentmux
git init

# Create agentmux binary crate
cargo new --bin crates/agentmux
```

Top-level `Cargo.toml` (workspace manifest — create this manually):

```toml
[workspace]
members = ["crates/agentmux-core", "crates/agentmux"]
resolver = "2"
```

---

## Crate Dependencies

### `crates/agentmux-core/Cargo.toml`

```toml
[package]
name = "agentmux-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
uuid = { version = "1", features = ["v4", "serde"] }
bytes = "1"
bincode = "2"
thiserror = "1"
directories = "5"
```

### `crates/agentmux/Cargo.toml`

```toml
[package]
name = "agentmux"
version = "0.1.0"
edition = "2021"

[dependencies]
agentmux-core = { path = "../agentmux-core" }

# CLI
clap = { version = "4", features = ["derive"] }

# Async
tokio = { version = "1", features = ["full"] }

# PTY (ConPTY on Windows)
portable-pty = "0.8"

# TUI
ratatui = "0.28"
crossterm = { version = "0.28", features = ["event-stream"] }

# VT100 parser + screen buffer
vt100 = "0.15"

# IPC framing
tokio-util = { version = "0.7", features = ["codec"] }
bincode = "2"
bytes = "1"

# Shared state
parking_lot = "0.12"

# Error handling
anyhow = "1"
thiserror = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Config
serde = { version = "1", features = ["derive"] }
toml = "0.8"
directories = "5"

[target.'cfg(windows)'.dependencies]
# tokio named pipes: tokio::net::windows::named_pipe — no extra crate needed
```

---

## Key Types (implement these first in agentmux-core)

### `session.rs`

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

pub type SessionId = Uuid;
pub type WindowId  = Uuid;
pub type PaneId    = Uuid;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Layout {
    Leaf(PaneId),
    HSplit { ratio: f32, left: Box<Layout>, right: Box<Layout> },
    VSplit { ratio: f32, top: Box<Layout>, bottom: Box<Layout> },
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
```

### `protocol.rs`

```rust
use serde::{Serialize, Deserialize};
use bytes::Bytes;
use crate::session::{Session, SessionId, PaneId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitDir { Horizontal, Vertical }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCode {
    SessionNotFound, PaneNotFound, AgentNotFound, PtySpawnFailed,
}

/// Client → Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    ListSessions,
    NewSession    { name: String },
    AttachSession { name: String },
    DetachSession { session_id: SessionId },
    KillSession   { session_id: SessionId },
    SplitPane     { pane_id: PaneId, direction: SplitDir, agent: String },
    ClosePane     { pane_id: PaneId },
    ResizePane    { pane_id: PaneId, cols: u16, rows: u16 },
    FocusPane     { pane_id: PaneId },
    PaneInput     { pane_id: PaneId, data: Vec<u8> },
    BroadcastInput { session_id: SessionId, data: Vec<u8> },
    SubscribePaneOutput { pane_id: PaneId },
}

/// Server → Client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMsg {
    SessionList     { sessions: Vec<Session> },
    SessionCreated  { session: Session },
    Attached        { session: Session },
    SessionUpdated  { session: Session },
    PaneOutput      { pane_id: PaneId, snapshot: ScreenSnapshot },
    PaneExited      { pane_id: PaneId },
    Error           { code: ErrorCode, message: String },
}

/// VT100 screen snapshot — sent over IPC on every changed frame (~30fps)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cursor_hidden: bool,
    pub cells: Vec<SnapshotCell>,   // row-major, cols*rows entries
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
    Ansi(u8),         // 0-15 standard ANSI
    Palette(u8),      // 0-255 256-color palette
    Rgb(u8, u8, u8),
}
```

### `config.rs`

```rust
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_prefix")]
    pub prefix: String,         // "ctrl+a"
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
    #[serde(default)]
    pub binds: Vec<KeybindDef>,
}

fn default_prefix() -> String { "ctrl+a".to_string() }

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeybindDef {
    pub key: String,
    pub action: String,
}
```

---

## Implementation Order (Phase 1 MVP — single in-process pane, no IPC)

Work entirely in `crates/agentmux/src/` for Phase 1. No server, no IPC yet.

### 1. `pty/spawn.rs`
- `pub fn spawn_agent(cmd: &str, args: &[&str], cols: u16, rows: u16) -> anyhow::Result<PtyHandle>`
- Use `portable_pty::native_pty_system()` → `openpty(PtySize{rows, cols, ..})` → `pair.slave.spawn_command(cmd)`
- `PtyHandle` holds: `master: Box<dyn MasterPty>`, `child: Box<dyn Child>`

### 2. `pty/reader.rs`
- `pub fn spawn_reader(master: Box<dyn MasterPty>) -> tokio::sync::mpsc::Receiver<bytes::Bytes>`
- `tokio::task::spawn_blocking` that calls `master.try_clone_reader()` then loops `reader.read(&mut buf)` → sends `Bytes` over channel
- Channel capacity: 64

### 3. `vt/screen.rs`
- `pub struct Screen { parser: vt100::Parser }`
- `pub fn feed(&mut self, data: &[u8])` → `self.parser.process(data)`
- `pub fn snapshot(&self) -> ScreenSnapshot` → iterate `parser.screen().cells()`, map colors/attrs

### 4. `tui/widgets/pane_view.rs`
- `pub struct PaneView<'a> { snapshot: &'a ScreenSnapshot, focused: bool }`
- Impl `ratatui::widgets::Widget` → iterate cells row/col, write to `buf.get_mut(x, y)`
- Color mapping: `SerColor → ratatui::style::Color`
- Wrap in `ratatui::widgets::Block` with border (bright cyan if focused, grey if not)

### 5. `client/input.rs`
- `pub enum InputState { Normal, WaitingKey, CommandPrompt(String) }`
- `pub enum Action { Quit, PaneInput(Vec<u8>), /* more later */ }`
- `pub fn handle_event(state: &mut InputState, ev: KeyEvent, prefix: KeyCode) -> Option<Action>`
- `fn keyevent_to_bytes(ev: KeyEvent) -> Vec<u8>` — covers printable, ctrl+letter, arrows, F-keys, Enter/Tab/Backspace

### 6. `client/app.rs`
- `pub async fn run(agent_cmd: &str) -> anyhow::Result<()>`
- `crossterm::terminal::enable_raw_mode()`
- Spawn PTY via `pty/spawn.rs`, spawn reader task via `pty/reader.rs`
- Spawn vt feed task: receives from reader channel → `screen.feed(data)`
- `ratatui::Terminal::new(CrosstermBackend::new(stdout))`
- 30fps loop: `tokio::select!` on crossterm `EventStream` and a tick interval
  - On tick: `terminal.draw(|f| { PaneView{snapshot: &screen.snapshot(), focused: true}.render(f.size(), f.buffer_mut()) })`
  - On key event: `handle_event(...)` → if `Action::PaneInput(bytes)` → write bytes to pty master writer
  - On `Action::Quit` → break

### 7. `main.rs` (Phase 1 — minimal)
```rust
fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(client::app::run("claude")).unwrap();
}
```

---

## Key Implementation Gotchas

1. **`portable-pty` is sync** — always bridge with `spawn_blocking`. Never `.await` on its reader directly.
2. **`vt100::Parser` resize** — call `parser.set_size(rows, cols)` on terminal resize events (not recreate — screen state is preserved).
3. **Color mapping from vt100** — `vt100::Color` has variants `Default`, `Idx(u8)`, `Rgb(r,g,b)`. Map to `ratatui::style::Color` in `pane_view.rs`.
4. **Cursor in ratatui** — use `frame.set_cursor_position((x, y))` for the focused pane's cursor. Only call this for the single focused pane.
5. **Windows console mode** — `crossterm::terminal::enable_raw_mode()` must be called before any event reading. Restore on exit via `disable_raw_mode()` + `LeaveAlternateScreen`.
6. **IPC pipe names** — `\\.\pipe\agentmux-<session-name>` on Windows. Session names restricted to `[a-zA-Z0-9_-]`.
7. **Named pipe server** — `tokio::net::windows::named_pipe::ServerOptions::new().create(pipe_name)` in server. Client uses `ClientOptions::new().open(pipe_name)`.
8. **Daemon spawn** — use `std::os::windows::process::CommandExt::creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)`. No `daemonize` crate needed.
9. **Child exit** — monitor via `WaitForSingleObject` in a `spawn_blocking` task; on exit, mark pane dead + notify.
10. **Config path** — `directories::BaseDirs::new().unwrap().config_dir()` → append `agentmux/agents.toml`.

---

## Keybinding Actions (implement in Phase 2+)

```
split_vertical    → SplitPane { direction: Vertical }
split_horizontal  → SplitPane { direction: Horizontal }
focus_left/right/up/down → traverse Layout tree
focus_pane_N      → FocusPane by index
new_pane          → show agent picker overlay
close_pane        → ClosePane
detach            → DetachSession + exit client
toggle_broadcast  → flip BroadcastMode state
command_prompt    → enter CommandPrompt state
rename_pane       → enter rename input
```

---

## Config File Examples

**`%APPDATA%\agentmux\agents.toml`**
```toml
[[agent]]
name = "claude-code"
command = "claude"
args = []

[[agent]]
name = "opencode"
command = "opencode"
args = []
```

**`%APPDATA%\agentmux\keybinds.toml`**
```toml
prefix = "ctrl+a"

[[bind]]
key = "|"
action = "split_vertical"

[[bind]]
key = "-"
action = "split_horizontal"

[[bind]]
key = "h"
action = "focus_left"

[[bind]]
key = "j"
action = "focus_down"

[[bind]]
key = "k"
action = "focus_up"

[[bind]]
key = "l"
action = "focus_right"

[[bind]]
key = "n"
action = "new_pane"

[[bind]]
key = "x"
action = "close_pane"

[[bind]]
key = "d"
action = "detach"

[[bind]]
key = "b"
action = "toggle_broadcast"

[[bind]]
key = ":"
action = "command_prompt"

[[bind]]
key = "1"
action = "focus_pane_1"

[[bind]]
key = "2"
action = "focus_pane_2"

[[bind]]
key = "3"
action = "focus_pane_3"

[[bind]]
key = "q"
action = "quit"
```

---

## CLI Subcommands (implement in Phase 3+)

```
agentmux               # auto: start server if needed, attach default session
agentmux new <name>    # create named session
agentmux attach <name> # attach to running session
agentmux list          # list sessions (prints table, exits)
agentmux kill <name>   # kill session (exits)
agentmux __server      # internal: run server daemon (not for direct use)
```

---

## Phases Summary

| Phase | Scope | Est. |
|-------|-------|------|
| 1 | Single-pane MVP: PTY → vt100 → ratatui, in-process | 2–3 weeks |
| 2 | Multi-pane layout: split/navigate/resize, status bar | 1–2 weeks |
| 3 | Client-server IPC: named pipes, detach/attach | 2–3 weeks |
| 4 | Agent config + broadcast mode + keybinds.toml | 1 week |
| 5 | Polish: screen diffs, scrollback, help overlay, CI, .exe release | 1–2 weeks |
