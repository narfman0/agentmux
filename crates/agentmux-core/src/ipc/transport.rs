/// Windows named-pipe helpers.
/// Pipe name convention: \\.\pipe\agentmux-<session>
use tokio::net::windows::named_pipe;

pub fn pipe_name(session: &str) -> String {
    format!(r"\\.\pipe\agentmux-{}", sanitize(session))
}

fn sanitize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect()
}

pub fn server_endpoint(session: &str) -> std::io::Result<named_pipe::NamedPipeServer> {
    named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .create(pipe_name(session))
}

pub fn next_server_instance(session: &str) -> std::io::Result<named_pipe::NamedPipeServer> {
    named_pipe::ServerOptions::new().create(pipe_name(session))
}
