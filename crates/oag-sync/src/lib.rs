pub mod client;
pub mod federation;
pub mod gossip;
pub mod presence;
pub mod rate_limit;
pub mod replication;
pub mod server;
pub mod service;
#[cfg(test)]
mod tests;
pub mod trust;
pub mod wire;

pub use client::{SyncClient, SyncClientError};
pub use federation::FederationPolicy;
pub use gossip::spawn_gossip_loop;
pub use presence::{spawn_heartbeat_loop, PresenceError, PresenceHeartbeat, PresenceStatus};
pub use replication::{target_replication_factor, LaggingPeer, ReplicationStatus};
pub use server::router;
pub use service::{SyncError, SyncService, SyncSummary};
pub use trust::{trust_score, TrustConfig, EPHEMERAL_MINIMUM_TRUST};
