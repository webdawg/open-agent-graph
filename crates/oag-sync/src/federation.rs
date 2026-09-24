use std::collections::HashSet;

use oag_crypto::PeerId;

/// Which origins' events this peer is willing to store (spec section 59).
/// This is where the federation policy actually bites — not at the
/// transport layer, since every event is self-authenticating regardless of
/// which connection carried it (spec section 60; see plan's "Trust model
/// simplification").
#[derive(Debug, Clone)]
pub enum FederationPolicy {
    Open,
    Allowlist(HashSet<PeerId>),
}

impl FederationPolicy {
    pub fn allows(&self, peer_id: &PeerId) -> bool {
        match self {
            FederationPolicy::Open => true,
            FederationPolicy::Allowlist(set) => set.contains(peer_id),
        }
    }
}
