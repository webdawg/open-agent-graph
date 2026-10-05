# Signed Event Protocol

**Source**: `crates/oag-events/src/{envelope,builder,validate,commit,ingest,projector}.rs`,
`crates/oag-core/src/ids.rs`.

## What it is

OAG's core invariant: **every mutation is a signed, append-only event, never a direct database
write.** A node, edge, assertion, or evidence row never comes into existence except as the
projection of some signed event. This is what makes the whole system auditable and replicable.

## The envelope

```
UnsignedEvent { version, origin_peer, sequence, previous_event, created_at, payload }
SignedEvent   { unsigned: UnsignedEvent, signature }
```

`event_id` is *not* a field — it's derived (`EventId::derive`) from the canonical bytes of the
whole signed envelope, so a caller can never supply their own id.

## Domain separation

Every identifier (`NodeId`, `EdgeId`, `EventId`, `ActorId`, `PeerId`) is
`BLAKE3("OAG:<KIND>:v1:" + input)`. The domain prefix means a `NodeId` and an `EventId` can never
collide even over the same raw bytes — an attacker can't construct input that hashes to a
valid-looking id of the wrong kind. Signatures follow the same pattern
(`sign_with_domain`/`verify_with_domain`): every signed structure is signed over
`domain_prefix + canonical_bytes`, so a signature valid for one purpose can never be replayed as
valid for another.

## Canonical serialization

Event payloads are serialized via RFC 8785 JSON Canonicalization (JCS, `serde_jcs`) before
signing. The same logical event always produces the same bytes to sign and verify, regardless of
which peer or language produced it — this eliminates signature-malleability bugs that ad-hoc JSON
serialization would otherwise introduce.

## Per-peer hash chain

Each peer maintains its own append-only chain: `sequence` starts at 1 and increments by exactly 1
per event that peer authors; `previous_event` is that peer's own prior `event_id` (or `None` for
its first event). This is a **per-origin** chain, not a global one — there is no global event
order, and two different peers' events are never ordered relative to each other.

`event_origins` (one row per peer this local database has ever seen events from) tracks
`highest_contiguous_sequence`, `highest_seen_sequence`, and `head_event_id` — enough bookkeeping to
validate the next incoming event from that origin without re-scanning the whole chain.

## Committing a local event

`GraphService::commit_event` is the **single serialization point** every mutation method goes
through — it is not just a convenience wrapper, it is load-bearing. Internally it calls
`commit_local_event`, which:

1. Reads this peer's own `event_origins` row for `highest_contiguous_sequence`/`head_event_id`.
2. Builds `UnsignedEvent { sequence: head + 1, previous_event: head_event_id, ... }`, canonicalizes,
   signs.
3. `INSERT INTO events` (the row is now permanent).
4. Runs the projector against the same payload.
5. Advances this peer's own `event_origins` head.

Steps 3-5 happen inside **one SQLite transaction**. If projection fails for any reason, the entire
transaction — including the event insert — rolls back. This is the invariant that makes `oag
rebuild` safe: every event that currently exists in the `events` table, by construction, already
projected successfully once.

### Concurrency: why `commit_event`'s lock exists

**`commit_local_event` is not safe to call concurrently for the same peer on its own.** Confirmed
live, not assumed: steps 1 and 2-5 are a read-then-write across one transaction, and in SQLite's
WAL mode, two interleaved calls can make the second one hit `SQLITE_BUSY_SNAPSHOT` outright rather
than wait — a `busy_timeout` cannot fix a stale read snapshot by waiting; only restarting the whole
read-then-write can. Before this was understood, 10-13 of 20 concurrent `GraphService` commits for
the same peer failed outright with a raw "database is locked" error — reachable by completely
ordinary concurrent REST/MCP usage (e.g. two simultaneous requests), not a contrived attack.

`GraphService` holds a `tokio::sync::Mutex<()>` (`commit_lock`) and every mutation method commits
through `commit_event`, which acquires it for the duration of the read-then-write. A given peer has
exactly one identity and therefore exactly one logical writer of its own chain ever, so there is no
reason to allow concurrent attempts at the SQLite level at all — the fix removes the possibility
in-process rather than hoping the database's own locking happens to cover it.

## Ingesting a remote event

`ingest_remote_event`:

1. Verify the signature and re-derive `event_id` from the received bytes — reject outright on
   failure (`EventsError::InvalidSignature`, `::Hex`, `::BadSignatureLength` depending on exactly
   what's wrong).
2. Verify the claimed `origin_peer` actually derives from the public key that produced the
   signature (`EventsError::OriginKeyMismatch` if not) — this holds regardless of what any local
   table claims that key maps to, since the check is computed purely from the key's own bytes. You
   cannot forge an event claiming to be from a peer whose key you don't hold, even if a local
   `peers` table has somehow been poisoned with a wrong pairing.
3. Check whether a *different* event already exists at this `(origin_peer, sequence)`. If so, this
   is a **fork** (see below) — neither event is projected.
4. Validate chain continuity: `sequence` must equal this origin's `expected_next_sequence`, and
   `previous_event` must equal this origin's current `head_event_id`
   (`EventsError::SequenceGap`/`::PreviousEventMismatch` otherwise).
5. Same insert → project → advance-head sequence as a local commit, same one-transaction guarantee.

## Fork detection

A fork is two distinct, validly-signed events claiming the same `(origin_peer, sequence)` — either
a bug in the origin peer's own tooling, or a peer deliberately presenting different histories to
different observers. OAG never tries to silently pick a winner:

- Neither event is projected into the graph.
- The origin peer is flagged `forked` in the `peers` table.
- A row is recorded in `peer_forks`, keyed `(peer_id, sequence)` so only the *first* fork detected
  at a given position is ever recorded.
- **The incoming (rejected) event's full signed JSON is retained** (`event_b_signed_json` column) —
  not just its bare id. This was a real gap, found and fixed: the table's own migration comment
  called it "evidence," but only hashes were ever stored, making it useless as actual evidence.
  `oag peer forks <peer_id>` shows it.

Forked events never enter the `events` table, so `oag rebuild`'s full-log replay needs no special
handling for them.

## Event types

| Event | Payload highlights | Produces |
|---|---|---|
| `ASSERT_RELATION` | subject/predicate/object + confidence + extraction method | An `Assertion` (id = this event's id) + its `Edge`/`Node`s, created if missing |
| `ADD_EVIDENCE` | assertion id + evidence fields | An `Evidence` row |
| `VERIFY_ASSERTION` | assertion id + result | An `Observation` row |
| `DISPUTE_ASSERTION` | assertion id + reason | `assertion_disputes` row + `status = Disputed` |
| `RETRACT_ASSERTION` | assertion id + reason | `assertion_retractions` row + `status = Retracted` |
| `SUPERSEDE_ASSERTION` | old + new assertion id | `assertion_supersessions` row + old's `status = Superseded` |
| `ACTOR_DECLARE` | actor type/name/identity_uri/key proof | An `Actor` row |
| `ACTOR_KEY_ADD` | actor id + public key + possession proof | Updates the actor's `public_key` |
| `ACTOR_KEY_REVOKE` | actor id + key hash | Flags that key `revoked` |
| `NODE_ALIAS` | node id + alias + alias type | A `node_aliases` row |

None of these ever delete a row. "Softening" a claim (dispute/retract/supersede) always inserts a
new audit row and flips a status flag on the original — the original assertion and its originating
event stay permanently inspectable. The one deliberate exception is evidence redaction, a narrow,
locally-opt-in departure covered in [17-redaction-and-suppression.md](17-redaction-and-suppression.md).

## Clock independence

Nothing in replication depends on wall-clock synchronization between peers. `sequence` numbers, not
timestamps, establish per-origin order; `created_at` is informational (feeds the `freshness`
ranking signal, see [06-corroboration-and-authority.md](06-corroboration-and-authority.md)) but
never used to determine validity or ordering across origins. A timestamp claimed far in the future
is clamped to the same maximum freshness an honestly-timestamped, genuinely-current claim already
gets — there's no way to gain an advantage by lying about it.

## Tests

`crates/oag-events/src/tests.rs` (commit/ingest/fork/signature/concurrency cases, including
`commit_local_event_itself_is_not_safe_for_concurrent_same_peer_calls`,
`forged_event_lying_about_its_origin_peer_is_rejected`,
`a_wrong_key_on_file_for_a_peer_blocks_their_real_events_rather_than_accepting_forgeries`,
`detects_and_records_fork`); `crates/oag-graph/src/tests.rs`'s
`concurrent_graph_service_commits_from_the_same_peer_all_succeed`.
