//! Spec section 86 (Metrics): a `/metrics`-style snapshot of this peer's
//! current state. This module deliberately covers only the subset of the
//! spec's suggested measurements that are cheap, correct, and queryable
//! *right now* from data this codebase already tracks (simple `COUNT(*)`s
//! and a `PRAGMA`-derived file size), plus the process-lifetime verification
//! counters (`verification_jobs`/`verification_failures`), which needed real
//! instrumentation but now have it on `GraphService`, plus the search-duration
//! histogram (`search_latency`), which needed a middleware-shaped counter rather
//! than a snapshot query. Still not covered: the REST request-duration histogram
//! (`api_latency`, which lives on `oag-api` because it measures the HTTP layer
//! rather than the graph) and event origin breakdowns, which need a decision on
//! label cardinality.
//! See the repo root's `OPEN_QUESTIONS.md` "Metrics" section for the full
//! list of what's deferred and why. No external metrics platform is required
//! to operate OAG (spec section 86, verbatim).
use oag_storage::repo::stats as stats_repo;

use crate::error::GraphError;
use crate::service::GraphService;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MetricsSnapshot {
    pub events_total: i64,
    pub nodes_total: i64,
    pub edges_total: i64,
    pub assertions_total: i64,
    pub evidence_total: i64,
    pub peer_count: i64,
    pub sqlite_size_bytes: i64,
    /// Process-lifetime count of `verify_assertion` calls (spec section 86's
    /// `verification_jobs`). Not a `COUNT(*)` -- it counts attempts this
    /// process made, including rejected inputs, which the event log does not
    /// record.
    pub verification_jobs: i64,
    /// Of `verification_jobs`, how many returned `Err` (a rejected input --
    /// bad assertion id, bad result string, missing permission). Spec section
    /// 86's `verification_failures`.
    pub verification_failures: i64,
    /// Process-lifetime durations of this process's searches, both keyword and semantic (spec
    /// section 86's `search_latency`). A histogram rather than a count: a count cannot answer
    /// "is this peer getting slower?".
    pub search_latency: crate::latency::LatencySnapshot,
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
            verification_jobs: self.verification_jobs.load(std::sync::atomic::Ordering::Relaxed) as i64,
            verification_failures: self.verification_failures.load(std::sync::atomic::Ordering::Relaxed) as i64,
            search_latency: self.search_latency.snapshot(),
        })
    }
}
