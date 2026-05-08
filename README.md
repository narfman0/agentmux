# agentmux

A Windows-native terminal multiplexer for AI coding agents. Run Claude Code, OpenCode, Codex, or any CLI agent side by side — switch between them instantly, keep sessions alive when you close the window, and broadcast the same prompt to all agents at once.

## How it works

**Dashboard** — the default view shows a live thumbnail preview of every agent. Arrow keys to navigate, Enter to focus one.

**Detail** — full-screen interaction with the selected agent. Escape to return to the dashboard.

```
┌─[1] claude ──────────┐  ┌─[2] opencode ─────────┐
│                      │  │                        │
│  [live preview]      │  │  [live preview]        │
│                      │  │                        │
└──────────────────────┘  └────────────────────────┘
 DASHBOARD  ↑↓←→: navigate  Enter: open  n: new  x: close  b: broadcast  q: quit
```

## Install

### From source (requires Rust 1.85+)

```powershell
git clone https://github.com/narfman0/agentmux
cd agentmux
cargo build --release
# binary at target\release\agentmux.exe
```

## Usage

```powershell
agentmux              # start default session with claude
agentmux opencode     # start with a different agent
agentmux attach       # reattach after closing the window
agentmux list         # list running sessions
agentmux new work     # create a named session
```

## Keyboard shortcuts

### Dashboard mode

| Key | Action |
|-----|--------|
| `↑ ↓ ← →` | Navigate between agent previews |
| `Enter` | Open selected agent (detail mode) |
| `n` | New agent pane (opens picker) |
| `x` | Close selected agent |
| `b` | Toggle broadcast mode |
| `q` / `Esc` | Quit |

### Detail mode

| Key | Action |
|-----|--------|
| `Esc` | Back to dashboard |
| everything else | Sent directly to the agent |

## Session persistence

agentmux runs a background daemon (`agentmux __server`) that keeps all agent processes alive even when you close the terminal window. Open a new terminal and run `agentmux attach` to reconnect.

## Broadcast mode

Press `b` in the dashboard to toggle broadcast mode. While active, keystrokes in detail mode are sent to **all** agent panes simultaneously — useful for running the same prompt across multiple agents and comparing results.

## Configuration

On first run agentmux creates `%APPDATA%\agentmux\agents.toml`:

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

Add any CLI agent here. The `n` key in the dashboard opens a picker with this list.

## Architecture

```
agentmux/
  crates/
    agentmux-core/   # shared protocol types, session model, config
    agentmux/        # binary: TUI client + background server daemon
      client/        # dashboard + detail UI (ratatui + crossterm)
      server/        # named pipe IPC daemon, session/pane management
      pty/           # Windows ConPTY via portable-pty
      vt/            # VT100 screen state (vt100 crate)
      ipc/           # length-delimited bincode framing over named pipes
      tui/widgets/   # dashboard grid, pane view, agent picker
```

Key crates: `portable-pty` (ConPTY), `ratatui` + `crossterm` (TUI), `vt100` (terminal emulation), `tokio` (async), `bincode` + named pipes (IPC).

## License

MIT
