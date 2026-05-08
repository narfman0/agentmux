/// Windows named-pipe helpers.
///
/// Pipe name convention: \\.\pipe\agentmux-<session>
/// Session names are restricted to [a-zA-Z0-9_-].
use tokio::net::windows::named_pipe;

pub fn pipe_name(session: &str) -> String {
    format!(r"\\.\pipe\agentmux-{}", sanitize(session))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// Create the server end of a named pipe.
pub fn server_endpoint(session: &str) -> std::io::Result<named_pipe::NamedPipeServer> {
    named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .create(pipe_name(session))
}

/// Create an additional listener instance (for accepting subsequent clients).
pub fn next_server_instance(session: &str) -> std::io::Result<named_pipe::NamedPipeServer> {
    named_pipe::ServerOptions::new()
        .create(pipe_name(session))
}

/// Connect to a running server pipe.
pub async fn connect_client(session: &str) -> std::io::Result<named_pipe::NamedPipeClient> {
    // Retry briefly — the server may still be starting.
    let path = pipe_name(session);
    for _ in 0..30 {
        match named_pipe::ClientOptions::new().open(&path) {
            Ok(c) => return Ok(c),
            Err(e) if e.raw_os_error() == Some(2) /* ERROR_FILE_NOT_FOUND */ => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("server pipe not found for session '{session}' after 3 seconds"),
    ))
}
