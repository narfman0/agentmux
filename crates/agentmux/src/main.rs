mod client;
mod config;
mod ipc;
mod layout_ops;
mod pane;
mod pty;
mod server;
mod tui;
mod vt;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agentmux", about = "Terminal multiplexer for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Initial agent to launch (default: claude)
    #[arg(default_value = "claude")]
    agent: String,
}

#[derive(Subcommand)]
enum Commands {
    /// List running sessions
    List,
    /// Attach to a named session
    Attach { name: String },
    /// Kill a named session
    Kill { name: String },
    /// Create a new named session
    New { name: String },
    /// [internal] Run as background server daemon
    #[command(name = "__server", hide = true)]
    Server { session: String, agent: String },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;

    match cli.command {
        // Internal: background server daemon
        Some(Commands::Server { session, agent }) => {
            init_tracing(&session);
            rt.block_on(server::listener::run_server(session, agent))?;
        }

        // Default: launch agentmux (start server if needed, then run TUI client)
        None => {
            let session = "default".to_string();
            ensure_server_running(&session, &cli.agent)?;
            rt.block_on(client::app::run(&cli.agent))?;
        }

        Some(Commands::New { name }) => {
            ensure_server_running(&name, &cli.agent)?;
            rt.block_on(client::app::run(&cli.agent))?;
        }

        Some(Commands::Attach { name: _ }) => {
            rt.block_on(client::app::run(&cli.agent))?;
        }

        Some(Commands::List) => {
            eprintln!("Session list via IPC coming in Phase 3 client.");
        }

        Some(Commands::Kill { name }) => {
            eprintln!("Kill '{name}' via IPC coming in Phase 3 client.");
        }
    }

    Ok(())
}

/// Spawn the background server if the pipe doesn't exist yet.
fn ensure_server_running(session: &str, agent: &str) -> anyhow::Result<()> {
    use crate::ipc::transport::pipe_name;

    let path = pipe_name(session);

    // Check if the server pipe is already listening.
    if std::fs::metadata(&path).is_ok()
        || named_pipe_exists(&path)
    {
        return Ok(());
    }

    // Spawn server in background.
    server::daemon::spawn_server_with_agent(session, agent)?;

    // Give it up to 3s to create the pipe.
    for _ in 0..30 {
        if named_pipe_exists(&path) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // If we can't connect we'll still try — the TUI client handles connection errors.
    Ok(())
}

fn named_pipe_exists(path: &str) -> bool {
    // On Windows, try opening the pipe with CreateFile in a non-blocking way.
    // We use a raw winapi check: if the path exists as a file-like object, the server is up.
    std::fs::OpenOptions::new().read(true).open(path).is_ok()
}

fn init_tracing(session: &str) {
    use tracing_subscriber::{fmt, EnvFilter};
    let log_path = format!("agentmux-{session}.log");
    if let Ok(f) = std::fs::File::create(&log_path) {
        fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(std::sync::Mutex::new(f))
            .init();
    }
}
