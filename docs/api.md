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
| GET | `/actors/{id}` | `graph:read` | Get an actor by id |
| GET | `/nodes/{id}/edges` | `graph:read` | List edges touching a node |
| GET | `/nodes/{id}/assertions` | `graph:read` | List assertions on any edge touching a node |
| GET | `/nodes/{id}/sources` | `graph:read` | All evidence backing any assertion whose edge touches this node (explainability, spec section 81) |
| GET | `/subgraph?node=&depth=` | `graph:read` | Compact neighborhood traversal around a node |
| GET | `/assertions/{id}` | `graph:read` | Get one assertion plus its evidence/disputes/retractions/observations |
| POST | `/assertions` | `graph:assert` | Create an assertion, optionally with evidence attached |
| POST | `/assertions/{id}/evidence` | `graph:assert` | Attach evidence to an existing assertion |
| POST | `/assertions/{id}/verify` | `graph:verify` | Record a verification observation |
| POST | `/assertions/{id}/dispute` | `graph:assert` | Dispute an assertion (disputing is itself a claim about a claim, same permission as making one) |
| POST | `/assertions/{id}/retract` | `graph:retract-own` | Retract your own assertion |
| POST | `/assertions/{id}/supersede` | `graph:assert` | Flag an assertion as superseded by a newer one (spec section 36) -- both must already exist |
| GET | `/edges/{id}` | `graph:read` | Get an edge (subject/predicate/object) by id |
| GET | `/edges/{id}/corroboration` | `graph:read` | Every ranking signal for one edge (see `docs/data-model.md`) |
| GET | `/history/{object_type}/{id}` | `graph:read` | Full event history for a node/edge/assertion |
| POST | `/crawl` | `graph:crawl` | Crawl one URL and assert what's found (spec section 71) -- see below |
| POST | `/evidence/{id}/redact` | `admin` | Permanently blank one evidence item's `title`/`excerpt` locally (spec section 85) -- see below |
| GET | `/redactions` | `admin` | List every redaction tombstone, every currently search-suppressed node, and every actor identity_uri redaction |
| POST | `/nodes/{id}/suppress` | `admin` | Hide a node from search results (reversible, doesn't touch the node/edges/assertions) |
| POST | `/nodes/{id}/unsuppress` | `admin` | Reverse `/nodes/{id}/suppress` |
| POST | `/actors/{id}/redact-identity-uri` | `admin` | Permanently blank one actor's `identity_uri` locally (spec section 85) -- see below |
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

### `/evidence/{id}/redact`, `/redactions`, `/nodes/{id}/suppress|unsuppress`, `/actors/{id}/redact-identity-uri` (spec section 85)

`POST /evidence/{id}/redact { "reason": "..." }` (reason optional) permanently blanks that
evidence row's `title`/`excerpt` in this peer's own local database -- irreversible at the tooling
level, and unable to reach any copy already replicated to another peer. Returns `409 Conflict`
(`AlreadyRedacted`) on a repeat call against the same id, never a silent no-op. `POST
/actors/{id}/redact-identity-uri { "reason": "..." }` is the exact same shape for one actor's
`identity_uri` field instead (`public_key`/`name`/`actor_type` untouched; `409 Conflict`/
`ActorAlreadyRedacted` on a repeat call). All five routes require the `admin` permission, the same
`Permission::Admin` check `oag redact *` has always run against a synthetic local-operator
`AuthContext` -- these routes reuse the identical `GraphService` methods with a real authenticated
actor instead, adding no new business logic. `GET /redactions` returns
`{"redactions": [...], "suppressed_nodes": [...], "actor_redactions": [...]}`, the same combined
shape `oag redact list` prints. `POST /nodes/{id}/suppress` / `.../unsuppress` are both idempotent
no-ops when the node is already in (or already out of) that state.

### `/metrics` (spec section 86)

Hand-rolled Prometheus exposition format, no external metrics platform required:

```
events_total, nodes_total, edges_total, assertions_total, evidence_total   (counters)
crawl_jobs, crawl_failures                                                 (counters, process-lifetime)
peer_sync_errors                                                           (counter, process-lifetime)
peer_count, sqlite_size_bytes                                              (gauges)
replication_target, replication_peers_caught_up,
replication_lagging_peers, replication_meets_target                       (gauges; aggregate, not per-peer)
```

The `replication_*` gauges are derived from the same computation `oag replication status` /
`GET /oag/sync/v1/replication-status` use, aggregated rather than broken out per peer (a per-peer
label set would be an unbounded-cardinality vector as the network grows). Omitted from the response
entirely (not zeroed) if that query fails.

`peer_sync_errors` counts whole-attempt sync failures (`SyncService::sync_with_peer` itself
returning `Err`) against any peer, across both the standard and cluster-only gossip loops --
not a partial per-event failure inside an otherwise-successful sync round. Unlike every other
counter above, it's shared from the exact `SyncService` instance the real gossip loop runs,
not computed from a read-only query, since no snapshot query can see it.

`events_by_origin`, request-latency histograms, `replication_bytes_in`/`out`, and verification job
counters are not yet implemented — see `OPEN_QUESTIONS.md`'s "Metrics" section for the exact scope
decision.

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
| `GET /ui/nodes/{id}?key=` | Name, type, canonical identifier, aliases, relationships, assertions (with their evidence/disputes/retraction/last observation), history |
| `GET /ui/assertions/{id}?key=` | Subject/predicate/object, actor, origin peer, event id, signature status, evidence, verification, disputes, retraction, supersession |

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
| `graph_get_actor` | `GET /actors/{id}` |
| `graph_get_edges` | `GET /nodes/{id}/edges` |
| `graph_get_edge` | `GET /edges/{id}` |
| `graph_get_corroboration` | `GET /edges/{id}/corroboration` |
| `graph_get_subgraph` | `GET /subgraph` |
| `graph_find_sources` | `GET /nodes/{id}/sources` |
| `graph_get_assertion` | `GET /assertions/{id}` |
| `graph_get_node_assertions` | `GET /nodes/{id}/assertions` |
| `graph_assert` | `POST /assertions` |
| `graph_add_evidence` | `POST /assertions/{id}/evidence` |
| `graph_verify_assertion` | `POST /assertions/{id}/verify` |
| `graph_dispute_assertion` | `POST /assertions/{id}/dispute` |
| `graph_retract_assertion` | `POST /assertions/{id}/retract` |
| `graph_supersede_assertion` | `POST /assertions/{id}/supersede` |
| `graph_get_history` | `GET /history/{object_type}/{id}` |
| `graph_crawl` | `POST /crawl` -- also requires `graph:crawl`, not just an authenticated key |
| `graph_redact_evidence` | `POST /evidence/{id}/redact` -- also requires `admin`, not just an authenticated key |
| `graph_list_redactions` | `GET /redactions` -- also requires `admin`, not just an authenticated key |
| `graph_suppress_node` | `POST /nodes/{id}/suppress` -- also requires `admin`, not just an authenticated key |
| `graph_unsuppress_node` | `POST /nodes/{id}/unsuppress` -- also requires `admin`, not just an authenticated key |
| `graph_redact_actor_identity_uri` | `POST /actors/{id}/redact-identity-uri` -- also requires `admin`, not just an authenticated key |

## CLI (`oag`)

Every subcommand accepts `--data-dir` (default `./data`); network-facing ones also accept
`--config`. Run `oag <command> --help` for exact flags.

| Command | Purpose |
|---|---|
| `oag serve` | Start this peer's REST + MCP + sync server |
| `oag status` | This peer's identity and basic status |
| `oag search` | Search the local graph (`--semantic` to rank by embedding similarity) |
| `oag node get <id>` | Get a node |
| `oag actor get <id>` | Get an actor |
| `oag assertion get <id>` | Get an assertion with its evidence |
| `oag edge get <id>` | Get an edge |
| `oag edge corroboration <id>` | Ranking signals for one edge |
| `oag identity show/backup/restore` | Peer identity lifecycle — see `docs/security.md` |
| `oag key create` | Issue a new API key scoped to specific permissions |
| `oag key list` | List every key ever issued (active and revoked), by hash -- never the raw key |
| `oag key revoke <hash>` | Revoke a key by the hash `oag key list` shows for it |
| `oag peer add/list/remove/sync` | Manage known peer addresses and trigger one-shot sync |
| `oag peer forks <peer_id>` | Show recorded fork evidence (spec section 55) -- both conflicting event ids, plus the rejected event's full signed JSON |
| `oag peer reticulum-address` | Print this peer's Reticulum destination address, to share out-of-band |
| `oag peer add-reticulum <address_hash> <via_tcp>` | One-shot sync with a peer reachable over Reticulum instead of HTTP |
| `oag peer resolve <peer_id>` | Resolve a peer's long-word DNS address via a real TXT lookup (`spec/30-long-word-addressing.md`) |
| `oag peer fetch <address> <server_peer_id> <cid> [--out <path>] [--max-hops <n>]` | Fetch pinned content directly from another peer, verify its mutual nonce-bound receipt, and record a corroborating `sent_to` assertion locally (`spec/31-service-receipts.md`). `--max-hops` (default `8`) permits `address` to relay the fetch onward to its own configured peers if it doesn't have the content locally (Phase 2) |
| `oag replication status` | Durability view — see `docs/replication.md` |
| `oag authority recompute` | Batch-recompute the PageRank-style `authority` signal |
| `oag embeddings recompute` | Batch-recompute node embeddings for semantic search |
| `oag tensor show` | This peer's current tensor pad values (`spec/23-tensor-pad-and-evolution-layer.md`) |
| `oag gravity show` | This peer's gravity level, rolling and persisting one on first use (`spec/25-gravity.md`) |
| `oag backup <path> [--pin]` | Consistent whole-database snapshot (`VACUUM INTO`), optionally pinned to IPFS (`spec/29-ipfs-pinning.md`) |
| `oag rebuild` | Wipe and replay every derived table from the event log |
| `oag redact evidence/list/suppress-node/unsuppress-node/actor-identity-uri` | Deletion and redaction — see `docs/security.md` (also reachable remotely as `POST /evidence/{id}/redact`, `GET /redactions`, `POST /nodes/{id}/suppress|unsuppress`, `POST /actors/{id}/redact-identity-uri` / `graph_redact_evidence`/`graph_list_redactions`/`graph_suppress_node`/`graph_unsuppress_node`/`graph_redact_actor_identity_uri`, all requiring `admin`) |
| `oag migrate node/edge/assertion/evidence/actor <id> [--force]` | Permanently rewrite one artifact's row to its current schema shape (`spec/27-artifact-versioning.md`) |
| `oag crawl <url>` | Fetch a URL, extract structured facts, assert them as evidence-backed claims (also available remotely as `POST /crawl` / `graph_crawl`, requiring `graph:crawl`) |
| `oag ipfs cat <cid>` | Retrieve previously-pinned content by CID (`spec/29-ipfs-pinning.md`) |
| `oag doctor [--config]` | Local health checks -- data dir, identity, database/migrations; validates `config.toml` (via the exact same resolver `oag serve` uses) if `--config` is given |

## Configuration (`config.toml`, spec section 94)

Sections: `[data]`, `[server]`, `[search]` (semantic search / embeddings, disabled by default),
`[mcp]`, `[network]` (bootstrap peers, sync interval), `[federation]` (`open` or `allowlist` — see
`docs/security.md`), `[crawler]` (SSRF defaults, LLM extraction config, disabled by default),
`[reticulum]` (optional additional transport, disabled by default), `[identity]` (opt-in ephemeral
per-restart identity + trust windows, `spec/24-ephemeral-peer-trust.md`), `[cluster]` (opt-in
signed cluster membership + its own gossip cadence, `spec/26-peer-clustering.md`), `[ipfs]` (opt-in
pinning provider, disabled by default, `spec/29-ipfs-pinning.md`), `[addressing]` (long-word DNS
addressing domain, unset by default, `spec/30-long-word-addressing.md`), `[transfer]` (multi-hop
relay peers for `POST /oag/transfer/v1/fetch`, empty by default, `spec/31-service-receipts.md`
Phase 2). Every section has a safe default; an empty or absent `config.toml` is a fully functional
single-peer, no-external-service configuration. `OAG_<SECTION>_<KEY>` environment variables
override file values.
