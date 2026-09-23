use serde::{Deserialize, Serialize};

use crate::payload::EventPayload;

pub const EVENT_ID_DOMAIN: &str = "OAG:EVENT:v1:";
pub const CURRENT_VERSION: u16 = 1;

/// An event before signing (spec section 27).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedEvent {
    pub version: u16,
    pub origin_peer: String,
    pub sequence: u64,
    pub previous_event: Option<String>,
    pub created_at: i64,
    #[serde(flatten)]
    pub payload: EventPayload,
}

/// A signed, hashable event (spec sections 27-28). `event_id` is *not*
/// stored on this struct — it's derived from the canonical bytes of the
/// whole signed envelope and computed once at construction time (see
/// [`crate::builder::build_and_sign`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEvent {
    #[serde(flatten)]
    pub unsigned: UnsignedEvent,
    pub signature: String,
}
