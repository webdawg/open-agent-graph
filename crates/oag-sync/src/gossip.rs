use std::time::Duration;

use tokio::task::JoinHandle;

use crate::service::SyncService;

/// Periodically sync with every known peer address — bootstrap-configured
/// ones plus anything discovered transitively along the way (spec section
/// 51: anti-entropy, not synchronous broadcast). One peer being unreachable
/// never stops the others or the next tick.
pub fn spawn_gossip_loop(
    service: SyncService,
    bootstrap_addrs: Vec<String>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
            as i64;
        // The gravity layer (spec/25): rolled once per process lifetime,
        // not re-queried every tick -- a node's own gravity level doesn't
        // change mid-flight.
        let gravity_level =
            crate::gravity::get_or_generate(service.pool(), &service.self_peer_id(), now).await.unwrap_or(0.0);
        loop {
            let mut addrs = bootstrap_addrs.clone();
            if let Ok(mut conn) = service.pool().acquire().await {
                if let Ok(peers) = oag_storage::repo::peers::list_peers(&mut conn).await {
                    for peer in peers {
                        if let Ok(known) = oag_storage::repo::peers::list_addresses(&mut conn, &peer.peer_id).await {
                            addrs.extend(known);
                        }
                    }
                }
            }
            addrs.sort();
            addrs.dedup();

            for addr in &addrs {
                // Gravity's "one at a time": this node's own very slight
                // processing slowdown, applied once per peer address it
                // processes in its own loop (spec/25).
                crate::gravity::apply_slowdown(gravity_level).await;
                match service.sync_with_peer(addr).await {
                    Ok(summary) => {
                        if summary.applied > 0 || !summary.errors.is_empty() {
                            tracing::info!(
                                peer = %addr,
                                applied = summary.applied,
                                already_known = summary.already_known,
                                forks = summary.forks,
                                errors = ?summary.errors,
                                "sync round complete"
                            );
                        }
                    }
                    Err(error) => {
                        tracing::warn!(peer = %addr, %error, "sync with peer failed");
                    }
                }
            }

            tokio::time::sleep(interval).await;
        }
    })
}
