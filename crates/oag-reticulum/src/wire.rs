//! The request/response protocol carried over a Reticulum
//! [`Link`](reticulum::destination::link::Link), framed via [`crate::framing`].
//!
//! Reuses `oag-sync`'s wire DTOs (plain serde structs, zero HTTP coupling)
//! rather than reinventing them, but only the three calls
//! `SyncService::sync_with_peer`'s pull loop actually needs — this first
//! pass doesn't cover `heads`/`post_events` (see the plan's "explicitly
//! deferred" section).
use oag_sync::wire::{EventsResponse, HelloResponse, PeersResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RnRequest {
    Hello,
    Events { origin_hex: String, from: u64, to: u64 },
    Peers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RnResponse {
    Hello(HelloResponse),
    Events(EventsResponse),
    Peers(PeersResponse),
    Error(String),
}
