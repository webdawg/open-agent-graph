# Ephemeral Peer Trust

**Source**: `crates/oag-sync/src/presence.rs`, `crates/oag-sync/src/trust.rs`,
`crates/oag-storage/migrations/0009_peer_presence.sql`,
`crates/oag-storage/src/repo/peer_presence.rs`, `crates/oag-cli/src/config.rs`'s
`IdentitySection`/`resolve_identity_section`, `crates/oag-cli/src/serve.rs`.

Opt-in (`[identity] ephemeral = true` in `config.toml`, default `false` — permanent,
`identity.key`-backed identity, same as before this milestone, stays the default for every
existing deployment). When on, `oag serve` generates a fresh `PeerIdentity` every process start
and never reads or writes `identity.key` at all — a clean fork at startup
(`serve.rs::run`), not a new mode layered onto the persistent path. Since the cryptographic
identity resets every restart, trust can no longer attach to the key — this milestone replaces
that with trust earned through sustained, independently-verifiable presence, inspired by the PKT
Network whitepaper's Route Server. The verbatim founding statements — "all it has is its data - if
it can use that to try to prove its identity - it can try," the "frozen in ice" latency-tolerance
framing, and the specific PKT mechanisms borrowed — are `USER_INPUT_RECORD.md` Entry 0; Entries 1
and 4 are the follow-up decisions made while actually planning this milestone
(`future_ephemeral_peer_trust.md` memory has the same material, kept as a secondary
cross-reference, not the primary source).

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
a second cliff, matching the "frozen in ice" latency-tolerance framing already on record
(`USER_INPUT_RECORD.md` Entry 0).

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

`oag identity backup`/`restore` (built before this milestone existed) exist to protect a durable
identity worth protecting — their core rationale weakens, though doesn't disappear, once ephemeral
identity exists as an alternative mode: there's deliberately nothing to back up or restore for a
peer that never intends to keep its identity past one process lifetime. Still useful for
pinning/testing scenarios and for the permanent-identity mode, which remains this project's
default.

## CLI

`oag peer list` gains a `trust=` column, computed the same way enforcement does
(`trust::compute_trust`), using default trust-check windows rather than whatever a running `oag
serve` was configured with — a read-only snapshot for a human to inspect, not itself enforcement.

## Phase 2: live config reload for the trust-check windows

`SyncService::trust_config` (`heartbeat_interval_seconds`/`trust_rebuild_seconds`/
`announced_restart_grace_seconds`) is `Arc<std::sync::RwLock<TrustConfig>>`, not a plain field —
every clone of a `SyncService` (the sync HTTP router, the gossip loop, the heartbeat loop, all
taken once at `oag serve` startup) shares the same underlying value, so a reload through any one
handle is visible to all of them. `SyncService::update_trust_config` replaces it; `federation_allows`
copies the small `Copy` value out under a read lock immediately before its one `.await` (a
`RwLockReadGuard` isn't `Send`, so it couldn't be held across the await regardless).

`oag serve` spawns `reload_trust_config_periodically` (`serve.rs`) only when `[identity].ephemeral
= true` *and* a `--config <path>` was actually given (nothing to reload from otherwise) — it wakes
every `TRUST_CONFIG_RELOAD_INTERVAL` (a fixed 30s, deliberately *not* itself hot-reloadable, to
avoid a "how often do we check for changes to the check-interval" regress), re-reads just
`[identity]` via the new `resolve_identity_section`, and unconditionally calls
`update_trust_config` — re-applying an unchanged value is a harmless no-op, so there's no diffing
logic to get wrong. A config file that's transiently invalid (e.g. mid-edit) is tolerated: the
parse error is swallowed for that tick and simply retried at the next one, never crashing the
running peer.

## Explicitly deferred

A literal multi-peer ballot/quorum/consensus protocol (the address-correlation heuristic above is
this milestone's full scope for "voting right"); PacketCrypt-style merkle-commit-and-random-
challenge proof of data volume; any token/blockchain machinery. See `future_ephemeral_peer_trust.md`
for what's tracked and why. (Live config reload, formerly listed here, is Phase 2 above.)

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

**Phase 2**: `oag-sync`'s `update_trust_config_is_observed_by_a_clone_taken_before_the_update` —
constructs a `SyncService`, clones it (mimicking the router/gossip/heartbeat-loop clones `oag
serve` takes at startup), calls `update_trust_config` with a different `TrustConfig` on the
*original* handle, and confirms the *clone* (not just the original) observes the change through
`federation_allows` — proving the `Arc<RwLock<_>>` sharing works across clones, which is the one
thing that actually matters for this to work in the real `oag serve` process. Live: a real `oag
serve --config <path>` with `identity.ephemeral = true` ran correctly with the reload loop active,
survived a real edit to `trust_rebuild_seconds` across a reload tick, and survived a config file
left transiently invalid (mid-edit) across a tick without crashing, recovering cleanly once valid
TOML was restored.
