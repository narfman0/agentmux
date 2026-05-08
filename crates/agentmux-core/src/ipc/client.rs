use anyhow::Result;
use bytes::BytesMut;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::codec::{Encoder, FramedRead};

use crate::protocol::{ClientMsg, ServerMsg};

use super::{codec::MsgCodec, transport::pipe_name};

pub struct IpcHandle {
    pub cmd_tx: mpsc::UnboundedSender<ClientMsg>,
    pub event_rx: mpsc::UnboundedReceiver<ServerMsg>,
}

pub async fn connect(session_name: &str) -> Result<IpcHandle> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let path = pipe_name(session_name);
    let pipe = ClientOptions::new().open(&path)?;
    let (read_half, mut write_half) = tokio::io::split(pipe);

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ServerMsg>();

    tokio::spawn(async move {
        let mut framed = FramedRead::new(read_half, MsgCodec::<ServerMsg>::new());
        while let Some(result) = framed.next().await {
            match result {
                Ok(msg) => {
                    if event_tx.send(msg).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!("IPC read error: {e}");
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut codec = MsgCodec::<ClientMsg>::new();
        while let Some(msg) = cmd_rx.recv().await {
            let mut buf = BytesMut::new();
            if codec.encode(msg, &mut buf).is_err() {
                break;
            }
            if write_half.write_all(&buf).await.is_err() {
                break;
            }
        }
    });

    Ok(IpcHandle { cmd_tx, event_rx })
}

pub async fn connect_with_retry(session_name: &str) -> Result<IpcHandle> {
    for _ in 0..30 {
        match connect(session_name).await {
            Ok(h) => return Ok(h),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
        }
    }
    connect(session_name).await
}
