//! Derives a stable Reticulum identity from an OAG peer's existing Ed25519
//! identity, with no extra file to persist.
//!
//! `reticulum::identity::PrivateIdentity::new_from_name` is a pure,
//! deterministic hash-derivation (confirmed by reading the crate's actual
//! `identity.rs`: it hashes the input string straight into the private key
//! material, no randomness involved) — the same input always yields the
//! same keys. Seeding it from the peer's own *signing key bytes* (private,
//! 0600-permissioned, never transmitted) rather than the public `PeerId`
//! matters: `PeerId` is public knowledge the moment a peer syncs with
//! anyone, so deriving from it would let anyone who has ever seen this
//! peer's `PeerId` reconstruct its exact Reticulum private identity and
//! impersonate it on the mesh.
use oag_crypto::PeerIdentity;
use reticulum::identity::PrivateIdentity;

const DOMAIN: &str = "oag-reticulum-identity-v1";

pub fn derive_reticulum_identity(peer_identity: &PeerIdentity) -> PrivateIdentity {
    let seed = format!("{DOMAIN}:{}", hex::encode(peer_identity.signing_key().to_bytes()));
    PrivateIdentity::new_from_name(&seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_oag_identity_yields_same_reticulum_address() {
        let identity = PeerIdentity::generate();
        let a = derive_reticulum_identity(&identity);
        let b = derive_reticulum_identity(&identity);
        assert_eq!(a.address_hash(), b.address_hash());
    }

    #[test]
    fn different_oag_identities_yield_different_reticulum_addresses() {
        let a = derive_reticulum_identity(&PeerIdentity::generate());
        let b = derive_reticulum_identity(&PeerIdentity::generate());
        assert_ne!(a.address_hash(), b.address_hash());
    }
}
