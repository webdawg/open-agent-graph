use std::collections::HashMap;
use std::sync::Arc;

use oag_crypto::{PeerId, VerifyingKey};
use oag_events::{ingest_remote_event, IngestOutcome};
use oag_storage::repo::{events as events_repo, peers as peers_repo, replication as replication_repo};
use oag_storage::SqlitePool;

use crate::client::{SyncClient, SyncClientError};
use crate::federation::FederationPolicy;
use crate::rate_limit::SyncRateLimiter;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Client(#[from] SyncClientError),
    #[error(transparent)]
    Storage(#[from] oag_storage::StorageError),
    #[error("invalid peer id in response: {0}")]
    PeerId(#[from] oag_crypto::PeerIdParseError),
    #[error("invalid or self-inconsistent public key in peer response")]
    InvalidPublicKey,
}

fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok()?.try_into().ok()
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Default)]
pub struct SyncSummary {
    pub applied: u64,
    pub already_known: u64,
    pub forks: u64,
    pub errors: Vec<String>,
}

/// Replication for one peer: pulls missing events from other origins,
/// verifies+projects them, and discovers new peers transitively (spec
/// sections 46, 50-52). Cheaply `Clone` (an `Arc`-backed pool + a
/// `reqwest::Client` under the hood) so it can be shared between the HTTP
/// server, the gossip loop, and one-shot CLI commands.
#[derive(Clone)]
pub struct SyncService {
    pool: SqlitePool,
    self_peer_id: PeerId,
    self_public_key: [u8; 32],
    federation: FederationPolicy,
    client: SyncClient,
    rate_limiter: Arc<SyncRateLimiter>,
}

impl SyncService {
    pub fn new(
        pool: SqlitePool,
        self_peer_id: PeerId,
        self_public_key: [u8; 32],
        federation: FederationPolicy,
    ) -> Self {
        Self {
            pool,
            self_peer_id,
            self_public_key,
            federation,
            client: SyncClient::new(),
            rate_limiter: Arc::new(SyncRateLimiter::default()),
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn self_peer_id(&self) -> PeerId {
        self.self_peer_id
    }

    pub fn self_public_key(&self) -> [u8; 32] {
        self.self_public_key
    }

    pub(crate) fn rate_limiter(&self) -> &SyncRateLimiter {
        &self.rate_limiter
    }

    pub fn federation_allows(&self, peer_id: &PeerId) -> bool {
        self.federation.allows(peer_id)
    }

    /// This peer's current head sequence for every origin it has any events
    /// from (including itself) — backs `/hello` and `/heads` (spec section
    /// 41/48).
    pub async fn local_heads(&self) -> Result<HashMap<String, u64>, SyncError> {
        let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
        let rows = events_repo::list_all_origins(&mut conn).await?;
        Ok(rows
            .into_iter()
            .map(|r| (hex::encode(&r.origin_peer_id), r.highest_contiguous_sequence as u64))
            .collect())
    }

    /// Pull everything `addr` has that we don't yet, for every origin whose
    /// signing key we know (or can learn from `addr` itself), then discover
    /// `addr`'s known peers for next time (spec section 46/50 — this is what
    /// lets a peer later fetch a *third* peer's history through `addr` as a
    /// relay, without ever contacting that third peer directly). Safe to
    /// call repeatedly; this is the gossip loop's steady-state operation.
    pub async fn sync_with_peer(&self, addr: &str) -> Result<SyncSummary, SyncError> {
        let hello = self.client.hello(addr).await?;
        let remote_peer_id: PeerId = hello.peer_id.parse()?;
        let remote_public_key_bytes = decode_hex32(&hello.public_key).ok_or(SyncError::InvalidPublicKey)?;
        let remote_public_key =
            VerifyingKey::from_bytes(&remote_public_key_bytes).map_err(|_| SyncError::InvalidPublicKey)?;
        if remote_peer_id != PeerId::from_public_key(&remote_public_key) {
            return Err(SyncError::InvalidPublicKey);
        }

        {
            let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
            let now = now_ts();
            peers_repo::upsert_peer(&mut conn, remote_peer_id.as_bytes(), &remote_public_key_bytes, None, now)
                .await?;
            peers_repo::add_address(&mut conn, remote_peer_id.as_bytes(), addr).await?;

            // Durability visibility (spec: never assume any one peer's own
            // storage is reliable — replication factor is a monitored
            // network property instead). `hello.heads` is `addr`'s own
            // claim about how far it's gotten with every origin it knows;
            // persist it so `replication_status()` can later ask "how many
            // peers are known to have caught up with *my* origin."
            for (origin_hex, sequence) in &hello.heads {
                if let Some(origin_bytes) = decode_hex32(origin_hex) {
                    replication_repo::upsert_known_head(
                        &mut conn,
                        remote_peer_id.as_bytes(),
                        &origin_bytes,
                        *sequence as i64,
                        now,
                    )
                    .await?;
                }
            }
        }

        // Peer discovery runs *before* event fetching so that, within this
        // same call, we can already learn the signing key of an origin we
        // only know about transitively through `addr` (spec section 99's
        // relay case) and fetch its events in the same pass rather than
        // needing a second gossip tick.
        self.discover_peers(addr).await;

        let mut summary = SyncSummary::default();
        let local_heads = self.local_heads().await?;

        for (origin_hex, remote_seq) in &hello.heads {
            let Some(origin_peer_id_bytes) = decode_hex32(origin_hex) else { continue };
            let origin_peer_id = PeerId::from_bytes(origin_peer_id_bytes);
            if !self.federation.allows(&origin_peer_id) {
                continue;
            }

            let local_seq = *local_heads.get(origin_hex).unwrap_or(&0);
            if *remote_seq <= local_seq {
                continue;
            }

            let verifying_key = if origin_peer_id == remote_peer_id {
                remote_public_key
            } else {
                let mut conn = self.pool.acquire().await.map_err(oag_storage::StorageError::from)?;
                match peers_repo::get_peer(&mut conn, &origin_peer_id_bytes).await? {
                    Some(info) => match VerifyingKey::from_bytes(&info.public_key) {
                        Ok(key) => key,
                        Err(_) => continue,
                    },
                    // Don't know this origin's key yet — skip until a future
                    // round discovers it (we just ran discovery above, so
                    // this only happens if no reachable peer advertises it).
                    None => continue,
                }
            };

            let events = match self.client.fetch_events(addr, origin_hex, local_seq + 1, *remote_seq).await {
                Ok(events) => events,
                Err(e) => {
                    summary.errors.push(format!("{origin_hex}: fetch failed: {e}"));
                    continue;
                }
            };

            for signed in events.events {
                let seq = signed.unsigned.sequence;
                match ingest_remote_event(&self.pool, verifying_key, signed, now_ts()).await {
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

    async fn discover_peers(&self, addr: &str) {
        // Spec section 61 ("peer Sybil attacks" / "disk exhaustion"): a
        // single malicious `/peers` response could otherwise claim an
        // unbounded number of fabricated peer identities, each getting a
        // row in our `peers`/`peer_addresses` tables for free — cap how
        // much of one response we act on, and how many addresses we'll
        // record per claimed peer.
        const MAX_PEERS_PER_RESPONSE: usize = 200;
        const MAX_ADDRESSES_PER_PEER: usize = 5;

        let Ok(peers) = self.client.fetch_peers(addr).await else { return };
        let Ok(mut conn) = self.pool.acquire().await else { return };
        let now = now_ts();
        for p in peers.peers.into_iter().take(MAX_PEERS_PER_RESPONSE) {
            let (Ok(pid), Some(pk)) = (p.peer_id.parse::<PeerId>(), decode_hex32(&p.public_key)) else {
                continue;
            };
            if pid == self.self_peer_id {
                continue;
            }
            if peers_repo::upsert_peer(&mut conn, pid.as_bytes(), &pk, None, now).await.is_err() {
                continue;
            }
            for a in p.addresses.into_iter().take(MAX_ADDRESSES_PER_PEER) {
                let _ = peers_repo::add_address(&mut conn, pid.as_bytes(), &a).await;
            }
        }
    }
}
