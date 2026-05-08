/// IPC listener: accepts named pipe connections and handles client sessions.
use std::sync::{Arc, Mutex};

use anyhow::Result;
use futures::StreamExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;

use agentmux_core::protocol::{ClientMsg, ServerMsg};

use crate::config::loader;
use crate::ipc::codec::MsgCodec;
use crate::ipc::transport::{next_server_instance, server_endpoint};
use crate::layout_ops::SplitDir;
use crate::server::session_mgr::ServerState;

pub async fn run_server(session_name: String, initial_agent: String) -> Result<()> {
    let cfg = loader::load();
    let state = Arc::new(Mutex::new(ServerState::new()));

    // Pre-create the first session with the initial agent.
    {
        let mut s = state.lock().unwrap();
        let agent_cfg = cfg.agent.iter().find(|a| a.name == initial_agent || a.command == initial_agent);
        let (cmd, args_owned) = match agent_cfg {
            Some(a) => (a.command.clone(), a.args.clone()),
            None => (initial_agent.clone(), vec![]),
        };
        let args_refs: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
        let session = crate::server::session_mgr::ServerSession::new(
            &session_name,
            &cmd,
            &args_refs,
            220,
            50,
        )?;
        s.sessions.insert(session_name.clone(), session);
    }

    // Accept loop — each connection spawns a handler task; a new pipe instance is
    // created immediately so the next client can connect without waiting.
    let mut server = server_endpoint(&session_name)?;
    loop {
        server.connect().await?;

        // Swap in a fresh instance for the next connection before handing off the current one.
        let connected = std::mem::replace(&mut server, next_server_instance(&session_name)?);

        let state_clone = state.clone();
        let session_name_clone = session_name.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(connected, state_clone, session_name_clone).await {
                tracing::warn!("client handler error: {e}");
            }
        });
    }
}

async fn handle_client(
    pipe: NamedPipeServer,
    state: Arc<Mutex<ServerState>>,
    session_name: String,
) -> Result<()> {
    // Channel for server→client push messages (PaneOutput etc.)
    let (push_tx, mut push_rx) = mpsc::channel::<ServerMsg>(64);

    let mut framed = Framed::new(pipe, MsgCodec::<ClientMsg>::new());

    // Send the current session state immediately on attach.
    {
        let s = state.lock().unwrap();
        if let Some(sess) = s.sessions.get(&session_name) {
            let msg = ServerMsg::Attached { session: sess.session_meta() };
            let _enc = MsgCodec::<ServerMsg>::new();
            // We can't use framed directly here (codec type mismatch) so we
            // serialise + send via push channel instead.
            let _ = push_tx.try_send(msg);

            // Subscribe all existing panes so the client gets output immediately.
            for pane_id in sess.panes.keys().copied().collect::<Vec<_>>() {
                if let Some(snap) = sess.subscribe_pane(pane_id, push_tx.clone()) {
                    let _ = push_tx.try_send(ServerMsg::PaneOutput(
                        agentmux_core::protocol::PaneOutput { pane_id, snapshot: snap },
                    ));
                }
            }
        }
    }

    loop {
        tokio::select! {
            // Inbound from client
            maybe_msg = framed.next() => {
                match maybe_msg {
                    None => break, // client disconnected
                    Some(Err(e)) => {
                        tracing::warn!("framed read error: {e}");
                        break;
                    }
                    Some(Ok(msg)) => {
                        dispatch(msg, &state, &session_name, &push_tx).await;
                    }
                }
            }

            // Outbound push to client
            Some(msg) = push_rx.recv() => {
                // Re-frame using a ResponseCodec — create a temporary bytes buffer.
                use bytes::BytesMut;
                use tokio_util::codec::Encoder;
                let mut buf = BytesMut::new();
                if MsgCodec::<ServerMsg>::new().encode(msg, &mut buf).is_ok() {
                    use tokio::io::AsyncWriteExt;
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
        ClientMsg::BroadcastInput { data, .. } => {
            sess.broadcast_input(&data);
        }
        ClientMsg::ResizePane { pane_id, cols, rows } => {
            sess.resize_pane(pane_id, cols, rows);
        }
        ClientMsg::SplitPane { pane_id, direction, agent } => {
            let dir = match direction {
                agentmux_core::protocol::SplitDir::Horizontal => SplitDir::Horizontal,
                agentmux_core::protocol::SplitDir::Vertical => SplitDir::Vertical,
            };
            if let Ok(new_id) = sess.split_pane(pane_id, dir, &agent, &[], 110, 50) {
                // Subscribe the new pane to this client's push channel
                sess.subscribe_pane(new_id, push_tx.clone());
                let _ = push_tx.try_send(ServerMsg::SessionUpdated {
                    session: sess.session_meta(),
                });
            }
        }
        ClientMsg::ClosePane { pane_id } => {
            sess.close_pane(pane_id);
            let _ = push_tx.try_send(ServerMsg::SessionUpdated {
                session: sess.session_meta(),
            });
        }
        ClientMsg::SubscribePaneOutput { pane_id } => {
            if let Some(snap) = sess.subscribe_pane(pane_id, push_tx.clone()) {
                let _ = push_tx.try_send(ServerMsg::PaneOutput(
                    agentmux_core::protocol::PaneOutput { pane_id, snapshot: snap },
                ));
            }
        }
        ClientMsg::AttachSession { .. } => {
            let _ = push_tx.try_send(ServerMsg::Attached { session: sess.session_meta() });
        }
        ClientMsg::ListSessions => {
            let sessions: Vec<_> = s.sessions.values().map(|s| s.session_meta()).collect();
            let _ = push_tx.try_send(ServerMsg::SessionList { sessions });
        }
        _ => {}
    }
}
