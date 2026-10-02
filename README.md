# Open Agent Graph (OAG)

An open, distributed, evidence-backed semantic graph of Internet resources, relationships,
claims, and provenance — infrastructure for AI agents rather than another search index.

Every peer is a single Rust binary with embedded SQLite: no Postgres, no Redis, no Kafka, no
external database. Contributions are signed, append-only events; claims are assertions backed by
evidence, not declarations of truth. See `.claude/plans/` history for the full v0.2 design spec
this implementation follows.

Status: signed event log, REST API, MCP server, and peer-to-peer replication (`oag-sync`, plus an
optional Reticulum mesh transport) are all implemented and share one service layer. Also built:
corroboration/ranking signals, semantic search, an LLM-extraction fallback for the crawler,
Prometheus-style metrics, structured request logging, local evidence redaction, and a minimal
human-browsable HTML view (`/ui/search`, `/ui/nodes/{id}`, `/ui/assertions/{id}`). See `docs/` for
the full picture — start with `docs/architecture.md`.

## Documentation

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
