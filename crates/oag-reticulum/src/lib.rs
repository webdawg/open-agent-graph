pub mod client;
pub mod framing;
pub mod handlers;
pub mod identity;
pub mod listener;
pub mod wire;

pub use client::{sync_with_peer, ReticulumError};
pub use listener::{local_address_hash, run_listener, ReticulumConfig};
pub use reticulum::error::RnsError;
pub use reticulum::hash::AddressHash;

/// Parse a hex-encoded Reticulum destination address, as printed by
/// `oag peer reticulum-address` — a thin wrapper so callers (`oag-cli`)
/// don't need a direct dependency on the `reticulum` crate itself.
///
/// Tolerates `AddressHash`'s own `Display` form (`/deadbeef.../`, slash-
/// delimited) as well as the plain hex `to_hex_string()` form — a user
/// copying an address out of a log line shouldn't have to know which one
/// they're holding.
pub fn parse_address_hash(hex: &str) -> Result<AddressHash, RnsError> {
    AddressHash::new_from_hex_string(hex.trim_matches('/'))
}
