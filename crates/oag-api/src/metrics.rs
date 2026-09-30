//! Spec section 86 (Metrics): `GET /metrics`, hand-rolled Prometheus text
//! exposition format. Deliberately no `metrics`/`metrics-exporter-prometheus`
//! crate -- the spec explicitly requires no external metrics platform to
//! operate OAG, and this is a handful of `# TYPE`/value lines, not enough
//! surface to justify a dependency. Unauthenticated, same as `/api/v1/status`:
//! metrics endpoints are conventionally open so a scraper doesn't need a
//! credential (see `OPEN_QUESTIONS.md`'s "Metrics" section for the full
//! scoping rationale, including which of the spec's suggested measurements
//! this milestone leaves out).
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use oag_graph::MetricsSnapshot;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn handler(State(state): State<AppState>) -> Result<Response, ApiError> {
    let snapshot = state.graph.metrics_snapshot().await?;
    let crawl_counts = CrawlCounts { total: state.crawler.crawls_total(), failed: state.crawler.crawls_failed() };
    // Best-effort: a metrics scrape shouldn't fail outright over a hiccup
    // reading replication state -- `None` just omits those gauges below.
    let replication = state.sync.replication_status().await.ok();
    let body = render(&snapshot, &crawl_counts, replication.as_ref());
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response())
}

/// `crawl_jobs`/`crawl_failures` (spec section 86) live on `CrawlerService`,
/// not `GraphService::metrics_snapshot` -- `oag-graph` has no dependency on
/// `oag-crawler` (correctly: the dependency runs the other way), so this
/// small struct is how the handler above combines both sources into one
/// rendered response.
pub struct CrawlCounts {
    pub total: u64,
    pub failed: u64,
}

/// Pure text-exposition-format rendering, kept separate from `handler` so it's
/// unit-testable against a known `MetricsSnapshot` without spinning up a
/// server. One `# HELP` + `# TYPE` + value line per metric, per the
/// Prometheus exposition format
/// (<https://prometheus.io/docs/instrumenting/exposition_formats/>).
pub fn render(
    snapshot: &MetricsSnapshot,
    crawl_counts: &CrawlCounts,
    replication: Option<&oag_sync::ReplicationStatus>,
) -> String {
    let mut out = String::new();
    push_metric(
        &mut out,
        "events_total",
        "Total number of events this peer has recorded.",
        "counter",
        snapshot.events_total,
    );
    push_metric(
        &mut out,
        "nodes_total",
        "Total number of nodes in this peer's local graph.",
        "counter",
        snapshot.nodes_total,
    );
    push_metric(
        &mut out,
        "edges_total",
        "Total number of edges in this peer's local graph.",
        "counter",
        snapshot.edges_total,
    );
    push_metric(
        &mut out,
        "assertions_total",
        "Total number of assertions this peer has ever recorded.",
        "counter",
        snapshot.assertions_total,
    );
    push_metric(
        &mut out,
        "evidence_total",
        "Total number of evidence records attached to assertions.",
        "counter",
        snapshot.evidence_total,
    );
    push_metric(
        &mut out,
        "peer_count",
        "Number of peers this peer currently knows about.",
        "gauge",
        snapshot.peer_count,
    );
    push_metric(
        &mut out,
        "sqlite_size_bytes",
        "Size in bytes of this peer's primary SQLite database (page_count * page_size).",
        "gauge",
        snapshot.sqlite_size_bytes,
    );
    push_metric(
        &mut out,
        "crawl_jobs",
        "Total crawl attempts this process has made (oag crawl, REST, or MCP), since process start.",
        "counter",
        crawl_counts.total as i64,
    );
    push_metric(
        &mut out,
        "crawl_failures",
        "Of crawl_jobs, how many failed (blocked by SSRF/robots policy, fetch error, etc.).",
        "counter",
        crawl_counts.failed as i64,
    );
    // Spec section 86's `peer_sync_lag`, in aggregate rather than per-peer
    // (a labeled metric here would be an unbounded-cardinality vector as
    // the network grows -- see OPEN_QUESTIONS.md's "Metrics" section).
    // `peer_sync_errors` still isn't covered: that needs a durable counter
    // wired through the gossip loop's own error path, not derivable from
    // this read-only status query.
    if let Some(replication) = replication {
        push_metric(
            &mut out,
            "replication_target",
            "This peer's current target replication factor (spec section 42), scaled by known_peer_count.",
            "gauge",
            replication.target_replication_factor as i64,
        );
        push_metric(
            &mut out,
            "replication_peers_caught_up",
            "Of this peer's known peers, how many have reported being fully caught up with this peer's own data.",
            "gauge",
            replication.peers_fully_caught_up as i64,
        );
        push_metric(
            &mut out,
            "replication_lagging_peers",
            "Of this peer's known peers, how many are behind (or have never reported a head at all).",
            "gauge",
            replication.lagging_peers.len() as i64,
        );
        push_metric(
            &mut out,
            "replication_meets_target",
            "1 if replication_peers_caught_up >= replication_target, else 0.",
            "gauge",
            replication.meets_target as i64,
        );
    }
    out
}

fn push_metric(out: &mut String, name: &str, help: &str, metric_type: &str, value: i64) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} {metric_type}\n"));
    out.push_str(&format!("{name} {value}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> MetricsSnapshot {
        MetricsSnapshot {
            events_total: 42,
            nodes_total: 10,
            edges_total: 5,
            assertions_total: 7,
            evidence_total: 3,
            peer_count: 2,
            sqlite_size_bytes: 45056,
        }
    }

    fn crawl_counts() -> CrawlCounts {
        CrawlCounts { total: 9, failed: 1 }
    }

    fn replication() -> oag_sync::ReplicationStatus {
        oag_sync::ReplicationStatus {
            known_peer_count: 4,
            target_replication_factor: 3,
            self_current_sequence: 10,
            peers_fully_caught_up: 3,
            meets_target: true,
            lagging_peers: vec![oag_sync::LaggingPeer { peer_id: "oagp_x".to_string(), known_sequence: Some(5) }],
        }
    }

    #[test]
    fn renders_exact_expected_text() {
        let expected = "\
# HELP events_total Total number of events this peer has recorded.
# TYPE events_total counter
events_total 42
# HELP nodes_total Total number of nodes in this peer's local graph.
# TYPE nodes_total counter
nodes_total 10
# HELP edges_total Total number of edges in this peer's local graph.
# TYPE edges_total counter
edges_total 5
# HELP assertions_total Total number of assertions this peer has ever recorded.
# TYPE assertions_total counter
assertions_total 7
# HELP evidence_total Total number of evidence records attached to assertions.
# TYPE evidence_total counter
evidence_total 3
# HELP peer_count Number of peers this peer currently knows about.
# TYPE peer_count gauge
peer_count 2
# HELP sqlite_size_bytes Size in bytes of this peer's primary SQLite database (page_count * page_size).
# TYPE sqlite_size_bytes gauge
sqlite_size_bytes 45056
# HELP crawl_jobs Total crawl attempts this process has made (oag crawl, REST, or MCP), since process start.
# TYPE crawl_jobs counter
crawl_jobs 9
# HELP crawl_failures Of crawl_jobs, how many failed (blocked by SSRF/robots policy, fetch error, etc.).
# TYPE crawl_failures counter
crawl_failures 1
# HELP replication_target This peer's current target replication factor (spec section 42), scaled by known_peer_count.
# TYPE replication_target gauge
replication_target 3
# HELP replication_peers_caught_up Of this peer's known peers, how many have reported being fully caught up with this peer's own data.
# TYPE replication_peers_caught_up gauge
replication_peers_caught_up 3
# HELP replication_lagging_peers Of this peer's known peers, how many are behind (or have never reported a head at all).
# TYPE replication_lagging_peers gauge
replication_lagging_peers 1
# HELP replication_meets_target 1 if replication_peers_caught_up >= replication_target, else 0.
# TYPE replication_meets_target gauge
replication_meets_target 1
";
        assert_eq!(render(&snapshot(), &crawl_counts(), Some(&replication())), expected);
    }

    #[test]
    fn replication_gauges_are_omitted_when_status_is_unavailable() {
        let text = render(&snapshot(), &crawl_counts(), None);
        assert!(!text.contains("replication_target"));
    }

    #[test]
    fn every_metric_line_parses_as_prometheus_syntax() {
        let text = render(&snapshot(), &crawl_counts(), Some(&replication()));
        let mut value_lines = 0;
        for line in text.lines() {
            if line.starts_with("# HELP") || line.starts_with("# TYPE") {
                continue;
            }
            let mut parts = line.split_whitespace();
            let name = parts.next().expect("value line must have a metric name");
            let value = parts.next().expect("value line must have a value");
            assert!(parts.next().is_none(), "unexpected extra token on line: {line}");
            assert!(!name.is_empty());
            value.parse::<i64>().unwrap_or_else(|e| panic!("value '{value}' for {name} did not parse as a number: {e}"));
            value_lines += 1;
        }
        assert_eq!(value_lines, 13, "expected exactly 13 in-scope metrics");
    }
}
