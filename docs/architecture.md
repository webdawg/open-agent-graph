# Architecture

## Mission, in one paragraph

OAG is an open, distributed, evidence-backed semantic graph for AI agents. Every peer is a single,
self-contained Rust binary embedding its own SQLite database — no Postgres, no Redis, no Kafka, no
required external service (spec section 11). Contributions are signed, append-only events, not
direct table writes. Claims are assertions backed by evidence and provenance, never bare
declarations of truth (spec sections 21-22). There is no global consensus, no leader election, and
no requirement that every peer agree — each peer maintains its own locally-consistent view, built by
replaying the events it has received (spec sections 5-7).

## Related work

OAG's shape — signed, append-only per-peer logs; gossip-based pull replication; no global
consensus; claims as evidence-backed assertions rather than bare facts — isn't novel in isolation;
each piece has real prior art. Naming it is useful orienting context for anyone (human or a
future context-free AI session) encountering this project cold, not a claim that any one piece
here is unprecedented:

- **Secure Scuttlebutt** is the closest architectural match for the replication layer: each
  identity owns one signed, hash-chained, append-only log (an Ed25519 keypair is the identity, the
  public key is the identifier), and peers gossip-replicate by asking "everything newer than what
  I already have" — close to `oag-sync`'s own `hello`/`heads`/pull model. SSB has no feed-level
  consensus either; each log is authoritative for itself. OAG differs mainly in what rides on top
  of the log: SSB's payload is general social content, OAG's is a typed, graph-projecting event
  schema (Node/Edge/Assertion/Evidence) with corroboration signals computed over it.
- **Nostr** is similar in spirit but structurally looser: a signed-event model with the same "no
  consensus, no required peer mesh" posture, but relays don't sync with each other the way OAG
  peers do — a *client*, not the network, assembles a consistent view across relays. OAG's peers
  do the syncing themselves.
- **Holochain** is the closest match for the identity model specifically — per-agent hash chains
  with no shared chain, DHT-distributed shared state layered on top. Its own documentation
  describes conflict resolution as minimal by default ("delete wins, otherwise first write wins"),
  left to the application. OAG instead resolves "conflict" (two origins disagreeing) by keeping
  both sides inspectable as competing assertions with their own corroboration scores, never
  picking a winner at the protocol layer.
- **OrbitDB** takes the opposite approach from both of the above: a shared, CRDT-merged database
  over IPFS, no per-agent log at all. Useful as a contrast — OAG deliberately keeps per-origin
  provenance rather than merging into one shared structure, since *who claimed what* is exactly
  the signal OAG's ranking depends on.
- **Nanopublications / the "Knowledge Space" network** (Kuhn et al.) is the closest existing match
  for the *claims* layer specifically, as opposed to the replication layer: small, signed,
  uniquely-identified RDF assertions with a dedicated provenance graph, published to a federated
  network explicitly designed to be queried by humans *and* AI agents, with a bounded
  trust-propagation algorithm rather than a blockchain. This is the first prior-art citation to
  reach for before describing OAG's own assertion+evidence+corroboration model as novel.
- **OriginTrail's Decentralized Knowledge Graph** solves a similar-looking problem (verifiable
  knowledge, queryable by agents via MCP) but is built on-chain across multiple EVM-compatible
  blockchains — the opposite of OAG's own "no blockchain, no token, no global ledger" stance.
- Adjacent, not overlapping: agent-identity/delegation-focused proposals such as the IETF's draft
  Intent Provenance Protocol and the LOKA Protocol concept address *who may an agent act as, and
  how far does its delegated authority extend* — a different problem from OAG's *what claims exist
  and how much should they be believed*. Worth distinguishing rather than folding into this
  project's own scope.
- **PKT Network's whitepaper** (DeLisle & Berger, Feb 2021, linked in `ENVIRONMENT.md`) is OAG's
  direct conceptual inspiration for ephemeral peer identity and trust-from-behavior specifically —
  see `spec/24-ephemeral-peer-trust.md` and `USER_INPUT_RECORD.md` Entry 0 for the full reasoning
  and the exact mechanisms borrowed (never PKT's blockchain or token machinery).

## Legacy compatibility

A permanent, binding principle (see `USER_INPUT_RECORD.md`, Entry 1): **OAG never becomes its own
gatekeeper.** No future version is allowed to require that old data, old clients, or old protocols
be upgraded or abandoned to keep working — legacy must always be allowed to flow. This is already
true by construction in several places: `Predicate` is an open string, not a closed enum a future
version could outgrow (spec section 20); `canonicalize_value` passes an already-scheme-prefixed
identifier through unchanged rather than forcing it into a house format; the event log never
deletes or rewrites anything (spec section 106 invariant 3), so nothing a peer has ever accepted can
later be invalidated by a protocol change. Any future schema or protocol evolution must add a new
path alongside the old one, never replace it. The granular data-versioning mechanism this implies
has its Phase 1 built: every node/edge/assertion/evidence row carries a `schema_version`, with a
migrate-in-place action and an always-on in-memory adapter both available at read time, neither
forced on the caller — see `spec/27-artifact-versioning.md`.

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

oag-ipfs        PinningProvider abstraction (disabled by default) for IPFS pinning/retrieval --
                a leaf crate like oag-embeddings, used by oag-crawler and directly by oag-cli's
                `oag ipfs cat`/`host-page`. Two real implementations: KuboHttpProvider (a thin
                reqwest client against a separately-run Kubo daemon) and EmbeddedIpfsProvider (a
                real IPFS node -- bitswap, Kademlia DHT, mDNS, a browser-dialable websocket
                listener -- embedded directly in this process via the `rust-ipfs` crate, no
                separate daemon).

oag-tensor      Per-peer tensor pad memory (TensorPad, TENSOR_PAD_DIM = 32) -- a fixed-width
                Vec<f32>, nudged by real oag-sync outcomes, never by training. See
                spec/23-tensor-pad-and-evolution-layer.md. (oag-brain/, a separate TensorFlow-
                backed attention layer that also programs this pad, lives outside this workspace
                by design -- not a gap in this list, see spec/23.)

oag-crawler     Safe HTTP fetch (SSRF-guarded), structured extraction (JSON-LD, ARD, A2A,
                llms.txt, HTML meta), and LLM-extraction fallback — turns a crawled page into
                the same signed events any other caller would produce.

oag-petname     Deterministic 128-word human-readable names derived from a peer's identity, for
                display purposes only (never used as an identifier) -- plus a separate, much
                shorter 4-word `address_label` for DNS addressing (spec/30), which is used.

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

## Software architecture reference space

The graph itself holds a dedicated hub node, `concept:software-architecture-reference`, under which
externally-sourced architectural *inspiration* is recorded via the `example_of` predicate — never
followed as a spec, only kept as a durable, hash-verified, provenance-tracked record that the intake
happened (see `USER_INPUT_RECORD.md`, Entry 2). The first contents: all nine markdown files from
`hackspherelabs.com/md/` (the project creator's own parallel collective site), crawled and asserted
on 2026-10-01. This is deliberately *data inside OAG*, not a `docs/` copy — the same signed event log
and evidence model used for everything else, applied reflexively to the project's own inspirations.

## What's deliberately not built yet

Several larger directions are tracked as future work. Phase 1 of three of them, plus one other,
has since landed (`spec/24-ephemeral-peer-trust.md`, `spec/26-peer-clustering.md`,
`spec/27-artifact-versioning.md`, `spec/28-roadmap-governance.md` — each marked below); the rest
remain not yet implemented and change nothing described elsewhere in this document:

- Ephemeral, session-scoped peer identity with data/uptime-driven trust (inspired by the PKT
  Network paper — see `ENVIRONMENT.md`). **Phase 1 and 2 implemented** (opt-in `[identity]
  ephemeral = true`, signed presence heartbeats, per-observer trust scoring, trust-weighted
  federation, and live config reload for the trust-check windows via a shared `Arc<RwLock<_>>` and
  a periodic re-read of `[identity]` — see `spec/24-ephemeral-peer-trust.md`). Still future work: a
  real multi-peer voting/consensus mechanism (Phase 1 uses a soft, local, address-correlation
  heuristic instead), and PacketCrypt-style proof-of-data-volume.
- Native IPFS integration for large evidence blobs and static exports (spec section 84's
  `blobs/<hash>` concept). **Phase 1, 2, and 3 implemented** — opt-in pinning of crawled evidence
  content (Phase 1) and `oag backup` snapshots (Phase 2), both via `[ipfs].pinning_provider`
  (default disabled, now `"disabled" | "kubo" | "embedded"`). Evidence's CID lands in the existing
  `Evidence.metadata` column (`{"ipfs_cid": "..."}`, no schema change); `oag ipfs cat <cid>`
  retrieves pinned content from either source — see `spec/29-ipfs-pinning.md`. OAG's own
  `content_hash` stays authoritative; a CID is purely a retrieval pointer. Phase 3: `oag ipfs
  host-page <dir>` pins a whole directory as one UnixFS tree, and `GET /ipfs-site/{cid}` serves a
  bootstrap page that loads a vendored, gateway-free browser bundle fetching the entire site (not
  just `index.html`) purely over bitswap/libp2p — `"embedded"` runs a real IPFS node (via
  `rust-ipfs`) directly inside this same process, no separate daemon, the thing that actually
  keeps this project's "single self-contained binary" promise for this feature; `"kubo"` (Phase
  1/2's original provider, talking to a *separately run* process over HTTP) remains a legitimate
  choice for an operator with existing Kubo infrastructure, just not the default recommendation.
  Still future work: pinning the crawler's auxiliary side-fetches, any REST/MCP pin/fetch
  endpoint, automatic/scheduled backup pinning, inlining large evidence bodies into IPFS in place
  of local storage, and compiling the browser-side client as Rust-to-WASM using the same
  `rust-ipfs` crate instead of the current JS/Helia bundle.
- Granular, per-artifact data versioning. **Phase 1 and 2 implemented** for nodes/edges/
  assertions/evidence (Phase 1) and actors/node-aliases/disputes/retractions/observations
  (Phase 2) — an explicit `schema_version` on every row, with the in-memory shim Entry 1 described
  (`USER_INPUT_RECORD.md`) applied automatically on every read for all nine types, plus the
  explicit migrate-in-place action (`oag migrate <type> <id> --force`) for the five types with a
  pre-existing singular get-by-id lookup to extend (see `spec/27-artifact-versioning.md`). Still
  future work: `assertion_supersessions` (deliberately — no typed read path exists to attach a
  shim to), and versioned `EventPayload` variants for a genuinely breaking payload shape change
  (deferred deliberately, not an oversight — see `spec/27`'s own explanation and
  `USER_INPUT_RECORD.md` Entries 7 and 10).
- Long-word, subdomain-based peer addressing: extending `oag-petname`'s existing deterministic
  word-based peer *display* names into actual *addressing*, the same "use an existing substrate in
  a way its designers didn't require" spirit as `oag-reticulum`. **Phase 1 implemented** — a new
  4-word, DNS-label-safe `address_label` (distinct from the existing 128-word display `peer_name`),
  `[addressing].domain` in `config.toml` (unset by default, nothing hardcoded), `oag identity show`
  printing the computed FQDN, and `oag peer resolve <peer_id>` doing a real TXT lookup — see
  `spec/30-long-word-addressing.md`. Scoped to `PeerId` only (a `NodeId` has no network target to
  resolve to); not wired into `oag-sync`'s automatic discovery/gossip path; no automatic DNS-record
  publishing (the operator publishes it themselves).
- Self-hosted roadmap governance: rather than a second, separate blockchain for "what should be built
  next" (`USER_INPUT_RECORD.md`, Entry 2), reuse OAG's own assertion/evidence/corroboration machinery
  pointed at itself. **Implemented** — `(proposal, instance_of, concept:oag-roadmap-proposal)` is
  an ordinary assertion, ranked by the same `source_independence`/`identity_assurance`/
  `evidence_strength` signals every other claim already gets, with no new consensus mechanism, no
  new predicate, and no new write path (`GET /api/v1/roadmap`, `GET /ui/roadmap`, the MCP tool
  `graph_list_roadmap_proposals` — see `spec/28-roadmap-governance.md`). Deliberately not built: a
  generic "list instances of any hub node" capability beyond this one use (`USER_INPUT_RECORD.md`
  Entry 8).
- Peer clustering: groups of mutually cooperating peers (an "ant colony" of small, cooperating
  units doing real-world physical work — sensors, traffic lights, smart objects), a concept
  distinct from the ephemeral-identity direction above since it's a topology/trust-radius question,
  not an identity-lifetime one. **Phase 1 and 2 implemented** (self-declared, signed cluster
  membership as a full federation override — `[cluster].name` in `config.toml` — see
  `spec/26-peer-clustering.md`, including its explicit security caveat: an unauthenticated cluster
  name is a weak shared secret, not a verified fact; Phase 2 adds a second, independent, faster
  gossip loop — `[cluster].gossip_interval_seconds`, default 5s — for same-cluster peers only,
  alongside the unchanged standard loop). Still future work: any corroboration mechanism for a
  claimed cluster, and the graph-level representation considered and declined for this phase.
- Ephemeral micro-networks and sensor input as a graph "packet" (`USER_INPUT_RECORD.md` Entries
  15-17). Not built at all — no phase landed. New application domains (instant vehicle-to-vehicle
  networks, drone/robot swarm control, branching exploratory "direction" trees) need no new
  mechanism by themselves; a car, a drone, a sensor, and an explored hypothesis are all just nodes.
  Two things named do go beyond what's built: networks lasting as little as 30 milliseconds, well
  past `spec/24-ephemeral-peer-trust.md`'s process-lifetime assumption, with no room for a
  multi-round `hello`/`heads`/pull exchange; and the fact that a specific set of peers briefly
  formed a network becoming itself a graph assertion ("these N peers were, for this instant, one
  thing") — recasting peer clustering's existing networking-only membership claim as an identity
  claim, something this project's own clustering direction above explicitly declined for lack of a
  corroboration story. "Each graph is a packet" reframes sensor input as a transport concept (a
  small, self-contained, signed subgraph), not a new storage type alongside Node/Edge/Assertion/
  Evidence. No design decided yet.
- Flood/broadcast propagation with traffic-analysis resistance, "fire like neuron mode"
  (`USER_INPUT_RECORD.md` Entry 18). Not built. A push-flood-to-every-connected-node model, the
  opposite of the pull-based gossip in `docs/replication.md`, used for Dining-Cryptographers/
  mixnet-style anonymity: when every peer receives every message, an observer watching one link
  can't tell real traffic from traffic a trusted peer is about to discard, so who-talks-to-whom
  stops being visible on the wire — see `docs/security.md`'s "Metadata / traffic-analysis
  resistance" section for why that's explicitly not true today. Flagged, not resolved: flooding
  multiplies bandwidth by fan-out, in real tension with
  `spec/18-rate-limiting-and-resource-limits.md` and the pull-over-push reasoning
  `docs/replication.md` itself gives. "Never ending morphing encryption" is named but not
  specified. Confirmed as the same mechanism behind the redundant-delivery corruption-attribution
  bullet above — one flood, two uses.
- Service-rendered announcements, multi-hop relay, and witness-based supply-chain audit
  (`USER_INPUT_RECORD.md` Entry 21). **Single-hop basic shape implemented** — see
  `spec/31-service-receipts.md`: `oag peer fetch` records a `(peer:<server>, sent_to,
  peer:<client>)` assertion on both sides, backed by a mutual nonce-bound receipt
  (`oag_crypto::mutual_receipt_hash`) neither side could fabricate alone, live-verified end-to-end
  with two real `oag` processes. One step toward a larger, explicitly deferred goal — a
  residential-CDN-style economy where peers get paid for what they serve — narrowed down to just
  the record-keeping layer: a serving peer announcing what it served, to whom. The basic
  announcement needed no new mechanism (an ordinary event with an open-string predicate, carried by
  either existing transport); three things named go beyond what's built so far: a modeled, signed multi-hop
  chain of custody (today's only "relay" notion is `oag-sync`'s transitive gossip propagation, not
  a per-transfer chain); a witness node independently verifying — not just disputing — a serving
  claim (closest existing primitive is the bare two-party `DisputeAssertionPayload`); and
  attributing corruption to a specific hop without a boot or a stationed witness — resolved, not
  left open, by content-hash self-verification at the receiving end (no trust needed to detect a
  mismatch) plus redundant multi-path delivery to localize *which* hop diverged, by comparing which
  paths agree on the final hash. That redundant-delivery path is the same push-flood mechanism as
  the "fire like neuron mode" bullet below, confirmed, not a second mechanism — one flood serving
  both anonymity and corruption attribution. Trust degrades (never a hard boot, consistent with
  `spec/24-ephemeral-peer-trust.md`'s "degrade, don't drop") only for a node *verified* this way as
  the corruption source. See `spec/22-future-work.md`'s matching section.
