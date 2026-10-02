# Event Protocol

Everything in OAG's graph is derived from one thing: a signed, append-only log of events. This
document describes the envelope, the signing/hashing scheme, the per-peer hash chain, the event
types, and how an event becomes a projected row.

## The envelope

```rust
// crates/oag-events/src/envelope.rs
pub struct UnsignedEvent {
    pub version: u16,
    pub origin_peer: String,       // the authoring peer's PeerId, as "oagp_..."
    pub sequence: u64,             // this origin's own monotonic counter, starting at 1
    pub previous_event: Option<String>, // this origin's previous event_id, or None for the first
    pub created_at: i64,
    pub payload: EventPayload,     // flattened — see "Event types" below
}

pub struct SignedEvent {
    pub unsigned: UnsignedEvent,   // flattened
    pub signature: String,         // hex-encoded Ed25519 signature
}
```

`event_id` is deliberately **not** a field on either struct — it's derived, not stored, computed as
`EventId::derive(canonical_json_bytes(&signed_event))` (domain-separated BLAKE3, see
`docs/security.md`). This means the id is a function of the *entire signed envelope*, including the
signature itself: you cannot construct a different signed event that happens to produce the same id,
and you cannot take a validly signed event and swap its signature without changing its id.

## Canonical serialization

Before signing, an event's bytes are produced via RFC 8785 JSON Canonicalization Scheme (JCS, the
`serde_jcs` crate) — spec section 28. JCS guarantees one canonical byte representation for any given
JSON value (sorted keys, fixed number formatting, no insignificant whitespace), so the same logical
event always serializes identically regardless of which peer, language, or JSON library produced it.
Without this, two semantically-identical events could hash to two different `event_id`s depending on
serialization details, breaking both signature verification portability and deduplication.

## Signing

`oag_crypto::sign_with_domain(signing_key, "OAG:EVENT:v1:", canonical_bytes)` — Ed25519 over the
domain prefix concatenated with the canonical bytes. The same domain-separation scheme is reused for
actor key-possession proofs (a different domain string), so a signature that's valid for one purpose
can never be replayed as valid for another. See `docs/security.md` for the full domain-separation
rationale.

## Per-peer hash chain

Each peer maintains its own append-only chain: `sequence` starts at 1 and increments by exactly 1
per event that peer authors; `previous_event` is the `event_id` of that peer's own prior event (or
`None` for its first). This is a **per-origin** chain, not a global one — spec section 30 is explicit
that there is no global event order. Two different peers' events are never ordered relative to each
other; only events from the *same* origin are chained.

`event_origins` (one row per peer this local database has ever seen events from) tracks
`highest_contiguous_sequence`, `highest_seen_sequence`, and `head_event_id` — the bookkeeping needed
to validate the next incoming event from that origin without re-scanning the whole chain.

### Committing a local event

`oag_events::commit::commit_local_event` (called by every `GraphService` write method):

1. Read this peer's own `event_origins` row for its `highest_contiguous_sequence`/`head_event_id`.
2. Build `UnsignedEvent { sequence: head + 1, previous_event: head_event_id, ... }`, canonicalize,
   sign.
3. `INSERT INTO events` (the row is now permanent).
4. Run the projector (see below) against the same payload.
5. Advance this peer's own `event_origins` head.

Steps 3-5 happen inside **one SQLite transaction**. If projection fails for any reason, the entire
transaction — including the event insert — rolls back. This is the invariant that makes `oag
rebuild` safe: every event that currently exists in the `events` table, by construction, already
projected successfully once, in exactly the order it was inserted.

### Ingesting a remote event

`oag_events::ingest::ingest_remote_event`:

1. Verify the signature and re-derive `event_id` from the received bytes — if either check fails,
   reject outright.
2. Verify the claimed `origin_peer` actually matches the public key that produced the signature —
   you cannot forge an event claiming to be from a peer whose key you don't hold.
3. Check whether a *different* event already exists at this `(origin_peer, sequence)`. If so, this
   is a **fork** — both events are kept, recorded in `peer_forks`, and neither is projected or
   silently preferred over the other.
4. Validate chain continuity: `sequence` must equal this origin's `expected_next_sequence`, and
   `previous_event` must equal this origin's current `head_event_id`. A gap or mismatch is rejected
   (`SequenceGap`/`PreviousEventMismatch`) rather than silently accepted out of order.
5. Same insert → project → advance-head sequence as a local commit, same one-transaction guarantee.

## Event types

| Event | Payload | Produces |
|---|---|---|
| `ASSERT_RELATION` | subject/predicate/object + actor confidence + extraction method | An `Assertion` (its id *is* this event's id) and the `Edge`/`Node`s it references, created if missing |
| `ADD_EVIDENCE` | assertion id + evidence type/uri/title/excerpt/content_hash | An `Evidence` row (its id *is* this event's id) |
| `VERIFY_ASSERTION` | assertion id + result (confirmed/contradicted/unreachable) | An `Observation` row |
| `DISPUTE_ASSERTION` | assertion id + reason | A row in `assertion_disputes` + `Assertion.status = Disputed` |
| `RETRACT_ASSERTION` | assertion id + reason | A row in `assertion_retractions` + `Assertion.status = Retracted` |
| `SUPERSEDE_ASSERTION` | old assertion id + new assertion id | A row in `assertion_supersessions` + old assertion's `status = Superseded` |
| `ACTOR_DECLARE` | actor type, name, identity_uri, optional public-key proof | An `Actor` row |
| `ACTOR_KEY_ADD` | actor id + public key + possession proof | Updates the actor's `public_key` |
| `ACTOR_KEY_REVOKE` | actor id + key hash | Flags that key `revoked`; past events it already signed stay valid, it just can't authenticate new requests |
| `NODE_ALIAS` | node identifier + alias + alias type | A row in `node_aliases` |

None of these events ever delete a row. "Softening" a claim (dispute/retract/supersede) always means
inserting a new audit-trail row and flipping a status flag on the original — the original assertion
and its originating event are permanently inspectable (spec section 37, and see `docs/data-model.md`
for why `Assertion.status` is explicitly documented as "a derived, overwritable cache," not the
source of truth). The one exception — evidence redaction — is a deliberate, narrowly-scoped, locally-
opt-in departure from this rule, covered in full in `docs/security.md`.

## Verification, not consensus

There is no global ordering, no leader, and no voting on which events are "real" (spec section 6).
A peer accepts any event that: verifies cryptographically, matches its claimed origin, and continues
that origin's chain without a gap or fork. Whether to *believe* an accepted event's content is a
completely separate question, answered by corroboration signals computed over whatever assertions
and evidence currently exist — see `docs/data-model.md`'s "Ranking signals" section.

## Rebuilding the projection

Because every event in the log is guaranteed (by the one-transaction commit/ingest invariant above)
to have projected successfully once already, replaying every event in the exact order the `events`
table's own `rowid` gives — not grouped by origin — reproduces the exact same projected state.
Cross-origin references (e.g. one peer's `ADD_EVIDENCE` targeting another peer's assertion) are only
guaranteed resolvable in original insertion order, which is why `oag rebuild` uses `ORDER BY rowid`
rather than replaying each origin's chain independently. See `docs/architecture.md`'s "Backup and
rebuild" section and `docs/security.md` for how this interacts with redaction.
