# RESEARCH_JOURNAL — Project OAG

Append-only record of research discoveries, technical hypotheses, references, and rejected
approaches. Newest entries at the bottom. Do not edit historical entries to flatter progress.

Scope note: this journal covers the whole mission. Repository-specific detail stays in that
repo's `OPEN_QUESTIONS.md` and `spec/`.

---

## 2026-11-09 — Onion routing: what the layered construction actually buys

**Hypothesis under test.** A hop-chain onion where each layer is ChaCha20-Poly1305 under an
X25519 key derived from a peer's Ed25519 identity gives per-hop unlinkability of the previous hop
from the next hop, at the cost of a fixed-size layered header.

**Prior art examined.** This is the classic "Sphinx-like" layered mix construction: nested
symmetric encryption with a per-hop key, where a relay can decrypt only its own layer and learns
only the next hop. The novelty in OAG's variant is *not* the cryptography — it is that the hop keys
are derived from identities that are already part of the signed event graph, so the routing layer
inherits peer identity rather than inventing a separate pseudonym system. The cryptographic
construction itself is standard and should be treated as rediscovered, not invented.

**Limitations of the standard construction that carry over here.**
- Fixed-size layers mean padding discipline is load-bearing; a size leak is a correlation vector.
- No reply support: the construction is one-way, so an anonymous responder has no way back without
  a separate return path (this is why `spec/14` uses a blinded *poll inbox* rather than a reverse
  onion route).
- Layer count is visible to the sender and, if the path is reused, becomes a correlation handle.

**Implemented.** `oag-mesh` `crates/oag-mesh/src/onion.rs`: layer encryption/decryption, X25519
key derivation from Ed25519, and rejection paths for empty path, key/length mismatch, bad hex,
wrong nonce length, and AEAD tag failure (`0a3a61f`).

**Open.** Whether the padding discipline is actually enforced end-to-end, and whether the hop
chain should be carried inside an anonymous event (the two mechanisms currently exist
independently and are not yet composed).

---

## 2026-11-09 — Rejected: deriving metrics by querying the event log

**Approach rejected.** Computing `verification_jobs` / `verification_failures` as a `COUNT(*)`
over the event log.

**Why it fails.** Rejected verification attempts never become events. A permission failure, an
unknown assertion id, or a schema mismatch produces no row, so the query returns a permanent zero
regardless of how much rejection traffic the peer sees. The number is structurally unavailable
from persisted state.

**What replaced it.** Process-lifetime `Arc<AtomicU64>` counters on `GraphService`, incremented in
`verify_assertion` *after* the outcome is known (so a permission failure counts), exposed on
`MetricsSnapshot` and rendered by `GET /metrics` (`0a84a78`).

**Generalizable lesson.** Before adding a metric, ask whether the quantity is recoverable from
persisted state. If it is not — because the failure modes are precisely the things that do not
persist — it must be instrumented in the code path, not derived in the metrics layer. This applies
to `crawls_failed`, `peer_sync_errors`, and any future "attempt was rejected" counter.

---

## 2026-11-08 — Rejected: HTTP gateway fetch for the IPFS site

**Approach rejected.** Serving `/ipfs-site/{cid}` by fetching `https://<gateway>/<cid>` and
proxying the response.

**Why it fails.** It reintroduces the exact trust dependency the feature was meant to remove: the
page's bytes are vouched for by a third-party gateway, and a compromised or malicious gateway can
serve different content under the same URL while the CID in the address bar stays constant. The
content-addressing guarantee is then cosmetic.

**What replaced it.** A vendored Helia bundle served by the peer itself, fetching over bitswap.
The CID in the URL is then genuinely the integrity check: the browser verifies the DAG against it.
The bundle is a static asset with no build step and no runtime network dependency other than the
local swarm. Live-verified with a real headless browser.

**Lesson.** A content-addressed URL is only meaningful if the fetcher verifies against the
address. Proxying through someone else's gateway silently converts a self-verifying scheme into a
trust-on-first-use scheme.

---

## 2026-11-08 — PoW is a cost floor, not an identity system

**Position settled across all three repos.** Proof-of-work appears in anonymous events, cluster
nesting, and membership attestation. In every case its job is to make bulk submission *cost
something* — bandwidth, CPU, or time — not to establish who the submitter is.

**Why this matters for design.** It means PoW must never be used as a gate for *reading* or for
*trusting*, because it carries no identity information. It also means the difficulty is a tuning
parameter that should be explicit and per-mechanism, and that the documentation must say plainly
that a well-resourced attacker is not stopped. Treating PoW as identity would produce a system
that is hard to use and still sybil-vulnerable — the worst of both.

**Consequence adopted.** PoW never excludes content from federation; it only shapes admission order
and cost. This is consistent with "degrade, don't drop."

---

## Open hypotheses not yet investigated

- **Adaptive routing from observed utility.** Whether mesh routing tables could strengthen or
  weaken links based on observed delivery success, without a central controller. No mechanism
  exists; `spec/13` (distributed route search) is a pull-based search, not an adaptive table.
  Feasibility unknown. Would need a convergence measurement before it is worth building.
- **Computation migrating toward data.** The imagination layer already moves *requests* toward
  pads; nothing moves *work* toward stored bytes. The distinction matters because the former is
  routing and the latter is scheduling.
- **Distributed memory consolidation.** Pads are per-node today; nothing consolidates a useful
  fact across peers into a shared structure. Corroboration is the closest existing mechanism and
  it does not store, it only scores.

---

## The tensor pad is a multi-armed bandit with no exploration term

**Date:** 2026-11-09. **Repository:** `oag-imagination-random-data-compute`. Recorded here because
the mission-wide records live in this repository.

**Observation.** `oag-imagination`'s tensor pad is a weight vector over a node's neighbours, and
`route` draws a neighbour with probability proportional to its weight. `reinforce` adds a constant
to the chosen neighbour after every hop. That is a multi-armed bandit: neighbours are arms, the
weights are the belief, and routing is the draw. The reward is always 1 for the arm taken, so the
weights grow without bound and the draw converges on whichever arm was reinforced first.

**What this explains.** The open-hypothesis entry above says "no mechanism exists" for adaptive
routing. That is not right -- the pad is one, and it is the only adaptive-routing mechanism in the
project. What it lacked was the other half of the bandit formulation: an exploration term. With no
exploration, an arm that is never drawn is never reinforced, so it is never drawn. The pad could
strengthen a path and let others lose share, but could not *form* one. `Engine::with_source` now
seeds a neighbour the pad does not cover with the mean weight -- optimistic initialisation, the
standard fix -- which makes "form" real. Measured over 256 fixed seeds: equal pads split about
evenly, a pad reinforced 100x carries nearly all the traffic.

**What remains.** The pad still cannot *weaken* or *dissolve*: `reinforce` only adds, so a path
that stops carrying traffic keeps its absolute weight forever and can never be retired. In bandit
terms this is a purely exploitative strategy with no forgetting -- it converges and then cannot
respond to topology change. A decay term (multiply the whole pad by a factor below 1 on each
routing decision, then reinforce) would bound the weights and make reinforcement take share *from*
the others, which is the weaken/dissolve half. It needs a rate, and the rate needs a measurement:
too fast and the pad cannot stabilise, too slow and it cannot adapt. **Status: proposed design, not
implemented.** The measurement that would justify it is a convergence test -- how many routing
decisions until the pad's chosen-neighbour distribution stops moving, with and without decay.

**Relationship to other components.** `oag-mesh`'s `link_quality` (`spec/13`) is the other
adaptive-routing candidate and it *does* forget -- quality is re-learned, and `spec/13` records
that it is dropped on restart. It is a liveness signal over peers, not a routing weight over
neighbours, so the two are complementary rather than competing: the pad decides *which neighbour*,
link quality decides *which peer to ask first*. Whether they should share a mechanism is open.
