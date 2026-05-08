mod client;
mod tui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agentmux", about = "Terminal multiplexer for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Session name to attach or create (default: "default")
    #[arg(default_value = "default")]
    session: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Attach to a named session
    Attach { name: String },
    /// List running sessions (coming soon)
    List,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;

    let session = match &cli.command {
        None => cli.session.clone(),
        Some(Commands::Attach { name }) => name.clone(),
        Some(Commands::List) => {
            eprintln!("Session list: not yet implemented.");
            return Ok(());
        }
    };

    agentmux_core::config::ensure_example_config();
    ensure_daemon_running()?;
    rt.block_on(client::app::run(&session))?;
    Ok(())
}

fn ensure_daemon_running() -> anyhow::Result<()> {
    use agentmux_core::ipc::transport::pipe_name;

    let path = pipe_name("default");
    if pipe_exists(&path) {
        return Ok(());
    }

    // Spawn agentmuxd from the same directory as this binary.
    let daemon_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("agentmuxd.exe")))
        .unwrap_or_else(|| std::path::PathBuf::from("agentmuxd.exe"));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new(&daemon_exe)
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(&daemon_exe).spawn()?;
    }

    // Wait up to 6 s for the pipe to appear.
    for _ in 0..60 {
        if pipe_exists(&path) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

fn pipe_exists(path: &str) -> bool {
    std::fs::OpenOptions::new().read(true).open(path).is_ok()
}
