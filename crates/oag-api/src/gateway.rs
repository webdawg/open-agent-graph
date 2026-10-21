//! A generalized "verified internet gateway" (new feature): peers serve
//! real public-internet protocols (NTP, HTTP, ... more can be added later
//! by adding `GatewayService` variants) but never answer from their own
//! local state. A peer asked to resolve a service either:
//!
//! 1. Answers from its own cache, if it has a *quorum-verified* answer
//!    still within its protocol-appropriate TTL (see `GatewayCache` below)
//!    -- this is deliberately the only thing ever cached: a peer never
//!    caches a raw single-source answer and re-serves it as if verified.
//! 2. Otherwise, if it has its own `[gateway].query_peers` configured, fans
//!    the same query out to all of them (concurrently), and only serves an
//!    answer if a majority agree (`GatewayError::NoQuorum` otherwise) --
//!    *this* is the verification step, and only its result is ever cached.
//! 3. Otherwise, if it's configured as a real upstream origin for this
//!    service (`[gateway.ntp].upstream_servers` / `[gateway.http].
//!    allow_upstream`), performs one real external fetch and returns it as
//!    its own single, honest contribution -- whoever asked it is the one
//!    running the quorum check across however many such peers *they* query.
//! 4. Otherwise relays to one of its own `[gateway].relay_peers` -- the
//!    exact same signed, re-sign-per-hop, cycle-and-depth-guarded relay
//!    shape `transfer.rs`'s Phase 2 multi-hop relay already uses (spec/31),
//!    generalized from "fetch a pinned CID" to "resolve this service
//!    query."
//!
//! A real front-end listener (a UDP NTP server, an HTTP reverse proxy)
//! fronting this for actual outside, non-OAG clients refuses to even start
//! unless enough `query_peers` are configured to meet its own quorum
//! threshold -- structurally guaranteeing step 2 always actually runs for
//! anything a real outside client ever sees, not just by convention.
//!
//! This is also the scaling mechanism behind "absorb a traffic spike
//! without everyone hammering the same real origin": once *any* peer with
//! `query_peers` configured produces a quorum-verified answer, every peer
//! that asks *it* (directly, or transitively through further relaying)
//! gets a cheap, still-honest answer from its cache -- the verified result
//! propagates and multiplies through the network like a cache wave, and
//! which peers end up serving it is simply whichever topology an operator
//! configured (no automatic discovery/routing here, same deliberate scope
//! cut as spec/31-service-receipts.md Phase 2 -- see `OPEN_QUESTIONS.md`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use oag_crypto::{sign_with_domain, verify_with_domain, PeerId, PeerIdentity, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::state::AppState;
use crate::transfer::{decode_32, decode_64, now_unix, RelayPeer};

pub const GATEWAY_REQUEST_DOMAIN: &str = "OAG:GATEWAY_REQUEST:v1:";
pub const GATEWAY_RESPONSE_DOMAIN: &str = "OAG:GATEWAY_RESPONSE:v1:";

/// Same tolerance as `transfer.rs`'s request/receipt freshness check.
const MAX_CLOCK_SKEW_SECONDS: i64 = 300;
const MAX_HTTP_REDIRECTS: u8 = 5;

// ---------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GatewayService {
    Ntp,
    /// GET-only for v1 -- no custom headers, no POST/PUT. A general-purpose
    /// verified HTTP proxy that also forwards arbitrary request semantics
    /// is a much bigger surface (what would "verified" even mean for a
    /// non-idempotent POST?) -- deliberately out of scope, see
    /// `OPEN_QUESTIONS.md`.
    Http { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedGatewayRequest {
    pub service: GatewayService,
    pub requester_peer_id: String,
    pub requester_public_key: String,
    /// Hex-encoded 32 random bytes -- binds a response to this specific
    /// request, same role as `transfer.rs`'s client nonce, even though
    /// there's no content transfer here to bind a mutual receipt hash to.
    pub nonce: String,
    pub requested_at: i64,
    /// Multi-hop relay depth budget, same shape as `transfer.rs`'s
    /// `relay_hops_remaining` (spec/31-service-receipts.md Phase 2).
    #[serde(default)]
    pub hops_remaining: u8,
    /// Peer ids already visited in this relay chain -- cycle prevention,
    /// same shape as `transfer.rs`'s `relay_path`.
    #[serde(default)]
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRequest {
    #[serde(flatten)]
    pub unsigned: UnsignedGatewayRequest,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GatewayResult {
    /// This responder's own best estimate of the current correct time, in
    /// milliseconds since the Unix epoch, at the moment it was computed.
    Ntp { estimated_unix_time_ms: i64 },
    Http {
        status: u16,
        /// `blake3:<hex>` of `body` -- what quorum agreement across
        /// candidates is actually compared on.
        content_hash: String,
        body: Vec<u8>,
        /// The upstream response's own `Cache-Control` header, if any --
        /// read only by whichever peer just ran the quorum step, to decide
        /// its own cache TTL for the aggregated result (see
        /// `compute_cache_ttl`).
        cache_control: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServedFrom {
    Cache,
    Quorum,
    Origin,
    Relay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedGatewayResponse {
    pub result: GatewayResult,
    pub responder_peer_id: String,
    pub responder_public_key: String,
    pub nonce: String,
    pub resolved_at: i64,
    pub served_from: ServedFrom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResponse {
    #[serde(flatten)]
    pub unsigned: UnsignedGatewayResponse,
    pub signature: String,
}

// ---------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
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
    #[error("response does not match the expected nonce for this exchange")]
    ResponseMismatch,
    #[error("timestamp is too far from now")]
    ClockSkew,
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("no quorum: fewer than the required number of query peers agreed")]
    NoQuorum,
    #[error("upstream fetch blocked or failed: {0}")]
    UpstreamBlocked(String),
    #[error("every configured upstream NTP server failed: {0}")]
    NtpUpstream(String),
    #[error("cannot resolve this query: not configured as an origin, and no relay peers available (or hops exhausted)")]
    CannotResolve,
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = match &self {
            GatewayError::PeerId(_)
            | GatewayError::InvalidPublicKey
            | GatewayError::PeerIdKeyMismatch
            | GatewayError::InvalidNonce
            | GatewayError::BadSignatureEncoding
            | GatewayError::BadSignature
            | GatewayError::ResponseMismatch
            | GatewayError::ClockSkew => StatusCode::UNAUTHORIZED,
            GatewayError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            GatewayError::NoQuorum | GatewayError::CannotResolve => StatusCode::BAD_GATEWAY,
            GatewayError::UpstreamBlocked(_) | GatewayError::NtpUpstream(_) => StatusCode::BAD_GATEWAY,
            GatewayError::Internal(_) => {
                tracing::error!(error = %self, "gateway endpoint internal error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}

// ---------------------------------------------------------------------
// Config (operator-facing knobs; `oag-cli` resolves `config.toml` into this)
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub hops_remaining: u8,
    /// `0` means "fan out to every configured `query_peers` entry."
    pub fanout_count: usize,
    pub quorum_min_agree: usize,
    pub ntp_tolerance_ms: i64,
    pub ntp_upstream_servers: Vec<String>,
    pub ntp_cache_ttl_ms: i64,
    pub http_allow_upstream: bool,
    pub http_allow_private_networks: bool,
    pub http_max_response_bytes: usize,
    pub http_default_cache_ttl_ms: i64,
    pub http_max_cache_ttl_ms: i64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            hops_remaining: 8,
            fanout_count: 0,
            quorum_min_agree: 2,
            ntp_tolerance_ms: 1_000,
            ntp_upstream_servers: Vec::new(),
            ntp_cache_ttl_ms: 2_000,
            http_allow_upstream: false,
            http_allow_private_networks: false,
            http_max_response_bytes: 2 * 1024 * 1024,
            http_default_cache_ttl_ms: 5_000,
            http_max_cache_ttl_ms: 60_000,
        }
    }
}

// ---------------------------------------------------------------------
// Cache -- the only place a verified (quorum) answer is ever stored
// ---------------------------------------------------------------------

#[derive(Clone)]
struct CacheEntry {
    result: GatewayResult,
    computed_at: Instant,
    ttl: Duration,
}

/// Keyed by service (NTP has exactly one entry; HTTP is keyed per target
/// URL). Deliberately never written to except by `resolve_gateway_query`'s
/// own quorum branch -- see this module's top-level doc comment.
#[derive(Default)]
pub struct GatewayCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl GatewayCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(service: &GatewayService) -> String {
        match service {
            GatewayService::Ntp => "ntp".to_string(),
            GatewayService::Http { url } => format!("http:{url}"),
        }
    }

    async fn get(&self, service: &GatewayService) -> Option<GatewayResult> {
        let key = Self::key(service);
        let entries = self.entries.read().await;
        let entry = entries.get(&key)?;
        let elapsed = entry.computed_at.elapsed();
        if entry.ttl.is_zero() || elapsed >= entry.ttl {
            return None;
        }
        Some(adjust_for_elapsed(&entry.result, elapsed))
    }

    async fn put(&self, service: &GatewayService, result: GatewayResult, ttl: Duration) {
        if ttl.is_zero() {
            return; // explicit no-store (e.g. upstream sent Cache-Control: no-store)
        }
        let key = Self::key(service);
        let mut entries = self.entries.write().await;
        entries.insert(key, CacheEntry { result, computed_at: Instant::now(), ttl });
    }
}

/// NTP's cached value is really an *offset* (verified time minus local wall
/// clock at the moment it was computed) in disguise -- re-adding how much
/// wall-clock time has elapsed since then reconstructs the correct current
/// estimate without re-running the fan-out. HTTP content doesn't drift the
/// same way, so it's returned as-is.
fn adjust_for_elapsed(result: &GatewayResult, elapsed: Duration) -> GatewayResult {
    match result {
        GatewayResult::Ntp { estimated_unix_time_ms } => {
            GatewayResult::Ntp { estimated_unix_time_ms: estimated_unix_time_ms + elapsed.as_millis() as i64 }
        }
        other => other.clone(),
    }
}

fn compute_cache_ttl(service: &GatewayService, result: &GatewayResult, config: &GatewayConfig) -> Duration {
    match (service, result) {
        (GatewayService::Ntp, _) => Duration::from_millis(config.ntp_cache_ttl_ms.max(0) as u64),
        (GatewayService::Http { .. }, GatewayResult::Http { cache_control, .. }) => {
            if let Some(cc) = cache_control {
                let lower = cc.to_lowercase();
                if lower.contains("no-store") || lower.contains("no-cache") {
                    return Duration::ZERO;
                }
                if let Some(max_age_secs) = parse_max_age(&lower) {
                    let capped_ms = (max_age_secs.saturating_mul(1000)).min(config.http_max_cache_ttl_ms.max(0) as u64);
                    return Duration::from_millis(capped_ms);
                }
            }
            Duration::from_millis(config.http_default_cache_ttl_ms.max(0) as u64)
        }
        _ => Duration::ZERO,
    }
}

fn parse_max_age(cache_control_lower: &str) -> Option<u64> {
    cache_control_lower
        .split(',')
        .map(str::trim)
        .find_map(|directive| directive.strip_prefix("max-age=")?.parse::<u64>().ok())
}

// ---------------------------------------------------------------------
// Sign / verify
// ---------------------------------------------------------------------

pub fn build_and_sign_gateway_request(
    identity: &PeerIdentity,
    service: GatewayService,
    nonce: &[u8; 32],
    requested_at: i64,
    hops_remaining: u8,
    path: &[String],
) -> GatewayRequest {
    let unsigned = UnsignedGatewayRequest {
        service,
        requester_peer_id: identity.peer_id().to_string(),
        requester_public_key: hex::encode(identity.verifying_key().to_bytes()),
        nonce: hex::encode(nonce),
        requested_at,
        hops_remaining,
        path: path.to_vec(),
    };
    let canonical = oag_core::canonical_json_bytes(&unsigned).expect("UnsignedGatewayRequest always serializes");
    let signature = sign_with_domain(identity.signing_key(), GATEWAY_REQUEST_DOMAIN, &canonical);
    GatewayRequest { unsigned, signature: hex::encode(signature.to_bytes()) }
}

pub fn verify_gateway_request(req: &GatewayRequest, now: i64) -> Result<(PeerId, [u8; 32]), GatewayError> {
    let peer_id: PeerId = req.unsigned.requester_peer_id.parse()?;
    let pk_bytes = decode_32(&req.unsigned.requester_public_key).ok_or(GatewayError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| GatewayError::InvalidPublicKey)?;
    if peer_id != PeerId::from_public_key(&verifying_key) {
        return Err(GatewayError::PeerIdKeyMismatch);
    }
    let canonical =
        oag_core::canonical_json_bytes(&req.unsigned).map_err(|e| GatewayError::Internal(e.to_string()))?;
    let sig_bytes = decode_64(&req.signature).ok_or(GatewayError::BadSignatureEncoding)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verify_with_domain(&verifying_key, GATEWAY_REQUEST_DOMAIN, &canonical, &signature)
        .map_err(|_| GatewayError::BadSignature)?;
    if (req.unsigned.requested_at - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(GatewayError::ClockSkew);
    }
    let nonce = decode_32(&req.unsigned.nonce).ok_or(GatewayError::InvalidNonce)?;
    Ok((peer_id, nonce))
}

pub fn build_and_sign_gateway_response(
    identity: &PeerIdentity,
    result: GatewayResult,
    nonce: &[u8; 32],
    resolved_at: i64,
    served_from: ServedFrom,
) -> GatewayResponse {
    let unsigned = UnsignedGatewayResponse {
        result,
        responder_peer_id: identity.peer_id().to_string(),
        responder_public_key: hex::encode(identity.verifying_key().to_bytes()),
        nonce: hex::encode(nonce),
        resolved_at,
        served_from,
    };
    let canonical = oag_core::canonical_json_bytes(&unsigned).expect("UnsignedGatewayResponse always serializes");
    let signature = sign_with_domain(identity.signing_key(), GATEWAY_RESPONSE_DOMAIN, &canonical);
    GatewayResponse { unsigned, signature: hex::encode(signature.to_bytes()) }
}

pub fn verify_gateway_response(
    resp: &GatewayResponse,
    expected_nonce: &[u8; 32],
    now: i64,
) -> Result<PeerId, GatewayError> {
    if decode_32(&resp.unsigned.nonce).as_ref() != Some(expected_nonce) {
        return Err(GatewayError::ResponseMismatch);
    }
    let peer_id: PeerId = resp.unsigned.responder_peer_id.parse()?;
    let pk_bytes = decode_32(&resp.unsigned.responder_public_key).ok_or(GatewayError::InvalidPublicKey)?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| GatewayError::InvalidPublicKey)?;
    if peer_id != PeerId::from_public_key(&verifying_key) {
        return Err(GatewayError::PeerIdKeyMismatch);
    }
    let canonical =
        oag_core::canonical_json_bytes(&resp.unsigned).map_err(|e| GatewayError::Internal(e.to_string()))?;
    let sig_bytes = decode_64(&resp.signature).ok_or(GatewayError::BadSignatureEncoding)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verify_with_domain(&verifying_key, GATEWAY_RESPONSE_DOMAIN, &canonical, &signature)
        .map_err(|_| GatewayError::BadSignature)?;
    if (resp.unsigned.resolved_at - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(GatewayError::ClockSkew);
    }
    Ok(peer_id)
}

// ---------------------------------------------------------------------
// Resolution engine
// ---------------------------------------------------------------------

pub struct ResolvedQuery {
    pub result: GatewayResult,
    pub served_from: ServedFrom,
}

/// The one function every path through this feature funnels through --
/// the real front-end listeners (with a fresh empty `path` and this peer's
/// own `hops_remaining` budget) and the `POST /oag/gateway/v1/query`
/// handler (with `hops_remaining`/`path` taken from the verified incoming
/// request) both call this. See this module's top-level doc comment for
/// the four-step precedence this implements.
pub async fn resolve_gateway_query(
    state: &AppState,
    identity: &PeerIdentity,
    service: &GatewayService,
    hops_remaining: u8,
    path: &[String],
) -> Result<ResolvedQuery, GatewayError> {
    if let Some(result) = state.gateway_cache.get(service).await {
        return Ok(ResolvedQuery { result, served_from: ServedFrom::Cache });
    }

    if !state.gateway_query_peers.is_empty() {
        let verified = fanout_and_verify(state, identity, service).await?;
        let ttl = compute_cache_ttl(service, &verified, &state.gateway_config);
        state.gateway_cache.put(service, verified.clone(), ttl).await;
        return Ok(ResolvedQuery { result: verified, served_from: ServedFrom::Quorum });
    }

    match resolve_directly(state, service).await {
        Ok(Some(result)) => Ok(ResolvedQuery { result, served_from: ServedFrom::Origin }),
        // Not configured as a direct origin for this service, or the
        // direct attempt failed -- try relaying onward, the same
        // any-local-failure-falls-through-to-relay posture
        // `transfer.rs`'s Phase 2 already uses.
        Ok(None) => relay_gateway_query(state, identity, service, hops_remaining, path)
            .await
            .map(|result| ResolvedQuery { result, served_from: ServedFrom::Relay })
            .ok_or(GatewayError::CannotResolve),
        Err(e) => {
            tracing::warn!(error = %e, "gateway: direct origin resolution failed, falling through to relay");
            relay_gateway_query(state, identity, service, hops_remaining, path)
                .await
                .map(|result| ResolvedQuery { result, served_from: ServedFrom::Relay })
                .ok_or(GatewayError::CannotResolve)
        }
    }
}

/// Entry point for a real front-end listener -- always a fresh top-level
/// resolution (empty path, this peer's own configured hop budget).
pub async fn resolve_for_front_end(state: &AppState, service: GatewayService) -> Result<GatewayResult, GatewayError> {
    let identity = state.graph.identity();
    resolve_gateway_query(state, identity, &service, state.gateway_config.hops_remaining, &[]).await.map(|r| r.result)
}

/// A real front-end listener must refuse to start unless enough
/// `query_peers` are configured to actually meet its own quorum threshold
/// -- a structural guarantee that any real outside client always gets a
/// quorum-checked answer, not a convention a misconfiguration could quietly
/// violate.
pub fn require_quorum_capable(state: &AppState) -> Result<(), String> {
    if state.gateway_query_peers.len() < state.gateway_config.quorum_min_agree {
        return Err(format!(
            "[gateway] front-end requires at least {} query_peers to meet quorum_min_agree (configured: {})",
            state.gateway_config.quorum_min_agree,
            state.gateway_query_peers.len()
        ));
    }
    Ok(())
}

async fn resolve_directly(state: &AppState, service: &GatewayService) -> Result<Option<GatewayResult>, GatewayError> {
    match service {
        GatewayService::Ntp => {
            if state.gateway_config.ntp_upstream_servers.is_empty() {
                return Ok(None);
            }
            for server in state.gateway_config.ntp_upstream_servers.iter() {
                match crate::ntp_wire::query_upstream(server).await {
                    Ok(estimated_unix_time_ms) => return Ok(Some(GatewayResult::Ntp { estimated_unix_time_ms })),
                    Err(e) => {
                        tracing::warn!(server, error = %e, "upstream NTP query failed, trying next configured server if any");
                    }
                }
            }
            Err(GatewayError::NtpUpstream("every configured upstream NTP server failed".to_string()))
        }
        GatewayService::Http { url } => {
            if !state.gateway_config.http_allow_upstream {
                return Ok(None);
            }
            fetch_upstream_http(
                url,
                state.gateway_config.http_allow_private_networks,
                state.gateway_config.http_max_response_bytes,
            )
            .await
            .map(Some)
        }
    }
}

/// `oag_crawler::fetch::build_pinned_client`'s repeated
/// `reqwest::ClientBuilder::resolve` calls only keep the *last* pinned
/// address for a given host (it overwrites rather than accumulating a
/// fallback list), so passing every DNS-resolved address through means
/// whichever happens to be last in resolver order wins -- in practice often
/// an IPv6 address a host can resolve but not actually route. Confirmed
/// live: a real sandbox resolved a real Cloudflare IPv6 address for a
/// dual-stack site but had no outbound IPv6 route at all, failing the
/// connection outright with no fallback to the (working) IPv4 address also
/// in the resolved set. Pins to the first IPv4 address when one exists --
/// the bias every real-world dual-stack-broken network needs -- leaving the
/// set untouched (and thus still IPv6-only-capable) when no IPv4 address
/// was returned.
fn prefer_ipv4(addrs: Vec<std::net::SocketAddr>) -> Vec<std::net::SocketAddr> {
    match addrs.iter().position(|a| a.is_ipv4()) {
        Some(pos) => vec![addrs[pos]],
        None => addrs,
    }
}

/// SSRF-guarded, DNS-pinned GET, reusing `oag_crawler::fetch`'s resolve/pin
/// primitives (the exact same safety property `oag-crawler`'s own
/// `safe_fetch` has) but without its narrow content-type allowlist -- a
/// general-purpose gateway must proxy arbitrary content (images, binaries,
/// anything), which a crawler extracting structured facts has no reason to.
async fn fetch_upstream_http(
    url: &str,
    allow_private_networks: bool,
    max_response_bytes: usize,
) -> Result<GatewayResult, GatewayError> {
    let mut current = url::Url::parse(url).map_err(|e| GatewayError::UpstreamBlocked(format!("invalid url: {e}")))?;

    for hop in 0..=MAX_HTTP_REDIRECTS {
        let scheme = current.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(GatewayError::UpstreamBlocked(format!("unsupported scheme '{scheme}'")));
        }
        let host = current.host_str().ok_or_else(|| GatewayError::UpstreamBlocked("no host".to_string()))?.to_string();
        let port = current
            .port_or_known_default()
            .ok_or_else(|| GatewayError::UpstreamBlocked("no port".to_string()))?;

        let addrs = oag_crawler::fetch::resolve_and_validate(&host, port, allow_private_networks)
            .await
            .map_err(|e| GatewayError::UpstreamBlocked(e.to_string()))?;
        let addrs = prefer_ipv4(addrs);
        let client = oag_crawler::fetch::build_pinned_client(
            &host,
            &addrs,
            &oag_crawler::CrawlerConfig {
                allow_private_networks,
                max_response_bytes,
                request_timeout: Duration::from_secs(15),
                user_agent: "oag-gateway/0.1 (+https://github.com/webdawg/open-agent-graph)".to_string(),
            },
        )
        .map_err(|e| GatewayError::UpstreamBlocked(e.to_string()))?;

        let response = client.get(current.clone()).send().await.map_err(|e| GatewayError::UpstreamBlocked(e.to_string()))?;
        let status = response.status();

        if status.is_redirection() {
            if hop == MAX_HTTP_REDIRECTS {
                return Err(GatewayError::UpstreamBlocked("too many redirects".to_string()));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| GatewayError::UpstreamBlocked("redirect without Location".to_string()))?
                .to_string();
            current = current.join(&location).map_err(|_| GatewayError::UpstreamBlocked("invalid redirect location".to_string()))?;
            continue;
        }

        let cache_control =
            response.headers().get(reqwest::header::CACHE_CONTROL).and_then(|v| v.to_str().ok()).map(str::to_string);
        if let Some(len) = response.content_length() {
            if len as usize > max_response_bytes {
                return Err(GatewayError::UpstreamBlocked(format!("response too large ({len} bytes)")));
            }
        }

        let status_code = status.as_u16();
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| GatewayError::UpstreamBlocked(e.to_string()))?;
            body.extend_from_slice(&chunk);
            if body.len() > max_response_bytes {
                return Err(GatewayError::UpstreamBlocked(format!("response exceeded {max_response_bytes} bytes")));
            }
        }

        let content_hash = oag_crypto::blake3_content_hash(&body);
        return Ok(GatewayResult::Http { status: status_code, content_hash, body, cache_control });
    }
    Err(GatewayError::UpstreamBlocked("too many redirects".to_string()))
}

/// Tries each configured `[gateway].relay_peers` entry in turn (first
/// success wins), skipping any already in `path` (cycle prevention) or once
/// `hops_remaining` is exhausted -- identical shape to `transfer.rs`'s
/// `relay_fetch`.
async fn relay_gateway_query(
    state: &AppState,
    identity: &PeerIdentity,
    service: &GatewayService,
    hops_remaining: u8,
    path: &[String],
) -> Option<GatewayResult> {
    if hops_remaining == 0 || state.gateway_relay_peers.is_empty() {
        return None;
    }
    let mut next_path = path.to_vec();
    next_path.push(identity.peer_id().to_string());

    for relay_peer in state.gateway_relay_peers.iter() {
        if next_path.contains(&relay_peer.peer_id.to_string()) {
            continue; // forwarding here would revisit a peer already in this chain
        }
        match query_peer(&state.http_client, identity, relay_peer, service, hops_remaining - 1, &next_path).await {
            Ok(result) => return Some(result),
            Err(e) => {
                tracing::warn!(relay_peer = %relay_peer.peer_id, error = %e, "gateway relay attempt failed, trying next configured relay peer if any");
            }
        }
    }
    None
}

/// Fans `service` out to every (or `fanout_count`) configured
/// `[gateway].query_peers`, concurrently, and requires a majority to agree
/// before returning anything -- the verification step this whole feature
/// exists for.
async fn fanout_and_verify(
    state: &AppState,
    identity: &PeerIdentity,
    service: &GatewayService,
) -> Result<GatewayResult, GatewayError> {
    let peers = state.gateway_query_peers.clone();
    let fanout_n = if state.gateway_config.fanout_count == 0 {
        peers.len()
    } else {
        state.gateway_config.fanout_count.min(peers.len())
    };

    let mut handles = Vec::with_capacity(fanout_n);
    for peer in peers.iter().take(fanout_n) {
        let http = state.http_client.clone();
        let identity = identity.clone();
        let peer = peer.clone();
        let svc = service.clone();
        let hops = state.gateway_config.hops_remaining;
        handles.push(tokio::spawn(async move { query_peer(&http, &identity, &peer, &svc, hops, &[]).await }));
    }
    let mut outcomes = Vec::with_capacity(handles.len());
    for h in handles {
        outcomes.push(h.await.unwrap_or_else(|e| Err(GatewayError::Internal(format!("fan-out task panicked: {e}")))));
    }

    aggregate_quorum(service, outcomes, state.gateway_config.ntp_tolerance_ms, state.gateway_config.quorum_min_agree)
}

/// Asks exactly one peer. For NTP, adjusts the returned estimate by half
/// this hop's own round-trip time -- a simple, deliberately-not-RFC-5905-
/// precise correction for a plain signed-JSON-over-HTTP hop (the *real*
/// four-timestamp NTP algorithm is used where it actually matters: against
/// a real upstream NTP server, in `ntp_wire::query_upstream`).
async fn query_peer(
    http: &reqwest::Client,
    identity: &PeerIdentity,
    peer: &RelayPeer,
    service: &GatewayService,
    hops_remaining: u8,
    path: &[String],
) -> Result<GatewayResult, GatewayError> {
    let nonce = oag_crypto::random_bytes_32();
    let request = build_and_sign_gateway_request(identity, service.clone(), &nonce, now_unix(), hops_remaining, path);

    let url = format!("{}/oag/gateway/v1/query", peer.address.trim_end_matches('/'));
    let sent_at = Instant::now();
    let response = http
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|e| GatewayError::Internal(format!("gateway query to {} failed: {e}", peer.address)))?;
    let rtt = sent_at.elapsed();
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(GatewayError::Internal(format!("gateway query to {} answered {status}: {body}", peer.address)));
    }

    let resp: GatewayResponse = response.json().await.map_err(|e| GatewayError::Internal(e.to_string()))?;
    let now = now_unix();
    let verified_peer = verify_gateway_response(&resp, &nonce, now)?;
    if verified_peer != peer.peer_id {
        return Err(GatewayError::PeerIdKeyMismatch);
    }

    let mut result = resp.unsigned.result;
    if let GatewayResult::Ntp { estimated_unix_time_ms } = &mut result {
        *estimated_unix_time_ms += rtt.as_millis() as i64 / 2;
    }
    Ok(result)
}

fn aggregate_quorum(
    service: &GatewayService,
    outcomes: Vec<Result<GatewayResult, GatewayError>>,
    ntp_tolerance_ms: i64,
    min_agree: usize,
) -> Result<GatewayResult, GatewayError> {
    let oks: Vec<GatewayResult> = outcomes
        .into_iter()
        .filter_map(|r| match r {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(error = %e, "gateway fan-out candidate failed");
                None
            }
        })
        .collect();

    match service {
        GatewayService::Ntp => {
            let values: Vec<i64> = oks
                .iter()
                .filter_map(|r| match r {
                    GatewayResult::Ntp { estimated_unix_time_ms } => Some(*estimated_unix_time_ms),
                    _ => None,
                })
                .collect();
            let mut best: Vec<i64> = Vec::new();
            for &v in &values {
                let cluster: Vec<i64> = values.iter().copied().filter(|&o| (o - v).abs() <= ntp_tolerance_ms).collect();
                if cluster.len() > best.len() {
                    best = cluster;
                }
            }
            if best.len() < min_agree {
                return Err(GatewayError::NoQuorum);
            }
            best.sort_unstable();
            let median = best[best.len() / 2];
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: median })
        }
        GatewayService::Http { .. } => {
            let mut groups: HashMap<String, Vec<GatewayResult>> = HashMap::new();
            for r in oks {
                if let GatewayResult::Http { ref content_hash, .. } = r {
                    groups.entry(content_hash.clone()).or_default().push(r);
                }
            }
            let Some((_, group)) = groups.into_iter().max_by_key(|(_, v)| v.len()) else {
                return Err(GatewayError::NoQuorum);
            };
            if group.len() < min_agree {
                return Err(GatewayError::NoQuorum);
            }
            Ok(group.into_iter().next().expect("group is non-empty"))
        }
    }
}

// ---------------------------------------------------------------------
// REST handler: POST /oag/gateway/v1/query
// ---------------------------------------------------------------------

pub async fn query(State(state): State<AppState>, Json(req): Json<GatewayRequest>) -> Response {
    match query_inner(state, req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn query_inner(state: AppState, req: GatewayRequest) -> Result<GatewayResponse, GatewayError> {
    let now = now_unix();
    let (requester_peer_id, nonce) = verify_gateway_request(&req, now)?;
    if !state.gateway_rate_limiter.check(&requester_peer_id.to_string()) {
        return Err(GatewayError::RateLimited);
    }

    let identity = state.graph.identity();
    let resolved =
        resolve_gateway_query(&state, identity, &req.unsigned.service, req.unsigned.hops_remaining, &req.unsigned.path)
            .await?;
    Ok(build_and_sign_gateway_response(identity, resolved.result, &nonce, now_unix(), resolved.served_from))
}

// ---------------------------------------------------------------------
// HTTP front-end: a real reverse proxy for actual outside HTTP clients.
// Convention: GET /<target-url> -- the request path (plus its own query
// string, reattached) IS the URL to fetch-and-verify. A target URL with
// its own un-encoded '?' before its intended query needs percent-encoding
// to avoid being split early; documented in spec/32.
// ---------------------------------------------------------------------

pub fn http_front_end_router(state: AppState) -> axum::Router {
    axum::Router::new().route("/{*target}", axum::routing::get(http_front_end_handler)).with_state(state)
}

async fn http_front_end_handler(State(state): State<AppState>, Path(target_path): Path<String>, uri: Uri) -> Response {
    let target = match uri.query() {
        Some(q) => format!("{target_path}?{q}"),
        None => target_path,
    };
    match resolve_for_front_end(&state, GatewayService::Http { url: target.clone() }).await {
        Ok(GatewayResult::Http { status, body, .. }) => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            (status_code, body).into_response()
        }
        Ok(GatewayResult::Ntp { .. }) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, target, "gateway: could not resolve a verified HTTP answer");
            e.into_response()
        }
    }
}

// ---------------------------------------------------------------------
// NTP front-end: a real UDP server for actual outside NTP clients.
// ---------------------------------------------------------------------

pub async fn run_ntp_front_end(state: AppState, listen_addr: &str) -> std::io::Result<()> {
    let socket = std::sync::Arc::new(tokio::net::UdpSocket::bind(listen_addr).await?);
    tracing::info!(listen_addr, "gateway: NTP front-end listening");
    let mut buf = [0u8; 512];
    loop {
        let (len, from) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "gateway NTP front-end recv error");
                continue;
            }
        };
        let Some(info) = crate::ntp_wire::parse_client_request(&buf[..len]) else { continue };
        let state = state.clone();
        let socket = socket.clone();
        tokio::spawn(async move {
            match resolve_for_front_end(&state, GatewayService::Ntp).await {
                Ok(GatewayResult::Ntp { estimated_unix_time_ms }) => {
                    let response = crate::ntp_wire::build_server_response(info.transmit_unix_ms, estimated_unix_time_ms);
                    if let Err(e) = socket.send_to(&response, from).await {
                        tracing::warn!(error = %e, "gateway NTP front-end send error");
                    }
                }
                Ok(GatewayResult::Http { .. }) => {}
                Err(e) => {
                    // Deliberately send nothing back -- a real NTP client
                    // that gets no response simply times out and tries
                    // another configured server, the ordinary behavior for
                    // an unreachable server. Never substitute this peer's
                    // own unverified system clock as a fallback.
                    tracing::warn!(error = %e, from = %from, "gateway: no verified NTP answer, not responding");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_sign_and_verify_round_trip() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let req = build_and_sign_gateway_request(&identity, GatewayService::Ntp, &nonce, 1_000, 8, &[]);
        let (peer_id, verified_nonce) = verify_gateway_request(&req, 1_010).unwrap();
        assert_eq!(peer_id, identity.peer_id());
        assert_eq!(verified_nonce, nonce);
    }

    #[test]
    fn request_tampered_service_fails_verification() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let mut req = build_and_sign_gateway_request(&identity, GatewayService::Ntp, &nonce, 1_000, 8, &[]);
        req.unsigned.service = GatewayService::Http { url: "https://example.com".to_string() };
        assert!(matches!(verify_gateway_request(&req, 1_010), Err(GatewayError::BadSignature)));
    }

    #[test]
    fn response_sign_and_verify_round_trip() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let resp = build_and_sign_gateway_response(
            &identity,
            GatewayResult::Ntp { estimated_unix_time_ms: 42 },
            &nonce,
            1_000,
            ServedFrom::Origin,
        );
        let peer_id = verify_gateway_response(&resp, &nonce, 1_010).unwrap();
        assert_eq!(peer_id, identity.peer_id());
    }

    #[test]
    fn response_with_wrong_nonce_is_rejected() {
        let identity = PeerIdentity::generate();
        let nonce = oag_crypto::random_bytes_32();
        let resp = build_and_sign_gateway_response(
            &identity,
            GatewayResult::Ntp { estimated_unix_time_ms: 42 },
            &nonce,
            1_000,
            ServedFrom::Origin,
        );
        let wrong_nonce = oag_crypto::random_bytes_32();
        assert!(matches!(
            verify_gateway_response(&resp, &wrong_nonce, 1_010),
            Err(GatewayError::ResponseMismatch)
        ));
    }

    #[test]
    fn ntp_quorum_clusters_close_values_and_ignores_an_outlier() {
        let outcomes = vec![
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: 1_000_000 }),
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: 1_000_200 }),
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: 1_000_100 }),
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: 9_999_999 }), // wildly off, must be excluded
        ];
        let result = aggregate_quorum(&GatewayService::Ntp, outcomes, 1_000, 2).unwrap();
        match result {
            GatewayResult::Ntp { estimated_unix_time_ms } => {
                assert!((1_000_000..=1_000_200).contains(&estimated_unix_time_ms));
            }
            _ => panic!("expected Ntp result"),
        }
    }

    #[test]
    fn ntp_quorum_fails_when_no_cluster_reaches_min_agree() {
        let outcomes = vec![
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: 1_000_000 }),
            Ok(GatewayResult::Ntp { estimated_unix_time_ms: 5_000_000 }),
        ];
        assert!(matches!(
            aggregate_quorum(&GatewayService::Ntp, outcomes, 1_000, 2),
            Err(GatewayError::NoQuorum)
        ));
    }

    #[test]
    fn http_quorum_requires_matching_content_hash() {
        let agree = GatewayResult::Http {
            status: 200,
            content_hash: "blake3:aaa".to_string(),
            body: b"hello".to_vec(),
            cache_control: None,
        };
        let disagree = GatewayResult::Http {
            status: 200,
            content_hash: "blake3:bbb".to_string(),
            body: b"tampered".to_vec(),
            cache_control: None,
        };
        let outcomes = vec![Ok(agree.clone()), Ok(agree.clone()), Ok(disagree)];
        let result = aggregate_quorum(&GatewayService::Http { url: "https://example.com".to_string() }, outcomes, 0, 2)
            .unwrap();
        assert_eq!(result, agree);
    }

    #[test]
    fn cache_ttl_honors_max_age_capped_by_config_max() {
        let config = GatewayConfig::default();
        let result = GatewayResult::Http {
            status: 200,
            content_hash: "blake3:aaa".to_string(),
            body: vec![],
            cache_control: Some("max-age=999999".to_string()),
        };
        let ttl = compute_cache_ttl(&GatewayService::Http { url: "x".to_string() }, &result, &config);
        assert_eq!(ttl, Duration::from_millis(config.http_max_cache_ttl_ms as u64));
    }

    #[test]
    fn cache_ttl_is_zero_for_no_store() {
        let config = GatewayConfig::default();
        let result = GatewayResult::Http {
            status: 200,
            content_hash: "blake3:aaa".to_string(),
            body: vec![],
            cache_control: Some("no-store".to_string()),
        };
        let ttl = compute_cache_ttl(&GatewayService::Http { url: "x".to_string() }, &result, &config);
        assert_eq!(ttl, Duration::ZERO);
    }

    #[tokio::test]
    async fn cache_reconstructs_elapsed_time_for_ntp_on_read() {
        let cache = GatewayCache::new();
        cache.put(&GatewayService::Ntp, GatewayResult::Ntp { estimated_unix_time_ms: 1_000_000 }, Duration::from_secs(10)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let got = cache.get(&GatewayService::Ntp).await.unwrap();
        match got {
            GatewayResult::Ntp { estimated_unix_time_ms } => assert!(estimated_unix_time_ms >= 1_000_050),
            _ => panic!("expected Ntp"),
        }
    }

    #[tokio::test]
    async fn cache_miss_after_ttl_expires() {
        let cache = GatewayCache::new();
        cache.put(&GatewayService::Ntp, GatewayResult::Ntp { estimated_unix_time_ms: 1_000_000 }, Duration::from_millis(10)).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(cache.get(&GatewayService::Ntp).await.is_none());
    }

    #[test]
    fn prefer_ipv4_picks_the_first_ipv4_address_when_one_exists() {
        let v6: std::net::SocketAddr = "[2606:4700:10::1]:80".parse().unwrap();
        let v4a: std::net::SocketAddr = "172.66.1.1:80".parse().unwrap();
        let v4b: std::net::SocketAddr = "172.66.1.2:80".parse().unwrap();
        let result = prefer_ipv4(vec![v6, v4a, v4b]);
        assert_eq!(result, vec![v4a]);
    }

    #[test]
    fn prefer_ipv4_leaves_an_ipv6_only_set_untouched() {
        let v6a: std::net::SocketAddr = "[2606:4700:10::1]:80".parse().unwrap();
        let v6b: std::net::SocketAddr = "[2606:4700:10::2]:80".parse().unwrap();
        let result = prefer_ipv4(vec![v6a, v6b]);
        assert_eq!(result, vec![v6a, v6b]);
    }

    #[tokio::test]
    async fn cache_put_is_a_no_op_for_zero_ttl() {
        let cache = GatewayCache::new();
        cache.put(&GatewayService::Ntp, GatewayResult::Ntp { estimated_unix_time_ms: 1_000_000 }, Duration::ZERO).await;
        assert!(cache.get(&GatewayService::Ntp).await.is_none());
    }
}
