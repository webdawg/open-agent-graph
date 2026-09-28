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
    let body = render(&snapshot);
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response())
}

/// Pure text-exposition-format rendering, kept separate from `handler` so it's
/// unit-testable against a known `MetricsSnapshot` without spinning up a
/// server. One `# HELP` + `# TYPE` + value line per metric, per the
/// Prometheus exposition format
/// (<https://prometheus.io/docs/instrumenting/exposition_formats/>).
pub fn render(snapshot: &MetricsSnapshot) -> String {
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
";
        assert_eq!(render(&snapshot()), expected);
    }

    #[test]
    fn every_metric_line_parses_as_prometheus_syntax() {
        let text = render(&snapshot());
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
        assert_eq!(value_lines, 7, "expected exactly 7 in-scope metrics");
    }
}
