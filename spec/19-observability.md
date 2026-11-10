# Observability

**Source**: `crates/oag-api/src/{metrics,logging}.rs`.

## Metrics

`GET /metrics` (top-level, not under `/api/v1` — matching Prometheus's own convention) exposes a
small, hand-rolled Prometheus text-format endpoint: event/node/edge/assertion/evidence counts, peer
count, database size, replication gauges (omitted when replication status is unavailable rather
than reported as a misleading zero), `crawls_total`/`crawl_failures`, the process-lifetime
verification counters `verification_jobs`/`verification_failures` (counted per `verify_assertion`
call, including rejected inputs -- a `COUNT(*)` cannot produce them, since rejected attempts never
become events), `events_pending_chain` (events whose predecessor is not present locally -- the one
metric here that distinguishes a complete chain from a pile of fragments, which `events_total`
cannot see), `replication_bytes_in`/`replication_bytes_out` (bytes this peer moved from/to peers
during replication, counted on the real gossip loop's client), and two duration histograms:
`api_latency` (milliseconds per REST request, recorded
by an HTTP-layer middleware that sees routing, auth, handler, and response as a whole) and
`search_latency` (milliseconds per search, recorded inside `GraphService` so REST, MCP, CLI, and
the Human Interface all measure the same thing, semantic search included). No external metrics platform
required — needs no auth, matching `/status`, since it's a conventional scrape-target posture.

### Why there are histograms at all

Every other metric here is a *count*, and a count answers "how much", not "how slow". Per-request
duration was already on every REST/MCP span as `duration_ms`, which is right for reading one request
in a log and useless for answering "is this peer getting slower?" across a million of them. A log
line is not an aggregate.

Values are **milliseconds**, matching `duration_ms` on the spans, and each `# HELP` line says so
rather than encoding the unit in the name. Buckets are fixed at 1/5/10/25/50/100/250/500/1000/
2500/5000/10000ms plus `+Inf`: a histogram whose boundaries can change at runtime cannot be compared
across scrapes, which defeats the point.

`search_latency` includes the embedding provider call for semantic search -- from a caller's point
of view that *is* the search latency, and excluding it would report semantic search as faster than
it is. `api_latency` deliberately overlaps it for a search request: one measures the request, the
other the search part.

### `replication_bytes_in`/`out`

`SyncClient` counts the streamed bodies of every replication read (`hello`/`heads`/`events`/
`peers`) into `bytes_in`, and each serialized POST body (`presence`, `cluster`) into `bytes_out`.
Both are process-lifetime counters on the client, shared by `Arc` so a metrics layer can hold the
exact ones the gossip loop increments.

**Why the sharing is needed.** `AppState` holds a *separately constructed* `SyncService` that never
runs the sync loop, so its own client's counters would always read zero -- the same trap
`peer_sync_errors` already documents. `oag serve` therefore hands `AppState` the real service's
counters at startup, exactly as it does for `sync_errors`.

**What is not counted, deliberately.** `bytes_in` covers streamed *read* bodies. The tiny unread
responses to `presence`/`cluster` POSTs are not counted, because reading them would mean buffering
a body on a fire-and-forget path for the sake of a few bytes. The `# HELP` line says so rather than
leaving the boundary to be discovered.

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
and a key without `graph:verify` each count once;
`search_latency_records_every_search_this_process_serves`, which checks an empty result is still
measured); `crates/oag-graph/src/latency.rs` (bucket boundaries, cumulative bucket counts, clamping
a negative duration, quantiles read off the buckets, shared `Arc` handles);
`crates/oag-storage/src/repo/stats.rs`
(`pending_chain_counts_events_whose_predecessor_is_missing`, which inserts a genesis event, a child
with its predecessor present, and a child whose predecessor never arrives -- the state normal ingest
never produces, so it is built by hand);
`crates/oag-sync/src/client.rs` (`replication_bytes_counters_move_with_real_traffic`, which
fetches a real response and sends a real heartbeat against a fixture server and asserts both
counters moved);
`crates/oag-api/src/tests.rs` (`api_latency_counts_rest_requests_end_to_end`, which serves a real
request and confirms the histogram counted it -- a unit test of the histogram would not prove the
middleware is wired to it); `crates/oag-api/src/logging.rs`
(`request_ids_are_unique_and_stable_shape`).
