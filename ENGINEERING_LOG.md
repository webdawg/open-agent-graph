# ENGINEERING_LOG — Project OAG

Append-only engineering journal. Newest entries at the top. Each entry records date,
invocation, repository, files modified, features implemented, tests executed, test results,
performance measurements, commit identifiers, push status, and remaining defects.

---

## 2026-11-09 — cap the imagination engine's route hops

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`.

**Gap closed.** The engine has no visited-set — the pad picks the next hop by weight, so a cycle
between two nodes is a possible routing outcome, not an impossible one. Without a hop budget such a
request would accumulate one contribution per hop and be re-sent in full each time, amplifying
bandwidth and memory until the carrier's `MAX_MESSAGE_BYTES` happened to reject it.

**Fix.** `MAX_ROUTE_HOPS` (64), well above `spec/02`'s own "6, 10, or 50 nodes" intent and matching
the spirit of the base project's `relay_hops_remaining`. A node that has exhausted the budget
returns `TooManyHops` instead of forwarding. It still records its own contribution: the bound is on
forwarding, not on acknowledging participation.

**The budget bounds forwarding, not answering.** A node that can answer does so however long the
chain behind the request is — refusing an answer would turn a bounded-chain problem into a failed
request.

**Tests executed.** `cargo test --workspace` — 45 passed (oag-imagination) + 3 passed (adapter).

**Test results.** oag-imagination 43 → 45. New: `a_request_at_the_hop_budget_is_not_forwarded`
(two below the budget still routes; one below reaches it and is refused) and
`the_hop_budget_does_not_stop_a_node_that_can_answer`.

**Performance measurements.** None. The bound costs one length comparison per handled request.

**Commit:** `548580d` — pushed.
**Remaining defects.** The engine still has no visited-set, so a request can revisit the same node
many times within the budget — the budget bounds the damage but does not detect the cycle. A
visited-set would cost memory proportional to the chain, which is the same trade the dream log
already makes.

---

## 2026-11-09 — bound the imagination dream log

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`.

**Defect fixed.** `DreamLog` was an unbounded `Vec<Request>`, and `Engine::handle` records every
answered request as a clone. Any caller who can make this node answer a request -- any peer that can
route one here -- could grow its memory without limit, amplified per request by the caller-controlled
JSON in `target`/`payload` and the contribution chain gathered along the route.

**Fix.** Both a count cap (1,000) and a serialized-byte cap (8 MiB), evicting the oldest until both
hold. A count cap alone would not have been enough: one request may be up to the carrier's
`MAX_MESSAGE_BYTES` (16 MiB over TCP), so "keep the last 1,000" still permits tens of gigabytes. An
entry larger than the whole byte budget is not retained at all -- otherwise a single request would
defeat the bound.

**Tests executed.** `cargo test --workspace` — 43 passed (oag-imagination) + 3 passed (adapter).

**Test results.** oag-imagination 40 → 43. New: `the_dream_log_evicts_the_oldest_entry_past_its_
count_cap`, `the_dream_log_evicts_past_its_byte_cap`, `a_single_entry_larger_than_the_budget_is_not_
retained`. The log previously had no tests at all, which is part of why this went unnoticed.

**Performance measurements.** None. The bound adds one serialization per answered request (cached
at record time) and a `pop_front` on eviction. No benchmark was run.

**Commit:** `9d77e3a` — pushed.
**Remaining defects.** The log is still in-memory only: "recording generated scenarios" holds for the
running process, not across restarts, and that is a scope statement rather than an oversight.
`MAX_MESSAGE_BYTES` itself is 16 MiB over TCP, which is generous for an imagination request; it is
bounded but not small, and a per-request cap would be the next tightening if memory mattered more.

---

## 2026-11-09 — new layer: oag-collisions (inference by collision)

**Repository:** `oag-collisions` (new; the operator created it as a fourth layer on top of the
three foundations, explicitly overriding the earlier workspace confinement)
**Files created:** `Cargo.toml`, `crates/oag-collisions/{Cargo.toml,src/lib.rs}`,
`crates/oag-collisions/src/aberration.rs`, `crates/oag-collisions/examples/aberration.rs`,
`SPEC.md`, `EXPERIMENTS.md`, `LICENSE`.

**What it is.** A collision is an inference result, not an error: two independently produced values
that land in the same slot are not a coincidence to discard, the collision *is* the answer, and its
strength is how much of the slot they share. Implemented as locality-sensitive hashing (SimHash over
BLAKE3 token hashes) with every prefix indexed, so a query collides partially with everything and the
longest shared prefix ranks the result. No vector database, no model, no training.

**The experiment (PART 0).** The operator's statement "the sun is travelling at light speed" was
taken literally and tested. Speed alone gives a constant aberration -- an offset indistinguishable
from the coordinate origin -- but an *accelerated* observer sees the sky slide, and a circular orbit
is continuous acceleration, at `a / c` radians per second for `a = v^2 / r`. That quantity is the
published secular aberration drift (VLBI/Gaia, ~5 microarcsec/year).

**Result.** The control passes: 230 km/s reproduces 4.539 microarcsec/year against ~5 published.
The claim does not survive: at light speed the drift would be **7.7 arcseconds per year**, 1.7
million times the measurement and 0.17 degrees across an 80-year lifetime -- visible to the naked
eye, never recorded. The discrepancy is a factor of ~1300 in velocity.

**Second control (EXP-002).** Earth's orbital speed is the AU over the Julian year -- both defined
quantities, known to four digits independently of any star catalogue -- and it produces the
classical aberration constant every star catalogue is corrected by: computed 20.48939 arcsec against
the IAU's 20.49552 (the 0.006 gap is exactly the 29.780 vs 29.7859 km/s difference). Earth's drift
is 128.716 arcsec/year, 2.836e7 times the galactic one. One un-tuned formula landing on two
independently known quantities, seven and a half orders of magnitude apart, is what makes the first
comparison a control rather than a fitted curve.

**Tests executed.** `cargo test -p oag-collisions` — 14 passed, 0 failed.

**Commits:** `cc7613a`, `04572c7`, `5e83384`, `e0bea3d`, `693ab0b`, `3bb7127`, `1dde7b6`,
`1a80111`, `2712e73`, `550525d`, `4fcd431` — pushed.

**Follow-on work (same layer).**
- `2712e73` — third measurement, the CMB dipole: 369.8 km/s against a published 369.8 ± 0.9, a
  completely independent method from the other two.
- `550525d` — the collision index was bounded. It stored every item under all 64 prefixes of its
  hash with no cap, so memory was O(items × 64) and grew without limit. Now
  `with_capacity_and_levels(n, levels)` with oldest-first eviction, plus a `bucket_entries()`
  accessor because the item count is the wrong quantity to reason about.
- `4fcd431`, `89044f9` — the prefix-level trade-off was measured rather than assumed
  (`examples/ranking_tradeoff.rs`). Two corrections to the first reading:
  - *Ties.* At 16 and 8 levels the correct variant is still in the top set but tied, so the first
    answer is arbitrary. The index has not lost the right answer; it has lost the ability to choose
    it. 32 is the fewest levels that gets top-1 right without a tie.
  - *Length.* The crossover does not survive moving the document length. At 200 tokens the correct
    variant wins on 51 shared bits; at 60 the best hit is an unrelated variant on 11 bits (chance);
    at 20 and 8 nothing crosses the 8-bit threshold at all. For short documents **no** level count
    works, because unigram SimHash is a majority vote over bit positions and with few tokens each
    bit's vote is decided by noise.
  The default stays at 64: one synthetic corpus, and changing a default on one result is the
  unmeasured trade the experiment exists to prevent.
- `362ca03` — the recorded follow-up was tested and **falsified**: hashing word bigrams rather than
  unigrams does not fix the short-document failure and *degrades* the long-document case (at 200
  tokens the unigram index returns the correct variant at 51 shared bits; the bigram index returns
  one result, an unrelated variant at 8 bits, with the correct answer absent). The `shingle` knob
  stays but defaults to 1, with the negative result recorded beside it — nobody should "fix"
  short-document performance with bigrams on the strength of an intuition.
- `3d70ffd` — character n-grams were falsified too (they also degrade the working case), but the
  diagnostic reframed the problem. Character trigrams give 887 units per document, and a query plus
  a variant sharing 885 of them produced hashes differing in **zero** bits. Measuring SimHash's
  sensitivity directly — fix a 2-token change, vary the unit count — gives 17 bits moved at 10
  units, 4 at 50, 0 at 200, 1 at 800. Sensitivity is **inversely** proportional to the unit count,
  so short documents are *over*-sensitive, contradicting the mechanism claimed in EXP-004. That
  write-up's measurements were right and its explanation was wrong; `EXPERIMENTS.md` now says so.
  Consequence: the unit count is a sensitivity dial as well as a memory dial, coupled in the
  opposite direction from intuition — indexing more text can silently freeze the index. Why 60
  tokens fails is recorded as an open question, not explained.
- `29458ce` — the sensitivity window mapped (`examples/sensitivity.rs`): units against edit
  distance, bits moved, averaged over 8 seeds. The ~19.5-bit plateau is the unrelated-document
  distance. Detection degrades gracefully, but the **ranking** gap between e=2 and e=4 collapses
  from 5.1 bits at 10 units to **0.1 bits at 800** — so at 800 units two near-duplicates at
  different distances cannot be ordered at all. The window is a ranking window, not a detection
  window. The surface also falsifies the sensitivity explanation for the 60-token failure: 60
  tokens sits comfortably inside the resolvable band, so that question stays open — narrowed.
- `6f472bf` — unigram SimHash is **exactly invariant to word order** (the EXP-007 gap). A 200-token
  document and its full reversal collide at **64 shared bits** through the index; an adjacent swap is
  likewise invisible. Structural, not tunable. Bigrams detect order (24 bits) but EXP-005 measured
  them degrading substitution, so the layer faces a measured either/or: unigrams are blind to
  order, bigrams cost substitution sensitivity. Also qualifies EXP-004 — at the same 200 tokens a
  2-token replacement moved 0 bits here, so the earlier “works at 200 tokens” was one draw of
  replacement tokens, not a property of the size.
- `0760b15` — **real defect**: `insert_text` ignored the index's tokenizer (it called the free
  `tokenize`, while `collide_text` honoured it), so any bigram or character index hashed unigrams on
  insert and the configured scheme on query. Found because identical trigram hashes produced *zero*
  collisions, which is impossible for a matching pair. Fixed with weighted units
  (`Tokenizer::tokenize` returns `(text, weight)`, `insert_units` is the single insert path,
  `simhash_weighted` added); the regression test is verified to fail when the fix is reverted.
  EXP-005 and EXP-008's character numbers are marked invalid in place; EXP-008's word-level
  order-invariance result is unaffected. Corrected: character trigrams freeze the hash completely
  (all 60 variants hash identically); word bigrams still do not fix short documents.
- `91534a3` — **bigram vote weight 2 improves both sensitivities**, overturning EXP-005. At 200
  tokens: weight 0 (unigrams) gives order 0 bits / substitution 1.5; weight 1 gives 24 / **0.9**
  (worst of both); weight 2 gives 31 / **4.2**; weight 4 gives 33 / 4.4. A 2-token substitution
  changes ~3 bigrams rather than 2 unigrams, so bigrams carry more substitution information than
  they cost in vote weight. And weight 2 **recovers the 60-token top-1** that unigrams miss and
  weight 1 loses, while keeping 200; weight 4 freezes and loses all. The default
  `with_capacity_levels_and_shingle` weight of 1 is now known to be the measured worst setting —
  recorded for follow-up rather than changed on one synthetic result. 8 tokens fails at every
  weight.
- `057be25` — fine sweep after exposing `unigram_weight` (the knob was integer-only with unigrams
  pinned at 1, so no ratio between 0 and 1 was expressible). Order sensitivity is smooth and
  monotone (0→33 reversal bits across ratios 0→4); substitution sensitivity is not monotone; and
  **top-1 is not a function of the ratio at all** — yes at 0.00 and 2.00, no at 0.10/0.20/0.25/0.50/
  1.00/3.00/4.00. That qualifies EXP-010: "weight 2 is the optimum" overstates it. The
  sensitivities are real and the 60-token gain stands, but top-1 flips between adjacent settings,
  so tuning by top-1 on one corpus is not a reliable method. Recorded as a methodological finding.
- `10f78f6` — replaced the knife-edge top-1 with a **margin measured over 8 corpora** (correct
  variant's shared bits minus the best other). It retires the parameter-tuning line rather than
  resolving it: mean margins 1-6 bits against standard deviations of 15-28, so every tokenizer
  setting is indistinguishable from zero within noise. EXP-010's “weight 2 improves both” and
  EXP-011's “optimum between 1 and 4” were both readings of a metric too knife-edge to support
  them. The dominant failure is **visibility, not ranking** — in most corpora the correct variant
  never reaches the collision threshold at all (0 to 8 of 8 invisible depending on the setting),
  which matches the EXP-007 surface: at 200 units a 2-token edit moves ~1.5 bits on average,
  frequently zero. The unit count is the binding constraint, not the tokenizer choice.
- `4d4aef2` — repeated the margin measurement at 40/60/100/200 tokens to test whether the
  tokenizer settings separate in the resolvable band. **They anti-separate.** At 100 tokens,
  where unigrams give a positive mean margin of 14.8, both bigram settings give **negative**
  margins (-8.5, -5.2) across six corpora each with small standard deviations. The mechanism is
  in the unit count: bigram settings hash roughly double (79 units for 40 tokens, 199 for 100),
  and EXP-007 measured that doubling the unit count collapses sensitivity. So the tokenizer
  choice and the unit-count constraint are the *same* constraint. This closes the
  parameter-tuning line with a mechanism rather than a shrug, and points at the real lever:
  **reducing** the unit count (hashing a sample of tokens rather than all of them), which is
  unmeasured and is recorded as the follow-up.
- `40f588c` — implemented that lever as `Tokenizer::WordsBudget`: a deterministic stride sample,
  stride `ceil(len / target)`, so the unit count is bounded regardless of document length. **The
  surface flattens**: with a budget of 50, document sizes 100/200/400/800 all land in a 2.4-4.9 bit
  band for e=1..2, against the unbudgeted 800-token row's 0.5-0.9. And the budget is the knob: at a
  fixed 800-token document, a 2-token edit moves 5.2/4.9/2.6/1.2/1.0/0.9 bits at budgets
  20/50/100/200/400/800 — monotone in the budget alone. Honest limit: budgeted sensitivity at a
  given budget is *below* an unbudgeted document of that unit count (5.2 at budget 20 vs 9.8 for a
  20-token document), because the stride sample has gaps and a small edit can miss the sampled
  units.
- `ca9c2f4` — measured stride against shuffle sampling at the same budget
  (`Tokenizer::WordsShuffledBudget`, Fisher-Yates from a deterministic LCG with document order
  restored). **The stride is a step function**: 1.0 bits for edits on a stride boundary, 0.0 for
  edits between them, and averaging over seeds does not remove it because the mean over seeds is
  the same step function. **The shuffle has no blind spot** — positive and smooth at every
  position, variance as the price. Also found a bias in the stride's favour: it always samples
  index 0, so every document's opening is over-weighted by construction. For a crawler comparing
  a page against its previous fetch, edits are arbitrary rather than periodic, so the shuffle's
  random failure is the better failure mode.
- `a819f12` — measured the third scheme: `Tokenizer::WordsWindowBudget` hashes a fixed budget of
  *windows* (runs of consecutive tokens) instead of individual tokens. A 2-token edit touches 2 of
  800 token-units but every window overlapping it. **The window scheme has no zeros**: 1.0 bits at
  on-stride positions and 1.0 bits between them, against the stride's 0.0 at between-stride
  positions. It combines the stride's spread coverage with the shuffle's lack of a periodic blind
  spot. Cost: a unit is a 3-token window, so the index says *which window* changed, not which
  token. Limits: one window width, one document size, one budget, one seed.
- `b402cdd` — measured the **false-negative rate** over every second position of an 800-token
  document, 8 seeds each, budget 200. Stride 35.4%, shuffle 40.6%, windows **86.5%**. This
  **corrects EXP-015/016's framing**: the window scheme wins on *coverage* (each unit spans 3
  tokens, so 600 of 800 token positions are covered), not on lacking a periodic blind spot — the
  shuffle has no blind spot and is barely better than the stride, because both sample the same
  fraction of tokens. Periodicity decides *which* edits are missed; coverage decides *how many*.
  Also explains the stride's 35.4% being below the naive 50%: a single-unit change is often
  absorbed by the majority vote.
- `819cc25` — swept the window width (2, 3, 5, 8) at a fixed budget of 200 on an 800-token
  document. Rates: 40.7%, 86.5%, 97.2%, 95.4% against coverage predictions of 50%, 75%, 100%,
  100%. **The trend confirms coverage but the absolutes do not.** The reason: the majority vote
  absorbs a changed unit -- a 2-token edit changes 3 of 799 windows under width 2, so with a budget
  of 200 the expected number of changed sampled units is under one, and one changed unit out of 200
  almost never flips a bit. The decisive quantity is therefore the fraction of *sampled* units that
  changed, not the fraction of positions covered. The absorption floor was inferred from the
  saturation residual, not measured directly.
- `8ddc2fa` — measured that floor by fixing the edit and sweeping the unit budget (window 3,
  800 tokens, 8 seeds). The rate is **not monotone**: 0.5%, 2.5%, 5%, 10%, 25%, 48.9%, **86.5%
  at budget 200**, then 56.2% and 44.9% at budgets 400 and 800. **Hashing every window is worse
  than sampling 25% of them.** Below the peak the limit is coverage; above it, absorption --
  SimHash's per-bit vote margin scales with `sqrt(N)`, so a fixed-size edit is relatively larger
  in a smaller sample and flips more bits. The unit budget is therefore a sensitivity dial as well
  as a memory dial, and "index everything" is the wrong default.
- `da45c8e` — re-ran the budget sweep at 800/1600/3200 tokens on a coarser budget grid. The
  1600 peak moved from 25.1% to 49.9% — a 25-point swing from the sampling, against ~2-point
  run-to-run reproducibility at 800 tokens. The peak is a property of the budget grid, not the
  document. **Withdraws the previous entry's conclusion**: neither the constant-fraction nor the
  drifting reading is supported; the three sizes read 33.1%, 49.9%, 41.6%, which is no trend.
  The unit budget's optimum is not a stable constant fraction; a budget in the 25-50% range is
  defensible, a specific fraction is not. Cause: detection is a single bit flip in a 200-unit
  vote, so the rate is a step function of which units the budget includes.
**Remaining defects.** SimHash is a majority vote over bit positions, so with very short inputs
(2–4 tokens) it is decided by noise; the tests were corrected to use realistic-length texts rather
than asserting on a property the mechanism does not have at that scale. ~~The collision index has no
eviction policy~~ — fixed in `550525d`: the index is bounded by item count (default 100,000,
evicting oldest-first), because each item costs 64 bucket entries and an unbounded index is a
memory-exhaustion vector.

---

## 2026-11-09 — record every pin path in `pinned_cids`

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `crates/oag-storage/migrations/0015_pinned_cids.sql`,
`crates/oag-storage/src/repo/pinned_cids.rs`.
**Files deleted:** `crates/oag-storage/src/repo/pins.rs` (replaced by `pinned_cids`; the endpoint
now reads one indexed table instead of scanning `evidence.metadata`).
**Files modified:** `crates/oag-storage/src/repo/mod.rs`, `crates/oag-graph/src/{pins,lib,assert,
tests}.rs`, `crates/oag-cli/src/commands.rs`, `crates/oag-graph/src/assert.rs`,
`spec/29-ipfs-pinning.md`, `OPEN_QUESTIONS.md`.

**Gap closed.** The discovery surface shipped reading only `evidence.metadata`'s `ipfs_cid` key,
which covered the crawler's page pins and nothing else — `oag ipfs pin`, `oag backup --pin`, and
`oag ipfs host-page` left no queryable trace. So it answered "what did this peer pin as evidence"
rather than "what does it hold".

**Fix.** One `pinned_cids` table recording every pin path: `evidence` (the assert/evidence choke
point, which is how the crawler's pin lands), `cli-pin`, `backup-pin`, `host-page`. `source` is
descriptive metadata for an operator, never a permission or trust signal. The migration backfills
existing evidence CIDs and is idempotent, so a long-running peer does not report an empty list
after upgrading and a re-run cannot duplicate rows.

**Best-effort recording on the assert path**, with a warning, same reasoning as
`observe_sync_outcome`: the pin record is a derived index, not evidentiary state, and the evidence
row keeps its own `ipfs_cid` metadata either way — a failure there must not fail the evidence write
it is observing. The CLI paths warn too, since a failure to index must not un-pin content.

**Tests executed.** `cargo test --workspace` — all 31 test binaries pass, no failures.

**Test results.** oag-storage 33 → 34, oag-graph 95 → 96. New: idempotent recording and
most-recent-first listing, the limit, the backfill's idempotence, `record_pin`'s own cap. The
existing endpoint, MCP, and service tests continue to pass against the new table.

**Performance measurements.** None. The change replaces a full scan of `evidence` with an indexed
lookup, so it is strictly cheaper than what it replaced — no benchmark was run and none is claimed.

**Commit:** `d52dc8f` — pushed.
**Remaining defects.** Recording on the assert path is best-effort, so a peer that fails to index a
pin will under-report until a later pin of the same CID or a rebuild. `pinned_cids` has no
garbage-collection: a peer that unpins content through its IPFS node directly still lists the CID,
which is the same "record, not live check" limit already documented.

---

## 2026-11-09 — CID discovery surface

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `crates/oag-storage/src/repo/pins.rs`, `crates/oag-graph/src/pins.rs`.
**Files modified:** `crates/oag-storage/src/repo/mod.rs`, `crates/oag-graph/src/lib.rs`,
`crates/oag-api/src/{dto,handlers,lib,tests}.rs`, `crates/oag-mcp/src/{params,server,tests}.rs`,
`crates/oag-graph/src/tests.rs`, `spec/29-ipfs-pinning.md`, `docs/api.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The discovery query that `spec/31` and `spec/32` both name as missing:
"how would a peer even learn *which* upstream peer has a given CID without being told?" A caller can
now ask a peer directly.

- `GET /api/v1/pins?cid=<cid>` -> `{"cid": ..., "holds": bool}`.
- `GET /api/v1/pins?limit=<n>` -> `{"pins": [...], "total": N, "limit": n}`, clamped to
  `[0, MAX_PIN_LIST_LIMIT]` at the service layer, matching search's cap.
- MCP `graph_pins`, same two modes. `graph:read`, like every other read surface.

**Scope, stated rather than implied.** The only CID this codebase records today is the projector's
`ipfs_cid` key in `evidence.metadata`, written when the crawler pins a page. So the surface answers
"which CIDs has this peer recorded pinning *as evidence*" — not "what does the IPFS node hold".
Pins from `oag ipfs pin`, `oag backup --pin`, and `oag ipfs host-page` are not recorded anywhere
queryable; closing that needs a `pinned_cids` table plus instrumenting three paths, and is
deliberately not done speculatively.

**And it is a record, not a live check.** Nothing fetches the content to confirm retrievability; a
peer that garbage-collected a CID still answers "yes". Same honesty property as a DNS name.

**Tests executed.** `cargo test -p oag-storage` (33), `-p oag-graph` (95), `-p oag-api` (62),
`-p oag-mcp` (20).

**Test results.** oag-storage 31 → 33, oag-graph 94 → 95, oag-api 61 → 62, oag-mcp 19 → 20.
New: storage tests for distinct-CID listing/counting/holding and the limit; a service test that
seeds real assertions with `pinned_cid` evidence and checks the duplicate collapses, `holds_pin`,
and the limit clamp; an HTTP test for the route's auth and both response shapes; and an MCP test for
the tool.

**Performance measurements.** None. The storage query is a full scan of `evidence` with a
`json_extract`, bounded by `LIMIT` — acceptable for an operator-driven query, and documented as
needing a real index if it ever becomes a hot path.

**Commit:** `8c676a0` — pushed.
**Remaining defects.** The surface under-reports what a peer holds (see scope above). No consumer
uses it yet: the relay still tries configured upstreams in order, and wiring a pre-check there adds
a failure mode (a failed check would skip a peer that might have worked) for a modest saving. That
trade is deliberately not made.

---

## 2026-11-09 — replication_bytes_in/out

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/src/{client,service}.rs`,
`crates/oag-api/src/{state,metrics}.rs`, `crates/oag-cli/src/serve.rs`,
`crates/oag-sync/src/client.rs` tests, `spec/19-observability.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The last two named request-duration metrics. `SyncClient` counts the
streamed bodies of every replication read (`hello`/`heads`/`events`/`peers`) into `bytes_in`,
including ones later rejected as oversized or invalid, and each serialized POST body (`presence`,
`cluster`) into `bytes_out`. Process-lifetime counters shared by `Arc`.

**Why the sharing mattered as much as the counting.** `AppState` holds a *separately constructed*
`SyncService` that never runs the sync loop, so its own client's counters would always read zero --
the same trap `peer_sync_errors` already documents. `oag serve` hands `AppState` the real service's
`Arc`s at startup, exactly as it does for `sync_errors`.

**Tests executed.** `cargo test -p oag-sync` — 41 passed. `cargo test -p oag-api` — 61 passed.

**Test results.** oag-sync 40 → 41. New: `replication_bytes_counters_move_with_real_traffic`,
which fetches a real response and sends a real signed heartbeat against a fixture server and
asserts both counters moved.

**Not counted, deliberately.** The tiny unread responses to the fire-and-forget POSTs: reading them
would buffer a body for a few bytes. The `# HELP` line states the boundary rather than leaving it
to be discovered.

**Performance measurements.** None. The counters cost one atomic add per read body and per POST.

**Commits:** `78b46d9`, `defda80`, `1b61899` — pushed.
**Remaining defects.** `events_by_origin` (a labeled metric needing a decision on label cardinality)
and `blob_store_size` (no blob store) remain unbuilt. `bytes_in` does not cover unread POST
responses, by design and documented.

---

## 2026-11-09 — onion endpoint rate limit

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs` (`onion_limiter` field on `MeshState` plus
both constructors; limiter check at the top of `receive_onion_layer`; restored a doc comment that an
earlier edit had left attached to the wrong constant), `spec/15-onion-routing.md`,
`OPEN_QUESTIONS.md`.

**Gap closed.** The onion endpoint is the one endpoint in this crate that makes the node send
traffic to an address chosen by the caller — the peeled `next_hop` is forwarded to, relay or final
target. Inherent to the feature, but with no bound it also made the relay a constrained proxy for
directing anonymous-event pushes at third parties.

**Fix.** A node-wide token bucket (16 capacity, 0.5/s refill), the same shape as route search's.
Deliberately a volume bound and not a permission system: the endpoint stays unauthenticated (onion
routing's point is that the relay cannot authenticate the sender), the payload is still limited to
an identity-stripped event plus proof-of-work or one more layer, and the target still enforces its
own proof-of-work.

**Tests executed.** `cargo test -p oag-mesh` — 188 passed, 0 failed.

**Test results.** oag-mesh 187 → 188. New: `onion_rate_limiter_rejects_a_burst_and_recovers`. The
limiter check runs before peeling, so a dummy layer exercises it: below the limit a request is
rejected as unpeelable (400), past it as rate-limited (429 + `Retry-After`), and it recovers after
the bucket refills.

**Performance measurements.** None. No benchmark was run. The limiter costs one token check per
request.

**Commit:** `f47bf83` — pushed.
**Remaining defects.** The unauthenticated, arbitrary-target design is unchanged and deliberate.
The bound is node-wide, so one caller can exhaust the bucket and starve legitimate ones — there is
no per-caller key because there is no caller identity. In-memory, so a restart resets it.

---

## 2026-11-09 — crawler dual-stack DNS pinning

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-crawler/src/fetch.rs`, `crates/oag-api/src/gateway.rs`,
`OPEN_QUESTIONS.md`.

**Defect fixed.** `build_pinned_client` looped over the resolved address set calling reqwest's
`resolve()`, which maps a host to exactly one SocketAddr and **overwrites** on every call — so only
the last entry survived. A dual-stack host resolved to `[IPv4, IPv6]` with no outbound IPv6 route
failed outright, with zero fallback to the working IPv4 address, purely because IPv6 happened to be
resolved last.

This was already recorded in `OPEN_QUESTIONS.md` as a latent bug found via the gateway feature and
deliberately left unfixed in the crawler. It is now fixed there too.

**Fix.** `build_pinned_client` pins exactly one address and documents why the loop form was wrong.
`safe_fetch` reduces its resolved set through a new `oag_crawler::fetch::prefer_ipv4` before pinning,
so the choice is deliberate rather than resolver-order-dependent; an IPv6-only host has no A record,
so it is unaffected. `oag-api`'s gateway now calls that shared helper instead of keeping its own
copy — the same logic in two places is how they drift.

**Tests executed.** `cargo test -p oag-crawler` — 75 passed. `cargo test -p oag-api` — 61 passed.

**Test results.** oag-crawler 69 → 75. New: `prefer_ipv4` picks the IPv4 address in both
resolver orders, leaves an IPv6-only set untouched, passes an empty set through;
`build_pinned_client_pins_the_first_address_in_the_set` (a set with a dead IPv6 second entry still
fetches — this is the regression); `pinning_an_unusable_first_address_fails_without_fallback`
(characterises the rule, and is why `safe_fetch` filters first); and
`the_safe_fetch_path_reduces_a_dual_stack_set_before_pinning`.

**Performance measurements.** None. No benchmark was run. The change alters *which* address is
pinned, not how many connections are attempted.

**Commit:** `33563d4` — pushed.
**Remaining defects.** The same overwrite behaviour would affect any other caller that loops
`resolve` over an address set; `build_pinned_client` is the only one in this codebase. IPv4-first is
a deliberate bias for dual-stack-broken networks, not a correctness property — an IPv6-only host is
handled, but a host whose IPv4 is firewalled and whose IPv6 works would now prefer the broken IPv4.

---

## 2026-11-09 — events_pending_chain

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-storage/src/repo/stats.rs`, `crates/oag-graph/src/metrics.rs`,
`crates/oag-api/src/metrics.rs`, `crates/oag-storage/src/{tests,backup}.rs`,
`spec/19-observability.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** `events_pending_chain` (spec section 86): events whose `previous_event_id`
is set but whose predecessor is not present locally. `events_total` counts rows and cannot tell a
complete chain from a pile of fragments; during replication a fragment is the normal state, and a
persistent non-zero value means a predecessor is missing for good. The self-join is on the primary
key, so it is `O(n log n)`.

**Defect found and fixed while doing it.** The earlier `search()` signature change (adding
`offset`) had broken four tests in `oag-storage` -- three in `tests.rs`, one in `backup.rs` -- and
that crate was not in the set run when the change landed. All four are fixed, and the full
workspace suite now passes: 419 tests across 16 crates.

**Tests executed.** `cargo test --workspace` — all crates pass.

**Test results.** oag-storage 30 → 31. New: `pending_chain_counts_events_whose_predecessor_is_
missing`, which inserts a genesis event, a child whose predecessor IS present, and a child whose
predecessor never arrives. That dangling state is exactly what normal ingest never produces, so it
is built by hand rather than driven through the service.

**Performance measurements.** None. The query's complexity is stated (`O(n log n)` via the primary
key); no timing was measured and none is claimed.

**Commit:** `7086294` — pushed.
**Remaining defects.** `events_by_origin` (a labeled metric needing a decision on label
cardinality), `replication_bytes_in`/`out`, and `blob_store_size` remain unbuilt. The lesson from the
broken storage tests is recorded rather than just fixed: a signature change to a shared storage
function warrants running that crate, and the crate list in the engineering log should include it.

---

## 2026-11-09 — api_latency and search_latency histograms

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `crates/oag-graph/src/latency.rs`, `crates/oag-api/src/latency.rs`.
**Files modified:** `crates/oag-graph/src/{lib,service,search,semantic_search,metrics}.rs`,
`crates/oag-api/src/{lib,state,metrics}.rs`, `crates/oag-graph/src/tests.rs`,
`crates/oag-api/src/tests.rs`, `spec/19-observability.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The two duration histograms spec section 86 names. `search_latency` is
recorded inside `GraphService::search` and `semantic_search` (so REST, MCP, CLI, and the Human
Interface all measure the same thing); `api_latency` is recorded by an axum middleware that sees
routing, auth, handler, and response as a whole. Fixed buckets, atomic counters, no dependency.
Values are milliseconds, matching `duration_ms` on every span.

**Why.** Every other metric here is a count, and a count answers "how much", not "how slow". The
per-request duration existed only as a `tracing` field, which is right for one request in a log and
useless for "is this peer getting slower?" across a million of them.

**Tests executed.** `cargo test -p oag-graph` — 94 passed. `cargo test -p oag-api` — 61 passed.

**Test results.** oag-graph 87 → 94 (6 histogram unit tests + `search_latency_records_every_
search_this_process_serves`). oag-api 60 → 61 (`api_latency_counts_rest_requests_end_to_end`,
which serves a real request and confirms the histogram counted it — a unit test of the histogram
would not prove the middleware is wired to it).

**Performance measurements.** None. No benchmark was run and none is claimed. The histograms cost
one atomic increment per observation by construction.

**Commit:** `fafc44a` — pushed.
**Remaining defects.** `events_by_origin` (a labeled metric needing a decision on label cardinality),
`events_pending_chain`, `replication_bytes_in`/`out`, and `blob_store_size` remain unbuilt. The
histogram sum is accumulated as integer microseconds, so a peer with extremely long uptime could in
principle lose precision in the sum; the bucket counts are unaffected.

---

## 2026-11-09 — order the mesh's anti-entropy pull loop

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs` (`sync_with_all_peers` orders its peers by
the `link_quality` ledger), `spec/10-anti-entropy.md`.

**Feature implemented.** The sequential pull loop now pulls best-quality peers first, so a periodic
tick gets the useful data before spending its time on peers that are probably down.

**Why.** `sync_with_all_peers` is sequential, so pulling in configuration order means every
black-holed peer is waited out -- a full connect/request timeout each, since the timeout fix landed
-- before a single live peer is reached. With a tick interval shorter than the accumulated
timeouts, the loop is permanently behind.

**Tests executed.** `cargo test -p oag-mesh` — 187 passed, 0 failed.

**Test results.** oag-mesh 186 → 187. New: `an_ordered_pull_loop_gets_live_data_before_paying_a_
dead_peers_timeout`, a two-arm control experiment. Same hanging peer, same live peer, same client;
the only difference is whether the ledger has an opinion. Without history the live data does not
arrive within the 1s window while the hanging peer blocks; with history it does.

**Performance measurements.** None. The total tick time is unchanged by ordering; what changes is
when the useful data arrives. No benchmark was run and none is claimed.

**Note on suite wall-clock.** The mesh's PoW tests are CPU-bound and the full suite's duration
varies substantially with machine load — the `corroboration::` tests alone measured 18s, 34s and
60s across three consecutive runs. That variance is environmental, not from this change.

**Commit:** `740576b` — pushed.
**Remaining defects.** Ordering does not reduce the total time a tick spends on dead peers; a
per-tick time budget would, and is not built. The ledger is still forgotten on restart, so the first
tick after a restart always pulls in configuration order.

---

## 2026-11-09 — sync client timeouts (base project)

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/src/client.rs`, `spec/18-rate-limiting-and-resource-limits.md`.

**Defect fixed.** The same one just fixed in the mesh: `SyncClient` was built from
`reqwest::Client::default()`, which sets no timeout. Its response *size* was already capped and
streamed (4 MiB, `MAX_RESPONSE_BYTES`), but nothing bounded how long a peer could make this node
wait. A peer that silently drops packets held a sync round open until the OS gave up.

**Fix.** `SYNC_CONNECT_TIMEOUT` (10s) and `SYNC_REQUEST_TIMEOUT` (120s). The total is deliberately
longer than the mesh's 30s: an `/events` response is a real batch of signed events, not a small
snapshot, so a legitimate peer on a slow link needs more room. `SyncClient::new()` uses the bounded
defaults; `sync_client_with_timeouts` takes the values as parameters so a test can prove the bound
without waiting the production value out; `with_http` lets an embedder supply its own transport.

**Tests executed.** `cargo test -p oag-sync` — 40 passed, 0 failed.

**Test results.** oag-sync 38 → 40. New: `a_peer_that_accepts_but_never_responds_is_abandoned_at_
the_timeout` and `the_default_client_is_bounded_and_reaches_a_real_peer`.

**Performance measurements.** None beyond the bound itself. No throughput benchmark was run.

**Commit:** `b2e2182` — pushed.
**Remaining defects.** The operator-configured external-service clients (LLM extractor, embeddings
provider, IPFS provider) still have no timeouts. They are a lower trust tier and are recorded in
`spec/18` rather than fixed silently. The CLI and gateway one-shot clients are likewise unbounded.

---

## 2026-11-09 — mesh outbound HTTP timeouts

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs` (new `HTTP_CONNECT_TIMEOUT` /
`HTTP_REQUEST_TIMEOUT` constants and `build_http_client`; both `MeshState` constructors now use
it), `spec/04-transport.md`, `OPEN_QUESTIONS.md`.

**Defect fixed.** Every outbound request used `reqwest::Client::new()`, which sets no timeout at
all. A peer that is unreachable *and silently dropping packets* — rather than refusing the
connection — holds the request open until the OS gives up, which can be minutes.

**Why it mattered more here than a slow pull.** `sync_with_all_peers` pulls peers sequentially, so
one black-holed peer stalls the whole tick before any live peer is reached. And
`search_for_route`'s fan-out calls `push_route_search` directly, on the path of an *inbound*
`POST /oag-mesh/v1/route-search` — so one broken peer can hang a request from a third party.

**Fix.** A 5s connect timeout (handshake never completes) and a 30s total request timeout
(connection succeeds, then silence). Both named constants; `build_http_client` takes them as
parameters so a test can prove the bound without waiting the production value out. A timed-out
request is an ordinary failed delivery: `pull_and_merge` reports it and `link_quality` records it.

**Tests executed.** `cargo test -p oag-mesh` — 186 passed, 0 failed.

**Test results.** oag-mesh 184 → 186. New: `a_peer_that_accepts_but_never_responds_is_abandoned_
at_the_timeout` (raw client, 50ms/200ms, asserts an error well inside 5s) and
`pull_and_merge_against_a_black_holed_peer_is_bounded_and_recorded` (end to end through the mesh's
own pull path, ~1.3s, and the failure is recorded).

**Performance measurements.** None beyond the bound itself: the pull returned in ~1.3s against a
200ms request timeout. No throughput benchmark was run and none is claimed.

**Commit:** `531b6f9` — pushed.
**Remaining defects.** The base project (`rust-ai`) has the same missing-timeout defect in
`oag-sync` and several CLI/API clients; not yet fixed there. The mesh's push-side broadcasts use
the same bounded client, so they are covered. The 30s total timeout is a starting value, not
derived from a deployment's link characteristics.

---

## 2026-11-09 — mesh link-quality fan-out ordering

**Repository:** `oag-mesh`
**Files created:** `crates/oag-mesh/src/link_quality.rs`.
**Files modified:** `crates/oag-mesh/src/lib.rs`, `crates/oag-mesh/src/transport.rs`
(`MeshState.link_quality`; `pull_and_merge` now returns whether it delivered; route search orders
peers and records both outcomes), `spec/13-distributed-route-search.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** A per-peer exponential moving average of whether each interaction
actually delivered. Route search asks its peers best-first, so the fan-out budget -- the one
scarce resource a search has -- is not spent on peers that have been refusing connections for
hours. Ordering only: the function is a permutation of the input, asserted as a property, so a
down peer is asked last rather than dropped.

**Design decisions.** Alpha 0.1, the same learning rate as the base project's tensor pad: a
single dropped packet moves a healthy peer by a tenth of the gap, so routing does not oscillate,
while a consistently-dead peer is demoted within a handful of rounds. Unseen peers score 0.5
(neutral, not optimistic) so a node with no history spreads its fan-out rather than preferring
configuration order. Both successes and failures are recorded -- a peer that did not answer is
exactly the observation the next search needs. In-memory only: losing it costs a little wasted
fan-out, and persisting it would add an eighth store and change the commit-marker set for a value
that is by nature ephemeral. It is a liveness signal, not a trust signal.

**Tests executed.** `cargo test -p oag-mesh` — 184 passed, 0 failed.

**Test results.** oag-mesh 175 → 184. New: 6 ledger unit tests (neutral unseen peer, dead sorts
last, ordering is a permutation that never drops, smoothed score, recovery, budget-of-one goes to
the proven peer) and 3 real-network transport tests (`a_failed_fan_out_is_recorded_and_the_next_
search_spends_its_budget_better`, `pull_and_merge_records_a_failure_against_an_unreachable_peer`,
`pull_and_merge_records_a_success_against_a_live_peer`).

**Performance measurements.** The ordering effect was measured on a real loopback network, not
modelled: with a dead peer and a fan-out budget of one, the first search returns `None` and the
second returns a path after a single observation. See `EXPERIMENTS.md` EXP-013. No throughput or
latency benchmark was run, and none is claimed.

**Commit:** `67ad91e` — pushed.
**Remaining defects.** Not exercised over radio, only loopback TCP. The end-to-end delivery-rate
effect is unmeasured -- only the budget-slot effect is. A peer that answers with garbage scores
exactly as well as one that answers correctly, which is deliberate (liveness, not trust) but means
this cannot be used as a reputation input.

---

## 2026-11-09 — imagination Reticulum carrier

**Repository:** `oag-imagination-random-data-compute`
**Files created:**
- `crates/oag-imagination-reticulum/Cargo.toml`, `src/lib.rs`, `tests/loopback.rs`
- workspace `Cargo.toml` (new member + `oag-imagination` path dep)
**Files modified:** `spec/03-transports.md`, `README.md`, workspace `Cargo.toml`.

**Feature implemented.** The last carrier left as a seam. `ReticulumTransport` (client, implements
both `ReticulumCarrier` and `Transport`) and `serve`/`serve_on` (server), framing over a real
Reticulum `Link` with the same 4-byte length-prefix + 1400-byte chunk format the base project's own
Reticulum transport uses.

**Design decisions.** A separate crate so `oag-imagination` keeps no `reticulum` dependency; and
`oag-reticulum` could not host the adapter either, because `oag-imagination` depends on the base
project's crates, making that a circular *repository* dependency. Identity comes from a seed string
(the caller passes the owning `oag` peer's id) rather than a peer identity, preserving the base
project's "never derive from the public `PeerId`" property without an `oag-crypto` dependency.

**Tests executed.** `cargo test --workspace` — 40 passed (oag-imagination) + 3 passed (new
adapter), 0 failed.

**Test results.** New: `a_request_travels_over_a_real_reticulum_link` (real `TcpServer`/`TcpClient`
loopback, real engine answer, reply carries the server node's contribution);
`a_send_to_an_unknown_destination_fails_softly`; `a_bad_destination_address_is_a_clean_error`.

**Performance measurements.** None. No benchmark was run. The suite runs in ~10s, dominated by the
failure-path timeout, which was shortened to 2s via a new `with_announce_wait` builder.

**Commits:** `3dfc759` (transport chunked-path coverage, rust-ai), `9f63af7` (adapter) — pushed.
**Remaining defects.** The adapter has not been exercised over real radio, only loopback TCP as the
Reticulum interface. The identity seed is a caller responsibility: a changed seed silently changes
the node's address, which is documented but not enforced. `serve` hardcodes a 5s announce interval.

---

## 2026-11-09 — mesh commit marker (crash-consistent store set)

**Repository:** `oag-mesh`
**Files modified:**
- `crates/oag-mesh/src/persistence.rs` — added `CommitEntry`/`CommitMarker`/`CommitState`,
  `hash_file`, `write_commit_marker`, `verify_commit_marker`, `STORE_FILES`, and a
  `PersistenceError::TornSet` variant; corrected the module doc (it still said "three in-memory
  stores" when there are seven).
- `crates/oag-mesh/src/transport.rs` — `save_to_dir` hashes each of the seven files and writes the
  marker last; `load_from_dir` verifies it before loading.
- `spec/08-persistence.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** A saved set of the seven stores is now atomic *to a reader*. Each file
was already individually atomic, but a crash between saves could leave a torn set where every file
parses and nothing downstream could tell they disagreed. The marker closes that: written last, and
verified first, with a `TornSet` refusal when the files contradict it.

**Tests executed.** `cargo test -p oag-mesh` — 175 passed, 0 failed.

**Test results.** oag-mesh 165 → 175. New: 7 persistence tests (legacy/no-marker, consistent,
deleted file, modified file, corrupt marker, wrong version, idempotent re-write) and 3 transport
tests (fresh save verifies; load refuses a torn set; load still accepts a pre-marker directory).

**Performance measurements.** None. BLAKE3 over seven small JSON files on load is not a measured
bottleneck and no benchmark was run.

**Commit:** `7223852` — pushed.
**Remaining defects.** No fsync, so a power loss can still lose a rename. No atomic multi-store
*write* on the pull side. A caller that saves rarely can still lose up to a whole save's worth of
in-memory state. The torn-set path is tested by constructing the torn state directly, not by
inducing a real crash -- a real kill mid-save is not reproducible on demand.

---

## 2026-11-09 — search pagination

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:**
- `crates/oag-storage/src/repo/nodes.rs` — `search` takes an `offset`; SQL is now
  `LIMIT ? OFFSET ?`.
- `crates/oag-graph/src/search.rs` — `search` takes an `offset`, clamped at 0.
- `crates/oag-graph/src/resolve.rs` — the internal candidate lookup passes 0.
- `crates/oag-api/src/dto.rs`, `handlers.rs` — REST `offset` query parameter.
- `crates/oag-mcp/src/params.rs`, `server.rs` — MCP `offset` argument.
- `crates/oag-cli/src/main.rs`, `commands.rs` — `--offset` flag.
- `crates/oag-api/src/human.rs`, `templates/search.html` — fixed 50-per-page with
  Previous/Next links.
- Tests: `crates/oag-graph/src/tests.rs`, `crates/oag-api/src/tests.rs`,
  `crates/oag-mcp/src/tests.rs`, `crates/oag-cli/src/commands.rs`.
- Docs: `spec/07-search.md`, `spec/10-rest-api.md`, `spec/11-mcp-server.md`,
  `spec/12-cli.md`, `spec/13-human-interface.md`, `docs/api.md`, `OPEN_QUESTIONS.md`,
  `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `offset` on all four search surfaces. Previously a `limit` with no
`offset` meant only the first page was reachable, and the Human Interface hard-coded 50 with no
way forward.

**Tests executed.** `cargo test -p oag-graph` (87 passed), `-p oag-api` (60 passed),
`-p oag-mcp` (19 passed), `-p oag-cli` (19 passed, 2 ignored for needing public DNS).

**Test results.** All passed. oag-graph 86 → 87, oag-api 59 → 60, oag-mcp 18 → 19,
oag-cli 20 → 21.

**Performance measurements.** None. No benchmark was run; the change adds one bound parameter to
an existing indexed FTS5 query and does not alter its plan.

**Commit:** `e1e6110` — pushed.
**Remaining defects.** Ordering is FTS5 `rank`, which is not a stable keyset: a node inserted
between two page fetches can shift later pages. Recorded in `OPEN_QUESTIONS.md`; a
`(rank, node_id)` cursor would fix it. Not built, because no current caller needs strict
consistency and the fix costs an extra `ORDER BY` column that every other caller would pay for.

---

## 2026-11-09 — verification metrics

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:**
- `crates/oag-graph/src/service.rs` — added `Arc<AtomicU64>` `verification_jobs` /
  `verification_failures` fields on `GraphService`; added `record_verification(bool)`.
- `crates/oag-graph/src/assert.rs` — `verify_assertion` now wraps the whole call (including the
  `Permission::GraphVerify` check) in one counted outcome.
- `crates/oag-graph/src/metrics.rs` — two new `MetricsSnapshot` fields, populated from the atoms.
- `crates/oag-graph/src/tests.rs` — new `verification_counters_count_jobs_and_failures`.
- `crates/oag-api/src/metrics.rs` — two new Prometheus lines; test fixture and expected text
  updated (14 → 16 metric lines).
- `OPEN_QUESTIONS.md`, `spec/19-observability.md` — documented.

**Feature implemented.** `verification_jobs` / `verification_failures`, counted per
`verify_assertion` call. A rejected attempt (unknown id, missing permission) counts as a failure,
because the count is taken *after* the outcome is known.

**Why not a `COUNT(*)`.** Rejected attempts never become events, so a snapshot query returns a
permanent zero. Recorded in `RESEARCH_JOURNAL.md`.

**Tests executed.** `cargo test -p oag-graph verification_counters` (1 passed);
`cargo test -p oag-api metrics` (4 passed).

**Test results.** All passed. oag-graph total now 86 tests; oag-api 59.

**Commit:** `0a84a78` — pushed.
**Remaining defects.** None known. `blob_store_size` remains unbuilt because no blob store exists.

---

## 2026-11-09 — `oag ipfs pin <file>`

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-cli/src/commands.rs`, `crates/oag-cli/src/main.rs`,
`spec/12-cli.md`.

**Feature implemented.** Pin a local file into the native IPFS node from the CLI, mirroring
`oag backup --pin`. Fails loudly when no provider is configured rather than silently doing
nothing — a pin command that appears to succeed while pinning nothing is worse than an error.

**Tests executed.** `cargo test -p oag-cli` — 20 passed (2 new: disabled-provider error,
directory/oversized rejection).

**Commit:** `bab5b74` — pushed. **Remaining defects:** none.

---

## 2026-11-09 — declare_alias over REST and MCP

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-api/src/dto.rs`, `crates/oag-api/src/handlers.rs`,
`crates/oag-api/src/lib.rs`, `crates/oag-api/src/tests.rs`, `crates/oag-mcp/src/params.rs`,
`crates/oag-mcp/src/server.rs`, `crates/oag-mcp/src/tests.rs`, `docs/api.md`,
`OPEN_QUESTIONS.md`.

**Feature implemented.** `POST /api/v1/nodes/{id}/aliases` and the `graph_declare_alias` MCP tool,
both `graph:assert`. Deliberately REST/MCP rather than CLI: it matches the established
dispute/retract/supersede convention, and the crawler already covers the CLI use case.

**Tests executed.** oag-api and oag-mcp targeted tests; all passed. MCP total now 18.

**Commit:** `1b6863e` — pushed. **Remaining defects:** none.

---

## 2026-11-08 — IPFS site routes tested; spec/22 corrected

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-api/src/ipfs_site.rs`, `crates/oag-api/src/tests.rs`,
`spec/22-future-work.md`.

**Feature implemented.** HTTP test `ipfs_site_routes_are_unauthenticated_and_inject_the_cid`.
Corrected a stale spec note claiming Phase 3 static hosting was unbuilt — it is built.

**Tests executed.** `cargo test -p oag-api ipfs_site` — passed.

**Commit:** `4ac3a57` — pushed. **Remaining defects:** none.

---

## 2026-11-08 — pad traffic reinforcement

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `spec/23-tensor-pad-and-evolution-layer.md`, imagination engine.

**Feature implemented.** Recorded that traffic strengthens the tensor pad and that pads act as
data pads. See `INVENTION_REGISTER.md` OAG-INV-005.

**Commit:** `4bdef2d` — pushed.

---

## 2026-11-08 — onion rejection paths

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/onion.rs`.

**Feature implemented.** Tests for the onion layer's rejection paths: empty path → `EmptyPath`,
key/path mismatch → `BadLength`, malformed layer (bad hex, wrong nonce length, AEAD tag failure).
Six onion tests.

**Tests executed.** `cargo test -p oag-mesh onion` — passed.

**Commit:** `0a3a61f` — pushed. **Remaining defects:** padding discipline end-to-end is unverified;
the onion chain and anonymous events are not yet composed.

---

## 2026-11-08 — cyclic route search terminates

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`.

**Feature implemented.** `find_route_across_mesh_terminates_on_a_cyclic_peer_graph` (A↔B, B→C,
C→B) with a 10s timeout. Guards against an infinite loop in route search on a cyclic topology.

**Commit:** `23c8310` — pushed.

---

## 2026-11-08 — persistence temp-name uniqueness

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/persistence.rs`.

**Feature implemented.** Each save gets a unique temp name (spec/08), removing a race where
concurrent saves could collide on a fixed temp path.

**Commit:** `be18343` — pushed.

---

## 2026-11-08 — blinded responder dispatch

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/dispatch.rs`, `crates/oag-mesh/src/persistence.rs`.

**Feature implemented.** Filter subscription under a responder-chosen random id plus a polled
inbox; the subscription registry and inbox are persisted (spec/08, spec/14). See
`INVENTION_REGISTER.md` OAG-INV-004.

**Commits:** `0b21f3c`, `7f4d364` — pushed.

---

## 2026-11-08 — three-node chain, stale carrier status

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/integration.rs`, `README.md`.

**Feature implemented.** `a_request_can_travel_through_a_three_node_chain`. Corrected the README's
stale carrier status.

**Commit:** `ca94a0d` — pushed. Imagination total now 37 tests.

---

## 2026-11-08 — hardware randomness seam resolved

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/source.rs`.

**Feature implemented.** Resolved the stale `source.rs` TODO: `DeviceRandom` already implements
the seam. Added `a_device_source_reads_exactly_the_device_bytes`.

**Commit:** `0559614` — pushed.

---

## 2026-11-08 — empty-node and address-less-peer paths

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/node.rs`.

**Feature implemented.** `a_node_with_no_pad_starts_empty_and_peers_without_addresses_are_skipped`.

**Commit:** `1b037db` — pushed.

---

## 2026-11-08 — unreachable next hop degrades

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`.

**Feature implemented.** `an_unreachable_next_hop_still_gets_a_response_not_a_hang` — ten socket
tests. Consistent with "degrade, don't drop."

**Commit:** `898c927` — pushed.

---

## 2026-11-08 — spec/03 carrier status corrected

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `spec/03-transports.md`.

**Feature implemented.** Corrected stale text: HTTP, tunnel, and IPFS carriers are built; only the
Reticulum adapter remains a seam.

**Commit:** `5b51569` — pushed.

---

## 2026-11-09 — persistent project records created

**Repository:** `open-agent-graph` (`rust-ai`)
**Files created:** `PROJECT_STATE.md`, `RESEARCH_JOURNAL.md`, `INVENTION_REGISTER.md`,
`ENGINEERING_LOG.md`, `EXPERIMENTS.md`, `ARCHITECTURE_DECISIONS.md`.

**Note.** A first draft of `PROJECT_STATE.md` was created and then removed at the operator's
request during a context-clearing exchange. The records were recreated on the next directive
because PART XV requires them and they must live inside the authorized repositories.

**Remaining defects.** None. These files are living documents and will be appended, not rewritten.

---

## 2026-11-09 — keyset pagination for search

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-storage/src/repo/nodes.rs`, `crates/oag-graph/src/search.rs`,
`crates/oag-graph/src/tests.rs`, `crates/oag-api/src/dto.rs`, `crates/oag-api/src/handlers.rs`,
`crates/oag-api/src/tests.rs`, `crates/oag-mcp/src/params.rs`, `crates/oag-mcp/src/server.rs`,
`crates/oag-mcp/src/tests.rs`, `crates/oag-cli/src/main.rs`, `crates/oag-cli/src/commands.rs`,
`spec/07-search.md`, `spec/12-cli.md`, `docs/api.md`, `OPEN_QUESTIONS.md`,
`PROJECT_STATE.md`, `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `cursor` / `--cursor` on REST, MCP, and CLI, backed by
`CollisionIndex`-free keyset search in the storage layer and a single `GraphService::search_page`
so the surfaces cannot drift. `ORDER BY rank` became `ORDER BY rank, nodes.node_id`.

**Tests executed.** `cargo test --workspace -j 2`: 398 passed, 0 failed (was 393). New tests:
`search_cursor_walk_covers_every_match_once_across_an_insertion` (service),
`search_cursor_rejects_a_malformed_token_by_starting_over` (service),
`search_cursor_walk_does_not_repeat_a_row_when_a_match_arrives_mid_walk` (REST).

**Performance measurements.** None. The cursor path adds one indexed lookup per page (resolving
the cursor row's current rank); the offset path is unchanged.

**Commit identifiers:** `2d28af3`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** A corpus change that *reorders* rows across the cursor boundary still
repeats or skips a row; unfixable by any pagination scheme and documented as such. The Human
Interface keeps offset-only paging.

---

## 2026-11-09 — the embedded IPFS node keeps a stable dial identity

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-ipfs/src/embedded.rs`, `crates/oag-ipfs/Cargo.toml`,
`spec/29-ipfs-pinning.md`, `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `EmbeddedIpfsProvider::new` loads or creates `<repo>/libp2p.key` and
builds with `DefaultIpfsBuilder::with_keypair` instead of `new()`, which generated a fresh
keypair per call. Added `peer_id()` and `shutdown()` accessors; `shutdown()` is needed because
dropping the handle does not release the repo lock.

**Tests executed.** `cargo test -p oag-ipfs`: 14 passed, 0 failed (was 11). New tests:
`the_libp2p_identity_survives_a_restart`, `a_corrupt_identity_file_is_refused_rather_than_regenerated`,
`the_persisted_identity_is_owner_only`.

**Performance measurements.** None; the change is a file read on startup.

**Commit identifiers:** `274ff31` (code), `9007284` (lockfile).
**Push status:** pushed to `origin/main`.
**Remaining defects.** None known. The key file is a secret and is written `0o600` on Unix; on
other platforms it relies on the containing directory's permissions, same as OAG's own
`identity.key`.

---

## 2026-11-09 — source independence accounts for mirrored content

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-graph/src/corroboration.rs`, `crates/oag-graph/src/tests.rs`,
`spec/06-corroboration-and-authority.md`, `ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `source_independence` now groups evidence by the connected components of
two relations -- same control group, or same `content_hash` -- instead of the control group alone.
One article syndicated to several domains was previously scored as several independent sources.

**Tests executed.** `cargo test -p oag-graph`: 101 passed, 0 failed (was 98). New tests:
`identical_content_on_two_domains_is_one_source_not_two`,
`distinct_content_on_one_domain_is_still_one_source`, `union_find_joins_transitively_and_is_deterministic`.

**Performance measurements.** None; the extra work is one hash map per corroboration query, which
was already O(evidence items) and still is.

**Commit identifiers:** `26b45da`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** A caller can assert distinct `content_hash` values for identical content and
regain the independence the grouping removes; `content_hash` is caller-asserted on the REST/MCP
path. Documented in `spec/06`.
