# Rebuild and Backup

**Source**: `crates/oag-graph/src/rebuild.rs`, `crates/oag-storage/src/repo/rebuild.rs`,
`crates/oag-cli/src/commands.rs`.

## Rebuild

`oag rebuild` wipes every *derived* table and replays the entire local event log, in original
insertion order, to regenerate them from scratch. It is a repair tool for a corrupted or suspect
local projection — it never touches the event log itself (`events`/`event_origins`/`event_refs`).

### What gets wiped and replayed

`nodes`, `edges`, `assertions`, `evidence`, `actors`, `node_aliases`,
`assertion_disputes`/`retractions`/`supersessions`, and the FTS5 search indexes.

### What survives rebuild untouched

- `events`/`event_origins`/`event_refs`/`peers`/`peer_addresses`/`peer_forks` — not derived
  projections at all, so `clear_projection_tables` never includes them.
- `node_authority`/`node_embeddings` — separately-recomputed caches. Since `edges` reconstructs
  byte-identical after a rebuild, existing authority/embedding scores stay valid without needing
  their own recompute pass.
- `redactions`/`search_suppressions` — durable operator decisions, not a cache of the event log.
  `clear_projection_tables` deliberately excludes them, and `rebuild_projection` *reapplies*
  outstanding redactions after replay finishes, specifically so a redaction can never be
  resurrected by replaying the original `ADD_EVIDENCE` event. Confirmed by test
  (`redaction_survives_rebuild`, `search_suppression_survives_rebuild`).

### Why this is safe

Every event currently in `events` already projected successfully once (the one-transaction
commit/ingest guarantee — see [02-event-protocol.md](02-event-protocol.md)). Replaying them in
original insertion order is therefore guaranteed to reproduce identical derived state, byte for
byte. Confirmed by test: `rebuild_projection_reproduces_identical_graph_state`.

### Forks and rebuild

Forked events never enter the `events` table in the first place (fork detection returns early,
before either conflicting event is inserted), so rebuild's replay loop needs no special-case
handling for forks — there is nothing forked to encounter.

## Backup

`oag backup <path>` snapshots the *entire* local database via SQLite's `VACUUM INTO`, safe to run
against a live, WAL-mode `oag serve` process (unlike copying `oag.sqlite`/`-wal`/`-shm` files
directly, which can capture a mid-write inconsistent state).

**Backups contain everything**, including any evidence that hasn't been redacted. Redacting
evidence after taking a backup does not retroactively redact the backup — treat backup files with
the same sensitivity as the live database.

## CLI surface

| Command | Behavior |
|---|---|
| `oag rebuild` | Wipe and replay every derived table from the event log. Best run with `oag serve` stopped. |
| `oag backup <path>` | Consistent whole-database snapshot via `VACUUM INTO` |

Neither has a REST/MCP surface — both are local, data-directory-trust operations, same posture as
identity and key management.

## Tests

`crates/oag-graph/src/tests.rs` (`rebuild_projection_reproduces_identical_graph_state`);
`crates/oag-graph/src/redaction.rs`'s rebuild-survival tests;
`crates/oag-cli/src/commands.rs`'s `backup_and_rebuild_smoke_test`.
