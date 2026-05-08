/// Length-delimited bincode framing for IPC messages.
/// Wire format: [u32 LE length][bincode payload]
use std::io;

use bincode::config::standard;
use bytes::{Buf, BufMut, BytesMut};
use serde::{de::DeserializeOwned, Serialize};
use tokio_util::codec::{Decoder, Encoder};

pub struct MsgCodec<T> {
    _marker: std::marker::PhantomData<T>,
}

impl<T> MsgCodec<T> {
    pub fn new() -> Self {
        Self { _marker: std::marker::PhantomData }
    }
}

impl<T: Serialize> Encoder<T> for MsgCodec<T> {
    type Error = io::Error;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> io::Result<()> {
        let payload = bincode::serde::encode_to_vec(&item, standard())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        dst.put_u32_le(payload.len() as u32);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}

impl<T: DeserializeOwned> Decoder for MsgCodec<T> {
    type Item = T;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<T>> {
        if src.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        src.advance(4);
        let payload = src.split_to(len);
        let (msg, _) = bincode::serde::decode_from_slice::<T, _>(&payload, standard())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(msg))
    }
}
