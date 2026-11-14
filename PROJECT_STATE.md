# PROJECT_STATE — OAG mission state

Authoritative current state of the whole Project OAG mission across its three repositories. This
file describes *where the mission stands*. The reasoning behind individual design decisions lives
in each repository's own `OPEN_QUESTIONS.md` and `spec/`; raw user prompts live in each repo's
`USER_INPUT_RECORD.md` where one exists.

Last updated: 2026-11-09.

## Repositories

| Layer | Repository | Local path | Role |
|---|---|---|---|
| 0 | `open-agent-graph` | `/home/webdawg/DATA/CODE/rust-ai` | one peer's signed event graph |
| 1 | `oag-mesh` | `/home/webdawg/DATA/CODE/oag-mesh` | a mesh of peers — clusters, federated trust, onion routing |
| 2 | `oag-collisions` | `/home/webdawg/DATA/CODE/oag-collisions` | inference by collision -- a similarity mechanism built on locality-sensitive hashing |
| 1 (parallel) | `oag-imagination-random-data-compute` | `/home/webdawg/DATA/CODE/oag-imagination-random-data-compute` | synthetic data — dreams from verifiable randomness |

These state files live in `rust-ai` because it is the foundational layer, but they describe the
mission as a whole.

## Current status (measured)

| Repository | Tests | Commits | Specs |
|---|---|---|---|
| `open-agent-graph` | 435 | 123 | 01–32 |
| `oag-mesh` | 188 | 43 | 01–15 |
| `oag-imagination-random-data-compute` | 48 | 31 | 01–03 |

All three working trees are clean and pushed as of the last update.

## Completed capabilities

### open-agent-graph (layer 0)
- Signed append-only event log, rebuildable projections, embedded SQLite, Ed25519 peer identity.
- Assertions / evidence / disputes / retractions / verifications / observations / supersessions;
  per-artifact schema versioning with a read shim and an explicit migrate-in-place action.
- Corroboration and authority ranking; gravity (processing throttle, not pad decay); tensor pads.
- REST + MCP + Human Interface (askama-templated, autoescaped) surfaces.
- Crawler with deterministic extractors (HTML meta, JSON-LD, markdown, `llms.txt`, ARD, A2A),
  SSRF/robots policy, LLM extraction fallback, IPFS pinning of the main page.
- Semantic search over pluggable embeddings (brute-force cosine; no mandatory vector DB).
- Replication: pull-based gossip over HTTP and over Reticulum; ephemeral peer identity with
  behaviour-driven trust; peer clustering with a faster intra-cluster loop; long-word DNS
  addressing.
- Service receipts and multi-hop relay; verified internet gateway (NTP + HTTP, quorum fan-out).
- Native IPFS: evidence pinning, `oag backup --pin`, `oag ipfs cat|pin|host-page`, the CID
  discovery query (`GET /api/v1/pins` + `graph_pins`) backed by a `pinned_cids` table that
  records every pin path (evidence, cli-pin, backup-pin, host-page), and
  `/ipfs-site/{cid}` static page hosting with a vendored gateway-free Helia bundle.
- Observability: hand-rolled Prometheus `/metrics` and structured `tracing` spans for REST, MCP,
  and event ingest. Metrics now include `crawls_total`/`crawls_failed`,
  `peer_syncs_total`/`peer_sync_errors`/`peer_sync_peers`,
  `verification_jobs`/`verification_failures`, `events_pending_chain` (events whose predecessor
  is missing locally -- the one metric that tells a complete chain from a pile of fragments), and
  `replication_bytes_in`/`out` (bytes moved from/to peers during replication, counted on the
  real gossip loop's client), and the two duration histograms `search_latency` (inside
  `GraphService`) and `api_latency` (an HTTP middleware).

### oag-mesh (layer 1)
- Cluster nesting (`BridgeDeclaration`) with escalating proof-of-work anti-sybil protection.
- Anonymous events: identity-stripped schema, escalating PoW anti-spam/anti-replay, search by
  kind/severity/location/recency, `AllClear` resolution, prune.
- Federated corroboration weighted by distinct cluster, not distinct peer; half- and
  fully-cross-checked best-value views.
- Cluster membership attestation (signed, distinct-attester corroborated, escalating PoW,
  per-attester rate limit).
- Responder acknowledgment (advisory, identity-bearing, no PoW).
- Blinded responder dispatch: filter subscription under a responder-chosen random id, polled inbox.
- Onion routing: ChaCha20-Poly1305 layers over X25519 keys derived from Ed25519 identities.
  The onion endpoint is volume-bounded by a node-wide token bucket — it is the one endpoint
  that makes a node send traffic to a caller-chosen address, so its bandwidth needed a bound.
- Transport: HTTP push + pull-based anti-entropy, optional periodic sync with jitter, disk
  persistence of all seven stores where each save writes a commit marker last and a load refuses
  a torn set.
- Adaptive ordering from a per-peer link-quality ledger (an exponential moving average of
  whether each interaction actually delivered, fed by route-search replies and `pull_and_merge`
  outcomes). It orders which peers a route search asks first, so a scarce fan-out budget is not
  spent on peers that have been dead for hours -- and it orders the sequential anti-entropy pull
  loop, so live peers' data arrives before dead peers' timeouts are paid. It orders and never
  excludes; in-memory only; a liveness signal, not a trust signal.

### oag-imagination-random-data-compute (layer 1, parallel)
- Pad-guided routing engine: a request enters a node, the tensor pad picks the next hop, each hop
  adds a contribution, the final response carries the whole chain back.
- Data pads: a file split into shards across many pads, gathered and reconstructed (not erasure
  coded; a missing shard is an error).
- Randomness sources: software, mixed (sometimes real data), hardware seam, and a byte-device
  source (`/dev/hwrng` or any device a driver exposes).
- Carriers: TCP, UDP, HTTP, HTTP `CONNECT` tunnel, IPFS read-by-CID and publish, and Reticulum
  (in a separate `oag-imagination-reticulum` crate, so the layer keeps no `reticulum` dependency).
- Target resolvers: local/store, URL, CID-over-gateway, peer transfer with receipt verification.
- The node adapter reads its pad and peers from `oag`'s own storage; nothing is configured.

## Standing decisions

- **Factual and synthetic are separate layers.** `oag` records evidence about what is known;
  `oag-imagination` generates possibilities and must never be represented as evidence. Enforced by
  repository separation and by the imagination layer having no writer into `oag`'s graph.
- **Small hand-rolled implementations over heavyweight crates** when the surface is narrow
  (Prometheus text, IPFS HTTP client, embeddings HTTP client, onion encryption). Heavy crates are
  used where the surface is broad (`rust-ipfs` for the embedded node, `reticulum` for the mesh).
- **No mandatory external infrastructure.** No Postgres/Redis/Kafka/Kubernetes; embedded SQLite;
  IPFS is opt-in; the gateway is opt-in.
- **Proof-of-work bounds cost, not identity.** It raises the price of sybil/spam from free to real
  and does not stop a well-resourced attacker. Stated plainly in every PoW-bearing mechanism.
- **"Degrade, don't drop."** Trust is a continuum with one hard floor; nothing else is hard-excluded
  from federation regardless of how low its nonzero trust gets.
- **The event log is never deleted or rewritten.** Redaction is a local tombstone, not a mutation.
- **Metrics are process-lifetime counters where a snapshot query structurally cannot produce the
  number.** Rejected verification attempts never become events, so `verification_jobs`/
  `verification_failures` are real instrumentation, not a `COUNT(*)`.

## Known limitations (open, tracked per-repo)

See `OPEN_QUESTIONS.md` in `rust-ai` and `oag-mesh`, and the spec files in all three. The largest:

- **No automatic peer/content discovery or routing.** Relay topology, gateway peers, and mesh
  peering are operator-configured. A real discovery mechanism is unbuilt everywhere.
- **No witness-based audit / trust consequence.** Redundant multi-path delivery plus content-hash
  self-verification resolves the *design* question, but the serving-announcement evidence shape,
  hop-chain shape, and witness payload variant are undesigned.
- **Flood/broadcast propagation with traffic-analysis resistance is unbuilt and undesigned**
  (bandwidth tension with pull-based replication; "never-ending morphing encryption" open).
- **Sub-second ephemeral micro-networks** (<30ms) and "network formation as an identity claim" are
  genuinely open; they need new spec work.
- **Mesh: no responder dispatch push, no `AllClear` semantics decision, poll-only acknowledgments,
  unauthenticated GETs, untuned PoW, no fsync (a power loss can still lose a rename), and no
  atomic multi-store *write* during a pull.** The read-side torn-set gap is closed: a saved set
  carries a commit marker and a load refuses a set that contradicts it. Every outbound request is
  bounded by a connect and a total-request timeout.
- **The base project's external-service clients are still unbounded in time.** `oag-sync`'s own
  client is now bounded (`b2e2182`), but the operator-configured LLM extractor, embeddings, and
  IPFS providers still build their clients without timeouts. They are a lower trust tier -- each
  requires an operator to enable it -- and are recorded in `spec/18` rather than fixed silently.
- **`blob_store_size` metric is unbuilt** because no blob store exists (`spec/29`'s `blobs/<hash>`
  is itself unimplemented).

## Completed recently

- **The pruner no longer races cargo.** The prune hook could delete the
  `incremental/`/`.fingerprint/`/`build/` directories cargo had created for the units it was about
  to compile, failing the build with `failed to write .../invoked.timestamp`. The wrapper now
  passes the cargo process's own start time as `--protect-after` and the pruner skips anything
  newer. 16 script tests pass (was 14); `cargo build --examples` is clean across repeated runs.
- **The mesh's heads-failure fallback is classified.** A pull whose heads request fails used to
  fall back to a full five-store pull regardless of why. For an unreachable peer that is six
  timeouts of pure waste (1.8s measured); `PullError::Request` with `is_connect()` or
  `is_timeout()` now returns after one, while a `Rejected` (an HTTP status -- a peer whose heads
  endpoint is broken but whose stores serve) still pays the fallback. Both halves are tested.
  `oag-mesh` 196 passed (was 195).
- **The mesh's anti-entropy tick is bounded.** Each pull was bounded but a tick is a loop over
  every peer, so unreachable peers cost their timeout each -- and a pull against a black-holed peer
  costs the heads request plus the five-store fallback (six timeouts, 1.8s measured). The live
  peers' next refresh waited for the dead ones. `sync_with_all_peers_bounded` and
  `spawn_periodic_sync_with_budget` cap the tick; the skipped peers are retried, so "never not
  asked" still holds. `oag-mesh` 194 passed (was 193).
- **EXP-027: the collision recall is base-dependent, and the multi-probe stabilises it.** A
  near-duplicate's flipped bits are the base document's near-zero majority-vote positions, which
  are essentially random per document. Across 24 bases the standard ladder recalled 21%-96% of the
  same 32-token edits (median 50%); the 1-flip multi-probe raised that to 67%-100% and the 2-flip
  to 92%-100% with a standard deviation of 0.7 variants. A deployment should measure the recall
  per corpus and choose the probe budget by how predictable it must be.
- **EXP-026 (negative result): a second independent hash is not worth its memory.** Two hashes of
  one corpus recalled 73% and 22%; their union 76%. The misses are roughly independent, so the
  gain is bounded by the better hash.
- **EXP-025: the collision index's recall is widened by a 1-flip multi-probe.** The prefix filter
  excluded 61% of variants and was steepest for the far near-duplicates. `collide_multi_probe`
  also queries the prefixes differing from the query's in a few high-order bits. With one flip it
  nearly doubles recall (38.8% → 73.8%) for 8 extra probes, at 14 near-duplicates per unrelated
  document admitted, and it recovers **every** variant at the 2-, 4- and 8-token edit distances.
  Two tests pin the contract: zero flips is the standard ladder, and extra probes never narrow the
  candidate set.
- **EXP-024: the collision index now ranks by Hamming distance, not by first differing bit.** The
  index ranked by the longest shared prefix, which is a weak proxy for the edit size: Spearman
  +0.916 between edit distance and Hamming distance, but only -0.443 between edit distance and
  shared prefix. The prefix ordering inverted 22.3% of comparable pairs (29.7% among the
  candidates). `collide_by_hamming` re-ranks the same candidates by the full-hash distance, which
  the index already stored and was not using, at `O(candidates)` and no extra memory. The
  candidate set is unchanged -- the prefix filter still decides recall.
- **EXP-023: collision detection measured as a distribution, not a peak.** The detection rate is a
  step function of the window stride, not the budget: budgets sharing a stride give identical rates
  (spread 0.0). The window scheme has a periodic blind spot of `(stride - window - 1)/stride` -- the
  gap residues are missed on 100% of seeds -- which is zero only when `stride <= window + 1` (at 800
  tokens and window 3, a budget of at least 200). Most detections are thin-margin (62.2% move 1-2
  bits at budget 200), and the margin is set by the edit, not the budget: 1.12 bits for a 1-token
  edit, 4.06 for a 16-token edit, with the 11+ bucket essentially unreachable. A high detection rate
  does not mean a confident detection. Recorded in `oag-collisions/EXPERIMENTS.md`, with a
  correction pointer on EXP-016's "no blind spot" claim.
- **The imagination pad's recovery from a deep lock-in is bounded.** The exploration floor was an
  absolute `1e-6`, meaningless against a pad normalized to `PAD_MEAN`; a penalized slot fell to
  denormal `f32` (~1e-45) and recovery took 1,061 measured decisions, growing with lock-in depth.
  `PAD_FLOOR_FRAC` (0.01) floors the routing slots relative to their own mean, in `route` and in
  the stored pad. The same switch now recovers in 69 decisions, matching the predicted
  `ln(neighbours / PAD_FLOOR_FRAC) / HOP_STEP` bound. `oag-imagination` spec/02.
- **Keyset pagination** (`cursor` / `--cursor`) on REST, MCP, and CLI. The cursor names the last
  row's `node_id`, not its rank, because FTS5's bm25 rank moves with the corpus.
- **The embedded IPFS node's libp2p identity is persisted** (`<repo>/libp2p.key`), so a restart
  keeps the same dial address. ADR-013.
- **Every multi-statement transaction takes `BEGIN IMMEDIATE`.** The deferred begin was the cause
  of 1,971 `SQLITE_BUSY_SNAPSHOT` errors in the 50-peer chaos run; that count is now **0**. ADR-015.
- **Source independence accounts for mirrored content** -- one article on several domains is one
  source, not several. ADR-014.
- **The crawler pins its side-fetches** (`llms.txt`, ARD, agent card), so the evidence backing
  extracted assertions is retrievable rather than cited by URI alone.

## Next priorities

1. **Collisions: choose the default probe budget.** EXP-025 and EXP-027 together show the
   1-flip and 2-flip schemes are a stated trade -- 8 probes / 3.1% unrelated / 67-100% recall
   versus 36 probes / 13.8% unrelated / 92-100% recall with a standard deviation of 0.7. The API
   leaves the choice to the caller, which is correct, but nothing in the repository records which
   budget a deployment should start from. The open question is a real corpus: every measurement so
   far is on one synthetic document shape, and EXP-027 showed the recall varies 21-96% across
   documents even at a fixed edit model.
2. **Mesh: the fallback classification's blind spot.** The classification is on
   `reqwest::Error::is_connect()` / `is_timeout()`. A slow-but-alive peer whose heads request times
   out will skip the fallback; its stores would likely time out too, so the skip is defensible --
   but it is a judgement, not a measurement. The open question is whether a timed-out heads request
   should still try one store (cheap) before giving up, which would need a measurement of how
   often a timed-out heads endpoint still serves stores.
3. **Mesh: no atomic multi-store *write* during a pull.** The commit marker closed the read side;
   a pull that updates several stores is still best-effort per store. Re-examine against
   `persistence.rs` first: `save_to_dir` already writes all seven stores plus a commit marker
   atomically, so the gap may be narrower than it looks -- the in-memory merge is per-store and a
   crash loses it, but that state is re-learned from the mesh.
4. **rust-ai static page hosting follow-ups** — scheduled backup pinning and evidence-body
   inlining remain (see `spec/29` "Explicitly deferred"); crawler side-fetch pinning is done.
5. **Mesh: `spec/06` (owner-controlled research data) and `spec/07` (object history/audit)** —
   proposed, not implemented; both are large and need a design pass before code.

## Verification

Last full pass across all three repositories: `open-agent-graph` 448 passed, `oag-mesh` 193,
`oag-imagination-random-data-compute` 54 (including the Reticulum carrier), `oag-collisions` 29 --
724 in total, 0 failed anywhere.

## How to work here

- Every invocation starts from this file plus the per-repo `OPEN_QUESTIONS.md` and recent git log.
- Prefer one coherent, independently verifiable increment over a speculative rewrite.
- Run small, independent tests rather than whole-workspace suites when the local machine is busy.
- Commit and push after every verified contribution; never rewrite published history.
