use bytes::Bytes;
use portable_pty::MasterPty;
use std::io::Read;
use tokio::sync::mpsc;

/// Bridges portable-pty's blocking reader to a tokio channel.
pub fn spawn_reader(master: &dyn MasterPty) -> anyhow::Result<mpsc::Receiver<Bytes>> {
    let mut reader = master
        .try_clone_reader()
        .map_err(|e| anyhow::anyhow!("failed to clone PTY reader: {e}"))?;

    let (tx, rx) = mpsc::channel::<Bytes>(64);

    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = Bytes::copy_from_slice(&buf[..n]);
                    if tx.blocking_send(data).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(rx)
}
