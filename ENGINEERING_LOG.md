# ENGINEERING_LOG — Project OAG

Append-only engineering journal. Newest entries at the top. Each entry records date,
invocation, repository, files modified, features implemented, tests executed, test results,
performance measurements, commit identifiers, push status, and remaining defects.

---

## 2026-11-12 — EXP-024: the index ranks by a weaker signal than it stores

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/src/lib.rs`,
`crates/oag-collisions/examples/ranking_margin.rs`, `EXPERIMENTS.md`, `README.md`.

**Question.** EXP-023 left open whether the thin margin limits the ranking. The question hides a
distinction: the index ranks by **longest shared prefix** (the position of the first differing
bit), while EXP-023 measured **Hamming distance** (the number of bits moved). A variant one bit
away at the most significant position shares nothing; one bit away at the least significant
position shares 63.

**Measurement.** `examples/ranking_margin.rs`: an 800-token document, window 3, budget 200, 20
variants at each edit distance from 2 to 256 tokens.

| signal | Spearman with edit distance |
|---|---|
| Hamming distance | **+0.916** |
| shared prefix (the index's ranking signal) | **-0.443** |

The prefix ordering inverts **22.3%** of the 10,409 comparable pairs relative to the Hamming
ordering (29.7% among the 62 candidates the prefix filter admits). The Hamming ordering inverts
0%, by construction.

**Fix.** `CollisionIndex::collide_by_hamming` (and `_text`) re-ranks the candidates by the
full-hash Hamming distance. The index already stored every item's full hash and already held the
candidate set in memory, so the re-rank is `O(candidates)` with no extra memory. `Collision` gains
a `hamming` field alongside `shared_bits`. The candidate set is unchanged -- the prefix buckets
still decide recall, and they exclude 61% of variants here (70% of 2-token variants are
candidates; 5% of 256-token ones).

**Result.** On the measured corpus the top-1 improves from a 4-token edit to the 2-token edit
that is the closest detectable document.

**Tests executed.** `cargo test -p oag-collisions`: 30 passed, 0 failed (was 29). New test
`collide_by_hamming_orders_the_candidates_by_full_hash_distance`. `cargo build --examples`: clean.

**Commit identifiers:** `e069c28`.
**Push status:** pushed.
**Remaining defects.** The recall limit of the prefix filter is described, not fixed: closing it
would require scanning every item, which is the cost the index exists to avoid. The Spearman
coefficients are over one synthetic corpus.

---

## 2026-11-12 — EXP-023 finding 4: the margin is set by the edit, not the budget

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/examples/distribution.rs`, `EXPERIMENTS.md`.

**Question.** EXP-023 separated the two causes of a miss -- the coverage gap and the
majority-vote margin -- and left the second described but not modelled. If the thin margin were a
budget problem, a larger budget would thicken it.

**Measurement.** Panel 4 of `examples/distribution.rs`: at a fixed budget of 200, vary the edit
length over 100 random positions x 5 seeds.

| edit | detected | mean bits moved |
|---|---|---|
| 1 token | 56.8% | 1.12 |
| 2 tokens | 84.2% | 1.68 |
| 4 tokens | 89.6% | 2.13 |
| 8 tokens | 96.4% | 3.12 |
| 16 tokens | 99.2% | 4.06 |

The margin grows with the number of changed units, and even a 16-token edit out of 800 moves only
about 4 bits; the 11+ bucket stays essentially empty. The mechanism is the majority vote: an edit
crosses only the margins of the units whose windows it overlaps.

**Consequence.** A high detection rate does not mean a confident detection. The rate saturates
near 100% for a 16-token edit while the mean margin is still 4 bits; at the common 2-token edit
the index detects 84% at a mean of 1.68 bits. A consumer that treats "detected" as "these
documents differ" is reading a 1-bit signal at the common edit size.

**Tests executed.** `cargo test -p oag-collisions`: 29 passed, 0 failed. `cargo build --examples`:
clean. Example runtime 22s.

**Commit identifiers:** `32a683e`.
**Push status:** pushed.
**Remaining defects.** The margin measurement and the ranking measurement are still unjoined:
if a detection is 1-2 bits, the longest-shared-prefix rank is 62-63 bits, and whether that orders
 two near-duplicates at different distances is the question EXP-004 raised and EXP-023 does not
 answer.

---

## 2026-11-12 — EXP-023 corrected: the detection rate is a step function of the stride

**Repository:** `oag-collisions`
**Files modified:** `crates/oag-collisions/examples/distribution.rs`, `EXPERIMENTS.md`.

**Defect in the previous entry's own measurement.** The first EXP-023 run sampled edit positions
with a systematic step. A step resonates with the window stride `ceil(798/budget)`: a step of 16
read 99.8% at budget 50 (the coverage of window *starts*, not of edits), and a step of 7 was
coprime with most strides but shared a factor with the strides at budgets 60, 120 and 130. The
reported curve was contaminated at those points. Positions are now sampled pseudo-randomly, which
has no resonance with any stride.

**Three findings from the clean measurement.**

1. **The detection rate is a function of the stride, not the budget.** Budgets sharing a stride
   give identical rates (spread exactly 0.0): stride 8 -> 41.0%, 7 -> 48.2%, 6 -> 65.8%, 5 -> 75.0%,
   4 -> 84.2%. The curve is a step function; the "jaggedness" EXP-022 could not resolve is the
   step, and there is no budget to tune between strides.
2. **The window scheme has a periodic blind spot, and it is total.** At budget 100 (stride 8), the
   residues modulo the stride are missed at 2.0%, 0.0%, 0.0%, **100.0%, 100.0%, 100.0%, 100.0%**,
   4.7%. The gap is `(stride - window - 1)/stride` of positions, zero only when
   `stride <= window + 1` -- at 800 tokens and window 3, a budget of at least 200. This refines
   EXP-016's "no blind spot" claim, which was measured at budget 200 and holds there but not below.
3. **Most detections are thin-margin.** At budget 200, 62.2% of detections move only 1-2 bits; the
   6-10 and 11+ buckets are nearly empty at every budget.

**Tests executed.** `cargo test -p oag-collisions`: 29 passed, 0 failed. `cargo build --examples`:
clean (one pre-existing `unused variable: shuffle` warning in `ranking_tradeoff.rs`, untouched).

**Commit identifiers:** `3e70a6a`.
**Push status:** pushed.
**Remaining defects.** The blind-spot formula is derived for a 2-token edit; the general form for
an `edit_len`-token edit is recorded in EXP-023's limits but not measured. The margin-failure
mechanism (why a covered edit does not flip a bit) is described, not modelled.

---

## 2026-11-12 — the pruner no longer races cargo; EXP-023 measures detection as a distribution

**Repository:** `open-agent-graph` (scripts) and `oag-collisions` (experiment).
**Files modified:** `scripts/prune-target.py`, `scripts/rustc-wrapper.sh`,
`scripts/test_prune_target.py`, `scripts/README.md` (all four repositories);
`oag-collisions/crates/oag-collisions/examples/distribution.rs`, `oag-collisions/EXPERIMENTS.md`.

**Defect repaired (introduced by the earlier prune hook).** The pruner deleted
`incremental/`, `.fingerprint/`, and `build/` directories that cargo had created for the units it
was about to compile, and cargo then failed with `failed to write .../invoked.timestamp: No such
file or directory`. Reproduced by `cargo build --examples` after clearing the fingerprint
directory: 2 of 3 runs failed. The wrapper now reads the cargo process's own start time
(`stat -c %Y /proc/<pid>`) and passes it as `--protect-after`; the pruner skips anything at or
after that horizon. The generation being built is never pruned; old generations still are.

**Tests executed.** `python3 scripts/test_prune_target.py`: 16 passed, 0 failed (was 14). New:
`test_protect_after_skips_generations_cargo_is_using`,
`test_protect_after_skips_dependency_outputs_cargo_is_writing`. Also `cargo build --examples` in
`oag-collisions`: 3 consecutive clean runs after the fix, 2 of 3 failing before it.

**Experiment EXP-023.** `examples/distribution.rs` implements the follow-up EXP-022 left open:
the distribution of detection outcomes over many (budget, position, seed) triples, not a peak.
An 800-token document, a 2-token edit, window sampling at width 3, a fine budget grid, 114 edit
positions, 5 seeds. Three findings, all measured:

- **The jaggedness is the normal case.** 10 of 19 adjacent budget pairs differ by more than their
  combined Wilson half-widths. Budget 120 detects 96.1%; budget 140 detects 62.5% -- increasing
  the budget can cost 34 points.
- **Most detections are thin-margin.** At budget 200, 60.4% of detections move only 1-2 bits.
- **The misses are positionally concentrated.** At budget 100 the per-position miss rate ranges
  0-100% with a 49-point standard deviation, so the window scheme does have positional blind
  spots after all.

**Methodological trap recorded.** The first version sampled edit positions every 16 tokens,
which is exactly the window stride at budget 50, so every tested position was a sampled window's
start and the rate read 99.8% -- the coverage of window starts, not of edits. A position step
must be coprime with the stride (7 is coprime with 16, 8, and 4).

**Commit identifiers:** `89014ff` (scripts fix, open-agent-graph), `adcee5a`/`8eb1863`/`8e9df64`
(same fix, mesh/imagination/collisions), `dd56214` (EXP-023, collisions).
**Push status:** all pushed.
**Remaining defects.** The positional blind spot EXP-023 found is unexplained: why the window
scheme, which has no periodic blind spot, still concentrates its misses at particular positions.
The pre-existing `unused variable: shuffle` warning in `ranking_tradeoff.rs` is untouched.

---

## 2026-11-12 — bound the imagination pad's recovery from a deep lock-in

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`,
`crates/oag-imagination/examples/convergence.rs`, `spec/02-tensor-pad-routing.md`.

**Defect.** The pad's exploration floor was an absolute `1e-6`, which is meaningless against a
pad normalized to `PAD_MEAN` over `PAD_DIM` slots. A slot penalized without limit was multiplied by
`hop_failed_penalty` every decision and fell to denormal `f32` (about `1e-45`), where the constant
left it a share of about `1e-8` -- effectively frozen. Measured in `examples/convergence.rs`: a
switch at decision 10,000 took **1,061** decisions to recover, because the stored ratio between the
strongest and weakest routing slot was about `1e45` and the ratio decays by `exp(-HOP_STEP)` per
decision. Recovery time grew with the depth of the lock-in, unbounded.

**Fix.** `PAD_FLOOR_FRAC` (0.01) floors every routing slot at a fraction of the routing slots'
mean. Applied twice: in `route` (so any pad, including one loaded from the layer below, is
explorable) and in the stored pad after every reinforcement via `floor_routing_slots` (so the
*stored* ratio -- the thing recovery is measured against -- is bounded). The floor is relative to
the routing slots' own mean, not `PAD_MEAN`, because the pad is normalized over all `PAD_DIM`
slots and the first `neighbours.len()` of them can sum to more or less than `neighbours.len()`.

**Measurement.** The same switch now recovers in **69** decisions, matching the predicted bound
`ln(neighbours / PAD_FLOOR_FRAC) / HOP_STEP` ~ 60 for four neighbours. The cost is that a
neighbour the pad has learned to avoid keeps `PAD_FLOOR_FRAC / neighbours` of the traffic; the A/B
control still separates cleanly (signal 99.1% on the answering neighbour vs control 59.3% on an
arbitrary one).

**Tests executed.** `cargo test -p oag-imagination`: 54 passed, 0 failed (was 51). New tests:
`a_penalized_neighbour_keeps_a_bounded_share`, `the_stored_pad_keeps_a_bounded_ratio`,
`recovery_from_a_deep_lock_in_is_bounded_by_the_floor`.

**Commit identifiers:** pending.
**Push status:** pending.
**Remaining defects.** None known. The floor's cost in exploration traffic is a tuning
question, not a defect; `PAD_FLOOR_FRAC` is a named constant for that reason.

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

---

## 2026-11-09 — per-store heads in the mesh anti-entropy pull

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/transport.rs`, `spec/10-anti-entropy.md`,
`OPEN_QUESTIONS.md`.

**Feature implemented.** `pull_and_merge` recorded a peer's `StoreHeads` only when *every*
attempted store pull succeeded. With one persistently failing store that condition never became
true, so no heads were ever recorded and all five stores were re-pulled in full on every tick,
forever. Heads are now recorded per store: a failed store keeps its previous version, or
`UNKNOWN_VERSION` (`u64::MAX`, which no monotonic sequence reports) when there is none, so it is
retried while the healthy stores are not.

**Tests executed.** `cargo test -p oag-mesh`: 190 passed, 0 failed (was 188). New tests:
`a_failing_store_is_retried_without_re_pulling_the_healthy_ones` (integration -- the failing
endpoint is fetched on both ticks, the healthy one on the first only),
`merge_heads_records_each_store_independently` (unit).

**Performance measurements.** Not benchmarked, but the effect is a full five-store snapshot fetch
becoming a single failing-store fetch on each tick once one store is persistently broken. The
healthy case is unchanged.

**Commit identifiers:** `04869e5` (`oag-mesh`).
**Push status:** pushed to `origin/main`.
**Remaining defects.** The in-memory merges across a pull are still not atomic, and `save_to_dir`
remains the embedder's call. Found while re-reading the open question: **no code in either
repository calls `save_to_dir`/`load_from_dir`** -- `open-agent-graph` does not depend on
`oag-mesh` yet, so the persistence layer is correct and tested but unreachable in production.
Recorded in `oag-mesh/OPEN_QUESTIONS.md`.

---

## 2026-11-09 — the tensor pad can form pathways, and its adaptation is measured

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md` (mission-wide records live in `open-agent-graph`).

**Feature implemented.** `Engine::with_source` seeds the pad to one slot per neighbour, filling
missing slots with the mean of the existing ones. The pad and the neighbour list come from different
layers and nothing reconciled them, so a neighbour past the end of the pad could be routed to (the
`1e-6` floor keeps it reachable) but never reinforced, because `reinforce` had no slot to add to.
That deadlock meant the pad could strengthen a path and let others lose share but could never *form*
one.

**Tests executed.** `cargo test -p oag-imagination`: 47 passed, 0 failed (was 45). New tests:
`reinforcing_a_neighbour_shifts_routing_toward_it` (the pad's central claim, measured as a
distribution over 256 fixed seeds -- `route` is deterministic given pad/neighbours/seed, so it is
an exact measurement, not a sample),
`a_neighbour_beyond_the_pad_is_explorable_not_frozen`.

**Performance measurements.** None; the change is a `Vec::resize` at engine construction.

**Commit identifiers:** `48c619a` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** `reinforce` only adds, so the pad cannot weaken or dissolve a pathway: a
path that stops carrying traffic keeps its absolute weight forever. Recorded in `spec/02` and in
`RESEARCH_JOURNAL.md` as a proposed design (decay) with the measurement that would justify it,
rather than implemented on speculation.

---

## 2026-11-09 — the tensor pad is normalized, and a zero slot is seeded

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md` (mission-wide records live in `open-agent-graph`).

**Feature implemented.** Pads are normalized to a mean of `PAD_MEAN` (1.0) at construction and
after every reinforcement. Routing is unchanged -- it reads weight ratios only -- but the pad is no
longer traffic-volume-weighted where it is published and where `combine_pads` adds two pads
element-wise. A slot that is missing *or zero* is seeded to the mean of the slots carrying
information; a zero slot was the same frozen-neighbour trap as a missing one, reachable from a pad
that came back from the layer below with never-used slots.

**Tests executed.** `cargo test -p oag-imagination`: 49 passed, 0 failed (was 47). New tests:
`two_nodes_with_the_same_preference_produce_the_same_pad`, `a_zero_slot_is_seeded_like_a_missing_one`;
`a_neighbour_beyond_the_pad_is_explorable_not_frozen` updated for normalization.

**Performance measurements.** None; the added work is one O(pad length) pass per routing decision,
and the pad is small.

**Commit identifiers:** `239b49c` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** The pad cannot forget: a path reinforced once keeps its weight forever, so a
topology change dilutes it rather than removing it. Restated precisely in `RESEARCH_JOURNAL.md`,
which also corrects the previous entry's over-claim.

---

## 2026-11-09 — bound every wait on the imagination TCP carrier

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`, `spec/03-transports.md`.

**Feature implemented.** `INBOUND_READ_TIMEOUT` (30s) closes a connection whose peer completes the
handshake and then sends nothing; `UPSTREAM_TIMEOUT` (30s) bounds the connect to the next hop and
the wait for its reply. Before these, a stalling client held a spawned task and a socket for as long
as it liked, and a black-holed next hop held the task for the OS connect timeout (minutes).
`serve_on_with_timeouts` exposes the values so a deployment can retune them and a test can make
them short; `serve`/`serve_on` keep the defaults.

**Tests executed.** `cargo test -p oag-imagination`: 50 passed, 0 failed (was 49). New test:
`a_client_that_sends_nothing_is_dropped_at_the_inbound_timeout` (50ms timeout, 2s assertion bound).

**Performance measurements.** None; a timeout wrapper per read.

**Commit identifiers:** `c7f64db`.
**Push status:** pushed to `origin/master`.
**Remaining defects.** Nothing capped concurrent connections, so many slow clients could still
exhaust tasks and sockets even though each was finite. Closed in `0d21ee0`; recorded in
`spec/03-transports.md` rather than left implied.

---

## 2026-11-09 — cap concurrent connections on the imagination carrier

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/socket.rs`,
`crates/oag-imagination/src/lib.rs`, `spec/03-transports.md`.

**Feature implemented.** `DEFAULT_MAX_CONNECTIONS` (1024) bounds how many connections the TCP
carrier serves at once, via a semaphore acquired *before* `accept`, so a full server stops taking
new connections rather than queueing them. `Limits` replaces the `Timeouts` struct from the previous
commit (the cap is not a timeout); `serve_on_with_limits` exposes it, `serve`/`serve_on` keep the
defaults. Closes the remaining defect the previous entry named.

**Tests executed.** `cargo test -p oag-imagination`: 51 passed, 0 failed (was 50). New test:
`a_full_server_does_not_serve_a_second_connection_concurrently`.

**Performance measurements.** None.

**Commit identifiers:** `0d21ee0`.
**Push status:** pushed to `origin/master`.
**Remaining defects.** None known for this surface. The UDP carrier has no equivalent cap, and
`serve_udp` is documented as single-datagram-in/single-datagram-out, so it is a different shape.

---

## 2026-11-09 — the crawler pins its side-fetches too

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-crawler/src/crawler.rs`, `crates/oag-crawler/src/tests.rs`,
`spec/29-ipfs-pinning.md`, `OPEN_QUESTIONS.md`.

**Feature implemented.** The `llms.txt`, ARD, and agent-card bodies are now pinned and their
`pinned_cid` recorded on the evidence they back, best-effort exactly like the main page's. Extracted
assertions clone that evidence, so they inherit the CID. The pin block was factored into
`pin_best_effort` so all four call sites share one failure posture.

**Why the deferred decision was reversed.** The recorded reasoning was that the side-fetches are
metadata-discovery mechanisms rather than primary content. But they are the *evidence* for every
relationship extracted from them, and the evidence row carries their URI and content hash. Citing
an assertion's backing content by URI while leaving the bytes unretrievable makes it unverifiable by
a third party -- the reason to record the evidence at all. The decision was right about the trade
and wrong about which side of it those files were on.

**Tests executed.** `cargo test -p oag-crawler`: 75 passed, 0 failed (was 74). The test that
encoded the old behaviour was rewritten as
`pinning_provider_cid_lands_on_the_page_and_every_side_fetch`: every evidence item carries the CID
and exactly four bodies are pinned (the page plus three side-fetches).

**Performance measurements.** Up to three extra pins per crawled page, each an HTTP call to the
pinning provider or a local add. Best-effort, so a pin failure costs a log line and nothing else.

**Commit identifiers:** `b8e1f71`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** None known. The crawl's side-fetch pins are sequential with the main page's,
so a slow pinning provider adds latency to a crawl; parallelising them would change the failure
ordering for no correctness gain.

---

## 2026-11-09 — the mesh gains a connection to the layer below

**Repository:** `oag-mesh`
**Files modified:** `crates/oag-mesh/src/node.rs` (new), `crates/oag-mesh/src/lib.rs`,
`crates/oag-mesh/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `spec/08-persistence.md`,
`OPEN_QUESTIONS.md`.

**Feature implemented.** `node::MeshNodeAdapter` builds a `MeshState` from an existing `oag`
deployment: identity from `<data-dir>/identity.key` (generated if absent, the same file `oag`
uses), peers from the base peer's own peer tables, mesh stores from `<data-dir>/mesh/`, and saves
them back. Base-project addresses are scheme-less and `oag-sync` prepends `http://`; the adapter
applies the same convention so the addresses are dialable.

**Why this matters.** The mesh had no code connecting it to the layer below. `MeshState::new` takes
an identity and a peer list, but nothing constructed them from a real deployment, and -- as found
two invocations ago -- nothing in either repository called `save_to_dir` or `load_from_dir`. The
commit marker and torn-set refusal had therefore never run outside a test. The adapter is the first
real caller and makes the mesh standable up next to an existing peer with no new configuration.

The identity is shared deliberately: a `BridgeDeclaration` signed by this node is then attributable
to a peer that already exists, and the mesh's peer id matches the id the base project uses for that
node. The adapter reads the base project's files and never writes them.

**Tests executed.** `cargo test -p oag-mesh`: 193 passed, 0 failed (was 190). New tests:
`the_adapter_loads_the_identity_and_peers_from_an_oag_data_dir`,
`an_address_that_already_has_a_scheme_is_left_alone`,
`save_then_load_keeps_the_mesh_state_and_leaves_the_base_files`.

**Performance measurements.** None; the adapter runs at startup and on an operator-triggered save.

**Commit identifiers:** `d4c52a7` (`oag-mesh`).
**Push status:** pushed to `origin/main`.
**Remaining defects.** `save_to_dir` still does not create its directory, so the adapter does; a
caller using `save_to_dir` directly still has to. The base project's identity is loaded or
generated, so a data directory without `identity.key` gets a fresh mesh identity -- correct for a
first run, but it means the mesh node and a later-created base peer would not share one.

---

## 2026-11-09 — measured what the tensor pad converges to

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/examples/convergence.rs` (new),
`spec/02-tensor-pad-routing.md`, `RESEARCH_JOURNAL.md`.

**Artifact.** `cargo run --example convergence [-- neighbours N] [-- decisions N] [--offset N]`
runs the real engine over a deterministic source (`BLAKE3(counter)`) with a resolver that never
answers, and prints the chosen-neighbour share at checkpoints. Deterministic, so a run is
reproducible and comparable.

**Result.** With 4 neighbours and 20,000 decisions, one neighbour takes ~96% of routing (0.568 at
1,000; 0.866 at 5,000; 0.933 at 10,000; 0.966 at 20,000). The pad mean stays exactly 1.0000, which
also confirms the normalization added earlier is stable under reinforce-then-renormalize.

**The control.** `--offset` shifts the source's start: offsets 0-3 crown `node-2`, offsets 4-5 crown
`node-3`, at the same ~96%. So the winner is decided by the early random draws, not by any property
of the neighbours.

**What it establishes.** `reinforce` adds a constant 1.0 to whichever hop was taken, regardless of
outcome, so the reinforcement carries no information about hop quality. The pad is a rich-get-richer
random walk that locks onto an arbitrary neighbour -- not learning. This corrects the previous
research entry's "bandit with no exploration term": a bandit's reward varies with the outcome, and
here it does not vary at all. It also reframe the decay question: decay would add forgetting without
adding signal, so the missing piece is a utility signal from the hop's outcome, which the TCP carrier
already observes and `Engine::handle` currently discards.

**Tests executed.** No test added; the artifact is the example, run by hand. `cargo test -p
oag-imagination` remains 51 passed, 0 failed.

**Performance measurements.** 20,000 decisions in ~0.47s (~42,000 decisions/sec), single-threaded,
debug build.

**Commit identifiers:** `f7ea069` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** The pad has no utility signal, so its documented "adaptation" is lock-in
rather than learning. Recorded as a proposed design in `RESEARCH_JOURNAL.md`, not implemented.

---

## 2026-11-09 — the pad learns from observed utility

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`,
`crates/oag-imagination/src/socket.rs`, `crates/oag-imagination/src/lib.rs`,
`crates/oag-imagination/examples/convergence.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md`.

**Feature implemented.** `Engine::handle` no longer reinforces the hop it chose. The TCP carrier
forwards the request, observes the outcome, and calls the new `Engine::observe_hop(neighbour,
HopOutcome)`: `Answered` multiplies the neighbour's weight by `1.05`, `Failed` by `0.90`, and
`Forwarded` leaves it alone, followed by normalization. Multiplicative rather than additive so a
penalty can never make the sum negative and invert the ordering.

**Why.** The convergence measurement showed the pad locking onto an arbitrary neighbour at ~96%
with the winner decided by the source offset: the constant reinforcement carried no information
about hop quality. The carrier is the only place that knows the outcome, and it was discarding it.

**Tests executed.** `cargo test -p oag-imagination`: 51 passed, 0 failed. The test that asserted
"the pad strengthened for the chosen hop" now asserts the opposite -- routing alone leaves the pad
untouched -- and that only `observe_hop` moves it. Workspace including Reticulum: 54 passed.

**Performance measurements.** `examples/convergence.rs`, 4 neighbours, 20,000 decisions: signal arm
converges to the answering neighbour at 99.8%, control arm (old constant reinforcement) to an
arbitrary one at 59.6%. Both arms share their first few hundred draws, so the difference is the
signal, not the luck. ~42,000 decisions/sec single-threaded debug build.

**Commit identifiers:** `d862492` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** The gain and penalty constants (`1.05`, `0.90`) are round numbers, not tuned;
a sweep is the obvious next measurement. Only the TCP carrier feeds back, because it is the only
carrier that forwards -- a UDP-served node keeps a pad that does not learn. And the reward is binary
and delayed by the whole downstream chain, so credit assignment across a multi-hop path is coarse.

---

## 2026-11-09 — one log-space step for the pad, and a measured recovery time

**Repository:** `oag-imagination-random-data-compute`
**Files modified:** `crates/oag-imagination/src/engine.rs`, `crates/oag-imagination/src/lib.rs`,
`crates/oag-imagination/examples/convergence.rs`, `spec/02-tensor-pad-routing.md`,
`RESEARCH_JOURNAL.md`.

**Feature implemented.** `HOP_ANSWERED_GAIN` (1.05) and `HOP_FAILED_PENALTY` (0.90) -- two
independent round numbers -- are replaced by one log-space step, `HOP_STEP = 0.1`, applied as
`exp(+HOP_STEP)` for an answered hop and `exp(-HOP_STEP)` for a failed one. The answered/failed
ratio per comparison is `exp(2 * HOP_STEP)` (~1.22) and the weight-ratio half-life is
`ln 2 / HOP_STEP` (~6.9 observations). The ratio is what carries meaning, and one step makes the
memory horizon derivable rather than tuned.

`examples/convergence.rs` gained `--switch-at N`, which changes the answering neighbour mid-run and
reports when the new neighbour's *weight* overtakes the old one's.

**Tests executed.** `cargo test -p oag-imagination`: 51 passed, 0 failed.

**Performance measurements.**
- A/B, 4 neighbours, 20,000 decisions, node-0 answering and the rest unreachable: signal arm
  99.9% on node-0, control arm (old constant reinforcement) 59.6% on an arbitrary node-2.
- Recovery: `--switch-at 10000` -- the new neighbour's weight overtakes the old one's **1,061
  decisions** later. That is ~150x the 6.9-observation half-life, and the gap is real: the
  half-life describes how fast a weight *ratio* moves per observation, but a deeply locked-in
  neighbour is selected almost every time, so its selection rate falls as its weight falls and the
  decay slows. Recovering from a ~1000:1 lock-in takes ~1,000 decisions, not ~66.

**Commit identifiers:** `00208cf` (`oag-imagination-random-data-compute`).
**Push status:** pushed to `origin/master`.
**Remaining defects.** Choosing `HOP_STEP` is now a stated trade with a measured number attached
(small step = noise-stable, slow to abandon a failing path; large step = the reverse) rather than two
unexplained constants. It is still a single global constant -- a per-neighbour or per-path step
would be the next refinement. And only the TCP carrier feeds outcomes back.

---

## 2026-11-09 — the chaos test measures its errors, and 429s say when to retry

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/tests/distributed_convergence.rs`,
`crates/oag-sync/src/rate_limit.rs`, `crates/oag-sync/src/tests.rs`,
`crates/oag-api/src/rate_limit.rs`, `spec/18-rate-limiting-and-resource-limits.md`,
`RESEARCH_JOURNAL.md`.

**What the ignored test surfaced.** `fifty_peers_converge_after_chaos` is `#[ignore]`d as slow and
had never been run in this session. It passes: 50 peers, 40 chaos rounds, 5 kills, 3 restarts, 5
partitions, 5 heals, 64 events converged, 0 timeouts. Its printed label for the error count --
"transient SQLite-contention errors" -- was an assumption: the test discarded the messages.

**Feature implemented.** The test now keeps the error strings, classifies them, and prints a
breakdown with a sample per category. Both rate limiters (sync's global and REST/MCP's per-key) now
send `Retry-After`, computed from the window they already track.

**Tests executed.** `cargo test -p oag-sync --test distributed_convergence -- --ignored`:
1 passed (50 peers, 77s). `cargo test -p oag-api --lib`: 64 passed (was 63; new
`a_rejected_key_is_told_when_to_retry`). `cargo test -p oag-sync --lib`: 41 passed.

**Performance measurements.** One chaos run: 4,565 recoverable errors, of which **2,594 were 429s
from the limiter itself**, 1,971 SQLite lock contention (`database is locked`, codes 5 and 517),
105 event fetch failures. So the old label was ~43% right. The count varies between runs (2,486 in
an earlier run) because it is timing-dependent. None of it blocked convergence.

**Commit identifiers:** `cb3d6f4`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** `SQLITE_BUSY_SNAPSHOT` (517) is not helped by `busy_timeout` -- WAL mode
returns it immediately when a read transaction cannot upgrade to a write while another connection
holds the writer. A single-writer queue would remove it and is not built. No sync client honours
`Retry-After` yet either; the limiter now states it and the client still treats a 429 as a generic
failure retried on the next cycle.

---

## 2026-11-09 — the sync client consumes Retry-After and the gossip loop backs off

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-sync/src/client.rs`, `crates/oag-sync/src/service.rs`,
`crates/oag-sync/src/gossip.rs`, `crates/oag-sync/src/tests.rs`, `spec/18-rate-limiting-and-resource-limits.md`,
`RESEARCH_JOURNAL.md`.

**Feature implemented.** `SyncClient` now turns a 429 into `SyncClientError::RateLimited {
retry_after }` instead of letting `error_for_status` flatten it into a generic failure, carrying the
peer's `Retry-After` (or a 30s default when absent). `SyncService` records a per-address deadline
from it -- keeping the later of two deadlines -- and `gossip::sync_round` skips any address still
inside that window.

**Why.** The previous entry made both limiters *send* `Retry-After`; nothing consumed it. A client
that cannot tell "rate limited, retry at T" from "peer down" has to guess, and the guessing is what
produced 2,594 retries against a limiter that had already answered.

**Tests executed.** `cargo test -p oag-sync --lib`: 43 passed, 0 failed (was 41). New:
`a_rate_limited_peer_is_distinct_from_a_dead_one_and_is_backed_off` (the 429 is distinct and
carries a non-zero `Retry-After`; the service records the backoff),
`sync_round_leaves_a_peer_in_its_rate_limit_backoff_window_alone` (a dead address increments the
shared error counter; a backed-off one does not).

**Performance measurements.** None added. The chaos test still drives `sync_with_peer` directly
and so still exercises the limiter rather than the backoff -- stated in `spec/18` rather than left
for a reader to assume the loop was measured.

**Commit identifiers:** `5772e11`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** A 429 on `fetch_events` is recorded as a backoff *and* counted as a
per-event fetch failure, so it is double-counted. `Retry-After` is honoured by the sync client only;
`oag-api`'s own outgoing calls (the gateway, the crawler) do not consume it either.

---

## 2026-11-09 — full verification pass, and the SQLITE_BUSY_SNAPSHOT question recorded not guessed

**Repositories:** all three.

**Tests executed.**
- `cargo test --workspace -j 2` in `open-agent-graph`: **445 passed, 0 failed**.
- `cargo test -j 2` in `oag-mesh`: **193 passed, 0 failed**.
- `cargo test --workspace -j 2` in `oag-imagination-random-data-compute`: **54 passed, 0 failed**
  (includes the Reticulum carrier).
- `oag-collisions`: **29 passed, 0 failed**.

**Why a full pass.** This closed a long run of increments (keyset pagination, persisted libp2p
identity, content-based source independence, side-fetch pinning, per-store sync heads, the mesh node
adapter, the pad's utility signal and step, rate-limit measurement and backoff) across three
repositories. A full pass after that many changes is the check that they compose.

**Commit identifiers:** none; verification only.
**Push status:** n/a.

**SQLITE_BUSY_SNAPSHOT (517), recorded rather than guessed.** The chaos test reports 1,971
`database is locked` errors, codes 5 and 517. `busy_timeout(5s)` is set, and the usual cause of
517 -- a deferred transaction that reads and then upgrades to a write -- is *not* present: the
storage layer opens no explicit transactions and issues no `RETURNING`. So the trigger is not
identified, and the three candidate explanations (sqlx holding a read snapshot across statements on
a pooled connection, the FTS external-content triggers reading the index before writing, or the
interaction of 50 peers' worth of concurrent writers) are untested. **Status: open question, not a
fix.** A single-writer queue is the standard remedy for 517 but would serialise every write in the
process; committing to it without knowing the cause and without measuring the write-throughput cost
would be exactly the kind of uninformed change this session has been correcting.

---

## 2026-11-09 — BEGIN IMMEDIATE: the SQLite lock contention goes to zero

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-events/src/ingest.rs`, `crates/oag-events/src/commit.rs`,
`crates/oag-graph/src/redaction.rs` (two sites), `crates/oag-graph/src/service.rs`,
`crates/oag-storage/src/tests.rs`, `crates/oag-events/src/tests.rs`,
`crates/oag-sync/tests/distributed_convergence.rs`, `ARCHITECTURE_DECISIONS.md`,
`RESEARCH_JOURNAL.md`.

**Root cause found, not guessed.** The previous entry recorded `SQLITE_BUSY_SNAPSHOT` as an open
question because the usual cause -- a deferred transaction reading then writing -- appeared absent.
It was present; the earlier grep searched `oag-storage` and `oag-sync` and the transactions live in
`oag-events` and `oag-graph`. Four read-then-write sites used `pool.begin()`, which takes the write
lock only at the first write, so a concurrent commit left the reader unable to upgrade.

**Feature implemented.** All four use `pool.begin_with("BEGIN IMMEDIATE")`. `rebuild` keeps a
deferred begin: its first statement is a write, so it never upgrades, and holding the write lock
across a whole rebuild would cost more.

**Tests executed.**
- `cargo test -p oag-storage`: 36 passed (was 35). New
  `begin_immediate_removes_the_busy_snapshot_failures_a_deferred_begin_causes` -- an A/B at 96
  concurrent writers: deferred 35x code-5 + 32x code-517, immediate 0 failures.
- `cargo test -p oag-events --lib`: 22 passed. `commit_local_event_itself_is_not_safe_for_concurrent_same_peer_calls`
  **reversed**: it asserted the commits must race, and its own message said to rerun before
  concluding if that changed. Reran three times, then reversed -- it now asserts 0 errors and
  contiguous sequences 1..=20.
- `cargo test -p oag-graph`, `-p oag-sync`: 101 and 43 passed.
- `cargo test -p oag-sync --test distributed_convergence -- --ignored`: 1 passed.

**Performance measurements.** The chaos run before this change: 4,565 recoverable errors (2,594
rate-limit 429s, 1,971 SQLite locked, 105 fetch failures). After: **635 errors, all of them the rate
limiter; `sqlite locked` 0** (was 1,971). Counts are timing-dependent between runs, so the
comparison is the same test on the same seed before and after, not two runs after.

**Commit identifiers:** `419c7bc`.
**Push status:** pushed to `origin/main`.
**Remaining defects.** `GraphService::commit_lock` is no longer load-bearing for 517; kept as
defence in depth and documented as such. `rebuild`'s deferred begin is correct only because its
first statement writes -- a reorder would reintroduce the hazard, and there is no test pinning that
ordering.

---

## 2026-11-09 — rebuild takes BEGIN IMMEDIATE too, and every transaction is now uniform

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-graph/src/rebuild.rs`, `crates/oag-graph/src/tests.rs`,
`ARCHITECTURE_DECISIONS.md`.

**Feature implemented.** `rebuild_projection` opens `BEGIN IMMEDIATE`. ADR-015 exempted it on the
reasoning that its first statement writes, so the write lock was taken immediately anyway and the
exemption cost nothing -- which was right about the cost and wrong about the risk: correctness
rested on `clear_projection_tables` staying first, and nothing tested that. A deferred transaction
that writes holds the lock until commit regardless, so taking it up front costs nothing here either.

**Tests executed.** `cargo test -p oag-graph`: 102 passed, 0 failed (was 101). New
`a_rebuild_concurrent_with_local_commits_produces_no_locking_error`: 200 seeded events, a rebuild
running concurrently with 32 commits through a *second* service handle (its own commit lock, so the
serialisation is the database's, not the application's). Both must succeed. `oag-events` 22,
`oag-storage` 36.

**Performance measurements.** None; the change is which statement acquires the write lock.

**Commit identifiers:** `2bc1537` (code and test), `d791a84` (ADR addendum).
**Push status:** pushed to `origin/main`.
**Remaining defects.** The new test asserts the property, not the overlap: the rebuild may finish
before the commits start. It would not have caught the original deferred-begin bug either, because
that bug needed a read *before* the first write -- which is exactly the ordering the change removes.

---

## 2026-11-09 — the relay chain of custody exists and is now tested; spec/22 corrected

**Repository:** `open-agent-graph` (`rust-ai`)
**Files modified:** `crates/oag-api/src/transfer.rs`, `spec/22-future-work.md`,
`RESEARCH_JOURNAL.md`.

**Finding.** `spec/22-future-work.md` claimed "No `hop_count`/`via_peer` field or multi-hop
relay-with-witness structure exists anywhere today." That is stale twice over:
`UnsignedTransferRequest` has `relay_hops_remaining` and `relay_path`, `UnsignedGatewayRequest` has
`hops_remaining` and `path`, each hop appends itself and refuses to forward to a peer already in the
chain, and the server records `relayed_via` on its evidence. The existing relay test exercises the
chain but never asserts the path.

**Feature implemented.** `a_relay_refuses_to_forward_to_a_peer_already_in_the_chain` -- a unit test
of `relay_fetch` with a dead relay address, so a `None` can only come from the path check. No network,
no timing.

**Tests executed.** `cargo test -p oag-api`: 65 passed, 0 failed (was 64).

**Commit identifiers:** pending.
**Push status:** pending.
**Remaining defects.** An attempt to write an *integration* test for a cyclic relay configuration
(two peers relaying to each other, neither holding the content) hung past a 20s bound. The path check
is not the cause -- the unit test above proves it refuses. The hang is undiagnosed and is recorded
here rather than claimed as a defect; the prime suspect is `EmbeddedIpfsProvider::fetch`, which has
no timeout and may block on a CID nobody can serve. That is a separate question and is not asserted.
