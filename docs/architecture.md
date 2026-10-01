# Architecture

## Mission, in one paragraph

OAG is an open, distributed, evidence-backed semantic graph for AI agents. Every peer is a single,
self-contained Rust binary embedding its own SQLite database — no Postgres, no Redis, no Kafka, no
required external service (spec section 11). Contributions are signed, append-only events, not
direct table writes. Claims are assertions backed by evidence and provenance, never bare
declarations of truth (spec sections 21-22). There is no global consensus, no leader election, and
no requirement that every peer agree — each peer maintains its own locally-consistent view, built by
replaying the events it has received (spec sections 5-7).

## Legacy compatibility

A permanent, binding principle (see `USER_INPUT_RECORD.md`, Entry 1): **OAG never becomes its own
gatekeeper.** No future version is allowed to require that old data, old clients, or old protocols
be upgraded or abandoned to keep working — legacy must always be allowed to flow. This is already
true by construction in several places: `Predicate` is an open string, not a closed enum a future
version could outgrow (spec section 20); `canonicalize_value` passes an already-scheme-prefixed
identifier through unchanged rather than forcing it into a house format; the event log never
deletes or rewrites anything (spec section 106 invariant 3), so nothing a peer has ever accepted can
later be invalidated by a protocol change. Any future schema or protocol evolution must add a new
path alongside the old one, never replace it — see "What's deliberately not built yet" below for
the granular data-versioning mechanism this implies.

## One program, many peers

`oag` is one binary with subcommands (`serve`, `search`, `crawl`, `backup`, `rebuild`, `redact`,
...) — there is no separate server/client/worker build. Running `oag serve` starts one peer: a REST
API, an MCP server, and an `oag-sync` replication endpoint, all sharing one `GraphService` and one
SQLite connection pool against one data directory.

## Crate map

```
oag-core        Pure data types: Node, Edge, Assertion, Evidence, Actor, Predicate, Permission,
                domain-separated identifiers (NodeId/EdgeId/EventId/ActorId), canonical JSON.
                No I/O, no async, no dependencies on any other oag-* crate.

oag-crypto      Ed25519 signing/verification with domain separation, PeerIdentity (the
                identity.key lifecycle), PeerId.

oag-storage     SQLite schema (migrations/), connection pooling, and one `repo::` module per
                table family — the only crate that writes raw SQL.

oag-events      The signed event envelope, canonical serialization, per-peer hash-chain
                validation, commit (local) / ingest (remote) paths, and the projector that turns
                a validated event into the storage-layer writes that represent it.

oag-graph       The service layer: GraphService, one method per capability (assert, resolve,
                search, semantic_search, get_edge_corroboration, recompute_authority,
                rebuild_projection, redact_evidence, ...). This is what oag-api, oag-mcp, and
                oag-cli all call — none of them talk to oag-storage or oag-events directly.

oag-sync        Peer-to-peer replication: hello/heads/events/peers HTTP endpoints, the gossip
                (anti-entropy) loop, federation policy, replication-factor/durability tracking.

oag-embeddings  EmbeddingProvider abstraction (disabled by default) + brute-force cosine
                similarity ranking, backing semantic search.

oag-crawler     Safe HTTP fetch (SSRF-guarded), structured extraction (JSON-LD, ARD, A2A,
                llms.txt, HTML meta), and LLM-extraction fallback — turns a crawled page into
                the same signed events any other caller would produce.

oag-petname     Deterministic 128-word human-readable names derived from a peer's identity, for
                display purposes only (never used as an identifier).

oag-reticulum   Optional additional peer transport over the Reticulum mesh-networking protocol,
                alongside (not replacing) oag-sync's HTTP path.

oag-api         Axum REST router + MCP mount point, a minimal human-browsable HTML view (spec
                section 80), request logging, rate limiting.

oag-mcp         MCP tool definitions, one per GraphService capability exposed to agents.

oag-cli         Argument parsing and the `oag` binary's `main()` — thin wrappers over
                GraphService/CrawlerService/SyncService.
```

Every arrow in this system points one direction: `oag-cli`/`oag-api`/`oag-mcp` → `oag-graph` →
(`oag-events` + `oag-storage`) → SQLite. There is no path that writes to storage without going
through `oag-events`' commit/ingest functions, and no path that reads graph state without going
through `oag-graph`. This is what makes `oag rebuild` (see below) trustworthy: every table
`oag-graph` reads is provably reconstructible from the event log, because nothing else ever wrote to
it.

## Request flow

A local write (e.g. REST `POST /api/v1/assertions`, or `oag crawl`):

```
caller → GraphService::assert() → oag_events::commit_local_event()
    → build_and_sign (canonical JSON + Ed25519 signature over it)
    → INSERT INTO events (one row, the source of truth)
    → oag_events::projector::project() (derives assertions/edges/nodes rows)
    → advance this peer's own event_origins head
  [all inside one SQLite transaction — projection failure rolls back the event insert too]
```

A remote write (an incoming replicated event):

```
oag-sync's POST /oag/sync/v1/events → oag_events::ingest_remote_event()
    → verify signature, re-derive event_id, check origin_peer matches the signing key
    → check for a fork at this (origin_peer, sequence) — record and stop if one exists
    → validate chain continuity (sequence/previous_event against this origin's known head)
    → INSERT INTO events → project() → advance this origin's head
  [same one-transaction guarantee as a local commit]
```

A read (search, resolve, get_node, ...) never touches `oag-events` at all — it's a direct query
against the projected tables (`nodes`, `edges`, `assertions`, `evidence`, ...) via `oag-storage`'s
`repo::` modules, wrapped by a `GraphService` method.

## Storage model

One SQLite database per peer (spec sections 8-10), WAL mode, `foreign_keys=ON`. Two categories of
table:

- **Source of truth**: `events`, `event_origins`, `event_refs` — append-only, never wiped, never
  rewritten by anything except a fresh local commit or a validated remote ingest.
- **Derived projections**: `nodes`, `edges`, `assertions`, `evidence`, `actors`, `node_aliases`,
  `assertion_disputes`/`retractions`/`supersessions`, and the FTS5 search indexes — every one of
  these is fully reconstructible from the event log by `oag rebuild` (spec section 83), and a few
  separately-recomputed caches (`node_authority`, `node_embeddings`) that survive rebuild without
  needing to be recomputed, since the graph they're derived from reconstructs identically.

Two tables sit outside both categories: `redactions` and `search_suppressions` (spec section 85) —
durable operator decisions that `oag rebuild` must never wipe, described in `docs/security.md`.

See `docs/data-model.md` for what each table actually represents, and `docs/event-protocol.md` for
exactly how an event becomes a projection.

## Backup and rebuild

`oag backup` takes a consistent snapshot via SQLite's `VACUUM INTO`, safe to run against a live,
WAL-mode `oag serve` (unlike copying `oag.sqlite`/`-wal`/`-shm` file-by-file, which can capture a
mid-write inconsistent state). `oag rebuild` wipes every derived projection table and replays the
entire event log, in original insertion order, to regenerate them — a repair tool for a corrupted or
suspect local projection, never touching the event log itself. See `docs/security.md` for how
redaction interacts with rebuild.

## Observability

`GET /metrics` (spec section 86) exposes a small, hand-rolled Prometheus text-format endpoint —
event/node/edge/assertion/evidence counts, peer count, and database size; no external metrics
platform required. Every REST request and MCP tool call gets a structured `tracing` span (spec
section 87) carrying `request_id`/`peer_id`/`route or tool`/`actor_id`/`status`/`duration_ms`/
`result`; every ingested replication event logs `event_id`/`origin_peer`/`result`
(`docs/replication.md`). See `docs/api.md` for the exact metric names and log fields.

## What's deliberately not built yet

Four larger directions are tracked as future work, not yet implemented. None changes anything
described in this document today:

- Ephemeral, session-scoped peer identity with data/uptime-driven trust (inspired by the PKT
  Network paper — see `ENVIRONMENT.md`).
- Native IPFS integration for large evidence blobs and static exports (spec section 84's
  `blobs/<hash>` concept).
- Granular, per-artifact data versioning: an explicit version number on every projected artifact
  (and on `EventPayload` variants themselves), with two access modes at read time — migrate a row
  to the current version in place, or keep serving an old version forever through an API-layer
  adapter. Follows directly from the legacy-compatibility principle above (`USER_INPUT_RECORD.md`,
  Entry 1) — without it, a future schema change would eventually force a choice between breaking
  old data and never evolving the format at all.
- Long-word, subdomain-based node addressing: extending `oag-petname`'s existing deterministic
  word-based peer *display* names into actual *addressing* — using one domain's enormous subdomain
  space (tens of millions of subdomains, each mapping to a `NodeId`/`PeerId`) as a way to mint vast
  numbers of addressable identities on top of DNS's existing, already-deployed namespace, the same
  "use an existing substrate in a way its designers didn't require" spirit as `oag-reticulum`.
