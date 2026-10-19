# Redaction and Suppression

**Source**: `crates/oag-graph/src/redaction.rs`,
`crates/oag-storage/src/repo/{redactions,search_suppressions,actor_redactions}.rs`.

**This is the single most important feature to understand before running a public-facing peer.**
Distributed immutable systems cannot promise that data already replicated to independent peers can
be physically erased everywhere, and this project does not pretend otherwise.

## Evidence redaction (`oag redact evidence <id>`)

Blanks an evidence record's `title`/`excerpt` text **on this one peer's own local database only**.

- Does **not** reach any peer this event has already been replicated to. No mechanism anywhere in
  `oag-sync` — and none planned — instructs another peer to delete or stop serving a copy it
  already has.
- Does **not** modify the underlying signed event. `events.canonical_payload` and its Ed25519
  signature are untouched forever. Only the locally materialized `evidence` row is scrubbed —
  this keeps the event log's own cryptographic integrity fully intact while still removing the
  sensitive content from what `oag search`/REST/MCP will ever surface locally.
- Is **irreversible at the tooling level**. No "unredact" command. A tombstone (`redactions` table)
  records that a redaction happened, when, and why, for audit purposes — it carries no way back.
  A second redaction attempt on an already-redacted id is a typed error
  (`GraphError::AlreadyRedacted`), not a silent no-op or a way to "re-redact" with different
  content.
- Refuses to run without `--force`, printing this limitation as a warning every time, so it's
  never invoked by muscle memory without the operator seeing it.
- Survives `oag rebuild`: `rebuild_projection` reapplies outstanding redactions after replaying the
  event log, specifically so the original `ADD_EVIDENCE` event's replay can never resurrect the
  redacted content.

`reason` is length-capped the same way every other mutation's text fields are (see
[04-data-model.md](04-data-model.md)) — found missing and fixed in the same pass that found
`declare_actor`'s equivalent gap.

## Search suppression (`oag redact suppress-node`/`unsuppress-node`)

A *much* weaker, fully reversible intervention: hides a node from search results only. The node,
its edges, and every assertion about it remain fully intact and directly retrievable by id — a
UI-layer omission, not a data-minimization control, and should not be relied on to keep anything
actually private.

### Listing suppressed nodes — found missing, added

`search_suppressions` had `suppress`/`unsuppress`/`is_suppressed` but no way to list what's
*currently* suppressed, unlike `redactions`, whose `list_all` has always backed `oag redact list`.
An operator suppressing nodes over time had no way to see the full current list without guessing
node ids one at a time. **Fix**: `search_suppressions::list_all`,
`GraphService::list_suppressed_nodes`, folded into `oag redact list`'s output —
`{"redactions": [...], "suppressed_nodes": [...]}` instead of a bare redactions array.

## Actor `identity_uri` redaction (`oag redact actor-identity-uri <id>`)

Blanks one actor's `identity_uri` field **on this one peer's own local database only** — the exact
same shape and guarantees as evidence redaction above, just a different column on a different
table. `public_key`/`name`/`actor_type` are left intact; only `identity_uri` is ever in scope.

Previously left out deliberately, tracked in `OPEN_QUESTIONS.md`'s "Deletion and redaction"
section as a "don't scope-creep past the clearest, safest case" decision. Confirmed safe to build
by direct code research before implementing it: `identity_uri` is a plain, optional, free-text
column (`crates/oag-storage/migrations/0001_init.sql`) — never cryptographic material (that's the
entirely separate `public_key` column and `PeerId`, which `PeerId::from_public_key` derives purely
from the Ed25519 verifying key, never from `identity_uri`). It briefly appears inside the signed
`ActorKeyProofMessage` at `declare_actor` time, but that signature is itself never persisted
(verified once, then discarded) — so nothing anywhere re-verifies a signature against
`identity_uri` later, and blanking it post-hoc breaks no future verification, peer recognition, or
API-key auth. The one real side effect: an actor with a blanked `identity_uri` and no `public_key`
scores lower on `identity_assurance_score` (a ranking signal, `crates/oag-graph/src/
corroboration.rs`) — an expected consequence of redaction, not a correctness break.

**One structural difference from evidence redaction**: `actors` (unlike `events`) *is* wiped and
replayed by `oag rebuild`, so the tombstone table
(`actor_identity_uri_redactions`) has no foreign key to `actors` — same reasoning as
`search_suppressions.node_id` having none to `nodes`, not `redactions.event_id`'s FK-to-`events`
pattern. `oag_graph::rebuild::rebuild_projection` reapplies outstanding actor-identity-uri
redactions after every replay (`RebuildSummary::actor_redactions_reapplied`), the same way it
already does for evidence redactions.

## Fork evidence (a related, third durable audit table)

`peer_forks` is covered in [02-event-protocol.md](02-event-protocol.md) and
[14-replication-sync.md](14-replication-sync.md) — it shares this feature's "durable operator-
facing audit record, survives rebuild" shape, including the same kind of gap: its own migration
comment called it "evidence," but only bare event-id hashes were ever stored until fixed to
retain the actual rejected event's signed JSON.

## Why `redactions`/`search_suppressions`/`actor_identity_uri_redactions` survive `oag rebuild`

All three tables sit outside the normal source-of-truth/derived-projection split (see
[03-rebuild-and-backup.md](03-rebuild-and-backup.md)): they're durable *operator decisions*, not a
cache of the event log, so `clear_projection_tables` explicitly excludes them from its wipe list.
`search_suppressions.node_id`/`actor_identity_uri_redactions.actor_id` deliberately have no foreign
key to `nodes`/`actors` — unlike `events`, both `nodes` and `actors` *are* wiped and replayed by
rebuild, and a foreign key here would make that delete fail with a constraint violation for as long
as any tombstone row survives rebuild, which it must, by design.

## Backups contain everything

`oag backup` snapshots the entire local database, including any evidence that hasn't been
redacted. Redacting evidence after taking a backup does not retroactively redact the backup.

## CLI surface

| Command | Behavior |
|---|---|
| `oag redact evidence <id> [--reason] --force` | Blank one evidence record's title/excerpt |
| `oag redact list` | Show every redaction tombstone, every currently-suppressed node, and every actor identity_uri redaction |
| `oag redact suppress-node <id>` / `unsuppress-node <id>` | Toggle search visibility |
| `oag redact actor-identity-uri <id> [--reason] --force` | Blank one actor's identity_uri |

CLI, gated by filesystem access to the data directory, using a synthetic local-operator
`AuthContext` carrying `Permission::Admin`. `GraphService::redact_evidence`/`list_redactions`/
`list_suppressed_nodes`/`suppress_node_from_search`/`unsuppress_node_from_search` all check
`Permission::Admin` internally, so no business logic is duplicated for the REST/MCP surface below
— only a handler producing a real authenticated `AuthContext` from a bearer key.

## REST / MCP surface

| REST | MCP | Behavior |
|---|---|---|
| `POST /api/v1/evidence/{id}/redact` | `graph_redact_evidence` | Blank one evidence record's title/excerpt |
| `GET /api/v1/redactions` | `graph_list_redactions` | Show every redaction tombstone, every currently-suppressed node, and every actor identity_uri redaction |
| `POST /api/v1/nodes/{id}/suppress` | `graph_suppress_node` | Hide a node from search results |
| `POST /api/v1/nodes/{id}/unsuppress` | `graph_unsuppress_node` | Reverse suppression |
| `POST /api/v1/actors/{id}/redact-identity-uri` | `graph_redact_actor_identity_uri` | Blank one actor's identity_uri |

Both require the `admin` permission, same as the CLI. Unlike the CLI, there is no `--force`
confirmation step or printed warning — that flag exists only to guard against a human invoking a
destructive command by muscle memory, not as a security boundary (`admin` is); a REST/MCP caller
holding a real `admin` key has already made a deliberate, single API call. See
`crates/oag-api/src/redaction.rs` and `crates/oag-mcp/src/server.rs`.

## Tests

`crates/oag-graph/src/redaction.rs` (`redact_evidence_is_reflected_via_list_evidence`,
`redacting_twice_is_a_typed_error_not_a_silent_noop`, `redaction_survives_rebuild`,
`suppress_node_hides_it_from_search_but_not_from_direct_lookup`,
`search_suppression_survives_rebuild`, `redacting_with_an_oversized_reason_is_rejected`,
`redact_actor_identity_uri_blanks_it_but_leaves_public_key_and_name_intact`,
`redacting_an_actors_identity_uri_twice_is_a_typed_error_not_a_silent_noop`,
`redacting_an_unknown_actors_identity_uri_is_not_found`,
`redacting_an_actors_identity_uri_requires_admin_permission`,
`actor_identity_uri_redaction_survives_rebuild`);
`crates/oag-storage/src/repo/actor_redactions.rs` (`insert_exists_and_list_round_trip`);
`crates/oag-cli/src/commands.rs` (`redact_evidence_refuses_without_force`,
`redact_evidence_succeeds_with_force_and_lists_the_reason`,
`redact_suppress_node_shows_up_in_list_suppressed_nodes`);
`crates/oag-api/src/tests.rs` (`redact_evidence_endpoint_blanks_it_and_rejects_a_repeat_call`,
`suppress_and_unsuppress_node_endpoints_round_trip`,
`redaction_endpoints_require_admin_not_just_an_authenticated_key`,
`redact_actor_identity_uri_endpoint_blanks_it_and_rejects_a_repeat_call`);
`crates/oag-mcp/src/tests.rs` (`graph_redact_evidence_blanks_it_and_lists_it`,
`graph_suppress_and_unsuppress_node_round_trip`, `graph_suppress_node_without_admin_is_rejected`,
`graph_redact_actor_identity_uri_blanks_it_and_lists_it`,
`graph_redact_actor_identity_uri_without_admin_is_rejected`).
Also live-verified against a real running `oag serve` process over real HTTP: a non-admin key
gets `403`, an admin key's redact returns `200` then `409` on repeat, title/excerpt come back
blanked while `uri` survives, and the suppress/unsuppress round trip visibly changes
`GET /search` result counts.
