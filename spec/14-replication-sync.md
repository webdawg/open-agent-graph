# Replication / Sync Protocol

**Source**: `crates/oag-sync/src/{server,service,client,gossip,federation,wire,rate_limit}.rs`.

## Wire protocol (`/oag/sync/v1/*`)

Deliberately unauthenticated — every event is self-authenticating via its own signature, so there's
no natural per-caller identity to gate behind a key.

| Endpoint | Purpose |
|---|---|
| `GET /hello` | This peer's `peer_id`/`public_key`/`heads` (per-origin highest contiguous sequence) |
| `GET /heads` | The cheap-to-poll subset of `hello` |
| `GET /events/{origin}?from=&to=` | Fetch a range of one origin's events |
| `POST /events` | Push a batch of events (protocol completeness; gossip/tests use pull as primary) |
| `GET /peers` | Every peer this node knows about, with addresses |
| `GET /replication-status` | Durability view |

## `sync_with_peer` — the pull path

1. `hello(addr)` → learn the remote's `peer_id`/`public_key`/`heads`.
2. **Verify `remote_peer_id == PeerId::from_public_key(remote_public_key)`** before storing
   anything — see [01-identity.md](01-identity.md).
3. `upsert_peer` + `add_address` for the directly-contacted peer; persist its claimed heads for
   durability tracking.
4. `discover_peers(addr)` — learn about *other* peers transitively through this one (see below).
5. For every origin in `hello.heads` whose key this peer knows (directly, or just discovered),
   fetch and ingest any events past the local head.

## `discover_peers` — gossip, and two real bugs found here

Learns about peers transitively through a relay, without ever contacting them directly — this is
what lets a peer later fetch a *third* peer's history through a relay it has never spoken to.
`MAX_PEERS_PER_RESPONSE` (200) and `MAX_ADDRESSES_PER_PEER` (5) bound how much of one `/peers`
response gets acted on.

### Bug 1 — missing peer_id/public_key consistency check

Unlike `sync_with_peer`'s own `hello` path, `discover_peers` originally stored *any* claimed
`(peer_id, public_key)` pairing with no check that the two actually derive from each other. A
malicious relay could offer a real third party's `peer_id` paired with the *attacker's own*
public key.

That pairing alone does **not** let the attacker get a *forged* event accepted under the victim's
identity — `ingest_remote_event`'s `OriginKeyMismatch` check derives the verifying peer id purely
from the key's own bytes, never from what a local table claims it maps to (see
[02-event-protocol.md](02-event-protocol.md)). What a bad pairing actually breaks is this peer's
ability to accept the *real* victim's own legitimate events afterward — they'd fail signature
verification against the wrong key — a denial-of-replication effect against whichever peer's
identity got poisoned, not an impersonation one. Both findings were confirmed by test rather than
assumed, including deliberately re-deriving the original (wrong) severity claim and correcting it
in the project's own records once the milder-but-still-real truth was confirmed.

**Fix**: `discover_peers` now makes the identical check `sync_with_peer`'s `hello` path always
has, before ever calling `upsert_peer`.

### Bug 2 — no cap on total known peers

`MAX_PEERS_PER_RESPONSE` bounds one gossip round, not the `peers` table's lifetime size. The
gossip loop re-syncs with every known address forever (see below), so a single malicious relay
feeding 200 fresh identities every round would grow `peers`/`peer_addresses` on disk without
bound — generating a real Ed25519 keypair is free for an attacker, so the consistency check above
doesn't stop this either; it only catches lying about an *existing* pairing, not minting unlimited
new, individually-valid ones.

**Fix**: a global `MAX_TOTAL_KNOWN_PEERS` (10,000) cap, checked once per `discover_peers` call. Once
a peer's table is full, it stops learning brand-new peers via gossip; already-known ones still
refresh normally through `sync_with_peer`'s own direct `hello` handshake, which isn't gated by
this.

## Gossip loop

Runs forever: every `sync_interval`, syncs with every bootstrap address plus every address learned
from every known peer so far. This is what makes bug 2 above a real, not just theoretical, growth
vector — a single tick's discovery feeds into the *next* tick's sync targets.

## Federation policy

Accepting an event into local storage is a different question from trusting its *content*.
`FederationPolicy` controls only the first:

- `Open` (default) — accept replicated events from any peer that can produce a validly signed
  event.
- `Allowlist(peers)` — only accept events from an explicit set of peer ids.

Either way, an accepted event is still just a signed claim — corroboration signals (see
[06-corroboration-and-authority.md](06-corroboration-and-authority.md)) determine how much weight
a reader gives it, not federation policy. A wide-open federation policy plus careful
corroboration-aware reading is the deliberate design: public-network abuse is expected, not
prevented by gatekeeping who can talk to you.

## Durability / replication-factor tracking

`replication_status()` reports `known_peer_count` and `peers_fully_caught_up` — a monitored network
property (never assume any one peer's own storage is reliable), populated from `hello`'s
self-reported heads, persisted via `upsert_known_head`.

## Resource limits on this surface

See [18-rate-limiting-and-resource-limits.md](18-rate-limiting-and-resource-limits.md) for:
`MAX_PUSH_BODY_BYTES` (server), the sync HTTP *client*'s own response-size cap (a peer syncing
*with* you is exactly as untrusted as one syncing *with* you), `SyncRateLimiter`'s global
600-req/60s counter, and `MAX_EVENTS_PER_PUSH`.

## Tests

`crates/oag-sync/src/tests.rs` — `two_peer_replication`, `partition_reconnect_converges`,
`peer_restart_resumes_and_converges`, `relay_without_origin_dependency`,
`gossip_with_a_mismatched_peer_id_and_public_key_is_rejected`,
`forged_event_lying_about_its_origin_peer_is_rejected`,
`a_wrong_key_on_file_for_a_peer_blocks_their_real_events_rather_than_accepting_forgeries`,
`discover_peers_does_not_grow_the_table_past_the_global_cap`; `tests/distributed_convergence.rs`'s
`fifty_peers_converge_after_chaos` (50 real peers, kills/restarts/partitions/heals, run explicitly
with `--ignored`).
