# API Reference

OAG exposes the same `GraphService` capabilities four ways: REST, MCP, the `oag` CLI, and a minimal
human-browsable HTML view. All four share one service layer (`docs/architecture.md`) — nothing is
implemented in only one of them by accident; a capability missing from one surface is a deliberate,
documented scope decision (see `OPEN_QUESTIONS.md`).

## REST (`/api/v1/*`, spec section 66)

Base URL: `http://<listen-address>/api/v1`. Auth: `Authorization: Bearer <api-key>` unless noted.

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/search?q=&limit=&semantic=` | `graph:read` | Keyword (or `semantic=true` embedding-ranked) node search |
| POST | `/resolve` | `graph:read` | Resolve an identifier string to its node, if one exists |
| GET | `/nodes/{id}` | `graph:read` | Get a node by id |
| GET | `/nodes/{id}/edges` | `graph:read` | List edges touching a node |
| GET | `/nodes/{id}/assertions` | `graph:read` | List assertions on any edge touching a node |
| GET | `/subgraph?node=&depth=` | `graph:read` | Compact neighborhood traversal around a node |
| GET | `/assertions/{id}` | `graph:read` | Get one assertion plus its evidence/disputes/observations |
| POST | `/assertions` | `graph:assert` | Create an assertion, optionally with evidence attached |
| POST | `/assertions/{id}/evidence` | `graph:assert` | Attach evidence to an existing assertion |
| POST | `/assertions/{id}/verify` | `graph:verify` | Record a verification observation |
| POST | `/assertions/{id}/dispute` | `graph:verify` | Dispute an assertion |
| POST | `/assertions/{id}/retract` | `graph:retract-own` | Retract your own assertion |
| GET | `/edges/{id}/corroboration` | `graph:read` | Every ranking signal for one edge (see `docs/data-model.md`) |
| GET | `/history/{object_type}/{id}` | `graph:read` | Full event history for a node/edge/assertion |
| POST | `/crawl` | `graph:crawl` | Crawl one URL and assert what's found (spec section 71) -- see below |
| GET | `/status` | none | Peer id, identity, basic status |
| GET | `/metrics` (top-level, not under `/api/v1`) | none | Prometheus text-format metrics (spec section 86) |

### `/crawl` (spec section 71)

`POST { "url": "https://..." }` triggers this peer's own crawler and returns the same summary
`oag crawl` prints (facts asserted, aliases declared, llms.txt/ARD/A2A found). Gated by
`graph:crawl`, a permission distinct from `graph:assert` -- crawling makes *this peer* issue an
outbound HTTP request to a caller-supplied URL, a meaningfully different risk than authoring a
claim, so an operator must grant it explicitly (`oag key create --permission graph:crawl`). There is
no `allow_private_networks` request field: a REST/MCP caller can never widen this peer's own
SSRF policy for their request -- only `[crawler].allow_private_networks` in `config.toml` can, and
that has nothing to do with any individual request's permissions.

### `/metrics` (spec section 86)

Hand-rolled Prometheus exposition format, no external metrics platform required:

```
events_total, nodes_total, edges_total, assertions_total, evidence_total   (counters)
crawl_jobs, crawl_failures                                                 (counters, process-lifetime)
peer_count, sqlite_size_bytes                                              (gauges)
replication_target, replication_peers_caught_up,
replication_lagging_peers, replication_meets_target                       (gauges; aggregate, not per-peer)
```

The `replication_*` gauges are derived from the same computation `oag replication status` /
`GET /oag/sync/v1/replication-status` use, aggregated rather than broken out per peer (a per-peer
label set would be an unbounded-cardinality vector as the network grows). Omitted from the response
entirely (not zeroed) if that query fails.

`events_by_origin`, request-latency histograms, `peer_sync_errors`, `replication_bytes_in`/`out`, and
verification job counters are not yet implemented — see `OPEN_QUESTIONS.md`'s "Metrics" section for
the exact scope decision.

### Request logging (spec section 87)

Every REST request gets a structured `tracing` span: `request_id`, `peer_id`, `route`, `method`,
`actor_id` (once auth resolves it), `status`, `duration_ms`, `result`. Set `RUST_LOG=info` (or finer)
to see it. MCP tool calls get the equivalent `mcp_tool` span (see below); `oag-sync`'s replication
path logs `event_id`/`origin_peer` on every ingested event (`docs/replication.md`).

### Replication API

`/oag/sync/v1/*` is a separate, unauthenticated router — see `docs/replication.md` for its full
endpoint list and rationale.

## Human Interface (`/ui/*`, spec section 80)

A minimal, human-browsable HTML view — secondary to the APIs, for making provenance visually
inspectable rather than for general browsing. Reuses the exact same `graph:read` API keys as REST,
accepted as a `?key=` query parameter since a plain browser link can't set an `Authorization`
header (see `docs/security.md`).

| Path | Purpose |
|---|---|
| `GET /ui/search?key=&q=` | Landing page: a search box over `GraphService::search`, results linking into node pages |
| `GET /ui/nodes/{id}?key=` | Name, type, canonical identifier, aliases, relationships, assertions (with their evidence/disputes/last observation), history |
| `GET /ui/assertions/{id}?key=` | Subject/predicate/object, actor, origin peer, event id, signature status, evidence, verification, disputes, supersession |

Rendered via `askama` compile-time HTML templates with default autoescaping — every field is HTML-
escaped, since several of them (crawled page titles, evidence excerpts, actor names) can contain
attacker/crawler-controlled text. No CSS, no pagination — see `OPEN_QUESTIONS.md`'s "Human Interface"
section for the exact scope decisions.

## MCP tools

Mounted at `/mcp` on the same `oag serve` process, same `Authorization: Bearer` auth as REST, one
tool per `GraphService` capability:

| Tool | Equivalent to |
|---|---|
| `graph_search` | `GET /search` |
| `graph_resolve` | `POST /resolve` |
| `graph_get_node` | `GET /nodes/{id}` |
| `graph_get_edges` | `GET /nodes/{id}/edges` |
| `graph_get_corroboration` | `GET /edges/{id}/corroboration` |
| `graph_get_subgraph` | `GET /subgraph` |
| `graph_find_sources` | No REST equivalent — evidence backing any assertion whose edge touches a node |
| `graph_assert` | `POST /assertions` |
| `graph_add_evidence` | `POST /assertions/{id}/evidence` |
| `graph_verify_assertion` | `POST /assertions/{id}/verify` |
| `graph_dispute_assertion` | `POST /assertions/{id}/dispute` |
| `graph_retract_assertion` | `POST /assertions/{id}/retract` |
| `graph_get_history` | `GET /history/{object_type}/{id}` |
| `graph_crawl` | `POST /crawl` -- also requires `graph:crawl`, not just an authenticated key |

## CLI (`oag`)

Every subcommand accepts `--data-dir` (default `./data`); network-facing ones also accept
`--config`. Run `oag <command> --help` for exact flags.

| Command | Purpose |
|---|---|
| `oag serve` | Start this peer's REST + MCP + sync server |
| `oag status` | This peer's identity and basic status |
| `oag search` | Search the local graph (`--semantic` to rank by embedding similarity) |
| `oag node get <id>` | Get a node |
| `oag assertion get <id>` | Get an assertion with its evidence |
| `oag edge corroboration <id>` | Ranking signals for one edge |
| `oag identity show/backup/restore` | Peer identity lifecycle — see `docs/security.md` |
| `oag key create` | Issue a new API key scoped to specific permissions |
| `oag key list` | List every key ever issued (active and revoked), by hash -- never the raw key |
| `oag key revoke <hash>` | Revoke a key by the hash `oag key list` shows for it |
| `oag peer add/list/remove/sync` | Manage known peer addresses and trigger one-shot sync |
| `oag replication status` | Durability view — see `docs/replication.md` |
| `oag authority recompute` | Batch-recompute the PageRank-style `authority` signal |
| `oag embeddings recompute` | Batch-recompute node embeddings for semantic search |
| `oag backup <path>` | Consistent whole-database snapshot (`VACUUM INTO`) |
| `oag rebuild` | Wipe and replay every derived table from the event log |
| `oag redact evidence/list/suppress-node/unsuppress-node` | Deletion and redaction — see `docs/security.md` |
| `oag crawl <url>` | Fetch a URL, extract structured facts, assert them as evidence-backed claims (also available remotely as `POST /crawl` / `graph_crawl`, requiring `graph:crawl`) |
| `oag doctor [--config]` | Local health checks -- data dir, identity, database/migrations; validates `config.toml` (via the exact same resolver `oag serve` uses) if `--config` is given |

## Configuration (`config.toml`, spec section 94)

Sections: `[data]`, `[server]`, `[search]` (semantic search / embeddings, disabled by default),
`[mcp]`, `[network]` (bootstrap peers, sync interval), `[federation]` (`open` or `allowlist` — see
`docs/security.md`), `[crawler]` (SSRF defaults, LLM extraction config, disabled by default),
`[reticulum]` (optional additional transport, disabled by default). Every section has a safe
default; an empty or absent `config.toml` is a fully functional single-peer, no-external-service
configuration. `OAG_<SECTION>_<KEY>` environment variables override file values.
