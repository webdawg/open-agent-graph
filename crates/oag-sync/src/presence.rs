//! Signed presence heartbeats (ephemeral peer trust, Phase 1 — PKT Route
//! Server-inspired: a periodic, signed, independently-verifiable statement
//! of "I am still here," not a trust-me claim). Mirrors `oag-events`'
//! `builder.rs`/`validate.rs` sign/verify pipeline exactly — canonical JSON
//! (RFC 8785) over the unsigned struct, `sign_with_domain`/`verify_with_domain`
//! with a dedicated domain prefix so a presence signature can never be
//! replayed as an event signature or vice versa.

use oag_crypto::{
    canonical_json_bytes, sign_with_domain, verify_with_domain, PeerId, PeerIdentity, Signature, VerifyingKey,
};
use serde::{Deserialize, Serialize};

pub const PRESENCE_DOMAIN: &str = "OAG:PRESENCE:v1:";

/// Generous on purpose — the "frozen in ice" latency-tolerance framing
/// already recorded in `future_ephemeral_peer_trust.md` means a heartbeat
/// shouldn't be rejected just because it took an unusually long time in
/// flight, only if its claimed timestamp is implausible outright.
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Online,
    /// Sent once, best-effort, right before an ephemeral-identity peer
    /// shuts down for a restart/upgrade it wants the network to know was
    /// planned (see `trust.rs`'s address-correlated grace window).
    GoingOffline,
}

impl PresenceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PresenceStatus::Online => "online",
            PresenceStatus::GoingOffline => "going_offline",
        }
    }
}

/// A presence heartbeat before signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedPresenceHeartbeat {
    pub peer_id: String,
    /// Hex-encoded Ed25519 public key — same convention as `HelloResponse`.
    pub public_key: String,
    pub status: PresenceStatus,
    /// When *this identity's current lifetime* began (set once at process
    /// startup, repeated on every heartbeat) — this, not `timestamp`, is
    /// what lets a receiver compute continuous-presence duration.
    pub session_started_at: i64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceHeartbeat {
    #[serde(flatten)]
    pub unsigned: UnsignedPresenceHeartbeat,
    pub signature: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PresenceError {
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
    #[error("heartbeat timestamp is too far from now")]
    ClockSkew,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Build and sign a fresh heartbeat (spec: see module doc for the pipeline).
pub fn build_and_sign(
    identity: &PeerIdentity,
    status: PresenceStatus,
    session_started_at: i64,
    timestamp: i64,
) -> Result<PresenceHeartbeat, PresenceError> {
    let unsigned = UnsignedPresenceHeartbeat {
        peer_id: identity.peer_id().to_string(),
        public_key: hex::encode(identity.verifying_key().to_bytes()),
        status,
        session_started_at,
        timestamp,
    };
    let canonical = canonical_json_bytes(&unsigned)?;
    let signature = sign_with_domain(identity.signing_key(), PRESENCE_DOMAIN, &canonical);
    Ok(PresenceHeartbeat {
        unsigned,
        signature: hex::encode(signature.to_bytes()),
    })
}

/// Verify a heartbeat's `(peer_id, public_key)` pairing, signature, and
/// timestamp freshness; returns the verified `PeerId` on success. The
/// pairing re-derivation is the exact same check `discover_peers` already
/// applies to a gossiped `(peer_id, public_key)` pair, against the same
/// spoofed-pairing/denial-of-replication risk described there.
pub fn verify(heartbeat: &PresenceHeartbeat, now: i64) -> Result<PeerId, PresenceError> {
    let peer_id: PeerId = heartbeat.unsigned.peer_id.parse()?;
    let pk_bytes: [u8; 32] = hex::decode(&heartbeat.unsigned.public_key)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(PresenceError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| PresenceError::InvalidPublicKey)?;
    if peer_id != PeerId::from_public_key(&verifying_key) {
        return Err(PresenceError::PeerIdKeyMismatch);
    }

    let canonical = canonical_json_bytes(&heartbeat.unsigned)?;
    let sig_bytes: [u8; 64] = hex::decode(&heartbeat.signature)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(PresenceError::BadSignatureEncoding)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verify_with_domain(&verifying_key, PRESENCE_DOMAIN, &canonical, &signature)
        .map_err(|_| PresenceError::BadSignature)?;

    if (heartbeat.unsigned.timestamp - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(PresenceError::ClockSkew);
    }

    Ok(peer_id)
}

/// Periodically broadcasts this peer's own `Online` heartbeat to every
/// known peer address (mirrors `gossip.rs::spawn_gossip_loop`'s shape —
/// one unreachable peer never stops the others or the next tick). A no-op
/// loop (returns immediately each tick without sending anything) when
/// `service` has no heartbeat identity, i.e. for every deployment not
/// running ephemeral mode.
pub fn spawn_heartbeat_loop(
    service: crate::service::SyncService,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            service.broadcast_presence(PresenceStatus::Online).await;
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
        let hb = build_and_sign(&identity, PresenceStatus::Online, 1_000, 1_030).unwrap();
        let verified = verify(&hb, 1_031).unwrap();
        assert_eq!(verified, identity.peer_id());
    }

    #[test]
    fn tampered_status_fails_verification() {
        let identity = identity();
        let mut hb = build_and_sign(&identity, PresenceStatus::Online, 1_000, 1_030).unwrap();
        hb.unsigned.status = PresenceStatus::GoingOffline;
        assert!(matches!(verify(&hb, 1_031), Err(PresenceError::BadSignature)));
    }

    #[test]
    fn mismatched_peer_id_and_public_key_pairing_is_rejected() {
        let victim = identity();
        let attacker = identity();
        let mut hb = build_and_sign(&victim, PresenceStatus::Online, 1_000, 1_030).unwrap();
        // Attacker swaps in their own key but keeps the victim's peer_id and
        // resigns with their own key -- signature itself would verify fine
        // against the swapped key, so the pairing check is what must catch this.
        hb.unsigned.public_key = hex::encode(attacker.verifying_key().to_bytes());
        let canonical = canonical_json_bytes(&hb.unsigned).unwrap();
        let sig = sign_with_domain(attacker.signing_key(), PRESENCE_DOMAIN, &canonical);
        hb.signature = hex::encode(sig.to_bytes());
        assert!(matches!(verify(&hb, 1_031), Err(PresenceError::PeerIdKeyMismatch)));
    }

    #[test]
    fn stale_timestamp_beyond_clock_skew_is_rejected() {
        let identity = identity();
        let hb = build_and_sign(&identity, PresenceStatus::Online, 1_000, 1_030).unwrap();
        let far_future = 1_030 + MAX_CLOCK_SKEW_SECONDS + 1;
        assert!(matches!(verify(&hb, far_future), Err(PresenceError::ClockSkew)));
    }
}
