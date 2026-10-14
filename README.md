# Open Agent Graph (OAG)

An open, distributed, evidence-backed semantic graph of Internet resources, relationships,
claims, and provenance — infrastructure for AI agents rather than another search index.

Every peer is a single Rust binary with embedded SQLite: no Postgres, no Redis, no Kafka, no
external database. Contributions are signed, append-only events; claims are assertions backed by
evidence, not declarations of truth. See [`spec/`](spec/README.md) for the current, authoritative,
per-feature specification — code comments referring to "spec section N" point to an earlier design
document that was never committed to this repository and is no longer retrievable anywhere;
`spec/` is what replaced it.

Status: signed event log, REST API, MCP server, and peer-to-peer replication (`oag-sync`, plus an
optional Reticulum mesh transport) are all implemented and share one service layer. Also built:
corroboration/ranking signals, semantic search, an LLM-extraction fallback for the crawler,
Prometheus-style metrics, structured request logging, local evidence redaction, a minimal
human-browsable HTML view (`/ui/search`, `/ui/nodes/{id}`, `/ui/assertions/{id}`), and a small
per-peer tensor-pad memory (`oag-tensor`) nudged by real sync outcomes. A separate, optional
evolution layer (`oag-brain/` — outside this workspace; needs `libtensorflow`) programs that pad
via real scaled dot-product self-attention between two peers. Also built: opt-in ephemeral peer
identity with signed presence heartbeats and trust-weighted federation (`[identity].ephemeral` in
`config.toml`), a small, truly-random per-node "gravity level" that very slightly throttles that
node's own processing speed, opt-in peer clustering (`[cluster].name`) where a self-declared,
signed cluster membership grants a full federation override — a deliberate, documented security
trade-off, not a verified fact — plus a second, faster, independently-configured gossip loop for
same-cluster peers only (`[cluster].gossip_interval_seconds`, default 5s, see
`spec/26-peer-clustering.md`) — and per-artifact data
versioning for nodes/edges/assertions/evidence/actors/node-aliases/disputes/retractions/
observations (`oag migrate <type> <id>` for the five with a singular lookup to extend), so legacy
rows can always either be converted in place or served forever through an in-memory adapter
applied automatically on every read (see `spec/27-artifact-versioning.md`), self-hosted roadmap
governance, where a roadmap proposal is just an ordinary assertion backed by evidence, ranked by
the graph's own corroboration signals with no new consensus mechanism (`GET /api/v1/roadmap`,
`GET /ui/roadmap` — see `spec/28-roadmap-governance.md`), opt-in native IPFS pinning
(`[ipfs].pinning_provider`, default disabled) of both crawled evidence content and `oag backup`
snapshots (`oag backup <out> --pin`), with each CID stored/printed as a retrieval pointer only and
`oag ipfs cat <cid>` for retrieval (see `spec/29-ipfs-pinning.md`), and opt-in long-word subdomain
addressing for a peer (`[addressing].domain`, default unset), resolved via a real DNS TXT lookup
(`oag peer resolve <peer_id>`) as a second, independent path to a `PeerId` you already know, never
a replacement for gossip-based peer discovery (see `spec/30-long-word-addressing.md`). See `docs/`
for the full picture — start with `docs/architecture.md`.

## Specification

[`spec/README.md`](spec/README.md) — one file per feature: what it does, what it guarantees, how
it's exposed, and what's deliberately not built yet. Start here to look up exactly what a specific
capability promises.

## Documentation

`docs/` is the narrative, architecture-and-request-flow-oriented walkthrough — start here to
understand how the pieces fit together; see `spec/` instead to look up one feature in detail.

- [`docs/architecture.md`](docs/architecture.md) — crate map and request/data flow
- [`docs/event-protocol.md`](docs/event-protocol.md) — the signed event envelope and hash chain
- [`docs/data-model.md`](docs/data-model.md) — nodes, edges, assertions, evidence, ranking signals
- [`docs/replication.md`](docs/replication.md) — peer sync protocol, gossip, durability
- [`docs/security.md`](docs/security.md) — identity, auth, SSRF defenses, and redaction's real limits
- [`docs/predicates.md`](docs/predicates.md) — the predicate vocabulary
- [`docs/api.md`](docs/api.md) — REST, MCP, and CLI reference

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
