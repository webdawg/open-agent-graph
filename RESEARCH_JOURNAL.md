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

---

## Correction: the tensor pad *can* weaken and dissolve -- relatively

**Date:** 2026-11-09. **Repository:** `oag-imagination-random-data-compute`.

**The previous entry over-claimed.** It said the pad "cannot weaken or dissolve" because
`reinforce` only adds. That is true of the pad's *absolute* values and false of its behaviour.
`route` computes `point = hash(seed) / u32::MAX * total` and walks the cumulative weights, so the
chosen neighbour depends only on weight **ratios**. A path that stops carrying traffic therefore
loses share as others are reinforced and can be out-competed by a newly seeded neighbour. Absolute
weights never fall, but nothing reads them.

**What the absolute scale actually cost.** The pad is published as the node's representation and
`combine_pads` adds pads element-wise, so the scale was not free after all: a node that had forwarded
a million requests dominated one that had forwarded ten, and two nodes' pads were not comparable.
That is a real defect, just not the one the earlier entry named.

**Consequence adopted.** Pads are normalized to a mean of 1.0 at construction and after every
reinforcement. Routing is unchanged; the representation becomes comparable and `combine_pads`
combines two preferences rather than two traffic counts. Verified by
`two_nodes_with_the_same_preference_produce_the_same_pad`.

**What remains, restated precisely.** The pad cannot *forget*: a path reinforced once keeps its
weight forever, so a topology change that makes it useless dilutes it but does not remove it. That
is the honest form of the earlier claim, and it is what a decay term would address. Still a
proposed design, still needing the convergence measurement to justify a rate.

**Method note.** Worth recording because it is the second time this session that a plausible
reading of a mechanism was wrong in the direction of making the system look more broken than it
is. Share-based comparisons and count-based comparisons are different things; the first question to
ask of any weight is what actually reads it.

---

## The tensor pad converges to an arbitrary neighbour: measured

**Date:** 2026-11-09. **Repository:** `oag-imagination-random-data-compute`.
**Artifact:** `crates/oag-imagination/examples/convergence.rs`.

**The claim under test.** `spec/02` says "more traffic through a path strengthens the pad along that path, so repeated requests for the same thing tend to reuse it". That is a
convergence claim, and it had only ever been asserted -- first as prose, then as a static
distribution over 256 fixed seeds. Neither says anything about what the pad converges *to*.

**Method.** Run the real engine over a deterministic source (`BLAKE3(counter)`), with a resolver
that answers nothing so every request is routed and the pad is exercised on its own. 20,000
decisions, 4 neighbours, share recorded every 1,000.

**Result.** The pad converges to one neighbour at about 96% of all routing:

| decisions | share of the eventual leader |
|---|---|
| 1,000 | 0.568 |
| 3,000 | 0.777 |
| 5,000 | 0.866 |
| 10,000 | 0.933 |
| 20,000 | 0.966 |

The pad mean stayed at exactly 1.0000 throughout, which independently confirms the normalization
added earlier is stable under the reinforce-then-renormalize loop.

**The control that decides it.** `--offset` shifts the source's starting point. Offsets 0-3 crown
`node-2`; offsets 4-5 crown `node-3`, at the same ~96%. The winner is therefore decided by the early
random draws, not by any property of the neighbours. Repeated runs at the same offset reproduce
exactly.

**What this means.** `reinforce` adds a constant `1.0` to whichever hop was taken, regardless of
whether that hop answered, forwarded, or failed. The reinforcement carries **no information about
the hop's quality**, so the pad is not learning which neighbour is better -- it is a
rich-get-richer random walk that locks onto whichever neighbour the first few hundred decisions
happened to favour. The earlier entry in this journal called it "a bandit with no exploration
term"; that was too generous. A bandit's reward varies with the arm's outcome; here it does not
vary at all.

**Consequence for the decay question.** The previous entry proposed decay as the missing
weaken/dissolve half. This measurement shows decay would add *forgetting* but still no *signal*:
a pad that forgets an arbitrary lock-in only drifts to another arbitrary lock-in. **The missing
piece is a utility signal -- feedback from a hop's outcome into the pad -- not a decay rate.** That
reframes the open design question, and it is now the honest one.

**Is a signal available?** Yes, in the carrier. The TCP serving loop forwards a request and then
reads the upstream response, so the node that routed a hop already observes whether it answered.
`Engine::handle` returns `RouteTo(next)` and discards that observation. Plumbing it back in means
the engine learning a reward rather than a constant -- a new input to `handle`, and a design change
rather than a parameter. **Status: proposed design, not implemented.**

---

## The pad now learns from observed utility: design and A/B result

**Date:** 2026-11-09. **Repository:** `oag-imagination-random-data-compute`.

**The problem, restated from the previous entry.** `reinforce` added a constant `1.0` to whichever
hop was taken. The reinforcement carried no information about whether the hop was any good, so the
pad was a rich-get-richer random walk that locked onto an arbitrary neighbour -- measured, with the
winner changing under a shifted source offset while the ~96% share did not.

**Design.** The signal exists and the carrier already has it: the TCP serving loop forwards a request
and then reads the upstream reply, so the node that routed a hop sees whether the chain was answered.
The change is that the engine no longer learns at routing time:

- `handle` routes and changes nothing. Whether the hop was any good is unknowable at that moment.
- The carrier calls `Engine::observe_hop(neighbour, outcome)` once it knows.
- `HopOutcome::{Answered, Forwarded, Failed}` multiply the neighbour's weight by `1.05`, leave it
  alone, or multiply by `0.90`, followed by the existing normalization.

Two deliberate details. Updates are **multiplicative**, not additive: an additive penalty could
drive a slot negative, and `normalize` divides by the sum, so a negative total would *invert* the
ordering instead of merely shrinking it. And `Forwarded` is **neutral** -- the hop passed the request
on and nothing is known against it, so it must not be punished for not being the answerer.

**A/B result.** `examples/convergence.rs --no-signal` runs both arms over the same deterministic
source, with node-0 answering and the other three unreachable:

| arm | leader | share |
|---|---|---|
| signal | `node-0` -- the one that answers | 99.8% |
| control (old constant reinforcement) | `node-2` -- arbitrary | 59.6% |

Both arms share their first few hundred draws, so the divergence is not luck: one arm has a utility
difference to learn and the other does not. The signal arm reaches 0.979 by 2,000 decisions.

**What this does and does not establish.** It establishes that the pad can now follow observed
utility, which it could not before. It does not establish that the gain and penalty constants are
well chosen -- `1.05` and `0.90` are round numbers, and a sweep of them is the obvious next
measurement. It also does not make the pad a good *bandit*: the reward is binary and delayed by the
whole downstream chain, so credit assignment across a multi-hop path is coarse. Stated plainly: this
is a utility-driven lock-in, not proven optimal routing.

**Relationship to the earlier entries.** This supersedes the "proposed design, not implemented"
note in the convergence entry. The decay proposal from two entries earlier is now clearly not the
point: the previous entry showed decay would add forgetting without signal, and this entry supplies
the signal instead.

**Novelty assessment.** Utility-driven link weighting is well-trodden -- ant-colony stigmergy,
multiplicative-weights/Hedge, and reinforcement of routing tables all do this. What is specific here
is the *carrier* as the observation point: the node that routes a hop is not the node that answers,
so the outcome has to be carried back one step by the forwarding layer. The contribution is that
plumbing, not the weighting rule.
