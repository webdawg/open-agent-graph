# EXPERIMENTS — Project OAG

Catalog of experiments, hypotheses, procedures, results, and conclusions. Simulation results are
labelled as such and are never presented as production measurements.

Status legend: **RUN** (executed, results recorded) · **PLANNED** (designed, not run) ·
**BLOCKED** (cannot run safely or lacks a dependency).

---

## EXP-001 — Route search terminates on a cyclic peer graph

**Repository:** `oag-mesh`
**Hypothesis.** A depth-first route search over a mesh with a cycle (A↔B, B→C, C→B) terminates
rather than looping forever.
**Procedure.** Build a three-peer graph containing a cycle; run route search with a 10-second
timeout; assert it returns.
**Result (RUN, `23c8310`).** Terminates. No hang observed.
**Conclusion.** The visited set is correctly maintained across the cycle. This is a regression
guard, not a performance result — no timing claim is made.

---

## EXP-002 — Onion layer rejection paths

**Repository:** `oag-mesh`
**Hypothesis.** A malformed onion layer is rejected locally without panicking, and the failure mode
is distinguishable (bad hex vs. wrong nonce length vs. AEAD tag failure).
**Procedure.** Hand-craft inputs: empty path, key/path length mismatch, non-hex payload, truncated
nonce, corrupted ciphertext.
**Result (RUN, `0a3a61f`).** Each case returns the expected distinct error; no panic.
**Conclusion.** The construction fails closed. Does **not** establish padding discipline or
end-to-end unlinkability — those remain unverified.

---

## EXP-003 — Gateway-free IPFS site hosting verifies against the CID

**Repository:** `open-agent-graph` (`rust-ai`)
**Hypothesis.** A page served at `/ipfs-site/{cid}` is fetched by the browser over bitswap from
the local node, so the CID in the URL is a genuine integrity check rather than a gateway label.
**Procedure.** Pin a real HTML page, serve it through the peer's own route, load it in a real
headless browser, assert the rendered content matches the pinned bytes.
**Result (RUN, `4ac3a57`).** The browser fetched and rendered the page; content matched.
**Conclusion.** The content-addressing guarantee holds because the fetcher verifies against the
address. Contrast with the rejected gateway-proxy design (`RESEARCH_JOURNAL.md`, 2026-11-08).
**Scope.** One page, local swarm, debug build. Not a production throughput measurement.

---

## EXP-004 — Verification counters observe rejected attempts

**Repository:** `open-agent-graph` (`rust-ai`)
**Hypothesis.** `verification_jobs`/`verification_failures` can distinguish a successful
verification, an unknown assertion id, and a missing `graph:verify` permission.
**Procedure.** Start from a fresh `GraphService`; assert both counters are zero; perform one
successful verify, one verify against an unknown id, and one verify with a read-only key; read the
snapshot after each.
**Result (RUN, `0a84a78`).** Final snapshot: jobs = 3, failures = 2. Counters started at zero.
**Conclusion.** The instrumentation is sensitive to exactly the failure modes that never become
events, which is why a `COUNT(*)` cannot replace it.

---

## EXP-005 — An unreachable next hop degrades instead of hanging

**Repository:** `oag-imagination-random-data-compute`
**Hypothesis.** A multi-hop request whose next hop is unreachable still produces a response within
a bounded time, rather than hanging the caller.
**Procedure.** Point a chain at a closed port; assert the caller receives an error response and
the elapsed time is bounded.
**Result (RUN, `898c927`).** A response is returned; no hang.
**Conclusion.** Consistent with the standing "degrade, don't drop" decision. Latency numbers were
not recorded and are not claimed.

---

## EXP-006 — Three-node chain end to end

**Repository:** `oag-imagination-random-data-compute`
**Hypothesis.** A request entering node A reaches node C through B, and the response carries the
whole chain back to A.
**Procedure.** Build a three-node chain; send a request; assert C contributed and A received a
response reflecting the full path.
**Result (RUN, `ca94a0d`).** Passed.
**Conclusion.** Multi-hop routing works across three nodes. Does not establish behaviour under
concurrent load.

---

## EXP-007 — Device randomness source reads exactly the device bytes

**Repository:** `oag-imagination-random-data-compute`
**Hypothesis.** A byte-device source returns exactly the bytes the device provides, with no
padding or truncation.
**Procedure.** Feed a synthetic device stream of known length; assert the output equals the input
byte-for-byte.
**Result (RUN, `0559614`).** Byte-exact.
**Conclusion.** The hardware seam is faithful. Does not establish entropy *quality* — a synthetic
stream was used, so no randomness-strength claim is made.

---

## EXP-011 — Does the commit marker actually catch a torn store set?

**Repository:** `oag-mesh`
**Hypothesis.** A set of stores saved with the marker, then damaged in any of the ways a crash
mid-save can damage it, is refused by `load_from_dir` rather than silently loaded.
**Procedure.** Save a real seven-store set; then independently (a) delete one store file, (b)
overwrite one with different bytes, (c) truncate the marker to invalid JSON, (d) write a marker
with an unsupported version. Assert `load_from_dir` refuses each.
**Result (RUN, `7223852`).** All four are refused with `TornSet` (the corrupt marker surfaces as
`Parse`). A directory with no marker still loads, and a fresh save verifies as `Consistent`.
**Conclusion.** The read-side gap is closed for every damage shape tested.
**Scope, stated plainly.** The torn states were *constructed*, not induced by killing a process
mid-save. A real crash could also lose a rename entirely (no fsync), which this mechanism does not
address and which the experiment does not test.

---

## EXP-008 (PLANNED) — Does traffic strengthen pads measurably?

**Hypothesis.** After N routed requests, a pad that carried traffic is selected more often than an
otherwise-identical pad that did not.
**Procedure.** Two identical pads, identical initial weights; route a bounded number of requests
through one; compare selection rates with a control run.
**Status.** PLANNED. Not run. This is the measurement that would justify OAG-INV-005 as an
EXPERIMENTALLY VERIFIED CAPABILITY rather than an IMPLEMENTED MECHANISM.

---

## EXP-009 (PLANNED) — Does adaptive routing improve delivery?

**Hypothesis.** A routing table reinforced by observed delivery success achieves a higher delivery
rate than a static table on the same lossy topology.
**Procedure.** Simulate a bounded mesh with random link failure; compare delivery rate and hop
count for adaptive vs. static routing over the same seed set.
**Status.** PLANNED. Requires a topology simulator that does not yet exist. No mechanism is
implemented, so there is nothing to measure yet.

---

## EXP-010 (BLOCKED) — Sub-second ephemeral micro-networks

**Hypothesis.** A network can form, transact, and dissolve in under 30ms, and formation itself can
serve as an identity signal.
**Status.** BLOCKED on design. No spec, no protocol, no measurement harness. Recorded in
`rust-ai/spec/22-future-work.md`. Not a resource block — a design block.
