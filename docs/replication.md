# Replication

OAG has no leader, no consensus round, and no global event order (spec sections 5-7, 30). Peers
replicate by **pulling** events from each other on a schedule and merging what they receive into
their own local event log. This document describes the wire protocol, the gossip loop, fork
handling, and durability tracking.

## The `/oag/sync/v1/*` API (spec section 67)

Deliberately unauthenticated at the HTTP layer (`crates/oag-sync/src/server.rs`) — every event is
self-authenticating via its own Ed25519 signature (spec section 60), so a session credential
wouldn't protect anything the signature check doesn't already cover. Hardened instead against abuse
(spec section 61) via a request-rate cap, a whole-body size cap, and per-handler result caps:

| Route | Method | Purpose |
|---|---|---|
| `/oag/sync/v1/hello` | GET | Protocol/version handshake, this peer's id, public key, and current heads |
| `/oag/sync/v1/heads` | GET | This peer's per-origin `(sequence, event_id)` heads — what it currently has |
| `/oag/sync/v1/events/{origin}?from=&to=` | GET | A range of one origin's events, capped at 1000 per call — a well-behaved caller paginates by advancing `from` rather than requesting huge ranges in one call |
| `/oag/sync/v1/events` | POST | Push events to this peer (validated exactly like `ingest_remote_event` — see `docs/event-protocol.md`) |
| `/oag/sync/v1/peers` | GET | Peer addresses this peer knows about, for transitive discovery |
| `/oag/sync/v1/replication-status` | GET | This peer's durability view — see "Durability tracking" below |

## Peer bootstrap and discovery

A peer starts with zero known addresses unless `[network].bootstrap_peers` in `config.toml` lists
some, or an operator runs `oag peer add <address>` (spec section 45). From there, `/peers` responses
from any contacted peer supply further addresses transitively (spec section 46) — a fully-connected
mesh is not required; knowing a handful of peers who each know a handful more is enough for events to
eventually reach everyone via gossip.

## Gossip (anti-entropy), not broadcast

`spawn_gossip_loop` (`crates/oag-sync/src/gossip.rs`) runs on a fixed interval
(`[network].sync_interval_seconds`, default 30s): for every known peer address (bootstrap-configured
plus anything discovered so far), fetch its heads, compare against local heads, and pull whatever
range of events is missing (spec section 51). One unreachable peer never blocks or fails the others
— each address is tried independently, and the next tick tries again. This is intentionally slow and
resilient rather than instantaneous: a partition heals itself gradually as connectivity returns
(spec section 52), with no special partition-recovery logic needed beyond "keep gossiping."

`SyncService::sync_with_peer` (`crates/oag-sync/src/service.rs`) is the one-shot version of this same
logic, also reachable directly via `oag peer sync <address>`.

## Federation policy vs. event trust

`FederationPolicy` (`Open` or `Allowlist(peer_ids)`, `crates/oag-sync/src/federation.rs`) gates
*whose* events this peer's gossip loop and push endpoint will accept into storage at all. It has
nothing to do with how much a reader should *believe* an accepted event's content — see
`docs/security.md`'s "Federation trust vs. data trust" section and `docs/data-model.md`'s ranking
signals for that half of the picture.

## Duplicate events and fork detection

Receiving the same event twice (e.g. from two different gossip peers) is a no-op — `event_id` is the
primary key, so a duplicate insert is simply rejected without disturbing anything (spec section 53).

A **fork** is different: two distinct events both claiming the same `(origin_peer, sequence)`. This
can only happen if an origin peer signed two different events at the same chain position — either a
bug, or a peer deliberately trying to present different histories to different peers. OAG does not
try to silently pick a winner: both events are kept, the fork is recorded in `peer_forks`, and
neither is projected into the graph until an operator investigates (spec section 55). This is a
direct consequence of per-peer hash chains rather than a global order — see `docs/event-protocol.md`.

## Clock independence

Nothing in replication depends on wall-clock synchronization between peers (spec section 56).
`sequence` numbers, not timestamps, establish per-origin order; `created_at` is informational
(used for the `freshness` ranking signal) but never used to determine whether an event is valid or
which of two events came "first" across different origins.

## Durability / replication-factor tracking

There is no fixed replica count enforced by consensus — instead, `target_replication_factor`
(`crates/oag-sync/src/replication.rs`) scales with how many peers the network currently has:

```rust
pub const MIN_REPLICATION_FACTOR: usize = 3;
pub fn target_replication_factor(known_peer_count: usize) -> usize {
    ceil(sqrt(known_peer_count)).max(MIN_REPLICATION_FACTOR)
}
```

At 10 known peers the target is 4; at 100 it's 10; at 10,000 it's 100 — durability expectations grow
with the network rather than staying fixed, on the premise that a bigger network can and should
spread risk further, without ever requiring a majority-write quorum the way a consensus system would.
`oag replication status` / `GET /oag/sync/v1/replication-status` reports `ReplicationStatus`: known
peer count, the computed target, how many peers are fully caught up, and which are lagging
(`LaggingPeer`) — an operational signal, not an enforced guarantee.

## Additional transports

`oag-sync`'s HTTP protocol is the default and only required transport. `oag-reticulum` (opt-in,
`[reticulum].enabled = true`) adds peer discovery and event relay over the Reticulum mesh-networking
protocol as an *additional* path, for links far more hostile than a typical data-center network
assumes — it duplicates rather than shares `oag-sync`'s pull-sync logic, deliberately, so this
optional, pre-1.0 dependency's integration never risks the proven HTTP path.
