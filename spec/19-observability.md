# Observability

**Source**: `crates/oag-api/src/{metrics,logging}.rs`.

## Metrics

`GET /metrics` (top-level, not under `/api/v1` — matching Prometheus's own convention) exposes a
small, hand-rolled Prometheus text-format endpoint: event/node/edge/assertion/evidence counts, peer
count, database size, replication gauges (omitted when replication status is unavailable rather
than reported as a misleading zero), `crawls_total`/`crawls_failed`, and the process-lifetime
verification counters `verification_jobs`/`verification_failures` (counted per `verify_assertion`
call, including rejected inputs -- a `COUNT(*)` cannot produce them, since rejected attempts never
become events). No external metrics platform
required — needs no auth, matching `/status`, since it's a conventional scrape-target posture.

## Structured logging

Every REST request and MCP tool call gets a structured `tracing` span.

- **REST** (`ApiMakeSpan`): `request_id` (process pid + monotonic counter, not a UUID — only needs
  to be unique within one peer's own logs), `peer_id`, `route` (path *template*, never the full
  query string), `method`, `actor_id` (recorded once auth resolves it, present regardless of how
  the request finishes), `status`, `duration_ms`, `result`.
- **MCP** (`self.traced`): its own independent span per tool call (`mcp_tool` with
  `tool`/`peer_id`/`actor_id`, `duration_ms`/`result`) — not reliant on REST's `TraceLayer`, which
  is why MCP calls are correctly logged despite not sharing that specific middleware.
- **Replication**: every ingested event logs `event_id`/`origin_peer`/`result`.

### Why `route` logs the path template, not the full query string

The Human Interface's `?key=` query parameter (see
[13-human-interface.md](13-human-interface.md)) would otherwise leak a live API key into this
project's own logs. Checked deliberately, not assumed: `route = %request.uri().path()` only ever
captures the path, confirmed by reading the actual field construction rather than trusting the
architectural description of it.

## What's deliberately scoped out

Per-event fields (`event_id`/`origin_peer`) on the *REST* span — relevant to replication, not a
single REST request, so they live on the replication log line instead, not duplicated onto every
REST span.

## Tests

`crates/oag-api/src/metrics.rs` (`renders_exact_expected_text`,
`replication_gauges_are_omitted_when_status_is_unavailable`,
`every_metric_line_parses_as_prometheus_syntax`); `crates/oag-graph/src/tests.rs`
(`verification_counters_count_jobs_and_failures`, which checks a success, an unknown assertion id,
and a key without `graph:verify` each count once); `crates/oag-api/src/logging.rs`
(`request_ids_are_unique_and_stable_shape`).
