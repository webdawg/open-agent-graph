//! Durability visibility: no single peer's local storage or memory is ever
//! assumed reliable (no ECC-grade hardware requirement, no "the disk won't
//! corrupt" assumption) — durability instead comes from how many *other*
//! peers are known to already have a given origin's data. This module
//! turns that into a monitored number rather than an unstated hope.
use oag_crypto::PeerId;
use oag_storage::repo::{events as events_repo, peers as peers_repo, replication as replication_repo};

use crate::service::{SyncError, SyncService};

/// The absolute floor, regardless of network size: the standard minimum for
/// tolerating simultaneous loss of any two nodes without losing data
/// outright.
pub const MIN_REPLICATION_FACTOR: usize = 3;

/// How many peers a given origin's data should be known to live on, given a
/// network of `known_peer_count` peers. Square-root scaling above the floor:
/// grows meaningfully as the network grows (10 peers -> 4, 100 -> 10, 10,000
/// -> 100) without demanding literal full replication to every peer — this
/// is a *monitored floor* to raise alarms against, not a cap on anything;
/// OAG's actual replication behavior (spec section 42, full replication)
/// already tends toward "every peer eventually has everything" regardless.
pub fn target_replication_factor(known_peer_count: usize) -> usize {
    let scaled = (known_peer_count as f64).sqrt().ceil();
    (scaled as usize).max(MIN_REPLICATION_FACTOR)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LaggingPeer {
    pub peer_id: String,
    /// `None` if this peer has never reported any head for our origin at all.
    pub known_sequence: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReplicationStatus {
    pub known_peer_count: usize,
    pub target_replication_factor: usize,
    pub self_current_sequence: u64,
    pub peers_fully_caught_up: usize,
    pub meets_target: bool,
    pub lagging_peers: Vec<LaggingPeer>,
}

impl SyncService {
    /// How well-replicated *this peer's own* data currently is, as far as
    /// this peer can tell from what other peers have self-reported during
    /// past syncs (never independently verified — see this module's doc
    /// comment and `replication` repo module for that caveat).
    pub async fn replication_status(&self) -> Result<ReplicationStatus, SyncError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;

        let self_peer_id = self.self_peer_id();
        let self_current_sequence = events_repo::get_origin(&mut conn, self_peer_id.as_bytes())
            .await?
            .map(|row| row.highest_contiguous_sequence as u64)
            .unwrap_or(0);

        let known_peers = peers_repo::list_peers(&mut conn).await?;
        let known_peer_count = known_peers.len();
        let target = target_replication_factor(known_peer_count);

        let known_heads = replication_repo::list_known_heads_for_origin(&mut conn, self_peer_id.as_bytes()).await?;
        let known_heads: std::collections::HashMap<[u8; 32], u64> =
            known_heads.into_iter().map(|h| (h.peer_id, h.sequence as u64)).collect();

        let mut peers_fully_caught_up = 0usize;
        let mut lagging_peers = Vec::new();
        for peer in &known_peers {
            match known_heads.get(&peer.peer_id) {
                Some(&sequence) if sequence >= self_current_sequence => {
                    peers_fully_caught_up += 1;
                }
                Some(&sequence) => {
                    lagging_peers.push(LaggingPeer {
                        peer_id: PeerId::from_bytes(peer.peer_id).to_string(),
                        known_sequence: Some(sequence),
                    });
                }
                None => {
                    lagging_peers.push(LaggingPeer {
                        peer_id: PeerId::from_bytes(peer.peer_id).to_string(),
                        known_sequence: None,
                    });
                }
            }
        }

        Ok(ReplicationStatus {
            known_peer_count,
            target_replication_factor: target,
            self_current_sequence,
            peers_fully_caught_up,
            meets_target: peers_fully_caught_up >= target,
            lagging_peers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_applies_below_nine_peers() {
        for n in [0, 1, 2, 3, 5, 8, 9] {
            assert_eq!(target_replication_factor(n), MIN_REPLICATION_FACTOR, "n={n}");
        }
    }

    #[test]
    fn scales_with_sqrt_above_the_floor() {
        assert_eq!(target_replication_factor(10), 4); // sqrt(10) = 3.16 -> ceil 4
        assert_eq!(target_replication_factor(16), 4); // sqrt(16) = 4.0
        assert_eq!(target_replication_factor(17), 5); // sqrt(17) = 4.12 -> ceil 5
        assert_eq!(target_replication_factor(100), 10);
        assert_eq!(target_replication_factor(10_000), 100);
    }
}
