# INVENTION_REGISTER — Project OAG

Catalog of original architectural ideas and their experimental status. Every entry states its
category explicitly. Categories:

- **THEORETICAL POSSIBILITY** — reasoned about, not built.
- **PROPOSED DESIGN** — designed, not implemented.
- **IMPLEMENTED MECHANISM** — code exists and runs.
- **EXPERIMENTALLY VERIFIED CAPABILITY** — measured, with the experiment recorded in `EXPERIMENTS.md`.
- **PRODUCTION-READY CAPABILITY** — deployed and relied upon.

Do not present one category as another. "Implemented" means code compiles and has tests; it does
not mean proven useful at scale.

---

## OAG-INV-001 — Gravity as a processing throttle

**Description.** A per-node decay applied at *processing* time to an artifact's weight, rather
than a mutation of stored data or a deletion.

**Theoretical foundation.** Reservoir/attention analogues: old information should become more
expensive to attend to, not disappear. Mutating stored state would break the signed-log invariant.

**Relationship to existing components.** Sits on top of the authority ranking in
`rust-ai/spec/06` and the corroboration scoring. Distinct from tensor-pad evolution
(`spec/23`), which changes what is *selected*, not how heavy a given artifact is.

**Novelty assessment.** Low-to-moderate. Decay-at-read is a known idea; the specific placement
(a throttle on processing cost, never on storage) is the original part.

**Feasibility.** High. **Implementation requirements.** A deterministic function of
`(now, observed_at, half_life)` applied in ranking queries.

**Status.** IMPLEMENTED MECHANISM — `rust-ai/spec/25-gravity.md`.

---

## OAG-INV-002 — Long-word addressing over DNS

**Description.** Encoding a peer's network location into a DNS name built from a wordlist, so an
address is human-speakable and human-checkable without a directory service.

**Theoretical foundation.** Address readability as an anti-phishing property: a name a human can
read and compare is harder to spoof convincingly than a hex string. Similar in spirit to
petname systems (`rust-ai/spec/21`).

**Relationship to existing components.** Provides the human-facing half of peer identity; the
cryptographic half is the Ed25519 key. Names are not identities — they are memorable handles over
a verified key.

**Novelty assessment.** Moderate. Mnemonic/wordlist encodings exist; using one as a *routing*
address that also carries a network location is the original part.

**Feasibility.** High. **Status.** IMPLEMENTED MECHANISM — `rust-ai/spec/30-long-word-addressing.md`.

---

## OAG-INV-003 — Gateway-free IPFS site hosting

**Description.** Serving content-addressed pages from the peer itself, with the browser fetching
over bitswap via a vendored bundle, so a CID in the URL is a real integrity check rather than a
label a third-party gateway resolves.

**Theoretical foundation.** Content addressing is only meaningful if the fetcher verifies against
the address. See `RESEARCH_JOURNAL.md` (2026-11-08, "Rejected: HTTP gateway fetch").

**Relationship to existing components.** Built on `spec/29` (IPFS pinning) and the crawler's
page-pin path. Serves evidence pages that are already pinned, so hosting adds no new trust surface.

**Novelty assessment.** Low as a technique; the original part is making it *the default* rather
than an opt-in gateway proxy.

**Status.** IMPLEMENTED MECHANISM, and EXPERIMENTALLY VERIFIED CAPABILITY (real headless browser
fetch verified) — `rust-ai/spec/29`, `EXPERIMENTS.md` EXP-003.

---

## OAG-INV-004 — Blinded responder dispatch with a poll inbox

**Description.** A requester subscribes to a responder under an id chosen by the responder, so the
requester cannot be linked to the subscription by a third party observing the subscription
traffic; the responder polls an inbox rather than being pushed to.

**Theoretical foundation.** Unlinkability via a responder-held capability. Push delivery would
require the responder to learn the requester's address; polling inverts the dependency.

**Relationship to existing components.** Composes with `spec/15` onion routing (which hides the
previous hop) and `spec/12` responder acknowledgment.

**Novelty assessment.** Moderate. Private-information-retrieval and anonymous mailbox constructions
are known; the poll-inversion as a *response* path is the original part.

**Known limitation.** Poll-only means latency is bounded by the poll interval, and a responder
that stops polling is silently unreachable. No liveness signal exists.

**Status.** IMPLEMENTED MECHANISM — `oag-mesh/spec/14-responder-dispatch.md`.

---

## OAG-INV-005 — Traffic strengthens the tensor pad

**Description.** A pad's routing preference is reinforced by observed traffic rather than by an
externally assigned weight, so frequently-useful pads become preferred paths.

**Theoretical foundation.** Hebbian-style reinforcement: pathways that carry traffic get
stronger, which is the mechanism by which biological networks prune and reinforce.

**Relationship to existing components.** `rust-ai/spec/23` (tensor pad and evolution layer) and the
imagination layer's pad-guided routing engine. This is the mechanism that makes the pad *adaptive*
rather than a static routing table.

**Novelty assessment.** Moderate. Adaptive routing from observed utility is well studied; applying
it to a tensor pad that also serves as a data store is the original part.

**Status.** IMPLEMENTED MECHANISM — `rust-ai/spec/23`; the imagination engine routes by pad
(`4bdef2d`).

---

## OAG-INV-006 — Cluster-scoped PoW escalation

**Description.** Proof-of-work difficulty escalates with the nesting depth of a cluster, so a
deeply nested sybil subtree costs more to populate than a shallow one.

**Theoretical foundation.** Cost asymmetry as an anti-sybil mechanism. Nesting is otherwise a
free multiplier on influence, so the cost must scale with it.

**Relationship to existing components.** `oag-mesh/spec/02` (cluster nesting). Applies the same
principle to membership attestation (`spec/09`) and anonymous events (`spec/03`).

**Novelty assessment.** Moderate. The escalation-by-depth rule is the original part; PoW itself is
not.

**Known limitation.** Bounds cost only. See `RESEARCH_JOURNAL.md` (2026-11-08, "PoW is a cost
floor, not an identity system").

**Status.** IMPLEMENTED MECHANISM — `oag-mesh/spec/02`.

---

## OAG-INV-007 — Witness-based audit of peer behaviour

**Description.** Independent peers observe and attest to another peer's serving behaviour, so
misbehaviour by a relay becomes independently checkable rather than merely reported.

**Theoretical foundation.** Redundant multi-path delivery plus content-hash self-verification.
A witness is only useful if it produces evidence that is verifiable by a third party without
trusting the witness.

**Status.** THEORETICAL POSSIBILITY / PROPOSED DESIGN. The *design question* (does redundant
delivery make misbehaviour observable?) is resolved. The serving-announcement evidence shape,
hop-chain shape, and witness payload variant are **undesigned**. Not started. This is the largest
unbuilt item in the mesh layer.

---

## OAG-INV-008 — Flood propagation with traffic-analysis resistance

**Description.** Event propagation that reaches the whole mesh rather than a configured relay set,
without revealing who is talking to whom.

**Status.** THEORETICAL POSSIBILITY. Unbuilt and **undesigned**. Known tension: pull-based
replication is bandwidth-cheap and reveals little; flooding is bandwidth-expensive and reveals
topology. "Never-ending morphing encryption" is open. Not started.

---

## OAG-INV-009 — Sub-second ephemeral micro-networks

**Description.** Networks that form, transact, and dissolve in under 30ms, and the claim that
*network formation itself* can serve as an identity signal.

**Status.** THEORETICAL POSSIBILITY. Genuinely open; needs new spec work before any code. Listed
in `rust-ai/spec/22-future-work.md`.

---

## OAG-INV-011 — Link-quality-ordered fan-out

**Description.** A per-peer exponential moving average of whether each interaction actually
delivered, used to order which peers a route search asks first.

**Theoretical foundation.** The mission's networking domain asks whether communication pathways
can strengthen or weaken based on observed utility. This is the smallest honest instance of that:
not a routing table, not next-hop selection, just "spend the scarce fan-out budget on the peers
that have been answering".

**Relationship to existing components.** Composes with `spec/13`'s route search and
`spec/10`'s `pull_and_merge`, both of which now report whether the interaction delivered. Uses the
same learning rate as the base project's tensor pad (`spec/23`), applied to connectivity rather
than to routing weights.

**Novelty assessment.** Low-to-moderate. Exponential-moving-average health scoring and
least-recently-used/least-successful-first ordering are both well known. The original part is the
placement: a liveness signal kept deliberately separate from the trust system, used only to order
a bounded fan-out, and guaranteed to be a permutation so it cannot become a censor.

**Feasibility.** High. **Implementation requirements.** A ledger, two instrumentation points, and
an ordering call.

**Experimental validation criteria.** Whether ordering actually improves delivery. Met on one axis:
with a dead peer and a fan-out budget of one, the first search fails and the second succeeds after
a single observation.

**Status.** IMPLEMENTED MECHANISM and EXPERIMENTALLY VERIFIED CAPABILITY on the fan-out-ordering
axis (`67ad91e`, `EXPERIMENTS.md` EXP-013). Not verified on radio, and not verified to improve
end-to-end delivery rate -- only to avoid wasting a budget slot.

---

## OAG-INV-010 — Deterministic replay of generated scenarios

**Description.** A generated scenario is fully reproducible from its seed, so a "dream" can be
re-run, compared, and audited without trusting the node that produced it.

**Theoretical foundation.** Determinism as an audit property. A synthetic scenario that cannot be
reproduced cannot be compared against another node's run.

**Relationship to existing components.** The imagination layer's randomness sources are the seam;
the requirement is that a scenario carries its seed and that the computation over synthetic data
is a pure function of `(seed, data, program)`.

**Novelty assessment.** Low as a technique; the original part is treating replay as a
*requirement* of the synthetic layer rather than a debugging convenience.

**Status.** IMPLEMENTED MECHANISM (seeded generation and deterministic compute exist);
EXPERIMENTAL VERIFICATION pending.
