# ARCHITECTURE_DECISIONS — Project OAG

Append-only record of important architectural decisions and why they were made. Newest at the
bottom. Do not rewrite entries to make progress look better than it was.

---

## ADR-001 — Factual and synthetic are separate repositories

**Decision.** Evidence (`open-agent-graph`) and synthetic generation
(`oag-imagination-random-data-compute`) remain separate repositories with no shared write path.

**Context.** Both deal in "knowledge", and it is tempting to let a generated scenario flow into
the graph as an assertion. That would destroy the evidentiary meaning of the graph.

**Consequences.**
- A synthetic scenario can never be promoted to evidence by any code path; only a real-world
  observation can.
- The imagination layer has no writer into `oag`'s event log.
- Cross-layer work must go through an explicit, documented seam rather than a shared database.

**Status.** Accepted. Enforced structurally, not by convention.

---

## ADR-002 — Hand-rolled implementations for narrow surfaces

**Decision.** Prometheus text exposition, the IPFS HTTP client, the embeddings HTTP client, and
onion encryption are implemented directly. `rust-ipfs` and `reticulum` are used where the surface
is broad.

**Context.** Dependency weight is a real cost in a peer that must run on modest hardware. A
Prometheus text renderer is a formatting problem, not a dependency problem.

**Consequences.**
- Fewer transitive dependencies and a smaller supply-chain surface.
- More code to maintain and to test in-repo.
- The narrow implementations must carry their own tests (they do: `spec/19`, `spec/29`, `spec/15`).

**Rejected alternative.** Adopting a full metrics library for a fixed set of ~16 gauges.

**Status.** Accepted.

---

## ADR-003 — No mandatory external infrastructure

**Decision.** No Postgres, Redis, Kafka, or Kubernetes. Embedded SQLite; IPFS and the internet
gateway are opt-in.

**Context.** The founding constraint is that a peer must operate independently. A mandatory
central service contradicts that at the architectural level, not merely operationally.

**Consequences.** A single peer runs with no external dependencies.
- Some capabilities (vector search at scale, durable blob storage) are intentionally out of scope
  or deferred rather than solved with an external store.
- `blob_store_size` is unbuilt because no blob store exists, and adding one would need its own ADR.

**Status.** Accepted.

---

## ADR-004 — PoW bounds cost, never identity

**Decision.** Proof-of-work appears in anonymous events, cluster nesting, and membership
attestation, and in every case it is documented as a cost floor rather than an identity mechanism.

**Context.** PoW is often presented as an anti-sybil measure. It is not: a well-resourced attacker
buys throughput. What it does is make bulk submission cost real resources.

**Consequences.**
- PoW is never used as a gate for reading or trusting.
- PoW never excludes content from federation; it shapes admission order and cost.
- Each mechanism's difficulty is an explicit, tunable parameter.
- The documentation says plainly that PoW does not stop a determined attacker.

**Status.** Accepted. See `RESEARCH_JOURNAL.md` (2026-11-08).

---

## ADR-005 — Degrade, don't drop

**Decision.** Trust is a continuum with a single hard floor (invalid signature, malformed event).
Nothing else is hard-excluded from federation, regardless of how low its nonzero trust gets.

**Context.** A peer that hard-excludes low-trust content on its own judgment becomes a censor, and
the operator cannot audit a decision the peer made unilaterally. A continuum keeps the operator in
a position to observe and override.

**Consequences.**
- Low-trust content is still replicated and still visible, with its trust state attached.
- Operators who want hard exclusion must implement it explicitly and locally, not inherit it.
- The unreachable-next-hop behaviour in the imagination layer follows the same rule: an error
  response, never a hang.

**Status.** Accepted.

---

## ADR-006 — The event log is never deleted or rewritten

**Decision.** Redaction is a local tombstone event, not a mutation of existing rows.

**Context.** The log is the evidentiary basis of the whole system. Rewriting it would invalidate
every signature chain that depends on the altered region and would make a peer's history
unverifiable by others.

**Consequences.**
- "Removing" content means appending a suppression marker; the original bytes remain verifiable.
- A peer that suppresses content may still be contradicted by a peer that does not — which is a
  feature, since suppression is then observable rather than absolute.
- Backup and restore must be log-preserving (`spec/03`).

**Status.** Accepted.

---

## ADR-007 — Metrics that cannot be derived are instrumented in the code path

**Decision.** When a metric is not recoverable from persisted state, it is a real counter in the
code path, not a query in the metrics layer.

**Context.** `verification_jobs`/`verification_failures` are the motivating case: rejected
verification attempts never become events, so a `COUNT(*)` returns a permanent zero no matter how
much rejection traffic the peer sees.

**Consequences.**
- `GraphService` holds `Arc<AtomicU64>` counters; a metrics layer holds the same counters, so it
  observes exactly what the service incremented.
- Counts are taken *after* the outcome is known, so a permission failure counts as a failure.
- The same reasoning applies to `crawls_failed` and `peer_sync_errors`.

**Rejected alternative.** Computing metrics from the event log (structurally impossible here).

**Status.** Accepted (`0a84a78`).

---

## ADR-008 — declare_alias is REST/MCP, not CLI

**Decision.** The alias declaration surface is `POST /api/v1/nodes/{id}/aliases` and the
`graph_declare_alias` MCP tool. No CLI subcommand.

**Context.** Dispute, retract, and supersede already established the convention that
graph-mutating assertions are REST/MCP-only. The crawler covers the CLI-shaped use case.

**Consequences.** One fewer CLI surface to maintain; a consistent convention across the
graph-mutating family.
**Status.** Accepted (`1b6863e`).

---

## ADR-009 — `oag ipfs pin` fails loudly

**Decision.** When no IPFS provider is configured, `oag ipfs pin <file>` returns an error rather
than silently succeeding.

**Context.** A pin command that appears to succeed while pinning nothing is worse than an error: it
gives false confidence that content is replicated.

**Consequences.** Matches the posture of `oag backup --pin`. The error names the missing
provider. **Status.** Accepted (`bab5b74`).

---

## ADR-010 — IPFS site hosting is gateway-free

**Decision.** `/ipfs-site/{cid}` serves a vendored Helia bundle; the browser fetches over bitswap
from the local node.

**Context.** A gateway proxy would make the CID cosmetic: a malicious gateway could serve different
bytes under the same URL while the address bar stayed constant, converting content addressing into
trust-on-first-use.

**Consequences.**
- The CID in the URL is a genuine integrity check.
- No third-party gateway dependency.
- A static bundle with no build step.

**Status.** Accepted (`4ac3a57`). See `RESEARCH_JOURNAL.md` (2026-11-08).

---

## ADR-021 — The imagination engine caps route hops

**Decision.** A request may travel at most `MAX_ROUTE_HOPS` (64) hops. A node that has exhausted the
budget returns `TooManyHops` instead of forwarding.

**Context.** The engine has no visited-set: the pad picks the next hop by weight, so a cycle between
two nodes is a possible routing outcome rather than an impossible one. Without a budget such a
request accumulates one contribution per hop and is re-sent in full each time, amplifying bandwidth
and memory until the carrier's `MAX_MESSAGE_BYTES` happens to reject it. That is a bound, but an
accidental one, and it is measured in megabytes of waste per cycle.

**Consequences.**
- The amplification is bounded by design rather than by an unrelated size limit.
- 64 is well above `spec/02`'s own "6, 10, or 50 nodes" intent, so no intended path is truncated.
- The base project's relay already bounds the same shape with `relay_hops_remaining`; this brings
  the imagination engine to the same posture.
- **The budget bounds forwarding, not answering.** A node that can answer does so however long the
  chain behind the request is; refusing an answer would turn a bounded-chain problem into a failed
  request.
- A refused node still records its own contribution — the bound is on forwarding, not on
  acknowledging participation.
- `HandleOutcome::TooManyHops` is distinct from `NoRoute`, so a caller can tell "nowhere to go" from
  "the chain is too long".

**Status.** Accepted (`548580d`).

---

## ADR-020 — The imagination dream log is bounded by count *and* bytes

**Decision.** `DreamLog` keeps at most 1,000 entries totalling at most 8 MiB serialized, evicting
the oldest until both hold. An entry larger than the whole byte budget is not retained.

**Context.** The log was an unbounded `Vec<Request>`, and `Engine::handle` records every answered
request as a clone. That is a memory-exhaustion vector reachable by any caller who can make the node
answer a request — which is any peer that can route one here — and it is amplified per request,
because a request carries caller-controlled JSON plus the contribution chain gathered along the
route.

**Consequences.**
- A **count** cap alone is insufficient, and this is the load-bearing observation: one request may be
  up to the carrier's `MAX_MESSAGE_BYTES` (16 MiB over TCP), so "keep the last 1,000" still permits
  tens of gigabytes. Both caps are needed.
- An entry larger than the byte budget is dropped entirely; otherwise a single request defeats the
  bound.
- `DreamLog::with_bounds` lets a deployment retune, and lets a test exercise eviction cheaply.
- **In-memory only.** "Recording generated scenarios" holds for the running process, not across
  restarts. Persisting it would need its own decisions — where, how long, who may read it — none of
  which are made here. The base project's tensor pad is the same shape.
- The log had **no tests at all**, which is part of why the unbounded growth went unnoticed. It now
  has three: count eviction, byte eviction, and the single-oversized-entry case.

**Status.** Accepted (`9d77e3a`).

---

## ADR-019 — The onion endpoint is volume-bounded, not authenticated

**Decision.** `POST /oag-mesh/v1/onion` gets a node-wide token bucket (16 capacity, 0.5/s refill),
the same shape as route search's. The endpoint stays deliberately unauthenticated.

**Context.** This is the one endpoint in the mesh that makes the node send traffic to an address
*chosen by the caller*: the peeled `next_hop` is forwarded to, whether it is another relay or the
final target. That is inherent to onion routing — an exit relay must be able to reach an arbitrary
target — but without a bound it also makes the relay a constrained proxy for directing
anonymous-event pushes at third parties.

**Consequences.**
- The relay's own bandwidth is bounded; an attacker can no longer spend it at will.
- **It is not an open relay and does not become one.** The payload is limited to an
  identity-stripped event plus proof-of-work, or one more encrypted layer. Arbitrary traffic cannot
  be sent through it. The target still enforces its own escalating proof-of-work.
- **It is a volume bound, not a permission system.** No authentication is added, because onion
  routing's whole point is that the relay cannot authenticate the sender.
- The limiter is checked before peeling, so the bound holds even against layers that would be
  rejected anyway — otherwise an attacker could get unlimited free peeling attempts.
- In-memory only, same posture as the crate's other limiters; a restart resets it.

**Status.** Accepted (`f47bf83`).

---

## ADR-018 — Durations are reported in milliseconds, with fixed histogram buckets

**Decision.** `search_latency` and `api_latency` are fixed-bucket histograms whose values are
milliseconds. A log line's `duration_ms` field and a `/metrics` histogram report the same unit.

**Context.** The per-request duration already existed as `duration_ms` on every REST/MCP span, but
a log line is not an aggregate: it answers "how long did this one request take", not "is this peer
getting slower?" across a million requests. Adding the aggregate half forced three small decisions.

**Consequences.**
- **Milliseconds, not seconds.** Prometheus convention is seconds, but this project already reports
  durations in milliseconds everywhere a human reads them. A reader comparing `/metrics` against the
  logs should not have to convert units in their head. The unit is stated in each `# HELP` line
  rather than encoded in the metric name, matching how the existing gauges here are named.
- **Fixed bucket boundaries** (1/5/10/25/50/100/250/500/1000/2500/5000/10000ms, plus `+Inf`). A
  histogram whose boundaries can change at runtime cannot be compared across scrapes, which defeats
  the point of having one.
- **`search_latency` is measured inside `GraphService`**, so REST, MCP, CLI, and the Human Interface
  all report the same number. `api_latency` is measured by an HTTP middleware, because the request
  as a whole — routing, auth, handler, response — is not visible to the graph service. The two
  overlap for a search request, and that is correct: one measures the request, the other the search.
- **Semantic search includes the embedding provider call.** From a caller's point of view that *is*
  the search latency; excluding it would report semantic search as faster than it is.
- **Empty results are measured too.** Excluding them would make the histogram a record of only the
  slow, successful queries — the opposite of what an operator needs.

**Status.** Accepted (`fafc44a`).

---

## ADR-017 — Outbound mesh requests are bounded; a timeout is a failed delivery, not a cancellation

**Decision.** Every outbound HTTP request in `oag-mesh` goes through a client with a 5-second
connect timeout and a 30-second total request timeout. A timed-out request is recorded as a failed
delivery by `link_quality`, exactly like a refusal.

**Context.** `reqwest::Client::new()` sets no timeout. For a client talking to an
operator-configured list of arbitrary addresses, that is not theoretical: a peer that silently
drops packets holds the request open until the operating system gives up, which can be minutes.
Two properties of this crate make that worse than a slow pull:

- `sync_with_all_peers` pulls peers sequentially, so one black-holed peer stalls the entire tick
  before any live peer is reached.
- `search_for_route` fans out through `push_route_search`, which sits behind an *inbound*
  `POST /oag-mesh/v1/route-search`. One broken peer can hang a third party's request.

**Consequences.**
- No outbound request can hold a sync tick or an inbound request open indefinitely.
- The connect timeout covers the handshake that never completes; the total timeout covers the
  connection that succeeds and then goes silent. Both are needed — neither subsumes the other.
- A timeout is *not* a cancellation: the peer may still complete the work and merge the state. That
  is safe because every merge is idempotent (`spec/10`), and it is the reason idempotence matters
  rather than being a convenience.
- A timed-out request feeds `link_quality` as a failure, which is the observation that makes the
  next fan-out spend its budget better. The two mechanisms compose.

**Status.** Accepted (`531b6f9`).

---

## ADR-016 — Link quality orders fan-out; it never excludes a peer

**Decision.** Route search asks its peers best-first by an exponential moving average of whether
each interaction delivered. A peer at score `0.0` is asked *last*, not dropped, and the ordering
function is a permutation of the input — asserted as a property test, not left as a comment.

**Context.** The fan-out budget is the one scarce resource a route search has, and every mechanism
in the mesh treated the configured peer list as an unordered set of equally-good addresses. A
search with a budget of one would spend it on whichever address was configured first, including
one that has been refusing connections for hours. That is the "pathways strengthen based on
observed utility" question from the mission's networking domain, in the one place this crate can
answer it without a routing-table redesign.

**Consequences.**
- A dead peer is demoted within a handful of rounds (alpha 0.1, the same learning rate as the base
  project's tensor pad), and a recovered peer climbs back.
- A peer that is down today may be the only path tomorrow, and nothing here makes this node forget
  an address its operator configured. That is the standing "degrade, don't drop" decision applied
  to connectivity.
- Both success and failure are recorded. A peer that did not answer is exactly the observation the
  next search needs, and it is invisible if only successes are counted.
- **It is a liveness signal, not a trust signal.** A peer that replies with garbage scores exactly
  as well as one that replies correctly. Honesty remains `spec/05`'s corroboration layer's job, and
  nothing here feeds `trust_score` or any federation decision.
- **In-memory only.** A restart re-learns quality within a few rounds. Losing it costs a little
  wasted fan-out, and persisting it would add an eighth store and change the commit-marker set
  (`spec/08`) for a value that is by nature ephemeral.

**Consequence applied to the pull loop.** `sync_with_all_peers` is sequential, so it now orders
its peers by the same ledger before pulling any of them. Without that, every black-holed peer is
waited out -- a full connect/request timeout each, since the timeout fix -- before a single live peer
is reached. Ordering only: every configured peer is still pulled.

**Status.** Accepted (`67ad91e`, `740576b`), and measured on a real loopback network rather than
only unit tested.

---

## ADR-015 — The imagination Reticulum adapter is a separate crate, and takes a seed string

**Decision.** `oag-imagination-reticulum` is its own crate in the imagination repository, not a
module of `oag-imagination`. It takes an identity *seed string* rather than an `oag` peer identity.

**Context.** Two constraints pulled in opposite directions. The spec says the imagination layer
does not depend on the `reticulum` crate; but `oag-reticulum` (which does) cannot host the adapter
either, because `oag-imagination` depends on `oag-core`/`oag-crypto`/`oag-storage` from the base
project, so `oag-reticulum` depending on `oag-imagination` would make the two *repositories*
mutually dependent. A separate crate satisfies both.

The identity question is sharper. The base project derives its Reticulum identity from the peer's
Ed25519 signing key, which is exactly right there: the key already exists, is private, and is
stable. The adapter crate has no `oag-crypto` dependency and no peer identity of its own. Deriving
from the peer's *public* id instead would be a security regression -- anyone who has ever seen the
peer's `PeerId` could reconstruct its Reticulum private identity and impersonate it on the mesh.

**Consequences.**
- The layer stays free of any `reticulum` dependency; a node running over TCP, HTTP, or IPFS never
  pays for this crate.
- The adapter takes a seed string and derives deterministically via
  `PrivateIdentity::new_from_name`. The natural seed is the owning `oag` peer's id, passed in by
  the caller, which preserves the base project's security property without a dependency.
- **The seed must be stable across restarts.** A changed seed means a changed address, and peers
  that knew the old address can no longer reach this node. This is a real operational footgun and is
  documented in the crate and the spec rather than left implicit.
- The adapter implements both `ReticulumCarrier` (Option-returning) and `Transport`
  (`io::Result`-returning), mapping `None` to a timeout `io::Error`, so `send_with` works unchanged.

**Status.** Accepted (`9f63af7`).

---

## ADR-014 — A commit marker makes a saved store set atomic to the reader

**Decision.** `MeshState::save_to_dir` writes all seven store files, hashes each one's exact bytes,
and then writes `commit.json` recording `{version, files: [{file, blake3}]}`. `load_from_dir`
verifies the marker first and refuses the set with `PersistenceError::TornSet` if any recorded file
is missing or differs.

**Context.** Each file was already atomic individually (temp + rename), but that only guarantees
that a file is not truncated -- not that a *set* of files is from the same snapshot. A crash
between the individual saves leaves some stores new and some old, every file still parses, and
nothing downstream can detect the disagreement. This is a silent-consistency failure, which is worse
than a crash: a node would act on stores that contradict each other with no signal.

**Consequences.**
- A torn set is refused rather than loaded. The node starts from whatever the previous consistent
  set was, or empty, instead of acting on contradictory state.
- The marker is written last, so its presence means the whole set is on disk; its absence after a
  crash means the previous set is intact.
- A corrupt or wrong-version marker is refused too, because it means the save was interrupted at
  the last step and consistency is unknown.
- **A directory with no marker still loads exactly as before.** This is deliberate: an existing
  deployment upgrades, its directory has seven store files and no marker, and refusing would strand
  every one of those nodes' state. Absence of a marker means "older version or first run", not
  corruption. Only a marker that contradicts the files is refused.
- BLAKE3 of the exact bytes, not a hash of the parsed value: the question is "did the bytes on disk
  change since the save", which is the only thing a crash can make ambiguous.

**What this does not do.** There is still no fsync, so a power loss can lose the rename itself;
and there is still no atomic multi-store *write* on the pull side. The marker closes the read-side
gap only.

**Status.** Accepted (`7223852`).

---

## ADR-012 — The Human Interface's search page size is fixed, not caller-controlled

**Decision.** `/ui/search` pages at a fixed 50 results. Only the offset is caller-controlled.
REST, MCP, and CLI keep a caller-chosen `limit` (clamped to `[0, 1000]`).

**Context.** Pagination was added because a `limit` with no `offset` made everything past the
first page unreachable, and the Human Interface hard-coded 50 with no way forward. The obvious
fix is to expose `limit` everywhere. That is the wrong fix for a browsing surface.

**Consequences.**
- The Human Interface cannot be made to serialize a whole matching node set into one HTML
  response, accidentally or deliberately.
- REST/MCP/CLI keep their existing caps, which are already bounded.
- Pagination there is offset-based, so a caller must fetch a page at a time; that is the point.

**Rejected alternative.** A shared, caller-controlled page size on all four surfaces. It would
be simpler and would weaken the one surface where the page size is a rendering cost rather than
a transport cost.

**Status.** Accepted (`e1e6110`).

---

## ADR-013 — A negative offset is clamped, not rejected

**Decision.** A negative `offset` is clamped to 0 in `GraphService::search`. The same value on
REST, MCP, CLI, and the Human Interface therefore behaves identically.

**Context.** The service layer is the one place all four surfaces call through. If clamping
lived at each surface, three might clamp and one might error on the same input, and the
inconsistency would be invisible until a caller hit it.

**Consequences.** A negative offset means "from the start" rather than an error. This is the
same posture already taken for a negative `limit`, which is clamped rather than rejected for the
same reason.

**Status.** Accepted (`e1e6110`).

---

## ADR-011 — Persistent project records live in the foundational repository

**Decision.** `PROJECT_STATE.md`, `RESEARCH_JOURNAL.md`, `INVENTION_REGISTER.md`,
`ENGINEERING_LOG.md`, `EXPERIMENTS.md`, and `ARCHITECTURE_DECISIONS.md` live in
`open-agent-graph` (`rust-ai`).

**Context.** The mission spans three repositories, and the directive forbids a fourth directory.
`rust-ai` is the foundational layer, so the mission-wide records belong there. Repository-specific
reasoning stays in each repo's `OPEN_QUESTIONS.md` and `spec/`.

**Consequences.**
- One authoritative mission state file rather than three divergent copies.
- Repository-specific detail is not duplicated.
- Historical entries are append-only; they are never rewritten to flatter progress.

**Status.** Accepted (2026-11-09).

---

## ADR-012 — A pagination cursor names a row, never a rank

**Decision.** Search pagination cursors carry the last row's `node_id`. The query re-resolves that
row's *current* FTS5 `rank` and selects the rows that follow `(rank, node_id)`. The rank value at
capture time is deliberately not stored.

**Context.** The first implementation stored `(rank, node_id)`. It repeated the boundary row. The
cause is not a coding error but a property of the rank function: FTS5's `rank` is bm25, whose idf
and length-normalisation terms read the corpus, so the same row scores differently before and
after an insertion. A cursor that stores a score compares a value computed under the old corpus
against values computed under the new one.

Measured, not inferred: after inserting one document, eleven seeded matches kept their exact
relative order while every one of their ranks moved (`-2.0000e-6` became `-1.9650e-6`).

**Consequences.**
- The cursor is exact under any *absolute* rank shift, which is the case that occurs in practice.
- It is still not exact under a *reordering*: if a corpus change moves a row across the boundary
  relative to its neighbours, that row repeats or skips. No pagination scheme survives that,
  because the ordering itself changed. This is documented in `spec/07-search.md` rather than
  papered over.
- A cursor whose node no longer matches the query (removed, or its text edited) cannot be
  resolved. The walk restarts from the beginning rather than erroring, because a caller cannot
  recover from a failure mid-walk and the alternative is a silently wrong page.
- `ORDER BY rank` became `ORDER BY rank, nodes.node_id` on the offset path too, so the two
  pagination contracts agree on where a page boundary falls inside a run of tied rows.

**Status.** Accepted.

---

## ADR-013 — The embedded IPFS node keeps a persistent libp2p identity

**Decision.** `EmbeddedIpfsProvider` loads its libp2p secret key from `<repo>/libp2p.key` --
32 raw ed25519 bytes, generated with `OsRng` on first run, written `0o600` on Unix -- and passes
it to `DefaultIpfsBuilder::with_keypair` instead of `new()`.

**Context.** `IpfsBuilder::new()` generates a fresh keypair on every call. The node's dial address
is `<multiaddr>/p2p/<peer-id>`, so every `oag serve` start advertised a new address, and a browser
holding the previous `/ipfs-site/{cid}` bootstrap page could not reach the node until it
reloaded. The original rationale for leaving it was that bitswap addresses content by CID rather
than by peer identity -- true of content *routing*, but the bootstrap page's entire job is to hand
the browser a dial address, and that address is the peer identity.

**Consequences.**
- A restart keeps the same dial address, which is the same posture as OAG's own peer identity: a
  process restart is not an identity change.
- A wrong-length key file is a hard error, not a silent regeneration. Losing the key changes the
  node's address -- the one thing persistence exists to keep stable -- so quietly replacing it
  would hide exactly the failure an operator needs to see. The file is left in place for the
  operator to fix.
- The key is separate from OAG's peer identity. Sharing one key across the two roles would make
  the dial address an identity assertion about the OAG peer, collapsing two distinct
  responsibilities.
- Existing deployments without the file generate one on first start, so the change is not a
  migration.

**Status.** Accepted.

---

## ADR-014 — Source independence is the union of two relations

**Decision.** `source_independence` groups evidence by the connected components of two relations
together: same control group (registrable domain, with the multi-tenant-host exception) **or** same
`content_hash`. A component's label is the key its members share, or the smallest key for a
singleton.

**Context.** The grouping was by registrable domain alone. That covers spec section 62's case --
ten sources under one organization are not ten confirmations -- but misses the mirror case: one
article syndicated to several domains is one source, not several, and it scored as independent
corroboration. `content_hash` is computed from the fetched body by the crawler, so equal hashes mean
equal bytes.

Neither relation implies the other, which is why the grouping is their union rather than either one
alone:

- Two distinct domains *can* carry identical text, so content must be able to join across domains.
- One domain *can* carry distinct text, so the domain must still join across articles -- otherwise
  the fix would silently split every multi-article organization into one source per article and
  inflate `source_independence` for exactly the case section 62 was written about.

**Consequences.**
- A mirrored article collapses to one source, labelled `content:<hash>` -- the label names the
  reason for the collapse.
- A multi-article organization stays one source, labelled by its domain, exactly as before.
- `evidence_strength` averages per component as before, so merging components changes the mean
  only where a merge actually occurred.
- `content_hash` is caller-asserted on the REST/MCP path, so a caller can assert distinct hashes for
  identical content and evade the collapse. This is the same posture OAG already takes with
  `content_hash` and `uri`: caller-asserted fields are assertions, not verified facts. Recorded in
  `spec/06` rather than implied away.

**Status.** Accepted.

---

## ADR-015 — Read-then-write transactions take `BEGIN IMMEDIATE`

**Decision.** Every transaction that reads and then writes opens with `pool.begin_with("BEGIN
IMMEDIATE")`, not `pool.begin()`. Four sites: `oag-events`' `ingest_remote_event` and
`commit_local_event`, and `oag-graph`'s two redaction paths. `oag-graph`'s `rebuild` keeps a
deferred begin because its first statement is a write, so it never upgrades a read snapshot -- and
holding the write lock for a whole rebuild would be worse.

**Context.** `pool.begin()` issues a *deferred* `BEGIN`: the write lock is taken at the first write,
not at the first read. If another connection commits between a transaction's read and its write,
SQLite cannot upgrade this connection's read snapshot and returns `SQLITE_BUSY_SNAPSHOT` (517)
immediately. `busy_timeout(5s)` does not cover it, because no amount of waiting makes a stale
snapshot valid -- the whole read-then-write has to restart.

This was the largest single error class in a 50-peer chaos run: 1,971 `database is locked` errors,
codes 5 and 517, all recovered by retry.

**Consequences.**
- Reproduced in isolation at 96 concurrent writers: deferred begin gave 35 code-5 and 32 code-517
  failures; `BEGIN IMMEDIATE` gave 96 successes and zero failures.
- End-to-end, the chaos run's `sqlite locked` count went from **1,971 to 0**, and total recoverable
  errors from 4,565 to 635 -- the remainder being the rate limiter doing its job against a full-mesh
  flood. The 429 count also fell (2,594 to 635) because transactions complete instead of retrying.
- **A workaround became redundant.** `GraphService`'s `commit_lock` existed specifically to stop
  concurrent same-peer commits hitting 517. With the cause fixed, a test that asserted those
  commits *must* race now asserts the opposite: 20 concurrent commits all succeed with contiguous
  sequences. The lock stays as defence in depth, and its doc comment says so.
- `BEGIN IMMEDIATE` serialises writers at the database rather than the application, so it also
  covers multiple processes sharing a database -- which a process-local mutex never could.

**Status.** Accepted.

**Addendum.** The `rebuild` clause above is superseded: it now opens `BEGIN IMMEDIATE` too. The
reasoning that exempted it was wrong -- a deferred transaction that writes holds the write lock
until commit either way, so taking it up front costs nothing there, and the exemption rested on
`clear_projection_tables` being the first statement, an ordering nothing tested. Every
multi-statement transaction in the project is now immediate, with no exception.
