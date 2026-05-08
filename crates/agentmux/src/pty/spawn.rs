use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

pub struct PtyHandle {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
}

pub fn spawn_agent(cmd: &str, args: &[&str], cols: u16, rows: u16, cwd: Option<&str>) -> Result<PtyHandle> {
    let pty_system = native_pty_system();

    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(size)
        .context("failed to open PTY")?;

    let mut builder = CommandBuilder::new(cmd);
    for arg in args {
        builder.arg(arg);
    }
    if let Some(dir) = cwd {
        builder.cwd(dir);
    }

    // Set TERM so agent TUIs enable full color/cursor sequences.
    builder.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(builder)
        .context("failed to spawn agent process")?;

    Ok(PtyHandle {
        master: pair.master,
        child,
    })
}
