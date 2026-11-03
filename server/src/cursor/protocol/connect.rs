//! Encodes and decodes Connect protocol frames.
use bytes::{BufMut, Bytes, BytesMut};
use prost::Message;
use serde::Serialize;

use crate::{Error, Result};

pub const END_STREAM_FLAG: u8 = 0x02;
const MAX_ENVELOPE_LEN: usize = 32 * 1024 * 1024;

/// Accumulates arbitrary network chunks and yields only complete Connect envelopes.
///
/// Official inference bytes arrive on HTTP/1 chunk boundaries. Those boundaries are not
/// envelope boundaries, so callers must not inspect a chunk's first byte as flags.
#[derive(Default)]
pub struct EnvelopeBuffer {
    pending: BytesMut,
}

impl EnvelopeBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn buffered(&self) -> usize {
        self.pending.len()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Bytes>> {
        self.pending.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while self.pending.len() >= 5 {
            let length = u32::from_be_bytes([
                self.pending[1],
                self.pending[2],
                self.pending[3],
                self.pending[4],
            ]) as usize;
            if length > MAX_ENVELOPE_LEN {
                return Err(Error::Protocol(format!(
                    "Connect envelope length {length} exceeds {MAX_ENVELOPE_LEN}"
                )));
            }
            let total = 5 + length;
            if self.pending.len() < total {
                break;
            }
            frames.push(self.pending.split_to(total).freeze());
        }
        Ok(frames)
    }
}

pub fn is_end_stream_frame(frame: &[u8]) -> bool {
    if frame.len() < 5 || frame[0] & END_STREAM_FLAG == 0 {
        return false;
    }
    let length = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;
    frame.len() == 5 + length
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectCode {
    Canceled,
    InvalidArgument,
    NotFound,
    Unavailable,
    Internal,
}

impl ConnectCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Canceled => "canceled",
            Self::InvalidArgument => "invalid_argument",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConnectErrorDetail {
    #[serde(rename = "type")]
    pub type_name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectStreamError {
    pub code: ConnectCode,
    pub message: String,
    pub details: Vec<ConnectErrorDetail>,
}

#[derive(Serialize)]
struct EndStreamResponse<'a> {
    error: WireError<'a>,
}

#[derive(Serialize)]
struct WireError<'a> {
    code: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    message: &'a str,
    #[serde(skip_serializing_if = "details_are_empty")]
    details: &'a [ConnectErrorDetail],
}

fn details_are_empty(details: &&[ConnectErrorDetail]) -> bool {
    details.is_empty()
}

pub fn encode_message<M: Message>(message: &M) -> Result<Bytes> {
    let len = message.encoded_len();
    let mut output = BytesMut::with_capacity(5 + len);
    output.put_u8(0);
    output.put_u32(len as u32);
    message.encode(&mut output)?;
    Ok(output.freeze())
}

pub fn encode_end_stream() -> Bytes {
    encode_end_stream_payload(b"{}")
}

pub fn encode_error_end_stream(error: &ConnectStreamError) -> Result<Bytes> {
    let payload = serde_json::to_vec(&EndStreamResponse {
        error: WireError {
            code: error.code.as_str(),
            message: &error.message,
            details: &error.details,
        },
    })?;
    Ok(encode_end_stream_payload(&payload))
}

fn encode_end_stream_payload(payload: &[u8]) -> Bytes {
    let mut output = BytesMut::with_capacity(5 + payload.len());
    output.put_u8(END_STREAM_FLAG);
    output.put_u32(payload.len() as u32);
    output.extend_from_slice(payload);
    output.freeze()
}

pub fn decode_unary<M: Message + Default>(body: &[u8]) -> Result<M> {
    if body.len() >= 5 {
        let flags = body[0];
        let length = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
        if flags & END_STREAM_FLAG == 0 && length == body.len() - 5 {
            return Ok(M::decode(&body[5..])?);
        }
    }
    Ok(M::decode(body)?)
}

pub fn decode_frames(mut body: &[u8]) -> Result<Vec<(u8, Bytes)>> {
    let mut frames = Vec::new();
    while !body.is_empty() {
        if body.len() < 5 {
            return Err(Error::Protocol("truncated Connect envelope".into()));
        }
        let flags = body[0];
        let length = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
        body = &body[5..];
        if body.len() < length {
            return Err(Error::Protocol("truncated Connect payload".into()));
        }
        frames.push((flags, Bytes::copy_from_slice(&body[..length])));
        body = &body[length..];
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(flags);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn split_chunks_do_not_emit_until_the_envelope_is_complete() {
        let frame = envelope(0, &[0x11, 0x02, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
        let split = 6;
        let mut buffer = EnvelopeBuffer::new();
        assert!(buffer.push(&frame[..split]).unwrap().is_empty());
        let frames = buffer.push(&frame[split..]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].as_ref(), frame.as_slice());
        assert!(!is_end_stream_frame(&frames[0]));
        assert!(buffer.is_empty());
    }

    #[test]
    fn one_chunk_can_hold_several_envelopes() {
        let first = envelope(0, b"one");
        let second = envelope(0, b"two");
        let mut bytes = first.clone();
        bytes.extend_from_slice(&second);
        let mut buffer = EnvelopeBuffer::new();
        let frames = buffer.push(&bytes).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].as_ref(), first.as_slice());
        assert_eq!(frames[1].as_ref(), second.as_slice());
    }

    #[test]
    fn end_stream_split_across_chunks_stays_one_frame() {
        let frame = envelope(END_STREAM_FLAG, b"{}");
        let mut buffer = EnvelopeBuffer::new();
        assert!(buffer.push(&frame[..1]).unwrap().is_empty());
        assert!(!is_end_stream_frame(&frame[..1]));
        let frames = buffer.push(&frame[1..]).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(is_end_stream_frame(&frames[0]));
    }

    #[test]
    fn rejects_an_envelope_larger_than_the_cap() {
        let mut header = [0_u8; 5];
        let length = (MAX_ENVELOPE_LEN as u32).saturating_add(1);
        header[1..].copy_from_slice(&length.to_be_bytes());
        let mut buffer = EnvelopeBuffer::new();
        assert!(buffer.push(&header).is_err());
    }
}
