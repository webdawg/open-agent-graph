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

- **oag-listend caps the item bound by the frame cap.** `--max-bytes` defaults to 1 GiB, but no
  carrier can move an item near that: a `Fetch` response at the frame cap would itself exceed the
  frame cap and be rejected by the client's decoder. `servable_bound` caps the store bound by
  `MAX_SERVABLE_BYTES`, and both `accept` and the directory load use it, so no accepted item is
  unfetchable. 41 unit + 16 end-to-end tests.
- **oag-listend rejects MP4 video by brand, not by major brand alone.** The sniff accepted any
  ISO BMFF file whose major brand was `isom` or `mp42` -- generic brands MP4 *video* uses -- so
  video was accepted as audio. It now reads the whole `ftyp` brand list and accepts only an audio
  brand (`M4A `/`M4B `/`M4P `), which also keeps ffmpeg-written M4A (`isom` major, `M4A `
  compatible). `ftyp` size 0 and the 64-bit largesize are handled. 40 unit + 16 end-to-end tests;
  verified by hand (`isom`/`avc1` → 400, `isom`+`M4A ` → 201, `M4A ` → 201).
- **oag-listend reports store byte usage and matches validator lists.** `GET /` and the startup
  line report bytes held against the bound, so an operator can see the bound working. And
  `If-None-Match` with a comma-separated validator list (what a shared cache sends) now matches,
  instead of being compared as one string and never revalidating. 38 unit + 16 end-to-end tests;
  verified by hand (`held 4976`, `bytes: 4976 / 1073741824`, list validator → 304).
- **oag-listend honours a zero listing limit and says when a bind is public.** `limit=0` was
  clamped to 1, so a client asking for the default got one item; `normalize_limit` now resolves
  zero to `DEFAULT_LIST_LIMIT` (capped by the carrier). And because the default bind is `0.0.0.0`
  (every interface), a non-loopback bind now prints a note rather than leaving the operator to
  discover it. 38 unit + 15 end-to-end tests; verified by hand.
- **oag-listend caches immutable audio.** A served item carries `ETag` (the content hash) and
  `Cache-Control: public, max-age=31536000, immutable`, and a matching `If-None-Match` gets 304 with
  no body. Content-addressed audio cannot change, so re-downloading it on every fetch was waste.
  37 unit + 15 end-to-end tests; verified with curl (200 + ETag, 304 on revalidation, 200 for a
  stale validator).
- **oag-listend's HTTP listing is paginated and query-string aware.** A query string was part of
  the path, so `GET /audio?offset=0` was a 404; the path is now split from the query, and
  `GET /audio` takes `?offset=&limit=` with `X-Total-Count`/`X-Offset`, matching the native listing.
  37 unit + 14 end-to-end tests; verified with curl (X-Total-Count 4 / X-Offset 0 and 2,
  `/audio/<hash>?x=1` → 200, `/nope?x=1` → 404).
- **oag-listend's listing is paginated.** `List` returned every hash in one frame, so a store with
  more than ~250k items would make the daemon send a frame its own client must reject as too long.
  `List { offset, limit }` now returns one page plus the store's `total`; stream carriers cap the
  page at `MAX_LIST_LIMIT`, UDP at a datagram-sized cap. 37 unit + 13 end-to-end tests; verified by
  hand (four sounds walked in two pages of two, total 4, unique 4, sorted).
- **oag-listend's load report is honest about eviction.** `LoadReport::loaded` counted entries
  accepted during a load, but a load exceeding the byte bound evicts older entries, so the number
  overstated what is served. The report now separates `loaded` (held), `skipped` (not read or
  rejected), and `evicted` (accepted then pushed out). 36 unit + 13 end-to-end tests; verified by
  hand (`loaded 2 skipped 0 evicted 2`).
- **oag-listend's startup generation no longer grows the store.** Generation ran on every start, so
  with `--dir` and the default clock seed each restart added four new sounds and would slowly evict
  the operator's own audio. It now seeds only an empty store and says so when it skips; a generated
  sound the store refuses is reported rather than dropped. 35 unit + 13 end-to-end tests; verified by
  hand (three restarts stayed at 4 items; `--max-bytes 5` reported all four refusals).
- **oag-listend's hello says what the daemon is.** `Hello` answered with only an item count; it now
  carries the daemon name, the frame protocol version, and the count, and an unrecognised name still
  decodes so a future daemon is recognisable rather than a protocol error. `GET /` reports the
  protocol version too. 33 unit + 13 end-to-end tests; verified by hand on TCP and UDP.
- **oag-listend's HTTP carrier allows cross-origin browser clients.** Every response carries
  `Access-Control-Allow-Origin: *` and `OPTIONS` is answered with the allowed methods and headers,
  so a page on another origin can `fetch()` the audio instead of being blocked at the preflight.
  32 unit + 13 end-to-end tests; verified with curl (204 preflight, ACAO on GET). IPFS and
  Reticulum are recorded as deliberately not built: both need external infrastructure or a
  physical interface and cannot be tested from this repository.
- **oag-listend runs as a service and stops cleanly.** It handled only SIGINT, so `systemctl stop`
  and `kill` bypassed the clean path; SIGINT and SIGTERM now both return the accept loops, remove
  the Unix socket, and exit 0. `scripts/oag-listend.service` runs all four carriers with a
  persistent store, and the README documents the install/start commands. Verified by hand: both
  signals gave exit 0 with the socket removed.
- **oag-listend's listing order is reproducible across a restart.** `hashes()` returned insertion
  order, which a restart rebuilds in filesystem-dependent directory-iteration order; it is now
  sorted by content hash, so the walk a client sees is identical across a restart. 32 unit + 12
  end-to-end tests; verified by hand (`GET /audio` before and after a restart produced the same
  four hashes in the same order).
- **oag-listend serves over a Unix domain socket.** `--unix PATH` serves the same frame protocol
  over a filesystem socket: no port to allocate, permissions decide who connects. The TCP session
  loop is generic over the stream, so the new carrier shares the protocol, bounds, and inbound rule
  by construction. A stale socket from a killed daemon is removed on bind and shutdown.
  32 unit + 11 end-to-end tests; verified by hand over AF_UNIX.
- **oag-listend enforces its store bound on the way in.** Two unbounded paths are closed:
  `accept` no longer writes an item larger than `--max-bytes` and immediately evicts it (it refuses
  with `ListendError::TooLarge`), and a startup file larger than the bound is skipped without being
  read. 32 unit + 9 end-to-end tests; verified by hand (4044-byte WAV vs a 100-byte bound refused,
  small WAV accepted).
- **oag-listend's HTTP carrier is self-describing.** `GET /audio` lists the content hashes (the
  list operation, on HTTP), so a client that only fetches can discover what exists, and
  `Expect: 100-continue` is answered before a large upload is read. 30 unit + 9 end-to-end tests;
  verified by hand (`GET /audio` listed the hashes; a raw POST with `Expect` got `100 Continue`
  then `201 Created`).
- **oag-listend's HTTP carrier streams.** Range requests (206 with `Content-Range`, or 416 when
  unsatisfiable; multiple ranges ignored), HEAD (entity length, no body), and a content type
  derived from the bytes rather than a declared one -- a WAV posted as
  `application/x-www-form-urlencoded` is served as `audio/wav`, so a browser will play it.
  30 unit + 8 end-to-end tests; verified with curl (`-r 0-9`, `-r -10`, `-r 999999-`, `-I`).
- **oag-listend survives a restart.** The store is content-addressed: `--dir PATH` writes every
  accepted item to `<PATH>/<hash>` before it is answerable (a write failure is a refusal, not a
  silent loss), and startup re-checks every entry -- name must be the BLAKE3 of the bytes, bytes
  must sniff as audio -- reporting `loaded N skipped M`. Eviction deletes the file, so
  `--max-bytes` bounds disk as well as memory. 30 unit + 6 end-to-end tests; verified by hand
  (restart loaded 5, served the posted WAV byte-identical). SPEC.md: "serves, and survives a
  restart".
- **oag-listend also serves over UDP.** The imagination seam's datagram transport: one datagram
  is one request, the reply returns to the sender, and the frame codec is reused unchanged, so the
  four operations and the inbound rule are identical. A response larger than 65,507 bytes is
  refused, never truncated. `push_pull` now works over TCP and UDP. 25 unit + 5 end-to-end tests.
  Verified by hand on both. Next: the store is still in-memory, so a restart loses the audio.
- **oag-listend also serves over HTTP.** A hand-rolled HTTP/1.1 carrier shares the native
  carrier's store: `GET /audio/{hash}`, `POST /audio` through `AudioStore::accept`, 404 elsewhere.
  Verified by hand with curl -- a 1644-byte WAV came back byte-identical, empty and HTML bodies
  labelled `audio/wav` were refused. 20 unit + 4 end-to-end tests. Next: the imagination layer's
  `Transport` carriers (UDP, IPFS, Reticulum).
- **oag-listend serves.** The TCP carrier is live: a listener, a length-prefixed frame protocol
  with four operations (hello, publish, fetch, list), and every inbound publish through
  `AudioStore::accept`. A seeded WAV generator (silence, tone, noise) is published at startup, the
  daemon binary is `oag-listend --listen --seed --max-bytes`, and `examples/push_pull.rs` exchanges
  audio by hand. 20 unit tests plus 2 end-to-end tests, the whole exchange bounded at 10s; verified
  against a running daemon (both sounds fetched `verified=true`, HTML labelled `audio/wav`
  rejected). `SPEC.md` status is now "serves". `target/` was untracked in the same commit.
- **oag-listend is founded** -- the fifth repository and the serving surface above the layers:
  the global daemon that serves audio to anything that connects, and accepts nothing but audio.
  The audio surface exists (`AudioItem`, `AudioStore`, `AudioFormat::sniff`, and the inbound rule
  `AudioStore::accept`, checked on bytes never on a declared type). AGPL-3.0-or-later, 5 tests, no
  carrier yet. `DIRECTIVE.md` names five repositories and "THE SERVING PURPOSE".

- **The pruner no longer races cargo.** The prune hook could delete the
  `incremental/`/`.fingerprint/`/`build/` directories cargo had created for the units it was about
  to compile, failing the build with `failed to write .../invoked.timestamp`. The wrapper now
  passes the cargo process's own start time as `--protect-after` and the pruner skips anything
  newer. 16 script tests pass (was 14); `cargo build --examples` is clean across repeated runs.
- **The imagination layer's data-pad gather fans out with a bounded concurrency.** The gather
  asked each node for its pad sequentially, so a file split across the default 1000 pads cost a
  thousand round-trips. `MAX_CONCURRENT_GATHERS` (32) caps the fan-out; `datapad::gather` restores
  manifest order. `oag-imagination` 55 passed (was 54).
- **The mesh's budget sync scheduler is tested.** `spawn_periodic_sync_with_budget` was added
  without a test; it is a thin wrapper over `sync_with_all_peers_bounded`, so the new test
  proves it is still a scheduler rather than a behaviour change behind a new name. `oag-mesh`
  197 passed (was 196).
- **Every outbound HTTP client in the workspace is bounded.** The API's gateway quorum fan-out
  and relayed transfer used a plain `reqwest::Client::new()` -- a peer that accepted but never
  responded would hold them open indefinitely. The operator-configured integrations were also
  unbounded, and `spec/18` had recorded that as a deliberate carve-out; that judgement did not
  hold (a timeout is not a trust decision, and the embeddings provider sits on the search path).
  All five clients now carry named connect and request timeouts.
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

1. **oag-listend: a carrier that does not need external infrastructure.** IPFS needs a Kubo node
   or an embedded libp2p node and Reticulum needs a physical interface, so both are recorded as
   deliberately not built. The honest next carrier is one that can be tested from this repository.
   The service unit and clean SIGTERM stop are done, so "turn this thing on" is
   `systemctl enable --now oag-listend`.
2. **oag-listend: the remaining carriers and a real address.** The HTTP carrier now streams; the
   native and UDP carriers and persistence are done. IPFS and Reticulum, each reusing
   `AudioStore::accept` and adding no operations; then give the daemon a real listening address on
   the operator's network and a documented way to run it (a unit file or equivalent), so "turn this
   thing on" is one command rather than a `cargo run`.
2. **Collisions: choose the default probe budget.** EXP-025 and EXP-027 together show the
   1-flip and 2-flip schemes are a stated trade -- 8 probes / 3.1% unrelated / 67-100% recall
   versus 36 probes / 13.8% unrelated / 92-100% recall with a standard deviation of 0.7. The API
   leaves the choice to the caller, which is correct, but nothing in the repository records which
   budget a deployment should start from. The open question is a real corpus: every measurement so
   far is on one synthetic document shape, and EXP-027 showed the recall varies 21-96% across
   documents even at a fixed edit model.
3. **Mesh: the fallback classification's blind spot.** The classification is on
   `reqwest::Error::is_connect()` / `is_timeout()`. A slow-but-alive peer whose heads request times
   out will skip the fallback; its stores would likely time out too, so the skip is defensible --
   but it is a judgement, not a measurement. The open question is whether a timed-out heads request
   should still try one store (cheap) before giving up, which would need a measurement of how
   often a timed-out heads endpoint still serves stores.
4. **Mesh: no atomic multi-store *write* during a pull.** The commit marker closed the read side;
   a pull that updates several stores is still best-effort per store. Re-examine against
   `persistence.rs` first: `save_to_dir` already writes all seven stores plus a commit marker
   atomically, so the gap may be narrower than it looks -- the in-memory merge is per-store and a
   crash loses it, but that state is re-learned from the mesh.
5. **rust-ai: make the new HTTP timeouts configurable.** Every outbound client is now bounded,
   but the values are fixed constants. An operator running a genuinely slow self-hosted model may
   need a longer embeddings timeout, and a slow Kubo daemon a longer pin timeout. The open
   question is whether these belong in `config.toml` (like `[gateway].query_peers`) or are fine as
   compile-time constants -- that needs an operator's requirement, not a guess.
6. **Imagination: the gather's bound and its node list.** `MAX_CONCURRENT_GATHERS` (32) is a fixed
   constant, and the fan-out loop is over the caller's `nodes` list rather than the manifest's
   `pad_ids` -- so a caller that passes more nodes than the manifest names still asks them all. The
   open question is whether the bound should be configurable per carrier and whether the loop should
   be driven by the manifest.
7. **rust-ai static page hosting follow-ups** — scheduled backup pinning and evidence-body
   inlining remain (see `spec/29` "Explicitly deferred"); crawler side-fetch pinning is done.
8. **Mesh: `spec/06` (owner-controlled research data) and `spec/07` (object history/audit)** —
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
