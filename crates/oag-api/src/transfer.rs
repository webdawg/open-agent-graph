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
    /// Multi-hop relay (spec/31-service-receipts.md Phase 2): how many more
    /// times a server that doesn't have `cid` locally may forward this
    /// request to one of its own configured `[transfer].relay_peers` before
    /// giving up. `0` (the default, via `#[serde(default)]` — an old client
    /// that predates this field) means "never relay," not "relay forever."
    /// Each hop that relays decrements this and re-signs an entirely new
    /// request as *its own* client of the next hop — this is never
    /// forwarded verbatim, so it carries no multi-party signature structure,
    /// consistent with "each hop here is independent" in the original spec.
    #[serde(default)]
    pub relay_hops_remaining: u8,
    /// Peer ids (string form) already visited in this relay chain, oldest
    /// first — lets a relaying server refuse to forward to a peer already
    /// in the chain (a cycle) rather than relying on `relay_hops_remaining`
    /// alone to eventually terminate it.
    #[serde(default)]
    pub relay_path: Vec<String>,
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
    /// Local fetch missed and either no `[transfer].relay_peers` are
    /// configured, every configured one refused/failed, or
    /// `relay_hops_remaining` was already `0` on the incoming request.
    #[error("content not available locally and could not be relayed")]
    RelayFailed,
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
            TransferError::RelayFailed => StatusCode::BAD_GATEWAY,
            TransferError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            TransferError::Graph(_) | TransferError::Internal(_) => {
                tracing::error!(error = %self, "transfer endpoint internal error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, axum::Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}

pub(crate) fn decode_32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok().and_then(|v| v.try_into().ok())
}

pub(crate) fn decode_64(s: &str) -> Option<[u8; 64]> {
    hex::decode(s).ok().and_then(|v| v.try_into().ok())
}

/// Build and sign a fresh request (used by the CLI client, and by a
/// relaying server building a fresh request of its own to the next hop).
/// Takes `requested_at` explicitly, rather than reading the clock itself, so
/// tests can exercise `verify_request`'s clock-skew check deterministically
/// (same reasoning as `oag-sync`'s `presence::build_and_sign`).
#[allow(clippy::too_many_arguments)]
pub fn build_and_sign_request(
    identity: &PeerIdentity,
    cid: &str,
    client_nonce: &[u8; 32],
    requested_at: i64,
    relay_hops_remaining: u8,
    relay_path: &[String],
) -> TransferRequest {
    let unsigned = UnsignedTransferRequest {
        cid: cid.to_string(),
        client_peer_id: identity.peer_id().to_string(),
        client_public_key: hex::encode(identity.verifying_key().to_bytes()),
        client_nonce: hex::encode(client_nonce),
        requested_at,
        relay_hops_remaining,
        relay_path: relay_path.to_vec(),
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
    relayed_via: Option<&PeerId>,
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

    let mut metadata = serde_json::json!({
        "protocol": "oag-transfer-v1",
        "byte_size": byte_size,
        "peer_role": peer_role,
        "counterparty_peer_id": counterparty_peer_id.to_string(),
        "client_nonce": client_nonce_hex,
        "server_nonce": server_nonce_hex,
        "receipt_hash": receipt_hash,
    });
    // Phase 2 (multi-hop relay): present only on the server-side record of a
    // hop that itself had to relay this content from an upstream peer,
    // rather than serving from its own local pin -- lets a later audit pass
    // reconstruct which hops were relays versus origins by following this
    // field across records that share a `content_hash`, without inventing
    // any new signed multi-party chain structure (see spec/31's "Explicitly
    // deferred" section for why that's still deliberately not built).
    if let Some(via) = relayed_via {
        metadata["relayed_via"] = serde_json::Value::String(via.to_string());
    }

    graph
        .add_evidence(
            &auth,
            assertion_id,
            EvidenceInput {
                evidence_type: Some("service_receipt".to_string()),
                content_hash: Some(content_hash.to_string()),
                observed_at: Some(served_at),
                pinned_cid: Some(cid.to_string()),
                metadata: Some(metadata),
                ..Default::default()
            },
        )
        .await?;

    Ok(assertion_id)
}

/// A statically operator-configured upstream peer (`[transfer].relay_peers`
/// in `config.toml`) this peer may forward a cache-miss fetch to. Static,
/// operator-chosen topology, not automatic CID/peer discovery/routing --
/// that's a separate, much bigger, unbuilt problem (see `OPEN_QUESTIONS.md`).
#[derive(Debug, Clone)]
pub struct RelayPeer {
    pub address: String,
    pub peer_id: PeerId,
}

/// The result of successfully fetching and verifying content from another
/// peer, whether that's the CLI's own `oag peer fetch` or a server relaying
/// a cache miss to an upstream peer on a caller's behalf.
pub struct VerifiedFetch {
    pub content: Vec<u8>,
    pub content_hash: String,
    pub receipt_hash: String,
    pub server_peer_id: PeerId,
}

/// Fetch `cid` directly from the peer at `address`, verify its receipt was
/// signed by exactly `expected_server_peer_id` and that the mutual-binding
/// check holds, and record a corroborating `sent_to` assertion in `graph` as
/// the client side (`ExtractionMethod::Verification`). Only writes anything
/// if every check passes. Shared by the CLI's `oag peer fetch` and this
/// crate's own relay-on-cache-miss path below -- both are "be the client of
/// another peer," just triggered differently.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_and_verify(
    http: &reqwest::Client,
    graph: &GraphService,
    local_identity: &PeerIdentity,
    address: &str,
    expected_server_peer_id: &PeerId,
    cid: &str,
    relay_hops_remaining: u8,
    relay_path: &[String],
) -> Result<VerifiedFetch, TransferError> {
    let client_nonce = oag_crypto::random_bytes_32();
    let request = build_and_sign_request(
        local_identity,
        cid,
        &client_nonce,
        now_unix(),
        relay_hops_remaining,
        relay_path,
    );

    let url = format!("{}/oag/transfer/v1/fetch", address.trim_end_matches('/'));
    let response = http
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|e| TransferError::Internal(format!("relay request to {address} failed: {e}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(TransferError::Internal(format!(
            "relay request to {address} answered {status}: {body}"
        )));
    }

    let receipt_header = response
        .headers()
        .get("x-oag-transfer-receipt")
        .ok_or_else(|| TransferError::Internal("response missing x-oag-transfer-receipt header".to_string()))?
        .to_str()
        .map_err(|e| TransferError::Internal(e.to_string()))?
        .to_string();
    let receipt: TransferReceipt =
        serde_json::from_str(&receipt_header).map_err(|e| TransferError::Internal(e.to_string()))?;
    let content = response
        .bytes()
        .await
        .map_err(|e| TransferError::Internal(e.to_string()))?
        .to_vec();

    let content_hash = oag_crypto::blake3_content_hash(&content);
    if content_hash != receipt.unsigned.content_hash {
        return Err(TransferError::ReceiptMismatch);
    }

    let now = now_unix();
    let verified_server_peer_id = verify_receipt(&receipt, cid, &client_nonce, now)?;
    if verified_server_peer_id != *expected_server_peer_id {
        return Err(TransferError::PeerIdKeyMismatch);
    }

    record_transfer(
        graph,
        local_identity,
        &verified_server_peer_id,
        &local_identity.peer_id(),
        cid,
        content.len(),
        &hex::encode(client_nonce),
        &receipt.unsigned.server_nonce,
        &content_hash,
        &receipt.unsigned.receipt_hash,
        "client",
        receipt.unsigned.served_at,
        ExtractionMethod::Verification,
        None,
    )
    .await?;

    Ok(VerifiedFetch {
        content,
        content_hash,
        receipt_hash: receipt.unsigned.receipt_hash,
        server_peer_id: verified_server_peer_id,
    })
}

/// Tries each configured `[transfer].relay_peers` entry in turn (first
/// success wins), skipping any already in `req`'s `relay_path` (cycle
/// prevention) or once `relay_hops_remaining` is exhausted. Returns `None`
/// if relaying isn't possible/permitted or every attempt failed -- the
/// caller decides what error that becomes (no relay configured vs. a
/// genuinely failed relay are reported differently, see `fetch_with_receipt_inner`).
async fn relay_fetch(state: &AppState, identity: &PeerIdentity, req: &TransferRequest) -> Option<VerifiedFetch> {
    if req.unsigned.relay_hops_remaining == 0 || state.relay_peers.is_empty() {
        return None;
    }

    let own_peer_id = identity.peer_id().to_string();
    let mut next_path = req.unsigned.relay_path.clone();
    next_path.push(own_peer_id);

    for relay_peer in state.relay_peers.iter() {
        if next_path.contains(&relay_peer.peer_id.to_string()) {
            continue; // forwarding here would revisit a peer already in this chain
        }
        match fetch_and_verify(
            &state.http_client,
            &state.graph,
            identity,
            &relay_peer.address,
            &relay_peer.peer_id,
            &req.unsigned.cid,
            req.unsigned.relay_hops_remaining - 1,
            &next_path,
        )
        .await
        {
            Ok(verified) => return Some(verified),
            Err(e) => {
                tracing::warn!(relay_peer = %relay_peer.peer_id, error = %e, "relay attempt failed, trying next configured relay peer if any");
                continue;
            }
        }
    }
    None
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

    let identity = state.graph.identity();
    let provider = state.pinning_provider.clone().ok_or(TransferError::ProviderDisabled)?;
    let (content, relayed_via) = match provider.fetch(&req.unsigned.cid).await {
        Ok(bytes) => (bytes, None),
        Err(oag_ipfs::IpfsError::Disabled) => return Err(TransferError::ProviderDisabled),
        Err(local_err) => {
            // Multi-hop relay (spec/31 Phase 2): "the server might be
            // servicing another node server" -- a local cache miss isn't
            // necessarily the end of the road if this peer has its own
            // configured upstream relay peers and the incoming request
            // still has hops left to spend.
            match relay_fetch(&state, identity, &req).await {
                Some(verified) => (verified.content, Some(verified.server_peer_id)),
                None if state.relay_peers.is_empty() || req.unsigned.relay_hops_remaining == 0 => {
                    return Err(TransferError::Ipfs(local_err))
                }
                None => return Err(TransferError::RelayFailed),
            }
        }
    };

    let content_hash = oag_crypto::blake3_content_hash(&content);
    let server_nonce = oag_crypto::random_bytes_32();
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
        relayed_via.as_ref(),
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
        let req = build_and_sign_request(&identity, "bafytestcid", &nonce, 1_000, 0, &[]);
        let (peer_id, verified_nonce) = verify_request(&req, 1_010).unwrap();
        assert_eq!(peer_id, identity.peer_id());
        assert_eq!(verified_nonce, nonce);
    }

    #[test]
    fn request_tampered_cid_fails_verification() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let mut req = build_and_sign_request(&identity, "bafytestcid", &nonce, 1_000, 0, &[]);
        req.unsigned.cid = "bafyotherdid".to_string();
        assert!(matches!(verify_request(&req, 1_010), Err(TransferError::BadSignature)));
    }

    #[test]
    fn request_mismatched_peer_id_and_public_key_is_rejected() {
        let victim = PeerIdentity::generate();
        let attacker = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let mut req = build_and_sign_request(&victim, "bafytestcid", &nonce, 1_000, 0, &[]);
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
        let req = build_and_sign_request(&identity, "bafytestcid", &nonce, 1_000, 0, &[]);
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

