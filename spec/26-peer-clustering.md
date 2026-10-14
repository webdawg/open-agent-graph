# Peer Clustering

**Source**: `crates/oag-sync/src/cluster.rs`, `crates/oag-sync/src/gossip.rs`,
`crates/oag-storage/migrations/0011_peer_cluster.sql`,
`crates/oag-storage/src/repo/peer_cluster.rs`, `crates/oag-cli/src/config.rs`'s `ClusterSection`,
`crates/oag-sync/src/service.rs`'s `federation_allows`/`same_cluster`/`cluster_peer_addrs`.

Opt-in (`[cluster] name = "..."` in `config.toml`, unset by default — zero behavior change for
every deployment that doesn't configure one). The other half of the "ant colony" founding
statement (`USER_INPUT_RECORD.md` Entry 2: "clusters of mutually cooperating nodes") that ephemeral
peer trust (`spec/24`) didn't cover — this is the first concept in this project about
*relationships between* peers, rather than a property of one peer alone.

## Decision: pure networking construct, not a graph concept

The graph layer (`oag-core`/`oag-graph`) could represent a cluster as an ordinary `Node` with
`member_of` `Edge`s to peer-nodes with zero new plumbing — the assertion pipeline, open
`NodeType`/`Predicate` strings, and triple-hash-derived edges already support exactly that shape.
**This was considered and explicitly declined** (`USER_INPUT_RECORD.md` Entry 6): clustering stays
entirely inside `oag-sync`/`oag-storage`. No `oag-core`/`oag-graph` changes exist for this feature,
and none are planned for this phase.

## Self-declared, signed membership

`cluster.rs` mirrors `presence.rs`'s sign/verify pipeline exactly (canonical JSON + domain-
separated Ed25519, same `(peer_id, public_key)` pairing re-derivation check as
`discover_peers`/`presence::verify`) but is a fully independent module, not a reuse of
`presence.rs`'s types — clustering must work for a permanent identity exactly as well as an
ephemeral one, so it needs its own sender rather than being accidentally coupled to Phase 1's
ephemeral-only heartbeat opt-in.

`ClusterDeclaration { peer_id, public_key, cluster, timestamp, signature }`, domain prefix
`OAG:CLUSTER:v1:`. `POST /oag/sync/v1/cluster` verifies and upserts `peer_cluster_claims` (an
ordinary upsert, unlike `node_gravity`'s `INSERT OR IGNORE` — a claimed cluster is current status,
so a later declaration legitimately overwrites an earlier one). `SyncService::with_cluster`
(independent of `with_minimum_trust`/`with_heartbeat_identity`) opts a service into broadcasting
its own declaration via `cluster::spawn_cluster_announce_loop`, on its own configurable interval
(`[cluster].announce_interval_seconds`, default 60s).

## The federation override — and its security caveat

`SyncService::federation_allows` checks `same_cluster(peer_id)` *first*, before the existing
binary `FederationPolicy` check and the Phase 1 trust floor. A same-cluster peer is **always**
allowed — a full override, not a nudge:

```rust
pub async fn federation_allows(&self, peer_id: &PeerId) -> bool {
    if self.same_cluster(peer_id).await { return true; }
    // ... existing FederationPolicy + trust-floor checks, unchanged
}
```

`same_cluster` compares the candidate's latest `peer_cluster_claims` row against this service's own
configured `[cluster].name`. This is a bare string comparison against a self-asserted claim with
**no corroboration and no authorization check** — both explicit decisions
(`USER_INPUT_RECORD.md` Entry 6):

- **No corroboration**: declining the graph-level representation (above) means there is no
  `source_independence`/`identity_assurance`-style signal backing a cluster claim. One signature
  from the claiming peer is the entire evidentiary basis.
- **No authorization**: any peer can self-declare any cluster name. There is no admin key, no
  permission type, no gatekeeping on the claim's content.

**The consequence, stated plainly rather than hidden**: a cluster name functions as a shared,
*unauthenticated* label. Any peer that learns or guesses your configured cluster name can declare
the same one and receive the identical full-trust federation override — bypassing a static
`FederationPolicy::Allowlist` entirely, not just lowering a trust floor. This is accepted, not
mitigated, in this phase. `future_peer_clustering.md`'s own open question before this phase was
built — "does intra-cluster replication need the same rigor as `FederationPolicy::Open`/
`Allowlist`?" — was answered explicitly: no, not this phase. Treat `[cluster].name` the way you'd
treat a weak shared secret, not a security boundary, until a stronger (likely graph-level,
evidence-backed) version exists.

## CLI

`oag peer list` gains a `cluster=<name|->` column, reading the same `peer_cluster_claims` table.

## Phase 2: differentiated sync cadence for same-cluster peers

The user picked up this feature from the backlog directly (`AskUserQuestion`, `USER_INPUT_RECORD.md`
Entry 14). On inspection, both open questions `future_peer_clustering.md` originally posed —
"first-class graph concept or pure networking?" and "does intra-cluster replication need extra
rigor?" — turned out to already be answered by Phase 1, above (pure networking; no, not this
phase). The one item genuinely still unbuilt from Phase 1's own "Explicitly deferred" list below is
differentiated sync *cadence*, which this phase adds.

`SyncService::cluster_peer_addrs` (`service.rs`) returns every known address belonging to a peer
whose latest signed cluster claim matches this service's own configured cluster — empty whenever no
cluster is configured. `gossip::spawn_cluster_gossip_loop` is a **second, independent** gossip loop
(not a parameter on the existing one) that syncs only against those addresses, on its own interval
(`[cluster].gossip_interval_seconds`, default 5s — meaningfully faster than `[network]`'s own 30s
default `sync_interval_seconds`). The standard `spawn_gossip_loop` keeps running unchanged, at its
own interval, for everyone, clustered or not — this is a strictly additive faster cadence for a
cluster's own members, not a replacement for anything. Both loops share the exact same per-address
sync/log/gravity-slowdown behavior (`gossip::sync_round`), so the only difference between them is
*which* addresses and *how often*.

`oag serve` spawns this loop unconditionally alongside `spawn_cluster_announce_loop`, inside the
same `if let Some(cluster_name) = ...` block — no separate gate is needed, since an unconfigured
cluster already makes `cluster_peer_addrs` return nothing, and the whole block is skipped anyway
when `[cluster].name` is unset.

## Explicitly deferred

- Any corroboration or verification of a claimed cluster name beyond proof that the claiming peer
  signed it.
- Multiple simultaneous cluster memberships per peer — v1 is one cluster name per peer, matching
  the "one cluster per physical location" framing already in `future_peer_clustering.md`.
- Tying `gossip_interval_seconds`'s *default* to `sync_interval_seconds`'s configured value
  programmatically — both are independent, operator-tunable knobs.
- The graph-level representation considered and declined above, should a stronger, corroborated
  version ever be wanted.

## Tests

`cluster.rs` (sign/verify round-trip, tampered-cluster-name rejection, mismatched
`peer_id`/`public_key` pairing rejection, stale-timestamp rejection — mirrors `presence.rs`'s tests
exactly). `peer_cluster.rs` repo tests (a missing claim reads as `None`; a later declaration
overwrites an earlier one — the opposite of `node_gravity`'s stability guarantee, confirming this
is current status, not a stable trait). `oag-sync`'s
`matching_cluster_declaration_overrides_an_allowlist_rejection` integration test: a real permanent-
identity peer running `FederationPolicy::Allowlist` that excludes a second peer, confirmed rejected
before any declaration arrives, then confirmed allowed the instant a real signed declaration
matching its own configured cluster arrives over real HTTP — and confirmed a *different* claimed
cluster from a third peer does not trigger the override. Live: two real `oag serve` instances with
`[cluster].name = "home-lab"`, confirmed the startup log shows the configured cluster, confirmed
`oag peer list`'s cluster column reflects each side's real received declaration after an ordinary
sync round.

Phase 2's new tests, `oag-sync`'s `tests.rs`: `cluster_peer_addrs_is_empty_without_a_configured_cluster`;
`cluster_peer_addrs_only_includes_matching_cluster_members` (same three-peer, two-cluster setup as
the Phase 1 override test, now checking the address list the new loop drives off of);
`same_cluster_peers_sync_on_their_own_faster_cadence` — two real, same-cluster `SyncService`s over
real HTTP, A's *standard* gossip loop deliberately given a far-too-slow interval (1 hour) so it
cannot explain anything observed, A's *cluster* loop given a short one (200ms); B creates a new
assertion without A's test code ever calling `sync_with_peer` directly, and the assertion is
confirmed present on A shortly after — proof the cluster-only loop, not the standard one and not a
direct test call, is what delivered it.
