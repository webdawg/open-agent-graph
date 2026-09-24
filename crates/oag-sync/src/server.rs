use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use oag_crypto::{PeerId, VerifyingKey};
use oag_events::{ingest_remote_event, IngestOutcome};
use oag_storage::repo::{events as events_repo, peers as peers_repo};
use serde::Deserialize;

use crate::rate_limit;
use crate::service::SyncService;
use crate::wire::{EventsResponse, HeadsResponse, HelloResponse, PeerRecord, PeersResponse, SYNC_PROTOCOL, SYNC_VERSION};

/// Maximum a single POST /events body may be, to bound memory/CPU spent on
/// an unsolicited push before any semantic validation happens (spec section
/// 61 — "gigantic evidence payloads" / "disk exhaustion").
const MAX_PUSH_BODY_BYTES: usize = 4 * 1024 * 1024;

/// The `/oag/sync/v1/*` replication API (spec section 67). Deliberately
/// unauthenticated at the HTTP layer — every event is self-authenticating
/// via its own signature (spec section 60), so there is nothing a session-
/// level credential would protect here that the signature checks don't
/// already cover (see plan's "Trust model simplification"). Hardened per
/// spec section 61: a global request-rate cap (`rate_limit`), a whole-body
/// size cap below, and per-handler event-count caps (`get_events`,
/// `post_events`) and peer-record caps (`SyncService::discover_peers`).
pub fn router(service: SyncService) -> Router {
    Router::new()
        .route("/oag/sync/v1/hello", get(hello))
        .route("/oag/sync/v1/heads", get(heads))
        .route("/oag/sync/v1/events/{origin}", get(get_events))
        .route("/oag/sync/v1/events", post(post_events))
        .route("/oag/sync/v1/peers", get(peers))
        .layer(middleware::from_fn_with_state(
            service.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(MAX_PUSH_BODY_BYTES))
        .with_state(service)
}

async fn hello(State(service): State<SyncService>) -> Json<HelloResponse> {
    let heads = service.local_heads().await.unwrap_or_default();
    Json(HelloResponse {
        protocol: SYNC_PROTOCOL.to_string(),
        version: SYNC_VERSION,
        peer_id: service.self_peer_id().to_string(),
        public_key: hex::encode(service.self_public_key()),
        heads,
    })
}

async fn heads(State(service): State<SyncService>) -> Json<HeadsResponse> {
    Json(HeadsResponse {
        heads: service.local_heads().await.unwrap_or_default(),
    })
}

#[derive(Deserialize)]
struct RangeQuery {
    from: u64,
    to: u64,
}

/// Spec section 61 ("event flooding"): even though a query's actual DB cost
/// is bounded by real matching rows (not the requested range width), a
/// single call returning tens of thousands of events in one response is
/// still an unbounded-memory/response-size vector. A well-behaved peer
/// paginates by re-requesting with an advanced `from`; this just forces
/// that rather than trusting the caller to ask for sane ranges.
const MAX_EVENTS_PER_FETCH: i64 = 1000;

async fn get_events(
    State(service): State<SyncService>,
    Path(origin_hex): Path<String>,
    Query(range): Query<RangeQuery>,
) -> Result<Json<EventsResponse>, StatusCode> {
    let origin_bytes = decode_hex32(&origin_hex).ok_or(StatusCode::BAD_REQUEST)?;
    if range.to < range.from {
        return Err(StatusCode::BAD_REQUEST);
    }
    let from = range.from as i64;
    let to = std::cmp::min(range.to as i64, from + MAX_EVENTS_PER_FETCH - 1);
    let mut conn = service
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = events_repo::list_range(&mut conn, &origin_bytes, from, to)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let events = rows
        .into_iter()
        .map(|row| serde_json::from_slice(&row.canonical_payload))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(EventsResponse { events }))
}

/// Spec section 61 ("event flooding" / "signature spam"): `MAX_PUSH_BODY_BYTES`
/// already bounds total request size, but many small events could still add
/// up to an enormous batch of individually-cheap-looking transactions in one
/// call. Cap the count directly too; a legitimate pusher just sends another
/// request for the rest.
const MAX_EVENTS_PER_PUSH: usize = 1000;

/// Accepts a push of events (spec section 67). Kept for protocol
/// completeness — the gossip loop and this milestone's tests use pull as
/// primary (matching the spec's own description of B *receiving* by
/// polling, section 109). An event is only applied if this peer already
/// knows the claimed origin's public key (learned via `hello` or `/peers`);
/// otherwise it's silently skipped rather than erroring the whole batch.
async fn post_events(
    State(service): State<SyncService>,
    Json(body): Json<EventsResponse>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.events.len() > MAX_EVENTS_PER_PUSH {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let mut applied = 0u64;
    let mut skipped = 0u64;
    for signed in body.events {
        let Ok(origin_peer_id) = signed.unsigned.origin_peer.parse::<PeerId>() else {
            skipped += 1;
            continue;
        };
        if !service.federation_allows(&origin_peer_id) {
            skipped += 1;
            continue;
        }
        let verifying_key = {
            let Ok(mut conn) = service.pool().acquire().await else {
                skipped += 1;
                continue;
            };
            let key = peers_repo::get_peer(&mut conn, origin_peer_id.as_bytes())
                .await
                .ok()
                .flatten()
                .and_then(|info| VerifyingKey::from_bytes(&info.public_key).ok());
            match key {
                Some(key) => key,
                None => {
                    skipped += 1;
                    continue;
                }
            }
        };
        let received_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        match ingest_remote_event(service.pool(), verifying_key, signed, received_at).await {
            Ok(IngestOutcome::Applied(_, _)) => applied += 1,
            Ok(_) => skipped += 1,
            Err(_) => skipped += 1,
        }
    }
    Ok(Json(serde_json::json!({ "applied": applied, "skipped": skipped })))
}

async fn peers(State(service): State<SyncService>) -> Json<PeersResponse> {
    let Ok(mut conn) = service.pool().acquire().await else {
        return Json(PeersResponse { peers: vec![] });
    };
    let infos = peers_repo::list_peers(&mut conn).await.unwrap_or_default();
    let mut records = Vec::with_capacity(infos.len());
    for info in infos {
        let addresses = peers_repo::list_addresses(&mut conn, &info.peer_id).await.unwrap_or_default();
        records.push(PeerRecord {
            peer_id: PeerId::from_bytes(info.peer_id).to_string(),
            public_key: hex::encode(info.public_key),
            addresses,
            last_seen: info.last_seen,
        });
    }
    Json(PeersResponse { peers: records })
}

fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok()?.try_into().ok()
}
