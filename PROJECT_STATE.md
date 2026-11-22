# PROJECT_STATE — OAG mission state

Authoritative current state of the whole Project OAG mission across its five repositories. This
file describes *where the mission stands*. The reasoning behind individual design decisions lives
in each repository's own `OPEN_QUESTIONS.md` and `spec/`; raw user prompts live in each repo's
`USER_INPUT_RECORD.md` where one exists.

Last updated: 2026-11-14.

## Repositories

| Layer | Repository | Local path | Role |
|---|---|---|---|
| 0 | `open-agent-graph` | `/home/webdawg/DATA/CODE/rust-ai` | one peer's signed event graph |
| 1 | `oag-mesh` | `/home/webdawg/DATA/CODE/oag-mesh` | a mesh of peers — clusters, federated trust, onion routing |
| 2 | `oag-collisions` | `/home/webdawg/DATA/CODE/oag-collisions` | inference by collision -- a similarity mechanism built on locality-sensitive hashing |
| 1 (parallel) | `oag-imagination-random-data-compute` | `/home/webdawg/DATA/CODE/oag-imagination-random-data-compute` | synthetic data — dreams from verifiable randomness |
| serving | `oag-listend` | `/home/webdawg/DATA/CODE/oag-listend` | the global daemon that serves audio and accepts nothing else |

These state files live in `rust-ai` because it is the foundational layer, but they describe the
mission as a whole.

## Current status (measured)

| Repository | Tests | Commits | Specs |
|---|---|---|---|
| `open-agent-graph` | 435 | 123 | 01–32 |
| `oag-mesh` | 188 | 43 | 01–15 |
| `oag-imagination-random-data-compute` | 48 | 31 | 01–03 |
| `oag-collisions` | — | 44 | `SPEC.md` |
| `oag-listend` | 127 | — | `SPEC.md` |

All five working trees are clean and pushed as of the last update.

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

### oag-listend (the serving surface)
The perpetual-directive repository: one stable audio surface behind pluggable carriers.
- Five ways to reach the audio, all sharing one store: native length-prefixed frames over TCP,
  HTTP/1.1, UDP datagrams, a Unix domain socket, and an IPFS mirror behind `--ipfs-api`.
- Inbound rule at the surface, on the bytes and never on a declared type: WAV, AIFF, FLAC, Ogg
  (Vorbis/Opus/FLAC, not Theora), MP3 (ID3 or raw frame), M4A by brand list (not generic
  `isom`/`mp42` video). Everything else is refused before it is stored, and a refusal never echoes
  the refused bytes.
- Content-addressed under BLAKE3; `--dir` persists and every entry is re-verified on load
  (`loaded`/`skipped`/`evicted`/`truncated`, never silent).
- Generated sound (silence, tone, noise) plus the canonical musical ping -- "even pings are music",
  the opening phrase of Twinkle Twinkle Little Star -- seeded only into an empty store, on every
  carrier.
- Bounds all measured by tests that were checked to fail when the property breaks: per-syscall
  read/write timeouts, session and connection totals, the 16 MiB frame cap, the store byte bound
  with eviction and off-lock unlink, the 64-session cap, lock release before byte copies, the HTTP
  head/body caps, the UDP listing page, and a hung IPFS node not stalling other clients.
- The IPFS surface is two-way behind `--ipfs-api`, and best-effort throughout. Writing uses one
  `accept_and_mirror`: accept under the store lock, then send with no lock held, so a slow Kubo cannot
  stall the daemon; a failure is recorded, never propagated, and bytes are local first. Reading is
  `GET /ipfs/{cid}`: audio the daemon already holds is served straight from the store (so a node that
  is down does not take held audio with it), and only an unfamiliar CID goes to `/api/v0/cat`.
  Whatever comes back faces the same sniff rule, so non-audio is refused un-echoed and un-stored.
  A fetched item carries its CID as an optional trailing field and as `X-IPFS-CID`; CID-served bytes
  carry `X-Content-Hash` and `Cache-Control: immutable`. Chunked replies are refused rather than
  misread as audio, and `https://` endpoints are refused rather than silently downgraded to
  cleartext. Tested against stubs mimicking `/api/v0/add` and `/api/v0/cat`; **untested against a
  real Kubo node**.
- Operator surface: `oag-listend` binary (`--version`, `--help`, usage errors exit 2, clean
  SIGINT/SIGTERM stop), `--check` (a store verification that does not serve and does not contact
  IPFS), `--seed`, `--no-generate`, and `push_pull` as the smallest honest client. systemd unit
  included.
- Every gate is one command: `scripts/check.sh [--release]` runs fmt, clippy, rustdoc with warnings
  denied, the examples build, the suite, and `systemd-analyze verify` on the shipped unit -- and
  `.github/workflows/check.yml` runs that same script on every push and pull request, so a merge
  cannot bypass it. All green, debug and release, and from a clean clone.
- A stop is graceful but bounded: accepting stops at once, in-flight exchanges get up to 2 s
  (`SHUTDOWN_GRACE`) to finish rather than being cut off mid-transfer, then the process exits 0; a
  client that never reads cannot hold a stop open past that bound. Both SIGINT and SIGTERM are
  registered before anything observable (a signal in that window previously killed the daemon), and
  both are covered by tests that confirm the daemon is genuinely serving before signalling.
- Hostile input is fuzzed for panic-safety: 5,000 deterministic random bodies through the codec, and
  garbage plus lying lengths across native, HTTP, UDP and Unix, with liveness asserted after.
- A stop is graceful but bounded: accepting stops at once, in-flight exchanges get up to 2 s
  (`SHUTDOWN_GRACE`) to finish rather than being cut off mid-transfer, then the process exits 0.
- Reticulum remains deliberately unbuilt: it needs a physical interface.

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

- **oag-listend takes the eviction unlink off the lock.** Eviction unlinked the file
  synchronously while holding the store mutex; it now records the paths and `accept` drains them
  asynchronously after the publish, so a slow disk delays the answer instead of stalling every
  other client's executor thread. A load drains inline. 51 unit + 24 end-to-end tests.
- **oag-listend resolves an uppercased content hash.** The hash validation accepted uppercase hex
  but the lookup was case-sensitive, so an uppercased hash was accepted and then reported missing.
  The reader and the HTTP route now normalise to lowercase. 51 unit + 24 end-to-end tests.
- **oag-listend's reader reports a length-prefixed string that runs past the remaining bytes as a
  bad string.** The pre-check compared against the whole buffer rather than the bytes remaining, so
  a length one past the end reported `Truncated` while a longer one reported `BadString`.
  49 unit + 24 end-to-end tests.
- **oag-listend's HEAD revalidation reports the entity length.** A `304` has no body, but a `HEAD`
  still describes the entity, so its `Content-Length` is the item's length rather than zero.
  49 unit + 24 end-to-end tests.
- **oag-listend's `push_pull` has a `--help`.** The daemon binary had one; the operator's client did
  not. It prints its usage and exits 0.
- **oag-listend's CLI argument errors are usage errors, not panics.** A missing or invalid flag
  value in the daemon binary and in `push_pull` used to panic (exit 101); both now print a message
  and exit 2, like the unknown-argument path. 49 unit + 24 end-to-end tests.
- **oag-listend is rustfmt-clean.** The crate had accumulated hand-wrapped lines; `cargo fmt` was
  run and `cargo fmt --check` is clean. Behaviour unchanged (49 unit + 24 end-to-end, hand-run
  acceptance path verified).
- **oag-listend's HEAD range refusals carry no body.** The 416 branch on both range routes built a
  body and did not clear it for `HEAD`, so a HEAD with an unsatisfiable range would have sent one.
  49 unit + 24 end-to-end tests.
- **oag-listend refuses a chunked upload clearly, without losing the response.** A
  `Transfer-Encoding: chunked` POST was read as zero bytes and rejected as "not audio", and closing
  with the body unread made TCP send a reset that could discard the 400. It now refuses with a
  message saying to send `Content-Length`, and drains the unread body after a half-close.
  49 unit + 24 end-to-end tests.
- **oag-listend ranges `GET /ping`.** The liveness sound is served audio, so it now ranges like
  `/audio/{hash}`: 206 with `Content-Range`, 416 when unsatisfiable, `Accept-Ranges` on the full
  response. 49 unit + 24 end-to-end tests.
- **oag-listend bounds and reports the directory entries a load considers.** A store directory is
  an operator-controlled path; a mistake or a symlink to somewhere huge must not make startup
  process an unbounded number of entries. The load stops at `MAX_LOAD_ENTRIES` (100,000) and reports
  `truncated` instead of silently ignoring the rest. 49 unit + 24 end-to-end tests.
- **oag-listend's startup set is one of each kind plus the ping.** `sounds()` cycles three kinds, so
  four seeded sounds produced silence twice while the docs said "silence, tone, noise".
  `STARTUP_SOUNDS` is now 3, giving exactly silence, tone, noise, plus the canonical ping (four
  items). Verified by hand.
- **oag-listend caps the generated sample count.** `generate_n` was public and took an unbounded
  count, so a caller could allocate without limit. It caps at `MAX_GENERATED_SAMPLES` (40,000
  frames, 80 KB) -- bounded, and deliberately above one UDP datagram's payload so the
  too-large-for-a-datagram path stays testable. 49 unit + 24 end-to-end tests.
- **oag-listend refuses to delete a non-socket at the Unix socket path.** `bind_shared` removed
  whatever was at the path before binding, so `--unix` pointed at a regular file destroyed it.
  Only a socket is removed now; anything else is refused with `InvalidInput` and left intact.
  48 unit + 23 end-to-end tests; verified by hand.
- **oag-listend implements the musical ping (operator directive, Entry 3).** "even pings are music
  - start with twinkle twinkle little star." `wav::twinkle()` generates the opening phrase
  deterministically; the native protocol's new `Ping` operation and HTTP's `GET /ping` both answer
  with that audio item and its BLAKE3 hash; `Hello` is unchanged. The canonical ping is in the
  startup set, so it is durable and fetchable by hash. The rendition is the opening phrase only
  (44,844 bytes) so it fits a UDP datagram -- a longer one made UDP answer with a rejection instead
  of music. All four carriers answer the ping. 47 unit + 21 end-to-end tests; verified by hand
  (native, HTTP, and UDP all returned the same WAV and hash).
- **oag-listend serves a `Content-Disposition` with a real filename.** A browser or player saving
  `/audio/<hash>` got a bare hash; it now gets `<hash>.<ext>`, with the extension from the sniffed
  format. 46 unit + 19 end-to-end tests; verified with curl.
- **oag-listend's self-heal attempts a restore on an error too.** `try_exists` errors were treated
  as "present", so a deleted store directory was not recreated and the item stayed without a durable
  copy. An error is now treated as absent: `persist` recreates the directory or fails loudly.
- **oag-listend skips non-regular files when loading the store.** A FIFO or device named like a
  content hash would make `std::fs::read` block forever during startup; a directory would error.
  Only regular files are read now (symlinks to regular files still load). 46 unit + 19 end-to-end
  tests; verified by hand (a FIFO named like a hash no longer blocks `--check`).
- **oag-listend restores a persisted file removed out of band.** The durable copy is the source of
  truth, but a file deleted out of band left the in-memory item serving audio the next start would
  lose. A republish of the same bytes now restores it; the existence check costs a syscall only on a
  duplicate publish, never on the read path. 45 unit + 19 end-to-end tests.
- **oag-listend's bare-run examples use a creatable socket path.** The README and SPEC examples used
  `/run/oag-listend/audio.sock`, which a non-root `cargo run` cannot create (only systemd's
  `RuntimeDirectory` does). Both now use `/tmp/oag-listend.sock`; the unit keeps `/run` and says
  why. Verified by hand: the example binds with no `mkdir`.
- **oag-listend bounds the persistence sidecar read.** `read_sidecar` read the whole `<hash>.json`
  file, so a hand-placed huge sidecar was pulled into memory on every load. A sidecar larger than
  4 KiB is now ignored. 44 unit + 19 end-to-end tests.
- **oag-listend bounds the whole HTTP connection.** `REQUEST_TIMEOUT` wrapped only the read, so a
  client that read the response slowly could hold a session slot for the entire transfer. The total
  now covers read, route, and response, symmetric with the native session bound. 43 unit + 19
  end-to-end tests.
- **oag-listend refuses to steal a live Unix socket.** `bind_shared` deleted an existing socket
  file before binding, so a second daemon on the same path silently stole it and clients split
  between the two. A socket still being served is now refused with `AddrInUse`; a stale socket from
  a killed daemon is still removed and reused. 43 unit + 19 end-to-end tests; verified by hand
  (second daemon exits 1 with "is already being served"; SIGKILL'd socket reused).
- **oag-listend bounds each I/O syscall, not the whole transfer.** The 10 s timeout wrapped the
  whole `write_all`/`read_exact`, so a legitimate transfer of the largest item to a slow client was
  cut off mid-transfer. `read_exact_bounded`/`write_all_bounded` apply the timeout per syscall.
  43 unit + 18 end-to-end tests, including two slow-but-progressing transfers that exceed the bound
  and succeed.
- **oag-listend rejects Ogg video, not just MP4 video.** Any `OggS` container was accepted, but Ogg
  also carries Theora video -- the same false positive as the MP4 major-brand check. The
  beginning-of-stream page's codec header is now read; Vorbis/Opus/FLAC/Speex/PCM pass, anything
  else (including Theora) does not. 41 unit + 18 end-to-end tests; verified by hand (vorbis 201,
  opus 201, theora 400, unknown 400).
- **oag-listend bounds the total length of a session and an HTTP request.** The timeouts were per
  read, so a client dribbling a byte every few seconds could hold a session open indefinitely, and
  64 of them would exhaust the session cap. Each session and each HTTP request now also has a total
  bound of 30 minutes -- generous enough for a large upload over a slow link, but finite.
- **oag-listend's HTTP listing releases the store lock before building its body.** The page was
  copied under the lock but the body was built while still holding it, contrary to the documented
  mutex scope. 41 unit + 18 end-to-end tests.
- **oag-listend has `--check` and `--version`, and the service user is documented.** `--check`
  opens the store and prints the load report and effective item bound without serving, so a store
  can be verified before the service starts. `--version` prints the binary and frame-protocol
  versions. The systemd unit runs as `oag` but the install steps never created it, so a fresh
  install would fail; the README and unit comment now create it. Verified by hand.
- **oag-listend's fetch no longer holds the store mutex during the byte copy.** `Fetch` cloned the
  item's bytes under the lock, so a large fetch stalled every other store operation for a
  multi-megabyte copy. `AudioStore` now holds `Arc<AudioItem>` and `serve` returns a cloned handle;
  the carriers copy after releasing the lock. 41 unit + 18 end-to-end tests; verified by hand.
- **oag-listend's persistence write no longer blocks the executor.** `accept` did a blocking
  `std::fs::write` while holding the async store mutex, so a slow disk stalled every carrier.
  `accept`/`persist` are now async and use `tokio::fs`; the write still completes before the item
  is answerable, so a failure is still a refusal. The eviction unlink stays synchronous
  (metadata-only). 41 unit + 18 end-to-end tests.
- **oag-listend's store is tested under concurrency.** The store is one mutex and the persistent
  `accept` does a blocking file write while holding it; neither had a concurrent test. Two bounded
  tests: 16 tasks x 20 publish+fetch against one in-memory store (320 distinct verified hashes, all
  listed), and 8 tasks x 10 publishes to a persistent store then a restart (all 80 survive and are
  served). Both pass. 41 unit + 18 end-to-end tests.
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

- **The pruner no longer races cargo, and the flaky release gate is fixed.** The prune hook could
  delete the `incremental/`/`.fingerprint/`/`build/` directories cargo had created for the units it
  was about to compile, failing the build with `failed to write .../invoked.timestamp`. The
  wrapper passes the running cargo process's start time as `--protect-after`. Two further causes
  of the same failure were found and fixed: a *new* cargo run starts after the previous one
  finished, so its horizon is later than the artifacts the previous run just wrote (a gate running
  several cargo commands back to back deleted them mid-run), and cargo reuses a fingerprint
  directory across builds, so the directory's own mtime goes stale while its contents are fresh.
  A generation is now aged by the newest mtime of the directory and its immediate children, and a
  generation written within the last 30 minutes is never pruned whatever cargo is doing. **The
  bound is `min(--protect-after, now - 30 min)`, not `max`.** `protect_after` means "skip
  anything at or after this time", so a later bound protects more, and the `max` first written
  here silently discarded the recent window on every real build -- the gate failed again the next
  invocation, and the 5/5 recorded in the previous entry was a narrower run than it looked. The
  minimum protects the union: this cargo's outputs *and* the last half hour's. 20 script tests pass
  (was 16), each new test failing when its fix is reverted, including
  `test_the_cargo_horizon_and_the_recent_window_protect_the_union`; `check.sh --release` passes
  6/6 after the correction. Residual, recorded rather than claimed away: a generation older than
  30 minutes that a later cargo run reuses can still be pruned mid-run; the operator's documented
  `target/.rustc_info.json` remedy applies.
- **A published media type can no longer outrun the store bound.** `AudioStore::accept` wrote a
  publisher's declared media type to `<hash>.json` with no length bound, while the loader already
  refuses a sidecar over 4 KiB. A frame carries ~16 MiB, so a publisher could hand the daemon
  megabytes of "metadata" that were written to disk and then ignored by every later load --
  `--max-bytes` bounded the audio but not the bytes stored. `AudioItem::new` now applies
  `MAX_SIDECAR_BYTES` and trims, so the write bound equals the read bound; an absurd type is
  dropped, not refused, so the audio is never lost because of its metadata. `oag-listend` 73 unit
  + 77 end-to-end (was 70 + 77).
- **A network-reachable panic in the IPFS chunked-reply decoder.** `decode_chunked` bounds-checked
  with `pos + size` and `out.len() + size`; a chunk size near `usize::MAX` wraps both sums to small
  numbers, so both checks pass and the slice panics (debug: "attempt to add with overflow"; release:
  the wrapped slice). The mirror is cleartext HTTP, so a hostile or compromised Kubo node -- or an
  on-path attacker -- only has to send one chunk size. Both halves (`add` and `cat`) go through it.
  Fixed with `checked_add`/`saturating_add`; pinned three times, at the parser, over a stub node
  through the carrier entry point, and at the binary's startup mirror; and the rest of the crate
  audited for the same shape. **Blast radius, measured:** the startup mirror pass runs on the main
  task, so that panic exited the process (101) -- a remote kill at startup with no client involved;
  a publish-triggered mirror runs in a session task and only killed that connection, with the daemon
  still serving. `oag-listend` 81 unit + 82 end-to-end (was 80 + 80).
- **Two false claims about the store lock, and a stale 413 quote, corrected.** SPEC said the
  eviction unlink "is taken off the lock" and that "a slow disk or a large item delays the answer
  that caused it, not every other client". `accept` runs with the lock taken and awaits both the
  durable write and the eviction unlink, so neither claim held; `accept_and_mirror`'s own doc always
  said the lock covers "the rule, the durable write, and the insert". Measured against the real
  binary with its store on the operator's network share, a 16 MiB publish and a stream of 16 MiB
  publishes with an eviction on every one, a concurrent fetch answered in 1-3 ms -- so the behaviour
  is fine and the *claim* was wrong. README also quoted a 413 message from before the message was
  corrected to name the footprint and the refusing bound; it is re-verified against the binary and
  now pinned by a test.
- **`--check`'s read-only contract is pinned at the binary level.** The library test passed while
  nothing exercised `main`'s choice of `check_directory`, so an edit back to `with_directory` would
  have deleted an operator's audio with the suite green. The new binary test runs the real command
  against three items and a bound that fits one, and asserts the report says `loaded 1 evicted 2`
  while all three files survive. Also in this pass: `evict_oldest` reports whether it evicted so
  the eviction loop cannot spin, `LoadReport::evicted` names a bound rather than the byte bound,
  and SPEC records the sniff's deliberate scope (signatures, not completeness -- a bare container
  header is accepted, and refusing audio would violate the first requirement).
  `oag-listend` 80 unit + 80 end-to-end (was 80 + 79).
- **The store is bounded in entries as well as bytes.** `bytes_held` bounds audio, but every item
  also costs a map entry, a queue entry and an `Arc`, so a publisher of tiny sounds could drive
  memory far past `--max-bytes` while the daemon reported itself within it. Measured against the
  real binary: 20,000 twelve-byte items cost 4 MB while `bytes_held` was 240 KB against a 1 GiB
  bound. `publish` now evicts oldest-first while either bound is exceeded, capped at
  `MAX_STORE_ENTRIES` -- the same number a directory load honours. Verified against the binary:
  150,000 twelve-byte publishes leave exactly 100,000 held, at 51 MB RSS. `oag-listend` 80 unit +
  79 end-to-end (was 79 + 79).
- **The shipped client cannot be made to reserve 4 GiB, and the suite cannot test a stale binary.**
  `push_pull` allocated `vec![0u8; len]` from a wire length prefix before checking it, on the TCP and
  Unix branches, so a broken or hostile daemon declaring `u32::MAX` could make the operator's client
  reserve 4 GiB. A shared `read_frame` now checks `MAX_FRAME_BYTES` first, as the daemon's own reader
  does. The test that pins this exposed a second defect: `push_pull()` built the example only when
  missing, and `cargo test` does not rebuild an example whose source changed -- so the test silently
  ran against the pre-fix client. The helper now always asks cargo to bring it up to date.
  `oag-listend` 79 unit + 79 end-to-end (was 79 + 78).
- **A 413 names the bound that actually refused the item, and the suite no longer races for test
  ports.** The HTTP POST handler rebuilt the `TooLarge` error from the body length and the store
  bound, so an item refused by the frame cap was reported as "larger than the 1073741824 byte store
  bound" -- false on both counts, and indistinguishable from a store-bound refusal. It now reports
  the error `accept` returned. Separately, `free_port()` reserved a port by binding and dropping it,
  leaving a window in which another process takes it; the daemon then exits on a bind failure the
  test reads as "never announced". Every test that can read the announcement now binds port 0 and
  takes the address from it, and the one that cannot (`/dev/full` stdout) retries on a fresh port.
  `announce` also lost a 200 ms per-read idle timeout that would misread the 30 s startup mirror as
  a crash. `oag-listend` 79 unit + 78 end-to-end (was 79 + 77).
- **`--check` no longer deletes anything.** It is documented as "purely a store verification", but
  it used the same load path as a start, and that path unlinks what `--max-bytes` pushes out.
  Reproduced with the release binary: four generated sounds, `--max-bytes 3000 --check`, left
  three files. `AudioStore::check_directory` now opens without changing anything -- over-bound
  entries are reported as `evicted` and their files are left alone, and a missing directory is an
  error rather than something to create. A real start still prunes on load. `oag-listend` 79 unit +
  77 end-to-end (was 77 + 77).
- **A kept media type now counts toward `--max-bytes`.** `bytes_held` counted only audio while
  `persist` also wrote the declared type to `<hash>.json`, so a 12-byte WAV with a 4 KiB type was
  accepted and the sidecar grew uncounted -- the directory could far exceed the bound while `GET /`
  reported the store within it. `AudioItem::footprint()` (audio + declared type) is what
  `bytes_held` sums and eviction subtracts, and `accept` enforces two distinct checks: the *audio*
  must fit a fetch reply (the media type is not in the reply, so it does not shrink that bound) and
  the *footprint* must fit the store bound. `GET /` says what it counts. `oag-listend` 77 unit + 77
  end-to-end (was 75 + 77).
- **The shipped CI service job now runs the acceptance test.** It started the unit, fetched
  `/ping` and `/`, and stopped -- which proves the unit serves HTTP but never exercises the frame
  protocol. It now also builds the shipped `push_pull` client and runs it against the running
  service over TCP and UDP, so a regression that left the native listener unable to publish or
  fetch fails the job instead of passing it. Verified by hand against a running systemd unit with
  the unit's own flags: `verified=true` on both carriers, hashes unchanged, non-audio refused; the
  exact CI shell snippet was run as written.
- **A sidecar cannot hang or exhaust the startup load.** `read_cid_sidecar` read the whole
  `<hash>.cid` file before rejecting it if it was over 128 bytes, so a hand-placed sidecar of
  arbitrary size was pulled fully into memory -- the exact pattern the audio path already refuses.
  Neither sidecar reader checked regularity either, so a FIFO named `<hash>.cid` or `<hash>.json`
  would have made `std::fs::read` block forever and hung startup. One `read_bounded_regular` helper
  now stats first and requires a regular file within the bound; both readers use it. Pinned by a
  test that puts a FIFO under both sidecar names and bounds the blocking load on a thread: it fails
  at 10 s with the old reader and passes in 0.00 s with the fix. `oag-listend` 75 unit + 77
  end-to-end (was 73 + 77).
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

1. **oag-listend: testing IPFS and Reticulum against the real things.** The imagination `Transport`
   seam is fully accounted for: TCP, UDP, HTTP, IPFS, the Unix socket, and Reticulum are all
   implemented here; the HTTP CONNECT tunnel is a *client-side* wrapper (`ProxiedTransport`) that
   needs no server code -- recorded so it is not re-investigated as missing. All of them share one
   store and serve the musical ping, and the IPFS mirror sits behind `--ipfs-api`.

   **Reticulum was built** (it is not hardware-bound: it speaks over Reticulum's own `TcpServer`
   interface, exactly as `oag-imagination-reticulum`'s loopback test does). It is opt-in behind
   `--features reticulum` because the crate adds ~160 packages; `--reticulum ADDR` serves the same
   audio, and it carries the same bounds as the other carriers (64-link cap, per-link worker, 10 s
   send bound, shutdown drain). What it has **not** been shown to do is interoperate with another
   Reticulum implementation -- loopback proves the audio surface, not interop.

   The IPFS mirror is validated against a real Kubo node (0.43.1): add returns a CID the node pins,
   `/api/v0/cat` returns exactly the bytes, `GET /ipfs/{cid}` serves them byte-identically -- for a
   CID the daemon holds and for one it does not -- a non-audio object fetched by CID is refused, and
   a restart re-adds nothing. Chunked replies are decoded by both halves — refusing them made the write
   half fail against every real node.
   Reticulum interop still needs a real transport instance, which this machine does not have.

   The service unit and clean SIGTERM stop are done, so "turn this thing on" is
   `systemctl enable --now oag-listend`; CI installs and starts that unit on every push
   (`check-service`), fetches the musical ping and the index over HTTP, **and runs the shipped
   `push_pull` client against the running service over both the TCP and UDP carriers** -- so the
   acceptance sentence (a second process pushes audio in, pulls it back, proves the bytes) is
   executed against the installed binary under systemd, not only against a test harness. The Unix
   socket is not probed there (the unit runs as `oag`, the socket is mode 0755); `unix_carrier.rs`
   covers it.
   Since the last update the operator surface has been closed out and every documented bound made
   observable: the binary itself (`--version`, `--help`, usage exit codes, start/serve/stop) and
   `--check` (load a content-addressed store, report loaded/skipped, skip a corrupted file) are
   automated; eviction, the per-frame cap, native listing pagination, per-format recognition and
   per-format content types are each exercised over the wire; SIGTERM is proven a clean exit 0 that
   removes the Unix socket; the 64-session cap, lock release before copying, the HTTP head/body caps,
   and the UDP listing page are each measured by a test that was checked to **fail** when its
   property is broken, including a hung IPFS node not stalling other clients. The IPFS mirror
   (`--ipfs-api`) offers accepted audio to a Kubo node best-effort and lock-free, mirrors the stored
   library on startup, and carries each CID on the wire and as `X-IPFS-CID`; it is tested against a
   stub and **not** against real IPFS; it also reads by CID (`GET /ipfs/{cid}`, local-first so held
   audio survives a node outage). The build-generation pruner is now wired in `oag-listend` too:
   its `.cargo/` directory existed but was empty, so the wrapper never ran in the repository whose
   `target/` is most likely to grow; the config is now sha256-identical across all five. Current bar:
   81 unit + 82 end-to-end tests, 0 failed; 173 with the opt-in Reticulum carrier.
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
4. **Mesh: no atomic multi-store *write* during a pull -- re-examined, gap is narrower.**
   `pull_changed_stores` merges each changed store **in memory only**; it writes nothing. And
   `save_to_dir` has exactly one production caller (`node.rs`, the shutdown/save path), so the
   atomic-multi-store-write machinery is not on the pull path at all. The residual gap is
   therefore not "best-effort per store on disk" but "a pull's merged state is durable only when
   someone later calls `save_to_dir`". A crash between the two loses the merge, and the mesh
   re-learns it -- so it is a **scheduling** question for the caller (the crate's own docs say a
   caller decides when to save), not a correctness hole here. Closed as examined; reopening needs
   an operator's requirement on pull durability, not a code change.
5. **rust-ai: make the new HTTP timeouts configurable.** Every outbound client is now bounded,
   but the values are fixed constants. An operator running a genuinely slow self-hosted model may
   need a longer embeddings timeout, and a slow Kubo daemon a longer pin timeout. The open
   question is whether these belong in `config.toml` (like `[gateway].query_peers`) or are fine as
   compile-time constants -- that needs an operator's requirement, not a guess.
6. **Imagination: the gather's bound and its node list -- behaviour is deliberate.**
   `MAX_CONCURRENT_GATHERS` (32) is a fixed constant, and the fan-out loop is over the caller's
   `nodes` list. Read the code: each request is a pad-agnostic `PadRequest { pad_id: "" }`, and
   `datapad::gather(manifest, &pads)` filters the responses against the manifest. So asking more
   nodes than the manifest names is **by design** -- a node returns what it holds and the manifest
   decides what is used -- not a defect. The only open question is whether the bound should be
   configurable per carrier, which needs an operator's requirement.
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
