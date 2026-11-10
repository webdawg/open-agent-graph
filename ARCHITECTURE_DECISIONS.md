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
