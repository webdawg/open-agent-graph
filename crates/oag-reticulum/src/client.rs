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
use oag_storage::repo::replication as replication_repo;
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

        // Durability visibility (see oag-sync::SyncService::sync_with_peer's
        // identical block) -- persisted here too since this transport
        // deliberately duplicates rather than shares oag-sync's pull logic.
        for (origin_hex, sequence) in &hello.heads {
            if let Some(origin_bytes) = decode_hex32(origin_hex) {
                replication_repo::upsert_known_head(&mut conn, remote_peer_id.as_bytes(), &origin_bytes, *sequence as i64, now)
                    .await?;
            }
        }
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

#[cfg(test)]
mod live_tests {
    //! Live loopback coverage for the transport's *packet path*.
    //!
    //! `tests/two_peer_sync.rs` already proves the whole stack end to end -- a real event over a
    //! real Reticulum `Link` -- and the `framing` tests prove chunking in memory. Neither covers
    //! the middle: a payload large enough to span many `data_packet` calls over an actual link,
    //! where each chunk becomes a separately encrypted packet and reassembly depends on real
    //! arrival order and timing. That is the path a sync of any sizeable batch of events takes,
    //! and it is exactly where an off-by-one in chunking would show up.
    //!
    //! These tests also isolate the transport from the sync handler: if `two_peer_sync` fails,
    //! this says whether the link itself or the logic above it is at fault.
    use std::sync::Arc;
    use std::time::Duration;

    use reticulum::destination::link::LinkEvent;
    use reticulum::destination::DestinationName;
    use reticulum::identity::PrivateIdentity;
    use reticulum::iface::tcp_client::TcpClient;
    use reticulum::iface::tcp_server::TcpServer;
    use reticulum::transport::{Transport, TransportConfig};

    use crate::framing::{decode_message, encode_message, Reassembler, CHUNK_SIZE};

    use super::{wait_for_activation, wait_for_announce};

    /// A free localhost port. The listener is dropped immediately, so there is a small race,
    /// but nothing else on a test machine contends for an ephemeral port.
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn test_identity(seed: &str) -> PrivateIdentity {
        PrivateIdentity::new_from_name(&format!("oag-live-test-{seed}"))
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
    struct Blob {
        /// Bytes as a string so the payload size is easy to choose; the framing layer does not
        /// care what the JSON body contains.
        text: String,
        /// Set by the server, so the client can tell a reply from an echo of its own request.
        reply_to: Option<String>,
    }

    impl Blob {
        fn request(bytes: usize) -> Self {
            Blob { text: "x".repeat(bytes), reply_to: None }
        }
    }

    /// Runs one loopback link round-trip for a payload of `payload_bytes`, returning the reply.
    /// Sets up a fresh pair of transports each call: the transports own background tasks that
    /// cannot be cleanly shut down, so sharing one pair across assertions would leak tasks and
    /// couple the cases to each other's timing.
    async fn round_trip_over_loopback(payload_bytes: usize) -> (String, usize) {
        let addr = format!("127.0.0.1:{}", free_port());

        // Server: announce one destination and answer every request with a reply that quotes back
        // the request's own length, mirroring `run_listener`'s loop shape.
        let mut server_transport =
            Transport::new(TransportConfig::new("oag-live-server", &test_identity("server"), false));
        server_transport.iface_manager().lock().await.spawn(
            TcpServer::new(addr.clone(), server_transport.iface_manager()),
            TcpServer::spawn,
        );
        let server_dest = server_transport
            .add_destination(test_identity("server"), DestinationName::new("oag", "live-test"))
            .await;
        let server_address = server_dest.lock().await.desc.address_hash;
        let server_transport = Arc::new(server_transport);

        {
            let server_transport = server_transport.clone();
            tokio::spawn(async move {
                let mut buffers: std::collections::HashMap<reticulum::hash::AddressHash, Reassembler> =
                    std::collections::HashMap::new();
                let mut events = server_transport.in_link_events();
                loop {
                    let event = match events.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    match event.event {
                        LinkEvent::Activated => {
                            buffers.insert(event.id, Reassembler::new());
                        }
                        LinkEvent::Closed => {
                            buffers.remove(&event.id);
                        }
                        LinkEvent::Data(payload) => {
                            let reassembler = buffers.entry(event.id).or_default();
                            let Ok(Some(bytes)) = reassembler.push(payload.as_slice()) else {
                                continue;
                            };
                            let request: Blob = match decode_message(&bytes) {
                                Ok(request) => request,
                                Err(_) => continue,
                            };
                            let request_len = request.text.len();
                            let reply = Blob {
                                text: request.text,
                                reply_to: Some(format!("got {request_len} bytes")),
                            };
                            let framed = match encode_message(&reply) {
                                Ok(framed) => framed,
                                Err(_) => continue,
                            };
                            let Some(link) = server_transport.find_in_link(&event.id).await else {
                                continue;
                            };
                            for chunk in framed.chunks(CHUNK_SIZE) {
                                if let Ok(packet) = link.lock().await.data_packet(chunk) {
                                    server_transport.send_packet(packet).await;
                                }
                            }
                        }
                    }
                }
            });
        }

        let client_transport =
            Transport::new(TransportConfig::new("oag-live-client", &test_identity("client"), false));
        client_transport
            .iface_manager()
            .lock()
            .await
            .spawn(TcpClient::new(addr), TcpClient::spawn);
        let client_transport = Arc::new(client_transport);
        let mut announces = client_transport.recv_announces().await;

        // Give the TCP interface time to connect before the first announce, then keep announcing
        // until the client observes one: an announce sent before the client subscribed, or before
        // the interface was up, would be missed.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let target = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                server_transport.send_announce(&server_dest, None).await;
                if let Ok(desc) = wait_for_announce(&mut announces, server_address).await {
                    return desc;
                }
            }
        })
        .await
        .expect("the server's announce should be observed over loopback TCP");

        let link = client_transport.link(target).await;
        let link_id = *link.lock().await.id();
        let mut link_events = client_transport.out_link_events();
        wait_for_activation(&mut link_events, link_id).await.expect("link should activate");

        let request = Blob::request(payload_bytes);
        let framed = encode_message(&request).unwrap();
        let chunks = framed.chunks(CHUNK_SIZE).count();
        for chunk in framed.chunks(CHUNK_SIZE) {
            let packet = link.lock().await.data_packet(chunk).unwrap();
            client_transport.send_packet(packet).await;
        }

        let reply = tokio::time::timeout(Duration::from_secs(20), async {
            let mut reassembler = Reassembler::new();
            loop {
                match link_events.recv().await {
                    Ok(event) if event.id == link_id => {
                        if let LinkEvent::Data(payload) = event.event {
                            if let Ok(Some(bytes)) = reassembler.push(payload.as_slice()) {
                                return decode_message::<Blob>(&bytes).unwrap();
                            }
                        }
                    }
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => panic!("link events closed"),
                }
            }
        })
        .await
        .expect("reply should arrive within the timeout");

        (reply.reply_to.expect("the server always sets reply_to"), chunks)
    }

    #[tokio::test]
    async fn a_real_link_round_trips_a_small_framed_message() {
        let (reply, chunks) = round_trip_over_loopback(16).await;
        assert_eq!(reply, "got 16 bytes");
        assert_eq!(chunks, 1, "a 16-byte payload should fit in one chunk");
    }

    #[tokio::test]
    async fn a_real_link_round_trips_a_message_spanning_many_chunks() {
        // Comfortably past one `CHUNK_SIZE` (1400) chunk and past any single Reticulum packet's
        // plaintext capacity, so this exercises real multi-packet chunking over a real link --
        // the path a sync of a sizeable event batch takes.
        let payload = CHUNK_SIZE * 8;
        let (reply, chunks) = round_trip_over_loopback(payload).await;
        assert_eq!(reply, format!("got {payload} bytes"));
        assert!(chunks > 8, "expected real multi-chunk framing, got {chunks} chunk(s)");
    }
}
