# Rate Limiting and Resource Limits

**Source**: `crates/oag-api/src/rate_limit.rs`, `crates/oag-sync/src/{rate_limit,client,server,service}.rs`,
`crates/oag-reticulum/src/listener.rs`, `crates/oag-crawler/src/fetch.rs`,
`crates/oag-graph/src/{search,semantic_search}.rs`.

This file consolidates every bound in the system against unbounded request volume, response size,
memory growth, and disk growth — the connective theme across the single largest block of real bugs
found and fixed in this project's history so far. The pattern each time: a threat the codebase's
own comments already named (most citing "spec section 61" — a lost document, see
[spec/README.md](README.md) — but the *threat model it described* is exactly "public-network
abuse," "disk exhaustion," "event flooding") was *partially* defended, with one boundary
genuinely uncovered. None of these were found by assumption — each was confirmed live against a
real running peer before being called a bug, and confirmed fixed live afterward.

## REST/MCP per-key rate limiting

120 requests/60s per API key (`RateLimiter`, `crates/oag-api/src/rate_limit.rs`), applied
identically to REST and MCP (see [11-mcp-server.md](11-mcp-server.md) for the layering bug that
initially let `/mcp` bypass this entirely). Keyed by the raw `Authorization` header string — a
request with no header at all shares a single `"anonymous"` bucket; a request *with* a header,
even a garbage one, gets its own bucket, since the check runs before any handler validates it.

### The limiter's own tracking map was unbounded — the most severe finding in this project

The per-key `HashMap` never evicted anything. Confirmed live: 30,000 requests, each with a
different never-valid bogus key, grew a peer's RSS from ~6 MB to ~22.7 MB with **zero valid
credentials required at any point** — the cheapest resource-exhaustion vector found in this
project, since every other one here needed at least a minimally-privileged key or an
already-admitted peer.

**Fix**: a threshold-triggered sweep — once the map exceeds 10,000 tracked keys, entries whose
window has already expired are purged, bounding steady-state size to roughly "distinct keys active
in the last window," not "every distinct key ever seen since process start." Confirmed live:
doubling the attack volume (60,000 requests) past the sweep threshold added no further measurable
growth (~23.5 MB, essentially flat).

## `oag-sync`'s own rate limiting

A single **global** (not per-key — there's no caller identity to key by on a deliberately
unauthenticated surface) fixed-window counter, 600 requests/60s (`SyncRateLimiter`). Generous
relative to legitimate traffic (default gossip interval is 30s per peer), sized to bound a genuine
flood rather than fingerprint normal usage.

## `oag-reticulum`'s listener had no rate limiting at all — found, fixed

The Reticulum listener answers the identical calls `oag-sync`'s HTTP endpoints do but had no
equivalent protection. See [15-reticulum-transport.md](15-reticulum-transport.md). Fixed by
reusing `SyncRateLimiter` directly.

## Request/response body size caps

Every HTTP boundary caps body size before fully buffering it, so no single request or response can
exhaust memory regardless of what the other side claims:

| Boundary | Cap | Direction |
|---|---|---|
| REST request bodies | 4 MiB (`MAX_REQUEST_BODY_BYTES`) | inbound |
| `oag-sync` incoming event pushes | 4 MiB (`MAX_PUSH_BODY_BYTES`) | inbound |
| Crawler fetch | streamed, never buffered whole | inbound (page content) |
| `oag-sync`'s HTTP **client** | 4 MiB, streamed | inbound (peer responses) |
| Reticulum message framing | 8 MiB declared-length check | inbound |

### `oag-sync`'s HTTP client had no cap — found, fixed

Every boundary above existed *except* the client side of `oag-sync` itself — the code this peer
uses to call another peer's `/hello`/`/heads`/`/events`/`/peers`. A peer this node syncs *with* is
exactly as untrusted as a peer syncing with it, so a malicious or compromised peer's response was
previously buffered without limit before any of this project's own item-count caps got a chance to
run. **Fix**: streamed with the same 4 MiB cap, matching the crawler's existing pattern.

## Item-count caps

| Cap | Value | Guards against |
|---|---|---|
| `MAX_PEERS_PER_RESPONSE` | 200 | One `/peers` gossip response claiming unlimited fabricated identities |
| `MAX_ADDRESSES_PER_PEER` | 5 | One peer claiming unlimited addresses |
| `MAX_EVENTS_PER_PUSH` | 1000 | One push batch containing an enormous number of individually-cheap events |
| `MAX_EVIDENCE_PER_ASSERTION` | 20 | One assertion request attaching unlimited evidence |

### No cap on *total* known peers — found, fixed

`MAX_PEERS_PER_RESPONSE` only ever bounded one gossip round, not the `peers` table's lifetime
size — the gossip loop re-syncs with every known address forever, so one malicious relay feeding
200 fresh (cheap, cryptographically valid) identities every round would grow `peers`/
`peer_addresses` on disk without bound. See [14-replication-sync.md](14-replication-sync.md).
**Fix**: a global `MAX_TOTAL_KNOWN_PEERS` (10,000) cap.

## The `limit` query parameter

SQLite treats a negative `LIMIT` as "no limit at all." Found live: `limit=-1` on `/search`
returned every matching row. See [07-search.md](07-search.md). **Fix**: clamped to `[0, 1000]` at
the one service layer every surface calls through.

## Explicitly *not* covered here, and why

Operator-configured external service integrations (the LLM extractor, the embeddings provider)
have the same "no response-size cap before `.json()`" shape as the sync client did, but were
deliberately left as-is: these require an operator to explicitly choose and enable the
integration, unlike every finding above, which was reachable by an already-admitted but otherwise
unprivileged peer or caller. A different, lower trust tier — checked, not assumed, and recorded in
`OPEN_QUESTIONS.md` rather than silently skipped.

## Tests

`crates/oag-api/src/rate_limit.rs` (`stale_entries_are_swept_once_the_map_grows_past_the_threshold`);
`crates/oag-sync/src/client.rs` (`fetch_peers_rejects_an_oversized_response_instead_of_buffering_it`);
`crates/oag-sync/src/tests.rs` (`discover_peers_does_not_grow_the_table_past_the_global_cap`,
`push_event_count_over_limit_is_rejected`, `rate_limiter_rejects_after_threshold`);
`crates/oag-reticulum/src/listener.rs` (`requests_beyond_the_window_get_a_rate_limit_error_instead_of_being_handled`);
`crates/oag-graph/src/tests.rs` (`search_with_a_negative_limit_returns_nothing_not_everything`,
`too_many_evidence_items_is_rejected`); `crates/oag-reticulum/src/framing.rs`
(`oversized_declared_length_is_rejected`).
