# REST API

**Source**: `crates/oag-api/src/{lib,handlers,crawl,human,auth,dto,state,rate_limit,logging,metrics}.rs`.

Base URL: `http://<listen-address>/api/v1` (plus `/metrics` at top level, and `/ui/*` — see
[13-human-interface.md](13-human-interface.md)). Auth: `Authorization: Bearer <api-key>` unless
noted. REST, MCP, CLI, and the Human Interface all share one service layer (`GraphService`) —
nothing is implemented in only one of them by accident; a capability missing from one surface is a
deliberate, documented scope decision (see `OPEN_QUESTIONS.md`), verified by an explicit
cross-surface audit during this session that found and closed every unintentional gap.

## Routes

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/search?q=&limit=&semantic=` | `graph:read` | Keyword (or `semantic=true`) node search |
| POST | `/resolve` | `graph:read` | Resolve an identifier string to its node |
| GET | `/nodes/{id}` | `graph:read` | Get a node by id |
| GET | `/actors/{id}` | `graph:read` | Get an actor by id |
| GET | `/nodes/{id}/edges` | `graph:read` | List edges touching a node |
| GET | `/nodes/{id}/assertions` | `graph:read` | List assertions on any edge touching a node |
| GET | `/nodes/{id}/sources` | `graph:read` | Explainability: all evidence backing any assertion touching this node |
| GET | `/subgraph?node=&depth=` | `graph:read` | Compact neighborhood traversal |
| GET | `/assertions/{id}` | `graph:read` | One assertion plus evidence/disputes/retractions/observations |
| POST | `/assertions` | `graph:assert` | Create an assertion, optionally with inline evidence |
| POST | `/assertions/{id}/evidence` | `graph:assert` | Attach evidence to an existing assertion |
| POST | `/assertions/{id}/verify` | `graph:verify` | Record a verification observation |
| POST | `/assertions/{id}/dispute` | `graph:assert` | Dispute an assertion |
| POST | `/assertions/{id}/retract` | `graph:retract-own` | Retract your own assertion |
| POST | `/assertions/{id}/supersede` | `graph:assert` | Flag an assertion as superseded by a newer one |
| GET | `/edges/{id}` | `graph:read` | Get an edge by id |
| GET | `/edges/{id}/corroboration` | `graph:read` | Every ranking signal for one edge |
| GET | `/history/{object_type}/{id}` | `graph:read` | Full event history for a node/edge/assertion |
| POST | `/crawl` | `graph:crawl` | Crawl one URL and assert what's found |
| GET | `/status` | none | Peer id, basic status |
| GET | `/metrics` (top-level) | none | Prometheus text-format metrics |

## Request body limit

`RequestBodyLimitLayer` caps every request body at 4 MiB (`MAX_REQUEST_BODY_BYTES`). See
[18-rate-limiting-and-resource-limits.md](18-rate-limiting-and-resource-limits.md).

## `/crawl`

`POST { "url": "https://..." }` triggers this peer's own crawler and returns the same summary
`oag crawl` prints. Gated by `graph:crawl`, distinct from `graph:assert` — crawling makes *this
peer* issue an outbound HTTP request to a caller-supplied URL, a meaningfully different risk than
authoring a claim. There is no `allow_private_networks` request field — only
`[crawler].allow_private_networks` in `config.toml` can widen the SSRF policy, and that's an
operator's own deploy-time choice, never something a caller's request can influence.

## Rate limiting on this surface

Every route goes through a per-API-key fixed-window limiter (120 req/60s) — see
[18-rate-limiting-and-resource-limits.md](18-rate-limiting-and-resource-limits.md) for the limiter
itself and a serious bug found in its own tracking map.

## Structured logging

Every REST request gets one `tracing` span: `request_id`, `peer_id`, `route` (path template, never
the full query string — so even the Human Interface's `?key=` doesn't leak into logs),
`method`, `actor_id` (recorded once auth resolves it, present regardless of how the request
finishes), `status`, `duration_ms`, `result`. See [19-observability.md](19-observability.md).

## Tests

`crates/oag-api/src/tests.rs` — `full_rest_vertical_slice` is the primary end-to-end coverage
(assert → evidence → verify → dispute → retract → history → final read-back, including retraction
detail); individual tests for auth rejection, oversized bodies, metrics, crawl permission gating,
the `/sources` explainability endpoint, and `/mcp` rate limiting (see
[11-mcp-server.md](11-mcp-server.md)).
