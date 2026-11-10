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

## EXP-012 — Does the imagination carrier actually carry a request over a real link?

**Repository:** `oag-imagination-random-data-compute`
**Hypothesis.** A request sent by `ReticulumTransport` reaches a real `serve` instance over a real
Reticulum `Link`, is answered by the engine's resolver, and the reply carries the server node's
contribution.
**Procedure.** Start `serve` on a loopback `TcpServer` interface with an engine whose resolver holds
one known target. Connect a `ReticulumTransport` over a `TcpClient` to the same port, dial the
server's address, send one request, read the reply.
**Result (RUN, `9f63af7`).** The reply came back with the resolver's answer and one contribution
attributed to the server node.
**Also run:** a send to an address nobody announces returns `None` at its own timeout rather than
hanging; a malformed address fails softly.
**Conclusion.** The carrier works end to end over a real socket. The test uses loopback TCP as the
Reticulum interface, not radio -- it proves the link, framing, and engine wiring, not radio
behaviour.
**Scope.** Three tests, ~10s total. The failure case's timeout was shortened to 2s via
`with_announce_wait` so the suite stays quick.

---

## EXP-013 — Does observed deliverability improve fan-out?

**Repository:** `oag-mesh`
**Hypothesis.** Recording whether each route-search interaction delivered, and asking peers
best-first, lets a constrained search succeed where it previously failed.
**Procedure.** A node configured with `[dead, live]` (a closed port and a real mesh node that
knows the target), a fan-out budget of one, and no history. Run the search; then run it again with
the same budget after the first search's outcome has been recorded.
**Result (RUN, `67ad91e`).** First search: `None` -- its single slot went to the dead peer.
`link_quality` recorded one attempt, zero successes, for the dead peer. Second search: a path was
found, because the live peer (still at the neutral `0.5`) now outranks the dead one (~`0.45`).
**Conclusion.** The adaptation is observable in behaviour after a single observation, not merely
present in the code.
**Scope, stated plainly.** This measures the *ordering* effect with a budget of one on a
two-peer topology. It does not measure end-to-end delivery rate, convergence over many rounds, or
behaviour over radio. Those remain open.

---

## EXP-014 — Is a black-holed peer actually bounded?

**Repository:** `oag-mesh`
**Hypothesis.** A peer that accepts the TCP connection and then never sends a byte cannot hold an
outbound request open past the configured timeout.
**Procedure.** Bind a listener that accepts connections and never responds. Point a client built
with a 50ms connect timeout and a 200ms request timeout at it, and separately drive
`pull_and_merge` through a `MeshState` using that client.
**Result (RUN, `531b6f9`).** The raw request errored in well under a second. `pull_and_merge`
returned `false` in ~1.3s and recorded one attempt, zero successes, for the peer.
**Conclusion.** The bound holds end to end, and a timed-out request is correctly visible to the
link-quality ledger as a failed delivery.
**Scope.** Loopback only. The production values (5s/30s) are named constants and were not waited
out; the test proves the mechanism, not the production numbers.

---

## EXP-015 — Is the sync client actually bounded?

**Repository:** `open-agent-graph` (`rust-ai`)
**Hypothesis.** A peer that accepts the TCP connection and then never sends a byte cannot hold a
sync request open past the configured timeout.
**Procedure.** Bind a listener that accepts connections and never responds; point a client built
with a 50ms connect timeout and a 200ms request timeout at it. Separately, point the default
`SyncClient::new()` at a reachable fixture peer.
**Result (RUN, `b2e2182`).** The raw request errored in well under a second. The default client
completed a real `/peers` fetch inside a 5s wrapper.
**Conclusion.** The bound holds, and the production constructor is the bounded one — not only the
test-only path.
**Scope.** Loopback only. The production values (10s/120s) were not waited out; the test proves the
mechanism, not the production numbers.

---

## EXP-016 — Does ordering the pull loop get live data out sooner?

**Repository:** `oag-mesh`
**Hypothesis.** Because `sync_with_all_peers` is sequential, ordering its peers by observed
deliverability means a live peer's data lands before a black-holed peer's timeout is paid.
**Procedure.** A control experiment with two arms and one difference. Same hanging peer (accepts
the TCP connection, never responds), same live peer holding one declaration, same client with a 5s
request timeout. Arm one has no ledger history, so configuration order applies and the hanging peer
is pulled first; arm two has the ledger primed with a failure for the hanging peer and a success for
the live one. In each arm, poll the pulling node's tree for the live declaration while the pull loop
is still running.
**Result (RUN, `740576b`).** Without history, the live data did **not** arrive within the 1s
observation window while the hanging peer was blocking. With history, it arrived within that window.
**Conclusion.** The ordering changes observable behaviour, and the control arm shows it is the
ordering -- not the data or the timings -- that makes the difference.
**Scope.** The ledger was primed directly rather than by a real preceding tick; how records arise is
covered by the ledger's unit tests and the route-search tests. Loopback only. The total tick time is
unchanged -- ordering changes *when* the useful data arrives, not how long the tick takes.

---

## EXP-017 — Are the latency histograms actually populated by real traffic?

**Repository:** `open-agent-graph` (`rust-ai`)
**Hypothesis.** Serving a real REST request increments `api_latency`, and serving a real search
increments `search_latency`, without either being derived from a log line or a snapshot query.
**Procedure.** `api_latency_counts_rest_requests_end_to_end`: scrape `/metrics` on a fresh peer
(count must be 0), serve one authenticated `/api/v1/status` request through the full middleware
stack, scrape again. `search_latency_records_every_search_this_process_serves`: serve one matching
search, one repeat, and one search with no matches.
**Result (RUN, `fafc44a`).** `api_latency` went from 0 to a positive count after one request.
`search_latency` counted each of the three searches, including the one with no matches.
**Conclusion.** Both histograms are wired to real traffic, not merely constructible.
**Scope.** Counts and wiring only. No percentile accuracy claim is made beyond the bucket boundary
the value fell in, and no comparison against an external load generator was run.

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
