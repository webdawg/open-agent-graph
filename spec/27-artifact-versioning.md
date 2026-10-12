# Artifact Versioning, Phase 1

**Source**: `crates/oag-storage/src/versioning.rs`,
`crates/oag-storage/migrations/0012_artifact_schema_version.sql`, `repo::{nodes, edges,
assertions}::migrate_to_current`/`migrate_evidence_to_current`, `oag-graph/src/redaction.rs`'s
`migrate_node`/`migrate_edge`/`migrate_assertion`/`migrate_evidence`, `oag-cli`'s `oag migrate
<node|edge|assertion|evidence>`.

The mechanism behind this project's "legacy must flow" principle (`docs/architecture.md`'s
"Legacy compatibility" section, `USER_INPUT_RECORD.md` Entry 1): "every single artifact of data
has a version number, and if we are accessing it we can convert it perm, or we can leave it as is
and use api interface code to access it." Every node, edge, assertion, and evidence row now
carries a `schema_version`, with two access modes available at read time — explained below —
neither forced on the caller.

## Scope: nodes, edges, assertions, evidence

These four are the artifact types named together everywhere this project's own docs discuss this
principle. Extending the same mechanism to actors, aliases, disputes, retractions, supersessions,
and observations is mechanically identical but not done this phase (see "Explicitly deferred").

## The mechanism

`oag-storage::versioning` is generic, pure, and knows nothing about any specific table:

- `HasSchemaVersion`: a row that knows its own stored `schema_version`.
- `Versioned`: a row type with a declared `CURRENT_VERSION` and a `MIGRATIONS` chain —
  `&'static [(i64, fn(Self) -> Self)]` pairs, each one a step that advances a row from exactly the
  version named to the next, setting the row's own new `schema_version` as part of what it
  returns.
- `migrate_to_version`: walks a row forward through its type's `MIGRATIONS` chain until it reaches
  `CURRENT_VERSION` or no step matches its current version (an incomplete chain returns the row
  as-is rather than panicking).

`NodeRow`, `EdgeRow`, `AssertionRow`, `EvidenceRow` each implement `Versioned` with
`CURRENT_VERSION = 1` and an **empty** `MIGRATIONS` chain — true today for every one of them, since
no second shape exists yet for any artifact type in this codebase. This makes the whole mechanism a
real, honest no-op right now: it exists and is wired through every read path, but changes nothing
about any row that exists today or any row inserted by code that hasn't been touched.

## The two access modes

**The shim (always on, non-destructive)**: every `row_to_node`/`row_to_edge`/`row_to_assertion`/
`row_to_evidence` function — the one shared conversion point every `repo::*` module already funnels
through before `GraphService`, and therefore REST, MCP, and the CLI, ever see a domain type — calls
`row.migrate_to_current()` before building the returned type. Every normal read already gets
current-shape data, in memory, regardless of what's actually on disk. This is what "leave it as is
and use api interface code to access it" means concretely: the stored bytes don't change, but
nothing downstream ever sees an old shape.

**Convert perm (explicit, operator-triggered, `--force`-gated)**: `repo::nodes::migrate_to_current`
and its edge/assertion/evidence equivalents look up the row, and if its `schema_version` is already
current, report `MigrationOutcome::AlreadyCurrent { version }` and touch nothing. Otherwise they
apply the same migration chain and `UPDATE ... SET schema_version = ?` in place, reporting
`MigrationOutcome::Migrated { from, to }` — a real, permanent rewrite, so every future read skips
the in-memory conversion step. `GraphService::migrate_node`/`migrate_edge`/`migrate_assertion`/
`migrate_evidence` gate this behind `Permission::Admin`, the same trust level
`redact_evidence`/`suppress_node_from_search` already use — a direct storage mutation outside the
event log, not something a normal actor calls, and deliberately **not** routed through
`commit_event`: migrating a row's internal shape isn't a new fact about the world, so it isn't a
new signed event.

`oag migrate node|edge|assertion|evidence <id> --force` (CLI) exposes the "convert perm" path.
`--force` is required **unconditionally**, even for the "nothing to migrate" case — see
`USER_INPUT_RECORD.md` Entry 7 for why this deviates from the originally-planned "report
already-current without requiring --force": requiring it unconditionally, matching
`redact_evidence`'s existing discipline exactly, means a caller who omits `--force` can never be
surprised by a real mutation the day a real second schema version exists, even though today the
flag changes nothing. The command refuses with an explicit explanation of what `--force` would do,
and an unknown id is a real `StorageError::NotFound`, not a silent `AlreadyCurrent`.

## What's deliberately not built yet, and why

- **Versioned `EventPayload` variants** (Part A of the original idea, speculated in
  `docs/architecture.md` before this phase, not stated by the project's own founding words in
  Entry 1): no real second shape exists for any payload variant today, so there is nothing to
  migrate and nothing non-synthetic to prove such a mechanism correct against. The existing
  `Option<T>`/`#[serde(default)]` convention (already used throughout `oag-core`, e.g.
  `metadata: JsonValue`) remains the mechanism for additive payload evolution.
  `UnsignedEvent::version: u16` (`oag-events/src/envelope.rs`) — already part of every signed
  event's hashed bytes, already permanently `1` and inert — is the reserved hook for the day a
  genuinely breaking payload change is needed; the projector's dispatch becomes version-aware using
  that field at that point, not before.
- Extending `schema_version`/the migrate-or-shim mechanism to actors, aliases, disputes,
  retractions, supersessions, observations.
- Any read-only "list artifacts behind current version" view.
- Rebuild-awareness of schema versions: `oag rebuild` replays the event log through today's
  projector, so every row it produces lands at whatever `CURRENT_VERSION` is at rebuild time —
  rebuild is, as an emergent side effect, already a full re-normalization to current shape for free.
  This only remains true as long as the projector itself doesn't need per-historical-event handling
  to reproduce a row correctly — the day that's no longer true is also the day Part A above becomes
  necessary.

## Tests

`versioning.rs`'s pure chain-walker, tested with a fake row type and a fake multi-step chain (not
a real artifact type, since none has a second shape to test against): ordering across multiple
steps, starting partway through a chain, an already-current row being untouched, and an incomplete
chain stopping rather than panicking. `oag-storage`'s own test suite: all four `migrate_to_current`
functions correctly report `AlreadyCurrent` for a normally-inserted row, and a genuinely unknown id
is a real `NotFound` error. `oag-graph`'s `redaction.rs` tests: the same four methods through
`GraphService`, `Permission::Admin` gating, and the not-found case. Live: a real `oag serve`
instance, a real asserted node/edge/assertion/evidence, `oag migrate <type> <id>` refusing without
`--force` and reporting `AlreadyCurrent` with it, and `oag rebuild` reproducing the same
already-current state afterward.
