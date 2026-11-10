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
