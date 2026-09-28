//! Spec section 86 (Metrics): a `/metrics`-style snapshot of this peer's
//! current state. This module deliberately covers only the subset of the
//! spec's suggested measurements that are cheap, correct, and queryable
//! *right now* from data this codebase already tracks (simple `COUNT(*)`s
//! and a `PRAGMA`-derived file size) — no request-duration histograms, event
//! origin breakdowns, or crawler/verifier/replication counters, since those
//! need real instrumentation wired through other crates, not a snapshot
//! query. See the repo root's `OPEN_QUESTIONS.md` "Metrics" section for the
//! full list of what's deferred and why. No external metrics platform is
//! required to operate OAG (spec section 86, verbatim).
use oag_storage::repo::stats as stats_repo;

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct MetricsSnapshot {
    pub events_total: i64,
    pub nodes_total: i64,
    pub edges_total: i64,
    pub assertions_total: i64,
    pub evidence_total: i64,
    pub peer_count: i64,
    pub sqlite_size_bytes: i64,
}

impl GraphService {
    /// A point-in-time snapshot of this peer's cheap-to-query metrics (spec
    /// section 86). Callers (currently `oag-api`'s `GET /metrics`) render
    /// this into whatever wire format they need.
    pub async fn metrics_snapshot(&self) -> Result<MetricsSnapshot, GraphError> {
        let mut conn = self.pool().acquire().await.map_err(oag_storage::StorageError::from)?;
        Ok(MetricsSnapshot {
            events_total: stats_repo::events_total(&mut conn).await?,
            nodes_total: stats_repo::nodes_total(&mut conn).await?,
            edges_total: stats_repo::edges_total(&mut conn).await?,
            assertions_total: stats_repo::assertions_total(&mut conn).await?,
            evidence_total: stats_repo::evidence_total(&mut conn).await?,
            peer_count: stats_repo::peer_count(&mut conn).await?,
            sqlite_size_bytes: stats_repo::sqlite_size_bytes(&mut conn).await?,
        })
    }
}
