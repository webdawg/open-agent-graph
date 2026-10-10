# Ephemeral Peer Trust, Phase 1

**Source**: `crates/oag-sync/src/presence.rs`, `crates/oag-sync/src/trust.rs`,
`crates/oag-storage/migrations/0009_peer_presence.sql`,
`crates/oag-storage/src/repo/peer_presence.rs`, `crates/oag-cli/src/config.rs`'s
`IdentitySection`, `crates/oag-cli/src/serve.rs`.

Opt-in (`[identity] ephemeral = true` in `config.toml`, default `false` — permanent,
`identity.key`-backed identity, same as before this milestone, stays the default for every
existing deployment). When on, `oag serve` generates a fresh `PeerIdentity` every process start
and never reads or writes `identity.key` at all — a clean fork at startup
(`serve.rs::run`), not a new mode layered onto the persistent path. Since the cryptographic
identity resets every restart, trust can no longer attach to the key — this milestone replaces
that with trust earned through sustained, independently-verifiable presence, inspired by the PKT
Network whitepaper's Route Server (see `future_ephemeral_peer_trust.md` memory for the full
founding reasoning and `USER_INPUT_RECORD.md` Entries 1 and 4 for the verbatim founding
statements).

## Signed presence heartbeats

`presence.rs` defines `PresenceHeartbeat` (`peer_id`, hex `public_key`, `status: Online |
GoingOffline`, `session_started_at`, `timestamp`), signed exactly the way `oag-events` already
signs an event envelope: canonical JSON (RFC 8785) over the unsigned struct, then
`sign_with_domain`/`verify_with_domain` with a dedicated domain prefix (`OAG:PRESENCE:v1:`) so a
presence signature can never be replayed as an event signature or vice versa. `verify` re-derives
`PeerId::from_public_key` and checks it against the claimed `peer_id` — the exact same
pairing-validation `discover_peers` already applies to a gossiped peer record, against the same
spoofed-pairing risk — and rejects a timestamp more than `MAX_CLOCK_SKEW_SECONDS` (300s) from now.

A heartbeat only ever claims its own identity's `session_started_at`, repeated on every beat —
this, not the heartbeat's own `timestamp`, is what lets a receiver compute *continuous*-presence
duration from a single stored row (`peer_presence`), rather than needing a history of every past
heartbeat.

`SyncService::broadcast_presence` builds and pushes one fresh heartbeat to every known peer
address (best-effort, one unreachable address never blocks the others), only when
`with_heartbeat_identity` was called. `presence::spawn_heartbeat_loop` (mirrors
`gossip::spawn_gossip_loop`'s exact shape) calls it on a configurable interval
(`[identity].heartbeat_interval_seconds`, default 30s). `oag serve` calls
`with_graceful_shutdown` with a SIGTERM/Ctrl-C handler that, in ephemeral mode, broadcasts one
final `GoingOffline` heartbeat before actually stopping.

## Trust scoring — "reliance, not paranoia"

`trust.rs::trust_score` is a pure function with no notion of *why* a peer looks unreliable — a
peer degraded by buggy hardware is scored identically to a malicious one, from the same observed
signal (`USER_INPUT_RECORD.md` Entry 4). Exactly `0.0` is the only hard floor: a `forked` peer
(the existing `peers.forked`/`peer_forks` signal, reused as-is) or one this observer has never
received a single heartbeat from at all. Above that floor, trust is a continuum: it rises from a
starting point toward `1.0` over `trust_rebuild_seconds` of continuous heartbeats, and — if
heartbeats go quiet beyond `heartbeat_interval_seconds × 3` — decays back down *patiently*, not as
a second cliff, matching the "frozen in ice" latency-tolerance framing already on record.

Trust is **per-observer**: every peer computes its own score for every other peer, purely from
its own `peer_presence` row for them. There is no globally agreed trust value anywhere in this
system, and `trust.rs::compute_trust` never reads another peer's opinion of a third peer.

## The address-correlated "announced restart" grace window

This milestone's concrete reading of "established peers get a voting right" (`USER_INPUT_RECORD.md`
Entry 4): since identity is deliberately unlinkable across a rotation, no peer can *prove* a new
`peer_id` is the same operator as one that just said it was leaving. So the leniency attaches to
network *address*, not identity: when a peer receives a `GoingOffline` heartbeat, it records every
address it currently knows for that sender in `address_going_offline` (`peer_presence.rs`). If a
brand-new `peer_id`'s first heartbeat later arrives and that `peer_id`'s own known address matches
one in the table within `announced_restart_grace_seconds`, `compute_trust` starts that peer's trust
curve from `ANNOUNCED_RESTART_STARTING_TRUST` (0.3) instead of `0.0` — many independent observers
each quietly extending the same leniency on their own judgment, with no coordination between them,
is the "voting." This is a soft, local, unprovable heuristic, not a cryptographic link between the
old and new identity — deliberately, since a provable link was explicitly decided against (see
"chain continuity" below).

## Federation enforcement — "degrade, don't drop"

`SyncService::federation_allows` keeps the existing binary `FederationPolicy` (`Open`/`Allowlist`)
check exactly as it was, unconditionally first. Only when `minimum_trust > 0.0` — set to
`EPHEMERAL_MINIMUM_TRUST` (`f32::MIN_POSITIVE`) only by `oag serve`'s ephemeral startup path, a
pure no-op for every other deployment — does a second, narrower check run at all: `trust_score(...)
>= minimum_trust`. Because the only value that threshold is ever set to is just above `0.0`, this
is a two-tier floor, not a second cliff: the *only* peers ever hard-rejected are forked or
never-seen ones (score exactly `0.0`); everything else, however low its nonzero trust, is still
accepted. Wired at both existing enforcement points this project already had
(`service.rs`'s outbound `sync_with_peer`, `server.rs`'s inbound `post_events`) — no new
enforcement architecture, one added condition on the existing one.

## Chain continuity on rotation — deliberately not built

An old `peer_id`'s rows (`events`, `event_origins`, `peers`, `tensor_pads`, `peer_presence`) become
historical and stale the moment that identity's process exits; nothing links them to whatever new
`peer_id` that operator starts next. No succession proof, no migration tool. This was an explicit
decision, not an oversight — see `future_ephemeral_peer_trust.md` for the full reasoning (a feature
whose point is needing *less* identity machinery shouldn't grow new crypto to patch over the gap it
deliberately leaves).

## CLI

`oag peer list` gains a `trust=` column, computed the same way enforcement does
(`trust::compute_trust`), using default trust-check windows rather than whatever a running `oag
serve` was configured with — a read-only snapshot for a human to inspect, not itself enforcement.

## Explicitly deferred

A literal multi-peer ballot/quorum/consensus protocol (the address-correlation heuristic above is
this milestone's full scope for "voting right"); true live/hot config reload without a restart;
PacketCrypt-style merkle-commit-and-random-challenge proof of data volume; any token/blockchain
machinery. See `future_ephemeral_peer_trust.md` for what's tracked and why.

## Tests

`presence.rs` (sign/verify round-trip, tampered-field rejection, mismatched `peer_id`/`public_key`
pairing rejection, stale-timestamp rejection). `trust.rs` (hand-checkable: forked and never-seen
peers are exactly `0.0`; a cold start rises linearly to `1.0` over the rebuild window; an announced
restart starts above zero but below full trust; a gone-quiet peer decays patiently rather than
cliff-dropping). `peer_presence.rs` (first-heartbeat-sets-session-start, later ones don't reset it;
going-offline is keyed by address, not peer_id). `oag-sync`'s
`ephemeral_peers_build_real_trust_from_signed_heartbeats` integration test: two real peers over
real HTTP, discovering each other via an ordinary sync round, exchanging real signed heartbeats on
a fast loop, each independently computing nonzero trust for the other purely from its own
observations, then confirming the two-tier floor (a low-but-nonzero-trust peer still passes; the
instant `peers.forked` is set, it's hard-rejected regardless of heartbeat history). Live: two real
`oag serve` instances with `identity.ephemeral = true`, confirmed a fresh `peer_id` each start,
watched `oag peer list`'s trust column rise from real heartbeat exchange, and confirmed `kill
-TERM` triggers the going-offline broadcast and the receiving side records it in
`address_going_offline`.
