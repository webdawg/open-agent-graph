//! Server side: announce this peer's `oag/sync` destination on a Reticulum
//! network and answer incoming Links with the same three calls
//! `SyncService::sync_with_peer`'s pull loop makes over HTTP (spec sections
//! 46-52), just carried over a Reticulum [`Link`] instead of `reqwest`.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use oag_crypto::PeerIdentity;
use oag_storage::SqlitePool;
use reticulum::destination::link::LinkEvent;
use reticulum::destination::DestinationName;
use reticulum::hash::AddressHash;
use reticulum::iface::tcp_client::TcpClient;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::transport::{Transport, TransportConfig};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};

use crate::framing::{encode_message, Reassembler, CHUNK_SIZE};
use crate::handlers::handle_request;
use crate::identity::derive_reticulum_identity;
use crate::wire::RnRequest;

#[derive(Debug, Clone)]
pub struct ReticulumConfig {
    /// If set, this peer accepts incoming Reticulum TCP connections here.
    pub listen_tcp: Option<SocketAddr>,
    /// If set, this peer also dials out to an existing Reticulum TCP
    /// interface (e.g. a shared transport node), joining a wider network
    /// rather than being reachable only by direct OAG-to-OAG TCP links.
    pub uplink_tcp: Option<String>,
    pub announce_interval: Duration,
}

/// This peer's Reticulum address — pure computation, no networking, so it's
/// cheap enough for a one-shot CLI command (`oag peer reticulum-address`).
pub fn local_address_hash(peer_identity: &PeerIdentity) -> AddressHash {
    let identity = derive_reticulum_identity(peer_identity);
    reticulum::destination::SingleInputDestination::new(identity, DestinationName::new("oag", "sync"))
        .desc
        .address_hash
}

/// Runs forever: announces this peer's destination periodically and answers
/// every incoming Link. Intended to be spawned as a background task
/// alongside `oag serve`'s existing REST/MCP/sync-HTTP servers.
pub async fn run_listener(pool: SqlitePool, peer_identity: PeerIdentity, config: ReticulumConfig) {
    let reticulum_identity = derive_reticulum_identity(&peer_identity);
    let mut transport = Transport::new(TransportConfig::new("oag-server", &reticulum_identity, false));

    if let Some(addr) = config.listen_tcp {
        transport
            .iface_manager()
            .lock()
            .await
            .spawn(TcpServer::new(addr.to_string(), transport.iface_manager()), TcpServer::spawn);
        info!("oag-reticulum: TCP interface listening on {addr}");
    }
    if let Some(addr) = &config.uplink_tcp {
        transport.iface_manager().lock().await.spawn(TcpClient::new(addr.clone()), TcpClient::spawn);
        info!("oag-reticulum: TCP uplink to {addr}");
    }

    let destination = transport.add_destination(reticulum_identity, DestinationName::new("oag", "sync")).await;
    let address_hash = destination.lock().await.desc.address_hash;
    info!("oag-reticulum: destination address {address_hash}");

    let transport = Arc::new(transport);

    {
        let transport = transport.clone();
        let destination = destination.clone();
        let interval = config.announce_interval;
        tokio::spawn(async move {
            loop {
                transport.send_announce(&destination, None).await;
                tokio::time::sleep(interval).await;
            }
        });
    }

    let self_peer_id = peer_identity.peer_id();
    let self_public_key = peer_identity.verifying_key().to_bytes();
    let mut events = transport.in_link_events();
    let mut buffers: HashMap<AddressHash, Reassembler> = HashMap::new();

    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(RecvError::Lagged(skipped)) => {
                warn!("oag-reticulum: listener lagged, dropped {skipped} link events");
                continue;
            }
            Err(RecvError::Closed) => break,
        };

        match event.event {
            LinkEvent::Activated => {
                debug!("oag-reticulum: link {} activated", event.id);
                buffers.insert(event.id, Reassembler::new());
            }
            LinkEvent::Closed => {
                buffers.remove(&event.id);
            }
            LinkEvent::Data(payload) => {
                let reassembler = buffers.entry(event.id).or_default();
                let complete = match reassembler.push(payload.as_slice()) {
                    Ok(complete) => complete,
                    Err(e) => {
                        warn!("oag-reticulum: framing error from link {}: {e}", event.id);
                        buffers.remove(&event.id);
                        continue;
                    }
                };
                let Some(message_bytes) = complete else { continue };

                let request: RnRequest = match serde_json::from_slice(&message_bytes) {
                    Ok(request) => request,
                    Err(e) => {
                        warn!("oag-reticulum: malformed request from link {}: {e}", event.id);
                        continue;
                    }
                };

                let response = handle_request(&pool, self_peer_id, &self_public_key, request).await;

                let Some(link) = transport.find_in_link(&event.id).await else {
                    warn!("oag-reticulum: no link found for id {} when replying", event.id);
                    continue;
                };
                let framed = match encode_message(&response) {
                    Ok(framed) => framed,
                    Err(e) => {
                        warn!("oag-reticulum: failed to encode response: {e}");
                        continue;
                    }
                };
                for chunk in framed.chunks(CHUNK_SIZE) {
                    let packet = link.lock().await.data_packet(chunk);
                    match packet {
                        Ok(packet) => transport.send_packet(packet).await,
                        Err(e) => {
                            warn!("oag-reticulum: failed to build response packet: {e:?}");
                            break;
                        }
                    }
                }
            }
        }
    }
}
