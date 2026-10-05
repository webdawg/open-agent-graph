# Open Agent Graph — Specification

This directory is the current, authoritative specification of Open Agent Graph: one file per
feature, describing what each part of the system actually does, the guarantees it makes, how it's
exposed, and what's deliberately not built yet.

## Why this exists now, not earlier

Code comments and `docs/` throughout this project refer to "spec section N" (e.g. "spec section
61," "spec section 85") — these refer to a v0.2 design document that drove most of this
implementation. That document was never committed to this repository; it lived only in a prior
Claude Code session's local `.claude/plans/` directory, which is not persisted across sessions or
environments. **That original spec no longer exists anywhere retrievable.** The numbered
references in code comments are historical fossils — accurate at the time they were written, but
pointing at a document nobody can open anymore.

Rather than guess at reconstructing 100+ numbered sections from secondhand references, this
directory starts fresh: a specification of the system **as it actually is today**, organized by
feature, written and verified against the real implementation and its test suite rather than
against memory of a lost document. Old "spec section N" comments in the code remain as historical
notes explaining *why* something was built a certain way; they are not contradicted or replaced by
renumbering, just no longer the live reference. New work should cite files in this directory
instead.

## How to keep this organized going forward

- **One file per feature.** If a change doesn't fit cleanly into an existing file's scope, that's
  a signal either the feature boundary needs adjusting or a new file is warranted — don't let one
  file quietly become a dumping ground for unrelated things.
- **Write what *is*, not what's planned.** Speculative/future work belongs in
  [`22-future-work.md`](22-future-work.md), not scattered as aspirational language inside a
  feature's own spec. A feature's spec should be true of the code today.
- **Cite source files and tests.** Every claim here should be checkable against
  `crates/*/src/*.rs` and its test suite — this is a specification of real behavior, not a wishlist.
- **Update the relevant file in the same change that changes the behavior.** Treat a spec file
  going stale the same as a failing test — it's a bug, just a slower-burning one.
- **This directory and `docs/` are not duplicates.** `docs/` remains the narrative,
  architecture-and-request-flow-oriented technical walkthrough (start there to understand how the
  pieces fit together). `spec/` is the feature-by-feature reference (go there to look up exactly
  what one specific capability guarantees). Where they overlap, `docs/` should link here rather
  than re-explain.

## Index

| File | Feature |
|---|---|
| [01-identity.md](01-identity.md) | Peer identity, keys, backup/restore |
| [02-event-protocol.md](02-event-protocol.md) | Signed event envelope, canonicalization, hash chain, commit/ingest, fork detection |
| [03-rebuild-and-backup.md](03-rebuild-and-backup.md) | `oag rebuild`, `oag backup` |
| [04-data-model.md](04-data-model.md) | Node/Edge/Assertion/Evidence/Actor/Predicate, identifier canonicalization |
| [05-assertions-lifecycle.md](05-assertions-lifecycle.md) | assert / evidence / dispute / retract / supersede / verify |
| [06-corroboration-and-authority.md](06-corroboration-and-authority.md) | Ranking signals, PageRank-style authority |
| [07-search.md](07-search.md) | Keyword (FTS5) and semantic (embeddings) search |
| [08-subgraph-and-history.md](08-subgraph-and-history.md) | Subgraph traversal, provenance/history, explainability |
| [09-authentication-and-authorization.md](09-authentication-and-authorization.md) | API keys, actors, permissions |
| [10-rest-api.md](10-rest-api.md) | REST surface |
| [11-mcp-server.md](11-mcp-server.md) | MCP surface |
| [12-cli.md](12-cli.md) | `oag` CLI surface |
| [13-human-interface.md](13-human-interface.md) | Human-browsable HTML views |
| [14-replication-sync.md](14-replication-sync.md) | Peer-to-peer sync protocol, gossip, federation policy, durability |
| [15-reticulum-transport.md](15-reticulum-transport.md) | Optional mesh-radio transport |
| [16-crawler.md](16-crawler.md) | Safe fetch, SSRF defenses, structured + LLM extraction |
| [17-redaction-and-suppression.md](17-redaction-and-suppression.md) | Evidence redaction, search suppression, fork evidence |
| [18-rate-limiting-and-resource-limits.md](18-rate-limiting-and-resource-limits.md) | Every request/response/memory/disk cap in the system |
| [19-observability.md](19-observability.md) | Metrics, structured logging |
| [20-configuration.md](20-configuration.md) | `config.toml` |
| [21-petname.md](21-petname.md) | Deterministic human-readable peer display names |
| [22-future-work.md](22-future-work.md) | Deliberately not built yet, and why |

## Founding context

[`USER_INPUT_RECORD.md`](../USER_INPUT_RECORD.md) (repo root) is the verbatim record of the
project's founding philosophical/architectural statements — read it for the *why* behind
principles like "legacy must flow" that shape several files here.
[`OPEN_QUESTIONS.md`](../OPEN_QUESTIONS.md) (repo root) is the day-to-day scope-decision log — read
it for *why* a specific boundary was drawn where it was, including real bugs found and fixed along
the way.
