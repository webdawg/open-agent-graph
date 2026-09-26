//! Request handling for an incoming Reticulum Link — deliberately mirrors
//! (does not call into) `oag-sync/src/server.rs`'s `hello`/`get_events`/
//! `peers` handler bodies. See the plan's "deliberately duplicates" note:
//! this keeps the proven HTTP path in `oag-sync` completely untouched while
//! this pre-1.0 transport is still new.
use std::collections::HashMap;

use oag_crypto::PeerId;
use oag_storage::repo::{events as events_repo, peers as peers_repo};
use oag_storage::SqlitePool;
use oag_sync::wire::{EventsResponse, HelloResponse, PeerRecord, PeersResponse, SYNC_PROTOCOL, SYNC_VERSION};

use crate::wire::{RnRequest, RnResponse};

/// Same cap `oag-sync/src/server.rs::get_events` applies, same reasoning
/// (spec section 61 — a single response must stay bounded even though the
/// query itself only returns real matching rows).
const MAX_EVENTS_PER_FETCH: i64 = 1000;

pub async fn handle_request(
    pool: &SqlitePool,
    self_peer_id: PeerId,
    self_public_key: &[u8; 32],
    request: RnRequest,
) -> RnResponse {
    match request {
        RnRequest::Hello => match local_heads(pool).await {
            Ok(heads) => RnResponse::Hello(HelloResponse {
                protocol: SYNC_PROTOCOL.to_string(),
                version: SYNC_VERSION,
                peer_id: self_peer_id.to_string(),
                public_key: hex::encode(self_public_key),
                heads,
            }),
            Err(e) => RnResponse::Error(e.to_string()),
        },
        RnRequest::Events { origin_hex, from, to } => match handle_events(pool, &origin_hex, from, to).await {
            Ok(events) => RnResponse::Events(EventsResponse { events }),
            Err(e) => RnResponse::Error(e.to_string()),
        },
        RnRequest::Peers => match handle_peers(pool).await {
            Ok(peers) => RnResponse::Peers(PeersResponse { peers }),
            Err(e) => RnResponse::Error(e.to_string()),
        },
    }
}

pub async fn local_heads(pool: &SqlitePool) -> Result<HashMap<String, u64>, oag_storage::StorageError> {
    let mut conn = pool.acquire().await.map_err(oag_storage::StorageError::from)?;
    let rows = events_repo::list_all_origins(&mut conn).await?;
    Ok(rows
        .into_iter()
        .map(|r| (hex::encode(&r.origin_peer_id), r.highest_contiguous_sequence as u64))
        .collect())
}

async fn handle_events(
    pool: &SqlitePool,
    origin_hex: &str,
    from: u64,
    to: u64,
) -> Result<Vec<oag_events::SignedEvent>, oag_storage::StorageError> {
    let origin_bytes: [u8; 32] = hex::decode(origin_hex)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(oag_storage::StorageError::BadIdLength(origin_hex.len()))?;
    let from = from as i64;
    let to = std::cmp::min(to as i64, from + MAX_EVENTS_PER_FETCH - 1);
    let mut conn = pool.acquire().await.map_err(oag_storage::StorageError::from)?;
    let rows = events_repo::list_range(&mut conn, &origin_bytes, from, to).await?;
    rows.into_iter()
        .map(|row| serde_json::from_slice(&row.canonical_payload).map_err(oag_storage::StorageError::from))
        .collect()
}

async fn handle_peers(pool: &SqlitePool) -> Result<Vec<PeerRecord>, oag_storage::StorageError> {
    let mut conn = pool.acquire().await.map_err(oag_storage::StorageError::from)?;
    let infos = peers_repo::list_peers(&mut conn).await?;
    let mut records = Vec::with_capacity(infos.len());
    for info in infos {
        let addresses = peers_repo::list_addresses(&mut conn, &info.peer_id).await?;
        records.push(PeerRecord {
            peer_id: PeerId::from_bytes(info.peer_id).to_string(),
            public_key: hex::encode(info.public_key),
            addresses,
            last_seen: info.last_seen,
        });
    }
    Ok(records)
}
