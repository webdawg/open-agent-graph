//! Client side: one-shot Reticulum sync with a peer identified by its
//! Reticulum `AddressHash`, replicating `SyncService::sync_with_peer`'s
//! exact pull-loop semantics (spec sections 46, 50-52) over a Reticulum
//! [`Link`] instead of `reqwest` — see the plan's "deliberately duplicates"
//! note for why this isn't shared code with `oag-sync`.
use std::time::Duration;

use oag_crypto::{PeerId, PeerIdentity, VerifyingKey};
use oag_events::ingest_remote_event;
use oag_events::IngestOutcome;
use oag_storage::repo::peers as peers_repo;
use oag_storage::SqlitePool;
use oag_sync::wire::{EventsResponse, HelloResponse, PeersResponse};
use oag_sync::SyncSummary;
use reticulum::destination::link::{Link, LinkEvent, LinkEventData};
use reticulum::destination::DestinationDesc;
use reticulum::hash::AddressHash;
use reticulum::iface::tcp_client::TcpClient;
use reticulum::transport::{Transport, TransportConfig};
use tokio::sync::broadcast::Receiver;
use tracing::warn;

use crate::framing::{decode_message, encode_message, Reassembler, CHUNK_SIZE};
use crate::identity::derive_reticulum_identity;
use crate::wire::{RnRequest, RnResponse};

type LinkHandle = std::sync::Arc<tokio::sync::Mutex<Link>>;

const ANNOUNCE_WAIT: Duration = Duration::from_secs(30);
const LINK_ACTIVATE_WAIT: Duration = Duration::from_secs(15);
const REQUEST_WAIT: Duration = Duration::from_secs(15);

/// Spec section 61 caps, mirroring `SyncService::discover_peers` exactly.
const MAX_PEERS_PER_RESPONSE: usize = 200;
const MAX_ADDRESSES_PER_PEER: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum ReticulumError {
    #[error(transparent)]
    Storage(#[from] oag_storage::StorageError),
    #[error(transparent)]
    Events(#[from] oag_events::EventsError),
    #[error(transparent)]
    Framing(#[from] crate::framing::FramingError),
    #[error("no announce for {0} arrived within the timeout")]
    NoAnnounce(String),
    #[error("link to {0} did not activate within the timeout")]
    LinkNotActivated(String),
    #[error("no response arrived within the timeout")]
    ResponseTimeout,
    #[error("remote peer returned an error: {0}")]
    RemoteError(String),
    #[error("unexpected response type from remote peer")]
    UnexpectedResponse,
    #[error("remote peer's claimed identity doesn't match its public key")]
    InvalidPublicKey,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok()?.try_into().ok()
}

/// Bring up a throwaway `Transport`, dial `uplink_tcp`, wait for the target
/// destination to announce itself, establish a `Link`, and pull every event
/// this peer is missing — one call, then the transport is torn down.
pub async fn sync_with_peer(
    pool: &SqlitePool,
    peer_identity: &PeerIdentity,
    uplink_tcp: &str,
    target: AddressHash,
) -> Result<SyncSummary, ReticulumError> {
    let reticulum_identity = derive_reticulum_identity(peer_identity);
    let transport = Transport::new(TransportConfig::new("oag-client", &reticulum_identity, false));

    let mut announces = transport.recv_announces().await;
    transport.iface_manager().lock().await.spawn(TcpClient::new(uplink_tcp.to_string()), TcpClient::spawn);

    let target_desc = wait_for_announce(&mut announces, target).await?;

    let link = transport.link(target_desc).await;
    let link_id = *link.lock().await.id();

    let mut link_events = transport.out_link_events();
    wait_for_activation(&mut link_events, link_id).await?;

    let hello: HelloResponse = match send_request(&transport, &link, link_id, &mut link_events, RnRequest::Hello).await? {
        RnResponse::Hello(hello) => hello,
        RnResponse::Error(e) => return Err(ReticulumError::RemoteError(e)),
        _ => return Err(ReticulumError::UnexpectedResponse),
    };

    let remote_peer_id: PeerId = hello.peer_id.parse().map_err(|_| ReticulumError::InvalidPublicKey)?;
    let remote_public_key_bytes = decode_hex32(&hello.public_key).ok_or(ReticulumError::InvalidPublicKey)?;
    let remote_public_key = VerifyingKey::from_bytes(&remote_public_key_bytes).map_err(|_| ReticulumError::InvalidPublicKey)?;
    if remote_peer_id != PeerId::from_public_key(&remote_public_key) {
        return Err(ReticulumError::InvalidPublicKey);
    }

    {
        let mut conn = pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        let now = now_ts();
        peers_repo::upsert_peer(&mut conn, remote_peer_id.as_bytes(), &remote_public_key_bytes, None, now).await?;
        peers_repo::add_address(&mut conn, remote_peer_id.as_bytes(), &format!("reticulum:{}", target.to_hex_string()))
            .await?;
    }

    discover_peers(pool, &transport, &link, link_id, &mut link_events).await;

    let mut summary = SyncSummary::default();
    let local_heads = crate::handlers::local_heads(pool).await?;

    for (origin_hex, remote_seq) in &hello.heads {
        let Some(origin_peer_id_bytes) = decode_hex32(origin_hex) else { continue };
        let origin_peer_id = PeerId::from_bytes(origin_peer_id_bytes);

        let local_seq = *local_heads.get(origin_hex).unwrap_or(&0);
        if *remote_seq <= local_seq {
            continue;
        }

        let verifying_key = if origin_peer_id == remote_peer_id {
            remote_public_key
        } else {
            let mut conn = pool.acquire().await.map_err(oag_storage::StorageError::from)?;
            match peers_repo::get_peer(&mut conn, &origin_peer_id_bytes).await? {
                Some(info) => match VerifyingKey::from_bytes(&info.public_key) {
                    Ok(key) => key,
                    Err(_) => continue,
                },
                None => continue,
            }
        };

        let request = RnRequest::Events { origin_hex: origin_hex.clone(), from: local_seq + 1, to: *remote_seq };
        let events: EventsResponse = match send_request(&transport, &link, link_id, &mut link_events, request).await {
            Ok(RnResponse::Events(events)) => events,
            Ok(RnResponse::Error(e)) => {
                summary.errors.push(format!("{origin_hex}: {e}"));
                continue;
            }
            Ok(_) => {
                summary.errors.push(format!("{origin_hex}: unexpected response type"));
                continue;
            }
            Err(e) => {
                summary.errors.push(format!("{origin_hex}: fetch failed: {e}"));
                continue;
            }
        };

        for signed in events.events {
            let seq = signed.unsigned.sequence;
            match ingest_remote_event(pool, verifying_key, signed, now_ts()).await {
                Ok(IngestOutcome::Applied(_, _)) => summary.applied += 1,
                Ok(IngestOutcome::AlreadyKnown(_)) => summary.already_known += 1,
                Ok(IngestOutcome::Forked { .. }) => {
                    summary.forks += 1;
                    break;
                }
                Err(e) => {
                    summary.errors.push(format!("{origin_hex}@{seq}: {e}"));
                    break;
                }
            }
        }
    }

    Ok(summary)
}

async fn discover_peers(
    pool: &SqlitePool,
    transport: &Transport,
    link: &LinkHandle,
    link_id: AddressHash,
    link_events: &mut Receiver<LinkEventData>,
) {
    let peers: PeersResponse = match send_request(transport, link, link_id, link_events, RnRequest::Peers).await {
        Ok(RnResponse::Peers(peers)) => peers,
        _ => return,
    };
    let Ok(mut conn) = pool.acquire().await else { return };
    let now = now_ts();
    for p in peers.peers.into_iter().take(MAX_PEERS_PER_RESPONSE) {
        let (Ok(pid), Some(pk)) = (p.peer_id.parse::<PeerId>(), decode_hex32(&p.public_key)) else { continue };
        if peers_repo::upsert_peer(&mut conn, pid.as_bytes(), &pk, None, now).await.is_err() {
            continue;
        }
        for a in p.addresses.into_iter().take(MAX_ADDRESSES_PER_PEER) {
            let _ = peers_repo::add_address(&mut conn, pid.as_bytes(), &a).await;
        }
    }
}

async fn wait_for_announce(
    announces: &mut Receiver<reticulum::transport::AnnounceEvent>,
    target: AddressHash,
) -> Result<DestinationDesc, ReticulumError> {
    let found = tokio::time::timeout(ANNOUNCE_WAIT, async {
        loop {
            match announces.recv().await {
                Ok(announce) => {
                    let desc = announce.destination.lock().await.desc;
                    if desc.address_hash == target {
                        return Some(desc);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .await
    .map_err(|_| ReticulumError::NoAnnounce(target.to_hex_string()))?;

    found.ok_or_else(|| ReticulumError::NoAnnounce(target.to_hex_string()))
}

async fn wait_for_activation(
    link_events: &mut Receiver<LinkEventData>,
    link_id: AddressHash,
) -> Result<(), ReticulumError> {
    tokio::time::timeout(LINK_ACTIVATE_WAIT, async {
        loop {
            match link_events.recv().await {
                Ok(event) if event.id == link_id && matches!(event.event, LinkEvent::Activated) => return true,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
            }
        }
    })
    .await
    .map_err(|_| ReticulumError::LinkNotActivated(link_id.to_hex_string()))
    .and_then(|activated| {
        if activated {
            Ok(())
        } else {
            Err(ReticulumError::LinkNotActivated(link_id.to_hex_string()))
        }
    })
}

async fn send_request(
    transport: &Transport,
    link: &LinkHandle,
    link_id: AddressHash,
    link_events: &mut Receiver<LinkEventData>,
    request: RnRequest,
) -> Result<RnResponse, ReticulumError> {
    let framed = encode_message(&request)?;
    for chunk in framed.chunks(CHUNK_SIZE) {
        let packet = link.lock().await.data_packet(chunk).map_err(|e| {
            warn!("oag-reticulum: failed to build request packet: {e:?}");
            ReticulumError::ResponseTimeout
        })?;
        transport.send_packet(packet).await;
    }

    let mut reassembler = Reassembler::new();
    tokio::time::timeout(REQUEST_WAIT, async {
        loop {
            match link_events.recv().await {
                Ok(event) if event.id == link_id => {
                    if let LinkEvent::Data(payload) = event.event {
                        if let Ok(Some(message)) = reassembler.push(payload.as_slice()) {
                            return Some(message);
                        }
                    }
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .await
    .map_err(|_| ReticulumError::ResponseTimeout)?
    .ok_or(ReticulumError::ResponseTimeout)
    .and_then(|bytes| decode_message(&bytes).map_err(ReticulumError::from))
}
