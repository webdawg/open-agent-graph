# Open Agent Graph (OAG)

An open, distributed, evidence-backed semantic graph of Internet resources, relationships,
claims, and provenance — infrastructure for AI agents rather than another search index.

Every peer is a single Rust binary with embedded SQLite: no Postgres, no Redis, no Kafka, no
external database. Contributions are signed, append-only events; claims are assertions backed by
evidence, not declarations of truth. See `.claude/plans/` history for the full v0.2 design spec
this implementation follows.

Status: single-peer core implemented — signed event log, REST API, and MCP server sharing one
service layer. Replication (`oag-sync`) is the next milestone.

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
