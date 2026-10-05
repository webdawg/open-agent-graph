# CLI

**Source**: `crates/oag-cli/src/{main,commands,serve,config}.rs`.

One binary (`oag`) with subcommands — there is no separate server/client/worker build. Every
subcommand accepts `--data-dir` (default `./data`); network-facing ones also accept `--config`.

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
| `oag identity show/backup/restore` | Peer identity lifecycle |
| `oag key create/list/revoke` | API key lifecycle |
| `oag peer add/list/remove/sync` | Manage known peer addresses and trigger one-shot sync |
| `oag peer forks <peer_id>` | Show recorded fork evidence |
| `oag peer reticulum-address` / `add-reticulum` | Reticulum transport peer management |
| `oag replication status` | Durability view |
| `oag authority recompute` | Batch-recompute node authority |
| `oag embeddings recompute` | Batch-recompute node embeddings |
| `oag backup <path>` | Consistent whole-database snapshot |
| `oag rebuild` | Wipe and replay every derived table from the event log |
| `oag redact evidence/list/suppress-node/unsuppress-node` | Deletion and redaction |
| `oag crawl <url>` | One-shot crawl |
| `oag doctor [--config]` | Local health checks |

## Design principle: thin wrapper

`oag-cli` contains no business logic — every command is a thin wrapper calling into
`GraphService`/`CrawlerService`/`SyncService`, the same service layer REST and MCP use. Mutation
commands that exist on the CLI but *not* REST/MCP (key management, redaction, backup/rebuild) are
deliberately local-only, filesystem-trust operations — the opposite case (assertion mutations
exist on REST/MCP but not CLI) is equally deliberate, matching the existing precedent that
assertion mutations are "sometimes CLI-or-REST/MCP, never both."

## `oag doctor`

Validates: data directory writable, peer identity loadable/generatable, database opens and
migrations are current, and (with `--config`) that `config.toml` parses via the *exact* resolver
`oag serve` itself uses — so `doctor` can never pass on a config that `serve` would actually
reject.

## `oag serve`'s startup sequence

Create/load identity → open/migrate SQLite → bootstrap admin key if this is a fresh data
directory → assemble REST+sync router → assemble MCP (with its own rate-limit layer, see
[11-mcp-server.md](11-mcp-server.md)) → bind and serve → spawn the gossip loop against any
configured bootstrap peers.

## Tests

`crates/oag-cli/src/commands.rs` (doctor, identity, key, redaction, backup/rebuild round trips);
`crates/oag-cli/src/serve.rs` (`mcp_endpoint_is_rate_limited_same_as_rest`, the project's first
full-app integration test spinning up a real combined REST+MCP server over a real TCP listener).
