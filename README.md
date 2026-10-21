# Open Agent Graph (OAG)

An open, distributed, evidence-backed semantic graph of Internet resources, relationships,
claims, and provenance — infrastructure for AI agents rather than another search index.

Every peer is a single Rust binary with embedded SQLite: no Postgres, no Redis, no Kafka, no
external database. Contributions are signed, append-only events; claims are assertions backed by
evidence, not declarations of truth. See [`spec/`](spec/README.md) for the current, authoritative,
per-feature specification — code comments referring to "spec section N" point to an earlier design
document that was never committed to this repository and is no longer retrievable anywhere;
`spec/` is what replaced it.

## Status

Core (one shared service layer, all implemented): signed event log, REST API, MCP server, and
peer-to-peer replication (`oag-sync`, plus an optional Reticulum mesh transport).

Also built:

- **Corroboration/ranking, search, crawling** — ranking signals over competing assertions,
  semantic search, an LLM-extraction fallback for the crawler, Prometheus-style metrics,
  structured request logging, local evidence redaction, a minimal human-browsable HTML view
  (`/ui/search`, `/ui/nodes/{id}`, `/ui/assertions/{id}`).
- **Tensor-pad memory** — a small per-peer pad (`oag-tensor`) nudged by real sync outcomes. A
  separate, optional evolution layer (`oag-brain/`, outside this workspace, needs
  `libtensorflow`) programs that pad via real scaled dot-product self-attention between two
  peers.
- **Ephemeral peer identity** (`[identity].ephemeral` in `config.toml`) — opt-in identity rotation
  with signed presence heartbeats and trust-weighted federation, see
  [`spec/24-ephemeral-peer-trust.md`](spec/24-ephemeral-peer-trust.md).
- **Gravity** — a small, truly-random per-node "gravity level" that very slightly throttles that
  node's own processing speed.
- **Peer clustering** (`[cluster].name`) — a self-declared, signed cluster membership that grants
  a full federation override (a deliberate, documented security trade-off, not a verified fact),
  plus a second, faster, independently-configured gossip loop for same-cluster peers only
  (`[cluster].gossip_interval_seconds`, default 5s), see
  [`spec/26-peer-clustering.md`](spec/26-peer-clustering.md).
- **Per-artifact data versioning** — nodes/edges/assertions/evidence/actors/node-aliases/
  disputes/retractions/observations each carry a schema version; legacy rows are converted in
  place or served through an automatic in-memory shim on every read (`oag migrate <type> <id>`
  for the five types with a singular lookup), see
  [`spec/27-artifact-versioning.md`](spec/27-artifact-versioning.md).
- **Self-hosted roadmap governance** — a roadmap proposal is just an ordinary assertion backed by
  evidence, ranked by the graph's own corroboration signals, no new consensus mechanism
  (`GET /api/v1/roadmap`, `GET /ui/roadmap`), see
  [`spec/28-roadmap-governance.md`](spec/28-roadmap-governance.md).
- **Native IPFS pinning and static site hosting** (`[ipfs].pinning_provider`, default disabled,
  `"disabled" | "kubo" | "embedded"`) — opt-in pinning of crawled evidence content and `oag backup`
  snapshots (`oag backup <out> --pin`); each CID is stored/printed as a retrieval pointer only,
  `oag ipfs cat <cid>` retrieves it. `"embedded"` runs a real IPFS node directly inside this same
  process (bitswap, Kademlia DHT, a browser-dialable websocket listener) — no separate daemon;
  `oag ipfs host-page <dir>` pins a whole directory and `GET /ipfs-site/<cid>` serves it to a
  browser, which fetches the entire site (not just one page) purely over bitswap/libp2p, never
  through an HTTP gateway/proxy. See [`spec/29-ipfs-pinning.md`](spec/29-ipfs-pinning.md).
- **Long-word subdomain addressing** (`[addressing].domain`, default unset) — resolves a `PeerId`
  via a real DNS TXT lookup (`oag peer resolve <peer_id>`), a second, independent path alongside
  gossip-based discovery, never a replacement for it, see
  [`spec/30-long-word-addressing.md`](spec/30-long-word-addressing.md).
- **Service receipts** (`oag peer fetch <address> <server_peer_id> <cid>`) — one peer fetches
  pinned content directly from another over a signed, peer-to-peer request, and both sides record
  a `(peer, sent_to, peer)` assertion backed by a mutual nonce-bound receipt that neither side could
  have fabricated alone. A first, deliberately small step toward a larger, explicitly deferred
  goal (peers getting paid for what they serve) — this piece is only the record-keeping, never
  payment. **Phase 2**: a server can relay a cache miss to its own statically configured
  `[transfer].relay_peers`, chaining independent single-hop transfers (live-verified with a real
  three-peer client→relay→origin chain) without any automatic peer/content discovery. See
  [`spec/31-service-receipts.md`](spec/31-service-receipts.md).
- **Verified internet gateway** (`[gateway]`, default disabled) — a peer never answers a real NTP
  or HTTP request from its own local state: it fans the query out to its configured
  `[gateway].query_peers` and only serves an answer once a quorum agree (NTP: within tolerance;
  HTTP: byte-identical by content hash), or relays onward. A real UDP NTP server and a real HTTP
  reverse proxy front this for actual outside clients, and refuse to even start unless enough
  query peers are configured to meet quorum — structurally, not just by convention. Only a
  quorum-verified result is ever cached, which is also the feature's horizontal-scaling mechanism:
  once any peer verifies an answer, every peer that asks it gets it back cheaply and just as
  honestly, propagating through the network like a cache wave. Live-verified with a real raw NTP
  client and `curl` against a real four-peer quorum. See
  [`spec/32-verified-internet-gateway.md`](spec/32-verified-internet-gateway.md).

Captured, not yet built — see `docs/architecture.md`'s "What's deliberately not built yet" and
[`spec/22-future-work.md`](spec/22-future-work.md): a real multi-peer trust-consensus mechanism,
ephemeral (sub-second) micro-networks with sensor input as a graph "packet," flood/broadcast
propagation for traffic-analysis resistance, and — still part of the pay-for-serving direction
above — automatic peer/content discovery/routing and a witness/flood-based supply-chain audit mode.

See `docs/` for the full picture — start with [`docs/architecture.md`](docs/architecture.md).

## Specification

[`spec/README.md`](spec/README.md) — one file per feature: what it does, what it guarantees, how
it's exposed, and what's deliberately not built yet. Start here to look up exactly what a specific
capability promises.

## Documentation

`docs/` is the narrative, architecture-and-request-flow-oriented walkthrough — start here to
understand how the pieces fit together; see `spec/` instead to look up one feature in detail.

- [`docs/architecture.md`](docs/architecture.md) — crate map, request/data flow, prior art
  (Secure Scuttlebutt, Nostr, Holochain, OrbitDB, nanopublications, and others), and what's
  deliberately not built yet
- [`docs/event-protocol.md`](docs/event-protocol.md) — the signed event envelope and hash chain
- [`docs/data-model.md`](docs/data-model.md) — nodes, edges, assertions, evidence, ranking signals
- [`docs/replication.md`](docs/replication.md) — peer sync protocol, gossip, durability
- [`docs/security.md`](docs/security.md) — identity, auth, SSRF defenses, and redaction's real limits
- [`docs/predicates.md`](docs/predicates.md) — the predicate vocabulary
- [`docs/api.md`](docs/api.md) — REST, MCP, and CLI reference

## Record-keeping

- [`USER_INPUT_RECORD.md`](USER_INPUT_RECORD.md) — verbatim, append-only record of founding
  philosophical/architectural statements from the project's creator, each paired with the
  technical response it led to. The *why* behind decisions that show up elsewhere as principles,
  deferred-future notes, or spec sections.
- [`authors_dream.md`](authors_dream.md) — a second, distinct record in the creator's other voice
  (he is also a science-fiction writer): scenes and dreams, not architectural statements, mostly
  about OAG's space applications. Treated with the same append-only rigor as the record above.
- [`other-people-building-too.md`](other-people-building-too.md) — a running list of active,
  adjacent contemporary projects, checked before describing any OAG capability as novel.
- [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md) — day-to-day scope questions that came up during
  background work, each with the default assumption being run with.

## Build & test

```bash
cargo build --workspace
cargo test --workspace
```

## Run

```bash
cargo run -p oag-cli -- serve --data-dir ./data
```

## License

GNU Affero General Public License v3.0 or later — see [LICENSE](LICENSE).
