use std::sync::{Arc, Mutex};

use agentmux_core::ipc::codec::MsgCodec;
use agentmux_core::ipc::transport::{next_server_instance, server_endpoint};
use agentmux_core::protocol::{ClientMsg, PaneOutput, ServerMsg};
use anyhow::Result;
use bytes::BytesMut;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::mpsc;
use tokio_util::codec::{Encoder, Framed};

use crate::layout_ops::SplitDir;
use crate::server::session_mgr::{ServerSession, ServerState};

pub async fn run_server(shutdown_rx: tokio::sync::oneshot::Receiver<()>) -> Result<()> {
    agentmux_core::config::ensure_example_config();
    let cfg = agentmux_core::config::load_config();
    let state = Arc::new(Mutex::new(ServerState::new()));

    // Pre-create default session with the first configured agent (or "claude").
    {
        let mut s = state.lock().unwrap();
        let (cmd, mut args, cwd) = cfg
            .agent
            .first()
            .map(|a| (a.command.clone(), a.args.clone(), a.cwd.clone()))
            .unwrap_or_else(|| ("claude".to_string(), vec![], None));
        if cfg.dangerously_skip_permissions
            && !args.iter().any(|a| a == "--dangerously-skip-permissions")
        {
            args.push("--dangerously-skip-permissions".to_string());
        }
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match ServerSession::new("default", &cmd, &args_refs, 220, 50, cwd.as_deref()) {
            Ok(sess) => { s.sessions.insert("default".to_string(), sess); }
            Err(e) => tracing::warn!("failed to create default session: {e}"),
        }
    }

    let mut server = match server_endpoint("default") {
        Ok(s) => s,
        Err(e) if e.raw_os_error() == Some(5) => {
            // ERROR_ACCESS_DENIED — another agentmuxd instance already owns the pipe.
            tracing::info!("agentmuxd already running (pipe owned by another instance)");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    tokio::pin!(shutdown_rx);

    loop {
        tokio::select! {
            result = server.connect() => {
                result?;
                let connected = std::mem::replace(&mut server, next_server_instance("default")?);
                let state_clone = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(connected, state_clone, "default".to_string()).await {
                        tracing::warn!("client handler error: {e}");
                    }
                });
            }
            _ = &mut shutdown_rx => {
                tracing::info!("agentmuxd server shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_client(
    pipe: NamedPipeServer,
    state: Arc<Mutex<ServerState>>,
    session_name: String,
) -> Result<()> {
    let (push_tx, mut push_rx) = mpsc::channel::<ServerMsg>(64);

    let mut framed = Framed::new(pipe, MsgCodec::<ClientMsg>::new());

    // Send current session state on connect.
    {
        let s = state.lock().unwrap();
        if let Some(sess) = s.sessions.get(&session_name) {
            let _ = push_tx.try_send(ServerMsg::Attached { session: sess.session_meta() });
            for pane_id in sess.panes.keys().copied().collect::<Vec<_>>() {
                if let Some(snap) = sess.subscribe_pane(pane_id, push_tx.clone()) {
                    let _ = push_tx.try_send(ServerMsg::PaneOutput(
                        PaneOutput { pane_id, snapshot: snap },
                    ));
                }
            }
        }
    }

    loop {
        tokio::select! {
            maybe_msg = framed.next() => {
                match maybe_msg {
                    None => break,
                    Some(Err(e)) => { tracing::warn!("framed read error: {e}"); break; }
                    Some(Ok(msg)) => dispatch(msg, &state, &session_name, &push_tx).await,
                }
            }
            Some(msg) = push_rx.recv() => {
                let mut buf = BytesMut::new();
                if MsgCodec::<ServerMsg>::new().encode(msg, &mut buf).is_ok() {
                    if framed.get_mut().write_all(&buf).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn dispatch(
    msg: ClientMsg,
    state: &Arc<Mutex<ServerState>>,
    session_name: &str,
    push_tx: &mpsc::Sender<ServerMsg>,
) {
    let mut s = state.lock().unwrap();
    let Some(sess) = s.sessions.get_mut(session_name) else { return };

    match msg {
        ClientMsg::PaneInput { pane_id, data } => {
            let _ = sess.write_pane(pane_id, &data);
        }
        ClientMsg::ResizePane { pane_id, cols, rows } => {
            sess.resize_pane(pane_id, cols, rows);
        }
        ClientMsg::SplitPane { pane_id, direction, agent } => {
            let dir = match direction {
                agentmux_core::protocol::SplitDir::Horizontal => SplitDir::Horizontal,
                agentmux_core::protocol::SplitDir::Vertical => SplitDir::Vertical,
            };
            if let Ok(new_id) = sess.split_pane(pane_id, dir, &agent, &[], 110, 50, None) {
                sess.subscribe_pane(new_id, push_tx.clone());
                let _ = push_tx.try_send(ServerMsg::SessionUpdated { session: sess.session_meta() });
            }
        }
        ClientMsg::SpawnPane { cmd, args, cwd } => {
            let target_id = sess.panes.keys().next().copied();
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let cwd_str = cwd.as_deref();
            if let Some(target) = target_id {
                if let Ok(new_id) = sess.split_pane(target, SplitDir::Horizontal, &cmd, &args_refs, 110, 50, cwd_str) {
                    sess.subscribe_pane(new_id, push_tx.clone());
                    let _ = push_tx.try_send(ServerMsg::SessionUpdated { session: sess.session_meta() });
                }
            }
        }
        ClientMsg::ClosePane { pane_id } => {
            sess.close_pane(pane_id);
            let _ = push_tx.try_send(ServerMsg::SessionUpdated { session: sess.session_meta() });
        }
        ClientMsg::SubscribePaneOutput { pane_id } => {
            if let Some(snap) = sess.subscribe_pane(pane_id, push_tx.clone()) {
                let _ = push_tx.try_send(ServerMsg::PaneOutput(PaneOutput { pane_id, snapshot: snap }));
            }
        }
        ClientMsg::AttachSession { name } => {
            // Re-send current state (client may be re-attaching).
            if name == session_name {
                let _ = push_tx.try_send(ServerMsg::Attached { session: sess.session_meta() });
            }
        }
        ClientMsg::ListSessions => {
            let sessions: Vec<_> = s.sessions.values().map(|s| s.session_meta()).collect();
            let _ = push_tx.try_send(ServerMsg::SessionList { sessions });
        }
        _ => {}
    }
}
