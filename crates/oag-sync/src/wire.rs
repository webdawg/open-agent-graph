use std::collections::HashMap;

use oag_events::SignedEvent;
use serde::{Deserialize, Serialize};

pub const SYNC_PROTOCOL: &str = "oag-sync";
pub const SYNC_VERSION: u16 = 1;

/// `GET /oag/sync/v1/hello` response (spec section 48). `heads` maps each
/// origin peer id (hex-encoded) this peer knows about to that origin's
/// `highest_contiguous_sequence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloResponse {
    pub protocol: String,
    pub version: u16,
    pub peer_id: String,
    /// Hex-encoded Ed25519 public key — lets a fresh peer verify this
    /// peer's own future events without a separate key-discovery step.
    pub public_key: String,
    pub heads: HashMap<String, u64>,
}

/// `GET /oag/sync/v1/heads` response — the cheap-to-poll subset of `hello`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadsResponse {
    pub heads: HashMap<String, u64>,
}

/// `GET /oag/sync/v1/events/{origin}?from=&to=` response and
/// `POST /oag/sync/v1/events` request body (spec section 49).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<SignedEvent>,
}

/// A known peer as advertised by `GET /oag/sync/v1/peers` (spec section 46)
/// — what lets a peer that never contacted an origin directly still learn
/// its public key and fetch its history from a relay (spec section 50).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub peer_id: String,
    pub public_key: String,
    pub addresses: Vec<String>,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeersResponse {
    pub peers: Vec<PeerRecord>,
}
