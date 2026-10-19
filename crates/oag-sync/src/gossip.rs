use std::time::Duration;

use tokio::task::JoinHandle;

use crate::service::SyncService;

/// One best-effort sync round against every address in `addrs`, applying
/// this peer's own gravity slowdown (spec/25) once per address and logging
/// exactly as the standard gossip loop always has. Shared by
/// [`spawn_gossip_loop`] and [`spawn_cluster_gossip_loop`] (peer clustering,
/// Phase 2 — spec/26) so both loops' per-peer behavior stays identical; only
/// *which* addresses they're given, and on what interval, differs.
async fn sync_round(service: &SyncService, addrs: &[String], gravity_level: f32) {
    for addr in addrs {
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
                service.record_sync_error();
                tracing::warn!(peer = %addr, %error, "sync with peer failed");
            }
        }
    }
}

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

            sync_round(&service, &addrs, gravity_level).await;

            tokio::time::sleep(interval).await;
        }
    })
}

/// Peer clustering, Phase 2 (spec/26): a second, independent gossip loop
/// that only ever syncs with this peer's *same-cluster* peers
/// (`SyncService::cluster_peer_addrs`), on its own, typically much faster,
/// configurable cadence (`[cluster].gossip_interval_seconds`) -- making the
/// "ant colony" framing from `future_peer_clustering.md` concrete: a
/// cluster's members replicate with each other faster than the standard,
/// graph-wide [`spawn_gossip_loop`] interval, without changing that
/// standard loop's own cadence or scope for anyone, clustered or not -- it
/// keeps running unchanged, alongside this one. A pure no-op loop (an empty
/// address list every tick, nothing ever synced) whenever this service has
/// no configured cluster, so spawning it unconditionally is harmless.
pub fn spawn_cluster_gossip_loop(service: SyncService, interval: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
            as i64;
        let gravity_level =
            crate::gravity::get_or_generate(service.pool(), &service.self_peer_id(), now).await.unwrap_or(0.0);
        loop {
            let addrs = service.cluster_peer_addrs().await;
            sync_round(&service, &addrs, gravity_level).await;
            tokio::time::sleep(interval).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oag_crypto::PeerIdentity;

    /// Spec section 86's `peer_sync_errors`: a whole-attempt sync failure
    /// (an unreachable address, never even a real peer) must increment the
    /// shared counter, and every clone of the same `SyncService` must see
    /// the increment -- confirming the `Arc<AtomicU64>` is actually shared,
    /// not just present.
    #[tokio::test]
    async fn sync_round_increments_the_shared_error_counter_on_a_whole_attempt_failure() {
        let dir = std::env::temp_dir().join(format!(
            "oag-sync-gossip-error-counter-test-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = oag_storage::pool::open_pool(&dir.join("oag.sqlite")).await.unwrap();
        let identity = PeerIdentity::generate();
        let service = SyncService::new(
            pool,
            identity.peer_id(),
            identity.verifying_key().to_bytes(),
            crate::federation::FederationPolicy::Open,
        );
        let clone = service.clone();
        assert_eq!(service.sync_errors(), 0);

        // Port 1 is never a real server in these tests -- a guaranteed,
        // fast connection failure, not a flaky real-network dependency.
        sync_round(&service, &["127.0.0.1:1".to_string()], 0.0).await;

        assert_eq!(service.sync_errors(), 1);
        assert_eq!(clone.sync_errors(), 1, "a clone must see the same increment via the shared Arc");
    }
}
