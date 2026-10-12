# Future Work

Deliberately not built yet. None of these change anything described elsewhere in this
specification today — each is tracked so a future session picks up the actual reasoning rather
than re-deriving it, the same motivation behind this whole `spec/` directory existing.

## Legacy compatibility as a binding principle (already true, Phase 1 of the mechanism built)

**Binding now, not deferred**: OAG must never become its own gatekeeper. No future version may
require old data, old clients, or old protocols be upgraded or abandoned to keep working. This
is already true by construction in several places — `Predicate` is an open string, not a closed
enum; `canonicalize_value` passes an already-scheme-prefixed identifier through unchanged rather
than forcing a house format; the event log never deletes or rewrites anything. See
`USER_INPUT_RECORD.md`, Entry 1.

**Phase 1 and 2 implemented** — granular, per-artifact data versioning for nodes/edges/assertions/
evidence (Phase 1) and actors/node-aliases/disputes/retractions/observations (Phase 2): an
explicit `schema_version` on every row, with the in-memory shim applied automatically on every
read for all nine types, plus an explicit migrate-in-place action
(`oag migrate <type> <id> --force`) for the five types that have a pre-existing singular
get-by-id lookup to extend (node/edge/assertion/evidence/actor). See
`spec/27-artifact-versioning.md`. Still not built: `assertion_supersessions` (deliberately —
its row type has no typed read path to attach a shim to, see `spec/27`'s "Scope" section), and
versioned `EventPayload` variants for a genuinely breaking (non-additive) payload shape change —
`spec/27`'s own "what's deliberately not built yet" section explains why the latter was deferred
rather than built speculatively.

## Ephemeral, session-scoped peer identity

Inspired by the PKT Network whitepaper (concepts only, not its blockchain/token machinery) — a
peer's identity would reset every process restart, with trust attached to the *behavior of the
data a peer presents right now* rather than a persisted key (Sybil-resistant: a misbehaving peer
can't launder reputation by rotating keys, since a fresh identity also means zero accumulated
standing). Uptime/connectivity itself would be a trust signal. See `USER_INPUT_RECORD.md`'s
"networks of temporary work with unguessable encryption," and the "ant colony" metaphor below,
which independently reinforces the same disposable-identity/persistent-collective-behavior shape.

This would be a significant change to [01-identity.md](01-identity.md)'s current permanent-key
model and is not scoped further than this description — explicitly flagged by the project's own
originator as a concept to capture, not yet to plan in detail.

## Long-word, subdomain-based node addressing

Extends [21-petname.md](21-petname.md)'s existing deterministic word-based *display* naming into
actual *addressing* — using one domain's enormous subdomain space (tens of millions of
subdomains, each mapping to a `NodeId`/`PeerId`) to mint vast numbers of addressable identities on
top of DNS's already-deployed namespace, the same "use an existing substrate in a way its
designers didn't require" spirit as [15-reticulum-transport.md](15-reticulum-transport.md).

Open questions not yet resolved: who controls/pays for the parent domain and what happens if it's
lost; the exact `NodeId`/`PeerId`-to-subdomain-label mapping scheme; DNS is a *resolution*
mechanism, not a *discovery* one, so this would need to pair with something else (gossip, a
directory peer) for discovery.

## Native IPFS integration

**Phase 1 and 2 implemented** — see [29-ipfs-pinning.md](29-ipfs-pinning.md). Evidence content
(Phase 1) and `oag backup` snapshots (Phase 2) can optionally be pinned to IPFS (opt-in,
`[ipfs].pinning_provider`, default disabled). Evidence's CID lands in the existing
`Evidence.metadata` JSON column as `{"ipfs_cid": "..."}` — no schema change, since that column
already existed. `oag-crawler` auto-pins the main page's body at crawl time; `oag backup --pin`
pins the resulting snapshot; `oag ipfs cat <cid>` retrieves pinned content from either. OAG's own
`content_hash` stays authoritative; a CID is purely an additional retrieval pointer, never a trust
signal. Still not built: pinning the crawler's `llms.txt`/ARD/agent-card side-fetches, any REST/MCP
pin/fetch endpoint, automatic/scheduled backup pinning, and inlining large evidence bodies into
IPFS in place of local storage (no local full-body evidence storage exists to replace).

## Self-hosted roadmap governance

**Implemented** — see [28-roadmap-governance.md](28-roadmap-governance.md). Rather than a second,
separate blockchain for "what should be built next," reuses OAG's own assertion/evidence/
corroboration machinery pointed at itself: a roadmap proposal is `(proposal, instance_of,
concept:oag-roadmap-proposal)`, an ordinary assertion backed by ordinary evidence, ranked by the
same signals every other claim already gets (see
[06-corroboration-and-authority.md](06-corroboration-and-authority.md)) — no new consensus
mechanism, no new predicate, no new write path. The two genuinely new things the original
recommendation named — a predicate/concept-namespace convention and a Human Interface view for
browsing/ranking — are both built; a generic "list instances of any hub node" capability was
considered and deliberately not built (see `USER_INPUT_RECORD.md` Entry 8).

## First-class peer clustering

Groups of mutually cooperating peers — an "ant colony" of small, cooperating units doing
real-world physical work (sensors, traffic lights, smart objects) — as a concept distinct from
today's flat peer set. Separate from ephemeral identity above: clustering is a topology/trust-
radius question (how tightly a group of peers cooperates relative to the rest of the graph), not
an identity-lifetime one. Open questions: is a cluster a first-class graph concept or a pure
networking-layer notion; do intra-cluster vs. inter-cluster replication get different trust
defaults.

## Code as graph data (speculative aside, not a direction yet)

The idea that this project's storage service could eventually store its own source code as graph
data was raised as a speculative aside, not a direction to build. Noted as "less speculative than
it sounds" given the crawler/evidence model already handles any text artifact's content hash — but
explicitly not pursued, to avoid duplicating git.

## ANN index for semantic search

[07-search.md](07-search.md)'s brute-force cosine similarity is correct v1 scope; swapping in a
real approximate-nearest-neighbor index is a reasonable later step once brute force stops scaling,
not a problem today.

## Software architecture reference space

The graph itself holds `concept:software-architecture-reference`, a hub node under which
externally-sourced architectural *inspiration* (currently: the project creator's own
`hackspherelabs.com` collective site) is recorded via the `example_of` predicate — never followed
as a spec, only kept as durable, hash-verified, provenance-tracked evidence that the intake
happened. See `USER_INPUT_RECORD.md`, Entry 2, for the full reasoning and what was and wasn't
adopted from it.

## Gravity layer

Implemented — see [25-gravity.md](25-gravity.md). (`USER_INPUT_RECORD.md` Entry 3 first captured
this as an open question between two readings; Entry 5 resolved it and this section's earlier
"not yet built" draft of the pad-decay reading was superseded, not built — gravity throttles a
node's own processing speed, and never touches the tensor pad.)
