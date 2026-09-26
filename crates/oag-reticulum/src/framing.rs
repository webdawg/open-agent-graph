//! Manual message framing over a Reticulum [`Link`](reticulum::destination::link::Link).
//!
//! A `Link`'s `data_packet` carries at most `PACKET_MDU` (2048) bytes once
//! encrypted, and our JSON request/response messages (a batch of signed
//! events can run to many KB) routinely exceed that. Rather than depend on
//! the crate's less-documented `Resource` API, frame explicitly: a 4-byte
//! big-endian length prefix followed by the JSON body, split into
//! [`CHUNK_SIZE`]-byte pieces sent as successive `data_packet` calls and
//! reassembled from the pieces on the other end.
use serde::{de::DeserializeOwned, Serialize};

/// Comfortably under `PACKET_MDU` (2048) even after the link's own
/// encryption overhead (Fernet-style AES-CBC + HMAC, well under 600 bytes).
pub const CHUNK_SIZE: usize = 1400;

/// Bounds how much a single logical message can declare itself to be,
/// independent of how many chunks it takes to arrive — the same spirit as
/// `oag-sync`'s `MAX_PUSH_BODY_BYTES`/`MAX_EVENTS_PER_FETCH` (spec section
/// 61): don't let a claimed length turn into an unbounded allocation.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

const LEN_PREFIX_BYTES: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("message of {0} bytes exceeds the {MAX_MESSAGE_BYTES}-byte limit")]
    MessageTooLarge(usize),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Serialize `msg` to JSON and prepend a 4-byte big-endian length. Split the
/// result on [`CHUNK_SIZE`] boundaries (plain `[u8]::chunks`) before handing
/// each piece to `Link::data_packet`.
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>, FramingError> {
    let json = serde_json::to_vec(msg)?;
    if json.len() > MAX_MESSAGE_BYTES {
        return Err(FramingError::MessageTooLarge(json.len()));
    }
    let mut framed = Vec::with_capacity(LEN_PREFIX_BYTES + json.len());
    framed.extend_from_slice(&(json.len() as u32).to_be_bytes());
    framed.extend_from_slice(&json);
    Ok(framed)
}

pub fn decode_message<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, FramingError> {
    Ok(serde_json::from_slice(bytes)?)
}

/// Accumulates chunks received from successive `LinkEvent::Data` events for
/// one link into complete framed messages. One `Reassembler` per link — a
/// link only ever has one message in flight at a time in this protocol
/// (strict request/response), so a single pending-length + buffer is
/// sufficient; a second message's bytes simply queue up after the first is
/// drained by [`Reassembler::push`] returning it.
#[derive(Default)]
pub struct Reassembler {
    buffer: Vec<u8>,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the next chunk. Returns `Ok(Some(message_bytes))` once a full
    /// message (length prefix + declared body length) has arrived; any
    /// bytes belonging to a subsequent message stay buffered for the next
    /// call. Returns `Err` if the declared length exceeds
    /// [`MAX_MESSAGE_BYTES`] — the caller should drop the link's state at
    /// that point rather than keep buffering from a misbehaving peer.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Option<Vec<u8>>, FramingError> {
        self.buffer.extend_from_slice(chunk);

        if self.buffer.len() < LEN_PREFIX_BYTES {
            return Ok(None);
        }
        let declared_len =
            u32::from_be_bytes(self.buffer[..LEN_PREFIX_BYTES].try_into().unwrap()) as usize;
        if declared_len > MAX_MESSAGE_BYTES {
            return Err(FramingError::MessageTooLarge(declared_len));
        }

        let total_len = LEN_PREFIX_BYTES + declared_len;
        if self.buffer.len() < total_len {
            return Ok(None);
        }

        let message = self.buffer[LEN_PREFIX_BYTES..total_len].to_vec();
        self.buffer.drain(..total_len);
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        n: u32,
        text: String,
    }

    #[test]
    fn round_trips_a_single_chunk_message() {
        let msg = Sample { n: 7, text: "hello".into() };
        let framed = encode_message(&msg).unwrap();

        let mut reassembler = Reassembler::new();
        let out = reassembler.push(&framed).unwrap();
        let decoded: Sample = decode_message(&out.unwrap()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn round_trips_a_message_spanning_many_chunks() {
        let msg = Sample { n: 42, text: "x".repeat(10_000) };
        let framed = encode_message(&msg).unwrap();
        assert!(framed.len() > CHUNK_SIZE, "test needs a genuinely multi-chunk message");

        let mut reassembler = Reassembler::new();
        let mut result = None;
        for chunk in framed.chunks(CHUNK_SIZE) {
            if let Some(complete) = reassembler.push(chunk).unwrap() {
                result = Some(complete);
            }
        }
        let decoded: Sample = decode_message(&result.expect("message should have completed")).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn byte_at_a_time_still_reassembles_correctly() {
        let msg = Sample { n: 1, text: "byte-by-byte".into() };
        let framed = encode_message(&msg).unwrap();

        let mut reassembler = Reassembler::new();
        let mut result = None;
        for byte in &framed {
            if let Some(complete) = reassembler.push(std::slice::from_ref(byte)).unwrap() {
                result = Some(complete);
            }
        }
        let decoded: Sample = decode_message(&result.unwrap()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn two_sequential_messages_on_the_same_reassembler_dont_interfere() {
        let a = Sample { n: 1, text: "a".into() };
        let b = Sample { n: 2, text: "b".into() };
        let mut combined = encode_message(&a).unwrap();
        combined.extend_from_slice(&encode_message(&b).unwrap());

        let mut reassembler = Reassembler::new();
        let first = reassembler.push(&combined).unwrap().unwrap();
        assert_eq!(decode_message::<Sample>(&first).unwrap(), a);

        // No further input needed -- the second message's bytes were
        // already buffered from the single `push` above.
        let second = reassembler.push(&[]).unwrap().unwrap();
        assert_eq!(decode_message::<Sample>(&second).unwrap(), b);
    }

    #[test]
    fn oversized_declared_length_is_rejected() {
        let mut fake_header = Vec::new();
        fake_header.extend_from_slice(&((MAX_MESSAGE_BYTES + 1) as u32).to_be_bytes());
        let mut reassembler = Reassembler::new();
        let result = reassembler.push(&fake_header);
        assert!(matches!(result, Err(FramingError::MessageTooLarge(_))), "got {result:?}");
    }
}
