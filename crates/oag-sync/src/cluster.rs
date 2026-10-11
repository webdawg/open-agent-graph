//! Signed cluster declarations (peer clustering, Phase 1 — spec/26): a
//! peer's own self-declared, signed claim of which cluster it belongs to.
//! Mirrors `presence.rs`'s sign/verify pipeline exactly — canonical JSON
//! (RFC 8785) over the unsigned struct, `sign_with_domain`/`verify_with_domain`
//! with a dedicated domain prefix so a cluster signature can never be
//! replayed as a presence or event signature. Deliberately **not** reusing
//! `presence.rs`'s types/sender despite the similarity: clustering must
//! work for every identity mode, including a permanent one that never
//! opts into Phase 1's heartbeat broadcasting, so it needs its own
//! independent sender rather than being accidentally coupled to that
//! opt-in.
//!
//! **Security note, stated plainly rather than hidden** (see spec/26 and
//! `USER_INPUT_RECORD.md` Entry 6): this is a bare self-assertion with no
//! corroboration and no authorization check, by explicit decision. A
//! cluster name functions as a shared, unauthenticated label — anyone who
//! learns or guesses it can claim the same one.

use oag_crypto::{
    canonical_json_bytes, sign_with_domain, verify_with_domain, PeerId, PeerIdentity, Signature, VerifyingKey,
};
use serde::{Deserialize, Serialize};

pub const CLUSTER_DOMAIN: &str = "OAG:CLUSTER:v1:";

/// Same generous tolerance as `presence::MAX_CLOCK_SKEW_SECONDS`, for the
/// same reason.
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

/// A cluster declaration before signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedClusterDeclaration {
    pub peer_id: String,
    /// Hex-encoded Ed25519 public key — same convention as `HelloResponse`.
    pub public_key: String,
    pub cluster: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterDeclaration {
    #[serde(flatten)]
    pub unsigned: UnsignedClusterDeclaration,
    pub signature: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    #[error("invalid peer id: {0}")]
    PeerId(#[from] oag_crypto::PeerIdParseError),
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("peer_id does not match the derived hash of public_key")]
    PeerIdKeyMismatch,
    #[error("bad signature encoding")]
    BadSignatureEncoding,
    #[error("signature verification failed")]
    BadSignature,
    #[error("declaration timestamp is too far from now")]
    ClockSkew,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Build and sign a fresh declaration (spec: see module doc for the
/// pipeline).
pub fn build_and_sign(
    identity: &PeerIdentity,
    cluster: &str,
    timestamp: i64,
) -> Result<ClusterDeclaration, ClusterError> {
    let unsigned = UnsignedClusterDeclaration {
        peer_id: identity.peer_id().to_string(),
        public_key: hex::encode(identity.verifying_key().to_bytes()),
        cluster: cluster.to_string(),
        timestamp,
    };
    let canonical = canonical_json_bytes(&unsigned)?;
    let signature = sign_with_domain(identity.signing_key(), CLUSTER_DOMAIN, &canonical);
    Ok(ClusterDeclaration {
        unsigned,
        signature: hex::encode(signature.to_bytes()),
    })
}

/// Verify a declaration's `(peer_id, public_key)` pairing, signature, and
/// timestamp freshness; returns the verified `PeerId` on success. Same
/// pairing re-derivation check as `presence::verify`/`discover_peers`,
/// against the same spoofed-pairing risk.
pub fn verify(declaration: &ClusterDeclaration, now: i64) -> Result<PeerId, ClusterError> {
    let peer_id: PeerId = declaration.unsigned.peer_id.parse()?;
    let pk_bytes: [u8; 32] = hex::decode(&declaration.unsigned.public_key)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(ClusterError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| ClusterError::InvalidPublicKey)?;
    if peer_id != PeerId::from_public_key(&verifying_key) {
        return Err(ClusterError::PeerIdKeyMismatch);
    }

    let canonical = canonical_json_bytes(&declaration.unsigned)?;
    let sig_bytes: [u8; 64] = hex::decode(&declaration.signature)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(ClusterError::BadSignatureEncoding)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verify_with_domain(&verifying_key, CLUSTER_DOMAIN, &canonical, &signature)
        .map_err(|_| ClusterError::BadSignature)?;

    if (declaration.unsigned.timestamp - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(ClusterError::ClockSkew);
    }

    Ok(peer_id)
}

/// Periodically broadcasts this peer's own cluster declaration to every
/// known peer address (mirrors `presence::spawn_heartbeat_loop`'s shape).
/// A no-op loop when `service` has no configured cluster -- i.e. for every
/// deployment that hasn't set `[cluster].name`.
pub fn spawn_cluster_announce_loop(
    service: crate::service::SyncService,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            service.broadcast_cluster_declaration().await;
            tokio::time::sleep(interval).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> PeerIdentity {
        PeerIdentity::generate()
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let identity = identity();
        let decl = build_and_sign(&identity, "home-lab", 1_030).unwrap();
        let verified = verify(&decl, 1_031).unwrap();
        assert_eq!(verified, identity.peer_id());
    }

    #[test]
    fn tampered_cluster_name_fails_verification() {
        let identity = identity();
        let mut decl = build_and_sign(&identity, "home-lab", 1_030).unwrap();
        decl.unsigned.cluster = "other-site".to_string();
        assert!(matches!(verify(&decl, 1_031), Err(ClusterError::BadSignature)));
    }

    #[test]
    fn mismatched_peer_id_and_public_key_pairing_is_rejected() {
        let victim = identity();
        let attacker = identity();
        let mut decl = build_and_sign(&victim, "home-lab", 1_030).unwrap();
        // Attacker swaps in their own key but keeps the victim's peer_id and
        // resigns with their own key -- signature itself would verify fine
        // against the swapped key, so the pairing check is what must catch this.
        decl.unsigned.public_key = hex::encode(attacker.verifying_key().to_bytes());
        let canonical = canonical_json_bytes(&decl.unsigned).unwrap();
        let sig = sign_with_domain(attacker.signing_key(), CLUSTER_DOMAIN, &canonical);
        decl.signature = hex::encode(sig.to_bytes());
        assert!(matches!(verify(&decl, 1_031), Err(ClusterError::PeerIdKeyMismatch)));
    }

    #[test]
    fn stale_timestamp_beyond_clock_skew_is_rejected() {
        let identity = identity();
        let decl = build_and_sign(&identity, "home-lab", 1_030).unwrap();
        let far_future = 1_030 + MAX_CLOCK_SKEW_SECONDS + 1;
        assert!(matches!(verify(&decl, far_future), Err(ClusterError::ClockSkew)));
    }
}
