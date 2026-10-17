//! `POST /oag/transfer/v1/fetch` (`spec/31-service-receipts.md`) — a peer
//! fetches pinned content from another peer, both contributing a random
//! nonce so neither side could have fabricated a matching receipt alone.
//! Authenticated by the request's own Ed25519 signature (self-certifying
//! `client_peer_id`/`client_public_key` pairing), same posture as
//! `oag-sync`'s `hello`/`events` endpoints — not gated by the REST API's
//! bearer-token auth layer, which has no natural way to authenticate one
//! peer to another.
//!
//! Deliberately NOT wired into the existing `/ipfs-site/{cid}` bitswap
//! serving path: bitswap's wire protocol is third-party (`rust-ipfs`/
//! libp2p), and this feature does not fork it to inject nonces into
//! individual block transfers. This is a separate, small, fully OAG-owned
//! transfer mechanism for two real OAG peers (not a browser) to use instead.

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use oag_core::{ActorType, ExtractionMethod, Permission};
use oag_crypto::{sign_with_domain, verify_with_domain, PeerId, PeerIdentity, Signature, VerifyingKey};
use oag_graph::{sign_actor_key_proof, AssertInput, AuthContext, EvidenceInput, GraphService};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

pub const TRANSFER_REQUEST_DOMAIN: &str = "OAG:TRANSFER_REQUEST:v1:";
pub const TRANSFER_RECEIPT_DOMAIN: &str = "OAG:TRANSFER_RECEIPT:v1:";
pub const SENT_TO_PREDICATE: &str = "sent_to";

/// Same tolerance as `oag-sync`'s presence heartbeats (`presence.rs`) —
/// generous on purpose, a request/receipt shouldn't be rejected just for
/// taking an unusually long time in flight, only if its claimed timestamp
/// is implausible outright.
const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedTransferRequest {
    pub cid: String,
    pub client_peer_id: String,
    /// Hex-encoded Ed25519 public key — same convention as `oag-sync`'s
    /// `HelloResponse`/presence heartbeats.
    pub client_public_key: String,
    /// Hex-encoded 32 random bytes (`oag_crypto::random_bytes_32`).
    pub client_nonce: String,
    pub requested_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequest {
    #[serde(flatten)]
    pub unsigned: UnsignedTransferRequest,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedTransferReceipt {
    pub cid: String,
    pub server_peer_id: String,
    pub server_public_key: String,
    pub client_nonce: String,
    pub server_nonce: String,
    pub content_hash: String,
    /// `oag_crypto::mutual_receipt_hash(client_nonce, server_nonce,
    /// content_hash)` — the binding neither side could produce alone.
    pub receipt_hash: String,
    pub served_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferReceipt {
    #[serde(flatten)]
    pub unsigned: UnsignedTransferReceipt,
    pub server_signature: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("invalid peer id: {0}")]
    PeerId(#[from] oag_crypto::PeerIdParseError),
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("peer_id does not match the derived hash of public_key")]
    PeerIdKeyMismatch,
    #[error("invalid nonce encoding")]
    InvalidNonce,
    #[error("bad signature encoding")]
    BadSignatureEncoding,
    #[error("signature verification failed")]
    BadSignature,
    #[error("receipt does not match the expected cid/nonce/hash for this exchange")]
    ReceiptMismatch,
    #[error("timestamp is too far from now")]
    ClockSkew,
    #[error("IPFS pinning is disabled on this peer -- set [ipfs].pinning_provider to enable serving")]
    ProviderDisabled,
    #[error("ipfs error: {0}")]
    Ipfs(#[from] oag_ipfs::IpfsError),
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("graph error: {0}")]
    Graph(#[from] oag_graph::GraphError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for TransferError {
    fn into_response(self) -> Response {
        let status = match &self {
            TransferError::PeerId(_)
            | TransferError::InvalidPublicKey
            | TransferError::PeerIdKeyMismatch
            | TransferError::InvalidNonce
            | TransferError::BadSignatureEncoding
            | TransferError::BadSignature
            | TransferError::ReceiptMismatch
            | TransferError::ClockSkew => StatusCode::UNAUTHORIZED,
            TransferError::ProviderDisabled => StatusCode::SERVICE_UNAVAILABLE,
            TransferError::Ipfs(_) => StatusCode::NOT_FOUND,
            TransferError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            TransferError::Graph(_) | TransferError::Internal(_) => {
                tracing::error!(error = %self, "transfer endpoint internal error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, axum::Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}

fn decode_32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok().and_then(|v| v.try_into().ok())
}

fn decode_64(s: &str) -> Option<[u8; 64]> {
    hex::decode(s).ok().and_then(|v| v.try_into().ok())
}

/// Build and sign a fresh request (used by the CLI client). Takes
/// `requested_at` explicitly, rather than reading the clock itself, so
/// tests can exercise `verify_request`'s clock-skew check deterministically
/// (same reasoning as `oag-sync`'s `presence::build_and_sign`).
pub fn build_and_sign_request(
    identity: &PeerIdentity,
    cid: &str,
    client_nonce: &[u8; 32],
    requested_at: i64,
) -> TransferRequest {
    let unsigned = UnsignedTransferRequest {
        cid: cid.to_string(),
        client_peer_id: identity.peer_id().to_string(),
        client_public_key: hex::encode(identity.verifying_key().to_bytes()),
        client_nonce: hex::encode(client_nonce),
        requested_at,
    };
    let canonical = oag_core::canonical_json_bytes(&unsigned).expect("UnsignedTransferRequest always serializes");
    let signature = sign_with_domain(identity.signing_key(), TRANSFER_REQUEST_DOMAIN, &canonical);
    TransferRequest {
        unsigned,
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Verify a request's `(peer_id, public_key)` pairing, signature, and
/// timestamp freshness. Returns the verified client `PeerId` and its raw
/// nonce bytes.
pub fn verify_request(req: &TransferRequest, now: i64) -> Result<(PeerId, [u8; 32]), TransferError> {
    let peer_id: PeerId = req.unsigned.client_peer_id.parse()?;
    let pk_bytes = decode_32(&req.unsigned.client_public_key).ok_or(TransferError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| TransferError::InvalidPublicKey)?;
    if peer_id != PeerId::from_public_key(&verifying_key) {
        return Err(TransferError::PeerIdKeyMismatch);
    }

    let canonical =
        oag_core::canonical_json_bytes(&req.unsigned).map_err(|e| TransferError::Internal(e.to_string()))?;
    let sig_bytes = decode_64(&req.signature).ok_or(TransferError::BadSignatureEncoding)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verify_with_domain(&verifying_key, TRANSFER_REQUEST_DOMAIN, &canonical, &signature)
        .map_err(|_| TransferError::BadSignature)?;

    if (req.unsigned.requested_at - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(TransferError::ClockSkew);
    }

    let client_nonce = decode_32(&req.unsigned.client_nonce).ok_or(TransferError::InvalidNonce)?;
    Ok((peer_id, client_nonce))
}

/// Build and sign a receipt for content just served (used by the server
/// handler below). Takes `served_at` explicitly for the same testability
/// reason as `build_and_sign_request`.
pub fn build_and_sign_receipt(
    identity: &PeerIdentity,
    cid: &str,
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
    content_hash: &str,
    served_at: i64,
) -> TransferReceipt {
    let receipt_hash = oag_crypto::mutual_receipt_hash(client_nonce, server_nonce, content_hash);
    let unsigned = UnsignedTransferReceipt {
        cid: cid.to_string(),
        server_peer_id: identity.peer_id().to_string(),
        server_public_key: hex::encode(identity.verifying_key().to_bytes()),
        client_nonce: hex::encode(client_nonce),
        server_nonce: hex::encode(server_nonce),
        content_hash: content_hash.to_string(),
        receipt_hash,
        served_at,
    };
    let canonical = oag_core::canonical_json_bytes(&unsigned).expect("UnsignedTransferReceipt always serializes");
    let signature = sign_with_domain(identity.signing_key(), TRANSFER_RECEIPT_DOMAIN, &canonical);
    TransferReceipt {
        unsigned,
        server_signature: hex::encode(signature.to_bytes()),
    }
}

/// Verify a receipt's `(peer_id, public_key)` pairing and signature, that
/// its `receipt_hash` really is `mutual_receipt_hash(client_nonce,
/// server_nonce, content_hash)` (the mutual-binding check — this is what a
/// client calls after independently recomputing `content_hash` from the
/// bytes it actually received), and timestamp freshness. `expected_cid` and
/// `expected_client_nonce` bind this specific exchange — a receipt for a
/// different CID or a replayed nonce from an earlier request must not pass.
pub fn verify_receipt(
    receipt: &TransferReceipt,
    expected_cid: &str,
    expected_client_nonce: &[u8; 32],
    now: i64,
) -> Result<PeerId, TransferError> {
    if receipt.unsigned.cid != expected_cid {
        return Err(TransferError::ReceiptMismatch);
    }
    if decode_32(&receipt.unsigned.client_nonce).as_ref() != Some(expected_client_nonce) {
        return Err(TransferError::ReceiptMismatch);
    }

    let peer_id: PeerId = receipt.unsigned.server_peer_id.parse()?;
    let pk_bytes = decode_32(&receipt.unsigned.server_public_key).ok_or(TransferError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| TransferError::InvalidPublicKey)?;
    if peer_id != PeerId::from_public_key(&verifying_key) {
        return Err(TransferError::PeerIdKeyMismatch);
    }

    let canonical =
        oag_core::canonical_json_bytes(&receipt.unsigned).map_err(|e| TransferError::Internal(e.to_string()))?;
    let sig_bytes = decode_64(&receipt.server_signature).ok_or(TransferError::BadSignatureEncoding)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verify_with_domain(&verifying_key, TRANSFER_RECEIPT_DOMAIN, &canonical, &signature)
        .map_err(|_| TransferError::BadSignature)?;

    if (receipt.unsigned.served_at - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(TransferError::ClockSkew);
    }

    let server_nonce = decode_32(&receipt.unsigned.server_nonce).ok_or(TransferError::InvalidNonce)?;
    let expected_receipt_hash =
        oag_crypto::mutual_receipt_hash(expected_client_nonce, &server_nonce, &receipt.unsigned.content_hash);
    if expected_receipt_hash != receipt.unsigned.receipt_hash {
        return Err(TransferError::ReceiptMismatch);
    }

    Ok(peer_id)
}

/// Record a `(peer:<server>, sent_to, peer:<client>)` assertion + evidence
/// in `graph` — the *local* peer's own perspective, whichever side it is.
/// Called once by the server (right after serving) and once by the CLI
/// client (right after independently verifying the receipt) — each writes
/// into its own separate local database; `oag-sync` gossip is what later
/// lets a third party see both sides' corroborating records on the same
/// edge, no new sync mechanism needed. Returns the new assertion's id.
#[allow(clippy::too_many_arguments)]
pub async fn record_transfer(
    graph: &GraphService,
    local_identity: &PeerIdentity,
    server_peer_id: &PeerId,
    client_peer_id: &PeerId,
    cid: &str,
    byte_size: usize,
    client_nonce_hex: &str,
    server_nonce_hex: &str,
    content_hash: &str,
    receipt_hash: &str,
    peer_role: &'static str,
    served_at: i64,
    extraction_method: ExtractionMethod,
) -> Result<oag_core::AssertionId, TransferError> {
    let proof = sign_actor_key_proof(local_identity.signing_key(), ActorType::Peer, None, None)?;
    let actor_id = graph
        .declare_actor(ActorType::Peer, None, None, Some(proof))
        .await?;
    let auth = AuthContext {
        actor_id,
        permissions: vec![Permission::GraphAssert],
    };

    let counterparty_peer_id = if peer_role == "server" {
        client_peer_id
    } else {
        server_peer_id
    };

    let assertion_id = graph
        .assert(
            &auth,
            AssertInput {
                subject: format!("peer:{server_peer_id}"),
                subject_type: Some("peer".to_string()),
                predicate: SENT_TO_PREDICATE.to_string(),
                object: format!("peer:{client_peer_id}"),
                object_type: Some("peer".to_string()),
                evidence: vec![],
                actor_confidence: None,
                observed_at: Some(served_at),
                extraction_method: Some(extraction_method),
            },
        )
        .await?;

    graph
        .add_evidence(
            &auth,
            assertion_id,
            EvidenceInput {
                evidence_type: Some("service_receipt".to_string()),
                content_hash: Some(content_hash.to_string()),
                observed_at: Some(served_at),
                pinned_cid: Some(cid.to_string()),
                metadata: Some(serde_json::json!({
                    "protocol": "oag-transfer-v1",
                    "byte_size": byte_size,
                    "peer_role": peer_role,
                    "counterparty_peer_id": counterparty_peer_id.to_string(),
                    "client_nonce": client_nonce_hex,
                    "server_nonce": server_nonce_hex,
                    "receipt_hash": receipt_hash,
                })),
                ..Default::default()
            },
        )
        .await?;

    Ok(assertion_id)
}

pub async fn fetch_with_receipt(State(state): State<AppState>, Json(req): Json<TransferRequest>) -> Response {
    match fetch_with_receipt_inner(state, req).await {
        Ok(response) => response,
        Err(err) => err.into_response(),
    }
}

async fn fetch_with_receipt_inner(state: AppState, req: TransferRequest) -> Result<Response, TransferError> {
    let now = now_unix();
    let (client_peer_id, client_nonce) = verify_request(&req, now)?;

    if !state.transfer_rate_limiter.check(&client_peer_id.to_string()) {
        return Err(TransferError::RateLimited);
    }

    let provider = state.pinning_provider.clone().ok_or(TransferError::ProviderDisabled)?;
    let content = provider.fetch(&req.unsigned.cid).await.map_err(|e| match e {
        oag_ipfs::IpfsError::Disabled => TransferError::ProviderDisabled,
        other => TransferError::Ipfs(other),
    })?;

    let content_hash = oag_crypto::blake3_content_hash(&content);
    let server_nonce = oag_crypto::random_bytes_32();
    let identity = state.graph.identity();
    let receipt = build_and_sign_receipt(
        identity,
        &req.unsigned.cid,
        &client_nonce,
        &server_nonce,
        &content_hash,
        now,
    );

    record_transfer(
        &state.graph,
        identity,
        &identity.peer_id(),
        &client_peer_id,
        &req.unsigned.cid,
        content.len(),
        &req.unsigned.client_nonce,
        &hex::encode(server_nonce),
        &content_hash,
        &receipt.unsigned.receipt_hash,
        "server",
        receipt.unsigned.served_at,
        ExtractionMethod::Direct,
    )
    .await?;

    let receipt_json = serde_json::to_string(&receipt).map_err(|e| TransferError::Internal(e.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        "x-oag-transfer-receipt",
        HeaderValue::from_str(&receipt_json).map_err(|e| TransferError::Internal(e.to_string()))?,
    );
    Ok((headers, content).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_sign_and_verify_round_trip() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let req = build_and_sign_request(&identity, "bafytestcid", &nonce, 1_000);
        let (peer_id, verified_nonce) = verify_request(&req, 1_010).unwrap();
        assert_eq!(peer_id, identity.peer_id());
        assert_eq!(verified_nonce, nonce);
    }

    #[test]
    fn request_tampered_cid_fails_verification() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let mut req = build_and_sign_request(&identity, "bafytestcid", &nonce, 1_000);
        req.unsigned.cid = "bafyotherdid".to_string();
        assert!(matches!(verify_request(&req, 1_010), Err(TransferError::BadSignature)));
    }

    #[test]
    fn request_mismatched_peer_id_and_public_key_is_rejected() {
        let victim = PeerIdentity::generate();
        let attacker = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let mut req = build_and_sign_request(&victim, "bafytestcid", &nonce, 1_000);
        // Attacker swaps in their own key but keeps the victim's claimed
        // peer_id, re-signing with their own key -- the signature alone
        // would verify fine against the swapped key, so the pairing check
        // is what must catch this (same shape as presence.rs's equivalent
        // regression test).
        req.unsigned.client_public_key = hex::encode(attacker.verifying_key().to_bytes());
        let canonical = oag_core::canonical_json_bytes(&req.unsigned).unwrap();
        let sig = sign_with_domain(attacker.signing_key(), TRANSFER_REQUEST_DOMAIN, &canonical);
        req.signature = hex::encode(sig.to_bytes());
        assert!(matches!(
            verify_request(&req, 1_010),
            Err(TransferError::PeerIdKeyMismatch)
        ));
    }

    #[test]
    fn request_stale_timestamp_is_rejected() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let req = build_and_sign_request(&identity, "bafytestcid", &nonce, 1_000);
        let far_future = 1_000 + MAX_CLOCK_SKEW_SECONDS + 1;
        assert!(matches!(
            verify_request(&req, far_future),
            Err(TransferError::ClockSkew)
        ));
    }

    #[test]
    fn receipt_sign_and_verify_round_trip() {
        let identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let server_nonce = oag_crypto::random_bytes_32();
        let content_hash = oag_crypto::blake3_content_hash(b"hello world");
        let receipt = build_and_sign_receipt(&identity, "bafytestcid", &client_nonce, &server_nonce, &content_hash, 1_000);
        let peer_id = verify_receipt(&receipt, "bafytestcid", &client_nonce, 1_010).unwrap();
        assert_eq!(peer_id, identity.peer_id());
    }

    #[test]
    fn receipt_for_a_different_cid_is_rejected() {
        let identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let server_nonce = oag_crypto::random_bytes_32();
        let content_hash = oag_crypto::blake3_content_hash(b"hello world");
        let receipt = build_and_sign_receipt(&identity, "bafytestcid", &client_nonce, &server_nonce, &content_hash, 1_000);
        assert!(matches!(
            verify_receipt(&receipt, "bafyadifferentcid", &client_nonce, 1_010),
            Err(TransferError::ReceiptMismatch)
        ));
    }

    #[test]
    fn receipt_with_wrong_client_nonce_is_rejected() {
        // Simulates a client checking a receipt against a nonce it didn't
        // actually send -- the whole point of the mutual binding: this
        // client could not have produced a matching receipt_hash for a
        // transfer it wasn't really party to.
        let identity = PeerIdentity::generate();
        let real_client_nonce = oag_crypto::random_bytes_32();
        let server_nonce = oag_crypto::random_bytes_32();
        let content_hash = oag_crypto::blake3_content_hash(b"hello world");
        let receipt = build_and_sign_receipt(
            &identity,
            "bafytestcid",
            &real_client_nonce,
            &server_nonce,
            &content_hash,
            1_000,
        );
        let wrong_nonce = oag_crypto::random_bytes_32();
        assert!(matches!(
            verify_receipt(&receipt, "bafytestcid", &wrong_nonce, 1_010),
            Err(TransferError::ReceiptMismatch)
        ));
    }

    #[test]
    fn receipt_tampered_content_hash_fails_the_mutual_binding_check() {
        let identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let server_nonce = oag_crypto::random_bytes_32();
        let content_hash = oag_crypto::blake3_content_hash(b"hello world");
        let mut receipt =
            build_and_sign_receipt(&identity, "bafytestcid", &client_nonce, &server_nonce, &content_hash, 1_000);
        // A server trying to claim it served different content than it
        // actually signed for -- tampering `content_hash` without
        // re-signing breaks the signature outright.
        receipt.unsigned.content_hash = oag_crypto::blake3_content_hash(b"substituted content");
        assert!(matches!(
            verify_receipt(&receipt, "bafytestcid", &client_nonce, 1_010),
            Err(TransferError::BadSignature)
        ));
    }

    #[test]
    fn receipt_mismatched_peer_id_and_public_key_is_rejected() {
        let victim = PeerIdentity::generate();
        let attacker = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let server_nonce = oag_crypto::random_bytes_32();
        let content_hash = oag_crypto::blake3_content_hash(b"hello world");
        let mut receipt =
            build_and_sign_receipt(&victim, "bafytestcid", &client_nonce, &server_nonce, &content_hash, 1_000);
        receipt.unsigned.server_public_key = hex::encode(attacker.verifying_key().to_bytes());
        let canonical = oag_core::canonical_json_bytes(&receipt.unsigned).unwrap();
        let sig = sign_with_domain(attacker.signing_key(), TRANSFER_RECEIPT_DOMAIN, &canonical);
        receipt.server_signature = hex::encode(sig.to_bytes());
        assert!(matches!(
            verify_receipt(&receipt, "bafytestcid", &client_nonce, 1_010),
            Err(TransferError::PeerIdKeyMismatch)
        ));
    }

    #[test]
    fn receipt_stale_timestamp_is_rejected() {
        let identity = PeerIdentity::generate();
        let client_nonce = oag_crypto::random_bytes_32();
        let server_nonce = oag_crypto::random_bytes_32();
        let content_hash = oag_crypto::blake3_content_hash(b"hello world");
        let receipt =
            build_and_sign_receipt(&identity, "bafytestcid", &client_nonce, &server_nonce, &content_hash, 1_000);
        let far_future = 1_000 + MAX_CLOCK_SKEW_SECONDS + 1;
        assert!(matches!(
            verify_receipt(&receipt, "bafytestcid", &client_nonce, far_future),
            Err(TransferError::ClockSkew)
        ));
    }
}

